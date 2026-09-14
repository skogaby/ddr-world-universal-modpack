//! Engine-facing adapter around the pure planner (design §4.8): reads a live
//! `GamePlayActor`'s Results vector into `NoteView`s each judge frame, runs
//! `planner::plan_frame`, and hands the flag block back to `foot_panel_swap`.
//!
//! Also the cabinet twin of `tools/bot_sim`'s planner-vs-judge self-check: the
//! first frame a tap note is observed judged, the game's grade (`result+0x0C`)
//! is compared with the planner's resolved plan; disagreements are counted and
//! reported in the song-end tally. A handful per song on dense Challenge
//! charts is the judge's one-accepted-note-per-frame race (a late-planned Good
//! keeps losing to better notes until the +160 Miss mark) — anything more
//! means the controller assumption broke on this build.
//!
//! **Target Score** (`BotMode::Target`): on the Results rebuild the filler
//! binds the HUMAN's ghost (`ghost_source::read_human_ghost`, cached for the
//! song so an in-place reset re-uses it), requires `ghost.len() == results
//! count`, and builds the planner state with the bytes + the armed
//! S-Marvelous floor. Any refusal falls back to Level 10 for the song with
//! ONE WARN naming the reason and a 3 s toast — the impersonation is already
//! applied at song select, long before the GhostActor exists, so the flip
//! cannot refuse ahead of time.
//!
//! Runs on the game thread inside the judge pre-callback (`Priority::Late`).
//! Panic-free by construction on the hot path (`catch_unwind` is the backstop);
//! allocation-free after the first frame of a song (the view vector's capacity
//! is retained); one `try_lock` per frame.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use super::eligibility::BotMode;
use super::ghost_source;
use super::planner::{self, NoteView, PanelFlags, SongState};
use super::skill::{self, Curve, Plan, Rng, GRADE_MISS};
use super::{ghost, GHOST_FALLBACK_LEVEL};
use crate::core::memory;
use crate::mods::s_marvelous;
use crate::services::foot_panel_swap::{BotPanelFlags, Controller, ACTOR_CUR_BEAT};
use crate::services::toast;
use crate::types::game_note::{self, result, GameNote};
use crate::{log_info, log_warn};

/// Bytes of the actor probed once per song before the first read
/// (`+0x84` side … `+0x168` cur_beat).
const ACTOR_PROBE_START: usize = 0x84;
const ACTOR_PROBE_END: usize = 0x170;
/// Sanity cap on the Results vector length (the densest World chart is ~1.5k).
const MAX_RESULTS: usize = 8192;

/// Toast hold for the no-ghost fallback notice (ms).
const FALLBACK_TOAST_HOLD_MS: u64 = 3000;

/// One song's bot state for a side.
struct SongCtx {
    st: SongState,
    rng: Rng,
    curve: Curve,
    mode: BotMode,
    seed: u64,
    views: Vec<NoteView>,
    /// Per view: the game's grade the last time we looked (0xFF unjudged).
    last_grade: Vec<u8>,
    validated: bool,
    frames: u32,
    mismatches: u32,
    /// Judged-note grade tally as the GAME assigned it (0..=3 taps, 5 Miss).
    judged: [u32; 6],
    /// Target mode: the human's ghost, cached at the first successful read
    /// (an in-place reset rebuilds the Results but not the ghost).
    ghost: Option<ghost_source::Ghost>,
    /// Target mode: the S-Marvelous floor the planner was built with.
    smarv_floor: i32,
    /// Target mode: why the song fell back to Level 10 (None = replaying).
    fallback: Option<&'static str>,
    /// Whether the fallback WARN/toast fired this song.
    fallback_announced: bool,
}

/// Song-end summary for the INFO line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SongSummary {
    pub mode: BotMode,
    pub seed: u64,
    /// Planner's own tally (planned grade of every judged note).
    pub planned: [u32; 6],
    /// The game's grades of the same notes.
    pub judged: [u32; 6],
    pub mismatches: u32,
    pub frames: u32,
    /// Target mode provenance: ghost id + length + per-class histogram.
    pub ghost: Option<(i64, usize, [u32; ghost::GRADE_CLASSES])>,
    /// Target mode: taps whose resolved grade differed from the ghost's.
    pub repro_miss: u32,
    /// Target mode: the fallback reason when the ghost was unusable.
    pub fallback: Option<&'static str>,
}

