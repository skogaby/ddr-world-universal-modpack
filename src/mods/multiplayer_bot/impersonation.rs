//! Windowed impersonation (design §4.5): make the non-entered side a genuine
//! second player for ONE song.
//!
//! At the song-select → stage edge (0-idx 25 → 26/27/28), when the
//! `eligibility` gate passes, the bot side's `PlayerWork` is dressed as an
//! entered player — chart identity mirrored from the human (`+0x50/+0x54/
//! +0x5C` and the stage record's `+0x04/+0x08`), the human's lane
//! `ddr::player::Option` copied with the gauge forced NORMAL, the name plate
//! `BOT LV<n>`, then `PlayerWork+0x4 = 1` and `GameWork+0x0 = 1`. The game
//! does the rest natively: the GAMEPLAY loader copies `+0x4` into the
//! `DancePlaySequence` ctor struct and creates a `GamePlayActor` per entered
//! side; every play-window reader of `GameWork+0x0` is a display selector
//! (versus HUD, both results panes, SE pan, save staging). The first scene
//! change OUT of the play window {26..=30} restores the snapshot (entered
//! byte, name, versus word, and the cosmetic SE-pan byte of D22), disarms
//! the controller and drops the bot side's score taint. Zero detours; every
//! address comes from `stage_records` (+ `game_audio` for the pan byte).
//!
//! Safety discipline: every pointer is `memory::is_readable`-probed before a
//! read or write; `apply` keeps a copy of every byte it wrote so a failure
//! after the first write (the controller refusing to arm) undoes all of it;
//! the state lock is never held across a call into another service (the
//! scene hook re-enters synchronously on quick restart's `finish`); nothing
//! here runs per frame.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use super::eligibility::{self, Inputs, Refusal};
use super::session::{self, Edge, NAME_LEN};
use super::{filler, self_test, skill};
use crate::core::memory;
use crate::services::foot_panel_swap::{self, Controller};
use crate::services::{game_audio, score_guard, stage_records, widget_renderer};
use crate::types::scenes::scene;
use crate::{log_info, log_warn};

// The pure module's scene literals must be the game's.
const _: () = assert!(session::SONG_SELECT == scene::SONG_SELECT);
const _: () = assert!(session::GAMEPLAY == scene::GAMEPLAY);
const _: () = assert!(
    session::PLAY_WINDOW[0] == scene::SONG_TO_STAGE_INTERSTITIAL
        && session::PLAY_WINDOW[1] == scene::STAGE_INDICATOR
        && session::PLAY_WINDOW[2] == scene::GAMEPLAY
        && session::PLAY_WINDOW[3] == scene::STAGE_RESULT
        && session::PLAY_WINDOW[4] == scene::RESULTS_DETAIL
);

// ── PlayerWork header (build-invariant, design §2.3 / §5.1) ──────────────
const PW_ENTERED: usize = 0x4;
const PW_NAME: usize = 0xC;
const PW_STYLE: usize = 0x50;
const PW_MCODE: usize = 0x54;
const PW_DIFFICULTY: usize = 0x5C;
/// Bytes probed on each PlayerWork (header through `+0x5C`).
const PW_PROBE_LEN: usize = 0x60;

// ── GameWork ────────────────────────────────────────────────────────────
const GW_VERSUS: usize = 0x0;
const GW_STYLE: usize = 0x4;
/// Minimum GameWork probe for the gate reads (event mode lives at `+0xD0`).
const GW_GATE_PROBE_LEN: usize = 0xD4;
/// GameWork probe for the flip write (the versus word only).
const GW_FLIP_PROBE_LEN: usize = 0x8;

// ── ddr::player::Option (A.6) ───────────────────────────────────────────
/// First copied byte — the vtable at `+0x00` is never touched.
const OPT_COPY_START: usize = 0x08;
/// `+0x08..=0x6C` (CutJump is the last 4-byte field).
const OPT_COPY_LEN: usize = 0x68;
const OPT_GAUGE: usize = 0x18;
const OPT_PROBE_LEN: usize = 0x70;

// ── Per-stage record header ─────────────────────────────────────────────
const REC_MCODE: usize = 0x00;
const REC_DIFFICULTY: usize = 0x04;
const REC_STYLE: usize = 0x08;
const REC_PROBE_LEN: usize = 0x0C;

/// Watchdog: WARN when no scene change follows the flip within this long.
const WATCHDOG_TIMEOUT: Duration = Duration::from_secs(20);

