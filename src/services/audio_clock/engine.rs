//! Render-thread observers of the XACT mixer (design §3 hook inventory).
//!
//! The detours themselves are owned by `audio_sync_diag::xact` (one detour per
//! target): its cursor hook (`0x435A50`, once per mix pass), its streaming
//! submission hook (`0x25ED0`) and the two hooks it installs on our behalf
//! (`0x43CAC0` source-node produce, `0x419DB0` in-memory submission) call the
//! entry points below. Everything here is lock-free and allocation-free after
//! [`install`]: the render-thread paths touch atomics and the single-owner
//! [`Fit`] only; publications go through [`SeqPub`].
//!
//! Frame-domain epochs: the DirectSound accumulators (`W`/`Wc`/`P`) restart
//! from zero on a backend Stop/recreate, and the ring's underrun clamp breaks
//! the steady-state relation the constant `C` relies on. Any such reset bumps
//! [`EPOCH`]; an onset whose epoch differs from the line's is not comparable
//! and the game side passes through.

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, AtomicUsize, Ordering};

use super::fit::{Fit, FitConfig, Line, Push, Reset, Sample};
use super::onset::Onset;
use super::SeqPub;
use crate::core::memory;
use crate::log_info;

/// DirectSound backend object layout (`docs/audio_clock_research.md` §2.4).
const DS_FORMAT_PTR: usize = 0x80;
const DS_WRITTEN_BYTES: usize = 0xC0;
const DS_WRITE_CURSOR_BYTES: usize = 0xC8;
const DS_PLAY_CURSOR_BYTES: usize = 0xD0;
const DS_OBJECT_SIZE: usize = 0xE0;
/// `WAVEFORMATEX`: `nSamplesPerSec` at +4, `nBlockAlign` at +0xC.
const FMT_HZ: usize = 4;
const FMT_BLOCK_ALIGN: usize = 0xC;
const FMT_SIZE: usize = 0x10;
/// Source node: cumulative source bytes consumed since Start (§2.5).
const NODE_CONSUMED_BYTES: usize = 0x5F8;
/// Extent of the source node we probe once at identity time (the object is
/// 0x658 bytes; `+0x5F8` is the last field we read).
const NODE_PROBE_LEN: usize = 0x600;
/// Reject absurd formats (protects every division below).
const MAX_HZ: u32 = 384_000;
/// Largest inter-pass gap the fit accepts before treating the render thread
/// as stalled (design §4: stock until passes resume).
const MAX_GAP_MS: u32 = 200;
/// Registered aux (assist-tick) sound banks.
const AUX_SLOTS: usize = 4;
/// Largest plausible `F0 − W(newest cursor sample)`: one mix pass at any
/// supported rate (441 frames at 44.1 kHz; 960 at 96 kHz). See `produce_post_inner`.
const MAX_PRODUCE_LEAD_FRAMES: i64 = 1000;

static INSTALLED: AtomicBool = AtomicBool::new(false);
static FREQUENCY: AtomicI64 = AtomicI64::new(0);

/// Validated DS backend pointer (0 = none) and its format.
static BACKEND: AtomicUsize = AtomicUsize::new(0);
static BACKEND_HZ: AtomicU32 = AtomicU32::new(0);
static BACKEND_BLOCK_ALIGN: AtomicU32 = AtomicU32::new(0);
static BACKEND_REJECTED: AtomicUsize = AtomicUsize::new(0);

/// Frame-domain epoch (see the module docs).
static EPOCH: AtomicU64 = AtomicU64::new(1);

/// Render-thread-owned fit. Allocated at [`install`] (factory-return window,
/// before the render thread exists); touched afterwards ONLY inside the
/// cursor hook.
static mut FIT: Option<Fit> = None;

/// Published line: [n, t_ref, p_ref, slope, resid_sd, mean_lead, mean_margin,
/// pass_frames, flags(bit0 ready, bit1 valid), epoch].
static LINE: SeqPub<10> = SeqPub::new();
/// Newest pass sample: [t, p, wc, w, epoch, valid].
static LAST_SAMPLE: SeqPub<6> = SeqPub::new();

