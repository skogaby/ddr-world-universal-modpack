//! The exactly-once wave-bank create transaction: TLS frame discipline,
//! commit/late-fail ordering, and exact-token recovery.
//!
//! This module is the pure heart of the non-100% commit protocol (design
//! §Wave-Bank Hook, §Published Snapshot). The windows detour wraps
//! [`call_create`] around the game's `wavebank_create` with the process
//! statics; host tests drive it with local instances and a fake original.
//!
//! Post-original code is allocation-free, lock-free, and panic-contained: it
//! only CASes the preallocated slot table, writes atomics/seqlock words,
//! and pushes fixed-size maintenance records. Everything that needs a mutex
//! or I/O happens later on the maintenance drain.

use std::panic::AssertUnwindSafe;

use super::clock_patch::RatePublication;
use super::lifecycle::LifecycleState;
use super::xact_runtime::{
    FrameIdentity, MaintenanceEvent, MaintenanceKind, MaintenanceQueue, XactSlotPhase, XactSlots,
};
use crate::services::score_guard::RateSaveLedger;

/// Capacity of the fixed maintenance queue (shared with the unregister path).
pub const MAINTENANCE_CAPACITY: usize = 16;

/// Boot-only reproducible fault injection (design Error Handling, req 41).
/// Parsed from `DDR_SONG_RATE_FAULT` in developer mode only; `None`
/// everywhere in production. One selector at a time. The transaction legs
/// inject here; the streaming legs inject at their preflight sites in
/// `binding::prepare_binding` (`mid-song-failure` arms the producer's
/// kill-after-N-blocks hook).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FaultSelector {
    /// Panic inside the detour before the original call.
    pub pre_original_panic: bool,
    /// Panic inside the detour after the original call.
    pub post_original_panic: bool,
    /// Corrupt TLS/frame identity after exposure (forces slot recovery, then
    /// recovery failure handling).
    pub token_mismatch: bool,
    /// Force the original's result to failure after exposure (simulated XACT
    /// rejection).
    pub xact_reject: bool,
    /// Simulate a saturated maintenance queue at enqueue time.
    pub maintenance_saturation: bool,
    /// Refuse the bind as if the FileManager source row were unreadable.
    pub source_read: bool,
    /// Refuse the bind at the header-synthesis site.
    pub header_synth: bool,
    /// Refuse the bind at the producer-start site.
    pub generator_start: bool,
    /// Refuse the bind outright (the generic refusal leg).
    pub bind_refused: bool,
    /// Refuse ONLY an identity (percent-100 training) bind — proves the
    /// training arm's fail-open degradation without touching the rate legs.
    pub identity_bind_refused: bool,
    /// Let the bind succeed, then kill the producer after N encoded blocks
    /// (exercises the silence-fill containment live).
    pub mid_song_failure: bool,
}

impl FaultSelector {
    /// Parse the boot-only selector value. Unknown values select nothing
    /// (and the caller warns).
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        let mut fault = Self::default();
        match value.trim() {
            "pre-original" => fault.pre_original_panic = true,
            "post-original" => fault.post_original_panic = true,
            "token-mismatch" => fault.token_mismatch = true,
            "xact-reject" => fault.xact_reject = true,
            "maintenance-saturation" => fault.maintenance_saturation = true,
            "source-read" => fault.source_read = true,
            "header-synth" => fault.header_synth = true,
            "generator-start" => fault.generator_start = true,
            "bind-refused" => fault.bind_refused = true,
            "identity-bind-refused" => fault.identity_bind_refused = true,
            "mid-song-failure" => fault.mid_song_failure = true,
            _ => return None,
        }
        Some(fault)
    }
}

/// Session-sticky score taint sink (production: a thin wrapper over
/// `score_guard::mark_session_tainted`; tests: a recorder).
pub trait SessionTaint: Sync {
    fn taint(&self, side: usize);
}

