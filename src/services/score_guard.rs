//! Score Submission Guard — shared taint state for keeping fraudulent or
//! incomplete scores out of the eamuse backend.
//!
//! This module is the single source of truth for the question "how must this
//! side's server save be treated right now?" It holds pure lock-free atomic
//! state and **owns no detour** — the enforcement itself happens in
//! `custom_options_persistence`'s `save_sender` trampoline, which is the one
//! detour already installed on the ess.dll profile-save path
//! (one-detour-per-target; see CLAUDE.md rule 5). Keeping the taint state here,
//! separate from that detour, lets two unrelated mods (`autoplay` and
//! `quick_restart_or_fail`) feed it without either owning the save hook.
//!
//! ## Taint model
//!
//! A play is "tainted" — and its score must not reach the eamuse backend —
//! when it was either **faked** (Autoplay enabled for that side) or
//! **incomplete** (a press-`3` Quick Failure, which fails out *both* sides at
//! once), or otherwise **assisted**: a Training Mode session altered the song
//! (a section bound engaged, a marker was set, or a seek fired — R5), or the
//! side played it with Assist Tick claps enabled. The profile save the game
//! performs is per-side, and there are two
//! save moments that carry score data (see `research/score-submission-re.md`):
//!
//!   * **Per-stage save** (`savekind == STAGE`, fired right after each song):
//!     carries just that song's `/result` block. Suppressed outright for side
//!     X iff X autoplayed this song OR a quick-fail fired this song.
//!   * **Logout save** (`savekind == LOGOUT`, fired at card-out): re-bundles
//!     *all* of the session's stages' results in one request — but it is also
//!     the ONLY save that carries the profile/customize write-back. A tainted
//!     side's logout save is therefore **sanitised, not suppressed**: on entry
//!     to the EAM_EXIT scene the sanitiser virginises the side's play records
//!     (`mcode = -1` — the marshal's skip key, so the stage list comes out
//!     empty) and the trampoline strips the `<league>` accumulator node from
//!     the request, then forwards it so the profile data persists. Clean
//!     stages were already uploaded by their own per-stage saves; the backend
//!     ignores regular-song results in the logout payload anyway. If any
//!     sanitiser piece could not arm (record layout decode failed, or the
//!     league-removal ordinal is missing), the save falls back to today's
//!     full suppression — fail closed on score integrity.
//!
//! Hence three tiers of state:
//!   * Per-song taint (`AUTOPLAY_TAINT`, `QUICK_FAIL_TAINT`,
//!     `TRAINING_TAINT`, `ASSIST_TICK_TAINT`) — gates the per-stage save;
//!     reset at each gameplay entry / quick restart (the level-written
//!     mirrors — autoplay and assist-tick — are refreshed by their own mods
//!     instead of cleared here).
//!   * Session-sticky taint (`SESSION_TAINTED`, read via [`logout_taint`]) —
//!     marks the side's logout save as needing sanitisation; reset only at
//!     card-in / new session.
//!   * Sanitised flag (`SANITISED`, via [`mark_logout_sanitised`] /
//!     [`was_logout_sanitised`]) — did the EAM_EXIT-entry sanitiser actually
//!     virginise this side's records this session? The trampoline forwards a
//!     tainted logout save only when this is set (and the league strip is
//!     available); otherwise it suppresses.
//!
//! The session-sticky flag is latched at the moment a per-stage save is
//! *actually suppressed* (via [`mark_session_tainted`]), not when a trigger is
//! merely armed. This matters because Autoplay can be toggled on and off in the
//! menu without ever playing a song: tying the session flag to a real
//! suppression keeps an otherwise-honest session's card-out save from being
//! needlessly sanitised.
//!
//! ## Song-rate pending stage saves (Song Playback Speed)
//!
//! A committed non-100% song is assisted play: its per-stage save must be
//! suppressed and its logout save sanitised. Unlike the Autoplay/Quick-Fail
//! flags, rate taint is a per-side *ledger* of pending stage saves
//! ([`RateSaveLedger`]) with exact (generation, stage) identity, because ESS
//! saves can be delayed, reordered, or retried: a pending entry must be
//! consumed by exactly the save it belongs to, and any ambiguity must fail
//! closed (suppress without consuming) rather than let a tainted score slip
//! through or eat an honest one's suppression slot. Scene changes never clear
//! the ledger; only a positively side-matched card-in reset does
//! ([`reset_rate_state_for_side`], called from the deferred profile-load path
//! in `custom_options_persistence`). The song-rate commit path appends
//! entries; nothing appends while the feature's transaction is inert.
//!
//! ## Failure mode (readiness)
//!
//! `HOOK_INSTALLED` is latched true by `custom_options_persistence::init()`
//! only when the ess `save_sender` detour actually installs. `autoplay` reads
//! `is_available()` and **refuses to enable** when it is false (fail-closed —
//! an autoplayed score is fabricated and must never be producible if we can't
//! guarantee suppression). `quick_restart_or_fail` does not gate on it
//! (fail-open — an incomplete score is lower-risk); its taint writes are simply
//! inert no-ops when no detour is installed to read them.
//!
//! Song Playback Speed requires the stronger
//! [`is_full_sanitization_available`]: its accepted logout behaviour is
//! sanitise-and-forward (profile persists, competitive data stripped), which
//! needs the save detour AND the decoded stage/course record layout AND the
//! scene manager AND the registered EAM_EXIT sanitiser AND the league-strip
//! ordinal. Missing any piece keeps the feature unavailable.
//!
//! ## Thread safety
//!
//! Writers run on the input / scene / render threads; the readers
//! (`is_stage_suppressed` / `logout_taint` / `was_logout_sanitised`) run on the
//! ess save thread. All state is plain atomics (writers `Release`, readers
//! `Acquire`), so no `Mutex` is needed and none is held across any callback
//! (CLAUDE.md rule 6). Every `side`-indexed access is bounds-guarded so nothing
//! panics across the FFI-reachable read path (CLAUDE.md rule 1).

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, AtomicU8, Ordering};