/// Pending voice identities (node pointers; 0 = none) + their generation and
/// the owning cue (for stop matching).
static PENDING_SONG_NODE: AtomicUsize = AtomicUsize::new(0);
static PENDING_SONG_GEN: AtomicU64 = AtomicU64::new(0);
static PENDING_SONG_CUE: AtomicUsize = AtomicUsize::new(0);
static PENDING_AUX_NODE: AtomicUsize = AtomicUsize::new(0);
static PENDING_AUX_GEN: AtomicU64 = AtomicU64::new(0);
static PENDING_AUX_CUE: AtomicUsize = AtomicUsize::new(0);
/// Published onsets: [generation, epoch, f0, t_k, p_k, wc_k, w_k, hz, valid].
static SONG_ONSET: SeqPub<9> = SeqPub::new();
static AUX_ONSET: SeqPub<9> = SeqPub::new();
/// Cue of the voice whose onset is published (0 = none).
static SONG_ONSET_CUE: AtomicUsize = AtomicUsize::new(0);
static AUX_ONSET_CUE: AtomicUsize = AtomicUsize::new(0);
/// The node behind the published aux onset (for `consumed_bytes`).
static AUX_ONSET_NODE: AtomicUsize = AtomicUsize::new(0);
static VOICE_GEN: AtomicU64 = AtomicU64::new(0);
static AUX_BANKS: [AtomicUsize; AUX_SLOTS] = [const { AtomicUsize::new(0) }; AUX_SLOTS];

/// Diagnostics counters (read by the mod's INFO lines).
static PASSES: AtomicU64 = AtomicU64::new(0);
static RESETS: AtomicU64 = AtomicU64::new(0);
static LAST_RESET: AtomicU32 = AtomicU32::new(0);
static CURSOR_FAILURES: AtomicU64 = AtomicU64::new(0);
/// Set by the render thread when an onset needs an INFO line; drained by the
/// game thread (`game.rs`) — never log on the render thread.
static ONSET_PENDING_LOG: AtomicU32 = AtomicU32::new(0);
static W_MISMATCH_WARNED: AtomicBool = AtomicBool::new(false);

/// Which voice a pending node belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Voice {
    Song,
    Aux,
}

/// Pre-original snapshot the produce hook carries across the original call.
#[derive(Clone, Copy, Debug)]
pub struct ProducePre {
    voice: Voice,
    generation: u64,
    consumed_before: u64,
    written_before_bytes: u64,
}

/// Install-time setup (called by `audio_sync_diag::xact::install_engine` in
/// the factory-return window when the mod is enabled in config).
pub fn install(frequency: i64) -> bool {
    if frequency <= 0 {
        return false;
    }
    FREQUENCY.store(frequency, Ordering::Release);
    let cfg = super::config();
    let fit_cfg = FitConfig::from_seconds(frequency, cfg.window_seconds, MAX_GAP_MS);
    // SAFETY: single-threaded boot window; the render thread does not exist
    // until XACT Initialize, which follows the factory return we are in.
    unsafe {
        *std::ptr::addr_of_mut!(FIT) = Some(Fit::new(fit_cfg));
    }
    INSTALLED.store(true, Ordering::Release);
    true
}

#[must_use]
pub fn installed() -> bool {
    INSTALLED.load(Ordering::Acquire)
}

#[must_use]
pub fn frequency() -> i64 {
    FREQUENCY.load(Ordering::Acquire)
}

#[must_use]
pub fn epoch() -> u64 {
    EPOCH.load(Ordering::Acquire)
}

/// Whether the render-thread hooks should do work this call.
#[inline]
fn active() -> bool {
    INSTALLED.load(Ordering::Relaxed)
}

fn read<T: Copy>(base: usize, offset: usize) -> Option<T> {
    if base == 0 {
        return None;
    }
    let p = base.checked_add(offset)? as *const u8;
    if !memory::is_readable(p, std::mem::size_of::<T>()) {
        return None;
    }
    // SAFETY: probed readable; engine-owned objects outlive the process.
    Some(unsafe { p.cast::<T>().read_unaligned() })
}