static CTX: [Mutex<Option<SongCtx>>; 2] = [Mutex::new(None), Mutex::new(None)];
static WARNED_CONTENTION: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
static WARNED_PANIC: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
static WARNED_UNREADABLE: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];

/// The skill curve a mode plays with: its level, or — for the Target replay,
/// whose magnitudes come from the ghost — the Level-10 curve (the side chain
/// it borrows, and the fallback's curve).
fn curve_for(mode: BotMode) -> Curve {
    match mode {
        BotMode::Level(l) => skill::curve(l),
        BotMode::Target => skill::curve(GHOST_FALLBACK_LEVEL),
    }
}

/// Begin a song for `side` in `mode` with a fresh RNG. The Target replay's
/// ghost binds lazily at the first frame (the Results count is needed).
pub fn start_song(side: usize, mode: BotMode, seed: u64) {
    if side >= 2 {
        return;
    }
    let curve = curve_for(mode);
    if let Ok(mut slot) = CTX[side].lock() {
        *slot = Some(SongCtx {
            st: SongState::new(0),
            rng: Rng::new(seed),
            curve,
            mode,
            seed,
            views: Vec::new(),
            last_grade: Vec::new(),
            validated: false,
            frames: 0,
            mismatches: 0,
            judged: [0; 6],
            ghost: None,
            smarv_floor: 0,
            fallback: None,
            fallback_announced: false,
        });
    }
    WARNED_CONTENTION[side].store(false, Ordering::Release);
    WARNED_PANIC[side].store(false, Ordering::Release);
    WARNED_UNREADABLE[side].store(false, Ordering::Release);
    match mode {
        BotMode::Level(level) => log_info!(
            "MultiplayerBot: filler start side={} level={} {} seed={:#x}",
            side,
            level,
            curve.describe(),
            seed
        ),
        BotMode::Target => log_info!(
            "MultiplayerBot: filler start side={} mode=target (ghost binds at the first frame) seed={:#x}",
            side,
            seed
        ),
    }
}

/// Drop the side's song state (the next `start_song` re-rolls).
pub fn reset(side: usize) {
    if side < 2 {
        if let Ok(mut slot) = CTX[side].lock() {
            *slot = None;
        }
    }
}

/// The song-end summary (None when no song is active on that side).
pub fn summary(side: usize) -> Option<SongSummary> {
    if side >= 2 {
        return None;
    }
    let guard = CTX[side].lock().ok()?;
    let ctx = guard.as_ref()?;
    Some(SongSummary {
        mode: ctx.mode,
        seed: ctx.seed,
        planned: ctx.st.tally(),
        judged: ctx.judged,
        mismatches: ctx.mismatches,
        frames: ctx.frames,
        ghost: ctx
            .ghost
            .as_ref()
            .map(|g| (g.id, g.bytes.len(), ghost::histogram(&g.bytes))),
        repro_miss: ctx.st.repro_miss(),
        fallback: ctx.fallback,
    })
}

/// The registered [`crate::services::foot_panel_swap::BotFillFn`].
pub fn fill(side: usize, actor: *mut u8, music_count: i32, out: &mut BotPanelFlags) {
    *out = BotPanelFlags::default();
    if side >= 2 || actor.is_null() {
        return;
    }
    let body = std::panic::AssertUnwindSafe(|| fill_inner(side, actor, music_count, out));
    if std::panic::catch_unwind(body).is_err() {
        *out = BotPanelFlags::default();
        if !WARNED_PANIC[side].swap(true, Ordering::AcqRel) {
            log_warn!(
                "MultiplayerBot: filler panicked on side {} -- empty flags for the rest of the song",
                side
            );
        }
    }
}

