//! Game-thread side of the deterministic audio clock (design §3/§4):
//!
//! - the `(T, QPC)` pairing detour on the input manager's per-frame tick
//!   function (`input_tick_function` — its last instruction stores the frame
//!   tick `T` that every gameplay clock reads; a post-original detour pairs
//!   that exact `T` with QPC at ~100 ns skew);
//! - [`corrected_rbx`], the call-out the `song_rate::clock_patch` stub makes
//!   right before its Q31 multiply (`rbx` = the stock `T − S − A`);
//! - the `song_reset` content-origin feed and the play-scene gating.
//!
//! Everything here runs on the game thread (the frame thread), except the
//! scene callback which also runs there. The engine side publishes through
//! seqlocks; nothing here blocks.

use std::ptr::addr_of_mut;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;

use retour::GenericDetour;

use super::engine;
use super::onset::{latency_constant_ms, Decision, Event, FrameInput, GatePolicy, Reason, Session};
use super::SeqPub;
use crate::core::signatures::SignatureStore;
use crate::core::{hooks, memory};
use crate::services::{game_audio, scene_manager, song_reset};
use crate::types::scenes::scene;
use crate::{log_info, log_warn};

/// `input_state + 0x1268` — the cached frame tick `T` (attested by the
/// `dps_timing_anchor_site` / `song_rate_clock_anchor` layouts).
const FRAME_TICK_FIELD: usize = 0x1268;
/// `GamePlayActor + 0x16C` — SOUND_OFFSET `S` (attested by
/// `song_rate_clock_anchor`'s `SUB EBX,[RCX+0x16C]`).
const ACTOR_SOUND_OFFSET: usize = 0x16C;
/// `GamePlayActor + 0x160` — the timing anchor `A` (for the onset INFO line).
const ACTOR_ANCHOR: usize = 0x160;

#[link(name = "kernel32")]
extern "system" {
    fn QueryPerformanceCounter(value: *mut i64) -> i32;
    fn QueryPerformanceFrequency(value: *mut i64) -> i32;
}

type TickFn = unsafe extern "C" fn();

static mut TICK_HOOK: Option<GenericDetour<TickFn>> = None;
static INIT: AtomicBool = AtomicBool::new(false);
static PAIRING_INSTALLED: AtomicBool = AtomicBool::new(false);
static FRAME_TICK_GLOBAL: AtomicUsize = AtomicUsize::new(0);
static FREQUENCY: AtomicU64 = AtomicU64::new(0);
/// `[T, QPC]` of the most recent input tick.
static TICK_PAIR: SeqPub<2> = SeqPub::new();
/// Content origin (wall ms of the current voice's sample 0) + generation.
static ORIGIN_WALL_MS: AtomicI32 = AtomicI32::new(0);
static ORIGIN_GEN: AtomicU64 = AtomicU64::new(1);
static ORIGIN_SEQ_SEEN: AtomicU64 = AtomicU64::new(0);
static SESSION: Mutex<Option<Session>> = Mutex::new(None);
/// Lock-free mirror of "the current scene is a play scene" (the voice
/// identity seam runs on engine threads and must not take the scene lock).
static IN_PLAY_SCENE: AtomicBool = AtomicBool::new(false);
/// Counters for the status line.
static CALLOUTS: AtomicU64 = AtomicU64::new(0);
static CORRECTED: AtomicU64 = AtomicU64::new(0);
static PAIR_MISMATCH: AtomicU64 = AtomicU64::new(0);
static LAST_ELAPSED_MILLI: AtomicI32 = AtomicI32::new(0);
static LAST_DELTA_MILLI: AtomicI32 = AtomicI32::new(0);
static ARMS: AtomicU32 = AtomicU32::new(0);
static REFUSALS: AtomicU32 = AtomicU32::new(0);
static REFUSED_WARNED: AtomicBool = AtomicBool::new(false);
static PANIC_LATCHED: AtomicBool = AtomicBool::new(false);
/// One-shot: engine observers still absent at the first play-scene entry.
static ENGINE_MISSING_WARNED: AtomicBool = AtomicBool::new(false);
/// Last arm's evidence for the diagnostics record (design §7).
static LAST_ARM: SeqPub<8> = SeqPub::new();