/// Plain read of a field inside an object whose whole extent was ALREADY
/// probed readable (the validated DS backend, the probed source node). No
/// VirtualQuery — this is what the per-pass render-thread paths use.
///
/// # Safety
/// `base + offset + size_of::<T>()` must lie inside a region a prior
/// `memory::is_readable` probe on this object covered.
#[inline]
unsafe fn read_probed<T: Copy>(base: usize, offset: usize) -> T {
    ((base + offset) as *const T).read_unaligned()
}

/// Validate (and cache) the DS backend + format. Probing costs a VirtualQuery,
/// so it runs once per distinct pointer; a rejected pointer is remembered too.
fn backend_format(backend: usize) -> Option<(u32, u32)> {
    if backend == 0 {
        return None;
    }
    if BACKEND.load(Ordering::Relaxed) == backend {
        return Some((
            BACKEND_HZ.load(Ordering::Relaxed),
            BACKEND_BLOCK_ALIGN.load(Ordering::Relaxed),
        ));
    }
    if BACKEND_REJECTED.load(Ordering::Relaxed) == backend {
        return None;
    }
    let ok = memory::is_readable(backend as *const u8, DS_OBJECT_SIZE)
        .then(|| read::<usize>(backend, DS_FORMAT_PTR))
        .flatten()
        .filter(|fmt| *fmt != 0 && memory::is_readable(*fmt as *const u8, FMT_SIZE))
        .and_then(|fmt| {
            let hz = read::<u32>(fmt, FMT_HZ)?;
            let ba = u32::from(read::<u16>(fmt, FMT_BLOCK_ALIGN)?);
            ((1..=MAX_HZ).contains(&hz) && (1..=64).contains(&ba)).then_some((hz, ba))
        });
    match ok {
        Some((hz, ba)) => {
            BACKEND_HZ.store(hz, Ordering::Relaxed);
            BACKEND_BLOCK_ALIGN.store(ba, Ordering::Relaxed);
            BACKEND.store(backend, Ordering::Release);
            Some((hz, ba))
        }
        None => {
            BACKEND_REJECTED.store(backend, Ordering::Relaxed);
            None
        }
    }
}

/// Cursor-read dispatcher entry (render thread, once per mix pass, AFTER the
/// original `0x435A50` returned). `result` is the original's return (lead in
/// bytes; 0 on the underrun clamp; −1 on failure).
pub fn on_pass(backend: usize, result: i32, t_pre: i64, t_post: i64) {
    if !active() {
        return;
    }
    let _ = std::panic::catch_unwind(|| pass_inner(backend, result, t_pre, t_post));
}