/// The values the restore writes back (design §4.5 step 2, plus the D22
/// SE-pan byte — `None` when the audio manager was unavailable at the flip,
/// in which case the pan is left alone both ways).
#[derive(Debug, Clone, Copy)]
struct Snapshot {
    entered_byte: u8,
    name: [u8; NAME_LEN],
    versus_word: i32,
    pan_byte: Option<u8>,
}

#[derive(Debug, Clone, Copy)]
struct Active {
    bot: usize,
    human: usize,
    level: u8,
    snap: Snapshot,
}

enum State {
    Idle,
    Active(Active),
}

static STATE: Mutex<State> = Mutex::new(State::Idle);
/// Lock-free mirror of the active bot side (−1 = none) for the extra-stage
/// guard's game-thread read.
static ACTIVE_BOT: AtomicI32 = AtomicI32::new(-1);
/// Scene changes observed while Active (the watchdog's "the game moved on"
/// signal).
static SCENE_CHANGES: AtomicU32 = AtomicU32::new(0);
static WATCHDOG_GEN: AtomicUsize = AtomicUsize::new(0);
static WARNED_UNAVAILABLE: AtomicBool = AtomicBool::new(false);

/// The state lock. A poisoned mutex holds plain data, so recovery is safe.
fn lock_state() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|poison| poison.into_inner())
}

fn current() -> Option<Active> {
    match &*lock_state() {
        State::Idle => None,
        State::Active(a) => Some(*a),
    }
}

/// The side currently impersonated as the bot, if any.
pub fn active_bot_side() -> Option<usize> {
    match ACTIVE_BOT.load(Ordering::Acquire) {
        s @ 0..=1 => Some(s as usize),
        _ => None,
    }
}

/// Scene-change hook (called FIRST from the mod's single scene callback).
pub fn on_scene_change(prev: i32, next: i32) {
    let active = current();
    if active.is_some() {
        SCENE_CHANGES.fetch_add(1, Ordering::AcqRel);
    }
    match session::classify(prev, next, active.is_some()) {
        Edge::Flip => try_flip(),
        Edge::Reseed => {
            if let Some(a) = active {
                reseed(&a, "GAMEPLAY entry");
            }
        }
        Edge::Restore => restore(),
        Edge::None => {}
    }
}

/// `song_reset` subscriber: an in-place restart rebuilds the Results, so the
/// bot re-rolls with a fresh seed.
pub fn on_song_reset(_t_ms: i32) {
    if let Some(a) = current() {
        reseed(&a, "song reset");
    }
}

/// Mod disable: undo an active impersonation immediately.
pub fn shutdown() {
    restore();
}

// ── Flip ────────────────────────────────────────────────────────────────

fn try_flip() {
    let inputs = gather_inputs();
    match eligibility::evaluate(&inputs) {
        Ok(plan) => apply(plan),
        Err(Refusal::Unavailable(what)) => {
            if !WARNED_UNAVAILABLE.swap(true, Ordering::AcqRel) {
                log_warn!(
                    "MultiplayerBot: gate input '{}' unavailable -- bot never engages this boot",
                    what
                );
            }
        }
        Err(refusal) => {
            if wants_bot(&inputs) {
                log_info!("MultiplayerBot: not engaging this song -- {:?}", refusal);
            }
        }
    }
}

/// Some ENTERED side has the option ON (never true for `OptionOff`).
fn wants_bot(i: &Inputs) -> bool {
    (0..2).any(|s| i.entered[s] == Some(true) && i.option_on[s])
}

fn gather_inputs() -> Inputs {
    let course_off = stage_records::course_field_offset();
    let probe_len = (course_off + 8).max(GW_GATE_PROBE_LEN);
    let gw = stage_records::game_work().filter(|gw| memory::is_readable(*gw, probe_len));
    Inputs {
        entered: [
            stage_records::side_entered(0),
            stage_records::side_entered(1),
        ],
        style: gw.map(|g| unsafe { memory::read_i32(g.add(GW_STYLE)) }),
        course_word: gw
            .filter(|_| course_off != 0)
            .map(|g| unsafe { memory::read_u64(g.add(course_off)) }),
        event_mode: stage_records::event_mode(),
        versus: gw.map(|g| unsafe { memory::read_i32(g.add(GW_VERSUS)) }),
        option_on: [super::option_on(0), super::option_on(1)],
        level: [super::level(0) as i32, super::level(1) as i32],
    }
}

/// Every game pointer the flip touches, all probed.
struct Ptrs {
    pw_h: *mut u8,
    pw_b: *mut u8,
    rec_h: *mut u8,
    rec_b: *mut u8,
    opt_h: *mut u8,
    opt_b: *mut u8,
    gw: *mut u8,
}

