//! Binding preflight and streaming runtime state for the rate engine.
//!
//! The streaming design binds one `{file_id, generation}` inside the
//! `wavebank_create` transaction and serves a synthesized virtual bank
//! through the game's XACT file-IO callbacks (design reqs 11–14, 23). This
//! module carries the surviving pure helpers — dance-bank path parsing and
//! the song-code digest the lifecycle binds on — plus the [`Binding`]
//! runtime core (bounded ring, pending read slots, epoch guard, and the
//! pure serve dispatch the IO-callback detours call verbatim; design reqs
//! 16–18, 20–21, 26–28). The producer that fills the ring lives in
//! `super::generator`.
//!
//! The preflight ([`prepare_binding`]) is the real validate → plan → copy →
//! producer-start pipeline; the [`BindingRegistry`] publishes the live
//! binding to the detours and defers buffer reclamation to the maintenance
//! drain (epoch guard + cooldown). Until plan Step 4's final task installs
//! the detour pair, the identity-only base stays structural:
//! `integration_available()` is false, boot readiness can never conjoin
//! true, and the SONG SPEED option row never registers.

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicI32, AtomicPtr, AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Instant;

use super::lifecycle::{GenerationPhase, LifecycleState};
use super::transaction::FaultSelector;
use crate::core::xact::digest;
use crate::core::xact::rate::RateRatio;
use crate::core::xact::virtual_bank::{self, PlanError, Region, VirtualBankLayout};
use crate::core::xact::{adpcm, xwb, WaveFormat};

/// The `<code>` stem (lowercased) when this normalized virtual path is a
/// streaming dance bank (`data/`-relative, e.g. `sound/win/dance/<code>.xwb`).
/// Arming is SONG-AGNOSTIC: whichever dance bank the game loads next is the
/// armed generation's song, so the code is derived from the path rather than
/// configured.
#[must_use]
pub fn dance_bank_song_code(normalized_path: &str) -> Option<String> {
    let lower = normalized_path.to_ascii_lowercase();
    if !lower.contains("sound/") {
        return None;
    }
    let mut components = lower.rsplit('/');
    let file = components.next()?;
    let parent = components.next()?;
    if parent != "dance" {
        return None;
    }
    let code = file.strip_suffix(".xwb")?;
    if code.is_empty() {
        return None;
    }
    Some(code.to_string())
}

/// A stable 64-bit digest of a song code, used to bind the armed generation
/// to one song (`LifecycleState::bind_song`; the low bit is forced so a
/// valid digest is never the unbound sentinel 0).
#[must_use]
pub fn song_code_digest(song_code: &str) -> u64 {
    let bytes = digest::md5_bytes(song_code.as_bytes()).0;
    u64::from_le_bytes(bytes[..8].try_into().expect("md5 has 16 bytes")) | 1
}

/// Why a qualifying bind request fell back to stock (→ EarlyFailed for a
/// first bind; a Quick-Restart refusal keeps the generation Committed with
/// the clock reset to identity). Every variant carries a stable wire code
/// for the registry's refusal mailbox — the preflight never logs directly;
/// the maintenance drain reports the coalesced refusal (design req 24).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum BindRefusal {
    /// The FileManager row's bytes could not be resolved (or the
    /// `source-read` fault leg injected the failure).
    SourceRead = 1,
    /// The source did not parse as a strict-profile song bank.
    UnsupportedProfile = 2,
    /// The rate plan refused (28-bit duration ceiling, unmappable loop).
    Plan = 3,
    /// The pre-data synthesis refused (structurally unreachable after a
    /// successful plan; also the `header-synth` fault leg's site).
    HeaderSynth = 4,
    /// The private source copy allocation failed (req 17's memcpy).
    SourceCopy = 5,
    /// Binding construction or producer-thread start failed (also the
    /// `generator-start` fault leg's site).
    ProducerStart = 6,
    /// The transaction slot claim/expose or the lifecycle phase advance
    /// failed (structurally near-impossible; typed for diagnostics).
    SlotExpose = 7,
    /// The `bind-refused` fault leg.
    Injected = 8,
}

impl BindRefusal {
    /// Mailbox wire code (nonzero; 0 is the empty-mailbox sentinel).
    #[must_use]
    pub fn code(self) -> u8 {
        self as u8
    }

    /// Decode a mailbox wire code (drain-side logging).
    #[must_use]
    pub fn from_code(code: u8) -> Option<Self> {
        Some(match code {
            1 => Self::SourceRead,
            2 => Self::UnsupportedProfile,
            3 => Self::Plan,
            4 => Self::HeaderSynth,
            5 => Self::SourceCopy,
            6 => Self::ProducerStart,
            7 => Self::SlotExpose,
            8 => Self::Injected,
            _ => return None,
        })
    }
}

/// Whether the streaming binding integration is installed: the XACT
/// file-IO callback detour pair is live (design req 40 — the flip from the
/// Step-1 constant false was task-04's deliberate act). Boot readiness
/// (`wavebank_hook::readiness`) conjoins on this, so an unresolved
/// signature set keeps the SONG SPEED row unregistered and everything
/// stock. Host builds have no detours and always report false — the
/// inverted identity-base tests assert the LINKAGE, not a literal.
#[must_use]
pub fn integration_available() -> bool {
    #[cfg(windows)]
    {
        super::io_callback_hook::installed()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// A borrowed view of the stock bank's bytes for the preflight to validate
/// and copy (design req 17, 23). Hosts inject plain buffers; the windows
/// glue constructs one over the FileManager row's pointer/size resolved
/// through the task-01 file-table derivation — valid only for the duration
/// of the create call, which is why [`prepare_binding`] copies before
/// returning.
pub struct SourceView<'a> {
    bytes: &'a [u8],
}

impl<'a> SourceView<'a> {
    #[must_use]
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }

    #[must_use]
    pub fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// Wrap a raw game-memory row (windows glue). Returns `None` for a
    /// null/empty row — the caller treats that as a source-read refusal.
    ///
    /// # Safety
    /// `ptr` must be valid for `len` reads for the lifetime `'a` (the
    /// FileManager row is stable for the duration of the create call).
    #[cfg(windows)]
    #[must_use]
    pub unsafe fn from_raw(ptr: *const u8, len: usize) -> Option<Self> {
        if ptr.is_null() || len == 0 {
            return None;
        }
        Some(Self {
            bytes: std::slice::from_raw_parts(ptr, len),
        })
    }
}

/// How many encoded blocks the `mid-song-failure` fault leg lets the
/// producer emit before it panics. Small on purpose: the producer runs
/// far ahead of realtime, so any bound dies during the pre-roll — a small
/// one also fires inside the host fixtures (~544 blocks at 50%), keeping
/// the live leg and the host test the same mechanism.
const FAULT_KILL_AFTER_BLOCKS: u64 = 64;

/// Preflight one qualifying bind request (design req 23 pre-original, 24):
/// validate the source through the injected view, compute the rate plan
/// (which synthesizes the virtual header's pre-data), copy the source into
/// a private allocation, construct the runtime state, and start the
/// producer. Runs in the create detour's pre-original context (game
/// thread, loading screen) where allocation is permitted; every refusal is
/// typed and the caller maps it to the failure policy (EarlyFailed / the
/// Quick-Restart conservative leg). Never logs — diagnostics go through
/// the registry mailbox to the drain.
///
/// `percent == 100` is the training identity arm (only reachable through a
/// training-arm request — ordinary 100% plays never qualify): the plan is
/// the verbatim `plan_identity_bank`, the binding serves the resident
/// source directly (`ServeMode::IdentityPassthrough`), and NO producer
/// thread is spawned (training design §4.5). The identity plan has no
/// target distinction, so `target` must be [`StretchTarget::Main`] there
/// (debug-asserted; unreachable by construction — the preview
/// qualification requires a non-100% rate).
pub fn prepare_binding(
    file_id: i32,
    generation: u64,
    percent: u32,
    preserve_pitch: bool,
    source: &SourceView<'_>,
    fault: &FaultSelector,
    target: virtual_bank::StretchTarget,
) -> Result<Arc<Binding>, BindRefusal> {
    if fault.bind_refused {
        return Err(BindRefusal::Injected);
    }
    if fault.identity_bind_refused && percent == 100 {
        return Err(BindRefusal::Injected);
    }
    if fault.source_read {
        return Err(BindRefusal::SourceRead);
    }
    let bytes = source.bytes();
    let bank = xwb::parse_song_bank(bytes).map_err(|_| BindRefusal::UnsupportedProfile)?;
    if percent == 100 {
        // Identity has no target distinction; Side is unreachable by
        // construction (the preview qualification requires ≠ 100%).
        debug_assert_eq!(target, virtual_bank::StretchTarget::Main);
        let layout = virtual_bank::plan_identity_bank(&bank).map_err(|error| match error {
            PlanError::PreData(_) => BindRefusal::HeaderSynth,
            _ => BindRefusal::Plan,
        })?;
        if fault.header_synth {
            return Err(BindRefusal::HeaderSynth);
        }
        drop(bank);
        let mut copy = Vec::new();
        copy.try_reserve_exact(bytes.len())
            .map_err(|_| BindRefusal::SourceCopy)?;
        copy.extend_from_slice(bytes);
        // No producer, no fault-kill hook (nothing to kill), no spawn.
        let binding =
            Binding::new_identity_passthrough(file_id, generation, layout, copy.into_boxed_slice())
                .map_err(|_| BindRefusal::ProducerStart)?;
        return Ok(Arc::new(binding));
    }
    let layout =
        virtual_bank::plan_virtual_bank(&bank, percent, target).map_err(|error| match error {
            PlanError::PreData(_) => BindRefusal::HeaderSynth,
            _ => BindRefusal::Plan,
        })?;
    if fault.header_synth {
        return Err(BindRefusal::HeaderSynth);
    }
    let rate = layout.entries[layout.target_entry_index].rate;
    drop(bank);
    // Private copy (req 17): no reads of game-owned memory after the bind
    // returns. `try_reserve_exact` keeps an allocation failure a typed
    // refusal instead of an abort.
    let mut copy = Vec::new();
    copy.try_reserve_exact(bytes.len())
        .map_err(|_| BindRefusal::SourceCopy)?;
    copy.extend_from_slice(bytes);
    let binding = Arc::new(
        Binding::new(
            file_id,
            generation,
            rate,
            layout,
            copy.into_boxed_slice(),
            preserve_pitch,
        )
        .map_err(|_| BindRefusal::ProducerStart)?,
    );
    if fault.mid_song_failure {
        binding.set_fault_kill_after_blocks(FAULT_KILL_AFTER_BLOCKS);
    }
    if fault.generator_start {
        return Err(BindRefusal::ProducerStart);
    }
    if super::generator::spawn(Arc::clone(&binding)).is_err() {
        // The thread never started; retire so the Arc's producer state can
        // never be observed half-live.
        binding.retire();
        return Err(BindRefusal::ProducerStart);
    }
    Ok(binding)
}

