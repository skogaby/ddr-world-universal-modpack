//! Pure song-rate generation lifecycle: phases, the supported scalar rate
//! domain, scene-26 eligibility classification, and the scene-transition
//! engine.
//!
//! Everything here is host-testable state-machine logic. Runtime glue
//! (`runtime.rs`, `#[cfg(windows)]`) gathers the raw inputs from game memory
//! and applies effects through the [`LifecycleSink`] trait, whose production
//! implementation writes the shared movie-policy contributor and the
//! seqlock-published rate snapshot. Keeping effects behind the trait lets
//! tests assert effect *ordering* (identity reset strictly before the movie
//! contributor clears, per the design's definitive lifecycle rules).
//!
//! Scene-driven transitions (arm / abandon / complete / reset) are fully
//! wired; the binding/commit phase entry points
//! ([`LifecycleState::mark_exposed`] and friends) validate transitions only —
//! the allocation-free commit ordering (score protection → movie confirmation
//! → snapshot → Q31-last) belongs to the wave-bank transaction and never
//! happens here. The clock stays at exact identity through every path in
//! this module.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, AtomicU8, Ordering};

use crate::types::scenes::{scene, ATTRACT_SCENE_MAX, ATTRACT_SCENE_MIN};

// ── Phases ───────────────────────────────────────────────────────────

/// Lifecycle phase of the (single) authoritative song-rate generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum GenerationPhase {
    /// No non-100% generation exists; stock behavior.
    Identity = 0,
    /// Scene 26 accepted a non-100% arm; audio not yet requested.
    Armed = 1,
    /// The create detour's pre-original preflight is binding this
    /// generation's song (synchronous inside `wavebank_create`).
    Binding = 2,
    /// A binding was published to the native call — uncancellable until
    /// XACT resolves.
    XactInFlight = 4,
    /// `wavebank_create` accepted the bound bank.
    Committed = 5,
    /// The attempt ended at a definitive gameplay-exit boundary.
    Completed = 6,
    /// Fallback to stock audio before any binding was published.
    EarlyFailed = 7,
    /// XACT rejected the bound bank after the binding was published.
    LateFailed = 8,
}

impl GenerationPhase {
    fn from_u8(raw: u8) -> Self {
        match raw {
            1 => Self::Armed,
            2 => Self::Binding,
            4 => Self::XactInFlight,
            5 => Self::Committed,
            6 => Self::Completed,
            7 => Self::EarlyFailed,
            8 => Self::LateFailed,
            _ => Self::Identity,
        }
    }
}

// ── Rate domain ──────────────────────────────────────────────────────

/// The maintainer-approved scalar rate domain (2026-08-07, superseding the
/// original 75/100/125 enum): multiples of 5 from 25% to 175%. 100 is the
/// identity rate — a valid selection that arms nothing.
pub const MIN_RATE_PERCENT: i32 = 25;
pub const MAX_RATE_PERCENT: i32 = 175;
pub const RATE_PERCENT_STEP: i32 = 5;
pub const IDENTITY_PERCENT: i32 = 100;

/// Is this percent inside the supported scalar domain (identity included)?
/// Anything else fails closed to identity — an out-of-domain persisted or
/// injected value must never partially arm.
#[must_use]
pub fn is_supported_rate_percent(percent: i32) -> bool {
    (MIN_RATE_PERCENT..=MAX_RATE_PERCENT).contains(&percent) && percent % RATE_PERCENT_STEP == 0
}

/// Normalize any persisted/injected value into the supported domain: clamp
/// to 25..=175, then snap half-up to the nearest multiple of 5. The SONG
/// SPEED option's load transform and change callback both delegate here, so
/// a legacy or hand-edited value can never leave a side desiring a percent
/// the classifier would fail closed on.
#[must_use]
pub fn snap_rate_percent(value: i32) -> i32 {
    let clamped = value.clamp(MIN_RATE_PERCENT, MAX_RATE_PERCENT);
    let snapped =
        (clamped + RATE_PERCENT_STEP / 2).div_euclid(RATE_PERCENT_STEP) * RATE_PERCENT_STEP;
    snapped.clamp(MIN_RATE_PERCENT, MAX_RATE_PERCENT)
}

// ── Scene-26 eligibility ─────────────────────────────────────────────