fn pass_inner(backend: usize, result: i32, t_pre: i64, t_post: i64) {
    PASSES.fetch_add(1, Ordering::Relaxed);
    if result < 0 || t_pre < 0 || t_post < t_pre {
        CURSOR_FAILURES.fetch_add(1, Ordering::Relaxed);
        return;
    }
    let previous_backend = BACKEND.load(Ordering::Relaxed);
    let Some((hz, ba)) = backend_format(backend) else {
        return;
    };
    // SAFETY: render thread — the single owner of FIT after install.
    let Some(fit) = (unsafe { (*std::ptr::addr_of_mut!(FIT)).as_mut() }) else {
        return;
    };
    if previous_backend != 0 && previous_backend != backend {
        note_reset(fit.reset(Reset::FormatChanged), true);
    }
    // SAFETY: `backend_format` probed the whole 0xE0-byte backend object.
    let (written, write_cursor, play) = unsafe {
        (
            read_probed::<u64>(backend, DS_WRITTEN_BYTES),
            read_probed::<u64>(backend, DS_WRITE_CURSOR_BYTES),
            read_probed::<u64>(backend, DS_PLAY_CURSOR_BYTES),
        )
    };
    let ba64 = u64::from(ba);
    let sample = Sample {
        t: t_pre + (t_post - t_pre) / 2,
        p: (play / ba64) as i64,
        wc: (write_cursor / ba64) as i64,
        w: (written / ba64) as i64,
    };
    // The engine's clamp path returns 0 after forcing Wc := W.
    let underrun = result == 0 && write_cursor >= written;
    match fit.push(sample, underrun) {
        Push::Accepted => {}
        Push::Reset(reason) => note_reset(
            Push::Reset(reason),
            matches!(
                reason,
                Reset::PlayDecreased | Reset::Underrun | Reset::FormatChanged
            ),
        ),
    }
    let epoch = EPOCH.load(Ordering::Relaxed);
    LAST_SAMPLE.write(&[
        sample.t as u64,
        sample.p as u64,
        sample.wc as u64,
        sample.w as u64,
        epoch,
        1,
    ]);
    // The fit always runs (its window statistics supply `C` and readiness in
    // BOTH modes); `raw` only swaps the phase/slope for the newest sample's
    // nominal-rate extrapolation.
    let fitted = fit.line();
    let line = match (super::config().mode, fitted) {
        (super::Mode::Raw, Some(fitted)) => {
            let raw = Line::raw(sample, hz, FREQUENCY.load(Ordering::Relaxed));
            Some(Line {
                t_ref: raw.t_ref,
                p_ref: raw.p_ref,
                slope: raw.slope,
                resid_sd: 0.0,
                ready: fitted.ready && raw.ready,
                ..fitted
            })
        }
        (_, fitted) => fitted,
    };
    publish_line(line.as_ref(), epoch);
}

fn note_reset(push: Push, new_epoch: bool) {
    if let Push::Reset(reason) = push {
        RESETS.fetch_add(1, Ordering::Relaxed);
        LAST_RESET.store(reason as u32, Ordering::Relaxed);
        if new_epoch {
            EPOCH.fetch_add(1, Ordering::AcqRel);
            // Onsets carry frame values of the old accumulator epoch.
            invalidate_onset(Voice::Song);
            invalidate_onset(Voice::Aux);
        }
    }
}

fn publish_line(line: Option<&Line>, epoch: u64) {
    match line {
        Some(l) => LINE.write(&[
            l.n as u64,
            l.t_ref as u64,
            l.p_ref.to_bits(),
            l.slope.to_bits(),
            l.resid_sd.to_bits(),
            l.mean_lead.to_bits(),
            l.mean_margin.to_bits(),
            l.pass_frames.to_bits(),
            u64::from(l.ready) | 2,
            epoch,
        ]),
        None => LINE.write(&[0; 10]),
    }
}

/// The published line (`None` until at least two passes) with its epoch.
#[must_use]
pub fn line() -> Option<(Line, u64)> {
    let w = LINE.read();
    if w[8] & 2 == 0 {
        return None;
    }
    Some((
        Line {
            n: w[0] as usize,
            t_ref: w[1] as i64,
            p_ref: f64::from_bits(w[2]),
            slope: f64::from_bits(w[3]),
            resid_sd: f64::from_bits(w[4]),
            mean_lead: f64::from_bits(w[5]),
            mean_margin: f64::from_bits(w[6]),
            pass_frames: f64::from_bits(w[7]),
            ready: w[8] & 1 != 0,
        },
        w[9],
    ))
}

/// Output format of the validated backend (Hz, block align), if any.
#[must_use]
pub fn output_format() -> Option<(u32, u32)> {
    let hz = BACKEND_HZ.load(Ordering::Acquire);
    let ba = BACKEND_BLOCK_ALIGN.load(Ordering::Acquire);
    (hz != 0 && ba != 0 && BACKEND.load(Ordering::Acquire) != 0).then_some((hz, ba))
}

/// Newest pass sample `(t, p, wc, w)` in frames, if any.
#[must_use]
pub fn last_sample() -> Option<(Sample, u64)> {
    let w = LAST_SAMPLE.read();
    (w[5] != 0).then(|| {
        (
            Sample {
                t: w[0] as i64,
                p: w[1] as i64,
                wc: w[2] as i64,
                w: w[3] as i64,
            },
            w[4],
        )
    })
}