/// How a create call relates to the current generation (the qualifying
/// gate, RE note §5: the preview player creates slot-5 banks through the
/// IDENTICAL path — binding is gated on the armed generation and the song
/// digest, never on the path alone).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindQualification {
    /// An armed generation's first bind: whichever dance bank loads next IS
    /// the armed song (arming is song-agnostic).
    FirstBind,
    /// A re-create of the committed generation's own song — Quick Restart
    /// (design req 5): the same generation is served again from offset zero.
    QuickRestart,
    /// Not a qualifying create: stock behavior, nothing changes. Covers
    /// every non-Armed/non-Committed phase, non-dance banks, and a
    /// different song's bank while committed — all silently.
    Decline,
}

/// The one-atomic-load pre-gate the create detour runs BEFORE any path or
/// game-memory work: only an Armed or Committed generation can possibly
/// bind. Ordinary boots (Identity) return here.
#[must_use]
pub fn bind_may_qualify(lifecycle: &LifecycleState) -> bool {
    matches!(
        lifecycle.phase(),
        GenerationPhase::Armed | GenerationPhase::Committed
    )
}

/// Classify one create against the lifecycle. The first load is one atomic
/// phase read — ordinary boots (Identity) decline before any path or
/// game-memory work happens.
#[must_use]
pub fn qualify_bind(lifecycle: &LifecycleState, song_code: Option<&str>) -> BindQualification {
    let phase = lifecycle.phase();
    if !matches!(phase, GenerationPhase::Armed | GenerationPhase::Committed) {
        return BindQualification::Decline;
    }
    // A non-dance bank while armed declines SILENTLY (not EarlyFailed): the
    // stage load creates several banks and the dance bank arrives later in
    // the same corridor.
    let Some(code) = song_code else {
        return BindQualification::Decline;
    };
    let digest = song_code_digest(code);
    match (phase, lifecycle.bound_song()) {
        // First bind of the generation: the digest is set by the caller at
        // bind time (arming cleared it). A leftover digest that disagrees
        // is defensively declined.
        (GenerationPhase::Armed, None) => BindQualification::FirstBind,
        (GenerationPhase::Armed, Some(bound)) if bound == digest => BindQualification::FirstBind,
        (GenerationPhase::Committed, Some(bound)) if bound == digest => {
            BindQualification::QuickRestart
        }
        _ => BindQualification::Decline,
    }
}

// ---------------------------------------------------------------------------
// The binding registry: how a live binding is published to the detours and
// how its buffers are reclaimed (design req 26). One ACTIVE slot (the
// gameplay generation — only one wave streams during gameplay), one
// independent PREVIEW slot (the song-select preview binding — preview
// design §Components 2/3: consulted by the io detours only after the
// active slot misses, never entangled with the lifecycle/clock/score
// machinery), a small retired list the maintenance drain sweeps, and
// per-slot coalescing refusal mailboxes (the preflight never logs; the
// drain reports).
// ---------------------------------------------------------------------------

/// Retired-list capacity. At most one gameplay song plus one song-select
/// preview binding are live at a time and reclamation runs every 250 ms,
/// so 4 slots are generous; overflow leaks the binding (bounded leak beats
/// use-after-free — the same policy as a pinned slot).
const RETIRED_CAPACITY: usize = 4;

/// Sweep ticks a retired binding must stay quiescent before its buffers
/// drop: with the 250 ms drain cadence this is ≥ 500 ms of grace between
/// unpublish and free, dwarfing the µs window in which a detour can hold a
/// pointer it loaded before the unpublish — the epoch-reclamation safety
/// argument for the lock-free `with_active` reads below.
const RECLAIM_COOLDOWN_TICKS: u8 = 2;

/// Publish/retire/reclaim state shared between the create/unregister
/// detours, the preflight, and the maintenance drain. Const-constructible
/// so host tests run independent instances; production uses [`registry`].
pub struct BindingRegistry {
    /// The one live gameplay binding (`Arc::into_raw`), or null.
    active: AtomicPtr<Binding>,
    /// The one live song-select PREVIEW binding, or null (preview design
    /// R8: same publish/retire/reclaim mechanics, zero lifecycle
    /// involvement).
    preview: AtomicPtr<Binding>,
    /// Retired bindings awaiting quiescent reclamation.
    retired: [AtomicPtr<Binding>; RETIRED_CAPACITY],
    /// Per-retired-slot cooldown (counts down only while reclaim-eligible).
    cooldowns: [AtomicU8; RETIRED_CAPACITY],
    /// Coalescing refusal mailbox: last refusal code/file, total count.
    refusal_code: AtomicU8,
    refusal_file: AtomicI32,
    refusal_count: AtomicU32,
    /// The preview bind path's own mailbox (preview refusals must never
    /// mask gameplay refusals or vice versa).
    preview_refusal_code: AtomicU8,
    preview_refusal_file: AtomicI32,
    preview_refusal_count: AtomicU32,
}

impl BindingRegistry {
    #[must_use]
    pub const fn new() -> Self {
        const NULL: AtomicPtr<Binding> = AtomicPtr::new(std::ptr::null_mut());
        const ZERO: AtomicU8 = AtomicU8::new(0);
        Self {
            active: AtomicPtr::new(std::ptr::null_mut()),
            preview: AtomicPtr::new(std::ptr::null_mut()),
            retired: [NULL; RETIRED_CAPACITY],
            cooldowns: [ZERO; RETIRED_CAPACITY],
            refusal_code: AtomicU8::new(0),
            refusal_file: AtomicI32::new(-1),
            refusal_count: AtomicU32::new(0),
            preview_refusal_code: AtomicU8::new(0),
            preview_refusal_file: AtomicI32::new(-1),
            preview_refusal_count: AtomicU32::new(0),
        }
    }

    /// Publish a freshly bound generation as THE active binding. A
    /// leftover active binding (structurally unreachable — unregister
    /// retires before the next create) is retired defensively.
    pub fn publish(&self, binding: Arc<Binding>) {
        self.publish_into(&self.active, binding);
    }

    /// Publish a song-select PREVIEW binding (preview design §Components
    /// 2): independent of the active slot and of the lifecycle machinery.
    /// A previous preview binding (a re-trigger or a wheel settle whose
    /// unregister was missed) is retired defensively.
    pub fn publish_preview(&self, binding: Arc<Binding>) {
        self.publish_into(&self.preview, binding);
    }

    /// Swap `binding` into `slot`; retire whatever was there onto the
    /// retired list.
    fn publish_into(&self, slot: &AtomicPtr<Binding>, binding: Arc<Binding>) {
        let raw = Arc::into_raw(binding).cast_mut();
        let previous = slot.swap(raw, Ordering::AcqRel);
        if !previous.is_null() {
            // SAFETY: `previous` came from `Arc::into_raw` in this
            // registry and has not been reclaimed (only `sweep` frees, and
            // only after the pointer left both slots and the retired list).
            unsafe { (*previous).retire() };
            self.push_retired(previous);
        }
    }

    /// Run `visit` against the active binding, if any. The reference must
    /// not escape the closure: its validity is guaranteed by the
    /// reclamation cooldown (see [`RECLAIM_COOLDOWN_TICKS`]), not by a
    /// refcount.
    pub fn with_active<R>(&self, visit: impl FnOnce(&Binding) -> R) -> Option<R> {
        Self::with_slot(&self.active, visit)
    }

    /// Run `visit` against the preview binding, if any (same non-escaping
    /// contract as [`BindingRegistry::with_active`]).
    pub fn with_preview<R>(&self, visit: impl FnOnce(&Binding) -> R) -> Option<R> {
        Self::with_slot(&self.preview, visit)
    }

    fn with_slot<R>(slot: &AtomicPtr<Binding>, visit: impl FnOnce(&Binding) -> R) -> Option<R> {
        let raw = slot.load(Ordering::Acquire);
        if raw.is_null() {
            return None;
        }
        // SAFETY: the pointer was published from `Arc::into_raw` and is
        // freed only by `sweep` ≥ RECLAIM_COOLDOWN_TICKS drain ticks after
        // it stopped being loadable from any slot or the retired list.
        Some(visit(unsafe { &*raw }))
    }

    /// Whether ANY binding (active or preview) is live — the io detours'
    /// fast gate: at most two Acquire loads before the trampoline on
    /// ordinary boots.
    #[must_use]
    pub fn any_bound(&self) -> bool {
        !self.active.load(Ordering::Acquire).is_null()
            || !self.preview.load(Ordering::Acquire).is_null()
    }

    /// Resolve one read's binding by file id: the ACTIVE slot first, the
    /// preview slot on miss (preview design §Components 3 — gameplay
    /// serving must never pay for the preview feature beyond a second
    /// Acquire after an active miss). `None` when neither slot serves
    /// `file_id`; the io detours then take the trampoline.
    pub fn with_bound_for_file<R>(
        &self,
        file_id: i32,
        visit: impl FnOnce(&Binding) -> R,
    ) -> Option<R> {
        let mut visit = Some(visit);
        if let Some(result) = Self::with_slot(&self.active, |binding| {
            if binding.file_id() == file_id {
                // `visit` is present: this is the first and only take.
                Some(visit.take().expect("first slot visit")(binding))
            } else {
                None
            }
        })
        .flatten()
        {
            return Some(result);
        }
        Self::with_slot(&self.preview, |binding| {
            if binding.file_id() == file_id {
                visit.take().map(|visit| visit(binding))
            } else {
                None
            }
        })
        .flatten()
    }