fn qpc() -> i64 {
    let mut value = 0;
    if unsafe { QueryPerformanceCounter(&mut value) } == 0 {
        -1
    } else {
        value
    }
}

#[must_use]
pub fn frequency() -> i64 {
    FREQUENCY.load(Ordering::Acquire) as i64
}

fn read<T: Copy>(base: usize, offset: usize) -> Option<T> {
    if base == 0 {
        return None;
    }
    let p = base.checked_add(offset)? as *const u8;
    if !memory::is_readable(p, std::mem::size_of::<T>()) {
        return None;
    }
    // SAFETY: probed readable.
    Some(unsafe { p.cast::<T>().read_unaligned() })
}

/// The frame tick `T` the game cached this frame (`*(global) + 0x1268`).
fn current_frame_tick() -> Option<u64> {
    let global = FRAME_TICK_GLOBAL.load(Ordering::Acquire);
    // The global and the state object are game statics the game itself just
    // dereferenced on this thread; plain reads (no VirtualQuery per frame).
    if global == 0 {
        return None;
    }
    // SAFETY: game-owned static + object; readable for the process lifetime.
    unsafe {
        let state = *(global as *const usize);
        if state == 0 {
            return None;
        }
        Some(*((state + FRAME_TICK_FIELD) as *const u64))
    }
}

unsafe extern "C" fn tick_hook() {
    let Some(hook) = (*addr_of_mut!(TICK_HOOK)).as_ref() else {
        return;
    };
    hook.call();
    let t = qpc();
    if let Some(tick) = current_frame_tick() {
        TICK_PAIR.write(&[tick, t as u64]);
    }
}

fn is_play_scene(scene_id: i32) -> bool {
    scene_id == scene::GAMEPLAY || scene_id == scene::ATTRACT_DEMO
}

/// Whether the current scene is one the clock arms in (the play scenes:
/// gameplay + the attract autoplay, which rides the same actor chain).
/// Lock-free (mirrored by the scene callback).
#[must_use]
pub fn in_play_scene() -> bool {
    IN_PLAY_SCENE.load(Ordering::Acquire)
}

/// Whether `bank` is the `IXACT2SoundBank*` in the game's per-song slot.
#[must_use]
pub fn is_song_bank(bank: usize) -> bool {
    bank != 0 && game_audio::sound_bank_in_slot(game_audio::SONG_BANK_SLOT) == Some(bank)
}

fn bump_origin(wall_ms: i32) {
    ORIGIN_WALL_MS.store(wall_ms, Ordering::Release);
    ORIGIN_GEN.fetch_add(1, Ordering::AcqRel);
}

/// Called by the service when the mod is live-disabled.
pub(super) fn on_disabled() {
    engine::clear_voices();
    if let Ok(mut guard) = SESSION.try_lock() {
        if let Some(session) = guard.as_mut() {
            if let Some(Event::Disarmed { generation, .. }) = session.disarm(Reason::Explicit) {
                log_info!("audio_clock: disarmed gen {} (mod disabled)", generation);
            }
        }
    }
}