/// Raw classification inputs gathered at normal scene-26 entry. `None`
/// anywhere means the underlying pointer/service could not be read — every
/// such case fails closed to identity. Matching/BPL/demo/event/special
/// chains never enter normal scene 26 at all (they use separate scene
/// ranges), which is the structural half of the mode exclusion; this
/// classifier only ever runs for the normal chain.
pub struct EligibilityInputs {
    /// Full runtime readiness conjunction (identity transaction + clock +
    /// movie policy + full score sanitization), latched by the runtime.
    pub services_ready: bool,
    /// Per-side desired rate percents (P1, P2) — the governing side's value
    /// (the single entered side, or P1 in versus — the SONG SPEED mod
    /// mirrors the rows so both sides agree) selects the shared rate for
    /// both audio and clock. `None` means no rate source is registered
    /// (the SONG SPEED mod is absent/disabled).
    pub desired: Option<[i32; 2]>,
    /// Per-side preserve-pitch flags (P1, P2) — the governing side's value
    /// selects the DSP mode (true = WSOLA stretch, false = plain resample).
    /// Plain (not Option): with no option row writing them the runtime
    /// atomics stay at their preserved default, and the flag never affects
    /// the eligibility decision itself.
    pub desired_preserve: [bool; 2],
    /// Per-side EFFECTIVE sync-background-video flags (P1, P2) — the
    /// governing side's value decides whether a non-identity arm keeps the
    /// movie (rate-synced by `movie_sync`'s clock proxy) instead of suppressing
    /// it. The runtime pre-ANDs the capability (movie-sync engine
    /// available — platform-uniform since the clock proxy superseded D14,
    /// 2026-08-24). Never affects the eligibility decision itself.
    pub desired_sync: [bool; 2],
    /// Training-arm request (training design §4.5, set by the training-mode
    /// mod before scene 26): an eligible entry at exactly 100% ARMS —
    /// producing an identity-passthrough binding so gestures can seek —
    /// instead of resolving [`IdentityReason::IdentityRate`]. Every other
    /// gate (course, unknown session, stage) applies unchanged — versus
    /// arms since the 2026-08-31 lift (P1 governs, both mask bits);
    /// without the request, identity behavior is bit-for-bit the shipped
    /// pin (100% never arms).
    pub training_arm: bool,
    /// GameWork course-mode field (nonzero = course session).
    pub course_field: Option<u64>,
    /// Per-side entered flags (`PlayerWork+0x4`).
    pub entered: Option<[bool; 2]>,
    /// 0-based GameWork stage counter.
    pub stage_index: Option<i32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdentityReason {
    ServicesUnavailable,
    /// No rate source registered (mod absent/disabled).
    NoRateSource,
    /// The entered side desires exactly 100% — the ordinary case.
    IdentityRate,
    /// The entered side's desired value is outside the supported scalar
    /// domain (fail closed — never partially arm an unknown value).
    UnsupportedRate,
    CourseMode,
    NoSideEntered,
    /// Session pointers unreadable.
    UnknownSession,
    /// Stage counter unavailable.
    StageUnknown,
}

/// An accepted non-100% arm request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArmRequest {
    pub requested_percent: i32,
    /// The governing side's preserve-pitch flag (true = pitch-preserved
    /// stretch, false = plain resample). Carried to the binding preflight;
    /// never consulted by eligibility.
    pub preserve_pitch: bool,
    /// The governing side's EFFECTIVE sync-background-video flag (see
    /// [`EligibilityInputs::desired_sync`]). True on a non-identity arm ⇒
    /// the tentative `MovieSuppressor::SongRate` set is skipped: the movie
    /// graph builds normally and `movie_sync` rate-locks it at graph open.
    /// Never consulted by eligibility.
    pub sync_movie: bool,
    /// Bit per participating side (bit 0 = P1, bit 1 = P2). One bit for a
    /// solo/doubles arm (the single entered side owns the shared rate);
    /// BOTH bits for a versus arm (the rate — and its score containment —
    /// applies to both sides; P1's desired values govern).
    pub participant_mask: u8,
    pub stage_index: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EligibilityDecision {
    Arm(ArmRequest),
    Identity(IdentityReason),
}

/// Classify a normal scene-26 entry. Pure; never blocks. Arming is
/// SONG-AGNOSTIC by design: the session-selected rate applies to whichever
/// song the player confirmed, so whichever dance bank the game loads next IS
/// the armed generation's song (the merged Steps 5+6 model — every armed
/// song genuinely commits, and tentative movie suppression through a failed
/// attempt is accepted design behavior).
pub fn classify_scene26(inputs: &EligibilityInputs) -> EligibilityDecision {
    use EligibilityDecision::Identity;
    if !inputs.services_ready {
        return Identity(IdentityReason::ServicesUnavailable);
    }
    let Some(desired) = inputs.desired else {
        return Identity(IdentityReason::NoRateSource);
    };
    let Some(course_field) = inputs.course_field else {
        return Identity(IdentityReason::UnknownSession);
    };
    if course_field != 0 {
        return Identity(IdentityReason::CourseMode);
    }
    let Some(entered) = inputs.entered else {
        return Identity(IdentityReason::UnknownSession);
    };
    let (side, participant_mask) = match (entered[0], entered[1]) {
        (false, false) => return Identity(IdentityReason::NoSideEntered),
        // Local versus arms as a SHARED rate: the mechanism is cabinet-
        // global (one clock factor, one dance bank), and the SONG SPEED /
        // training mods mirror both sides' rows while versus is active
        // (`versus_mirror`), so P1 — the authoritative mirror seed —
        // governs. Both mask bits: score containment (per-stage
        // suppression + logout sanitization) applies to both sides.
        // Training arms included (2026-08-31 versus-training lift):
        // gestures/loops/bounds move the one shared timeline for both
        // players by construction.
        (true, true) => (0u8, 0b11u8),
        (true, false) => (0u8, 0b01),
        (false, true) => (1u8, 0b10),
    };
    let requested_percent = desired[usize::from(side)];
    if requested_percent == IDENTITY_PERCENT && !inputs.training_arm {
        return Identity(IdentityReason::IdentityRate);
    }
    if !is_supported_rate_percent(requested_percent) {
        return Identity(IdentityReason::UnsupportedRate);
    }
    let Some(stage_index) = inputs.stage_index else {
        return Identity(IdentityReason::StageUnknown);
    };
    if stage_index < 0 {
        return Identity(IdentityReason::StageUnknown);
    }
    EligibilityDecision::Arm(ArmRequest {
        requested_percent,
        preserve_pitch: inputs.desired_preserve[usize::from(side)],
        sync_movie: inputs.desired_sync[usize::from(side)],
        participant_mask,
        stage_index,
    })
}

// ── Effects sink ─────────────────────────────────────────────────────

/// Effects the lifecycle engine applies at scene-driven boundaries. The
/// production implementation (runtime glue) forwards to the shared movie
/// policy and the clock-patch publication; tests record call order.
///
/// Every implementation must be nonblocking: these run inside the scene
/// manager's callback iteration (which holds its lock), so they may only
/// touch atomics/seqlock words.
pub trait LifecycleSink {
    /// Publish an identity snapshot carrying the new generation identity
    /// (`committed = false`; the Q31 factor stays identity).
    fn publish_identity(&self, generation: u64, participant_mask: u8);
    /// Write Q31 identity first, then the identity snapshot (the
    /// `RatePublication::reset_identity` contract).
    fn reset_identity(&self);
    /// Set/clear the song-rate movie contributor.
    fn set_movie_suppressed(&self, suppressed: bool);
}

// ── Lifecycle state machine ──────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArmOutcome {
    /// Non-100% generation armed (tentative movie suppression active).
    Armed { generation: u64 },
    /// Scene 26 resolved to identity.
    Identity(IdentityReason),
    /// An in-flight generation is awaiting XACT — the arm is refused and the
    /// caller leaves everything untouched (supersession refusal).
    Deferred,
    /// Another writer held the guard; fail closed to identity, change
    /// nothing.
    Busy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransitionOutcome {
    NoChange,
    /// GAMEPLAY → GAMEPLAY: retain generation, movie policy, and rate state.
    QuickRestartRetained,
    /// GAMEPLAY → non-GAMEPLAY: identity reset (Q31 first), movie cleared,
    /// phase Completed. Score state untouched.
    CompletedAttempt,
    /// A pre-binding attempt left the 26→27→28 corridor without reaching
    /// gameplay: identity + movie cleared.
    AbandonedPreExposure,
    /// Title/attract reset: forced identity, movie cleared.
    ForcedIdentity,
    /// Transition ignored because the generation is XACT-in-flight.
    XactStillInFlight,
    /// Gameplay began while the generation is LateFailed (the aborted load
    /// unexpectedly proceeded). The caller must add conservative pending and
    /// session score taint for the generation's participants before any
    /// score can be trusted (design req 49).
    GameplayEnteredLateFailed,
    /// Guard contention — nothing changed (bounded-warn at the caller).
    Busy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhaseError {
    /// The transition is illegal from the current phase.
    WrongPhase(GenerationPhase),
    /// The caller's generation is stale.
    GenerationMismatch,
    Busy,
}

/// The single authoritative generation's state. All fields are atomics; the
/// `guard` CAS serializes writers without blocking (contended writers fail
/// closed and report [`ArmOutcome::Busy`]/[`TransitionOutcome::Busy`]).
pub struct LifecycleState {
    guard: AtomicBool,
    phase: AtomicU8,
    generation: AtomicU64,
    next_generation: AtomicU64,
    requested_percent: AtomicI32,
    /// The armed generation's DSP mode (see [`ArmRequest::preserve_pitch`]).
    preserve_pitch: AtomicBool,
    /// The armed generation's sync-background-video flag (see
    /// [`ArmRequest::sync_movie`]). Consulted by the commit's movie
    /// confirmation (skip re-asserting suppression) and by the
    /// movie-rate-directive accessor `movie_sync` reads at graph open.
    sync_movie: AtomicBool,
    participant_mask: AtomicU8,
    stage_index: AtomicI32,
    /// The 64-bit song-code digest the generation is bound to (0 = none).
    /// Arming is song-agnostic, so the generation binds to its song at the
    /// FIRST successful bind; the binding preflight then refuses any other
    /// dance bank — a rate must never commit against a song the binding
    /// never served.
    bound_song_digest: AtomicU64,
}

/// RAII guard-release so every early return unlocks.
struct WriterGuard<'a>(&'a AtomicBool);

impl Drop for WriterGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl LifecycleState {
    pub const fn new() -> Self {
        Self {
            guard: AtomicBool::new(false),
            phase: AtomicU8::new(GenerationPhase::Identity as u8),
            generation: AtomicU64::new(0),
            next_generation: AtomicU64::new(1),
            requested_percent: AtomicI32::new(100),
            preserve_pitch: AtomicBool::new(true),
            sync_movie: AtomicBool::new(false),
            participant_mask: AtomicU8::new(0),
            stage_index: AtomicI32::new(-1),
            bound_song_digest: AtomicU64::new(0),
        }
    }

