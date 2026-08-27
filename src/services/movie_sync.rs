//! Background-movie sync engine — keeps the DirectShow background movie
//! aligned with the song.
//!
//! ## Mechanism (Step 2: always-on position sync)
//!
//! The engine captures the live `DShowPlayer` from `movie_policy`'s
//! BuildGraph detour on every REAL successful graph build
//! (`CallOutcome::Passthrough`, hr == 0) and drops the capture on every
//! scene change. Song-timeline jumps arrive as `song_reset::on_song_reset`
//! notifications (quick restart, training loops/scrubs/restart-from-A,
//! the SONG-START silent start adjust) plus a capture-time check for a
//! LATE graph build over an already-anchored run. Every event queues a
//! **pending seek** drained by a per-frame driver
//! (`input_manager::on_frame`, game thread) the moment the player is
//! RUNNING — the queued value is only a trigger: the drain classifies
//! against the LIVE music count via the pure [`drain_action`] rule
//! (uniform `is_advancing` stability gate filters reset-transaction
//! transients), then EVERYTHING executes as a **park** — the single sync
//! mechanism: seek the video to a destination, measure this machine's
//! pipeline-restart latency from the frame-deposit signal, PAUSE on the
//! destination frame's arrival, and RUN (pre-issued by the measured
//! startup latency) when the live count crosses the destination.
//! Approach events (loop wrap, restarts, SONG START) park at their
//! trigger; jumps (scrubs — audio already running) park at `live + Δ`
//! where Δ = 2× the measured per-direction restart estimate
//! ([`jump_delta`]) — Δ sizes only the brief post-scrub freeze, never
//! the sync (the crossing governs), and self-neutralizes to a plain
//! seek on instant-seek machines. No hardcoded sync offsets anywhere:
//! every quantity is runtime-measured per machine. The seek goes through the game's own
//! null-guarded absolute seek (player vtbl +0x58) with the target mapped
//! per the player's loop flag: modulo duration when looping; clamped to
//! just short of the end when not (seeking AT the end fires the stock
//! EC_COMPLETE stop — unrecoverable for the drain, live-observed as a
//! frozen movie).
//!
//! ## Rate sync (SYNC BACKGROUND VIDEO — platform-uniform)
//!
//! A committed non-identity SONG SPEED with the entered side's SYNC
//! BACKGROUND VIDEO latched ON skips the song-rate movie suppression, so
//! the graph builds normally and lands here; the capture then consumes
//! `song_rate::runtime::movie_rate_directive()` and installs the
//! **scaled reference-clock proxy** ([`rate_clock`]): the graph's own
//! sync source wrapped so `GetTime` runs at `source/output × real time`,
//! made the graph clock via `IMediaFilter::SetSyncSource` — in place
//! when the FGM allows a paused-graph swap (Wine), escalating to a brief
//! stop → swap → re-pause on `VFW_E_NOT_STOPPED` (Windows quartz's hard
//! stopped-state rule; tests #4/#5). Filters pace their streaming
//! by timestamp vs the graph clock, so the movie plays at the scaled
//! rate with zero cooperation from the chain. `IMediaSeeking::SetRate` — the design's
//! original mechanism — is NOT used: the game's WMV chain (its own
//! custom renderer '0001' + WMVideo Decoder DMO + WM ASF Reader) refuses
//! it categorically (E_INVALIDARG at every rate, paused AND running,
//! per-filter probes included — Windows cabinet tests #1–#3, 2026-08-23).
//! Install success is judged by a `GetSyncSource` POINTER READBACK,
//! never the HRESULT alone (the Wine silent-no-op lesson generalized);
//! any failure degrades PRE-RUN via [`degrade_to_suppressed`] — stop the
//! graph directly and zero the `opened` byte, reducing the player to the
//! suppress path's exact observable state so the game shows its static
//! no-movie background (a post-Run stop with `opened=1` drew an empty
//! black plane; a post-Run `opened=0` froze the first presented frame —
//! both live-observed and designed out by degrading before Run).
//! Cabinet-validated on real Windows (test #5, 2026-08-24: escalated
//! swap) AND under CrossOver/Wine (trial #2, 2026-08-24: in-place swap;
//! this superseded the original D14 Windows-only gate, whose rationale
//! was SetRate-specific). Under Wine, movies exist at all only in
//! `non_native_os_support.movie_mode="fallback"` — suppress mode never
//! builds a graph, so the toggle simply has nothing to act on there.
//! Position sync needs no rate awareness: sync events and
//! the movie timeline are both content-domain, and the scaled clock
//! stretches presentation exactly as the Q31 patch stretches the game
//! clock.
//!
//! ## Why seeks only happen while RUNNING (probe v1 post-mortem, 2026-08-21)
//!
//! Probe v1 issued SetRate + seek right at graph open, while the graph sat
//! PAUSED before the game's delayed Run. On Wine (CrossOver) that revealed:
//! - `SetRate`: full silent no-op (S_OK, readback 1.000, no visual speedup
//!   — confirmed by a gameplay recording compared against the raw file).
//!   Rate verification MUST use a GetRate readback, never the hr.
//! - Position readbacks echo the last seek target (30000 ms reported while
//!   the video played from ~0) — untrustworthy after a SetPositions.
//! - The paused-state seek itself kicked Wine's graph into presenting
//!   immediately: the video began at graph open (scroll start) instead of
//!   the game's Run at audio start. The stock game only ever seeks the
//!   graph BEFORE pausing it (inside BuildGraph) — a paused-state seek is
//!   a code path the game never exercises.
//!
//! Probe v2 (deploy #2, CrossOver) validated the production shape:
//! **running-state seeks work on Wine** (visible content jump, playback
//! continues; genuine clock positions read back correctly on an untouched
//! graph). It also proved the `on_frame` dispatch is NOT per rendered
//! frame (~2 kHz observed), so all timing here is wall-clock based.
//!
//! ## Diagnostic probe (`DDR_MOVIE_SYNC_PROBE`, dev only)
//!
//! With the env var set, each movie additionally gets: `SetRate(1.5)` +
//! readback at ~5 s of playback and a mid-song seek to 60 s at ~12 s, all
//! logged — plus the graph dump at capture (filters/CLSIDs, sync source,
//! per-filter SetRate verdicts): the evidence trail that killed SetRate
//! and motivated the clock proxy. Never set it in normal play — it
//! deliberately desyncs the movie.
//!
//! ## Fault injection (`DDR_MOVIE_SYNC_FAULT`, dev mode only)
//!
//! Gated on `layeredfs.developer_mode` (the `DDR_SONG_RATE_FAULT` policy):
//! `set-rate` corrupts the rate readback so every rate directive takes the
//! NFR-1 stop rung (movie stops, static background, song unaffected);
//! `seek` marks every capture non-seekable (position sync disabled — the
//! status-quo-desync rung). Exercises the failure ladder on a live
//! cabinet without touching game code.
//!
//! ## Player facts (RE, verified on gamemdx 20260616 + 20260721)
//!
//! `DShowPlayer` (captured `this` from the BuildGraph hook):
//! - `+0x08` state dword (0 closed / 2 running / 3 opened-not-running,
//!   refreshed per frame from `IMediaControl::GetState` by get-frame)
//! - `+0x14` opened byte — 1 only after a REAL graph build (stays 0 for
//!   faked/suppressed epilogues); the primary touch-gate
//! - `+0x16` loop flag (request flag bit 0; gameplay movies observe 0)
//! - `+0x58` `IMediaSeeking*` — **legitimately null** when the graph is not
//!   absolutely seekable (BuildGraph releases it on a failed
//!   `GetCapabilities & CanSeekAbsolute` check)
//! - player vtbl `+0x58` = the game's own null-guarded absolute seek
//!   (`IMediaSeeking::SetPositions(&pos_100ns, AbsolutePositioning, &0,
//!   NoPositioning)`).
//!
//! RE record: `.agents/planning/2026-08-20-background-movie-sync/research/movie-sync-re.md`.
//!
//! ## Threading
//!
//! `on_graph_opened` (BuildGraph detour), the `on_song_reset` subscriber,
//! the scene callback, and the `on_frame` driver all run on the game's
//! main update thread — the same thread that serializes every stock COM
//! call on this player (get-frame dispatch, event pump). The atomics are
//! belt-and-suspenders, not a cross-thread protocol.

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering};

#[cfg(windows)]
use std::ffi::c_void;
#[cfg(windows)]
use std::sync::atomic::AtomicPtr;

#[cfg(windows)]
use crate::{log_info, log_warn};

// ── DShowPlayer field offsets (RE-verified, stable across builds) ────────
#[cfg(windows)]
const PLAYER_STATE: usize = 0x08;
#[cfg(windows)]
const PLAYER_OPENED: usize = 0x14;
#[cfg(windows)]
const PLAYER_LOOP_FLAG: usize = 0x16;
#[cfg(windows)]
const PLAYER_MEDIA_SEEKING: usize = 0x58;
/// Player vtable slot: the game's own null-guarded absolute seek
/// (`fn(player, position_100ns)`).
#[cfg(windows)]
const PLAYER_VTBL_NATIVE_SEEK: usize = 0x58;
/// Player vtable slots: the tiny command setters (write the `+0x0C`
/// command byte; the game's own get-frame dispatch performs the actual
/// `IMediaControl::Pause`/`Run` on its next frame — the single stock COM
/// path). Semantics verified in the get-frame decompile: command 1 →
/// Pause (vtbl +0x40), command 2 → Run (vtbl +0x38).
#[cfg(windows)]
const PLAYER_VTBL_CMD_PAUSE: usize = 0x18;
#[cfg(windows)]
const PLAYER_VTBL_CMD_RUN: usize = 0x20;
/// Stop command setter (command 4 → `IMediaControl::Stop` on the game's
/// next get-frame). NOT used by the rate-sync failure rung anymore: that
/// rung zeroes the `opened` byte, after which get-frame never dispatches
/// the command byte again — it stops the graph directly instead (see
/// [`degrade_to_suppressed`]). Kept for future consumers of the stock
/// command path.
#[cfg(windows)]
const PLAYER_VTBL_CMD_STOP: usize = 0x28;
/// `IMediaControl*` (`player+0x50`, RE-verified: BuildGraph QIs it first).
#[cfg(windows)]
const PLAYER_MEDIA_CONTROL: usize = 0x50;
/// `IMediaControl` vtable: `Stop` (IUnknown 3 + IDispatch 4 + Run/Pause →
/// slot 9). Matches the get-frame decompile's Run=+0x38 / Pause=+0x40.
#[cfg(windows)]
const MC_STOP: usize = 0x48;
/// `IMediaControl::Pause` (slot 8) — the clock-proxy install's re-pause
/// after its stop/swap (see `rate_clock::install`).
#[cfg(windows)]
const MC_PAUSE: usize = 0x40;
/// Shared allocator/renderer struct pointer (`player+0x78`, RE-verified —
/// get-frame dereferences it every frame).
#[cfg(windows)]
const PLAYER_SHARED: usize = 0x78;
/// Frame-deposit slot inside the shared struct: the streaming thread
/// writes each new frame's sample pointer here; get-frame atomically
/// takes it (exchange-to-0). Deposits STOP during a seek's pipeline
/// restart and resume when decode reaches the target — the engine's
/// presentation-accurate restart-latency signal (the IMediaSeeking
/// position readback tracks the graph clock, NOT presentation, on Wine).
#[cfg(windows)]
const SHARED_DEPOSIT_SLOT: usize = 0x250;
/// Player state value while the graph is running (see module docs).
#[cfg(windows)]
const STATE_RUNNING: u32 = 2;

// ── IMediaSeeking vtable offsets (standard COM layout) ───────────────────
#[cfg(windows)]
const MS_GET_DURATION: usize = 0x50;
#[cfg(windows)]
const MS_GET_CURRENT_POSITION: usize = 0x60;
#[cfg(windows)]
const MS_SET_RATE: usize = 0x88;
#[cfg(windows)]
const MS_GET_RATE: usize = 0x90;

/// Sentinel: no pending seek.
const PENDING_NONE: i64 = i64::MIN;
/// Capture-time music counts closer to zero than this don't queue a
/// pending seek (the movie is already at 0; sub-500 ms alignment noise
/// isn't worth a startup seek).
const CAPTURE_SYNC_THRESHOLD_MS: i64 = 500;
/// Non-looping seek targets clamp to `duration - this` instead of the
/// exact end: seeking AT the end fires EC_COMPLETE, whose stock non-loop
/// handling STOPS the graph — an unrecoverable state for the drain (it
/// requires RUNNING), live-observed as a frozen movie on 2026-08-21
/// (cabinet test #3). A genuinely past-the-end song still ends the movie
/// naturally through the clock; we just never trigger the stop ourselves.
const CLAMP_END_MARGIN_100NS: i64 = 500 * 10_000;
/// The live count is "settled" when it sits within this window of the
/// queued trigger value. Must be LARGER than the training silent-approach
/// lead (2.5 s — the legitimate divergence between a notification's
/// destination and the live count) and SMALLER than one FF/RW scrub step
/// (5000 ms): with a ±5000 window, mashed scrubs let the drain accept the
/// STALE pre-scrub count (exactly one step away from the new trigger) and
/// seek to the old position (cabinet test #6, 2026-08-21 — mash desync).
/// It also rejects the reset transaction's ~0 transient (cabinet test #5).
const SETTLE_WINDOW_MS: i64 = 3_000;
/// If the live count never settles (or is unreadable) this long after the
/// queue, stop waiting: seek to the live count if readable (it is the
/// ground truth once the transaction is done), else to the queued value.
const SETTLE_TIMEOUT_MS: u64 = 500;

