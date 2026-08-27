//! Windows runtime glue for the song-rate lifecycle: the one permanent scene
//! callback, raw eligibility-input gathering, and the production
//! [`LifecycleSink`] over the shared movie policy + clock publication.
//!
//! The scene callback runs while `scene_manager` holds its callback-iteration
//! lock, so the hot path here reads only atomics and raw validated game
//! memory (through `stage_records`' null-guarded accessors), applies effects
//! through lock-free sinks, and never waits, performs I/O, calls a game
//! function, or re-enters the scene manager. Boot readiness is latched ONCE
//! at [`init`] — its inputs are boot-static.
//!
//! Arming is driven by the per-side desired-rate atomics
//! ([`set_desired_percent`], written by the SONG SPEED option's callbacks —
//! defaults 100 = identity). Arming is SONG-AGNOSTIC: the session-selected
//! rate applies to whichever song the player confirms, so whichever dance
//! bank loads next is the armed generation's song. Identity boots keep zero
//! footprint: no maintenance drain thread and no bank-timeline recording
//! until the first accepted non-identity arm.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, AtomicUsize, Ordering};
use std::sync::OnceLock;

use super::clock_patch;
use super::lifecycle::{
    classify_scene26, ArmOutcome, EligibilityInputs, LifecycleSink, LifecycleState,
    TransitionOutcome, IDENTITY_PERCENT,
};
use super::transaction::FaultSelector;
use crate::core::memory;
use crate::services::movie_policy::{self, MovieSuppressor};
use crate::services::{scene_manager, score_guard, stage_records};
use crate::types::scenes::scene;
use crate::{log_info, log_warn};

/// Side-entered byte inside PlayerWork (non-zero once the side has joined) —
/// the same decode quick_logout's session gate uses.
const PLAYER_WORK_ENTERED_OFFSET: usize = 0x4;

/// The single authoritative generation lifecycle.
static LIFECYCLE: LifecycleState = LifecycleState::new();
/// Per-side desired rate percents, written by the SONG SPEED option's
/// callbacks ([`set_desired_percent`]) and read at scene-26 arming. Default
/// 100 (identity) — with no rate source writing them, every scene 26
/// resolves to identity.
static DESIRED_PERCENT: [AtomicI32; 2] = [
    AtomicI32::new(IDENTITY_PERCENT),
    AtomicI32::new(IDENTITY_PERCENT),
];
/// Per-side desired preserve-pitch flags, written by the PRESERVE SONG
/// PITCH option's callbacks ([`set_desired_preserve_pitch`]) and read at
/// scene-26 arming. Default true (pitch-preserved — the shipped WSOLA
/// behavior); with no option row writing them, every arm stretches.
static DESIRED_PRESERVE_PITCH: [AtomicBool; 2] = [AtomicBool::new(true), AtomicBool::new(true)];
/// Per-side desired sync-background-video flags, written by the SYNC
/// BACKGROUND VIDEO option's callbacks ([`set_desired_sync_movie`]) and
/// read at scene-26 arming. Default false (non-identity suppresses the
/// movie — the shipped behavior); the RAW desire — the platform capability
/// (movie_sync available AND real Windows, D14) is ANDed in at input
/// gathering, so the latched [`ArmRequest::sync_movie`] is always
/// effective.
static DESIRED_SYNC_MOVIE: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
/// One INFO per boot when a raw sync desire exists but the platform
/// capability gate clears it (Wine / engine unavailable).
static SYNC_GATE_LOGGED: AtomicBool = AtomicBool::new(false);
/// Training-arm request (training design §4.5, written by the training-mode
/// mod via [`set_training_arm`]): read at scene-26 arming alongside the
/// desired-percent atomics. While set, an eligible entry at exactly 100%
/// ARMS (identity passthrough) so training gestures can seek; while clear,
/// identity behavior is bit-for-bit the shipped pin (100% never arms).
static TRAINING_ARM: AtomicBool = AtomicBool::new(false);
/// Bind-time content pre-shift `(shift_ms << 32 | lead_ms)`, written by the
/// training-mode mod via [`set_initial_content_mapping_ms`] and consumed by
/// the create detour on every qualifying bind (R15: the mapping must be in
/// place before bank prepare's buffering reads — a post-publication call
/// loses that race by design). Packed so a reader can never observe a torn
/// pair; 0 = unmapped.
static INITIAL_MAPPING_MS: AtomicU64 = AtomicU64::new(0);
/// The song digest the pre-shift was computed for (0 = no constraint).
/// Written AFTER the mapping (see [`set_initial_content_mapping_ms`]'s
/// ordering note); the create detour declines a mapping whose stamp does
/// not match the bank it is creating.
static INITIAL_MAPPING_DIGEST: AtomicU64 = AtomicU64::new(0);
/// Sticky "a non-identity generation armed this boot": spawns the
/// maintenance drain on first arm and gates the bank-event timeline. One
/// atomic load — detour-legal.
static RATE_ARMED_THIS_BOOT: AtomicBool = AtomicBool::new(false);
/// Boot-latched identity-transaction readiness (clock + wave hooks + binding
/// integration + movie policy), computed by `lib.rs` after all inits. The
/// full score-sanitization conjunction is re-checked live per arm (it is
/// atomics-only).
static BOOT_READY: AtomicBool = AtomicBool::new(false);
/// One-time registration guard for the permanent scene callback.
static CALLBACK_REGISTERED: AtomicBool = AtomicBool::new(false);
/// Bounded-warning latches (one WARN per boot each).
static BUSY_WARNED: AtomicBool = AtomicBool::new(false);
static IN_FLIGHT_WARNED: AtomicBool = AtomicBool::new(false);
/// Last generation whose commit the maintenance drain reported. The commit
/// can land on the loader thread AFTER the GAMEPLAY scene event, so the
/// gameplay-entry log alone can miss a live Q31 (cabinet-observed
/// 2026-08-06); the drain polls the snapshot and reports each committed
/// generation exactly once.
static COMMIT_LOGGED_GENERATION: AtomicU64 = AtomicU64::new(0);