/// Diagnostics snapshot: (passes, resets, last reset code, cursor failures).
#[must_use]
pub fn counters() -> (u64, u64, u32, u64) {
    (
        PASSES.load(Ordering::Relaxed),
        RESETS.load(Ordering::Relaxed),
        LAST_RESET.load(Ordering::Relaxed),
        CURSOR_FAILURES.load(Ordering::Relaxed),
    )
}

fn pending(
    voice: Voice,
) -> (
    &'static AtomicUsize,
    &'static AtomicU64,
    &'static AtomicUsize,
) {
    match voice {
        Voice::Song => (&PENDING_SONG_NODE, &PENDING_SONG_GEN, &PENDING_SONG_CUE),
        Voice::Aux => (&PENDING_AUX_NODE, &PENDING_AUX_GEN, &PENDING_AUX_CUE),
    }
}

fn onset_pub(voice: Voice) -> (&'static SeqPub<9>, &'static AtomicUsize) {
    match voice {
        Voice::Song => (&SONG_ONSET, &SONG_ONSET_CUE),
        Voice::Aux => (&AUX_ONSET, &AUX_ONSET_CUE),
    }
}

fn invalidate_onset(voice: Voice) {
    let (publication, cue) = onset_pub(voice);
    publication.write(&[0; 9]);
    cue.store(0, Ordering::Release);
    if voice == Voice::Aux {
        AUX_ONSET_NODE.store(0, Ordering::Release);
    }
}

/// Produce hook, PRE-original (render thread, per voice per pass). ~one
/// relaxed load when no voice is pending; a node match costs two reads.
#[inline]
pub fn produce_pre(node: usize) -> Option<ProducePre> {
    if !active() || node == 0 {
        return None;
    }
    let voice = if node == PENDING_SONG_NODE.load(Ordering::Relaxed) {
        Voice::Song
    } else if node == PENDING_AUX_NODE.load(Ordering::Relaxed) {
        Voice::Aux
    } else {
        return None;
    };
    let (_, generation, _) = pending(voice);
    let backend = BACKEND.load(Ordering::Relaxed);
    if backend == 0 {
        return None;
    }
    // SAFETY: the node was probed (NODE_PROBE_LEN) in `on_voice_start`
    // before it became pending; the backend object in `backend_format`.
    let (consumed_before, written_before_bytes) = unsafe {
        (
            read_probed::<u64>(node, NODE_CONSUMED_BYTES),
            read_probed::<u64>(backend, DS_WRITTEN_BYTES),
        )
    };
    Some(ProducePre {
        voice,
        generation: generation.load(Ordering::Relaxed),
        consumed_before,
        written_before_bytes,
    })
}

/// Produce hook, POST-original: latch `F0` on the 0 → >0 edge of the node's
/// consumed-bytes counter.
pub fn produce_post(node: usize, pre: ProducePre) {
    let _ = std::panic::catch_unwind(|| produce_post_inner(node, pre));
}