    /// The active binding's generation (diagnostics/tests).
    #[must_use]
    pub fn active_generation(&self) -> Option<u64> {
        self.with_active(Binding::generation)
    }

    /// Run `visit` against a RETIRED binding for `file_id` (drain
    /// diagnostics and tests; same non-escaping contract as
    /// [`BindingRegistry::with_active`]).
    pub fn with_retired<R>(&self, file_id: i32, visit: impl FnOnce(&Binding) -> R) -> Option<R> {
        for slot in &self.retired {
            let raw = slot.load(Ordering::Acquire);
            if raw.is_null() {
                continue;
            }
            // SAFETY: as in `with_active` — retired pointers are freed only
            // by `sweep` after the cooldown.
            let binding = unsafe { &*raw };
            if binding.file_id() == file_id {
                return Some(visit(binding));
            }
        }
        None
    }

    /// Retire whichever binding (active first, then preview) serves
    /// `file_id` — the unregister prelude and the late-fail cleanup: new
    /// reads refuse, armed slots cancel with clamp semantics, the producer
    /// stops, and the binding moves to the retired list for the drain to
    /// reclaim at quiescence. Covering BOTH slots is what retires preview
    /// bindings on every natural teardown (wheel move, song confirm, scene
    /// exit) and on the restart's forced unregister with no new call sites
    /// (preview design §Components 2). Atomics-only — legal in
    /// post-original/detour context.
    pub fn retire_by_file(&self, file_id: i32) -> bool {
        self.retire_slot(&self.active, Some(file_id))
            || self.retire_slot(&self.preview, Some(file_id))
    }

    /// Unconditionally retire the preview binding, if any — the scene-exit
    /// defense (leaving SONG_SELECT) and the mod-disable path.
    pub fn retire_preview(&self) -> bool {
        self.retire_slot(&self.preview, None)
    }

    /// Retire `slot`'s binding when it matches `file_id` (or
    /// unconditionally for `None`). CAS-guarded: exactly one caller wins
    /// the pointer's transfer to the retired list.
    fn retire_slot(&self, slot: &AtomicPtr<Binding>, file_id: Option<i32>) -> bool {
        let raw = slot.load(Ordering::Acquire);
        if raw.is_null() {
            return false;
        }
        // SAFETY: slot pointers are valid until swept (see `with_slot`).
        if let Some(file_id) = file_id {
            if unsafe { (*raw).file_id() } != file_id {
                return false;
            }
        }
        if slot
            .compare_exchange(
                raw,
                std::ptr::null_mut(),
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_err()
        {
            // Another retire/publish won the race; nothing to do.
            return false;
        }
        // SAFETY: as above; this thread now owns the pointer's transfer to
        // the retired list.
        unsafe { (*raw).retire() };
        self.push_retired(raw);
        true
    }

    fn push_retired(&self, raw: *mut Binding) {
        for (slot, cooldown) in self.retired.iter().zip(&self.cooldowns) {
            if slot
                .compare_exchange(
                    std::ptr::null_mut(),
                    raw,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_ok()
            {
                cooldown.store(RECLAIM_COOLDOWN_TICKS, Ordering::Release);
                return;
            }
        }
        // Retired list full (structurally unreachable): leak the binding —
        // a bounded leak beats a use-after-free, matching the pinned-slot
        // policy on a saturated maintenance queue.
    }

    /// Number of bindings awaiting reclamation (diagnostics/tests).
    #[must_use]
    pub fn retired_count(&self) -> usize {
        self.retired
            .iter()
            .filter(|slot| !slot.load(Ordering::Acquire).is_null())
            .count()
    }

    /// One maintenance-drain pass over the retired list: a binding is
    /// freed only at `Retired ∧ readers == 0` (the epoch guard) AND after
    /// its cooldown ticks down — otherwise it is simply re-polled on the
    /// next 250 ms tick (the "re-poll, bounded" contract). `report` runs
    /// once per freed binding with its generation and final producer
    /// metrics, BEFORE the buffers drop (the drain logs them).
    pub fn sweep(&self, mut report: impl FnMut(u64, BindingMetrics)) {
        for (slot, cooldown) in self.retired.iter().zip(&self.cooldowns) {
            let raw = slot.load(Ordering::Acquire);
            if raw.is_null() {
                continue;
            }
            // SAFETY: retired pointers are freed only below, by this
            // single-consumer sweep (the maintenance drain).
            let binding = unsafe { &*raw };
            if !binding.reclaim_eligible() {
                continue;
            }
            let remaining = cooldown.load(Ordering::Acquire);
            if remaining > 1 {
                cooldown.store(remaining - 1, Ordering::Release);
                continue;
            }
            let generation = binding.generation();
            let metrics = binding.metrics_snapshot();
            if slot
                .compare_exchange(
                    raw,
                    std::ptr::null_mut(),
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_ok()
            {
                report(generation, metrics);
                // SAFETY: reconstitute the Arc published by `publish`/
                // `push_retired` and drop this registry's reference (the
                // producer thread's clone, if still exiting, keeps the
                // allocation alive until it finishes).
                drop(unsafe { Arc::from_raw(raw.cast_const()) });
            }
        }
    }

    /// Record one preflight refusal for the drain to report (coalescing:
    /// the last refusal's identity plus a total count — the preflight and
    /// detours never log directly, design req 24).
    pub fn note_refusal(&self, refusal: BindRefusal, file_id: i32) {
        self.refusal_code.store(refusal.code(), Ordering::Relaxed);
        self.refusal_file.store(file_id, Ordering::Relaxed);
        self.refusal_count.fetch_add(1, Ordering::AcqRel);
    }

    /// Drain the refusal mailbox: `(last refusal, last file_id, total)`
    /// since the previous take, or `None`.
    #[must_use]
    pub fn take_refusal(&self) -> Option<(BindRefusal, i32, u32)> {
        let count = self.refusal_count.swap(0, Ordering::AcqRel);
        if count == 0 {
            return None;
        }
        let refusal = BindRefusal::from_code(self.refusal_code.load(Ordering::Relaxed))?;
        Some((refusal, self.refusal_file.load(Ordering::Relaxed), count))
    }

    /// Record one PREVIEW bind refusal (the create detour's preview branch
    /// never logs; the drain reports). Independent of the gameplay mailbox
    /// so neither can mask the other.
    pub fn note_preview_refusal(&self, refusal: BindRefusal, file_id: i32) {
        self.preview_refusal_code
            .store(refusal.code(), Ordering::Relaxed);
        self.preview_refusal_file.store(file_id, Ordering::Relaxed);
        self.preview_refusal_count.fetch_add(1, Ordering::AcqRel);
    }

    /// Drain the preview refusal mailbox (see
    /// [`BindingRegistry::take_refusal`]).
    #[must_use]
    pub fn take_preview_refusal(&self) -> Option<(BindRefusal, i32, u32)> {
        let count = self.preview_refusal_count.swap(0, Ordering::AcqRel);
        if count == 0 {
            return None;
        }
        let refusal = BindRefusal::from_code(self.preview_refusal_code.load(Ordering::Relaxed))?;
        Some((
            refusal,
            self.preview_refusal_file.load(Ordering::Relaxed),
            count,
        ))
    }

    /// Publish a content mapping onto THE live binding (the training-mode
    /// seek/pre-shift surface — `Binding::set_content_mapping` semantics).
    /// Returns `false` when no binding is live or the values are out of
    /// range — callers fail open (the seek reports `Refused` and the song
    /// continues unmapped). Atomics-only; legal from any thread that may
    /// not block.
    pub fn set_active_content_mapping(&self, shift_blocks: u64, lead_blocks: u64) -> bool {
        self.with_active(|binding| binding.set_content_mapping(shift_blocks, lead_blocks))
            .unwrap_or(false)
    }

    /// The live binding's current content mapping `(shift_blocks,
    /// lead_blocks)`, or `None` when no binding is live. Read-only — the
    /// t=0 restart's "clear a leftover seek shift" guard consumes it.
    #[must_use]
    pub fn active_content_mapping(&self) -> Option<(u64, u64)> {
        self.with_active(|binding| binding.content_mapping())
    }

    /// The live binding's main-entry SERVED-stream block grid (the
    /// content-mapping unit): samples per block, sample rate, and the
    /// stream's whole-block count. `None` when no binding is live — the
    /// seek transaction's "audio half available" preflight (training
    /// design §4.4 gate 2).
    #[must_use]
    pub fn active_content_grid(&self) -> Option<ContentGrid> {
        self.with_active(|binding| {
            let layout = binding.layout();
            let entry = layout.target_entry_index;
            let format = binding.entry_format(entry);
            let align = u64::from(format.block_align()).max(1);
            ContentGrid {
                samples_per_block: format.samples_per_block(),
                sample_rate: format.sample_rate(),
                stream_blocks: layout.entries[entry].streamed.data_len as u64 / align,
            }
        })
    }
}

/// The served-stream block grid of a live binding's target entry (see
/// [`BindingRegistry::active_content_grid`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContentGrid {
    pub samples_per_block: u32,
    pub sample_rate: u32,
    pub stream_blocks: u64,
}

impl Default for BindingRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// The process-wide registry (the windows glue and maintenance drain share
/// it; host tests construct their own instances).
static REGISTRY: BindingRegistry = BindingRegistry::new();

#[must_use]
pub fn registry() -> &'static BindingRegistry {
    &REGISTRY
}

/// The song-select preview bindings' own monotonic identity counter
/// (preview design R15): previews never consume or advance the gameplay
/// generation. Starts at 1 (0 is "never").
static PREVIEW_GENERATION: AtomicU64 = AtomicU64::new(1);

/// Allocate the next preview-binding identity (logs/metrics only).
#[must_use]
pub fn next_preview_generation() -> u64 {
    PREVIEW_GENERATION.fetch_add(1, Ordering::AcqRel)
}

// ---------------------------------------------------------------------------
// Streaming runtime state: ring + pending slots + epoch guard + the pure
// serve dispatch (design reqs 16–18, 20–21, 26–28). The serve/poll surface
// is DETOUR CODE by contract: allocation-free, log-free, panic-free — the
// IO-callback detours (Step-4 task-04) call it verbatim from the game's
// engine pump threads. Diagnostics flow through the metrics counters, never
// logging.
// ---------------------------------------------------------------------------

/// Production ring capacity (16 MiB) — an internal constant per design
/// req 38 (no operator knobs). Memory is independent of song length.
pub const RING_CAPACITY: usize = 16 * 1024 * 1024;

/// Ring allocation for identity-passthrough bindings: the target entry is
/// served straight from the resident source, so the ring is never read or
/// written — this only keeps the wrap arithmetic well-defined without
/// paying 16 MiB per identity arm.
const IDENTITY_RING_CAPACITY: usize = 4_096;

/// Pending read slots. The engine keeps ONE outstanding read per stream
/// (RE note §3) and only one wave streams during gameplay; four slots are
/// generous headroom, never a queue.
pub const PENDING_SLOT_COUNT: usize = 4;

/// How far past the engine's consumption high-water the producer runs
/// before idling: half the ring, so the other half stays resident BEHIND
/// the cursor for the engine's short backward re-reads (the 0x1000 header
/// read overlapping entry-0 data). Internal constant.
const PACE_NUMERATOR: usize = 1;
const PACE_DENOMINATOR: usize = 2;

/// `Binding::regen_target` sentinel: no regeneration requested.
const REGEN_NONE: u64 = u64::MAX;

const STATE_ACTIVE: u8 = 0;
const STATE_SILENCE_FILL: u8 = 1;
const STATE_RETIRED: u8 = 2;

const SLOT_FREE: u8 = 0;
const SLOT_ARMING: u8 = 1;
const SLOT_ARMED: u8 = 2;
const SLOT_COMPLETING: u8 = 3;
const SLOT_COMPLETE: u8 = 4;

/// Lifecycle of one bound generation's runtime state (design req 28's
/// silence-fill containment plus the retire/reclaim epoch, req 26).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindingState {
    /// Producer live (or catching up): reads serve from the ring or defer.
    Active,
    /// The producer died mid-song: all further data reads complete
    /// instantly with valid pre-encoded silent ADPCM blocks (req 28).
    SilenceFill,
    /// Unregistered: new reads refuse; reclamation waits for quiescence.
    Retired,
}