/// Boot-only fault selector (`DDR_SONG_RATE_FAULT`, developer mode only).
static FAULT: OnceLock<FaultSelector> = OnceLock::new();
/// One-time drain-thread spawn guard.
static DRAIN_STARTED: AtomicBool = AtomicBool::new(false);

/// Production effects: the shared movie-policy contributor and the clock
/// patch's seqlock publication. Both are lock-free.
struct ProductionSink;

impl LifecycleSink for ProductionSink {
    fn publish_identity(&self, generation: u64, participant_mask: u8) {
        if let Some(publication) = clock_patch::publication() {
            publication.publish_identity(generation, participant_mask);
        }
    }

    fn reset_identity(&self) {
        if let Some(publication) = clock_patch::publication() {
            let _ = publication.reset_identity();
        }
    }

    fn set_movie_suppressed(&self, suppressed: bool) {
        movie_policy::set_suppressed(MovieSuppressor::SongRate, suppressed);
    }
}

/// Register the permanent lifecycle scene callback and latch boot readiness.
/// `identity_ready` is the caller-computed identity-transaction conjunction
/// (`wavebank_hook::readiness(..).is_ready()`); `retired_diagnostic_present`
/// reports the retired Step 4 `diagnostic` config key for a one-line INFO;
/// `fault` configures the transaction pieces (developer mode only). The
/// callback registers even when nothing can arm — lifecycle observation is
/// permanent by design and a disabled/unready feature simply resolves every
/// scene 26 to identity.
pub fn init(identity_ready: bool, retired_diagnostic_present: bool, fault: Option<FaultSelector>) {
    if retired_diagnostic_present {
        log_info!(
            "song_rate: 'song_playback_speed.diagnostic' is retired and ignored — rates are selected through the SONG SPEED option"
        );
    }
    if let Some(fault) = fault {
        let _ = FAULT.set(fault);
        log_warn!(
            "song_rate: FAULT INJECTION ACTIVE ({:?}) — developer builds only",
            fault
        );
    }
    BOOT_READY.store(identity_ready, Ordering::Release);
    if CALLBACK_REGISTERED.swap(true, Ordering::AcqRel) {
        return;
    }
    scene_manager::on_scene_change(Box::new(|prev, next| {
        on_scene_event(prev, next);
    }));
    log_info!(
        "song_rate: lifecycle scene callback registered (identity_ready={})",
        identity_ready
    );
}

/// Set one side's desired rate percent (the SONG SPEED option's normalized
/// value; the option callback performs no I/O and calls no game API — this
/// is one atomic store). Out-of-domain values are stored as-is and fail
/// closed to identity at classification.
pub fn set_desired_percent(side: usize, percent: i32) {
    if let Some(slot) = DESIRED_PERCENT.get(side) {
        slot.store(percent, Ordering::Release);
    }
}

/// One side's current desired rate percent.
#[must_use]
pub fn desired_percent(side: usize) -> i32 {
    DESIRED_PERCENT
        .get(side)
        .map(|slot| slot.load(Ordering::Acquire))
        .unwrap_or(IDENTITY_PERCENT)
}

/// Set one side's desired preserve-pitch flag (the PRESERVE SONG PITCH
/// option's value; one atomic store, callback-legal).
pub fn set_desired_preserve_pitch(side: usize, preserve: bool) {
    if let Some(slot) = DESIRED_PRESERVE_PITCH.get(side) {
        slot.store(preserve, Ordering::Release);
    }
}