/// What one drain tick should do with a pending seek. Pure — host-tested.
///
/// Two execution paths (the whole sync model, post-simplification):
/// - **Jump**: the audio continues immediately at the new position
///   (scrubs). The video chases a moving target, so the seek leads by the
///   measured restart latency (per-direction estimates + one bounded
///   correction).
/// - **Park**: the audio will START at a known content time after a
///   silent lead-in (loop wrap, quick/delayed restart, SONG START — every
///   event whose live count sits meaningfully BELOW its trigger, or is
///   still negative). No prediction at all: seek the video to the
///   destination, PAUSE it when its frame arrives, RUN it when the count
///   crosses the destination. Deterministic event ordering; parks never
///   touch the jump path's learned estimates (the two scenario classes
///   have different real latencies — sharing one estimate made them
///   flip-flop, cabinet tests #7–#9).
#[derive(Debug, PartialEq, Eq)]
pub enum DrainAction {
    /// Keep the pending seek armed; re-evaluate next tick.
    Hold,
    /// Audio already running at the target: compensated seek to this
    /// content time (ms).
    Jump(i64),
    /// Audio starts at `run_at` (content ms) after a silent approach:
    /// seek there, pause on frame arrival, run at the crossing.
    Park { run_at: i64 },
}

/// Minimum wall gap between two live-count samples before the advancing
/// test is meaningful.
const ADVANCE_MIN_GAP_MS: i64 = 20;
/// An approach is parked when the trigger sits at least this far ABOVE
/// the live count (the training lead is 2.5 s; scrub landings differ by
/// only wall-progression ms).
const APPROACH_MIN_MS: i64 = 500;

/// Whether the live count is advancing coherently (~1 ms per wall ms)
/// between two samples — the uniform transient filter: mid-transaction
/// counts jump or freeze, settled counts progress at wall rate. Pure.
#[must_use]
pub fn is_advancing(d_live_ms: i64, d_wall_ms: i64) -> bool {
    d_wall_ms >= ADVANCE_MIN_GAP_MS && (d_live_ms - d_wall_ms).abs() <= d_wall_ms / 2 + 10
}

// ── Seek restart-latency compensation (cabinet tests #6/#7). A RUNNING
// seek restarts the pipeline (flush → reposition → decode to target);
// the game keeps playing during that restart, so the movie resumes
// exactly `R` behind — the 200–400 ms lag observed after EVERY seek
// scenario. `R` is machine/movie-dependent and MUST NOT be hardcoded
// (maintainer directive): it is measured at runtime from the streaming
// thread's frame-deposit slot (`*(player+0x78)+0x250` — written per
// movie frame by the streaming thread, atomically taken by get-frame;
// deposits STOP during the restart and resume when decode reaches the
// target, so seek→first-deposit = R). Each seek leads its target by the
// current estimate and re-measures; one corrective re-seek fires when
// the estimate was off. On a platform where seeks restart instantly
// (real Windows), R measures ~0 and the lead self-neutralizes.
//
// Rev D's clock-readback verification is GONE: Wine's
// `GetCurrentPosition` tracks the graph clock, not presentation — it
// reported "in sync" while the eye saw 200–400 ms (cabinet test #7), and
// its one "correction" was built on that lie. Never measure sync with
// the position readback on Wine. ─────────────────────────────────────────
/// First-ever jump park (no measurement yet) assumes this restart
/// headroom (ms) — one-time; measurements take over immediately.
const JUMP_DELTA_DEFAULT_MS: i64 = 500;
/// Restart measurements above this are abandoned (movie ended, stopped,
/// or the deposit signal is broken) — the estimate keeps its last value.
const MEASURE_TIMEOUT_MS: u64 = 3_000;
/// The lead estimate is clamped to this range (ms).
const LEAD_CLAMP_MS: i64 = 2_000;
/// Sentinel for "no estimate yet".
const LEAD_UNMEASURED: i64 = -1;

/// Fold a new restart-latency measurement into the running estimate.
/// First measurement adopts the value; later ones average toward it
/// (cheap EWMA, alpha 0.5). Clamped to `0..=LEAD_CLAMP_MS`. Pure.
#[must_use]
pub fn lead_update(previous_ms: i64, measured_ms: i64) -> i64 {
    let next = if previous_ms < 0 {
        measured_ms
    } else {
        (previous_ms + measured_ms) / 2
    };
    next.clamp(0, LEAD_CLAMP_MS)
}

/// The park-ahead headroom for a jump (scrub): the video parks at
/// `live + Δ` and runs when the audio arrives. Δ only sizes the brief
/// post-scrub freeze — SYNC no longer depends on it (the park's crossing
/// governs) — so it just needs to exceed this machine's restart latency
/// most of the time: 2× the measured per-direction estimate (headroom
/// for the observed ~2× per-seek variance), clamped. Machines with
/// instant seeks measure ~0 ⇒ Δ→0 ⇒ the park degrades to a plain seek
/// (crossing passes before the frame arrives; no pause). Pure.
#[must_use]
pub fn jump_delta(estimate_ms: i64) -> i64 {
    if estimate_ms < 0 {
        JUMP_DELTA_DEFAULT_MS
    } else {
        (estimate_ms * 2).min(LEAD_CLAMP_MS)
    }
}

/// Restart latency is DIRECTION-ASYMMETRIC (cabinet test #8: after FF
/// mashes the video sat slightly behind, after RW mashes ~150–200 ms
/// AHEAD — one mixed estimate over-leads backward seeks, whose targets
/// lie in already-read/cached content and restart faster than forward
/// ones needing fresh demux+decode). Seeks therefore classify as
/// forward/backward against the movie's EXPECTED content position (last
/// seek target + wall time since, i.e. where the movie should be now)
/// and use a per-direction estimate. Pure.
#[must_use]
pub fn is_backward_seek(target_ms: i64, expected_movie_ms: i64) -> bool {
    target_ms < expected_movie_ms
}

/// The drain rule: classify the pending seek given the queued trigger,
/// the live count, whether the count is advancing coherently (see
/// [`is_advancing`]), and the time since queueing.
///
/// - Negative live count: the engine's own pre-song/pre-target approach
///   toward 0 — Park at the trigger (covers quick AND delayed restarts).
/// - Not yet advancing coherently: the reset transaction's transient
///   (counts jump around / freeze) — Hold (timeout escapes toward the
///   live count).
/// - Advancing, trigger ≥ [`APPROACH_MIN_MS`] above the live count and
///   within the settle window: a silent approach — Park at the trigger.
/// - Advancing within the settle window otherwise: Jump to the live
///   count (locks through the residual difference).
/// - Advancing but far from the trigger past the timeout: trust the live
///   count; unreadable count past the timeout: the queued value.
#[must_use]
pub fn drain_action(
    queued_ms: i64,
    live_ms: Option<i64>,
    advancing: bool,
    elapsed_ms: u64,
) -> DrainAction {
    match live_ms {
        Some(live) if live < 0 => DrainAction::Park {
            run_at: queued_ms.max(0),
        },
        Some(live) => {
            if !advancing {
                if elapsed_ms >= SETTLE_TIMEOUT_MS {
                    DrainAction::Jump(live)
                } else {
                    DrainAction::Hold
                }
            } else if (live - queued_ms).abs() <= SETTLE_WINDOW_MS {
                if queued_ms - live >= APPROACH_MIN_MS {
                    DrainAction::Park { run_at: queued_ms }
                } else {
                    DrainAction::Jump(live)
                }
            } else if elapsed_ms >= SETTLE_TIMEOUT_MS {
                DrainAction::Jump(live)
            } else {
                DrainAction::Hold
            }
        }
        None => {
            if elapsed_ms >= SETTLE_TIMEOUT_MS {
                DrainAction::Jump(queued_ms)
            } else {
                DrainAction::Hold
            }
        }
    }
}

/// Whether the engine initialized (movie_policy + scene_manager available).
static AVAILABLE: AtomicBool = AtomicBool::new(false);

// ── Capture state (one player at a time, scene-scoped) ──────────────────
#[cfg(windows)]
static CAPTURED: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
/// Bumped on every capture AND every drop; consumers validate their
/// snapshot against it before touching the player.
#[cfg(windows)]
static GENERATION: AtomicU32 = AtomicU32::new(0);
/// Captured player's loop flag (`+0x16`).
static CAPTURE_LOOP: AtomicBool = AtomicBool::new(false);
/// Captured player has a live `IMediaSeeking` (position sync possible).
static CAPTURE_SEEKABLE: AtomicBool = AtomicBool::new(false);
/// Movie duration in 100 ns; 0 = not yet read (lazy — the read is a COM
/// call, deferred to the first RUNNING frame like every other COM touch).
static CAPTURE_DURATION_100NS: AtomicI64 = AtomicI64::new(0);
/// Whether the lazy duration read already happened for this capture.
static DURATION_READ: AtomicBool = AtomicBool::new(false);

/// The pending seek target (song content ms), drained while RUNNING.
/// `PENDING_NONE` = empty. Written by the `on_song_reset` subscriber and
/// the capture-time music-count sync; consumed by the frame driver.
static PENDING_SEEK_MS: AtomicI64 = AtomicI64::new(PENDING_NONE);
/// Wall-clock ms when the pending seek was queued (settle timeout basis).
static PENDING_QUEUED_AT_MS: AtomicU64 = AtomicU64::new(0);

// ── Restart-latency estimates (size the jump-park Δ; see `jump_delta`) ──
/// Learned restart latencies (ms), split by seek direction (see
/// [`is_backward_seek`]); `LEAD_UNMEASURED` until first measured. Fed by
/// every park's seek→first-deposit gap. **PER-CAPTURE** (reset with the
/// capture): carried cross-song state consistently performed WORSE than
/// the fresh defaults (cabinet tests #12–#13: first playthroughs perfect,
/// repeats desynced) — every song is treated as a first playthrough, and
/// within-song measurements only resize that song's freezes.
static LEAD_FORWARD_MS: AtomicI64 = AtomicI64::new(LEAD_UNMEASURED);
static LEAD_BACKWARD_MS: AtomicI64 = AtomicI64::new(LEAD_UNMEASURED);
/// Which estimate the in-flight park's measurement belongs to.
static MEASURE_BACKWARD: AtomicBool = AtomicBool::new(false);
/// Movie-position expectation basis: the last seek's CONTENT target (ms)
/// and its wall-clock time. Per-capture (reset on drop) — expectations
/// must not survive across movies.
static LAST_SEEK_TARGET_MS: AtomicI64 = AtomicI64::new(i64::MIN);
static LAST_SEEK_AT_MS: AtomicU64 = AtomicU64::new(0);

// ── Park state (the approach path — see `DrainAction::Park`) ────────────
const PARK_IDLE: u32 = 0;
/// Seeked to the destination; waiting for its frame deposit to pause on.
const PARK_WAIT_DEPOSIT: u32 = 1;
/// Paused at the destination; waiting for the live count to cross it.
const PARK_PAUSED: u32 = 2;
/// Deposit never came in time — no pause issued; just wait the crossing
/// out (video restarts on its own, late by the restart remainder).
const PARK_NO_PAUSE: u32 = 3;
static PARK_STAGE: AtomicU32 = AtomicU32::new(PARK_IDLE);
/// Content ms the parked movie runs at (the audio start).
static PARK_RUN_AT_MS: AtomicI64 = AtomicI64::new(0);
/// Wall-clock ms of the park seek (deposit timeout basis).
static PARK_SEEK_AT_MS: AtomicU64 = AtomicU64::new(0);
/// Deposit-slot snapshot at park-seek time (stale-frame filter).
static PARK_DEPOSIT_SNAPSHOT: AtomicI64 = AtomicI64::new(0);

// ── Live-count stability sampling (the `is_advancing` inputs) ───────────
static SAMPLE_LIVE_MS: AtomicI64 = AtomicI64::new(i64::MIN);
static SAMPLE_WALL_MS: AtomicU64 = AtomicU64::new(0);