/// How the TARGET entry's bytes are produced (training design §4.5). The verbatim
/// (preview) entry is a verbatim passthrough in both modes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServeMode {
    /// The producer thread synthesizes the rate-adjusted stream into the
    /// ring (the shipped streaming path).
    Stretch,
    /// The training-mode identity arm: the target (== main) entry is served
    /// allocation-free straight from the resident source copy under the
    /// content mapping — no ring, no DSP, no producer thread.
    IdentityPassthrough,
}

/// Outcome of one serve-dispatch call (the readFile detour body's verdict).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServeOutcome {
    /// Completed synchronously: `n` bytes copied to `dest` and accumulated
    /// (stock protocol: the callback returns TRUE).
    Served(u32),
    /// Deferred into a pending slot (stock protocol: FALSE +
    /// `ERROR_IO_PENDING`); the producer completes it.
    Pending,
    /// The binding is retired — or, structurally unreachable with the stock
    /// engine (one outstanding read per stream, four slots), no pending
    /// slot was free. The caller must treat this as a hard fault.
    Refused,
}

/// Outcome of one completion poll (the getOverlappedResult detour body).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PollOutcome {
    /// A pending slot matches this accumulator and is not yet complete
    /// (stock protocol: FALSE + `ERROR_IO_INCOMPLETE`).
    Incomplete,
    /// The matched slot completed: the accumulated byte count is reported
    /// and zeroed, and the slot is freed (stock report-and-zero protocol).
    Complete(u64),
    /// No pending slot matches: the caller handles the synchronous-serve
    /// accounting itself (report and zero the accumulator).
    NotPending,
}

/// Why constructing a [`Binding`] failed (preflight-time; → EarlyFailed).
#[derive(Debug)]
pub enum BindingError {
    /// The private source copy did not reparse as a strict-profile song
    /// bank (the preflight validates before copying, so this is a
    /// should-not-happen guard, not a policy leg).
    UnparseableSource(String),
    /// Pre-encoding the per-entry silent block failed (structurally
    /// unreachable for a parseable strict-profile bank).
    SilenceEncode(String),
    /// An identity-passthrough construction was handed a layout whose main
    /// entry is not passthrough-shaped against the source (must come from
    /// `plan_identity_bank` — never `plan_entry(…, 100)`).
    IdentityLayoutMismatch,
}

impl std::fmt::Display for BindingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnparseableSource(detail) => {
                write!(f, "source copy does not reparse: {detail}")
            }
            Self::SilenceEncode(detail) => {
                write!(f, "silent-block encode failed: {detail}")
            }
            Self::IdentityLayoutMismatch => {
                write!(f, "identity layout is not passthrough-shaped")
            }
        }
    }
}

impl std::error::Error for BindingError {}

/// The bounded output ring. Cursors are ABSOLUTE VIRTUAL FILE OFFSETS — one
/// linear producer cursor covers entry 0's stream, the zero-filled
/// alignment gap, and entry 1's stream; a byte at virtual offset `o` lives
/// at `buf[o % capacity]`. The design sketch's `base` cursor is derivable
/// (`produced − capacity`) and therefore omitted (field shapes free,
/// behavior binding — recorded at breakdown approval).
///
/// Concurrency: single producer (the generator), multiple readers (the
/// detours, read-only). `produced` is Release-published after the bytes are
/// written and Acquire-read before they are copied. Because both forward
/// production and regeneration REWRITE ring bytes, readers validate
/// seqlock-style: `rewinds` (bumped before every rewind) and the window low
/// edge are re-checked after the copy; a failed validation falls back to
/// the deferral path, where the producer (the only writer) completes the
/// read race-free.
struct Ring {
    buf: UnsafeCell<Box<[u8]>>,
    capacity: usize,
    /// Watermark: bytes at `[produced − capacity, produced)` are valid.
    produced: AtomicU64,
    /// Engine read high-water: the producer paces against this.
    consumed: AtomicU64,
    /// Seqlock counter: incremented (Release) before every rewind.
    rewinds: AtomicU64,
}

// SAFETY: the ring buffer is written only by the single producer thread and
// read by detour threads under the seqlock validation documented above; a
// reader that raced a rewrite discards its copy and defers instead.
unsafe impl Sync for Ring {}

impl Ring {
    fn new(capacity: usize, start: u64) -> Self {
        Self {
            buf: UnsafeCell::new(vec![0u8; capacity].into_boxed_slice()),
            capacity,
            produced: AtomicU64::new(start),
            consumed: AtomicU64::new(start),
            rewinds: AtomicU64::new(0),
        }
    }

    /// Copy `len` bytes at absolute virtual offset `abs` out of the ring
    /// (wrap-aware). Caller has validated the window; torn copies are
    /// caught by the post-copy seqlock re-validation.
    unsafe fn copy_out(&self, abs: u64, dest: *mut u8, len: usize) {
        let buf = (*self.buf.get()).as_ptr();
        let mut index = (abs % self.capacity as u64) as usize;
        let mut out = dest;
        let mut remaining = len;
        while remaining > 0 {
            let chunk = remaining.min(self.capacity - index);
            std::ptr::copy_nonoverlapping(buf.add(index), out, chunk);
            out = out.add(chunk);
            index = (index + chunk) % self.capacity;
            remaining -= chunk;
        }
    }

    /// Producer-only: write `bytes` at absolute virtual offset `abs`
    /// (wrap-aware). Publication is a separate Release store on `produced`.
    unsafe fn write(&self, abs: u64, bytes: &[u8]) {
        let buf = (*self.buf.get()).as_mut_ptr();
        let mut index = (abs % self.capacity as u64) as usize;
        let mut source = bytes.as_ptr();
        let mut remaining = bytes.len();
        while remaining > 0 {
            let chunk = remaining.min(self.capacity - index);
            std::ptr::copy_nonoverlapping(source, buf.add(index), chunk);
            source = source.add(chunk);
            index = (index + chunk) % self.capacity;
            remaining -= chunk;
        }
    }
}

/// One deferred engine read: an SPSC handoff from the read detour (arm) to
/// the producer (complete) to the poll detour (consume). The accumulator
/// pointer abstracts `OVERLAPPED.Internal` — hosts pass a local cell,
/// task-04 passes the real field — preserving the stock "accumulate on
/// serve, report-and-zero on poll" protocol exactly (RE note §7).
struct PendingSlot {
    /// SLOT_FREE → SLOT_ARMING (reader claims by CAS) → SLOT_ARMED
    /// (fields published, Release) → SLOT_COMPLETING (completer claims by
    /// CAS — producer, silence-flip, or retire-cancel, exactly one wins) →
    /// SLOT_COMPLETE (Release) → SLOT_FREE (poll consumes).
    state: AtomicU8,
    buffer: AtomicPtr<u8>,
    accumulator: AtomicPtr<u64>,
    offset: AtomicU64,
    len: AtomicU32,
    /// Nanoseconds since binding construction at arm time (deferral-latency
    /// metric; an `Instant` cannot be stored atomically).
    armed_at_nanos: AtomicU64,
}

impl PendingSlot {
    fn new() -> Self {
        Self {
            state: AtomicU8::new(SLOT_FREE),
            buffer: AtomicPtr::new(std::ptr::null_mut()),
            accumulator: AtomicPtr::new(std::ptr::null_mut()),
            offset: AtomicU64::new(0),
            len: AtomicU32::new(0),
            armed_at_nanos: AtomicU64::new(0),
        }
    }
}