/// One side's current desired preserve-pitch flag.
#[must_use]
pub fn desired_preserve_pitch(side: usize) -> bool {
    DESIRED_PRESERVE_PITCH
        .get(side)
        .map(|slot| slot.load(Ordering::Acquire))
        .unwrap_or(true)
}

/// Set one side's desired sync-background-video flag (the SYNC BACKGROUND
/// VIDEO option's value; one atomic store, callback-legal).
pub fn set_desired_sync_movie(side: usize, sync: bool) {
    if let Some(slot) = DESIRED_SYNC_MOVIE.get(side) {
        slot.store(sync, Ordering::Release);
    }
}

/// One side's current RAW desired sync-background-video flag (platform
/// capability not applied — see [`sync_movie_capable`]).
#[must_use]
pub fn desired_sync_movie(side: usize) -> bool {
    DESIRED_SYNC_MOVIE
        .get(side)
        .map(|slot| slot.load(Ordering::Acquire))
        .unwrap_or(false)
}

/// Whether this cabinet can honor a sync-background-video desire: the
/// movie-sync engine initialized. Platform-uniform since 2026-08-24 (D14
/// superseded): the rate mechanism is movie_sync's scaled reference-clock
/// proxy — validated on Windows (cabinet test #5) AND under CrossOver/Wine
/// (trial #2) — not the `SetRate` call whose Wine silent-no-op motivated
/// the original Windows-only gate. Under Wine, movies additionally need
/// `non_native_os_support.movie_mode="fallback"` to exist at all
/// (suppress mode never builds movie graphs), which is the NonNativeOs
/// contributor's own concern — a sync-ON arm in suppress mode simply
/// keeps showing no movie.
#[must_use]
fn sync_movie_capable() -> bool {
    crate::services::movie_sync::is_available()
}

/// Set the training-arm request (the training-mode mod's scene-26 latch —
/// one atomic store, callback-legal). While set, eligible entries arm even
/// at 100% (identity passthrough); clearing it restores the shipped
/// identity pin exactly.
pub fn set_training_arm(requested: bool) {
    TRAINING_ARM.store(requested, Ordering::Release);
}

/// Whether a training arm is currently requested.
#[must_use]
pub fn training_arm_requested() -> bool {
    TRAINING_ARM.load(Ordering::Acquire)
}

/// Publish a content mapping onto the live binding (training design §4.5:
/// bind-time pre-shifts and stop/replay-time seeks). Returns `false` when
/// no binding is live or the values are out of range — callers fail open.
/// Atomics-only; legal from render-thread callbacks.
pub fn set_content_mapping(shift_blocks: u64, lead_blocks: u64) -> bool {
    super::binding::registry().set_active_content_mapping(shift_blocks, lead_blocks)
}

/// The live binding's content mapping `(shift_blocks, lead_blocks)`, or
/// `None` when no binding is live — the t=0 restart's leftover-shift guard.
#[must_use]
pub fn active_content_mapping() -> Option<(u64, u64)> {
    super::binding::registry().active_content_mapping()
}

/// The live binding's main-entry served-stream block grid, or `None` when
/// no binding is live — the seek transaction's mapping preflight AND its
/// quantization parameters (training design §4.4).
#[must_use]
pub fn active_content_grid() -> Option<super::binding::ContentGrid> {
    super::binding::registry().active_content_grid()
}

/// Set the sticky bind-time pre-shift `(shift_ms, lead_ms)` applied to
/// every subsequent qualifying bind (R15: skip-first must be in place
/// before the engine's first read of the bank), stamped with the song
/// digest it was computed FOR (`0` = no constraint — publication-less
/// cabinets). `(0, 0, 0)` clears it. Values clamp to u32 milliseconds
/// (~49 days — far beyond any song).
///
/// Write order is load-bearing (mapping THEN digest, both Release): the
/// create detour reads digest-then-mapping, so every torn interleaving
/// yields either a consistent pair or a stale-digest pair the coherence
/// check declines — a wrong-song shift can never arm.
pub fn set_initial_content_mapping_ms(shift_ms: u64, lead_ms: u64, song_digest: u64) {
    let shift = u32::try_from(shift_ms).unwrap_or(u32::MAX);
    let lead = u32::try_from(lead_ms).unwrap_or(u32::MAX);
    INITIAL_MAPPING_MS.store(u64::from(shift) << 32 | u64::from(lead), Ordering::Release);
    INITIAL_MAPPING_DIGEST.store(song_digest, Ordering::Release);
}

/// The current bind-time pre-shift `(shift_ms, lead_ms)` — the raw sticky
/// request, digest-unchecked (the training driver's "was a silent start
/// requested" read; binds use
/// [`initial_content_mapping_coherent`] instead).
#[must_use]
pub fn initial_content_mapping_ms() -> (u64, u64) {
    let packed = INITIAL_MAPPING_MS.load(Ordering::Acquire);
    (packed >> 32, packed & u64::from(u32::MAX))
}