/// Every byte `apply` writes, so a late failure can put all of it back.
struct Written {
    snap: Snapshot,
    pw_style: i32,
    pw_mcode: i32,
    pw_difficulty: i32,
    rec_difficulty: i32,
    rec_style: i32,
    option: [u8; OPT_COPY_LEN],
}

fn resolve_ptrs(plan: &eligibility::Plan) -> Option<Ptrs> {
    let refuse = |what: &str| {
        log_warn!(
            "MultiplayerBot: {} unavailable at the flip -- stock 1P song",
            what
        );
        None
    };
    let Some(pw_h) = stage_records::player_work(plan.human) else {
        return refuse("human PlayerWork");
    };
    let Some(pw_b) = stage_records::player_work(plan.bot) else {
        return refuse("bot PlayerWork");
    };
    let Some(stage) = stage_records::stage_counter() else {
        return refuse("stage counter");
    };
    let Ok(stage) = usize::try_from(stage) else {
        return refuse("stage counter (negative)");
    };
    if stage >= stage_records::MAX_STAGE_RECORDS {
        return refuse("stage counter (out of range)");
    }
    let Some(rec_h) = stage_records::stage_record(plan.human, stage) else {
        return refuse("human stage record");
    };
    let Some(rec_b) = stage_records::stage_record(plan.bot, stage) else {
        return refuse("bot stage record");
    };
    let Some(opt_off) = stage_records::player_option_offset() else {
        return refuse("player Option offset");
    };
    let Some(gw) = stage_records::game_work() else {
        return refuse("GameWork");
    };
    let opt_h = unsafe { pw_h.add(opt_off) };
    let opt_b = unsafe { pw_b.add(opt_off) };
    let probes: [(*const u8, usize, &str); 7] = [
        (pw_h, PW_PROBE_LEN, "human PlayerWork"),
        (pw_b, PW_PROBE_LEN, "bot PlayerWork"),
        (rec_h, REC_PROBE_LEN, "human stage record"),
        (rec_b, REC_PROBE_LEN, "bot stage record"),
        (opt_h, OPT_PROBE_LEN, "human Option"),
        (opt_b, OPT_PROBE_LEN, "bot Option"),
        (gw, GW_FLIP_PROBE_LEN, "GameWork"),
    ];
    for (p, len, what) in probes {
        if !memory::is_readable(p, len) {
            log_warn!(
                "MultiplayerBot: {} at {:p} not readable at the flip -- stock 1P song",
                what,
                p
            );
            return None;
        }
    }
    Some(Ptrs {
        pw_h,
        pw_b,
        rec_h,
        rec_b,
        opt_h,
        opt_b,
        gw,
    })
}