fn produce_post_inner(node: usize, pre: ProducePre) {
    if pre.consumed_before != 0 {
        return;
    }
    // SAFETY: node probed in `on_voice_start` (see produce_pre).
    let after = unsafe { read_probed::<u64>(node, NODE_CONSUMED_BYTES) };
    if after == 0 {
        return;
    }
    let (pending_node, pending_gen, pending_cue) = pending(pre.voice);
    // Consume the pending slot only if it is still THIS node/generation.
    if pending_node
        .compare_exchange(node, 0, Ordering::AcqRel, Ordering::Relaxed)
        .is_err()
        || pending_gen.load(Ordering::Acquire) != pre.generation
    {
        return;
    }
    let ba = u64::from(BACKEND_BLOCK_ALIGN.load(Ordering::Relaxed).max(1));
    let hz = BACKEND_HZ.load(Ordering::Relaxed);
    let f0 = (pre.written_before_bytes / ba) as i64;
    let (sample, epoch) = last_sample().unwrap_or((
        Sample {
            t: -1,
            p: 0,
            wc: 0,
            w: f0,
        },
        EPOCH.load(Ordering::Relaxed),
    ));
    // Ordering, cabinet-observed 2026-09-09: source nodes produce in the
    // render loop's pre-render step, BEFORE the master pass that reads the
    // cursor and writes their block — so at produce time the newest cursor
    // sample is the PREVIOUS pass's and `F0 == sample.w + one pass` exactly
    // (1446480 − 1446039 = 441 on the first arm). Both values are consistent
    // with the model (F0 is the block the coming master pass writes; the
    // sample's lead/margin are that pass's inputs); anything else means the
    // pass structure changed — flag it for the game thread once.
    let ahead = f0.wrapping_sub(sample.w);
    if !(0..=MAX_PRODUCE_LEAD_FRAMES).contains(&ahead) && !W_MISMATCH_WARNED.load(Ordering::Relaxed)
    {
        W_MISMATCH_WARNED.store(true, Ordering::Relaxed);
    }
    let cue = pending_cue.load(Ordering::Acquire);
    let (publication, onset_cue) = onset_pub(pre.voice);
    onset_cue.store(cue, Ordering::Release);
    if pre.voice == Voice::Aux {
        AUX_ONSET_NODE.store(node, Ordering::Release);
    }
    publication.write(&[
        pre.generation,
        epoch,
        f0 as u64,
        sample.t as u64,
        sample.p as u64,
        sample.wc as u64,
        sample.w as u64,
        u64::from(hz),
        1,
    ]);
    ONSET_PENDING_LOG.fetch_or(
        match pre.voice {
            Voice::Song => 1,
            Voice::Aux => 2,
        },
        Ordering::Release,
    );
}

fn read_onset(publication: &SeqPub<9>) -> Option<Onset> {
    let w = publication.read();
    (w[8] != 0).then(|| Onset {
        generation: w[0],
        epoch: w[1],
        f0: w[2] as i64,
        t_k: w[3] as i64,
        p_k: w[4] as i64,
        wc_k: w[5] as i64,
        w_k: w[6] as i64,
        hz: w[7] as u32,
    })
}

/// The song voice's onset, if latched.
#[must_use]
pub fn song_onset() -> Option<Onset> {
    read_onset(&SONG_ONSET)
}

/// The aux (assist-tick) voice's onset, if latched.
#[must_use]
pub fn aux_onset() -> Option<Onset> {
    read_onset(&AUX_ONSET)
}

/// Generation assigned to the most recently identified aux (assist-tick)
/// voice at its Start (0 = none yet). NOT guaranteed to reflect a `Play` the
/// caller just issued: the engine's in-memory submission (where the identity
/// lands) can run a few ms AFTER `SoundBank::Play` returns (cabinet-observed
/// 2026-09-09 — the first commit of a session read 0 here). Callers must
/// treat the value as a lower bound for the onset generation they wait for.
#[must_use]
pub fn aux_voice_generation() -> u64 {
    PENDING_AUX_GEN.load(Ordering::Acquire)
}

/// The node behind the published aux onset (for [`consumed_bytes`]).
#[must_use]
pub fn aux_onset_node() -> Option<usize> {
    let node = AUX_ONSET_NODE.load(Ordering::Acquire);
    (node != 0).then_some(node)
}

/// Snapshot of a node's cumulative consumed source bytes (`node+0x5F8`).
/// Racy by design (monotonic counter read from the game thread while the
/// render thread advances it) — callers add a safety margin.
#[must_use]
pub fn consumed_bytes(node: usize) -> Option<u64> {
    read::<u64>(node, NODE_CONSUMED_BYTES)
}

/// Drain the "onset latched" flags (bit0 song, bit1 aux) for logging on the
/// game thread. Also reports (and clears) the W-mismatch latch as bit2.
pub fn take_onset_flags() -> u32 {
    let mut flags = ONSET_PENDING_LOG.swap(0, Ordering::AcqRel);
    if W_MISMATCH_WARNED.swap(false, Ordering::AcqRel) {
        flags |= 4;
    }
    flags
}