/// The bind-time pre-shift IF its digest stamp is coherent with the song
/// being created (`fresh_digest` = the same-call publication), else
/// `(0, 0)` (the fast-confirm race: rows/pre-shift still describe the
/// previously highlighted song — the stale shift must not bind into this
/// one; `selected_song::digests_coherent` is the rule). Read order digest
/// THEN mapping — see [`set_initial_content_mapping_ms`].
#[must_use]
pub fn initial_content_mapping_coherent(fresh_digest: Option<u64>) -> (u64, u64) {
    let stamp = INITIAL_MAPPING_DIGEST.load(Ordering::Acquire);
    if !super::selected_song::digests_coherent(stamp, fresh_digest) {
        return (0, 0);
    }
    initial_content_mapping_ms()
}

/// Current lifecycle state (transaction wiring + diagnostics).
#[must_use]
pub fn lifecycle() -> &'static LifecycleState {
    &LIFECYCLE
}

/// The committed movie-rate directive `movie_sync` consumes at graph open:
/// `Some(effective_rate)` — the exact committed `source/output` ratio as
/// the `IMediaSeeking::SetRate` argument — only when a committed
/// non-identity generation latched SYNC BACKGROUND VIDEO ON (effective:
/// real Windows with the engine available, by construction — under Wine or
/// with sync OFF the arm suppressed the graph, so BuildGraph never opens a
/// movie that could ask). Everything else (identity, uncommitted, sync
/// OFF) reads `None` and the movie plays plain with position sync only.
#[must_use]
pub fn movie_rate_directive() -> Option<f64> {
    if !LIFECYCLE.sync_movie() {
        return None;
    }
    let snapshot = clock_patch::snapshot();
    if !snapshot.is_non_identity_commit() {
        return None;
    }
    if snapshot.effective_rate.output_frames == 0 {
        return None;
    }
    Some(
        snapshot.effective_rate.source_frames as f64 / snapshot.effective_rate.output_frames as f64,
    )
}

/// The boot fault selector (default = no faults).
#[must_use]
pub fn fault_selector() -> FaultSelector {
    FAULT.get().copied().unwrap_or_default()
}

/// Whether a non-identity generation has armed this boot (gates the
/// bank-event timeline recording; a plain atomic load, detour-legal).
#[must_use]
pub fn rate_recording_active() -> bool {
    RATE_ARMED_THIS_BOOT.load(Ordering::Acquire)
}

/// Full runtime-integration readiness for the SONG SPEED option: the
/// boot-latched identity-transaction conjunction (clock patch + wave hooks +
/// binding integration + movie policy) AND the live score-guard
/// full-sanitization conjunction. The mod hides its row while this is false —
/// a rate the shared service cannot guarantee audio/clock/score/movie
/// integration for must not be selectable. Structurally false in the Step-1
/// identity base: the binding leg (`binding::integration_available`) stays
/// false until plan Step 4 installs the streaming integration.
#[must_use]
pub fn integration_ready() -> bool {
    BOOT_READY.load(Ordering::Acquire) && score_guard::is_full_sanitization_available()
}

/// Background maintenance drain: polls commit visibility (the wave-bank
/// detour itself never logs, and the commit can land after the GAMEPLAY
/// scene event, so this is the guaranteed once-per-generation "a rate is
/// live" line) and drains the diagnostic bank-event timeline.
fn spawn_maintenance_drain() {
    if DRAIN_STARTED.swap(true, Ordering::AcqRel) {
        return;
    }
    std::thread::spawn(|| loop {
        std::thread::sleep(std::time::Duration::from_millis(250));
        log_committed_snapshot_once();
        drain_bank_timeline();
        drain_binding_maintenance();
    });
}

/// Idempotent drain start for the preview bind path (preview design
/// §Components 5): preview bindings need the sweep/reporting even on
/// boots that never arm a gameplay generation. Same lazy-spawn contract —
/// identity boots with the preview feature idle keep zero footprint.
pub fn ensure_maintenance_drain() {
    spawn_maintenance_drain();
}

/// Last generation whose silence-fill engagement was WARNed (design req 28:
/// one WARN via the drain when the producer dies mid-song).
static SILENCE_WARNED_GENERATION: AtomicU64 = AtomicU64::new(0);

/// Last PREVIEW generation whose publication was INFO'd (the drain's
/// publish latch — the detour branch never logs).
static PREVIEW_LOGGED_GENERATION: AtomicU64 = AtomicU64::new(0);