fn fill_inner(side: usize, actor: *mut u8, music_count: i32, out: &mut BotPanelFlags) {
    let mut guard = match CTX[side].try_lock() {
        Ok(g) => g,
        Err(_) => {
            if !WARNED_CONTENTION[side].swap(true, Ordering::AcqRel) {
                log_warn!(
                    "MultiplayerBot: filler lock contended on side {} -- frame skipped",
                    side
                );
            }
            return;
        }
    };
    let Some(ctx) = guard.as_mut() else {
        return;
    };

    // One-time readability probe of the actor fields we touch.
    if !ctx.validated {
        if !memory::is_readable(
            unsafe { actor.add(ACTOR_PROBE_START) },
            ACTOR_PROBE_END - ACTOR_PROBE_START,
        ) {
            if !WARNED_UNREADABLE[side].swap(true, Ordering::AcqRel) {
                log_warn!(
                    "MultiplayerBot: GamePlayActor {:p} not readable on side {} -- bot idle",
                    actor,
                    side
                );
            }
            return;
        }
        ctx.validated = true;
    }

    let cur_beat = unsafe { memory::read_i32(actor.add(ACTOR_CUR_BEAT)) };
    let (begin, end) = unsafe { game_note::actor_results_range(actor) };
    if begin.is_null() || end.is_null() || end <= begin {
        return;
    }
    let span = (end as usize).wrapping_sub(begin as usize);
    if !span.is_multiple_of(result::STRIDE) {
        return;
    }
    let count = span / result::STRIDE;
    if count == 0 || count > MAX_RESULTS {
        return;
    }
    if !memory::is_readable(begin, span) {
        return;
    }

    // Rebuild the view vector in place. A length change (song reset rebuilt
    // the Results) rebuilds from scratch; otherwise only the judged state is
    // refreshed, and grade flips feed the self-check.
    if ctx.views.len() != count {
        ctx.views.clear();
        ctx.last_grade.clear();
        ctx.st = build_song_state(ctx, side, count);
        ctx.views.reserve(count);
        ctx.last_grade.reserve(count);
        // Walk the raw vector ourselves (`for_each_result` skips null note
        // pointers, which would misalign indices with the game's Results).
        for i in 0..count {
            let entry = unsafe { begin.add(i * result::STRIDE) };
            let note_ptr =
                unsafe { memory::read_ptr(entry.add(result::OFFSET_NOTE_PTR)) } as *const GameNote;
            let (view, grade) = read_view(i, entry, note_ptr);
            ctx.views.push(view);
            ctx.last_grade.push(grade);
        }
    } else {
        for i in 0..count {
            let entry = unsafe { begin.add(i * result::STRIDE) };
            let ts = unsafe { memory::read_i32(entry.add(result::OFFSET_JUDGE_TIMESTAMP)) };
            let grade = unsafe { memory::read_u32(entry.add(result::OFFSET_GRADE)) };
            let unjudged = ts < 0 && grade == 0xFF;
            let prev = ctx.last_grade.get(i).copied().unwrap_or(0xFF);
            if let Some(v) = ctx.views.get_mut(i) {
                v.unjudged = unjudged;
            }
            if !unjudged && prev == 0xFF {
                let g8 = grade.min(0xFF) as u8;
                self_check(ctx, i, g8);
                if let Some(slot) = ctx.last_grade.get_mut(i) {
                    *slot = g8;
                }
            }
        }
    }

    let mut flags = PanelFlags::default();
    planner::plan_frame(
        &ctx.views,
        &mut ctx.st,
        &mut ctx.rng,
        &ctx.curve,
        music_count,
        cur_beat,
        &mut flags,
    );
    out.is_held = flags.is_held;
    out.was_just_pressed = flags.was_just_pressed;
    out.event_mc = flags.event_mc;
    ctx.frames = ctx.frames.wrapping_add(1);
}