/// Everything the transaction touches, injected so the full ordering/fault
/// matrix runs host-side. All references are to lock-free structures; the
/// movie confirm closure writes an atomic contributor.
pub struct TransactionParts<'a> {
    pub slots: &'a XactSlots,
    pub maintenance: &'a MaintenanceQueue<MAINTENANCE_CAPACITY>,
    pub publication: &'a RatePublication,
    pub ledger: &'a RateSaveLedger,
    pub lifecycle: &'a LifecycleState,
    /// Movie-contributor confirm (commit re-asserts suppression).
    pub confirm_movie: &'a (dyn Fn() + Sync),
    /// Session-sticky score taint for participating sides.
    pub taint_session: &'a dyn SessionTaint,
    pub fault: FaultSelector,
}

/// What the injected pre-original bind step did (design req 23: "expose"
/// replaced by "bind"). The closure owns every effect — slot expose,
/// lifecycle advance, registry publication, refusal accounting — so the
/// transaction only sequences it (pre-original, panic-contained, exactly
/// once, only when a TLS frame exists). The variants are diagnostic; the
/// post-original machinery discovers an exposure through the frame/slot
/// protocol exactly as before.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindOutcome {
    /// Not a qualifying create — nothing bound, stock behavior.
    Stock,
    /// A binding was published and the token exposed into the in-flight
    /// slot; the original's result decides commit vs late-fail.
    Bound,
    /// A qualifying create's preflight refused: no token, no binding, the
    /// original runs unbound (EarlyFailed is the closure's doing).
    Refused,
}

/// What the create call did (diagnostics/logging; the u8 return travels to
/// the game separately).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CreateOutcome {
    /// No token was exposed — an ordinary stock/static bank.
    Stock,
    /// Exposed token committed (score → movie → snapshot → Q31-last done).
    Committed { generation: u64 },
    /// XACT rejected the exposed bank; slot quarantined, maintenance
    /// enqueued (or left pinned on a full queue).
    LateFailed { generation: u64, enqueued: bool },
    /// Exposure was known but no exact record could be recovered: the return
    /// value was overridden to failure and every candidate slot quarantined.
    RecoveryFailed,
    /// The TLS stack was exhausted; the original ran without any binding
    /// possibility.
    TlsOverflow,
}

/// Run one `wavebank_create` under the exactly-once protocol. The original
/// is invoked exactly once on every leg; the returned u8 is what the game
/// receives. `bind` is the pre-original bind step (design req 23): it runs
/// panic-contained after the frame opens and before the original, and only
/// when a frame exists (a TLS overflow leaves nothing to attribute a
/// binding to). A bind panic is contained exactly like the pre-original
/// fault leg: the frame clears and the call proceeds stock — unless the
/// bind had already exposed a slot, in which case the existing conservative
/// recovery machinery consumes it post-original.
pub fn call_create(
    parts: &TransactionParts<'_>,
    file_id: i32,
    owner_thread: u64,
    bind: impl FnOnce(i32) -> BindOutcome,
    original: impl FnOnce(i32) -> u8,
) -> (u8, CreateOutcome) {
    // Pre-original: open the call-nonced TLS frame. Overflow means no
    // binding can be attributed — stock behavior, original still exactly
    // once. A pre-original panic (fault-injected here; defensive containment
    // in the detour) must clear the frame and still call the original once
    // without a binding.
    let frame = match super::xact_runtime::enter_frame(file_id) {
        Ok(frame) => Some(frame),
        Err(_) => None,
    };
    let tls_overflowed = frame.is_none();

    let pre_panic = std::panic::catch_unwind(AssertUnwindSafe(|| {
        if parts.fault.pre_original_panic {
            panic!("song_rate fault injection: pre-original");
        }
        if !tls_overflowed {
            let _ = bind(file_id);
        }
    }));
    let frame = if pre_panic.is_err() {
        // The frame guard is dropped (frame cleared) and the call proceeds
        // as a plain stock bank.
        drop(frame);
        None
    } else {
        frame
    };

    // The original — exactly once, on every leg.
    let mut result = original(file_id);

    // Post-original, panic-contained end to end.
    let mut outcome = CreateOutcome::Stock;
    let post = std::panic::catch_unwind(AssertUnwindSafe(|| {
        if parts.fault.xact_reject && frame_exposed(parts.slots, frame.as_ref()) {
            result = 0;
        }
        if parts.fault.post_original_panic {
            panic!("song_rate fault injection: post-original");
        }
        outcome = finish_create(parts, file_id, owner_thread, frame.as_ref(), result);
        if matches!(outcome, CreateOutcome::RecoveryFailed) {
            result = 0;
        }
        result
    }));
    let result = match post {
        Ok(result) => result,
        Err(_) => {
            // A post-original panic is contained; if a token had been
            // exposed, conservative recovery still runs (second, minimal
            // containment pass) so the slot cannot leak in Exposed.
            let fallback = std::panic::catch_unwind(AssertUnwindSafe(|| {
                outcome = finish_create(parts, file_id, owner_thread, frame.as_ref(), result);
                if matches!(outcome, CreateOutcome::RecoveryFailed) {
                    0
                } else {
                    result
                }
            }));
            fallback.unwrap_or(result)
        }
    };
    drop(frame);
    if tls_overflowed {
        return (result, CreateOutcome::TlsOverflow);
    }
    (result, outcome)
}