/// Preview-slot silence-fill WARN latch (mirror of
/// [`SILENCE_WARNED_GENERATION`]).
static PREVIEW_SILENCE_WARNED_GENERATION: AtomicU64 = AtomicU64::new(0);

/// Last-reported loader-chain probe outcome (plan Step-4 demo: the
/// create detour's preview branch probes the restart half's `TS child →
/// View → AudioPlayer → AudioLoader` walk after each publish; the drain
/// reports each DISTINCT outcome once — first resolve, and any
/// subsequent class change — never per-settle).
static PREVIEW_CHAIN_PROBE_LOGGED: AtomicUsize = AtomicUsize::new(0);

/// Per-slot dedupe for the stuck-read diagnostic (armed_at nanos of the
/// last instance reported per pending slot).
static STUCK_READ_LOGGED: [AtomicU64; 4] = [
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
];

/// Preview-slot twin of [`STUCK_READ_LOGGED`].
static PREVIEW_STUCK_READ_LOGGED: [AtomicU64; 4] = [
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
];

/// Diagnostic (step05-fix bring-up): name any engine read stuck pending
/// longer than this. One line per arm instance, drain-side only.
const STUCK_READ_NANOS: u64 = 500_000_000;
/// Preview-slot threshold: a pitch-preserved preview's FIRST packet
/// legitimately takes ~583 ms to synthesize (WSOLA is output-frame-bound
/// under CrossOver — the accepted "slightly late but reliable" latency
/// the play watchdog covers), so 500 ms would WARN on expected behavior.
/// Anything past ~1.5 s on a preview read is genuinely stuck.
const PREVIEW_STUCK_READ_NANOS: u64 = 1_500_000_000;