/// Resolve the game-side pieces and install the pairing detour. Call once
/// after `resolve_derived`, scene_manager and song_reset init. Fail-open:
/// returns false (clock stays stock) when anything is missing.
pub fn init(signatures: &SignatureStore) -> bool {
    if INIT.swap(true, Ordering::AcqRel) {
        return PAIRING_INSTALLED.load(Ordering::Acquire);
    }
    if !super::wants_engine() {
        return false;
    }
    let mut frequency = 0;
    if unsafe { QueryPerformanceFrequency(&mut frequency) } == 0 || frequency <= 0 {
        log_warn!("audio_clock: QPC frequency unavailable -- clock stays stock");
        return false;
    }
    FREQUENCY.store(frequency as u64, Ordering::Release);
    let Some(global) = signatures.get_address("frame_tick_global") else {
        log_warn!("audio_clock: frame_tick_global unresolved -- clock stays stock");
        return false;
    };
    FRAME_TICK_GLOBAL.store(global as usize, Ordering::Release);
    let Some(entry) = signatures.get_address("input_tick_function") else {
        log_warn!("audio_clock: input_tick_function unresolved -- clock stays stock");
        return false;
    };
    let installed = unsafe {
        hooks::install_enabled(
            addr_of_mut!(TICK_HOOK),
            std::mem::transmute::<*const u8, TickFn>(entry),
            tick_hook,
        )
    };
    if let Err(error) = installed {
        log_warn!(
            "audio_clock: input-tick pairing detour failed ({}) -- clock stays stock",
            error
        );
        return false;
    }
    PAIRING_INSTALLED.store(true, Ordering::Release);
    if let Ok(mut guard) = SESSION.lock() {
        *guard = Some(Session::new(GatePolicy::new(frequency)));
    }
    if scene_manager::is_available() {
        IN_PLAY_SCENE.store(
            is_play_scene(scene_manager::current_scene()),
            Ordering::Release,
        );
        scene_manager::on_scene_change(Box::new(|_previous, scene| {
            let play = is_play_scene(scene);
            IN_PLAY_SCENE.store(play, Ordering::Release);
            if play
                && super::is_enabled()
                && !engine::installed()
                && !ENGINE_MISSING_WARNED.swap(true, Ordering::AcqRel)
            {
                log_warn!(
                    "audio_clock: entering a play scene with the engine observers NOT installed (XACT factory window missed or engine unsupported) -- the gameplay clock stays stock this session"
                );
            }
            // Every scene change ends the current voice's relevance: leaving
            // the play scenes disarms; entering one starts a natural song
            // (content origin 0) — song_reset republishes for seeks.
            engine::clear_voices();
            bump_origin(0);
            if let Ok(mut guard) = SESSION.try_lock() {
                if let Some(session) = guard.as_mut() {
                    if let Some(Event::Disarmed { generation, .. }) =
                        session.disarm(Reason::Explicit)
                    {
                        log_info!(
                            "audio_clock: disarmed gen {} (scene {} -> {}{})",
                            generation,
                            _previous,
                            scene,
                            if play { ", play scene" } else { "" }
                        );
                    }
                }
            }
        }));
    } else {
        log_warn!("audio_clock: scene manager unavailable -- arming only by voice identity");
    }
    if song_reset::is_available() {
        song_reset::on_song_reset(|_t_q| {
            // The origin is published BEFORE the subscribers run.
            let (wall_ms, seq) = song_reset::voice_origin();
            if ORIGIN_SEQ_SEEN.swap(seq, Ordering::AcqRel) != seq {
                bump_origin(wall_ms);
                log_info!(
                    "audio_clock: content origin {} ms (song_reset seq {})",
                    wall_ms,
                    seq
                );
            }
        });
    }
    log_info!(
        "audio_clock: game side ready -- (T,QPC) pairing detour on input_tick_function @ {:p}, frame_tick_global @ {:p}, QPC {} Hz",
        entry,
        global,
        frequency
    );
    true
}

#[must_use]
pub fn pairing_installed() -> bool {
    PAIRING_INSTALLED.load(Ordering::Acquire)
}

/// The current content origin (wall ms of the song voice's sample 0).
#[must_use]
pub fn current_origin_ms() -> i32 {
    ORIGIN_WALL_MS.load(Ordering::Acquire)
}

/// Whether the song clock is ACTIVE (the corrected count is what the game
/// runs on right now). Consumers that align other audio to the DAC clock
/// must only do so while this holds — otherwise the game is on the stock
/// clock and DAC-aligned audio would be misaligned against it.
#[must_use]
pub fn session_active() -> bool {
    SESSION
        .try_lock()
        .ok()
        .and_then(|guard| guard.as_ref().map(Session::is_active))
        .unwrap_or(false)
}