/// Number of player sides (P1, P2).
const SIDES: usize = 2;

/// True once the ess.dll `save_sender` detour is confirmed installed.
/// Latched by `custom_options_persistence::init()` after a successful hook so
/// `autoplay` can fail-closed against it via [`is_available`].
static HOOK_INSTALLED: AtomicBool = AtomicBool::new(false);

/// Per-side Autoplay taint for the current song. A live mirror of `autoplay`'s
/// own per-side enable flag, kept here so the save trampoline has a single
/// taint authority and never reaches into another module's internals. Read at
/// per-stage save time (design R4: "autoplay read live at save time").
static AUTOPLAY_TAINT: [AtomicBool; SIDES] = [AtomicBool::new(false), AtomicBool::new(false)];

/// Quick-fail taint for the current song. A press-`3` gesture fails out both
/// sides at once, so this single flag forces suppression of the per-stage save
/// for *both* sides. Set when the gesture fires; cleared at gameplay (re)entry.
static QUICK_FAIL_TAINT: AtomicBool = AtomicBool::new(false);

/// Per-side Training Mode taint for the current song (design R5). One-way and
/// idempotent: set by the training mod whenever the session alters the played
/// song — a section bound engages at gameplay entry, an A/B marker gesture
/// fires, or a seek lands at t > 0 (producers wired in
/// `mods/training_mode`). Cleared per song by [`reset_song_taint`] (the
/// honest-replay contract: a press-1 restart of an untouched song submits;
/// the training mod's `on_song_reset(t>0)` subscriber re-taints when the
/// restart actually seeks to a marker) and per session by [`reset_session`].
static TRAINING_TAINT: [AtomicBool; SIDES] = [AtomicBool::new(false), AtomicBool::new(false)];

/// Per-side Assist Tick taint for the current song (design R5 — a deliberate
/// behavior change to the shipped mod: a side that played with claps enabled
/// must not submit its score). A level-written mirror of `assist_tick`'s
/// per-song enable latch, exactly the autoplay model: the mod writes true AND
/// false for both sides at every GAMEPLAY entry. Deliberately NOT cleared by
/// [`reset_song_taint`] — that fires from `quick_restart_or_fail`'s scene
/// callback on the same scene change that drives assist_tick's latch, and
/// scene-callback ordering across mods is unspecified; the level writes make
/// any ordering correct. Cleared at card-in by [`reset_session`].
static ASSIST_TICK_TAINT: [AtomicBool; SIDES] = [AtomicBool::new(false), AtomicBool::new(false)];