/// Streaming-binding maintenance (design req 26): consume the fixed
/// maintenance queue's reclaim records (transaction-slot recycling for
/// released and quarantined slots), sweep the binding registry's retired
/// list — buffers are freed only at `Retired ∧ readers == 0` after the
/// reclamation cooldown, with one line per reclaimed generation carrying
/// the producer's throughput/deferral metrics (plan Step 5's benchmark run
/// sheet) — and report coalesced preflight refusals (the detours and the
/// preflight never log; this drain does).
fn drain_binding_maintenance() {
    super::wavebank_hook::drain_maintenance(|event| match event.kind {
        super::xact_runtime::MaintenanceKind::ReclaimBinding => {
            let Some(slots) = super::wavebank_hook::slots() else {
                return;
            };
            let index = usize::from(event.slot_index);
            // The slot record is bookkeeping independent of the binding's
            // buffers (those wait for reader quiescence in the registry):
            // released and quarantined slots free here; anything else is a
            // stale/duplicate record and is dropped.
            match slots.phase(index) {
                Some(super::xact_runtime::XactSlotPhase::ReleasePending) => {
                    let _ = slots.finish_release(index);
                }
                Some(super::xact_runtime::XactSlotPhase::Quarantined) => {
                    let _ = slots.finish_quarantine(index);
                }
                _ => {}
            }
        }
    });
    let registry = super::binding::registry();
    // Diagnostic: name any read the engine has been stuck on for too long
    // (500 ms active / 1.5 s preview — the preview's first WSOLA packet
    // legitimately takes ~583 ms) — offset, resolved region, and the
    // producer's cursors. One line per arm instance (deduped by
    // armed-at), drain-side only. Covers BOTH slots (the deploy-#1
    // incident was diagnosed half-blind without preview coverage).
    for (slot_name, stuck, threshold) in [
        ("active", &STUCK_READ_LOGGED, STUCK_READ_NANOS),
        (
            "preview",
            &PREVIEW_STUCK_READ_LOGGED,
            PREVIEW_STUCK_READ_NANOS,
        ),
    ] {
        let visit = |binding: &super::binding::Binding| {
            binding.for_each_armed_slot(|slot, offset, len, age_nanos, armed_at| {
                if age_nanos < threshold {
                    return;
                }
                if stuck[slot & 3].swap(armed_at, Ordering::AcqRel) == armed_at {
                    return;
                }
                let span = binding.layout().resolve(offset, len);
                let (ring_produced, ring_consumed) = binding.ring_cursors();
                log_warn!(
                    "song_rate: STUCK READ ({}) slot={} offset={} len={} age={}ms region={:?} ring=({}, {}) virtual_size={}",
                    slot_name,
                    slot,
                    offset,
                    len,
                    age_nanos / 1_000_000,
                    span.region,
                    ring_produced,
                    ring_consumed,
                    binding.layout().virtual_size,
                );
            });
        };
        if slot_name == "active" {
            registry.with_active(visit);
        } else {
            registry.with_preview(visit);
        }
    }
    // Design req 28: a producer that died mid-song flipped the ACTIVE
    // binding to silence-fill — report it exactly once per generation (the
    // detours and the producer never log; this drain does). Preview
    // bindings get the same coverage against their own latch.
    if let Some((generation, state)) =
        registry.with_active(|binding| (binding.generation(), binding.state()))
    {
        if state == super::binding::BindingState::SilenceFill
            && SILENCE_WARNED_GENERATION.swap(generation, Ordering::AcqRel) != generation
        {
            log_warn!(
                "song_rate: generation {} producer failed mid-song — silence-fill engaged (clock, score taint, and movie policy retained)",
                generation
            );
        }
    }
    if let Some((generation, state)) =
        registry.with_preview(|binding| (binding.generation(), binding.state()))
    {
        if state == super::binding::BindingState::SilenceFill
            && PREVIEW_SILENCE_WARNED_GENERATION.swap(generation, Ordering::AcqRel) != generation
        {
            log_warn!(
                "song_rate: preview generation {} producer failed — silence-fill engaged (preview plays silence until the next settle)",
                generation
            );
        }
    }
    registry.sweep(|generation, metrics| {
        log_info!(
            "song_rate: generation {} binding reclaimed — {} frames produced in {} ms wall, {} deferrals (max latency {} us)",
            generation,
            metrics.frames_produced,
            metrics.wall_nanos / 1_000_000,
            metrics.deferral_count,
            metrics.max_deferral_nanos / 1_000,
        );
    });
    if let Some((refusal, file_id, count)) = registry.take_refusal() {
        log_warn!(
            "song_rate: bind refused for file_id {} ({:?}; {} since last report) — stock 100% playback",
            file_id,
            refusal,
            count,
        );
    }
    // Preview visibility (preview design §Components 8): one INFO per
    // published preview binding (latched by its own generation counter)
    // and the preview mailbox's coalesced refusal WARN.
    if let Some((generation, file_id, rate, preserve)) = registry.with_preview(|binding| {
        (
            binding.generation(),
            binding.file_id(),
            binding.rate(),
            binding.preserve_pitch(),
        )
    }) {
        if PREVIEW_LOGGED_GENERATION.swap(generation, Ordering::AcqRel) != generation {
            log_info!(
                "song_rate: preview binding live — preview generation {}, file_id {}, rate {}/{}, {}",
                generation,
                file_id,
                rate.source_frames,
                rate.output_frames,
                if preserve {
                    "pitch-preserved"
                } else {
                    "resample"
                },
            );
        }
    }
    if let Some((refusal, file_id, count)) = registry.take_preview_refusal() {
        log_warn!(
            "song_rate: preview bind refused for file_id {} ({:?}; {} since last report) — stock preview",
            file_id,
            refusal,
            count,
        );
    }
    // Parse forensics for UnsupportedProfile preview refusals (the strict
    // parser rejecting a resident row the engine plays fine — the
    // 2026-08-16 sticky-refusal incident). One packet per drain cycle at
    // most; the head bytes + error classify the failure from the log.
    if let Some(forensics) = super::preview::take_parse_forensics() {
        log_warn!(
            "song_rate: preview parse forensics — file_id {} path '{}' buf {:#x} len {} row_state {:?} head {:02X?} error: {}",
            forensics.file_id,
            forensics.path,
            forensics.buffer_ptr,
            forensics.buffer_len,
            forensics.row_state,
            forensics.head,
            forensics.error,
        );
    }
    // Loader-chain probe report (plan Step-4 demo): one line per distinct
    // outcome class. OK proves the restart half's walk resolves against a
    // live preview; either failure class means the Step-5 executor would
    // decline (fail-open — the preview keeps playing as-is).
    let probe = super::preview::take_chain_probe();
    if probe != 0 && PREVIEW_CHAIN_PROBE_LOGGED.swap(probe, Ordering::AcqRel) != probe {
        match probe {
            super::preview::CHAIN_PROBE_OK => log_info!(
                "song_rate: preview loader chain resolved (identity gates + field sanity passed) — restart half ready"
            ),
            super::preview::CHAIN_PROBE_UNRESOLVED => log_warn!(
                "song_rate: preview loader chain failed to resolve while a preview binding is live — the restart executor would decline (fail-open)"
            ),
            super::preview::CHAIN_PROBE_INSANE => log_warn!(
                "song_rate: preview loader chain resolved but field sanity failed (not the slot-5 one-shot shape) — the restart executor would decline (fail-open)"
            ),
            _ => {}
        }
    }
}