// ── Rate-sync consumption diagnostic (one INFO per engaged song) ────────
/// Armed by a successful clock-proxy install; ~2 s after the game's Run
/// the frame driver logs how often the graph consulted the proxy
/// (`rate_clock::consumption`). Near-zero numbers with a verified install
/// mean the renderer paces by some OTHER clock — the discriminator the
/// Wine trial (and any future chain change) needs.
static RATE_DIAG_ARMED: AtomicBool = AtomicBool::new(false);
/// Wall-clock deadline for the report; 0 = Run not yet observed.
static RATE_DIAG_DEADLINE_MS: AtomicU64 = AtomicU64::new(0);
/// Wall-clock ms when the diagnostic armed (install time) — the wedge
/// watchdog's basis: a player that never reaches RUNNING within
/// [`RATE_DIAG_WEDGE_MS`] of a verified install is a wedged graph
/// (CrossOver trial #1's silent failure mode, now named in the log).
static RATE_DIAG_ARMED_AT_MS: AtomicU64 = AtomicU64::new(0);
const RATE_DIAG_DELAY_MS: u64 = 2_000;
/// Songs Run their movie within ~5–8 s of the graph open (the stage
/// loading screen); 15 s without RUNNING after an install is a wedge.
const RATE_DIAG_WEDGE_MS: u64 = 15_000;

// ── Fault injection (dev only, `DDR_MOVIE_SYNC_FAULT`) ───────────────────
// Boot-only, `layeredfs.developer_mode` gated (same policy as
// `DDR_SONG_RATE_FAULT`): exercises the NFR-1 failure ladder on a live
// cabinet. `set-rate` forces the rate-sync failure rung (the clock-proxy
// install is treated as failed — the movie degrades to the static
// background pre-Run); `seek` treats every captured graph as non-seekable
// (position sync disabled — the status-quo-desync rung). One selector at
// a time.
static FAULT_SET_RATE: AtomicBool = AtomicBool::new(false);
static FAULT_SEEK: AtomicBool = AtomicBool::new(false);

// ── Probe state (dev only, `DDR_MOVIE_SYNC_PROBE`) ───────────────────────
/// `DDR_MOVIE_SYNC_PROBE` latched at init.
static PROBE: AtomicBool = AtomicBool::new(false);
/// 0 = idle/done for this capture, 1 = waiting for Run, 2 = running.
static PROBE_STAGE: AtomicU32 = AtomicU32::new(0);
/// Wall-clock ms timestamp of the first observed RUNNING frame.
static PROBE_RUN_STARTED_MS: AtomicU64 = AtomicU64::new(0);
/// Bitmask of probe actions already fired this capture (1 = rate, 2 = seek).
static PROBE_FIRED: AtomicU32 = AtomicU32::new(0);

const PROBE_STAGE_IDLE: u32 = 0;
const PROBE_STAGE_WAIT_RUN: u32 = 1;
const PROBE_STAGE_RUNNING: u32 = 2;
/// Wall-clock delays after the game's Run for the probe actions. Probe v2
/// proved the frame dispatch is ~2 kHz, so frame counting is meaningless —
/// these are real seconds of visible playback.
const PROBE_RATE_AT_MS: u64 = 5_000;
const PROBE_SEEK_AT_MS: u64 = 12_000;

/// Map a song content time (ms) to a movie seek target (100 ns units),
/// honoring the player's loop flag and known duration. Pure — host-tested.
///
/// - Negative content times (delayed-restart pre-anchor countdown) clamp
///   to 0: the movie waits at its first frame.
/// - Unknown duration (0) passes the target through unmapped.
/// - Looping movies wrap modulo duration (the game's own EC_COMPLETE
///   loop-wrap equivalent); non-looping movies clamp to
///   `duration - CLAMP_END_MARGIN_100NS` — never the exact end, which
///   would fire EC_COMPLETE and stop the graph unrecoverably (see the
///   margin const).
#[must_use]
pub fn map_position(t_ms: i64, duration_100ns: i64, loop_flag: bool) -> i64 {
    let target = t_ms.max(0).saturating_mul(10_000);
    if duration_100ns <= 0 {
        return target;
    }
    if loop_flag {
        target % duration_100ns
    } else {
        target.min((duration_100ns - CLAMP_END_MARGIN_100NS).max(0))
    }
}

/// Pure validity predicate for touching a captured player's COM surface.
/// `state`/`opened` are the raw player fields; `iface_null` reports the
/// needed interface pointer. Kept pure for host tests.
#[must_use]
pub fn gate_ok(state: u32, opened: u8, iface_null: bool) -> bool {
    opened == 1 && state != 0 && !iface_null
}

// ── Rate sync (the committed non-identity directive) ─────────────────────
/// Identity guard: a directive this close to 1.0 needs no rate mechanism.
/// (Kept from the SetRate era; the supported domain is multiples of 0.05
/// in 0.25..=1.75, so 0.01 cleanly separates identity.)
const RATE_EPSILON: f64 = 0.01;

// ── Scaled-clock time mapping (pure — host-tested) ──────────────────────
// The rate mechanism: DirectShow filters pace themselves by TIMESTAMP vs
// the graph's reference clock, so a clock that runs at `rate × real time`
// plays the movie at `rate` with zero cooperation from any filter —
// `IMediaSeeking::SetRate` needs the chain's consent and the game's
// WMV chain categorically refuses it (E_INVALIDARG at every rate, both
// states, per-filter probes included; Windows cabinet tests #1–#3,
// 2026-08-23). Proxy time: `T(t) = T0 + (t − t0)·r`; an advise deadline X
// in proxy time is reached at real time `t0 + (X − T0)/r`.

/// Map a real-clock instant to proxy time. `rate <= 0` degrades to 1.0
/// (defensive — the directive domain is 0.25..=1.75).
#[must_use]
pub fn scale_clock_time(t_real: i64, basis_real: i64, basis_ours: i64, rate: f64) -> i64 {
    let r = if rate > 0.0 { rate } else { 1.0 };
    basis_ours.saturating_add((t_real.saturating_sub(basis_real) as f64 * r) as i64)
}

/// Map a proxy-time advise deadline to the real-clock instant at which the
/// proxy reaches it (the AdviseTime/AdvisePeriodic start forwarding rule).
#[must_use]
pub fn map_advise_deadline(deadline_ours: i64, basis_real: i64, basis_ours: i64, rate: f64) -> i64 {
    let r = if rate > 0.0 { rate } else { 1.0 };
    basis_real.saturating_add((deadline_ours.saturating_sub(basis_ours) as f64 / r) as i64)
}

/// Scale an advise period from proxy time to real time, floored at one
/// 100 ns unit (a zero period would busy-signal the semaphore).
#[must_use]
pub fn map_advise_period(period_ours: i64, rate: f64) -> i64 {
    let r = if rate > 0.0 { rate } else { 1.0 };
    ((period_ours as f64 / r) as i64).max(1)
}

#[must_use]
pub fn is_available() -> bool {
    AVAILABLE.load(Ordering::Acquire)
}

/// Initialize the engine. Requires the shared movie hook (capture source)
/// and a working scene manager (the capture-drop safety invariant: a
/// captured player must never outlive the scene it was opened in).
/// `song_reset` needs no availability gate: an uninitialized song_reset
/// simply never fires the subscription and reports no music count.
#[cfg(windows)]
pub fn init(movie_policy_ok: bool, scene_manager_ok: bool) -> bool {
    use crate::services::{input_manager, scene_manager, song_reset};

    if !movie_policy_ok {
        log_warn!("movie_sync: shared movie hook unavailable -- engine disabled");
        return false;
    }
    if !scene_manager_ok {
        log_warn!("movie_sync: scene manager unavailable -- engine disabled (capture hygiene)");
        return false;
    }
    let probe = std::env::var("DDR_MOVIE_SYNC_PROBE")
        .map(|v| !v.is_empty())
        .unwrap_or(false);
    PROBE.store(probe, Ordering::Release);
    // Boot-only fault injection (developer mode only; unknown values warn
    // and select nothing — the DDR_SONG_RATE_FAULT policy).
    if let Ok(value) = std::env::var("DDR_MOVIE_SYNC_FAULT") {
        let dev_mode = crate::mods::config::get()
            .and_then(|c| c.layeredfs.as_ref())
            .map(|l| l.developer_mode)
            .unwrap_or(false);
        if !dev_mode {
            log_warn!(
                "movie_sync: DDR_MOVIE_SYNC_FAULT ignored (requires layeredfs.developer_mode)"
            );
        } else {
            match value.trim() {
                "set-rate" => {
                    FAULT_SET_RATE.store(true, Ordering::Release);
                    log_warn!(
                        "movie_sync: FAULT INJECTION ACTIVE (set-rate) — every rate directive takes the stop rung"
                    );
                }
                "seek" => {
                    FAULT_SEEK.store(true, Ordering::Release);
                    log_warn!(
                        "movie_sync: FAULT INJECTION ACTIVE (seek) — every capture is treated as non-seekable"
                    );
                }
                other => {
                    log_warn!("movie_sync: unknown DDR_MOVIE_SYNC_FAULT value '{}'", other);
                }
            }
        }
    }

    scene_manager::on_scene_change(Box::new(|_prev, _next| {
        drop_capture();
    }));
    // Timeline jumps: quick restart, training loop/scrub/restart-from-A —
    // every completed in-place reset/seek, content-domain ms, game thread.
    song_reset::on_song_reset(|t_ms| {
        queue_seek(i64::from(t_ms), "reset");
    });
    // Per-frame driver (game thread): pending-seek drain + probe stages.
    input_manager::on_frame(std::sync::Arc::new(|| {
        frame_tick();
    }));

    AVAILABLE.store(true, Ordering::Release);
    if probe {
        log_info!(
            "movie_sync: initialized WITH live probe (DDR_MOVIE_SYNC_PROBE) -- per movie: \
             SetRate(1.5) at +5s of playback, seek to 60s at +12s. Do not judge sync with this set."
        );
    } else {
        log_info!("movie_sync: initialized (position sync active)");
    }
    true
}

/// Queue a pending seek to `t_ms` (song content ms). Drained by the frame
/// driver at the next RUNNING frame — typically the same frame for
/// mid-song events, or the game's own Run for pre-song targets.
#[cfg(windows)]
fn queue_seek(t_ms: i64, source: &str) {
    if CAPTURED.load(Ordering::Acquire).is_null() {
        return;
    }
    if !CAPTURE_SEEKABLE.load(Ordering::Acquire) {
        // Warned once at capture; stay quiet per event.
        return;
    }
    PENDING_QUEUED_AT_MS.store(now_ms(), Ordering::Release);
    SAMPLE_LIVE_MS.store(i64::MIN, Ordering::Release);
    PENDING_SEEK_MS.store(t_ms, Ordering::Release);
    crate::log_debug!("movie_sync: pending seek {} ms ({})", t_ms, source);
}

/// Drop the captured player (scene change / teardown awareness).
#[cfg(windows)]
fn drop_capture() {
    let prev = CAPTURED.swap(std::ptr::null_mut(), Ordering::AcqRel);
    if !prev.is_null() {
        GENERATION.fetch_add(1, Ordering::AcqRel);
    }
    PENDING_SEEK_MS.store(PENDING_NONE, Ordering::Release);
    CAPTURE_DURATION_100NS.store(0, Ordering::Release);
    RATE_DIAG_ARMED.store(false, Ordering::Release);
    RATE_DIAG_DEADLINE_MS.store(0, Ordering::Release);
    RATE_DIAG_ARMED_AT_MS.store(0, Ordering::Release);
    DURATION_READ.store(false, Ordering::Release);
    CAPTURE_SEEKABLE.store(false, Ordering::Release);
    PARK_STAGE.store(PARK_IDLE, Ordering::Release);
    SAMPLE_LIVE_MS.store(i64::MIN, Ordering::Release);
    LEAD_FORWARD_MS.store(LEAD_UNMEASURED, Ordering::Release);
    LEAD_BACKWARD_MS.store(LEAD_UNMEASURED, Ordering::Release);
    // The learned lead estimates are process-global and survive captures;
    // only the movie-position expectation resets.
    LAST_SEEK_TARGET_MS.store(i64::MIN, Ordering::Release);
    LAST_SEEK_AT_MS.store(0, Ordering::Release);
    PROBE_STAGE.store(PROBE_STAGE_IDLE, Ordering::Release);
    PROBE_FIRED.store(0, Ordering::Release);
}

/// Called by `movie_policy`'s BuildGraph detour after a REAL successful
/// graph build (`CallOutcome::Passthrough` with hr == 0). Game update
/// thread. Panic-contained. Performs COM only for the rate directive
/// (the clock-proxy install — see [`apply_rate_directive`]); all
/// other COM (seeks, duration) stays deferred to the first RUNNING frame
/// (probe v1 showed even a paused-state seek disturbs Wine's graph; see
/// module docs).
#[cfg(windows)]
pub fn on_graph_opened(player: *mut c_void) {
    if !AVAILABLE.load(Ordering::Acquire) || player.is_null() {
        return;
    }
    let result = std::panic::catch_unwind(|| unsafe { capture(player) });
    if result.is_err() {
        log_warn!("movie_sync: panic contained in on_graph_opened -- capture dropped");
        drop_capture();
    }
}