/// Snapshot of the last arm: `[gen, F0, W, P, Wc, t_k, delta_milli, C_milli]`.
#[must_use]
pub fn last_arm() -> [u64; 8] {
    LAST_ARM.read()
}

/// Counters: (callouts, corrected frames, pair mismatches, arms, refusals).
#[must_use]
pub fn counters() -> (u64, u64, u64, u32, u32) {
    (
        CALLOUTS.load(Ordering::Relaxed),
        CORRECTED.load(Ordering::Relaxed),
        PAIR_MISMATCH.load(Ordering::Relaxed),
        ARMS.load(Ordering::Relaxed),
        REFUSALS.load(Ordering::Relaxed),
    )
}

/// The clock-patch stub's call-out (Win64 `extern "C"`: `rcx` = the
/// GamePlayActor, `edx` = the stock `rbx` = `T − S − A`). Returns the `rbx`
/// the stub should continue with: unchanged unless the session is ACTIVE.
/// Runs once per GamePlayActor per frame on the game thread — no locks
/// beyond an uncontended `try_lock`, no allocation, no logging except on
/// state transitions.
pub extern "C" fn corrected_rbx(actor: *mut u8, rbx: i32) -> i32 {
    if !super::is_available() || PANIC_LATCHED.load(Ordering::Relaxed) {
        return rbx;
    }
    match std::panic::catch_unwind(|| callout_inner(actor as usize, rbx)) {
        Ok(value) => value,
        Err(_) => {
            PANIC_LATCHED.store(true, Ordering::Release);
            rbx
        }
    }
}

fn callout_inner(actor: usize, rbx: i32) -> i32 {
    CALLOUTS.fetch_add(1, Ordering::Relaxed);
    if actor == 0 {
        return rbx;
    }
    // The pair must describe THIS frame's T (the stub computed rbx from it).
    let Some(tick) = current_frame_tick() else {
        return rbx;
    };
    let pair = TICK_PAIR.read();
    if pair[0] != tick || pair[1] == 0 {
        PAIR_MISMATCH.fetch_add(1, Ordering::Relaxed);
        return rbx;
    }
    let t_frame = pair[1] as i64;
    // SAFETY: the game dereferenced actor+0x160/+0x16C a few instructions
    // before the patch site on this very thread.
    let sound_offset = unsafe { *((actor + ACTOR_SOUND_OFFSET) as *const i32) };
    let stock_elapsed = rbx.wrapping_add(sound_offset);
    let onset = engine::song_onset();
    let published = engine::line();
    let cfg = super::config();
    let (line, c_ms) = match (&published, onset) {
        (Some((line, epoch)), Some(onset)) if *epoch == onset.epoch => (
            Some(line),
            latency_constant_ms(line, onset.hz, f64::from(cfg.latency_bias_ms)),
        ),
        _ => (None, None),
    };
    let Ok(mut guard) = SESSION.try_lock() else {
        return rbx;
    };
    let Some(session) = guard.as_mut() else {
        return rbx;
    };
    let (decision, event) = session.frame(FrameInput {
        onset,
        line,
        t_frame,
        stock_elapsed_ms: stock_elapsed,
        sound_offset_ms: sound_offset,
        c_ms,
        origin_ms: ORIGIN_WALL_MS.load(Ordering::Acquire),
        origin_generation: ORIGIN_GEN.load(Ordering::Acquire),
    });
    drop(guard);
    if let Some(event) = event {
        report(event, actor, onset, line.copied(), stock_elapsed);
    }
    match decision {
        Decision::Passthrough => rbx,
        Decision::Corrected { rbx, elapsed_ms } => {
            CORRECTED.fetch_add(1, Ordering::Relaxed);
            LAST_ELAPSED_MILLI.store((elapsed_ms * 1000.0) as i32, Ordering::Relaxed);
            LAST_DELTA_MILLI.store(
                ((elapsed_ms - f64::from(stock_elapsed)) * 1000.0) as i32,
                Ordering::Relaxed,
            );
            rbx
        }
    }
}