/// Design §4.5 steps 1–8.
fn apply(plan: eligibility::Plan) {
    let (human, bot, level) = (plan.human, plan.bot, plan.level);
    // 1. Pointers + probes.
    let Some(p) = resolve_ptrs(&plan) else {
        return;
    };
    // 2. The commit must have prepared BOTH records for this song.
    let (mcode_h, mcode_b) = unsafe {
        (
            memory::read_i32(p.rec_h.add(REC_MCODE)),
            memory::read_i32(p.rec_b.add(REC_MCODE)),
        )
    };
    if mcode_h < 0 || mcode_h != mcode_b {
        log_warn!(
            "MultiplayerBot: stage record mismatch (human mcode {}, bot mcode {}) -- commit did not prepare the bot record, stock 1P song",
            mcode_h,
            mcode_b
        );
        return;
    }
    // 3. Snapshot everything we are about to write.
    let written = unsafe {
        let mut name = [0u8; NAME_LEN];
        std::ptr::copy_nonoverlapping(p.pw_b.add(PW_NAME), name.as_mut_ptr(), NAME_LEN);
        let mut option = [0u8; OPT_COPY_LEN];
        std::ptr::copy_nonoverlapping(
            p.opt_b.add(OPT_COPY_START),
            option.as_mut_ptr(),
            OPT_COPY_LEN,
        );
        Written {
            snap: Snapshot {
                entered_byte: memory::read_u8(p.pw_b.add(PW_ENTERED)),
                name,
                versus_word: memory::read_i32(p.gw.add(GW_VERSUS)),
                pan_byte: game_audio::versus_pan(),
            },
            pw_style: memory::read_i32(p.pw_b.add(PW_STYLE)),
            pw_mcode: memory::read_i32(p.pw_b.add(PW_MCODE)),
            pw_difficulty: memory::read_i32(p.pw_b.add(PW_DIFFICULTY)),
            rec_difficulty: memory::read_i32(p.rec_b.add(REC_DIFFICULTY)),
            rec_style: memory::read_i32(p.rec_b.add(REC_STYLE)),
            option,
        }
    };
    let (song_mcode, song_difficulty) = unsafe {
        (
            memory::read_i32(p.pw_h.add(PW_MCODE)),
            memory::read_i32(p.pw_h.add(PW_DIFFICULTY)),
        )
    };
    unsafe {
        // 4. Mirror the chart identity.
        memory::write_i32(p.pw_b.add(PW_STYLE), memory::read_i32(p.pw_h.add(PW_STYLE)));
        memory::write_i32(p.pw_b.add(PW_MCODE), song_mcode);
        memory::write_i32(p.pw_b.add(PW_DIFFICULTY), song_difficulty);
        memory::write_i32(
            p.rec_b.add(REC_DIFFICULTY),
            memory::read_i32(p.rec_h.add(REC_DIFFICULTY)),
        );
        memory::write_i32(
            p.rec_b.add(REC_STYLE),
            memory::read_i32(p.rec_h.add(REC_STYLE)),
        );
        // 5. The human's lane options, gauge forced NORMAL. Never the vtable.
        std::ptr::copy_nonoverlapping(
            p.opt_h.add(OPT_COPY_START),
            p.opt_b.add(OPT_COPY_START),
            OPT_COPY_LEN,
        );
        memory::write_i32(p.opt_b.add(OPT_GAUGE), 0);
        // 6. Name plate.
        let name = session::format_bot_name(level);
        std::ptr::copy_nonoverlapping(name.as_ptr(), p.pw_b.add(PW_NAME), NAME_LEN);
        // 7. The two load-bearing words.
        memory::write_u8(p.pw_b.add(PW_ENTERED), 1);
        memory::write_i32(p.gw.add(GW_VERSUS), 1);
    }
    // 7b. Cosmetic (D22): pan SEs by side like a real versus session. Stock
    // writes this byte only at scene-chain boundaries the flip never
    // crosses; unavailable ⇒ stock pan, one INFO, never a refusal.
    if written.snap.pan_byte.is_some() && !game_audio::set_versus_pan(1) {
        log_info!("MultiplayerBot: SE pan byte unavailable -- SEs keep the stock pan");
    }
    // 8. Controller + taint.
    let seed = skill::seed(self_test::qpc(), song_mcode, song_difficulty, level);
    filler::start_song(bot, level, seed);
    if !foot_panel_swap::arm_bot(bot, filler::fill) {
        filler::reset(bot);
        undo(&p, &written);
        log_warn!(
            "MultiplayerBot: bot controller refused to arm on side {} -- flip undone, stock 1P song",
            bot
        );
        return;
    }
    score_guard::set_autoplay_taint(bot, true);

    *lock_state() = State::Active(Active {
        bot,
        human,
        level,
        snap: written.snap,
    });
    ACTIVE_BOT.store(bot as i32, Ordering::Release);
    SCENE_CHANGES.store(0, Ordering::Release);
    start_watchdog();

    let c = skill::curve(level);
    log_info!(
        "MultiplayerBot: side {} impersonated as \"BOT LV{}\" for P{}'s song mcode={} diff={} ({} seed={:#x})",
        bot,
        level,
        human + 1,
        song_mcode,
        song_difficulty,
        c.describe(),
        seed
    );
}

/// Put back every byte `apply` wrote (failure path only — the normal
/// restore writes back the three snapshot items alone, design §4.5).
fn undo(p: &Ptrs, w: &Written) {
    if let Some(pan) = w.snap.pan_byte {
        game_audio::set_versus_pan(pan);
    }
    unsafe {
        memory::write_i32(p.gw.add(GW_VERSUS), w.snap.versus_word);
        memory::write_u8(p.pw_b.add(PW_ENTERED), w.snap.entered_byte);
        std::ptr::copy_nonoverlapping(w.snap.name.as_ptr(), p.pw_b.add(PW_NAME), NAME_LEN);
        std::ptr::copy_nonoverlapping(w.option.as_ptr(), p.opt_b.add(OPT_COPY_START), OPT_COPY_LEN);
        memory::write_i32(p.rec_b.add(REC_STYLE), w.rec_style);
        memory::write_i32(p.rec_b.add(REC_DIFFICULTY), w.rec_difficulty);
        memory::write_i32(p.pw_b.add(PW_DIFFICULTY), w.pw_difficulty);
        memory::write_i32(p.pw_b.add(PW_MCODE), w.pw_mcode);
        memory::write_i32(p.pw_b.add(PW_STYLE), w.pw_style);
    }
}