/// Session-sticky per-side taint: has *any* stage this session been tainted
/// (by either trigger) for this side? Marks the side's card-out logout save as
/// needing sanitisation (design R8, amended by D21–D26). Latched on first
/// taint; cleared only on card-in / new session via [`reset_session`].
static SESSION_TAINTED: [AtomicBool; SIDES] = [AtomicBool::new(false), AtomicBool::new(false)];

/// Per-side "the EAM_EXIT-entry sanitiser virginised this side's records this
/// session" flag. Set by the sanitiser in `custom_options_persistence`; read
/// by the save trampoline to decide sanitise-and-forward vs suppress. Cleared
/// on card-in via [`reset_session`].
static SANITISED: [AtomicBool; SIDES] = [AtomicBool::new(false), AtomicBool::new(false)];

// ── Readiness (R6) ───────────────────────────────────────────────────

/// Latch that the ess `save_sender` detour installed successfully. Called once
/// from `custom_options_persistence::init()` on the success path only.
pub fn mark_hook_installed() {
    HOOK_INSTALLED.store(true, Ordering::Release);
}

/// Whether the suppression hook is live. `autoplay::enable()` consults this to
/// fail closed (refuse to enable when suppression can't be guaranteed).
pub fn is_available() -> bool {
    HOOK_INSTALLED.load(Ordering::Acquire)
}

// ── Full-sanitization readiness (Song Playback Speed) ────────────────

/// Boot-latched prerequisites for the sanitise-and-forward logout policy,
/// beyond the bare save detour. Each flag is latched once by the service that
/// proves the capability; none is ever cleared (capabilities are static for
/// the process lifetime). Kept as an injectable struct so host tests can
/// exercise the conjunction without touching process-global state.
pub struct SanitizationReadiness {
    /// `stage_records` decoded the per-stage + course record layout.
    stage_records: AtomicBool,
    /// `scene_manager`'s transition hook installed.
    scene_manager: AtomicBool,
    /// The EAM_EXIT logout-record sanitiser scene callback is registered.
    sanitiser_registered: AtomicBool,
    /// libavs Ordinal 164 (`property_node_remove`, the league strip) resolved.
    league_strip: AtomicBool,
}

impl SanitizationReadiness {
    pub const fn new() -> Self {
        Self {
            stage_records: AtomicBool::new(false),
            scene_manager: AtomicBool::new(false),
            sanitiser_registered: AtomicBool::new(false),
            league_strip: AtomicBool::new(false),
        }
    }

    pub fn mark_stage_records_ready(&self) {
        self.stage_records.store(true, Ordering::Release);
    }

    pub fn mark_scene_manager_ready(&self) {
        self.scene_manager.store(true, Ordering::Release);
    }

    pub fn mark_sanitiser_registered(&self) {
        self.sanitiser_registered.store(true, Ordering::Release);
    }

    pub fn mark_league_strip_available(&self) {
        self.league_strip.store(true, Ordering::Release);
    }

    #[must_use]
    pub fn league_strip_available(&self) -> bool {
        self.league_strip.load(Ordering::Acquire)
    }

    /// The full conjunction, given the externally owned save-detour flag.
    #[must_use]
    pub fn is_complete(&self, save_detour_installed: bool) -> bool {
        save_detour_installed
            && self.stage_records.load(Ordering::Acquire)
            && self.scene_manager.load(Ordering::Acquire)
            && self.sanitiser_registered.load(Ordering::Acquire)
            && self.league_strip.load(Ordering::Acquire)
    }
}

impl Default for SanitizationReadiness {
    fn default() -> Self {
        Self::new()
    }
}

static SANITIZATION_READINESS: SanitizationReadiness = SanitizationReadiness::new();

/// Latched by `lib.rs` after a successful `stage_records::init`.
pub fn mark_stage_records_ready() {
    SANITIZATION_READINESS.mark_stage_records_ready();
}

/// Latched by `lib.rs` after a successful `scene_manager::init`.
pub fn mark_scene_manager_ready() {
    SANITIZATION_READINESS.mark_scene_manager_ready();
}

/// Latched by `custom_options_persistence::register_logout_sanitiser`.
pub fn mark_sanitiser_registered() {
    SANITIZATION_READINESS.mark_sanitiser_registered();
}

/// Latched by `custom_options_persistence::resolve_libavs_ordinals` when
/// Ordinal 164 (`property_node_remove`) resolves. Single source of truth —
/// the save trampoline's logout three-way policy reads it back through
/// [`league_strip_available`].
pub fn mark_league_strip_available() {
    SANITIZATION_READINESS.mark_league_strip_available();
}