/// Drain the diagnostic bank-event timeline into the log (single consumer).
/// This is the 2026-08-06 stock-audio investigation instrument: it shows
/// every wave-bank create/unregister the detours observed — which bank
/// instances coexisted, their file ids, and which create carried our
/// binding.
fn drain_bank_timeline() {
    let Some(timeline) = super::wavebank_hook::timeline() else {
        return;
    };
    while let Some(event) = timeline.pop() {
        match event.kind {
            super::xact_runtime::BankEventKind::Create => log_info!(
                "song_rate: bank timeline t+{}ms CREATE file_id={} status={} path={:?}",
                event.tick_ms,
                event.file_id,
                event.status,
                event.path,
            ),
            super::xact_runtime::BankEventKind::Unregister => log_info!(
                "song_rate: bank timeline t+{}ms UNREGISTER file_id={}",
                event.tick_ms,
                event.file_id,
            ),
        }
    }
    let dropped = timeline.take_dropped();
    if dropped > 0 {
        log_warn!(
            "song_rate: bank timeline dropped {} events (ring full)",
            dropped
        );
    }
}

/// Log a newly committed snapshot exactly once per generation. Safe from any
/// thread: reads only the seqlock snapshot and one atomic.
fn log_committed_snapshot_once() {
    let snapshot = clock_patch::snapshot();
    if !snapshot.committed {
        return;
    }
    if COMMIT_LOGGED_GENERATION.swap(snapshot.generation, Ordering::AcqRel) == snapshot.generation {
        return;
    }
    log_info!(
        "song_rate: generation {} committed ({}%, rate {}/{}, q31 {})",
        snapshot.generation,
        snapshot.requested_percent,
        snapshot.effective_rate.source_frames,
        snapshot.effective_rate.output_frames,
        snapshot
            .effective_rate
            .q31()
            .map(|q| q.to_string())
            .unwrap_or_else(|_| "unrepresentable".to_string()),
    );
}

fn on_scene_event(prev: i32, next: i32) {
    let sink = ProductionSink;
    if next == scene::SONG_TO_STAGE_INTERSTITIAL {
        let desired = [desired_percent(0), desired_percent(1)];
        let training_arm = training_arm_requested();
        // Skip the raw game-memory reads entirely when both sides desire
        // identity AND no training arm is requested (the overwhelmingly
        // common case) — the classifier would resolve IdentityRate anyway,
        // and the identity arm still runs to clear any stale attempt state.
        // A training request needs the session reads even at 100%: the
        // eligibility gates (course/versus/stage) apply unchanged.
        let session_reads =
            training_arm || desired.iter().any(|&percent| percent != IDENTITY_PERCENT);
        // Effective per-side sync flags: raw desire AND platform capability
        // (D14 — under Wine, or with the movie-sync engine unavailable, the
        // desire is cleared and non-identity suppresses as shipped). Only
        // evaluated when a non-identity rate could arm.
        let desired_sync = if session_reads {
            let raw = [desired_sync_movie(0), desired_sync_movie(1)];
            let capable = (raw[0] || raw[1]) && sync_movie_capable();
            if (raw[0] || raw[1]) && !capable && !SYNC_GATE_LOGGED.swap(true, Ordering::AcqRel) {
                log_info!(
                    "song_rate: SYNC BACKGROUND VIDEO desired but the movie-sync engine is not initialized — non-100% keeps suppressing the movie"
                );
            }
            [raw[0] && capable, raw[1] && capable]
        } else {
            [false, false]
        };
        let inputs = EligibilityInputs {
            services_ready: BOOT_READY.load(Ordering::Acquire)
                && score_guard::is_full_sanitization_available(),
            desired: Some(desired),
            desired_preserve: [desired_preserve_pitch(0), desired_preserve_pitch(1)],
            desired_sync,
            training_arm,
            course_field: if session_reads {
                read_course_field()
            } else {
                None
            },
            entered: if session_reads { entered_sides() } else { None },
            stage_index: if session_reads {
                stage_records::stage_counter()
            } else {
                None
            },
        };
        match LIFECYCLE.on_scene26(&sink, classify_scene26(&inputs)) {
            ArmOutcome::Armed { generation } => {
                // First arm this boot: enable the bank-event timeline and
                // spawn the maintenance drain (identity boots never reach
                // here, keeping zero footprint).
                if !RATE_ARMED_THIS_BOOT.swap(true, Ordering::AcqRel) {
                    spawn_maintenance_drain();
                }
                log_info!(
                    "song_rate: generation {} armed ({}%, preserve_pitch={}, sync_movie={}, mask 0b{:02b}, stage {}) — {}, clock identity",
                    generation,
                    LIFECYCLE.requested_percent(),
                    LIFECYCLE.preserve_pitch(),
                    LIFECYCLE.sync_movie(),
                    LIFECYCLE.participant_mask(),
                    LIFECYCLE.stage_index(),
                    if LIFECYCLE.sync_movie() {
                        "movie kept for rate sync"
                    } else {
                        "movie tentatively suppressed"
                    }
                );
            }
            ArmOutcome::Identity(reason) => {
                // Bounded: only meaningful (and logged) when a non-identity
                // rate was desired — otherwise every ordinary song start
                // would emit an expected IdentityRate line.
                if session_reads {
                    log_info!("song_rate: scene 26 resolved to identity ({:?})", reason);
                }
            }
            ArmOutcome::Deferred => {
                if !IN_FLIGHT_WARNED.swap(true, Ordering::AcqRel) {
                    log_warn!(
                        "song_rate: arm deferred — a generation is XACT-in-flight (supersession refused)"
                    );
                }
            }
            ArmOutcome::Busy => warn_busy(),
        }
    } else {
        // Bounded commit visibility (Task 3 oracle): entering gameplay with a
        // committed generation logs the exact applied ratio + Q31 once per
        // song. The commit may instead land AFTER this scene event (loader
        // thread), in which case the maintenance drain's poll reports it —
        // the two lines are distinct and independently latched. (The audio
        // detours themselves are allocation-free and never log.)
        if next == scene::GAMEPLAY
            && prev != scene::GAMEPLAY
            && LIFECYCLE.phase() == super::lifecycle::GenerationPhase::Committed
        {
            let snapshot = clock_patch::snapshot();
            log_info!(
                "song_rate: gameplay started with committed generation {} ({}%, rate {}/{}, q31 {})",
                snapshot.generation,
                snapshot.requested_percent,
                snapshot.effective_rate.source_frames,
                snapshot.effective_rate.output_frames,
                snapshot
                    .effective_rate
                    .q31()
                    .map(|q| q.to_string())
                    .unwrap_or_else(|_| "unrepresentable".to_string()),
            );
        }
        match LIFECYCLE.on_transition(&sink, prev, next) {
            TransitionOutcome::CompletedAttempt => {
                log_info!(
                    "song_rate: generation {} completed at gameplay exit — identity reset first, movie contributor cleared",
                    LIFECYCLE.generation()
                );
            }
            TransitionOutcome::AbandonedPreExposure => {
                log_info!(
                    "song_rate: generation {} abandoned pre-exposure (left the stage corridor) — identity restored",
                    LIFECYCLE.generation()
                );
            }
            TransitionOutcome::ForcedIdentity => {
                log_info!(
                    "song_rate: title/attract reset — forced identity, movie contributor cleared"
                );
            }
            TransitionOutcome::XactStillInFlight => {
                if !IN_FLIGHT_WARNED.swap(true, Ordering::AcqRel) {
                    log_warn!(
                        "song_rate: scene transition ignored while XACT-in-flight (prev={}, next={})",
                        prev,
                        next
                    );
                }
            }
            TransitionOutcome::GameplayEnteredLateFailed => {
                // Design req 49: the aborted load unexpectedly reached
                // gameplay — taint before any score can be trusted.
                apply_late_failed_gameplay_taint();
            }
            TransitionOutcome::Busy => warn_busy(),
            TransitionOutcome::QuickRestartRetained | TransitionOutcome::NoChange => {}
        }
    }
}