/// # Safety
/// `player` points at a live `DShowPlayer` whose BuildGraph just returned
/// success on this thread (COM pointers valid, epilogue fields written).
#[cfg(windows)]
unsafe fn capture(player: *mut c_void) {
    use crate::services::song_reset;

    let base = player as *const u8;
    let state = std::ptr::read_volatile(base.add(PLAYER_STATE) as *const u32);
    let opened = std::ptr::read_volatile(base.add(PLAYER_OPENED));
    let loop_flag = std::ptr::read_volatile(base.add(PLAYER_LOOP_FLAG)) != 0;
    let seeking = std::ptr::read_volatile(base.add(PLAYER_MEDIA_SEEKING) as *const *mut c_void);

    if !gate_ok(state, opened, false) {
        // A Passthrough success without opened==1 would contradict the
        // epilogue RE — log loudly rather than capture garbage.
        log_warn!(
            "movie_sync: post-open gate failed (state={}, opened={}) -- not captured",
            state,
            opened
        );
        return;
    }

    // Fresh capture: reset per-capture state before publishing the pointer.
    drop_capture();
    let seek_fault = FAULT_SEEK.load(Ordering::Acquire);
    CAPTURE_LOOP.store(loop_flag, Ordering::Release);
    CAPTURE_SEEKABLE.store(!seeking.is_null() && !seek_fault, Ordering::Release);
    CAPTURED.store(player, Ordering::Release);
    GENERATION.fetch_add(1, Ordering::AcqRel);
    log_info!(
        "movie_sync: captured player {:p} (loop={}, state={}, seekable={})",
        player,
        loop_flag,
        state,
        !seeking.is_null()
    );
    // Rate directive: a committed non-identity rate with SYNC BACKGROUND
    // VIDEO latched ON (either platform — D14 superseded 2026-08-24).
    // Install the scaled reference-clock proxy
    // (readback-verified); failure degrades pre-Run to the
    // suppressed-equivalent state (clean static background).
    if !apply_rate_directive(player) {
        return;
    }
    if seeking.is_null() {
        log_warn!("movie_sync: graph not seekable -- position sync unavailable for this movie");
        return;
    }
    if seek_fault {
        // Dev fault injection: the status-quo-desync rung — the movie
        // plays but no sync event will ever touch it (queue_seek refuses
        // on the seekable flag).
        log_warn!(
            "movie_sync: FAULT seek — position sync disabled for this capture (movie free-runs)"
        );
        return;
    }

    // Capture-time position sync: if the song is already mid-run (a LATE
    // graph build — the run is live and anchored), queue it. Gating on
    // `first_anchored_frame()` is load-bearing: before the game's own
    // 0x1044 anchors the run, the raw count field reads session-wall-clock
    // garbage that passes the sanity range (454647 ms live-observed,
    // cabinet test #3) — queueing it seeked the movie to its end, whose
    // EC_COMPLETE stop froze it unrecoverably. In the normal ordering
    // (graph opens pre-song, unanchored) the training start-adjust /
    // reset notifications carry the position instead.
    if song_reset::first_anchored_frame() {
        if let Some(t) = song_reset::current_raw_music_count() {
            let t = i64::from(t);
            if t > CAPTURE_SYNC_THRESHOLD_MS {
                PENDING_QUEUED_AT_MS.store(now_ms(), Ordering::Release);
                PENDING_SEEK_MS.store(t, Ordering::Release);
                log_info!(
                    "movie_sync: capture-time sync queued ({} ms, anchored run)",
                    t
                );
            }
        }
    }

    if PROBE.load(Ordering::Acquire) {
        probe_dump_graph(player);
        PROBE_FIRED.store(0, Ordering::Release);
        PROBE_STAGE.store(PROBE_STAGE_WAIT_RUN, Ordering::Release);
    }
}

// ── Probe graph dump (dev only) ──────────────────────────────────────────
// Answers "WHO rejects SetRate": enumerates the live graph's filters
// (name + CLSID), reports the sync source, and probes each filter's OWN
// IMediaSeeking with SetRate(1.5) → readback → SetRate(1.0) restore. The
// graph-level SetRate is distributed by the filter graph manager and fails
// if ANY participant refuses (Windows test #2: E_INVALIDARG at every rate
// in both states) — this pinpoints the refusing filter. Paused-graph safe:
// enumeration is read-only QI walking, and the per-filter probes restore
// 1.0 immediately.

#[cfg(windows)]
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
struct ComGuid {
    d1: u32,
    d2: u16,
    d3: u16,
    d4: [u8; 8],
}

#[cfg(windows)]
const IID_IFILTER_GRAPH: ComGuid = ComGuid {
    d1: 0x56A8_689F,
    d2: 0x0AD4,
    d3: 0x11CE,
    d4: [0xB0, 0x3A, 0x00, 0x20, 0xAF, 0x0B, 0xA7, 0x70],
};
#[cfg(windows)]
const IID_IMEDIA_FILTER: ComGuid = ComGuid {
    d1: 0x56A8_6899,
    d2: 0x0AD4,
    d3: 0x11CE,
    d4: [0xB0, 0x3A, 0x00, 0x20, 0xAF, 0x0B, 0xA7, 0x70],
};
#[cfg(windows)]
const IID_IMEDIA_SEEKING: ComGuid = ComGuid {
    d1: 0x36B7_3880,
    d2: 0xC2C8,
    d3: 0x11CF,
    d4: [0x8B, 0x46, 0x00, 0x80, 0x5F, 0x6C, 0xEF, 0x60],
};

/// IUnknown::QueryInterface (vtbl slot 0). Returns an AddRef'd interface.
///
/// # Safety
/// `unknown` is a live COM interface pointer.
#[cfg(windows)]
unsafe fn com_qi(unknown: *mut c_void, iid: &ComGuid) -> Option<*mut c_void> {
    type QiFn = unsafe extern "system" fn(*mut c_void, *const ComGuid, *mut *mut c_void) -> i32;
    let vtbl = *(unknown as *const *const u8);
    let qi: QiFn = std::mem::transmute(*(vtbl as *const usize));
    let mut out: *mut c_void = std::ptr::null_mut();
    if qi(unknown, iid, &mut out) >= 0 && !out.is_null() {
        Some(out)
    } else {
        None
    }
}

/// IUnknown::Release (vtbl slot 2).
///
/// # Safety
/// `unknown` is a live, owned COM interface pointer.
#[cfg(windows)]
unsafe fn com_release(unknown: *mut c_void) {
    type ReleaseFn = unsafe extern "system" fn(*mut c_void) -> u32;
    let vtbl = *(unknown as *const *const u8);
    let release: ReleaseFn = std::mem::transmute(*(vtbl.add(0x10) as *const usize));
    let _ = release(unknown);
}

/// One-shot diagnostic dump of the freshly built graph (probe env only).
///
/// # Safety
/// Caller contract of [`capture`] (live player, game update thread).
#[cfg(windows)]
unsafe fn probe_dump_graph(player: *mut c_void) {
    let base = player as *const u8;
    let media_control =
        std::ptr::read_volatile(base.add(PLAYER_MEDIA_CONTROL) as *const *mut c_void);
    if media_control.is_null() {
        log_info!("movie_sync[probe]: no IMediaControl -- graph dump unavailable");
        return;
    }
    let Some(graph) = com_qi(media_control, &IID_IFILTER_GRAPH) else {
        log_info!("movie_sync[probe]: IFilterGraph QI failed -- graph dump unavailable");
        return;
    };
    // Sync source: IMediaFilter::GetSyncSource (slot 9). A live clock is
    // the prerequisite for any rate-by-clock alternative.
    if let Some(media_filter) = com_qi(graph, &IID_IMEDIA_FILTER) {
        type GetSyncFn = unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32;
        let vtbl = *(media_filter as *const *const u8);
        let get_sync: GetSyncFn = std::mem::transmute(*(vtbl.add(0x48) as *const usize));
        let mut clock: *mut c_void = std::ptr::null_mut();
        let hr = get_sync(media_filter, &mut clock);
        log_info!(
            "movie_sync[probe]: graph sync source hr={:#010x} clock={:p}",
            hr,
            clock
        );
        if !clock.is_null() {
            com_release(clock);
        }
        com_release(media_filter);
    }
    // IFilterGraph::EnumFilters (slot 5).
    type EnumFiltersFn = unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32;
    let graph_vtbl = *(graph as *const *const u8);
    let enum_filters: EnumFiltersFn = std::mem::transmute(*(graph_vtbl.add(0x28) as *const usize));
    let mut enumerator: *mut c_void = std::ptr::null_mut();
    if enum_filters(graph, &mut enumerator) < 0 || enumerator.is_null() {
        log_info!("movie_sync[probe]: EnumFilters failed");
        com_release(graph);
        return;
    }
    // IEnumFilters::Next (slot 3).
    type NextFn = unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void, *mut u32) -> i32;
    let next: NextFn = {
        let vtbl = *(enumerator as *const *const u8);
        std::mem::transmute(*(vtbl.add(0x18) as *const usize))
    };
    for index in 0..16 {
        let mut filter: *mut c_void = std::ptr::null_mut();
        let mut fetched: u32 = 0;
        if next(enumerator, 1, &mut filter, &mut fetched) != 0 || fetched == 0 || filter.is_null() {
            break;
        }
        probe_dump_filter(index, filter);
        com_release(filter);
    }
    com_release(enumerator);
    com_release(graph);
}

/// Log one filter's identity (CLSID + name) and its own IMediaSeeking's
/// SetRate verdict (probe 1.5, restore 1.0).
///
/// # Safety
/// `filter` is a live, owned `IBaseFilter*`.
#[cfg(windows)]
unsafe fn probe_dump_filter(index: usize, filter: *mut c_void) {
    let vtbl = *(filter as *const *const u8);
    // IPersist::GetClassID (slot 3).
    type GetClassIdFn = unsafe extern "system" fn(*mut c_void, *mut ComGuid) -> i32;
    let get_class_id: GetClassIdFn = std::mem::transmute(*(vtbl.add(0x18) as *const usize));
    let mut clsid = ComGuid {
        d1: 0,
        d2: 0,
        d3: 0,
        d4: [0; 8],
    };
    let _ = get_class_id(filter, &mut clsid);
    // IBaseFilter::QueryFilterInfo (slot 12): FILTER_INFO = WCHAR[128] +
    // an AddRef'd IFilterGraph* that must be released.
    #[repr(C)]
    struct FilterInfo {
        name: [u16; 128],
        graph: *mut c_void,
    }
    type QueryInfoFn = unsafe extern "system" fn(*mut c_void, *mut FilterInfo) -> i32;
    let query_info: QueryInfoFn = std::mem::transmute(*(vtbl.add(0x60) as *const usize));
    let mut info = FilterInfo {
        name: [0; 128],
        graph: std::ptr::null_mut(),
    };
    let name = if query_info(filter, &mut info) >= 0 {
        if !info.graph.is_null() {
            com_release(info.graph);
        }
        let len = info.name.iter().position(|&c| c == 0).unwrap_or(128);
        String::from_utf16_lossy(&info.name[..len])
    } else {
        "<unnamed>".to_string()
    };
    // The filter's OWN seeking interface (renderers pass through upstream;
    // the source filter's answer is the authority the FGM distributes to).
    let (seek_report, caps) = match com_qi(filter, &IID_IMEDIA_SEEKING) {
        Some(seeking) => {
            type GetCapsFn = unsafe extern "system" fn(*mut c_void, *mut u32) -> i32;
            let ms_vtbl = *(seeking as *const *const u8);
            let get_caps: GetCapsFn = std::mem::transmute(*(ms_vtbl.add(0x18) as *const usize));
            let mut caps: u32 = 0;
            let _ = get_caps(seeking, &mut caps);
            let hr_set = ms_set_rate(seeking, 1.5);
            let mut readback = 0.0_f64;
            let hr_get = ms_get_rate(seeking, &mut readback);
            let _ = ms_set_rate(seeking, 1.0); // restore
            com_release(seeking);
            (
                format!(
                    "SetRate(1.5) hr={:#010x} readback hr={:#010x} {:.3}",
                    hr_set, hr_get, readback
                ),
                caps,
            )
        }
        None => ("no IMediaSeeking".to_string(), 0),
    };
    log_info!(
        "movie_sync[probe]: filter[{}] '{}' clsid={{{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}} caps={:#06x} | {}",
        index,
        name,
        clsid.d1,
        clsid.d2,
        clsid.d3,
        clsid.d4[0],
        clsid.d4[1],
        clsid.d4[2],
        clsid.d4[3],
        clsid.d4[4],
        clsid.d4[5],
        clsid.d4[6],
        clsid.d4[7],
        caps,
        seek_report
    );
}