/// Whether the `<data><league>` strip half of the logout sanitiser is armed.
pub fn league_strip_available() -> bool {
    SANITIZATION_READINESS.league_strip_available()
}

/// Stronger-than-[`is_available`] readiness required by Song Playback Speed:
/// the accepted behaviour for a rate-tainted logout is sanitise-and-forward,
/// not full suppression, so every sanitiser prerequisite must be present
/// before a non-100% generation may arm.
pub fn is_full_sanitization_available() -> bool {
    SANITIZATION_READINESS.is_complete(is_available())
}

// ── Logout league-strip semantics (Song Playback Speed R6) ───────────

/// Tri-state result of the logout `<data><league>` strip. The AVS return
/// status is never ignored: an absent node is safe (nothing to leak), a
/// successful removal is safe, and a removal *failure* means the built
/// request still carries the league accumulator and must not be forwarded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeagueStripOutcome {
    /// `<data><league>` was not present in the built request.
    NodeAbsent,
    /// The node was found and successfully removed.
    Removed,
    /// The node was found but `property_node_remove` reported failure.
    RemovalFailed,
}

/// Forward/fail-closed election for a sanitised logout save, given the strip
/// outcome. `false` means the trampoline must signal sender failure (return
/// 0) instead of letting the league-bearing request go out.
#[must_use]
pub fn logout_league_forward_allowed(outcome: LeagueStripOutcome) -> bool {
    match outcome {
        LeagueStripOutcome::NodeAbsent | LeagueStripOutcome::Removed => true,
        LeagueStripOutcome::RemovalFailed => false,
    }
}

// ── Pending rate-save ledger (Song Playback Speed) ───────────────────

/// Ring capacity per side — above the game's five per-stage records plus the
/// course record, so a legal session can never legitimately outgrow it.
const RATE_RING_SIZE: usize = 8;

/// `PendingRateSave.state` values. `Init` is a transient reservation while an
/// appender writes the identity fields; readers treat it as pending
/// (fail-closed) but never claim it.
const RATE_FREE: u8 = 0;
const RATE_INIT: u8 = 1;
const RATE_PENDING: u8 = 2;
const RATE_CLAIMED: u8 = 3;
const RATE_CONSUMED: u8 = 4;

/// One pending (or consumed-tombstone) rate-tainted stage save.
struct PendingRateSave {
    /// Song-rate generation id (0 = slot empty). Appends are idempotent per
    /// generation, which is what makes Quick Restart / idempotent recommit of
    /// the same generation unable to double-count.
    generation: AtomicU64,
    /// Scene-26 stage index the generation armed with.
    stage_index: AtomicI32,
    /// Per-side append order, for oldest-first claims.
    sequence: AtomicU64,
    state: AtomicU8,
}

impl PendingRateSave {
    const fn new() -> Self {
        Self {
            generation: AtomicU64::new(0),
            stage_index: AtomicI32::new(-1),
            sequence: AtomicU64::new(0),
            state: AtomicU8::new(RATE_FREE),
        }
    }
}

struct RateSideState {
    entries: [PendingRateSave; RATE_RING_SIZE],
    append_seq: AtomicU64,
    /// Sticky fail-closed flag: the ring overflowed (or an append was
    /// otherwise unrecordable), so this side's stage saves are all suppressed
    /// until a positively matched card-in reset.
    overflow: AtomicBool,
}

impl RateSideState {
    const fn new() -> Self {
        Self {
            entries: [const { PendingRateSave::new() }; RATE_RING_SIZE],
            append_seq: AtomicU64::new(1),
            overflow: AtomicBool::new(false),
        }
    }
}

/// Outcome of [`RateSaveLedger::append`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RateAppendOutcome {
    Appended,
    /// The generation is already recorded (Quick Restart / recommit dedup).
    AlreadyRecorded,
    /// No free slot — the side is now sticky fail-closed.
    Overflow,
    /// Rejected input (side out of range or the reserved generation 0).
    Invalid,
}

/// A claimed-but-not-yet-consumed pending entry (`Pending -> Claimed`).
/// Consume it with [`RateSaveLedger::consume`]; non-cloneable by design so a
/// claim maps to at most one consumption.
pub struct RateClaim {
    side: usize,
    slot: usize,
    pub generation: u64,
    pub stage_index: i32,
}