fn frame_exposed(slots: &XactSlots, frame: Option<&super::xact_runtime::FrameGuard>) -> bool {
    frame
        .and_then(|frame| {
            super::xact_runtime::current_frame()
                .filter(|current| current.nonce == frame.identity().nonce)
        })
        .and_then(|identity| identity.slot_index)
        .and_then(|index| slots.phase(index))
        == Some(XactSlotPhase::Exposed)
}

/// Consume the exact exposed record (TLS slot index, else owner/nonce/file-id
/// recovery) and apply commit or late-fail. Allocation-free and lock-free.
fn finish_create(
    parts: &TransactionParts<'_>,
    file_id: i32,
    owner_thread: u64,
    frame: Option<&super::xact_runtime::FrameGuard>,
    result: u8,
) -> CreateOutcome {
    // Read the LIVE top frame rather than the guard's copy: the binding
    // path attaches the slot index to the thread-local stack while the guard
    // sits further down this call's stack (see `attach_slot_to_current`).
    let identity: Option<FrameIdentity> = frame.and_then(|frame| {
        super::xact_runtime::current_frame()
            .filter(|current| current.nonce == frame.identity().nonce)
    });
    // Fault: corrupt the frame identity so the exact-token match fails,
    // exercising recovery-failure handling.
    let identity = match identity {
        Some(mut id) if parts.fault.token_mismatch => {
            id.nonce = id.nonce.wrapping_add(0x5A5A);
            id.slot_index = None;
            Some(id)
        }
        other => other,
    };

    let nonce = identity.map_or(0, |id| id.nonce);
    let attached = identity.and_then(|id| id.slot_index);
    let candidate = attached.or_else(|| {
        parts
            .slots
            .recover_exposed(owner_thread, nonce, file_id)
            .ok()
    });

    let Some(index) = candidate else {
        // No exact record. If any slot is still Exposed, exposure is known
        // but unrecoverable: fail the load, quarantine every candidate,
        // taint conservatively, never call the original again.
        if any_exposed(parts.slots) {
            return recovery_failure(parts);
        }
        return CreateOutcome::Stock;
    };

    if result != 0 {
        // Commit: consume the exact token, then the infallible ordering —
        // score protection first, movie confirmation second, snapshot third,
        // non-identity Q31 last (inside publish_committed).
        let token = match parts.slots.commit(index, owner_thread, nonce, file_id) {
            Ok(token) => token,
            Err(_) => {
                // The slot exists but the exact identity does not match: a
                // known exposure that cannot be consumed exactly is a
                // recovery failure, not a stock bank.
                if any_exposed(parts.slots) {
                    return recovery_failure(parts);
                }
                return CreateOutcome::Stock;
            }
        };
        // Identity (training) commits carry NO score protection and NO
        // movie confirmation: arming alone is not an alteration — the
        // served audio is byte-identical and the clock stays 1:1 (training
        // design §4.1/§4.5). Score containment for training engages later,
        // through its own taint, only when a bound/seek actually fires.
        if token.requested_percent != super::lifecycle::IDENTITY_PERCENT {
            for side in 0..2usize {
                if token.participant_mask & (1 << side) != 0 {
                    let _ = parts
                        .ledger
                        .append(side, token.generation, token.stage_index);
                    parts.taint_session.taint(side);
                }
            }
            // Movie confirmation: the commit re-asserts suppression for a
            // rate-adjusted song — UNLESS the generation latched SYNC
            // BACKGROUND VIDEO (effective: real Windows with the movie-sync
            // engine, by the arm's platform gate — D14): then the graph
            // builds normally and movie_sync rate-locks it at graph open.
            // Score protection above applies regardless — a rate-played
            // song is score-contained whether its movie plays or not.
            if !parts.lifecycle.sync_movie() {
                (parts.confirm_movie)();
            }
        }
        parts.publication.publish_committed(
            token.generation,
            token.requested_percent,
            token.participant_mask,
            token.effective_rate,
        );
        advance_phase(|| parts.lifecycle.mark_committed(token.generation));
        CreateOutcome::Committed {
            generation: token.generation,
        }
    } else {
        // Late failure: XACT rejected the exposed bank. The original's false
        // return owns the loading abort; the clock never left identity; the
        // movie contributor stays; score taint (if gameplay somehow starts)
        // is the gameplay-entry policy's job. Only CAS the slot and enqueue
        // fixed-size maintenance — everything needing a mutex or I/O happens
        // on the drain.
        let generation = parts.lifecycle.generation();
        let enqueued = match parts.slots.quarantine(index) {
            Ok(()) => enqueue_reclaim(parts, index),
            Err(_) => false,
        };
        advance_phase(|| parts.lifecycle.mark_late_failed(generation));
        CreateOutcome::LateFailed {
            generation,
            enqueued,
        }
    }
}