/// Consume the committed movie-rate directive at graph open (FR-3, as
/// amended): install the scaled reference-clock proxy on the freshly
/// built graph (briefly stopped for the swap, re-paused after — see
/// [`rate_clock::install`]). Renderers pace every frame by timestamp vs
/// the graph clock, so a clock running at `rate × real time` plays the
/// movie at `rate` with zero cooperation from the chain — the mechanism
/// that replaced `IMediaSeeking::SetRate` after Windows cabinet tests
/// #1–#3 proved the game's WMV chain (its own custom renderer + WMVideo
/// Decoder DMO + WM ASF Reader) refuses SetRate categorically
/// (E_INVALIDARG at every rate, paused AND running, per-filter included).
///
/// Success is judged by a `GetSyncSource` POINTER READBACK — never the
/// HRESULT alone (house rule since the Wine silent-no-op SetRate). Any
/// failure degrades pre-Run via [`degrade_to_suppressed`]: the movie
/// actor never latches movie mode, so the game shows its clean static
/// background (no frozen first frame — that cosmetic only existed for
/// post-Run degradations).
///
/// Returns `true` to continue the capture; `false` when the capture was
/// degraded.
///
/// # Safety
/// Caller contract of [`capture`]: `player` is the live `DShowPlayer`
/// whose BuildGraph just returned success on this thread.
#[cfg(windows)]
unsafe fn apply_rate_directive(player: *mut c_void) -> bool {
    let Some(rate) = crate::services::song_rate::runtime::movie_rate_directive() else {
        return true;
    };
    // Defensive: a directive at identity needs no rate mechanism
    // (unreachable by construction — non-identity commits never carry a
    // 1/1 ratio).
    if (rate - 1.0).abs() <= RATE_EPSILON {
        return true;
    }
    let base = player as *const u8;
    let media_control =
        std::ptr::read_volatile(base.add(PLAYER_MEDIA_CONTROL) as *const *mut c_void);
    let installed = if media_control.is_null() {
        log_warn!("movie_sync: no IMediaControl — clock proxy uninstallable");
        false
    } else if FAULT_SET_RATE.load(Ordering::Acquire) {
        // Dev fault injection: exercise the failure rung without touching
        // the graph.
        log_warn!("movie_sync: FAULT set-rate — clock-proxy install treated as failed");
        false
    } else {
        rate_clock::install(media_control, rate)
    };
    if installed {
        log_info!(
            "movie_sync: rate sync engaged — graph clock scaled to {:.4} (proxy verified by GetSyncSource readback)",
            rate
        );
        RATE_DIAG_ARMED.store(true, Ordering::Release);
        RATE_DIAG_DEADLINE_MS.store(0, Ordering::Release);
        RATE_DIAG_ARMED_AT_MS.store(now_ms(), Ordering::Release);
        true
    } else {
        log_warn!(
            "movie_sync: rate sync unavailable (clock proxy not installed, requested {:.4}) — movie suppressed (static background), song unaffected",
            rate
        );
        degrade_to_suppressed(player);
        false
    }
}

/// The rate-sync failure rung: reduce the player to the SUPPRESS path's
/// observable state so the game shows its static no-movie background —
/// NOT a black movie plane (live-observed 2026-08-23: the earlier stop
/// command left `opened=1`, so the game kept drawing a video plane that
/// never received a frame — black all song).
///
/// Mechanism: stop the real graph directly via `IMediaControl::Stop`
/// (`player+0x50`, vtbl +0x48), then zero the `opened` byte (+0x14). With
/// `opened=0` the game's get-frame early-returns before any COM or plane
/// draw — the exact `fake_opened` shape (state 3, opened 0) the suppress
/// and fallback-faked paths already prove out. Direct COM is safe and
/// race-free HERE: everything COM on this player is serialized on this
/// thread (RE §5), and once `opened` drops the game will never dispatch
/// the command byte again, so the stock stop path is structurally
/// unavailable. Teardown (`FUN_18023b270`) releases the COM pointers
/// unconditionally at song end — nothing leaks.
///
/// # Safety
/// `player` is the live captured `DShowPlayer` on the game update thread.
#[cfg(windows)]
unsafe fn degrade_to_suppressed(player: *mut c_void) {
    let base = player as *const u8;
    let media_control =
        std::ptr::read_volatile(base.add(PLAYER_MEDIA_CONTROL) as *const *mut c_void);
    if !media_control.is_null() {
        type StopFn = unsafe extern "system" fn(*mut c_void) -> i32;
        let vtbl = *(media_control as *const *const u8);
        let stop: StopFn = std::mem::transmute(*(vtbl.add(MC_STOP) as *const usize));
        let _ = stop(media_control);
    }
    std::ptr::write_volatile((player as *mut u8).add(PLAYER_OPENED), 0u8);
    drop_capture();
}