/// Why a stage save was suppressed without consuming a pending entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RateSuppressReason {
    /// The save's side could not be decoded while rate state exists anywhere.
    /// Never defaults to P1.
    UnknownSide,
    /// The save's stage could not be decoded while this side has pending
    /// entries.
    UnknownStage,
    /// This side has pending entries but none matches the decoded stage
    /// (reordered/delayed save) — blanket fail-closed until the exact save
    /// arrives.
    NoExactMatch,
    /// A Claimed/Consumed tombstone matches — duplicate sender retry.
    Duplicate,
    /// The side's ring overflowed; everything suppresses until card-in reset.
    Overflow,
}

/// Rate-policy election for one per-stage save.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RateSavePolicy {
    /// No rate state applies — the caller proceeds with the legacy
    /// (autoplay/quick-fail) policy, including its historical unknown-side
    /// default.
    NoRateOpinion,
    /// Suppress; exactly one matching pending entry was claimed and consumed.
    SuppressConsume { generation: u64, stage_index: i32 },
    /// Suppress without consuming anything (fail-closed ambiguity).
    SuppressNoConsume(RateSuppressReason),
}

/// Per-side pending rate-save rings. Kept as an instantiable struct so host
/// tests run against private instances; production state lives in the
/// [`RATE_LEDGER`] static behind the free-function API below.
///
/// Concurrency model: appends come from the song-rate commit path (one wave
/// thread), claims/consumes from the ess save thread, resets from the render
/// thread's deferred card-in path. Every transition is a CAS on the entry
/// `state`, so interleavings degrade to fail-closed outcomes (an entry reset
/// mid-claim simply makes the consume CAS a no-op).
pub struct RateSaveLedger {
    sides: [RateSideState; SIDES],
}

impl RateSaveLedger {
    pub const fn new() -> Self {
        Self {
            sides: [const { RateSideState::new() }; SIDES],
        }
    }

    /// Record one pending rate-tainted stage save. Idempotent per generation.
    pub fn append(&self, side: usize, generation: u64, stage_index: i32) -> RateAppendOutcome {
        if side >= SIDES || generation == 0 {
            return RateAppendOutcome::Invalid;
        }
        let state = &self.sides[side];
        for entry in &state.entries {
            if entry.state.load(Ordering::Acquire) != RATE_FREE
                && entry.generation.load(Ordering::Acquire) == generation
            {
                return RateAppendOutcome::AlreadyRecorded;
            }
        }
        for entry in &state.entries {
            if entry
                .state
                .compare_exchange(RATE_FREE, RATE_INIT, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                entry.generation.store(generation, Ordering::Release);
                entry.stage_index.store(stage_index, Ordering::Release);
                entry.sequence.store(
                    state.append_seq.fetch_add(1, Ordering::AcqRel),
                    Ordering::Release,
                );
                entry.state.store(RATE_PENDING, Ordering::Release);
                return RateAppendOutcome::Appended;
            }
        }
        state.overflow.store(true, Ordering::Release);
        RateAppendOutcome::Overflow
    }

    /// Pending entries (Init/Pending/Claimed — everything that must still
    /// suppress) for a side. Out-of-range side reads 0.
    #[must_use]
    pub fn pending_count(&self, side: usize) -> usize {
        if side >= SIDES {
            return 0;
        }
        self.sides[side]
            .entries
            .iter()
            .filter(|entry| {
                matches!(
                    entry.state.load(Ordering::Acquire),
                    RATE_INIT | RATE_PENDING | RATE_CLAIMED
                )
            })
            .count()
    }

    /// Sticky overflow fail-closed flag for a side.
    #[must_use]
    pub fn overflowed(&self, side: usize) -> bool {
        side < SIDES && self.sides[side].overflow.load(Ordering::Acquire)
    }

    /// Any pending entry or overflow flag on either side — the gate for the
    /// unknown-side fail-closed rule.
    #[must_use]
    pub fn any_rate_state(&self) -> bool {
        (0..SIDES).any(|side| self.pending_count(side) > 0 || self.overflowed(side))
    }