    /// Bind the generation to the song whose bank the binding preflight
    /// validated (set at the first bind of the generation). `digest` must be
    /// nonzero.
    pub fn bind_song(&self, digest: u64) {
        self.bound_song_digest.store(digest, Ordering::Release);
    }

    /// The bound song digest, or `None` while unbound.
    #[must_use]
    pub fn bound_song(&self) -> Option<u64> {
        match self.bound_song_digest.load(Ordering::Acquire) {
            0 => None,
            digest => Some(digest),
        }
    }

    fn try_lock(&self) -> Option<WriterGuard<'_>> {
        self.guard
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| WriterGuard(&self.guard))
    }

    #[must_use]
    pub fn phase(&self) -> GenerationPhase {
        GenerationPhase::from_u8(self.phase.load(Ordering::Acquire))
    }

    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    #[must_use]
    pub fn participant_mask(&self) -> u8 {
        self.participant_mask.load(Ordering::Acquire)
    }

    #[must_use]
    pub fn stage_index(&self) -> i32 {
        self.stage_index.load(Ordering::Acquire)
    }

    #[must_use]
    pub fn requested_percent(&self) -> i32 {
        self.requested_percent.load(Ordering::Acquire)
    }

    /// The armed generation's preserve-pitch flag (true = pitch-preserved
    /// stretch; the identity default).
    #[must_use]
    pub fn preserve_pitch(&self) -> bool {
        self.preserve_pitch.load(Ordering::Acquire)
    }

    /// The armed generation's sync-background-video flag (false = the
    /// shipped suppression behavior; the identity default).
    #[must_use]
    pub fn sync_movie(&self) -> bool {
        self.sync_movie.load(Ordering::Acquire)
    }

    fn set_phase(&self, phase: GenerationPhase) {
        self.phase.store(phase as u8, Ordering::Release);
    }

    /// Apply a scene-26 eligibility decision. Nonblocking; the caller
    /// classifies first (input gathering happens outside this state).
    pub fn on_scene26(
        &self,
        sink: &dyn LifecycleSink,
        decision: EligibilityDecision,
    ) -> ArmOutcome {
        let Some(_guard) = self.try_lock() else {
            return ArmOutcome::Busy;
        };
        if self.phase() == GenerationPhase::XactInFlight {
            // Uncancellable: a later arm waits for the native call to
            // resolve. Nothing (movie included) may change.
            return ArmOutcome::Deferred;
        }
        match decision {
            EligibilityDecision::Identity(reason) => {
                self.set_phase(GenerationPhase::Identity);
                self.bound_song_digest.store(0, Ordering::Release);
                self.preserve_pitch.store(true, Ordering::Release);
                self.sync_movie.store(false, Ordering::Release);
                sink.set_movie_suppressed(false);
                sink.reset_identity();
                ArmOutcome::Identity(reason)
            }
            EligibilityDecision::Arm(request) => {
                let generation = self.next_generation.fetch_add(1, Ordering::AcqRel);
                self.generation.store(generation, Ordering::Release);
                self.requested_percent
                    .store(request.requested_percent, Ordering::Release);
                self.preserve_pitch
                    .store(request.preserve_pitch, Ordering::Release);
                self.sync_movie.store(request.sync_movie, Ordering::Release);
                self.participant_mask
                    .store(request.participant_mask, Ordering::Release);
                self.stage_index
                    .store(request.stage_index, Ordering::Release);
                // The new generation is unbound until the binding preflight
                // binds a specific song's bank.
                self.bound_song_digest.store(0, Ordering::Release);
                self.set_phase(GenerationPhase::Armed);
                // Non-identity: tentative suppression BEFORE stage
                // construction can call BuildGraph (design req 42) — UNLESS
                // the entered side latched SYNC BACKGROUND VIDEO (effective:
                // movie-sync engine available): then the graph builds
                // normally and `movie_sync` rate-locks it at open.
                // Identity (training) arms: NO movie suppression — the
                // clock and audio stay 1:1, so the DirectShow graph can
                // follow (training design §4.5); the explicit clear also
                // flushes stale suppression from a prior attempt. The
                // clock remains identity either way.
                sink.set_movie_suppressed(
                    request.requested_percent != IDENTITY_PERCENT && !request.sync_movie,
                );
                sink.publish_identity(generation, request.participant_mask);
                ArmOutcome::Armed { generation }
            }
        }
    }

    /// Apply the definitive scene rules for every non-scene-26 transition.
    pub fn on_transition(
        &self,
        sink: &dyn LifecycleSink,
        prev: i32,
        next: i32,
    ) -> TransitionOutcome {
        let Some(_guard) = self.try_lock() else {
            return TransitionOutcome::Busy;
        };
        let phase = self.phase();

        // GAMEPLAY → GAMEPLAY: Quick Restart. Retain generation, Q31, movie
        // policy, and (elsewhere) pending score counts; the bank re-create
        // serves the same generation again (regeneration, not reuse).
        if prev == scene::GAMEPLAY && next == scene::GAMEPLAY {
            return TransitionOutcome::QuickRestartRetained;
        }

        // Gameplay unexpectedly starting while LateFailed: the aborted load
        // proceeded anyway — the caller must taint before trusting anything.
        // State intentionally stays LateFailed (identity clock, movie held).
        if next == scene::GAMEPLAY && phase == GenerationPhase::LateFailed {
            return TransitionOutcome::GameplayEnteredLateFailed;
        }

        // GAMEPLAY → any non-GAMEPLAY: the attempt's definitive boundary.
        if prev == scene::GAMEPLAY {
            return match phase {
                GenerationPhase::Armed
                | GenerationPhase::Binding
                | GenerationPhase::Committed
                | GenerationPhase::EarlyFailed => {
                    // Q31 identity FIRST, then the movie contributor clears.
                    sink.reset_identity();
                    sink.set_movie_suppressed(false);
                    self.set_phase(GenerationPhase::Completed);
                    TransitionOutcome::CompletedAttempt
                }
                GenerationPhase::XactInFlight => TransitionOutcome::XactStillInFlight,
                // Identity / Completed / LateFailed: the clock is already
                // identity; LateFailed keeps its movie suppression until the
                // next clean selection or session reset.
                _ => TransitionOutcome::NoChange,
            };
        }

        // Title/attract/new-session reset: force runtime identity and clear
        // non-score generation state (score state is owned elsewhere).
        if (ATTRACT_SCENE_MIN..=ATTRACT_SCENE_MAX).contains(&next) {
            return match phase {
                GenerationPhase::Identity => TransitionOutcome::NoChange,
                GenerationPhase::XactInFlight => TransitionOutcome::XactStillInFlight,
                _ => {
                    self.set_phase(GenerationPhase::Identity);
                    sink.reset_identity();
                    sink.set_movie_suppressed(false);
                    TransitionOutcome::ForcedIdentity
                }
            };
        }

        // A pre-binding attempt leaving the 26→27→28 corridor without
        // reaching gameplay was abandoned (e.g. an interstitial bailed back
        // to song select).
        if matches!(phase, GenerationPhase::Armed | GenerationPhase::Binding)
            && !matches!(
                next,
                scene::SONG_TO_STAGE_INTERSTITIAL | scene::STAGE_INDICATOR | scene::GAMEPLAY
            )
        {
            self.set_phase(GenerationPhase::Identity);
            sink.reset_identity();
            sink.set_movie_suppressed(false);
            return TransitionOutcome::AbandonedPreExposure;
        }

        TransitionOutcome::NoChange
    }

    // ── Phase entry points for the transaction wiring ────────────────
    //
    // These validate phase/generation legality only. The commit ordering
    // effects (score protection → movie confirmation → snapshot → Q31-last)
    // are wired by the transaction integration, not here.

    fn advance(
        &self,
        generation: u64,
        expected: GenerationPhase,
        to: GenerationPhase,
    ) -> Result<(), PhaseError> {
        let Some(_guard) = self.try_lock() else {
            return Err(PhaseError::Busy);
        };
        if self.generation() != generation {
            return Err(PhaseError::GenerationMismatch);
        }
        let phase = self.phase();
        if phase != expected {
            return Err(PhaseError::WrongPhase(phase));
        }
        self.set_phase(to);
        Ok(())
    }

    /// Armed → Binding (the create detour's pre-original preflight began
    /// binding a qualifying dance bank).
    pub fn begin_binding(&self, generation: u64) -> Result<(), PhaseError> {
        self.advance(generation, GenerationPhase::Armed, GenerationPhase::Binding)
    }

    /// Binding → XactInFlight (the binding was published and the original
    /// entered — uncancellable until XACT resolves).
    pub fn mark_exposed(&self, generation: u64) -> Result<(), PhaseError> {
        self.advance(
            generation,
            GenerationPhase::Binding,
            GenerationPhase::XactInFlight,
        )
    }

    /// Committed → XactInFlight for the SAME generation: idempotent bank
    /// re-create when a supported build reloads the slot-5 bank (Quick
    /// Restart, design req 5 — the same generation is served again from
    /// offset zero; regeneration, not reuse). The subsequent commit re-runs
    /// idempotently (the pending-save ledger dedups by generation and the
    /// snapshot republishes identical values).
    pub fn mark_reexposed(&self, generation: u64) -> Result<(), PhaseError> {
        self.advance(
            generation,
            GenerationPhase::Committed,
            GenerationPhase::XactInFlight,
        )
    }

    /// XactInFlight → Committed (`wavebank_create` accepted the bank).
    pub fn mark_committed(&self, generation: u64) -> Result<(), PhaseError> {
        self.advance(
            generation,
            GenerationPhase::XactInFlight,
            GenerationPhase::Committed,
        )
    }

    /// XactInFlight → LateFailed (XACT rejected after the binding was
    /// published). Movie suppression and identity clock retention are the
    /// caller's transaction contract; the phase transition itself changes
    /// nothing else.
    pub fn mark_late_failed(&self, generation: u64) -> Result<(), PhaseError> {
        self.advance(
            generation,
            GenerationPhase::XactInFlight,
            GenerationPhase::LateFailed,
        )
    }

    /// Armed/Binding → EarlyFailed (stock-100% fallback before a binding was
    /// published). The tentative movie suppression deliberately STAYS
    /// through the attempt.
    pub fn mark_early_failed(&self, generation: u64) -> Result<(), PhaseError> {
        let Some(_guard) = self.try_lock() else {
            return Err(PhaseError::Busy);
        };
        if self.generation() != generation {
            return Err(PhaseError::GenerationMismatch);
        }
        let phase = self.phase();
        if !matches!(phase, GenerationPhase::Armed | GenerationPhase::Binding) {
            return Err(PhaseError::WrongPhase(phase));
        }
        self.set_phase(GenerationPhase::EarlyFailed);
        Ok(())
    }
}

impl Default for LifecycleState {
    fn default() -> Self {
        Self::new()
    }
}