// ── The scaled reference-clock proxy (the rate mechanism) ────────────────
/// A minimal single-instance `IReferenceClock` COM object that wraps the
/// graph's own clock and runs at `rate × real time`. Every filter paces
/// its streaming against the scaled time — the
/// renderer presents frames early/late by exactly the rate factor, which
/// IS rate-adjusted playback, with zero cooperation required from the
/// chain (`IMediaSeeking::SetRate` needed that cooperation and the game's
/// chain refuses it — Windows cabinet tests #1–#3). Installed via
/// `IMediaFilter::SetSyncSource` on the freshly built graph — in place
/// where the FGM allows it (Wine), briefly stopped/re-paused where it
/// refuses paused-graph swaps (`VFW_E_NOT_STOPPED` — Windows, test #4).
///
/// Shape:
/// - `GetTime` = `T0 + (inner − t0)·r`, with a `fetch_max` monotonic floor
///   (reference clocks must never run backwards) and basis continuity
///   `T0 = t0` at install (consumers may hold times from the old clock).
/// - `AdviseTime(base, stream, …)` forwards to the inner clock with the
///   deadline inverse-mapped (`t0 + (X − T0)/r`); `AdvisePeriodic` maps
///   the start the same way and divides the period; `Unadvise` forwards
///   the inner cookie verbatim. The inner clock does all the actual
///   waiting/signaling — this object owns no threads and no timers.
/// - Static instance, static vtable, cosmetic refcount (never freed —
///   process lifetime). The AddRef'd inner clock is deliberately held
///   until the NEXT install rather than released at capture drop: graph
///   threads may still call `GetTime` between a scene change and the
///   game's teardown, and holding our reference merely keeps the (tiny,
///   refcounted) clock object alive until then.
///
/// Threading: `GetTime`/advises arrive from arbitrary graph threads; all
/// state is atomics, written only by [`install`] on the game thread
/// BEFORE the proxy is published via `SetSyncSource`.
#[cfg(windows)]
mod rate_clock {
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicI64, AtomicPtr, AtomicU32, AtomicU64, Ordering};

    use super::{
        com_qi, com_release, map_advise_deadline, map_advise_period, scale_clock_time, ComGuid,
        IID_IMEDIA_FILTER,
    };
    use crate::log_warn;

    const IID_IUNKNOWN: ComGuid = ComGuid {
        d1: 0x0000_0000,
        d2: 0x0000,
        d3: 0x0000,
        d4: [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
    };
    const IID_IREFERENCE_CLOCK: ComGuid = ComGuid {
        d1: 0x56A8_6897,
        d2: 0x0AD4,
        d3: 0x11CE,
        d4: [0xB0, 0x3A, 0x00, 0x20, 0xAF, 0x0B, 0xA7, 0x70],
    };

    const S_OK: i32 = 0;
    const E_NOINTERFACE: i32 = 0x8000_4002u32 as i32;
    const E_POINTER: i32 = 0x8000_4003u32 as i32;
    const E_FAIL: i32 = 0x8000_4005u32 as i32;
    const E_INVALIDARG: i32 = 0x8007_0057u32 as i32;

    /// `IReferenceClock` vtable layout (IUnknown + 4 methods). Cookie and
    /// handle parameters are pointer-sized (`DWORD_PTR` / `HEVENT`).
    #[repr(C)]
    struct Vtbl {
        query_interface:
            unsafe extern "system" fn(*mut c_void, *const ComGuid, *mut *mut c_void) -> i32,
        add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
        release: unsafe extern "system" fn(*mut c_void) -> u32,
        get_time: unsafe extern "system" fn(*mut c_void, *mut i64) -> i32,
        advise_time: unsafe extern "system" fn(*mut c_void, i64, i64, usize, *mut usize) -> i32,
        advise_periodic: unsafe extern "system" fn(*mut c_void, i64, i64, usize, *mut usize) -> i32,
        unadvise: unsafe extern "system" fn(*mut c_void, usize) -> i32,
    }

    static VTBL: Vtbl = Vtbl {
        query_interface,
        add_ref,
        release,
        get_time,
        advise_time,
        advise_periodic,
        unadvise,
    };

    #[repr(C)]
    struct Instance {
        vtbl: *const Vtbl,
    }
    // SAFETY: the instance is immutable (one vtable pointer); all mutable
    // state lives in the atomics below.
    unsafe impl Sync for Instance {}
    static INSTANCE: Instance = Instance { vtbl: &VTBL };

    /// The wrapped graph clock (AddRef'd; held until the next install).
    static INNER: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
    /// `f64::to_bits(rate)`; 0 = never installed (rate reads as 1.0).
    static RATE_BITS: AtomicU64 = AtomicU64::new(0);
    /// Basis pair: proxy time T0 == inner time t0 at install (continuity).
    static BASIS_REAL: AtomicI64 = AtomicI64::new(0);
    static BASIS_OURS: AtomicI64 = AtomicI64::new(0);
    /// Monotonic floor for `GetTime` (spec: clocks never run backwards).
    static LAST_TIME: AtomicI64 = AtomicI64::new(0);
    /// Cosmetic COM refcount (static object — never freed).
    static REFS: AtomicU32 = AtomicU32::new(1);
    /// Consumption counters (reset per install): proof the graph actually
    /// consults the proxy — the discriminator for "install verified but
    /// the movie ignores the rate" (a renderer pacing by another clock).
    static GET_TIME_CALLS: AtomicU64 = AtomicU64::new(0);
    static ADVISE_CALLS: AtomicU64 = AtomicU64::new(0);

    /// `(GetTime calls, Advise/AdvisePeriodic calls)` since the last
    /// install — the post-Run consumption diagnostic's data.
    pub(super) fn consumption() -> (u64, u64) {
        (
            GET_TIME_CALLS.load(Ordering::Relaxed),
            ADVISE_CALLS.load(Ordering::Relaxed),
        )
    }

    fn instance_ptr() -> *mut c_void {
        &INSTANCE as *const Instance as *mut c_void
    }

    fn rate() -> f64 {
        match RATE_BITS.load(Ordering::Acquire) {
            0 => 1.0,
            bits => f64::from_bits(bits),
        }
    }

    // ── Inner-clock forwarding (IReferenceClock vtbl offsets) ───────────
    /// # Safety
    /// `inner` is a live `IReferenceClock*`.
    unsafe fn inner_get_time(inner: *mut c_void, out: &mut i64) -> i32 {
        type GetTimeFn = unsafe extern "system" fn(*mut c_void, *mut i64) -> i32;
        let vtbl = *(inner as *const *const u8);
        let f: GetTimeFn = std::mem::transmute(*(vtbl.add(0x18) as *const usize));
        f(inner, out)
    }

    // ── COM methods ──────────────────────────────────────────────────────
    unsafe extern "system" fn query_interface(
        this: *mut c_void,
        riid: *const ComGuid,
        out: *mut *mut c_void,
    ) -> i32 {
        if out.is_null() {
            return E_POINTER;
        }
        if riid.is_null() {
            *out = std::ptr::null_mut();
            return E_POINTER;
        }
        let iid = *riid;
        if iid == IID_IUNKNOWN || iid == IID_IREFERENCE_CLOCK {
            REFS.fetch_add(1, Ordering::AcqRel);
            *out = this;
            S_OK
        } else {
            *out = std::ptr::null_mut();
            E_NOINTERFACE
        }
    }

    unsafe extern "system" fn add_ref(_this: *mut c_void) -> u32 {
        REFS.fetch_add(1, Ordering::AcqRel) + 1
    }

    unsafe extern "system" fn release(_this: *mut c_void) -> u32 {
        // Static object: the count is diagnostic only, floored at 1.
        let previous = REFS.fetch_sub(1, Ordering::AcqRel);
        if previous <= 1 {
            REFS.store(1, Ordering::Release);
            1
        } else {
            previous - 1
        }
    }

    unsafe extern "system" fn get_time(_this: *mut c_void, out: *mut i64) -> i32 {
        GET_TIME_CALLS.fetch_add(1, Ordering::Relaxed);
        if out.is_null() {
            return E_POINTER;
        }
        let inner = INNER.load(Ordering::Acquire);
        if inner.is_null() {
            return E_FAIL;
        }
        let mut t = 0i64;
        let hr = inner_get_time(inner, &mut t);
        if hr < 0 {
            return hr;
        }
        let ours = scale_clock_time(
            t,
            BASIS_REAL.load(Ordering::Acquire),
            BASIS_OURS.load(Ordering::Acquire),
            rate(),
        );
        // Monotonic floor: fetch_max returns the PREVIOUS floor.
        let floored = LAST_TIME.fetch_max(ours, Ordering::AcqRel).max(ours);
        *out = floored;
        S_OK
    }

    unsafe extern "system" fn advise_time(
        _this: *mut c_void,
        base_time: i64,
        stream_time: i64,
        h_event: usize,
        cookie: *mut usize,
    ) -> i32 {
        ADVISE_CALLS.fetch_add(1, Ordering::Relaxed);
        if cookie.is_null() {
            return E_POINTER;
        }
        let inner = INNER.load(Ordering::Acquire);
        if inner.is_null() {
            return E_FAIL;
        }
        let deadline = base_time.saturating_add(stream_time);
        let real = map_advise_deadline(
            deadline,
            BASIS_REAL.load(Ordering::Acquire),
            BASIS_OURS.load(Ordering::Acquire),
            rate(),
        );
        type AdviseTimeFn =
            unsafe extern "system" fn(*mut c_void, i64, i64, usize, *mut usize) -> i32;
        let vtbl = *(inner as *const *const u8);
        let f: AdviseTimeFn = std::mem::transmute(*(vtbl.add(0x20) as *const usize));
        f(inner, real, 0, h_event, cookie)
    }

    unsafe extern "system" fn advise_periodic(
        _this: *mut c_void,
        start_time: i64,
        period_time: i64,
        h_semaphore: usize,
        cookie: *mut usize,
    ) -> i32 {
        ADVISE_CALLS.fetch_add(1, Ordering::Relaxed);
        if cookie.is_null() {
            return E_POINTER;
        }
        if period_time <= 0 {
            return E_INVALIDARG;
        }
        let inner = INNER.load(Ordering::Acquire);
        if inner.is_null() {
            return E_FAIL;
        }
        let r = rate();
        let real_start = map_advise_deadline(
            start_time,
            BASIS_REAL.load(Ordering::Acquire),
            BASIS_OURS.load(Ordering::Acquire),
            r,
        );
        let real_period = map_advise_period(period_time, r);
        type AdvisePeriodicFn =
            unsafe extern "system" fn(*mut c_void, i64, i64, usize, *mut usize) -> i32;
        let vtbl = *(inner as *const *const u8);
        let f: AdvisePeriodicFn = std::mem::transmute(*(vtbl.add(0x28) as *const usize));
        f(inner, real_start, real_period, h_semaphore, cookie)
    }

    unsafe extern "system" fn unadvise(_this: *mut c_void, cookie: usize) -> i32 {
        let inner = INNER.load(Ordering::Acquire);
        if inner.is_null() {
            return E_FAIL;
        }
        type UnadviseFn = unsafe extern "system" fn(*mut c_void, usize) -> i32;
        let vtbl = *(inner as *const *const u8);
        let f: UnadviseFn = std::mem::transmute(*(vtbl.add(0x30) as *const usize));
        f(inner, cookie)
    }

    // ── Installation ─────────────────────────────────────────────────────
    /// Wrap the graph's current sync source and make the proxy the graph
    /// clock via `IMediaFilter::SetSyncSource` — ADAPTIVELY: in place on
    /// the paused post-BuildGraph graph when the FGM allows it (Wine's
    /// quartz does — and its cue/stop state machine wedges on the
    /// stop/pause dance, CrossOver trial #1), escalating to
    /// stop → swap → re-pause only on `VFW_E_NOT_STOPPED` (Windows
    /// quartz's hard stopped-state rule — the escalated path is exactly
    /// what Windows cabinet test #5 validated). All pre-Run on the single
    /// COM thread (the game has not even seen the build result yet), so
    /// no dispatch races exist. Success is judged by a `GetSyncSource`
    /// POINTER READBACK, never the HRESULT alone.
    ///
    /// # Safety
    /// `media_control` is a live interface of the player's filter graph
    /// manager, on the game update thread (BuildGraph detour).
    pub(super) unsafe fn install(media_control: *mut c_void, rate_value: f64) -> bool {
        let Some(media_filter) = com_qi(media_control, &IID_IMEDIA_FILTER) else {
            log_warn!("movie_sync: IMediaFilter QI failed — clock proxy uninstallable");
            return false;
        };
        type GetSyncFn = unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32;
        type SetSyncFn = unsafe extern "system" fn(*mut c_void, *mut c_void) -> i32;
        type ControlFn = unsafe extern "system" fn(*mut c_void) -> i32;
        let vtbl = *(media_filter as *const *const u8);
        let set_sync: SetSyncFn = std::mem::transmute(*(vtbl.add(0x40) as *const usize));
        let get_sync: GetSyncFn = std::mem::transmute(*(vtbl.add(0x48) as *const usize));
        let mc_vtbl = *(media_control as *const *const u8);
        let mc_stop: ControlFn =
            std::mem::transmute(*(mc_vtbl.add(super::MC_STOP) as *const usize));
        let mc_pause: ControlFn =
            std::mem::transmute(*(mc_vtbl.add(super::MC_PAUSE) as *const usize));

        let mut inner: *mut c_void = std::ptr::null_mut();
        let hr_get = get_sync(media_filter, &mut inner);
        if inner == instance_ptr() {
            // Already ours (defensive — captures come from fresh graphs).
            com_release(inner);
            com_release(media_filter);
            RATE_BITS.store(rate_value.to_bits(), Ordering::Release);
            return true;
        }
        if hr_get < 0 || inner.is_null() {
            log_warn!(
                "movie_sync: graph has no sync source (hr={:#010x}) — clock proxy uninstallable",
                hr_get
            );
            com_release(media_filter);
            return false;
        }
        let mut t0 = 0i64;
        let hr_time = inner_get_time(inner, &mut t0);
        if hr_time < 0 {
            log_warn!(
                "movie_sync: inner clock GetTime failed (hr={:#010x}) — clock proxy uninstallable",
                hr_time
            );
            com_release(inner);
            com_release(media_filter);
            return false;
        }
        // Publish the basis BEFORE the proxy becomes reachable through
        // SetSyncSource. The previous song's inner clock (its graph is
        // long dead) is released only now — see the module docs.
        let previous = INNER.swap(inner, Ordering::AcqRel);
        RATE_BITS.store(rate_value.to_bits(), Ordering::Release);
        BASIS_REAL.store(t0, Ordering::Release);
        BASIS_OURS.store(t0, Ordering::Release);
        LAST_TIME.store(t0, Ordering::Release);
        GET_TIME_CALLS.store(0, Ordering::Relaxed);
        ADVISE_CALLS.store(0, Ordering::Relaxed);
        if !previous.is_null() && previous != inner {
            com_release(previous);
        }

        // Adaptive swap: try IN PLACE first — Wine's quartz accepts a
        // paused-graph SetSyncSource, and its async cue/stop state machine
        // WEDGED on the stop→re-pause dance (CrossOver trial #1: install
        // verified, then the graph never reached RUNNING all song). Only
        // when the FGM answers VFW_E_NOT_STOPPED (Windows quartz's hard
        // stopped-state rule, cabinet test #4) escalate to
        // stop → swap → re-pause — the exact sequence Windows test #5
        // validated. `Stop` does not reset the position (still cued at 0
        // from BuildGraph's own seek); the re-`Pause` re-cues frame 0 like
        // the stock epilogue's pause.
        const VFW_E_NOT_STOPPED: i32 = 0x8004_0224u32 as i32;
        let mut hr_set = set_sync(media_filter, instance_ptr());
        let mut escalated = false;
        if hr_set == VFW_E_NOT_STOPPED {
            escalated = true;
            let hr_stop = mc_stop(media_control);
            if hr_stop < 0 {
                log_warn!(
                    "movie_sync: pre-swap Stop failed (hr={:#010x}) — clock proxy uninstallable",
                    hr_stop
                );
                com_release(media_filter);
                return false;
            }
            hr_set = set_sync(media_filter, instance_ptr());
        }
        let mut check: *mut c_void = std::ptr::null_mut();
        let hr_check = get_sync(media_filter, &mut check);
        let verified = hr_set >= 0 && hr_check >= 0 && check == instance_ptr();
        if !check.is_null() {
            com_release(check);
        }
        if escalated {
            // Restore the paused state BuildGraph left regardless of the
            // swap outcome (the failure ladder's degrade stops the graph
            // anyway; a successful swap must hand the game back exactly
            // what it expects: a paused graph cued at frame 0). S_FALSE
            // (async cue) is success.
            let hr_pause = mc_pause(media_control);
            if hr_pause < 0 {
                // The game's own Run at song start also starts a stopped
                // graph, so this is survivable — note it and continue.
                log_warn!(
                    "movie_sync: post-swap re-Pause failed (hr={:#010x}) — the game's Run will cue the movie itself",
                    hr_pause
                );
            }
        }
        com_release(media_filter);
        if !verified {
            log_warn!(
                "movie_sync: SetSyncSource not verified (hr_set={:#010x}, hr_check={:#010x}, readback {:p} vs proxy {:p}, escalated={})",
                hr_set,
                hr_check,
                check,
                instance_ptr(),
                escalated
            );
        }
        verified
    }
}

/// Wall-clock ms (probe timing only — the frame dispatch rate is not
/// tied to rendered frames; ~2 kHz observed on CrossOver).
#[cfg(windows)]
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Per-frame driver (game thread, from `input_manager::on_frame`). The
/// dispatcher is panic-contained; this body is kept panic-free regardless.
#[cfg(windows)]
fn frame_tick() {
    let player = CAPTURED.load(Ordering::Acquire);
    if player.is_null() {
        return;
    }
    unsafe {
        drain_pending_seek(player);
        park_tick(player);
        rate_diag_tick(player);
        if PROBE_STAGE.load(Ordering::Acquire) != PROBE_STAGE_IDLE {
            probe_tick(player);
        }
    }
}

/// One-shot rate-sync consumption report (see [`RATE_DIAG_ARMED`]).
///
/// # Safety
/// `player` is the currently captured `DShowPlayer` (scene-scoped).
#[cfg(windows)]
unsafe fn rate_diag_tick(player: *mut c_void) {
    if !RATE_DIAG_ARMED.load(Ordering::Acquire) {
        return;
    }
    let base = player as *const u8;
    let state = std::ptr::read_volatile(base.add(PLAYER_STATE) as *const u32);
    let now = now_ms();
    if state != STATE_RUNNING {
        // Wedge watchdog: a verified install whose player never reaches
        // RUNNING is a wedged graph (CrossOver trial #1) — name it.
        let armed_at = RATE_DIAG_ARMED_AT_MS.load(Ordering::Acquire);
        if armed_at != 0 && now > armed_at.saturating_add(RATE_DIAG_WEDGE_MS) {
            RATE_DIAG_ARMED.store(false, Ordering::Release);
            log_warn!(
                "movie_sync: rate sync engaged but the player NEVER reached RUNNING within {}s — graph wedged after the clock swap (movie absent, song unaffected)",
                RATE_DIAG_WEDGE_MS / 1_000
            );
        }
        return;
    }
    let deadline = RATE_DIAG_DEADLINE_MS.load(Ordering::Acquire);
    if deadline == 0 {
        RATE_DIAG_DEADLINE_MS.store(now + RATE_DIAG_DELAY_MS, Ordering::Release);
        return;
    }
    if now < deadline {
        return;
    }
    RATE_DIAG_ARMED.store(false, Ordering::Release);
    let (get_time_calls, advise_calls) = rate_clock::consumption();
    log_info!(
        "movie_sync: clock proxy consumption at Run+{}s — {} GetTime call(s), {} advise(s) (near-zero with a verified install means the renderer paces by another clock)",
        RATE_DIAG_DELAY_MS / 1_000,
        get_time_calls,
        advise_calls
    );
}