/// Register an aux (assist-tick) sound bank whose voices should be
/// onset-tracked. Idempotent; at most [`AUX_SLOTS`] banks.
pub fn register_aux_bank(bank: usize) -> bool {
    if bank == 0 {
        return false;
    }
    for slot in AUX_BANKS.iter() {
        let current = slot.load(Ordering::Acquire);
        if current == bank {
            return true;
        }
        if current == 0
            && slot
                .compare_exchange(0, bank, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        {
            return true;
        }
    }
    false
}

fn is_aux_bank(bank: usize) -> bool {
    bank != 0 && AUX_BANKS.iter().any(|s| s.load(Ordering::Relaxed) == bank)
}

/// Voice-start identity (game or notify thread, PRE-original of the
/// streaming / in-memory submission). `bank` = the cue's `IXACT2SoundBank*`
/// (`cue+0x240`), `cue` = the cue object, `node` = the engine source node.
pub fn on_voice_start(bank: usize, cue: usize, node: usize) {
    if !active() || !super::is_enabled() || node == 0 {
        return;
    }
    let voice = if super::game::is_song_bank(bank) && super::game::in_play_scene() {
        Voice::Song
    } else if is_aux_bank(bank) {
        Voice::Aux
    } else {
        return;
    };
    // Probe the node ONCE here (game/notify thread) so the per-pass produce
    // hook can read `+0x5F8` without a VirtualQuery.
    if !memory::is_readable(node as *const u8, NODE_PROBE_LEN) {
        return;
    }
    let generation = VOICE_GEN.fetch_add(1, Ordering::AcqRel) + 1;
    let (pending_node, pending_gen, pending_cue) = pending(voice);
    // Order: invalidate the old onset, then arm the pending slot with the
    // generation BEFORE the node (the produce hook reads node first, then gen).
    invalidate_onset(voice);
    pending_cue.store(cue, Ordering::Release);
    pending_gen.store(generation, Ordering::Release);
    pending_node.store(node, Ordering::Release);
    if voice == Voice::Song {
        log_info!(
            "audio_clock: song voice identified (gen {}, node {:#x}, cue {:#x}) -- awaiting first produce",
            generation,
            node,
            cue
        );
    }
}

/// A sound stopped / a cue was destroyed: drop any pending/published onset
/// that belongs to it.
pub fn on_voice_stop(cue: usize) {
    if !active() || cue == 0 {
        return;
    }
    for voice in [Voice::Song, Voice::Aux] {
        let (pending_node, _, pending_cue) = pending(voice);
        if pending_cue.load(Ordering::Acquire) == cue {
            pending_node.store(0, Ordering::Release);
            pending_cue.store(0, Ordering::Release);
        }
        let (_, onset_cue) = onset_pub(voice);
        if onset_cue.load(Ordering::Acquire) == cue {
            invalidate_onset(voice);
        }
    }
}

/// Every bank was unregistered (the diag's `invalidate_all` seam) or the
/// scene left the play scenes: forget every voice.
pub fn clear_voices() {
    if !active() {
        return;
    }
    for voice in [Voice::Song, Voice::Aux] {
        let (pending_node, _, pending_cue) = pending(voice);
        pending_node.store(0, Ordering::Release);
        pending_cue.store(0, Ordering::Release);
        invalidate_onset(voice);
    }
}

/// One-line status for the mod's boot INFO.
pub fn log_status() {
    let (passes, resets, last_reset, failures) = counters();
    log_info!(
        "audio_clock: engine observers {} (QPC {} Hz, mode {:?}, window {} s, bias {:.1} ms); passes={} resets={} last_reset={} cursor_failures={}",
        if installed() { "installed" } else { "NOT installed" },
        frequency(),
        super::config().mode,
        super::config().window_seconds,
        super::config().latency_bias_ms,
        passes,
        resets,
        last_reset,
        failures
    );
    if let Some((hz, ba)) = output_format() {
        log_info!("audio_clock: output format {} Hz, block align {}", hz, ba);
    }
}