/// Per-generation production metrics, exposed for the maintenance drain to
/// log at generation end (task-03) and for plan Step 5's live benchmark.
#[derive(Clone, Copy, Debug, Default)]
pub struct BindingMetrics {
    /// Stretched output frames produced into the ring (regenerated frames
    /// count again, and so do repositioning discards — the metric tracks
    /// producer work, not stream length).
    pub frames_produced: u64,
    /// Nanoseconds from binding construction to producer-thread exit.
    pub wall_nanos: u64,
    /// Reads that deferred into a pending slot.
    pub deferral_count: u64,
    /// Longest arm-to-completion latency among deferred reads.
    pub max_deferral_nanos: u64,
}

struct MetricCells {
    frames_produced: AtomicU64,
    wall_nanos: AtomicU64,
    deferral_count: AtomicU64,
    max_deferral_nanos: AtomicU64,
}

/// Availability verdict for one (EOF-clamped) read against the ring window.
enum SpanCheck {
    /// Every byte is copyable right now; `total` is the clamped serve count.
    Available { total: u32 },
    /// Some entry-data span is ahead of the produced watermark.
    NotProduced,
    /// Some entry-data span fell below the ring window; `target` is the
    /// block-aligned absolute virtual offset regeneration must restart at.
    BehindWindow { target: u64 },
}

/// The runtime state of one bound generation (design req 16–18): the
/// virtual-bank layout and private source copy from the preflight, the
/// bounded ring the producer fills with the TARGET (stretched) entry's
/// stream, the resident source serving the verbatim entry, the
/// pending-slot deferral surface, and the epoch-guarded lifecycle. Shared
/// `Arc`-style between the producer thread and the detours; reclamation is
/// deferred until `Retired ∧ readers == 0` (req 26).
///
/// Two-region serving model (step05-fix, maintainer-approved 2026-08-10):
/// the engine's bank prepare primes a stream context for EVERY wave —
/// including the preview entry — and the loading screen waits for those
/// reads. The VERBATIM entry is a passthrough: the header advertises its
/// stock values and its reads are served straight from the resident
/// source copy (zero DSP — WSOLA at the game's 47 kHz runs only a few ×
/// realtime on cabinet-class hardware, so stretching content the player
/// never hears is unaffordable; gameplay passes the preview through and
/// cost 10–25 s live before that fix). The ring covers only the TARGET
/// entry's range (`layout.target_entry_index` — main for gameplay, the
/// `_s` preview for song-select preview bindings), so the producer never
/// traverses the rest of the file and the window can never slide past
/// the engine's read position.
pub struct Binding {
    file_id: i32,
    generation: u64,
    rate: RateRatio,
    /// DSP mode for the target entry (design: preserve-pitch option). True =
    /// WSOLA time-stretch (pitch preserved); false = plain resample (pitch
    /// follows the rate). Latched at construction; the generator reads it
    /// once.
    preserve_pitch: bool,
    /// How the target entry's bytes are produced (training design §4.5):
    /// [`ServeMode::Stretch`] rides the ring/producer;
    /// [`ServeMode::IdentityPassthrough`] serves the resident source under
    /// the content mapping with no producer at all.
    serve_mode: ServeMode,
    /// The content mapping `{shift_blocks, lead_blocks}` (training design
    /// §4.5), packed shift:u32 hi | lead:u32 lo into ONE atomic so a reader
    /// can never observe a torn shift/lead pair. Block units on the main
    /// entry's served-stream grid; default `{0, 0}` = unmapped.
    mapping: AtomicU64,
    /// Bumped by every accepted [`Binding::set_content_mapping`]; the
    /// producer (Stretch mode) compares it against `mapping_applied` and
    /// restarts production at output 0 when they diverge.
    mapping_epoch: AtomicU64,
    /// The last epoch the producer finished applying. While it trails
    /// `mapping_epoch`, main-entry reads on a Stretch binding defer (the
    /// ring still holds the previous mapping's bytes).
    mapping_applied: AtomicU64,
    layout: VirtualBankLayout,
    /// Private copy of the stock bank (req 17): the generator's source and
    /// the verbatim entry's serving bytes.
    source: Box<[u8]>,
    /// Per-entry formats, reparsed once from the source copy (the layout's
    /// entry plans carry stretched values, not formats).
    formats: Vec<WaveFormat>,
    /// One pre-encoded silent ADPCM block per entry, so SilenceFill serving
    /// is allocation-free in detour context (req 28).
    silent_blocks: Vec<Vec<u8>>,
    ring: Ring,
    /// Every entry's stock data offset within `source`. Entries other than
    /// `layout.target_entry_index` are served VERBATIM from here (the
    /// `<code>_s` preview during gameplay, the main + any `goru_ac`-style
    /// variants during a preview bind) — resident from construction, always
    /// available; the ring/producer never touch them.
    entry_source_offsets: Vec<usize>,
    /// The TARGET (ring-served) entry's stock data range within `source` —
    /// the identity passthrough's verbatim serving base (unused in Stretch
    /// mode).
    target_source_offset: usize,
    target_source_len: u64,
    pending: [PendingSlot; PENDING_SLOT_COUNT],
    state: AtomicU8,
    /// Epoch guard (req 26): incremented before the state is validated,
    /// decremented after copy-out.
    readers: AtomicU32,
    /// Generation token: the producer checks this every hop and exits
    /// promptly (retire and supersession share this edge).
    stop: AtomicU8,
    /// Lowest requested regeneration target (absolute virtual offset), or
    /// [`REGEN_NONE`]. Main-entry offsets only by construction.
    regen_target: AtomicU64,
    /// Fault hook for task-03's `mid-song-failure` selector: the producer
    /// panics after encoding this many blocks (0 = disabled).
    fault_kill_after_blocks: AtomicU64,
    started_at: Instant,
    metrics: MetricCells,
}

/// Byte offset of `bank.entries[index]`'s data within `source`.
fn entry_source_offsets_target(bank: &xwb::SongBank<'_>, source: &[u8], index: usize) -> usize {
    bank.entries[index].data.as_ptr() as usize - source.as_ptr() as usize
}

impl Binding {
    /// Construct the runtime state around an already-planned layout and the
    /// private source copy (task-03's preflight produces both). Runs in the
    /// preflight context (game thread, loading screen): allocation is
    /// permitted here, and only here.
    pub fn new(
        file_id: i32,
        generation: u64,
        rate: RateRatio,
        layout: VirtualBankLayout,
        source: Box<[u8]>,
        preserve_pitch: bool,
    ) -> Result<Self, BindingError> {
        Self::with_ring_capacity(
            file_id,
            generation,
            rate,
            layout,
            source,
            preserve_pitch,
            RING_CAPACITY,
        )
    }

    /// [`Binding::new`] with an explicit ring capacity — host tests shrink
    /// the window to force behind-window regeneration with small fixtures.
    /// Production code always uses [`RING_CAPACITY`].
    #[allow(clippy::too_many_arguments)]
    pub fn with_ring_capacity(
        file_id: i32,
        generation: u64,
        rate: RateRatio,
        layout: VirtualBankLayout,
        source: Box<[u8]>,
        preserve_pitch: bool,
        ring_capacity: usize,
    ) -> Result<Self, BindingError> {
        Self::build(
            file_id,
            generation,
            rate,
            layout,
            source,
            preserve_pitch,
            ring_capacity,
            ServeMode::Stretch,
        )
    }