/// The planner state for a (re)built Results vector of `count` entries: the
/// skill model for a level, the ghost replay for Target — or, when the ghost
/// is unusable, the Level-10 skill model with the reason recorded and
/// announced once per song (WARN + toast).
fn build_song_state(ctx: &mut SongCtx, side: usize, count: usize) -> SongState {
    if ctx.mode != BotMode::Target {
        return SongState::new(count);
    }
    if ctx.ghost.is_none() && ctx.fallback.is_none() {
        match ghost_source::read_human_ghost(side) {
            Ok(g) => ctx.ghost = Some(g),
            Err(r) => ctx.fallback = Some(r.reason()),
        }
    }
    // Alignment is checked against EVERY rebuild's count (an in-place reset
    // rebuilds the same chart, so this only ever trips on the first frame).
    if let Some(g) = &ctx.ghost {
        if g.bytes.len() != count && ctx.fallback.is_none() {
            log_warn!(
                "MultiplayerBot: target ghost id={} has {} bytes but the chart has {} notes -- indices would misalign",
                g.id,
                g.bytes.len(),
                count
            );
            ctx.fallback = Some("len mismatch");
        }
    }
    if let Some(r) = ctx.fallback {
        if !ctx.fallback_announced {
            ctx.fallback_announced = true;
            log_warn!(
                "MultiplayerBot: no usable target ghost ({}) on side {} -- playing this song at LV{}",
                r,
                side,
                GHOST_FALLBACK_LEVEL
            );
            toast::flash_with_hold(
                format!("NO TARGET GHOST - BOT LV{GHOST_FALLBACK_LEVEL}"),
                FALLBACK_TOAST_HOLD_MS,
            );
        }
    }
    match (&ctx.ghost, ctx.fallback) {
        (Some(g), None) => {
            // The S-Marv arm happens at play-scene entry, before the first
            // judge frame; read it at bind time (per-song latch, like the
            // mod's own consumers).
            ctx.smarv_floor = if s_marvelous::is_enabled() {
                s_marvelous::state::armed_window(side)
            } else {
                0
            };
            if ctx.frames == 0 {
                let h = ghost::histogram(&g.bytes);
                log_info!(
                    "MultiplayerBot: target ghost bound on side {} id={} notes={} smarv_floor={} target=[m={} p={} g={} gd={} miss={} ok={} ng={}]",
                    side,
                    g.id,
                    g.bytes.len(),
                    ctx.smarv_floor,
                    h[0],
                    h[1],
                    h[2],
                    h[3],
                    h[5],
                    h[6],
                    h[7]
                );
            }
            SongState::with_ghost(count, g.bytes.clone(), ctx.smarv_floor)
        }
        _ => SongState::new(count),
    }
}

/// Read one Results entry into a `NoteView` (+ the game's current grade).
fn read_view(i: usize, entry: *mut u8, note_ptr: *const GameNote) -> (NoteView, u8) {
    let ts = unsafe { memory::read_i32(entry.add(result::OFFSET_JUDGE_TIMESTAMP)) };
    let grade = unsafe { memory::read_u32(entry.add(result::OFFSET_GRADE)) };
    let unjudged = ts < 0 && grade == 0xFF;
    let g8 = grade.min(0xFF) as u8;
    if note_ptr.is_null()
        || !memory::is_readable(note_ptr as *const u8, std::mem::size_of::<GameNote>())
    {
        // A hole: never decided, never pressed.
        return (
            NoteView {
                idx: i,
                kind: -1,
                music_count: 0,
                beat_count: 0,
                state: [0; 8],
                length: [0; 8],
                unjudged: false,
            },
            g8,
        );
    }
    let n = unsafe { note_ptr.read_unaligned() };
    (
        NoteView {
            idx: i,
            kind: n.kind,
            music_count: n.music_count,
            beat_count: n.beat_count,
            state: n.state,
            length: n.length,
            unjudged,
        },
        g8,
    )
}

/// Compare the game's grade of a freshly judged tap with the planner's plan.
fn self_check(ctx: &mut SongCtx, i: usize, game_grade: u8) {
    let Some(v) = ctx.views.get(i) else { return };
    if v.kind != 0 {
        return; // freeze tails / holes: not the planner's
    }
    let shock = v.state[..4].iter().all(|&s| s == 1) || v.state[4..].iter().all(|&s| s == 1);
    if shock {
        return;
    }
    let bucket = match game_grade {
        0..=3 => game_grade as usize,
        5 => 5,
        _ => 4, // Boo/OK/NG on a tap — never expected
    };
    if let Some(slot) = ctx.judged.get_mut(bucket) {
        *slot += 1;
    }
    let planned = match ctx.st.plans.get(i).copied().flatten() {
        Some(Plan::Hit { d_ms }) => skill::grade_for_offset(d_ms),
        Some(Plan::Miss) => GRADE_MISS,
        None => return, // judged before the planner ever saw it (late arm)
    };
    if planned != game_grade {
        ctx.mismatches = ctx.mismatches.saturating_add(1);
    }
}

/// Whether the side is currently driven by the bot controller (for the
/// self-test's disarm bookkeeping).
pub fn is_bot_side(side: usize) -> bool {
    crate::services::foot_panel_swap::controller(side) == Controller::Bot
}