fn report(
    event: Event,
    actor: usize,
    onset: Option<super::onset::Onset>,
    line: Option<super::fit::Line>,
    stock_elapsed: i32,
) {
    match event {
        Event::Armed {
            generation,
            delta_ms,
            elapsed_ms,
            offset_ms,
            c_ms,
            waited_frames,
        } => {
            ARMS.fetch_add(1, Ordering::Relaxed);
            let anchor = read::<u64>(actor, ACTOR_ANCHOR).unwrap_or(0);
            let (f0, w, p, wc, t_k, lead_margin) = onset
                .map(|o| (o.f0, o.w_k, o.p_k, o.wc_k, o.t_k, o.lead_margin_ms()))
                .unwrap_or((0, 0, 0, 0, 0, 0.0));
            let (n, sd, slope) = line
                .map(|l| {
                    let hz = onset.map_or(0.0, |o| f64::from(o.hz));
                    let sd_ms = if hz > 0.0 {
                        l.resid_sd * 1000.0 / hz
                    } else {
                        0.0
                    };
                    let slope_fs = l.slope * frequency() as f64;
                    (l.n, sd_ms, slope_fs)
                })
                .unwrap_or((0, 0.0, 0.0));
            LAST_ARM.write(&[
                generation,
                f0 as u64,
                w as u64,
                p as u64,
                wc as u64,
                t_k as u64,
                (delta_ms * 1000.0) as i64 as u64,
                (c_ms * 1000.0) as i64 as u64,
            ]);
            log_info!(
                "audio_clock: armed gen={} F0={} (W={} P={} Wc={} lead+margin={:.2} ms) delta_vs_stock={:+.2} ms C={:.2} ms offset={} ms E={:.2} ms stock={} ms anchor={} waited={} frame(s) fit(n={}, sd={:.3} ms, slope={:.2} f/s)",
                generation,
                f0,
                w,
                p,
                wc,
                lead_margin,
                delta_ms,
                c_ms,
                offset_ms,
                elapsed_ms,
                stock_elapsed,
                anchor,
                waited_frames,
                n,
                sd,
                slope
            );
            let flags = engine::take_onset_flags();
            if flags & 4 != 0 {
                log_warn!("audio_clock: F0 is not within one pass of the newest cursor sample's W (F0={} W={}) -- unexpected pass structure; produce-time F0 used", f0, w);
            }
            crate::services::audio_sync_diag::record_onset(
                generation,
                onset.map_or(0, |o| o.hz),
                f0,
                w,
                p,
                wc,
                t_k,
                delta_ms,
                c_ms,
                offset_ms,
                n,
                sd,
            );
        }
        Event::Refused {
            generation,
            last_delta_ms,
        } => {
            REFUSALS.fetch_add(1, Ordering::Relaxed);
            let _ = engine::take_onset_flags();
            if !REFUSED_WARNED.swap(true, Ordering::AcqRel) {
                log_warn!(
                    "audio_clock: REFUSED gen {} -- |E - (T - A)| stayed {:+.1} ms (> sanity window) -- stock clock for this voice (further refusals logged at INFO)",
                    generation,
                    last_delta_ms
                );
            } else {
                log_info!(
                    "audio_clock: refused gen {} (delta {:+.1} ms) -- stock clock for this voice",
                    generation,
                    last_delta_ms
                );
            }
        }
        Event::Disarmed { generation, reason } => {
            log_info!("audio_clock: disarmed gen {} ({:?})", generation, reason);
        }
    }
}

/// One-line status for the mod's INFO output.
pub fn log_status() {
    let (callouts, corrected, mismatches, arms, refusals) = counters();
    log_info!(
        "audio_clock: game side -- pairing detour {}; callouts={} corrected={} pair_mismatch={} arms={} refusals={} last E={:.3} ms delta={:+.3} ms",
        if pairing_installed() { "installed" } else { "NOT installed" },
        callouts,
        corrected,
        mismatches,
        arms,
        refusals,
        LAST_ELAPSED_MILLI.load(Ordering::Relaxed) as f64 / 1000.0,
        LAST_DELTA_MILLI.load(Ordering::Relaxed) as f64 / 1000.0
    );
}