    /// Construct the training-mode identity arm's runtime state (training
    /// design §4.5): the target (== main) entry is served verbatim from the resident
    /// source under the content mapping — no ring production, no DSP, and
    /// NO producer thread (the caller must never `generator::spawn` one).
    /// The layout must come from `plan_identity_bank`: a target entry whose
    /// advertised length diverges from the stock data refuses
    /// ([`BindingError::IdentityLayoutMismatch`]) rather than serving out
    /// of the source's bounds.
    pub fn new_identity_passthrough(
        file_id: i32,
        generation: u64,
        layout: VirtualBankLayout,
        source: Box<[u8]>,
    ) -> Result<Self, BindingError> {
        Self::build(
            file_id,
            generation,
            RateRatio::IDENTITY,
            layout,
            source,
            true,
            IDENTITY_RING_CAPACITY,
            ServeMode::IdentityPassthrough,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn build(
        file_id: i32,
        generation: u64,
        rate: RateRatio,
        layout: VirtualBankLayout,
        source: Box<[u8]>,
        preserve_pitch: bool,
        ring_capacity: usize,
        serve_mode: ServeMode,
    ) -> Result<Self, BindingError> {
        let (formats, silent_blocks, entry_source_offsets, target_source_offset, target_source_len) = {
            let bank = xwb::parse_song_bank(&source)
                .map_err(|error| BindingError::UnparseableSource(error.to_string()))?;
            if bank.entries.len() != layout.entries.len()
                || layout.target_entry_index >= bank.entries.len()
            {
                return Err(BindingError::IdentityLayoutMismatch);
            }
            let mut formats = Vec::with_capacity(bank.entries.len());
            let mut silent_blocks = Vec::with_capacity(bank.entries.len());
            let mut entry_source_offsets = Vec::with_capacity(bank.entries.len());
            for (index, entry) in bank.entries.iter().enumerate() {
                let format = entry.format;
                let frames = format.samples_per_block() as usize;
                let zeros = vec![0i16; frames * format.channels() as usize];
                let mut block = Vec::new();
                adpcm::encode_block(&zeros, format, &mut block)
                    .map_err(|error| BindingError::SilenceEncode(error.to_string()))?;
                formats.push(format);
                silent_blocks.push(block);
                // Each entry's data slice borrows `source`; its offset is
                // that entry's verbatim serving base. The passthrough plan
                // guarantees a non-target entry's declared length equals
                // the stock length.
                debug_assert!(
                    index == layout.target_entry_index
                        || entry.data.len() == layout.entries[index].streamed.data_len
                );
                entry_source_offsets.push(entry.data.as_ptr() as usize - source.as_ptr() as usize);
            }
            // The TARGET entry's stock range: the identity passthrough's
            // serving base. An identity layout that diverges from the
            // stock length would serve out of the source's bounds —
            // refuse (the layout must come from `plan_identity_bank`).
            let target_data = bank.entries[layout.target_entry_index].data;
            if serve_mode == ServeMode::IdentityPassthrough
                && target_data.len() != layout.entries[layout.target_entry_index].streamed.data_len
            {
                return Err(BindingError::IdentityLayoutMismatch);
            }
            (
                formats,
                silent_blocks,
                entry_source_offsets,
                entry_source_offsets_target(&bank, &source, layout.target_entry_index),
                target_data.len() as u64,
            )
        };
        let ring_base = layout.entry_offsets[layout.target_entry_index];
        Ok(Self {
            file_id,
            generation,
            rate,
            preserve_pitch,
            serve_mode,
            mapping: AtomicU64::new(0),
            mapping_epoch: AtomicU64::new(0),
            mapping_applied: AtomicU64::new(0),
            layout,
            source,
            formats,
            silent_blocks,
            ring: Ring::new(ring_capacity, ring_base),
            entry_source_offsets,
            target_source_offset,
            target_source_len,
            pending: [
                PendingSlot::new(),
                PendingSlot::new(),
                PendingSlot::new(),
                PendingSlot::new(),
            ],
            state: AtomicU8::new(STATE_ACTIVE),
            readers: AtomicU32::new(0),
            stop: AtomicU8::new(0),
            regen_target: AtomicU64::new(REGEN_NONE),
            fault_kill_after_blocks: AtomicU64::new(0),
            started_at: Instant::now(),
            metrics: MetricCells {
                frames_produced: AtomicU64::new(0),
                wall_nanos: AtomicU64::new(0),
                deferral_count: AtomicU64::new(0),
                max_deferral_nanos: AtomicU64::new(0),
            },
        })
    }

    // ── Identity and lifecycle ───────────────────────────────────────

    #[must_use]
    pub fn file_id(&self) -> i32 {
        self.file_id
    }

    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation
    }

    #[must_use]
    pub fn rate(&self) -> RateRatio {
        self.rate
    }

    /// The latched DSP mode: true = pitch-preserved stretch, false = plain
    /// resample.
    #[must_use]
    pub fn preserve_pitch(&self) -> bool {
        self.preserve_pitch
    }

    /// How the target entry's bytes are produced (training design §4.5).
    #[must_use]
    pub fn serve_mode(&self) -> ServeMode {
        self.serve_mode
    }

    /// Publish a content mapping `{shift_blocks, lead_blocks}` (training
    /// design §4.5): virtual main-entry block `v < lead` serves the
    /// pre-encoded silent block; block `v ≥ lead` serves served-stream
    /// block `v − lead + shift`, with silent tiling past the content end.
    /// Block units on the target entry's served-stream grid (== source
    /// blocks at identity, stretched output blocks at rate). May be set at
    /// bind time (skip-first pre-shift) or between cue stop and replay
    /// (seeks) — never while the engine is actively reading a Stretch
    /// binding's target entry (reads in flight would defer until the
    /// producer applies the remap). Returns `false` (unchanged) when a
    /// component exceeds the packed u32 range.
    pub fn set_content_mapping(&self, shift_blocks: u64, lead_blocks: u64) -> bool {
        let (Ok(shift), Ok(lead)) = (u32::try_from(shift_blocks), u32::try_from(lead_blocks))
        else {
            return false;
        };
        // One packed word so a reader can never observe a torn pair.
        self.mapping
            .store(u64::from(shift) << 32 | u64::from(lead), Ordering::Release);
        // Stretch mode: the epoch bump tells the producer to restart
        // production at output 0 under the new mapping (its `ring_rewind`
        // bumps the ring seqlock); until it acknowledges, main-entry reads
        // defer. Identity mode reads the mapping at serve time and needs
        // no acknowledgement.
        self.mapping_epoch.fetch_add(1, Ordering::AcqRel);
        true
    }

    /// The current content mapping `(shift_blocks, lead_blocks)`.
    #[must_use]
    pub fn content_mapping(&self) -> (u64, u64) {
        let packed = self.mapping.load(Ordering::Acquire);
        (packed >> 32, packed & u64::from(u32::MAX))
    }

    /// The design's `B(T)`: convert a millisecond count to whole blocks on
    /// the TARGET (ring-served) entry's served-stream grid (floor —
    /// block-aligned by construction). The block duration comes from the entry's own parsed
    /// format (~2.90 ms for the production 44.1 kHz banks), so callers
    /// never hardcode a sample rate.
    #[must_use]
    pub fn ms_to_blocks(&self, ms: u64) -> u64 {
        let format = self.formats[self.layout.target_entry_index];
        let frames = ms.saturating_mul(u64::from(format.sample_rate())) / 1_000;
        frames / u64::from(format.samples_per_block().max(1))
    }

    /// Whether a published mapping awaits the producer (Stretch mode):
    /// `Some(epoch)` until [`Binding::mark_mapping_applied`] catches up.
    pub(crate) fn mapping_pending(&self) -> Option<u64> {
        let epoch = self.mapping_epoch.load(Ordering::Acquire);
        (epoch != self.mapping_applied.load(Ordering::Acquire)).then_some(epoch)
    }

    /// Producer-only: acknowledge a mapping epoch after restarting
    /// production under it.
    pub(crate) fn mark_mapping_applied(&self, epoch: u64) {
        self.mapping_applied.store(epoch, Ordering::Release);
    }

    /// One entry's pre-encoded silent block (the generator's silent
    /// lead/tail emission source).
    pub(crate) fn silent_block(&self, entry: usize) -> &[u8] {
        &self.silent_blocks[entry]
    }

    /// Ring seqlock counter (diagnostics/tests): bumped by every producer
    /// rewind, including the mapping-change restart.
    #[must_use]
    pub fn ring_rewind_count(&self) -> u64 {
        self.ring.rewinds.load(Ordering::Acquire)
    }

    #[must_use]
    pub fn layout(&self) -> &VirtualBankLayout {
        &self.layout
    }

    #[must_use]
    pub fn entry_format(&self, entry: usize) -> WaveFormat {
        self.formats[entry]
    }

    #[must_use]
    pub fn state(&self) -> BindingState {
        match self.state.load(Ordering::Acquire) {
            STATE_ACTIVE => BindingState::Active,
            STATE_SILENCE_FILL => BindingState::SilenceFill,
            _ => BindingState::Retired,
        }
    }

    /// Request the producer to stop at its next hop (the generation token's
    /// stop edge: retirement and supersession share it).
    pub fn request_stop(&self) {
        self.stop.store(1, Ordering::Release);
    }

    /// Retire the binding (the unregister detour's pre-original step): new
    /// reads refuse, the producer stops, and armed pending slots complete
    /// with the EOF-clamp cancellation semantics (0-byte completions).
    /// Reclamation stays deferred until [`Binding::reclaim_eligible`].
    pub fn retire(&self) {
        self.request_stop();
        self.state.store(STATE_RETIRED, Ordering::Release);
        for slot in &self.pending {
            self.cancel_slot(slot);
        }
    }

    /// Whether the buffers may be reclaimed: `Retired ∧ readers == 0`
    /// (req 26; the maintenance drain checks this — task-03 wires it).
    #[must_use]
    pub fn reclaim_eligible(&self) -> bool {
        self.state.load(Ordering::Acquire) == STATE_RETIRED
            && self.readers.load(Ordering::Acquire) == 0
    }

    /// Enter the epoch guard (readers increment BEFORE validating the
    /// state). `serve` guards itself; this surface exists for callers that
    /// hold ring-derived data across their own critical section (the poll
    /// detour, tests).
    pub fn reader_enter(&self) {
        self.readers.fetch_add(1, Ordering::AcqRel);
    }

    /// Leave the epoch guard.
    pub fn reader_exit(&self) {
        self.readers.fetch_sub(1, Ordering::AcqRel);
    }

    /// Arm the mid-song fault hook (dev-mode `DDR_SONG_RATE_FAULT`
    /// selector, task-03): the producer panics after encoding this many
    /// blocks, exercising the real `catch_unwind` → SilenceFill path.
    pub fn set_fault_kill_after_blocks(&self, blocks: u64) {
        self.fault_kill_after_blocks
            .store(blocks, Ordering::Release);
    }

    #[must_use]
    pub fn metrics_snapshot(&self) -> BindingMetrics {
        BindingMetrics {
            frames_produced: self.metrics.frames_produced.load(Ordering::Acquire),
            wall_nanos: self.metrics.wall_nanos.load(Ordering::Acquire),
            deferral_count: self.metrics.deferral_count.load(Ordering::Acquire),
            max_deferral_nanos: self.metrics.max_deferral_nanos.load(Ordering::Acquire),
        }
    }

    fn elapsed_nanos(&self) -> u64 {
        u64::try_from(self.started_at.elapsed().as_nanos()).unwrap_or(u64::MAX)
    }

    // ── The serve dispatch (detour context: no alloc/log/panic) ─────

    /// Serve one engine read against the virtual bank. `dest` must be valid
    /// for `len` bytes and `accumulator` must point at the request's
    /// completion accumulator (`OVERLAPPED.Internal`); both must stay valid
    /// until the request is consumed through [`Binding::poll`] (the stock
    /// contract — the engine owns both for the life of the request).
    ///
    /// Region semantics (design req 12, 21, 28): pre-data/gap/EOF serve
    /// synchronously (copy/zero/clamp); entry data inside the produced
    /// window copies; not-yet-produced or behind-window arms a pending slot
    /// (behind-window records the regeneration target first) and reports
    /// [`ServeOutcome::Pending`]; SilenceFill serves valid silent blocks
    /// immediately; Retired refuses.
    ///
    /// # Safety
    /// `dest` and `accumulator` must be valid, writable, and non-aliased
    /// for the duration described above.
    pub unsafe fn serve(
        &self,
        offset: u64,
        len: u32,
        dest: *mut u8,
        accumulator: *mut u64,
    ) -> ServeOutcome {
        self.reader_enter();
        let outcome = self.serve_locked(offset, len, dest, accumulator);
        self.reader_exit();
        outcome
    }

    unsafe fn serve_locked(
        &self,
        offset: u64,
        len: u32,
        dest: *mut u8,
        accumulator: *mut u64,
    ) -> ServeOutcome {
        match self.state.load(Ordering::Acquire) {
            STATE_RETIRED => ServeOutcome::Refused,
            STATE_SILENCE_FILL => {
                let total = self.copy_spans_silent(offset, len, dest);
                *accumulator += u64::from(total);
                ServeOutcome::Served(total)
            }
            _ => {
                let rewinds_before = self.ring.rewinds.load(Ordering::Acquire);
                let produced_before = self.ring.produced.load(Ordering::Acquire);
                match self.check_spans(offset, len, produced_before) {
                    SpanCheck::Available { total } => {
                        self.copy_spans(offset, total, dest);
                        // Seqlock re-validation: a rewind or a low-edge
                        // advance during the copy may have rewritten the
                        // bytes underneath us — discard and defer.
                        let produced_after = self.ring.produced.load(Ordering::Acquire);
                        let rewinds_after = self.ring.rewinds.load(Ordering::Acquire);
                        let stable = rewinds_after == rewinds_before
                            && matches!(
                                self.check_spans(offset, total, produced_after),
                                SpanCheck::Available { .. }
                            );
                        if stable {
                            *accumulator += u64::from(total);
                            self.ring
                                .consumed
                                .fetch_max(offset + u64::from(total), Ordering::AcqRel);
                            ServeOutcome::Served(total)
                        } else {
                            self.arm_slot(offset, len, dest, accumulator)
                        }
                    }
                    SpanCheck::NotProduced => self.arm_slot(offset, len, dest, accumulator),
                    SpanCheck::BehindWindow { target } => {
                        self.regen_target.fetch_min(target, Ordering::AcqRel);
                        self.arm_slot(offset, len, dest, accumulator)
                    }
                }
            }
        }
    }

    /// Poll one request for completion (the getOverlappedResult detour
    /// body). Matches pending slots by accumulator pointer; a completed
    /// match reports the accumulated count, zeroes it, and frees the slot
    /// (the stock report-and-zero protocol). No match means the request
    /// completed synchronously: the caller reports-and-zeroes itself.
    ///
    /// # Safety
    /// `accumulator` must be the pointer passed to [`Binding::serve`].
    pub unsafe fn poll(&self, accumulator: *mut u64) -> PollOutcome {
        for slot in &self.pending {
            let state = slot.state.load(Ordering::Acquire);
            if state == SLOT_FREE || state == SLOT_ARMING {
                continue;
            }
            if slot.accumulator.load(Ordering::Relaxed) != accumulator {
                continue;
            }
            return match state {
                SLOT_ARMED | SLOT_COMPLETING => PollOutcome::Incomplete,
                _ => {
                    let bytes = *accumulator;
                    *accumulator = 0;
                    slot.state.store(SLOT_FREE, Ordering::Release);
                    PollOutcome::Complete(bytes)
                }
            };
        }
        PollOutcome::NotPending
    }

    unsafe fn arm_slot(
        &self,
        offset: u64,
        len: u32,
        dest: *mut u8,
        accumulator: *mut u64,
    ) -> ServeOutcome {
        for slot in &self.pending {
            if slot
                .state
                .compare_exchange(SLOT_FREE, SLOT_ARMING, Ordering::Acquire, Ordering::Relaxed)
                .is_err()
            {
                continue;
            }
            slot.buffer.store(dest, Ordering::Relaxed);
            slot.accumulator.store(accumulator, Ordering::Relaxed);
            slot.offset.store(offset, Ordering::Relaxed);
            slot.len.store(len, Ordering::Relaxed);
            slot.armed_at_nanos
                .store(self.elapsed_nanos(), Ordering::Relaxed);
            self.metrics.deferral_count.fetch_add(1, Ordering::Relaxed);
            slot.state.store(SLOT_ARMED, Ordering::Release);
            // Close the arm-vs-terminal-transition race: a flip between our
            // state check and the arm would have missed this slot in its
            // cancellation sweep, stranding the request forever.
            match self.state.load(Ordering::Acquire) {
                STATE_SILENCE_FILL => self.complete_slot_silent(slot),
                STATE_RETIRED => self.cancel_slot(slot),
                _ => {}
            }
            return ServeOutcome::Pending;
        }
        // Structurally unreachable with the stock engine (one outstanding
        // read per stream); a hard fault for the caller if it ever happens.
        ServeOutcome::Refused
    }

    /// EOF-clamp cancellation: exactly one completer claims the slot; the
    /// request completes with zero bytes (permitted by the clamp contract).
    fn cancel_slot(&self, slot: &PendingSlot) {
        if slot
            .state
            .compare_exchange(
                SLOT_ARMED,
                SLOT_COMPLETING,
                Ordering::AcqRel,
                Ordering::Relaxed,
            )
            .is_ok()
        {
            self.note_deferral_latency(slot);
            slot.state.store(SLOT_COMPLETE, Ordering::Release);
        }
    }

    /// Complete an armed slot with silent blocks (the silence-fill sweep).
    fn complete_slot_silent(&self, slot: &PendingSlot) {
        if slot
            .state
            .compare_exchange(
                SLOT_ARMED,
                SLOT_COMPLETING,
                Ordering::AcqRel,
                Ordering::Relaxed,
            )
            .is_err()
        {
            return;
        }
        let offset = slot.offset.load(Ordering::Relaxed);
        let len = slot.len.load(Ordering::Relaxed);
        let dest = slot.buffer.load(Ordering::Relaxed);
        let accumulator = slot.accumulator.load(Ordering::Relaxed);
        // SAFETY: the engine keeps `dest`/`accumulator` valid until the
        // request is consumed (the serve contract).
        unsafe {
            let total = self.copy_spans_silent(offset, len, dest);
            *accumulator += u64::from(total);
        }
        self.note_deferral_latency(slot);
        slot.state.store(SLOT_COMPLETE, Ordering::Release);
    }

    fn note_deferral_latency(&self, slot: &PendingSlot) {
        let armed_at = slot.armed_at_nanos.load(Ordering::Relaxed);
        let latency = self.elapsed_nanos().saturating_sub(armed_at);
        self.metrics
            .max_deferral_nanos
            .fetch_max(latency, Ordering::Relaxed);
    }

    // ── Span walking ─────────────────────────────────────────────────

    /// Availability of one read against the produced state: walks the
    /// layout's regions (the engine's 0x1000 header read spans pre-data
    /// into entry-0 data) applying the stock EOF clamp. Verbatim-entry
    /// spans are ALWAYS available (served from the resident source), as
    /// are target-entry spans in IdentityPassthrough mode (served from the
    /// resident source under the content mapping); Stretch-mode
    /// target-entry spans must sit inside the ring window `[produced −
    /// capacity, produced)` and defer while a published mapping awaits the
    /// producer (the ring still holds the previous mapping's bytes).
    fn check_spans(&self, offset: u64, len: u32, produced: u64) -> SpanCheck {
        let low_edge = produced.saturating_sub(self.ring.capacity as u64);
        let mut walked = 0u32;
        while walked < len {
            let position = offset + u64::from(walked);
            let span = self.layout.resolve(position, len - walked);
            if span.len == 0 {
                break; // EOF clamp
            }
            if let Region::EntryData {
                entry,
                offset: within,
            } = span.region
            {
                if entry == self.layout.target_entry_index && self.serve_mode == ServeMode::Stretch
                {
                    if self.mapping_pending().is_some() {
                        return SpanCheck::NotProduced;
                    }
                    if position + u64::from(span.len) > produced {
                        return SpanCheck::NotProduced;
                    }
                    if position < low_edge {
                        let align = u64::from(self.formats[entry].block_align());
                        let block_offset = within / align * align;
                        return SpanCheck::BehindWindow {
                            target: self.layout.entry_offsets[entry] + block_offset,
                        };
                    }
                }
            }
            walked += span.len;
        }
        SpanCheck::Available { total: walked }
    }

    /// Copy `total` (pre-validated) bytes starting at `offset` into `dest`,
    /// region by region.
    unsafe fn copy_spans(&self, offset: u64, total: u32, dest: *mut u8) {
        let mut walked = 0u32;
        while walked < total {
            let position = offset + u64::from(walked);
            let span = self.layout.resolve(position, total - walked);
            let out = dest.add(walked as usize);
            let span_len = span.len as usize;
            match span.region {
                Region::PreData { offset: pre } => {
                    std::ptr::copy_nonoverlapping(
                        self.layout.pre_data.as_ptr().add(pre),
                        out,
                        span_len,
                    );
                }
                Region::Gap => std::ptr::write_bytes(out, 0, span_len),
                Region::EntryData {
                    entry,
                    offset: within,
                } if entry != self.layout.target_entry_index => {
                    // Verbatim passthrough: the stock bytes from the
                    // resident source copy (resolve clamped `within` to
                    // the declared — stock — length).
                    std::ptr::copy_nonoverlapping(
                        self.source
                            .as_ptr()
                            .add(self.entry_source_offsets[entry] + within as usize),
                        out,
                        span_len,
                    );
                }
                Region::EntryData {
                    entry,
                    offset: within,
                } => match self.serve_mode {
                    ServeMode::Stretch => self.ring.copy_out(position, out, span_len),
                    ServeMode::IdentityPassthrough => {
                        self.copy_mapped_target(entry, within, out, span_len);
                    }
                },
                Region::Eof => return, // span.len == 0: nothing left
            }
            walked += span.len;
        }
    }

    /// The identity passthrough's mapped target-entry copy (training
    /// design §4.5; identity plans set target == main): within one span, walk the lead / content / tail sub-regions —
    /// silent-block tiling for the lead, verbatim source bytes at
    /// `within − lead + shift` for the content, silent tiling past the
    /// content end. The mapping is loaded ONCE per call (one packed word),
    /// so a span can never observe a torn pair. Allocation-free, log-free,
    /// panic-free: detour context.
    unsafe fn copy_mapped_target(&self, entry: usize, within: u64, dest: *mut u8, len: usize) {
        let align = u64::from(self.formats[entry].block_align());
        let (shift_blocks, lead_blocks) = self.content_mapping();
        let shift = shift_blocks * align;
        let lead = lead_blocks * align;
        let block = &self.silent_blocks[entry];
        let mut within = within;
        let mut out = dest;
        let mut remaining = len;
        while remaining > 0 {
            let (run, content_at) = if within < lead {
                ((lead - within).min(remaining as u64) as usize, None)
            } else {
                let content_pos = within - lead + shift;
                if content_pos < self.target_source_len {
                    (
                        (self.target_source_len - content_pos).min(remaining as u64) as usize,
                        Some(content_pos as usize),
                    )
                } else {
                    // Silent tail: tiles to the end of the span.
                    (remaining, None)
                }
            };
            match content_at {
                Some(content_pos) => std::ptr::copy_nonoverlapping(
                    self.source
                        .as_ptr()
                        .add(self.target_source_offset + content_pos),
                    out,
                    run,
                ),
                None => {
                    // Silent-block tiling: lead/shift are block multiples,
                    // so the virtual phase `within % align` is the block-
                    // internal offset in every silent sub-region.
                    for index in 0..run {
                        *out.add(index) = block[((within + index as u64) % align) as usize];
                    }
                }
            }
            within += run as u64;
            out = out.add(run);
            remaining -= run;
        }
    }

    /// Serve one read with entry data replaced by the pre-encoded silent
    /// block (req 28): the stream stays block-aligned valid ADPCM. Returns
    /// the EOF-clamped byte count.
    unsafe fn copy_spans_silent(&self, offset: u64, len: u32, dest: *mut u8) -> u32 {
        let mut walked = 0u32;
        while walked < len {
            let position = offset + u64::from(walked);
            let span = self.layout.resolve(position, len - walked);
            if span.len == 0 {
                break;
            }
            let out = dest.add(walked as usize);
            let span_len = span.len as usize;
            match span.region {
                Region::PreData { offset: pre } => {
                    std::ptr::copy_nonoverlapping(
                        self.layout.pre_data.as_ptr().add(pre),
                        out,
                        span_len,
                    );
                }
                Region::Gap => std::ptr::write_bytes(out, 0, span_len),
                Region::EntryData {
                    entry,
                    offset: within,
                } => {
                    if entry != self.layout.target_entry_index {
                        // Verbatim entries are static source data — no
                        // producer involvement, so silence-fill serves the
                        // real bytes.
                        std::ptr::copy_nonoverlapping(
                            self.source
                                .as_ptr()
                                .add(self.entry_source_offsets[entry] + within as usize),
                            out,
                            span_len,
                        );
                    } else {
                        let block = &self.silent_blocks[entry];
                        let align = block.len();
                        for index in 0..span_len {
                            *out.add(index) = block[(within as usize + index) % align];
                        }
                    }
                }
                Region::Eof => break,
            }
            walked += span.len;
        }
        walked
    }

    // ── Producer surface (generator-only) ────────────────────────────

    pub(crate) fn source(&self) -> &[u8] {
        &self.source
    }

    /// Absolute virtual offset of the TARGET (ring-served) entry's data —
    /// the ring's start and the producer's linear cursor origin (the
    /// verbatim entry never enters the ring).
    pub(crate) fn target_data_start(&self) -> u64 {
        self.layout.entry_offsets[self.layout.target_entry_index]
    }

    /// Exclusive end of the TARGET entry's data — the producer's linear
    /// cursor bound.
    pub(crate) fn target_data_end(&self) -> u64 {
        self.target_data_start()
            + self.layout.entries[self.layout.target_entry_index]
                .streamed
                .data_len as u64
    }

    pub(crate) fn stop_requested(&self) -> bool {
        self.stop.load(Ordering::Acquire) != 0
    }

    pub(crate) fn fault_kill_after_blocks(&self) -> u64 {
        self.fault_kill_after_blocks.load(Ordering::Acquire)
    }

    pub(crate) fn ring_produced(&self) -> u64 {
        self.ring.produced.load(Ordering::Acquire)
    }

    pub(crate) fn ring_capacity(&self) -> usize {
        self.ring.capacity
    }

    /// Producer-only: write `bytes` at the cursor and publish the new
    /// watermark (Release — the bytes happen-before any Acquire reader).
    pub(crate) unsafe fn ring_append(&self, at: u64, bytes: &[u8]) {
        self.ring.write(at, bytes);
        self.ring
            .produced
            .store(at + bytes.len() as u64, Ordering::Release);
    }

    /// Producer-only: rewind the watermark for regeneration. The seqlock
    /// counter is bumped FIRST so a racing reader discards its copy.
    pub(crate) fn ring_rewind(&self, new_produced: u64) {
        self.ring.rewinds.fetch_add(1, Ordering::AcqRel);
        self.ring.produced.store(new_produced, Ordering::Release);
    }

    /// Producer pacing bound: produce until `produced` reaches the engine's
    /// consumption high-water plus half the ring (the rest stays resident
    /// behind the cursor for short backward re-reads).
    pub(crate) fn pace_limit(&self) -> u64 {
        let ahead = (self.ring.capacity * PACE_NUMERATOR / PACE_DENOMINATOR) as u64;
        self.ring
            .consumed
            .load(Ordering::Acquire)
            .max(self.target_data_start())
            + ahead
    }

    /// Whether any pending slot is armed (an armed slot overrides pacing —
    /// the engine is literally waiting on those bytes).
    pub(crate) fn armed_slot_pending(&self) -> bool {
        self.pending
            .iter()
            .any(|slot| slot.state.load(Ordering::Acquire) == SLOT_ARMED)
    }

    /// Drain-side diagnostic: visit every ARMED pending slot as
    /// `(slot_index, offset, len, age_nanos, armed_at_nanos)` — the
    /// instrument for reads the engine is stuck waiting on (`armed_at`
    /// identifies the arm instance so the drain logs each once).
    pub fn for_each_armed_slot(&self, mut visit: impl FnMut(usize, u64, u32, u64, u64)) {
        for (index, slot) in self.pending.iter().enumerate() {
            if slot.state.load(Ordering::Acquire) != SLOT_ARMED {
                continue;
            }
            let armed_at = slot.armed_at_nanos.load(Ordering::Relaxed);
            let age = self.elapsed_nanos().saturating_sub(armed_at);
            visit(
                index,
                slot.offset.load(Ordering::Relaxed),
                slot.len.load(Ordering::Relaxed),
                age,
                armed_at,
            );
        }
    }

    /// Ring cursors snapshot (diagnostics): `(produced, consumed)`.
    #[must_use]
    pub fn ring_cursors(&self) -> (u64, u64) {
        (
            self.ring.produced.load(Ordering::Acquire),
            self.ring.consumed.load(Ordering::Acquire),
        )
    }

    /// Producer-only: take the lowest requested regeneration target.
    pub(crate) fn take_regen_target(&self) -> Option<u64> {
        let target = self.regen_target.swap(REGEN_NONE, Ordering::AcqRel);
        (target != REGEN_NONE).then_some(target)
    }

    pub(crate) fn add_frames_produced(&self, frames: u64) {
        self.metrics
            .frames_produced
            .fetch_add(frames, Ordering::Relaxed);
    }

    /// Record the producer's wall time (called once at thread exit).
    pub(crate) fn record_wall(&self) {
        self.metrics
            .wall_nanos
            .store(self.elapsed_nanos(), Ordering::Release);
    }

    /// Flip to SilenceFill (the producer-death containment boundary,
    /// req 28) and complete every armed slot with silent blocks. Never
    /// overrides Retired (retire wins). Panic-free by construction — it
    /// runs OUTSIDE the producer's `catch_unwind`.
    pub(crate) fn enter_silence_fill(&self) {
        let _ = self.state.compare_exchange(
            STATE_ACTIVE,
            STATE_SILENCE_FILL,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        if self.state.load(Ordering::Acquire) == STATE_SILENCE_FILL {
            for slot in &self.pending {
                self.complete_slot_silent(slot);
            }
        }
    }

    /// Producer-only: complete every armed slot whose range is now inside
    /// the produced window. The producer is the only writer, so no seqlock
    /// is needed here — `produced` cannot move under its feet.
    pub(crate) fn producer_complete_ready_slots(&self) {
        let produced = self.ring.produced.load(Ordering::Acquire);
        for slot in &self.pending {
            if slot.state.load(Ordering::Acquire) != SLOT_ARMED {
                continue;
            }
            let offset = slot.offset.load(Ordering::Relaxed);
            let len = slot.len.load(Ordering::Relaxed);
            match self.check_spans(offset, len, produced) {
                SpanCheck::Available { total } => {
                    if slot
                        .state
                        .compare_exchange(
                            SLOT_ARMED,
                            SLOT_COMPLETING,
                            Ordering::AcqRel,
                            Ordering::Relaxed,
                        )
                        .is_err()
                    {
                        continue;
                    }
                    let dest = slot.buffer.load(Ordering::Relaxed);
                    let accumulator = slot.accumulator.load(Ordering::Relaxed);
                    // SAFETY: the engine keeps the request's buffer and
                    // accumulator valid until consumption (serve contract).
                    unsafe {
                        self.copy_spans(offset, total, dest);
                        *accumulator += u64::from(total);
                    }
                    self.note_deferral_latency(slot);
                    self.ring
                        .consumed
                        .fetch_max(offset + u64::from(total), Ordering::AcqRel);
                    slot.state.store(SLOT_COMPLETE, Ordering::Release);
                }
                SpanCheck::BehindWindow { target } => {
                    self.regen_target.fetch_min(target, Ordering::AcqRel);
                }
                SpanCheck::NotProduced => {}
            }
        }
    }
}