// ── Restore / reseed ────────────────────────────────────────────────────

/// Undo the flip. Idempotent (a no-op when Idle).
fn restore() {
    let prev = std::mem::replace(&mut *lock_state(), State::Idle);
    ACTIVE_BOT.store(-1, Ordering::Release);
    WATCHDOG_GEN.fetch_add(1, Ordering::AcqRel); // cancels a pending watchdog
    let State::Active(a) = prev else {
        return;
    };

    match stage_records::player_work(a.bot) {
        Some(pw_b) if memory::is_readable(pw_b, PW_PROBE_LEN) => unsafe {
            memory::write_u8(pw_b.add(PW_ENTERED), a.snap.entered_byte);
            std::ptr::copy_nonoverlapping(a.snap.name.as_ptr(), pw_b.add(PW_NAME), NAME_LEN);
        },
        _ => log_warn!(
            "MultiplayerBot: bot PlayerWork unreadable at restore -- entered byte / name NOT restored on side {}",
            a.bot
        ),
    }
    match stage_records::game_work() {
        Some(gw) if memory::is_readable(gw, GW_FLIP_PROBE_LEN) => unsafe {
            memory::write_i32(gw.add(GW_VERSUS), a.snap.versus_word);
        },
        _ => {
            log_warn!("MultiplayerBot: GameWork unreadable at restore -- versus word NOT restored")
        }
    }
    if let Some(pan) = a.snap.pan_byte {
        game_audio::set_versus_pan(pan);
    }

    foot_panel_swap::disarm_bot(a.bot);
    // The taint mirrors autoplay's own state once the bot is gone: a cached
    // `autoplay = ON` on that side keeps its taint, anything else clears.
    score_guard::set_autoplay_taint(
        a.bot,
        foot_panel_swap::controller(a.bot) == Controller::Perfect,
    );

    match filler::summary(a.bot) {
        Some(s) => log_info!(
            "MultiplayerBot: side {} restored (BOT LV{} seed={:#x}) planned marv={} perf={} great={} good={} miss={} | judged marv={} perf={} great={} good={} miss={} other={} | mismatch={} frames={}",
            a.bot,
            s.level,
            s.seed,
            s.planned[0],
            s.planned[1],
            s.planned[2],
            s.planned[3],
            s.planned[5],
            s.judged[0],
            s.judged[1],
            s.judged[2],
            s.judged[3],
            s.judged[5],
            s.judged[4],
            s.mismatches,
            s.frames
        ),
        None => log_info!(
            "MultiplayerBot: side {} restored (BOT LV{}) -- no song played",
            a.bot,
            a.level
        ),
    }
    filler::reset(a.bot);
}

/// Fresh seed for the same song (GAMEPLAY entry / quick restart / song reset).
fn reseed(a: &Active, reason: &str) {
    let (mcode, difficulty) = match stage_records::player_work(a.human) {
        Some(pw_h) if memory::is_readable(pw_h, PW_PROBE_LEN) => unsafe {
            (
                memory::read_i32(pw_h.add(PW_MCODE)),
                memory::read_i32(pw_h.add(PW_DIFFICULTY)),
            )
        },
        _ => (0, 0),
    };
    let seed = skill::seed(self_test::qpc(), mcode, difficulty, a.level);
    filler::start_song(a.bot, a.level, seed);
    log_info!(
        "MultiplayerBot: side {} re-rolled on {} (seed={:#x})",
        a.bot,
        reason,
        seed
    );
}

// ── Watchdog (diagnostic only) ──────────────────────────────────────────

fn start_watchdog() {
    if !widget_renderer::frame_dispatch_available() {
        return;
    }
    let generation = WATCHDOG_GEN.fetch_add(1, Ordering::AcqRel) + 1;
    watchdog_tick(generation, Instant::now());
}

fn watchdog_tick(generation: usize, started: Instant) {
    widget_renderer::run_on_render_thread(move || {
        if WATCHDOG_GEN.load(Ordering::Acquire) != generation || active_bot_side().is_none() {
            return;
        }
        if SCENE_CHANGES.load(Ordering::Acquire) > 0 {
            return; // the game moved on — nothing to report
        }
        if started.elapsed() > WATCHDOG_TIMEOUT {
            log_warn!(
                "MultiplayerBot[watchdog]: no scene change {} s after the flip (scene {}) -- bot session may be stuck in the loader",
                WATCHDOG_TIMEOUT.as_secs(),
                crate::services::scene_manager::current_scene()
            );
            return;
        }
        watchdog_tick(generation, started);
    });
}