    /// Claim (Pending -> Claimed) the oldest pending entry matching
    /// (side, stage). Returns `None` when nothing matches.
    pub fn claim(&self, side: usize, stage_index: i32) -> Option<RateClaim> {
        if side >= SIDES {
            return None;
        }
        let state = &self.sides[side];
        loop {
            let mut best: Option<(usize, u64)> = None;
            for (slot, entry) in state.entries.iter().enumerate() {
                if entry.state.load(Ordering::Acquire) == RATE_PENDING
                    && entry.stage_index.load(Ordering::Acquire) == stage_index
                {
                    let sequence = entry.sequence.load(Ordering::Acquire);
                    if best.is_none_or(|(_, s)| sequence < s) {
                        best = Some((slot, sequence));
                    }
                }
            }
            let Some((slot, _)) = best else {
                return None;
            };
            let entry = &state.entries[slot];
            if entry
                .state
                .compare_exchange(
                    RATE_PENDING,
                    RATE_CLAIMED,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_ok()
            {
                return Some(RateClaim {
                    side,
                    slot,
                    generation: entry.generation.load(Ordering::Acquire),
                    stage_index,
                });
            }
            // Lost a race on that slot; rescan.
        }
    }

    /// Consume a claim (Claimed -> Consumed). A no-op if a reset raced the
    /// claim (the entry is gone; the save was suppressed regardless).
    pub fn consume(&self, claim: RateClaim) {
        let entry = &self.sides[claim.side].entries[claim.slot];
        let _ = entry.state.compare_exchange(
            RATE_CLAIMED,
            RATE_CONSUMED,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }

    /// Does a Claimed/Consumed tombstone for this stage exist? (Duplicate
    /// sender retries of an already-consumed save keep suppressing.)
    #[must_use]
    fn has_tombstone(&self, side: usize, stage_index: i32) -> bool {
        if side >= SIDES {
            return false;
        }
        self.sides[side].entries.iter().any(|entry| {
            matches!(
                entry.state.load(Ordering::Acquire),
                RATE_CLAIMED | RATE_CONSUMED
            ) && entry.stage_index.load(Ordering::Acquire) == stage_index
        })
    }

    /// The complete rate election for one per-stage save, given the decoded
    /// (possibly unknown) side and stage identity. See [`RateSavePolicy`].
    pub fn elect(&self, side: Option<usize>, stage_index: Option<i32>) -> RateSavePolicy {
        let side = match side {
            Some(side) if side < SIDES => side,
            _ => {
                // Unknown side: fail closed while ANY rate state exists —
                // never default to P1, never consume anything.
                return if self.any_rate_state() {
                    RateSavePolicy::SuppressNoConsume(RateSuppressReason::UnknownSide)
                } else {
                    RateSavePolicy::NoRateOpinion
                };
            }
        };
        if self.overflowed(side) {
            return RateSavePolicy::SuppressNoConsume(RateSuppressReason::Overflow);
        }
        let pending = self.pending_count(side);
        let Some(stage_index) = stage_index else {
            return if pending > 0 {
                RateSavePolicy::SuppressNoConsume(RateSuppressReason::UnknownStage)
            } else {
                RateSavePolicy::NoRateOpinion
            };
        };
        if let Some(claim) = self.claim(side, stage_index) {
            let (generation, stage_index) = (claim.generation, claim.stage_index);
            self.consume(claim);
            return RateSavePolicy::SuppressConsume {
                generation,
                stage_index,
            };
        }
        if self.has_tombstone(side, stage_index) {
            return RateSavePolicy::SuppressNoConsume(RateSuppressReason::Duplicate);
        }
        if pending > 0 {
            return RateSavePolicy::SuppressNoConsume(RateSuppressReason::NoExactMatch);
        }
        RateSavePolicy::NoRateOpinion
    }

    /// Clear one side's ring and overflow flag. Called ONLY from the
    /// positively side-matched card-in reset path — a P2 load can never erase
    /// P1 state. States go Free first so racing claims fail cleanly before
    /// the identity fields are cleared.
    pub fn reset_side(&self, side: usize) {
        if side >= SIDES {
            return;
        }
        let state = &self.sides[side];
        for entry in &state.entries {
            entry.state.store(RATE_FREE, Ordering::Release);
            entry.generation.store(0, Ordering::Release);
            entry.stage_index.store(-1, Ordering::Release);
            entry.sequence.store(0, Ordering::Release);
        }
        state.overflow.store(false, Ordering::Release);
    }
}

impl Default for RateSaveLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// Process-global ledger consumed by the save trampoline and the song-rate
/// commit path.
static RATE_LEDGER: RateSaveLedger = RateSaveLedger::new();

/// Borrow the process-global ledger (the song-rate transaction takes it as
/// an injected part so its host tests can run against local instances).
pub fn rate_ledger() -> &'static RateSaveLedger {
    &RATE_LEDGER
}

/// Record one pending rate-tainted stage save (song-rate commit path;
/// idempotent per generation).
pub fn append_pending_rate_save(
    side: usize,
    generation: u64,
    stage_index: i32,
) -> RateAppendOutcome {
    RATE_LEDGER.append(side, generation, stage_index)
}

/// Pending rate-save count for a side (feeds [`is_stage_suppressed`]).
pub fn pending_rate_count(side: usize) -> usize {
    RATE_LEDGER.pending_count(side)
}

/// Sticky ring-overflow fail-closed flag for a side.
pub fn rate_overflowed(side: usize) -> bool {
    RATE_LEDGER.overflowed(side)
}

/// Any pending rate state (either side) — unknown-side saves fail closed
/// while this is true.
pub fn any_pending_rate_state() -> bool {
    RATE_LEDGER.any_rate_state()
}

/// Rate election for one per-stage save (save trampoline).
pub fn elect_rate_save_policy(side: Option<usize>, stage_index: Option<i32>) -> RateSavePolicy {
    RATE_LEDGER.elect(side, stage_index)
}

/// Positively side-matched card-in reset of the rate ledger.
pub fn reset_rate_state_for_side(side: usize) {
    RATE_LEDGER.reset_side(side);
}

// ── Taint writers (called by the trigger mods / scene callbacks) ─────

/// Mirror Autoplay's per-side enable state. Called from `autoplay`'s option
/// change callback. Only updates the per-song flag — the session-sticky flag is
/// latched later, when a save is actually suppressed, so toggling autoplay on
/// and off in the menu without playing leaves the session clean.
pub fn set_autoplay_taint(side: usize, on: bool) {
    if side >= SIDES {
        return;
    }
    AUTOPLAY_TAINT[side].store(on, Ordering::Release);
}

/// Record that a Quick Failure fired this song. Forces the per-stage save of
/// both sides to be suppressed. The session-sticky flag is latched when those
/// saves are actually suppressed, not here.
pub fn set_quick_fail() {
    QUICK_FAIL_TAINT.store(true, Ordering::Release);
}

/// Record that a training session altered this side's current song (a section
/// bound engaged at gameplay entry, an A/B marker was set, or a seek fired at
/// t > 0 — see [`TRAINING_TAINT`]; producers live in `mods/training_mode`,
/// wired in Step 5 task-02). One-way per song and idempotent; cleared by
/// [`reset_song_taint`] / [`reset_session`]. Out-of-range `side` is ignored.
pub fn set_training_taint(side: usize) {
    if side >= SIDES {
        return;
    }
    TRAINING_TAINT[side].store(true, Ordering::Release);
}

/// Mirror Assist Tick's per-side per-song enable latch (the autoplay model —
/// level-written true or false for both sides at every GAMEPLAY entry; see
/// [`ASSIST_TICK_TAINT`] for why [`reset_song_taint`] must not touch it).
/// Out-of-range `side` is ignored.
pub fn set_assist_tick_taint(side: usize, on: bool) {
    if side >= SIDES {
        return;
    }
    ASSIST_TICK_TAINT[side].store(on, Ordering::Release);
}

/// Latch the session-sticky taint for a side. Called at the moment a per-stage
/// save is actually suppressed, so the matching card-out logout save (which
/// re-bundles every stage's result) is sanitised (or, failing that,
/// suppressed) too.
pub fn mark_session_tainted(side: usize) {
    if side >= SIDES {
        return;
    }
    SESSION_TAINTED[side].store(true, Ordering::Release);
}

/// Record that the EAM_EXIT-entry sanitiser virginised this side's play
/// records (array + course record) this session. Called by the logout-save
/// sanitiser in `custom_options_persistence` after its writes succeed.
pub fn mark_logout_sanitised(side: usize) {
    if side >= SIDES {
        return;
    }
    SANITISED[side].store(true, Ordering::Release);
}

/// Did the sanitiser actually run for this side this session? The save
/// trampoline forwards a tainted logout save only when this is true (and the
/// league strip is available) — otherwise it falls back to full suppression.
/// Out-of-range `side` reads as not-sanitised (fail closed).
pub fn was_logout_sanitised(side: usize) -> bool {
    if side >= SIDES {
        return false;
    }
    SANITISED[side].load(Ordering::Acquire)
}

/// Reset the per-song taint at the start of a fresh gameplay (or on quick
/// restart). Clears the quick-fail flag and both sides' training taint — the
/// honest-replay contract: a restarted untouched song reads clean, and the
/// training mod's `on_song_reset(t>0)` subscriber re-taints when the restart
/// actually seeks into a marked section. The autoplay mirror is updated live
/// by `set_autoplay_taint`, the assist-tick mirror is level-written at every
/// gameplay entry by its own mod (and MUST NOT be cleared here — see
/// [`ASSIST_TICK_TAINT`]), and the session-sticky flag must persist across
/// songs (only `reset_session` clears it).
pub fn reset_song_taint() {
    QUICK_FAIL_TAINT.store(false, Ordering::Release);
    for side in TRAINING_TAINT.iter() {
        side.store(false, Ordering::Release);
    }
}

/// Reset the session-sticky taint (and the sanitised flags) at the start of a
/// new player session (card-in). After this, a clean session uploads normally
/// even if the previous session was tainted.
///
/// The song-rate pending-save ledger deliberately does NOT ride this broad
/// reset (it fires for both sides on any profile load): rate state is cleared
/// per side only after a successful load is positively matched to that side —
/// see [`reset_rate_state_for_side`] and the deferred card-in path in
/// `custom_options_persistence`.
pub fn reset_session() {
    for side in SESSION_TAINTED.iter() {
        side.store(false, Ordering::Release);
    }
    for side in SANITISED.iter() {
        side.store(false, Ordering::Release);
    }
    for side in TRAINING_TAINT.iter() {
        side.store(false, Ordering::Release);
    }
    for side in ASSIST_TICK_TAINT.iter() {
        side.store(false, Ordering::Release);
    }
}

// ── Taint readers (called by the ess save_sender trampoline) ─────────

/// Per-stage save (`savekind == STAGE`): is this side's just-played song
/// tainted? True if a quick-fail fired this song, this side had Autoplay
/// enabled, a training session altered the song for this side, this side
/// played with Assist Tick claps enabled, or this side has outstanding
/// rate-tainted stage saves (pending ledger entries or the overflow
/// fail-closed flag — the blanket backstop that keeps a mis-decoded rate
/// save from slipping through while any rate state is unresolved).
/// Out-of-range `side` reads as not-suppressed (fail-open on decode; never
/// block a save we can't positively classify — the rate ledger's own
/// unknown-side rule is enforced separately by [`elect_rate_save_policy`]).
pub fn is_stage_suppressed(side: usize) -> bool {
    if side >= SIDES {
        return false;
    }
    QUICK_FAIL_TAINT.load(Ordering::Acquire)
        || AUTOPLAY_TAINT[side].load(Ordering::Acquire)
        || TRAINING_TAINT[side].load(Ordering::Acquire)
        || ASSIST_TICK_TAINT[side].load(Ordering::Acquire)
        || RATE_LEDGER.pending_count(side) > 0
        || RATE_LEDGER.overflowed(side)
}

/// Autoplay taint ALONE (no quick-fail / training / assist-tick / rate
/// bits). The auto-calibration apply guard reads this: autoplay's ~0 ms
/// machine-timed steps would wipe out the measurement, but the other taints
/// are score-policy concerns whose steps are humanly real — a quick-failed
/// or training-scrubbed calibration song still measures honestly.
/// Out-of-range `side` reads as clean.
pub fn is_autoplay_tainted(side: usize) -> bool {
    if side >= SIDES {
        return false;
    }
    AUTOPLAY_TAINT[side].load(Ordering::Acquire)
}

/// Logout save (`savekind == LOGOUT`): did any stage taint this side this
/// session? A true here means the side's logout save must be sanitised
/// (score-stripped, profile forwarded) — or suppressed if the sanitiser
/// couldn't arm. Out-of-range `side` reads as clean.
pub fn logout_taint(side: usize) -> bool {
    if side >= SIDES {
        return false;
    }
    SESSION_TAINTED[side].load(Ordering::Acquire)
}