/// Apply the pending seek once the player is RUNNING (see module docs for
/// why never earlier) and the live count has SETTLED per
/// [`settle_decision`]: the queued value is a trigger, the live music
/// count is the target — but the reset transaction transiently reports
/// ~0 for a tick or two after the notification (the re-anchor passes
/// through the song-start protocol's playhead-0 state before the target
/// adjust lands; cabinet test #5 caught the drain seeking every scrub to
/// 0), so a divergent live count holds until it agrees with the queued
/// trigger (window = the max approach lead) or the timeout trusts it.
///
/// # Safety
/// `player` is the currently captured `DShowPlayer` (scene-scoped; the
/// capture is dropped before the owning actor can die).
#[cfg(windows)]
unsafe fn drain_pending_seek(player: *mut c_void) {
    use crate::services::song_reset;

    let queued_ms = PENDING_SEEK_MS.load(Ordering::Acquire);
    if queued_ms == PENDING_NONE {
        return;
    }
    // A fresh event supersedes any in-flight park: un-freeze first so the
    // classification below starts from a playing movie (a paused park
    // would otherwise never return to RUNNING and deadlock this drain).
    park_cancel(player);
    let base = player as *const u8;
    let state = std::ptr::read_volatile(base.add(PLAYER_STATE) as *const u32);
    if state != STATE_RUNNING {
        return; // hold until the game's own Run
    }
    let seeking = std::ptr::read_volatile(base.add(PLAYER_MEDIA_SEEKING) as *const *mut c_void);
    if seeking.is_null() {
        PENDING_SEEK_MS.store(PENDING_NONE, Ordering::Release);
        return;
    }

    // Stability sampling: the advancing test needs two live samples at
    // least ADVANCE_MIN_GAP_MS apart (uniform transient filter).
    let now = now_ms();
    let elapsed = now.saturating_sub(PENDING_QUEUED_AT_MS.load(Ordering::Acquire));
    let live = song_reset::current_raw_music_count().map(i64::from);
    let advancing = match live {
        Some(l) => {
            let prev = SAMPLE_LIVE_MS.load(Ordering::Acquire);
            let prev_wall = SAMPLE_WALL_MS.load(Ordering::Acquire);
            let d_wall = now.saturating_sub(prev_wall) as i64;
            if prev == i64::MIN {
                SAMPLE_LIVE_MS.store(l, Ordering::Release);
                SAMPLE_WALL_MS.store(now, Ordering::Release);
                false // first sample — no baseline yet
            } else if d_wall < ADVANCE_MIN_GAP_MS {
                false // gap too small to judge; keep the baseline
            } else {
                let adv = is_advancing(l - prev, d_wall);
                SAMPLE_LIVE_MS.store(l, Ordering::Release);
                SAMPLE_WALL_MS.store(now, Ordering::Release);
                adv
            }
        }
        None => false,
    };
    // Lazy duration read (first COM touch happens here, in RUNNING state).
    if !DURATION_READ.swap(true, Ordering::AcqRel) {
        let mut duration: i64 = 0;
        if ms_get_i64(seeking, MS_GET_DURATION, &mut duration) >= 0 && duration > 0 {
            CAPTURE_DURATION_100NS.store(duration, Ordering::Release);
        }
    }
    match drain_action(queued_ms, live, advancing, elapsed) {
        DrainAction::Hold => {}
        DrainAction::Jump(t) => {
            // A jump is a park at `live + Δ`: the audio catches up to the
            // parked frame instead of the video chasing a moving target.
            PENDING_SEEK_MS.store(PENDING_NONE, Ordering::Release);
            let backward = seek_is_backward(t);
            let estimate = if backward {
                &LEAD_BACKWARD_MS
            } else {
                &LEAD_FORWARD_MS
            };
            let delta = jump_delta(estimate.load(Ordering::Acquire));
            park_begin(player, t + delta);
        }
        DrainAction::Park { run_at } => {
            PENDING_SEEK_MS.store(PENDING_NONE, Ordering::Release);
            park_begin(player, run_at);
        }
    }
}

/// Whether a seek to `target_ms` moves the movie backward, against the
/// expectation from the last park (target + wall time since).
///
/// # Safety
/// None beyond atomics; kept `unsafe`-free.
#[cfg(windows)]
fn seek_is_backward(target_ms: i64) -> bool {
    let last_target = LAST_SEEK_TARGET_MS.load(Ordering::Acquire);
    let expected_movie = if last_target == i64::MIN {
        target_ms
    } else {
        last_target + now_ms().saturating_sub(LAST_SEEK_AT_MS.load(Ordering::Acquire)) as i64
    };
    is_backward_seek(target_ms, expected_movie)
}

/// Begin a park (see [`DrainAction::Park`]): seek to the destination with
/// NO lead and NO measurement, then wait for the frame deposit to pause
/// on. Deterministic — parks never touch the jump path's estimates.
///
/// # Safety
/// Caller contract of `drain_pending_seek` (player validated RUNNING).
#[cfg(windows)]
unsafe fn park_begin(player: *mut c_void, run_at: i64) {
    let base = player as *const u8;
    let duration = CAPTURE_DURATION_100NS.load(Ordering::Acquire);
    let loop_flag = CAPTURE_LOOP.load(Ordering::Acquire);
    let target = map_position(run_at, duration, loop_flag);
    let shared = std::ptr::read_volatile(base.add(PLAYER_SHARED) as *const *const u8);
    let snapshot = if shared.is_null() {
        0
    } else {
        std::ptr::read_volatile(shared.add(SHARED_DEPOSIT_SLOT) as *const i64)
    };
    MEASURE_BACKWARD.store(seek_is_backward(run_at), Ordering::Release);
    native_seek(player, target);
    LAST_SEEK_TARGET_MS.store(run_at, Ordering::Release);
    LAST_SEEK_AT_MS.store(now_ms(), Ordering::Release);
    PARK_DEPOSIT_SNAPSHOT.store(snapshot, Ordering::Release);
    PARK_RUN_AT_MS.store(run_at, Ordering::Release);
    PARK_SEEK_AT_MS.store(now_ms(), Ordering::Release);
    PARK_STAGE.store(
        if shared.is_null() {
            PARK_NO_PAUSE
        } else {
            PARK_WAIT_DEPOSIT
        },
        Ordering::Release,
    );
    log_info!(
        "movie_sync: park -> {} ms (seek issued; pause on frame arrival, run at the crossing)",
        run_at
    );
}

/// Cancel an in-flight park (a newer event supersedes it): un-pause if
/// paused and return to idle.
///
/// # Safety
/// Caller contract of `drain_pending_seek`.
#[cfg(windows)]
unsafe fn park_cancel(player: *mut c_void) {
    let stage = PARK_STAGE.swap(PARK_IDLE, Ordering::AcqRel);
    if stage == PARK_PAUSED {
        player_command(player, PLAYER_VTBL_CMD_RUN);
        log_info!("movie_sync: park cancelled by a newer event -- movie resumed");
    }
}

/// Park state machine tick: pause on the destination frame's arrival,
/// run at the live count's crossing of the destination.
///
/// # Safety
/// Caller contract of `drain_pending_seek`.
#[cfg(windows)]
unsafe fn park_tick(player: *mut c_void) {
    use crate::services::song_reset;

    let stage = PARK_STAGE.load(Ordering::Acquire);
    if stage == PARK_IDLE {
        return;
    }
    let base = player as *const u8;
    let run_at = PARK_RUN_AT_MS.load(Ordering::Acquire);
    let live = song_reset::current_raw_music_count().map(i64::from);

    if stage == PARK_WAIT_DEPOSIT {
        let shared = std::ptr::read_volatile(base.add(PLAYER_SHARED) as *const *const u8);
        if shared.is_null() {
            PARK_STAGE.store(PARK_NO_PAUSE, Ordering::Release);
            return;
        }
        let elapsed = now_ms().saturating_sub(PARK_SEEK_AT_MS.load(Ordering::Acquire));
        let slot = std::ptr::read_volatile(shared.add(SHARED_DEPOSIT_SLOT) as *const i64);
        let snapshot = PARK_DEPOSIT_SNAPSHOT.load(Ordering::Acquire);
        let deposited = if snapshot != 0 {
            if slot == 0 {
                PARK_DEPOSIT_SNAPSHOT.store(0, Ordering::Release);
                false
            } else {
                slot != snapshot
            }
        } else {
            slot != 0
        };
        if deposited {
            // The seek→first-deposit gap IS this machine's restart
            // latency: feed the per-direction estimate that sizes the
            // next jump-park's Δ.
            let backward = MEASURE_BACKWARD.load(Ordering::Acquire);
            let estimate = if backward {
                &LEAD_BACKWARD_MS
            } else {
                &LEAD_FORWARD_MS
            };
            let previous = estimate.load(Ordering::Acquire);
            let next = lead_update(previous, elapsed as i64);
            estimate.store(next, Ordering::Release);
            log_info!(
                "movie_sync: restart latency {} ms ({}, estimate {} -> {} ms)",
                elapsed,
                if backward { "bwd" } else { "fwd" },
                previous,
                next
            );
            // The crossing may already have passed (short approach, slow
            // restart, or a near-zero Δ on a fast machine) — then the
            // movie is where it should be: just let it play.
            if live.map(|l| l >= run_at).unwrap_or(false) {
                PARK_STAGE.store(PARK_IDLE, Ordering::Release);
                return;
            }
            player_command(player, PLAYER_VTBL_CMD_PAUSE);
            PARK_STAGE.store(PARK_PAUSED, Ordering::Release);
            log_info!(
                "movie_sync: parked at {} ms (paused, awaiting the crossing)",
                run_at
            );
        } else if elapsed > MEASURE_TIMEOUT_MS {
            PARK_STAGE.store(PARK_NO_PAUSE, Ordering::Release);
        }
        return;
    }
    // PARK_PAUSED / PARK_NO_PAUSE: run (or finish) at the crossing.
    // No pre-issue: the dispatch chain costs ~1–3 frames of lateness,
    // which live testing showed is imperceptible — while the removed
    // startup-lead estimator's failure mode (early video from stall-
    // polluted estimates) was the dominant desync source (tests #12–#13).
    let crossed = live.map(|l| l >= run_at).unwrap_or(false);
    if !crossed {
        return;
    }
    if stage == PARK_PAUSED {
        player_command(player, PLAYER_VTBL_CMD_RUN);
        log_info!(
            "movie_sync: crossing reached -- parked movie running ({} ms)",
            run_at
        );
    }
    PARK_STAGE.store(PARK_IDLE, Ordering::Release);
}

/// Probe stage machine (dev only; see module docs).
///
/// # Safety
/// Same contract as `drain_pending_seek`.
#[cfg(windows)]
unsafe fn probe_tick(player: *mut c_void) {
    let base = player as *const u8;
    let state = std::ptr::read_volatile(base.add(PLAYER_STATE) as *const u32);
    let stage = PROBE_STAGE.load(Ordering::Acquire);

    if stage == PROBE_STAGE_WAIT_RUN {
        if state == STATE_RUNNING {
            log_info!("movie_sync[probe]: game ran the movie (state 3->2) -- wall timer starts");
            PROBE_RUN_STARTED_MS.store(now_ms(), Ordering::Release);
            PROBE_STAGE.store(PROBE_STAGE_RUNNING, Ordering::Release);
        }
        return;
    }
    // PROBE_STAGE_RUNNING
    if state != STATE_RUNNING {
        return; // paused/stopped mid-probe (song end race) — freeze until drop
    }
    let seeking = std::ptr::read_volatile(base.add(PLAYER_MEDIA_SEEKING) as *const *mut c_void);
    if seeking.is_null() {
        PROBE_STAGE.store(PROBE_STAGE_IDLE, Ordering::Release);
        return;
    }
    let elapsed = now_ms().saturating_sub(PROBE_RUN_STARTED_MS.load(Ordering::Acquire));
    let fired = PROBE_FIRED.load(Ordering::Acquire);

    if elapsed >= PROBE_RATE_AT_MS && fired & 1 == 0 {
        PROBE_FIRED.store(fired | 1, Ordering::Release);
        let mut duration: i64 = 0;
        let hr_dur = ms_get_i64(seeking, MS_GET_DURATION, &mut duration);
        let mut pos: i64 = -1;
        let hr_pos = ms_get_i64(seeking, MS_GET_CURRENT_POSITION, &mut pos);
        log_info!(
            "movie_sync[probe]: +{} ms running | duration hr={:#010x} {} ms | \
             live pos hr={:#010x} {} ms (untouched graph -- genuine clock position)",
            elapsed,
            hr_dur,
            duration / 10_000,
            hr_pos,
            pos / 10_000
        );
        let hr_set = ms_set_rate(seeking, 1.5);
        let mut rate_back: f64 = 0.0;
        let hr_back = ms_get_rate(seeking, &mut rate_back);
        log_info!(
            "movie_sync[probe]: SetRate(1.5) hr={:#010x} | readback hr={:#010x} {:.3} -- \
             if applied, the video visibly runs 50% fast from here",
            hr_set,
            hr_back,
            rate_back
        );
    } else if elapsed >= PROBE_SEEK_AT_MS && fired & 2 == 0 {
        PROBE_FIRED.store(fired | 2, Ordering::Release);
        let mut pre: i64 = -1;
        let hr_pre = ms_get_i64(seeking, MS_GET_CURRENT_POSITION, &mut pre);
        native_seek(player, 60_000 * 10_000);
        let mut post: i64 = -1;
        let hr_post = ms_get_i64(seeking, MS_GET_CURRENT_POSITION, &mut post);
        log_info!(
            "movie_sync[probe]: RUNNING-STATE seek to 60000 ms | pre hr={:#010x} {} ms -> \
             post hr={:#010x} {} ms -- EXPECT a visible content jump right now",
            hr_pre,
            pre / 10_000,
            hr_post,
            post / 10_000
        );
        PROBE_STAGE.store(PROBE_STAGE_IDLE, Ordering::Release);
    }
}

// ── COM helpers (game thread only; callers hold a validated player) ─────