fn any_exposed(slots: &XactSlots) -> bool {
    (0..super::xact_runtime::MAX_XACT_SLOTS)
        .any(|index| slots.phase(index) == Some(XactSlotPhase::Exposed))
}

/// Known exposure without exact recovery: override the return to failure,
/// quarantine every Exposed slot, and taint both sides/session conservatively
/// (the participant mask is unknowable here).
fn recovery_failure(parts: &TransactionParts<'_>) -> CreateOutcome {
    for candidate in 0..super::xact_runtime::MAX_XACT_SLOTS {
        if parts.slots.phase(candidate) == Some(XactSlotPhase::Exposed) {
            if parts.slots.quarantine(candidate).is_ok() {
                let _ = enqueue_reclaim(parts, candidate);
            }
        }
    }
    parts.taint_session.taint(0);
    parts.taint_session.taint(1);
    let generation = parts.lifecycle.generation();
    advance_phase(|| parts.lifecycle.mark_late_failed(generation));
    CreateOutcome::RecoveryFailed
}

/// Bounded spin over the lifecycle guard: its writer sections are tiny and
/// never wait, so contention resolves in a few iterations; a persistent
/// failure (impossible short of a wedged scene thread) is abandoned rather
/// than looped forever.
fn advance_phase(mut attempt: impl FnMut() -> Result<(), super::lifecycle::PhaseError>) {
    for _ in 0..1024 {
        match attempt() {
            Err(super::lifecycle::PhaseError::Busy) => std::hint::spin_loop(),
            _ => return,
        }
    }
}

/// Push a binding-reclamation record for a quarantined slot; a full queue
/// leaves the slot pinned for the process lifetime (bounded leak beats
/// use-after-delete).
fn enqueue_reclaim(parts: &TransactionParts<'_>, slot_index: usize) -> bool {
    if parts.fault.maintenance_saturation {
        return false;
    }
    parts
        .maintenance
        .push(MaintenanceEvent {
            kind: MaintenanceKind::ReclaimBinding,
            slot_index: slot_index as u8,
        })
        .is_ok()
}