fn warn_busy() {
    if !BUSY_WARNED.swap(true, Ordering::AcqRel) {
        log_warn!("song_rate: lifecycle writer contention — event dropped fail-closed to identity");
    }
}

/// Conservative gameplay-entry score policy for a LateFailed generation
/// (design req 49): the aborted load unexpectedly proceeded, so pending and
/// session taint are added for the generation's participants before any
/// score can be trusted.
fn apply_late_failed_gameplay_taint() {
    let mask = LIFECYCLE.participant_mask();
    let generation = LIFECYCLE.generation();
    let stage_index = LIFECYCLE.stage_index();
    for side in 0..2usize {
        if mask & (1 << side) != 0 {
            let _ = score_guard::append_pending_rate_save(side, generation, stage_index);
            score_guard::mark_session_tainted(side);
        }
    }
    log_warn!(
        "song_rate: gameplay entered while generation {} is LateFailed — conservative score taint applied (mask 0b{:02b})",
        generation,
        mask
    );
}

/// GameWork course-mode field (`None` = unreadable → identity).
fn read_course_field() -> Option<u64> {
    let game_work = stage_records::game_work()?;
    Some(unsafe { memory::read_u64(game_work.add(stage_records::course_field_offset())) })
}

/// Per-side entered flags (`PlayerWork+0x4 != 0`), `None` when the layout is
/// unavailable.
fn entered_sides() -> Option<[bool; 2]> {
    if !stage_records::is_available() {
        return None;
    }
    let mut entered = [false; 2];
    for (side, flag) in entered.iter_mut().enumerate() {
        match stage_records::player_work(side) {
            Some(work) => {
                *flag = unsafe { memory::read_u8(work.add(PLAYER_WORK_ENTERED_OFFSET)) } != 0;
            }
            None => return None,
        }
    }
    Some(entered)
}