/// # Safety
/// `seeking` is a live `IMediaSeeking*`; `vtbl_offset` is one of the
/// `MS_GET_*` i64-out slots.
#[cfg(windows)]
unsafe fn ms_get_i64(seeking: *mut c_void, vtbl_offset: usize, out: &mut i64) -> i32 {
    type GetI64Fn = unsafe extern "system" fn(*mut c_void, *mut i64) -> i32;
    let vtbl = *(seeking as *const *const u8);
    let f: GetI64Fn = std::mem::transmute(*(vtbl.add(vtbl_offset) as *const usize));
    f(seeking, out)
}

/// # Safety
/// `seeking` is a live `IMediaSeeking*`.
#[cfg(windows)]
unsafe fn ms_set_rate(seeking: *mut c_void, rate: f64) -> i32 {
    type SetRateFn = unsafe extern "system" fn(*mut c_void, f64) -> i32;
    let vtbl = *(seeking as *const *const u8);
    let f: SetRateFn = std::mem::transmute(*(vtbl.add(MS_SET_RATE) as *const usize));
    f(seeking, rate)
}

/// # Safety
/// `seeking` is a live `IMediaSeeking*`.
#[cfg(windows)]
unsafe fn ms_get_rate(seeking: *mut c_void, out: &mut f64) -> i32 {
    type GetF64Fn = unsafe extern "system" fn(*mut c_void, *mut f64) -> i32;
    let vtbl = *(seeking as *const *const u8);
    let f: GetF64Fn = std::mem::transmute(*(vtbl.add(MS_GET_RATE) as *const usize));
    f(seeking, out)
}

/// Issue a player command through the game's own tiny vtable setters
/// (`PLAYER_VTBL_CMD_PAUSE` / `PLAYER_VTBL_CMD_RUN` — they only write the
/// `+0x0C` command byte; get-frame dispatches the actual IMediaControl
/// call on the game's next frame).
///
/// # Safety
/// `player` is a live `DShowPlayer`.
#[cfg(windows)]
unsafe fn player_command(player: *mut c_void, vtbl_slot: usize) {
    type CommandFn = unsafe extern "system" fn(*mut c_void);
    let vtbl = *(player as *const *const u8);
    let f: CommandFn = std::mem::transmute(*(vtbl.add(vtbl_slot) as *const usize));
    f(player);
}

/// The game's own null-guarded absolute seek (player vtbl +0x58).
///
/// # Safety
/// `player` is a live `DShowPlayer`.
#[cfg(windows)]
unsafe fn native_seek(player: *mut c_void, position_100ns: i64) {
    type NativeSeekFn = unsafe extern "system" fn(*mut c_void, i64);
    let vtbl = *(player as *const *const u8);
    let f: NativeSeekFn = std::mem::transmute(*(vtbl.add(PLAYER_VTBL_NATIVE_SEEK) as *const usize));
    f(player, position_100ns);
}

#[cfg(test)]
mod tests {
    use super::{gate_ok, map_position};

    #[test]
    fn gate_requires_opened_and_live_state_and_interface() {
        // The canonical good captures: opened, running or opened-not-running.
        assert!(gate_ok(2, 1, false));
        assert!(gate_ok(3, 1, false));
        // Faked/suppressed epilogue: state written 3 but opened stays 0.
        assert!(!gate_ok(3, 0, false));
        // Stopped/closed graph (get-frame's stop path zeroes the state).
        assert!(!gate_ok(0, 1, false));
        // Needed COM interface is null (non-seekable graph).
        assert!(!gate_ok(2, 1, true));
        // Never-opened object.
        assert!(!gate_ok(0, 0, true));
    }

    const SEC: i64 = 10_000_000; // 1 s in 100 ns units

    #[test]
    fn map_position_clamps_non_looping_movies_short_of_the_end() {
        let dur = 131 * SEC;
        let end_margin = dur - super::CLAMP_END_MARGIN_100NS; // 130.5 s
        assert_eq!(map_position(30_000, dur, false), 30 * SEC);
        // At/past the end: never the exact end (EC_COMPLETE stop trap).
        assert_eq!(map_position(131_000, dur, false), end_margin);
        assert_eq!(map_position(200_000, dur, false), end_margin);
        assert_eq!(map_position(0, dur, false), 0);
        // Degenerate: movie shorter than the margin — floor at 0.
        assert_eq!(map_position(10_000, 3_000_000, false), 0);
    }

    #[test]
    fn map_position_wraps_looping_movies_modulo_duration() {
        let dur = 60 * SEC;
        assert_eq!(map_position(30_000, dur, true), 30 * SEC);
        assert_eq!(map_position(60_000, dur, true), 0);
        assert_eq!(map_position(90_000, dur, true), 30 * SEC);
        assert_eq!(map_position(150_000, dur, true), 30 * SEC);
    }

    #[test]
    fn map_position_negative_times_clamp_to_zero() {
        // Delayed-restart pre-anchor countdown: hold the first frame.
        assert_eq!(map_position(-3_000, 60 * SEC, false), 0);
        assert_eq!(map_position(-3_000, 60 * SEC, true), 0);
        assert_eq!(map_position(-3_000, 0, false), 0);
    }

    #[test]
    fn map_position_unknown_duration_passes_through() {
        assert_eq!(map_position(45_000, 0, false), 45 * SEC);
        assert_eq!(map_position(45_000, 0, true), 45 * SEC);
    }

    #[test]
    fn map_position_saturates_extreme_inputs() {
        // No overflow panic even at absurd content times.
        let _ = map_position(i64::MAX / 2, 0, false);
        let _ = map_position(i64::MAX, 60 * SEC, true);
    }

    use super::{drain_action, is_advancing, DrainAction};

    #[test]
    fn advancing_requires_wall_rate_progression() {
        // Settled count: ~1 ms per wall ms.
        assert!(is_advancing(52, 50));
        // Transaction transient: frozen or jumping counts.
        assert!(!is_advancing(0, 50));
        assert!(!is_advancing(5_000, 50));
        // Gap too small to judge.
        assert!(!is_advancing(10, 10));
    }

    #[test]
    fn drain_scrub_jumps_to_the_live_count() {
        // Scrub landed, count advancing at wall rate near the trigger.
        assert_eq!(
            drain_action(44_670, Some(44_672), true, 30),
            DrainAction::Jump(44_672)
        );
    }

    #[test]
    fn drain_holds_transients_until_advancing() {
        // The reset transaction's ~0 transient (cabinet test #5) and the
        // stale pre-scrub count during mashes (cabinet test #6): both
        // fail the advancing test now — one uniform filter.
        assert_eq!(drain_action(44_670, Some(0), false, 1), DrainAction::Hold);
        assert_eq!(
            drain_action(49_718, Some(44_718), false, 5),
            DrainAction::Hold
        );
        // A stale-but-advancing count one full scrub step away is OUTSIDE
        // the settle window — held (it is a mash artifact, not an
        // approach: approaches diverge by at most the 2.5 s lead).
        assert_eq!(
            drain_action(64_030, Some(59_032), true, 30),
            DrainAction::Hold
        );
    }

    #[test]
    fn drain_parks_approaches_at_the_trigger() {
        // Loop wrap / SONG START: count advancing 2.5 s below the trigger.
        assert_eq!(
            drain_action(114_997, Some(112_500), true, 40),
            DrainAction::Park { run_at: 114_997 }
        );
        // Quick/delayed restart: negative count parks at the trigger
        // immediately (no advancing requirement — the sign is decisive).
        assert_eq!(
            drain_action(0, Some(-2_400), false, 0),
            DrainAction::Park { run_at: 0 }
        );
        assert_eq!(
            drain_action(0, Some(-9_500), true, 8_000),
            DrainAction::Park { run_at: 0 }
        );
    }

    #[test]
    fn drain_scrub_within_approach_margin_still_jumps() {
        // A jump trigger sits at/behind the live count (FF landing plays
        // past the trigger while we sample) — never park those.
        assert_eq!(
            drain_action(44_670, Some(44_800), true, 60),
            DrainAction::Jump(44_800)
        );
    }

    #[test]
    fn drain_timeout_trusts_live_then_queued() {
        // Divergent but real count past the timeout.
        assert_eq!(
            drain_action(44_670, Some(90_000), true, 500),
            DrainAction::Jump(90_000)
        );
        // Non-advancing count past the timeout: still the live value.
        assert_eq!(
            drain_action(44_670, Some(90_000), false, 500),
            DrainAction::Jump(90_000)
        );
        // Unreadable count past the timeout: the queued value.
        assert_eq!(drain_action(44_670, None, false, 10), DrainAction::Hold);
        assert_eq!(
            drain_action(44_670, None, false, 500),
            DrainAction::Jump(44_670)
        );
    }

    use super::{jump_delta, lead_update, LEAD_UNMEASURED};

    #[test]
    fn lead_update_adopts_the_first_measurement() {
        assert_eq!(lead_update(LEAD_UNMEASURED, 320), 320);
    }

    #[test]
    fn lead_update_averages_subsequent_measurements() {
        assert_eq!(lead_update(320, 280), 300);
        // Converges toward a changed latency instead of pinning.
        assert_eq!(lead_update(300, 100), 200);
    }

    #[test]
    fn lead_update_clamps_to_sane_bounds() {
        assert_eq!(lead_update(LEAD_UNMEASURED, 50_000), 2_000);
        assert_eq!(lead_update(LEAD_UNMEASURED, -20), 0);
    }

    #[test]
    fn jump_delta_gives_variance_headroom_and_self_neutralizes() {
        // Unmeasured: the one-time default headroom.
        assert_eq!(jump_delta(LEAD_UNMEASURED), 500);
        // Measured: 2x the estimate (observed ~2x per-seek variance).
        assert_eq!(jump_delta(480), 960);
        // Instant-seek machines (real Windows): Δ -> 0, the park degrades
        // to a plain seek (crossing passes before the frame arrives).
        assert_eq!(jump_delta(0), 0);
        // Clamped.
        assert_eq!(jump_delta(1_500), 2_000);
    }

    use super::{map_advise_deadline, map_advise_period, scale_clock_time};

    const TICK_SEC: i64 = 10_000_000; // 1 s in 100 ns units

    #[test]
    fn clock_scaling_is_identity_at_rate_one() {
        let t0 = 555 * TICK_SEC;
        for elapsed in [0, 1, TICK_SEC, 141 * TICK_SEC] {
            assert_eq!(scale_clock_time(t0 + elapsed, t0, t0, 1.0), t0 + elapsed);
            assert_eq!(map_advise_deadline(t0 + elapsed, t0, t0, 1.0), t0 + elapsed);
        }
        assert_eq!(map_advise_period(400_000, 1.0), 400_000);
    }

    #[test]
    fn clock_scaling_runs_fast_and_slow_about_the_basis() {
        let t0 = 100 * TICK_SEC;
        // 1.75x: 10 real seconds -> 17.5 proxy seconds.
        assert_eq!(
            scale_clock_time(t0 + 10 * TICK_SEC, t0, t0, 1.75),
            t0 + 175 * TICK_SEC / 10
        );
        // 0.5x: 10 real seconds -> 5 proxy seconds.
        assert_eq!(
            scale_clock_time(t0 + 10 * TICK_SEC, t0, t0, 0.5),
            t0 + 5 * TICK_SEC
        );
        // Pre-basis instants map symmetrically (no underflow surprises).
        assert_eq!(
            scale_clock_time(t0 - 2 * TICK_SEC, t0, t0, 0.5),
            t0 - TICK_SEC
        );
    }

    #[test]
    fn advise_deadline_inverse_maps_the_scaling() {
        let t0 = 100 * TICK_SEC;
        // The proxy reaches t0+17.5s at real t0+10s under 1.75x.
        assert_eq!(
            map_advise_deadline(t0 + 175 * TICK_SEC / 10, t0, t0, 1.75),
            t0 + 10 * TICK_SEC
        );
        // Roundtrip within rounding: real -> proxy -> real.
        for rate in [0.25, 0.75, 1.2, 1.75] {
            for elapsed in [0, 137, TICK_SEC, 141 * TICK_SEC] {
                let ours = scale_clock_time(t0 + elapsed, t0, t0, rate);
                let back = map_advise_deadline(ours, t0, t0, rate);
                assert!(
                    (back - (t0 + elapsed)).abs() <= 8,
                    "roundtrip rate {rate} elapsed {elapsed}: {back}"
                );
            }
        }
        // Past deadlines map to past real instants (inner signals at once).
        assert!(map_advise_deadline(t0 - TICK_SEC, t0, t0, 1.75) < t0);
    }

    #[test]
    fn advise_period_scales_and_never_reaches_zero() {
        // A 33.3 ms frame cadence at 1.75x waits a shorter real period.
        assert_eq!(map_advise_period(333_333, 1.75), 190_476);
        // Slow rates wait longer.
        assert_eq!(map_advise_period(400_000, 0.5), 800_000);
        // Degenerate tiny periods floor at one tick instead of zero.
        assert_eq!(map_advise_period(1, 1.75), 1);
        // Defensive rate guard: nonpositive rates degrade to identity.
        assert_eq!(map_advise_period(400_000, 0.0), 400_000);
        assert_eq!(scale_clock_time(7, 0, 0, -3.0), 7);
    }
}
