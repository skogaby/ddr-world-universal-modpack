//! Ownership of the streaming wave-bank create/unregister hooks: the
//! identity-protocol wrappers, and the bind/unbind composition the windows
//! detours run around [`super::transaction::call_create`] (design req 23,
//! 26). The composition helpers are cfg-agnostic so the host suites drive
//! the full bind → create → commit/late-fail/unregister matrix.

use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use super::binding::{
    prepare_binding, qualify_bind, BindQualification, BindRefusal, BindingRegistry, SourceView,
};
use super::clock_patch::{self, RatePublication};
use super::lifecycle::LifecycleState;
use super::transaction::{BindOutcome, CreateOutcome, FaultSelector, MAINTENANCE_CAPACITY};
use super::xact_runtime::{
    attach_slot_to_current, current_frame, enter_frame, BankTimeline, MaintenanceEvent,
    MaintenanceKind, MaintenanceQueue, RedirectToken, XactSlots,
};
#[cfg(windows)]
use super::xact_runtime::{BankCreatePath, BankEventKind};

#[cfg(windows)]
use crate::core::{hooks, signatures::SignatureStore};
#[cfg(windows)]
use crate::{log_info, log_warn};
#[cfg(windows)]
use retour::GenericDetour;
#[cfg(windows)]
use std::ptr::{addr_of, addr_of_mut};

static SLOTS: OnceLock<XactSlots> = OnceLock::new();
static MAINTENANCE: OnceLock<MaintenanceQueue<MAINTENANCE_CAPACITY>> = OnceLock::new();
/// Diagnostic bank-event timeline (drained by the maintenance thread; only
/// recorded into on diagnostic boots).
static TIMELINE: OnceLock<BankTimeline> = OnceLock::new();
static CREATE_READY: AtomicBool = AtomicBool::new(false);
static UNREGISTER_READY: AtomicBool = AtomicBool::new(false);

/// The shared slot table (set at init; the binding path and maintenance
/// drain consume it).
pub fn slots() -> Option<&'static XactSlots> {
    SLOTS.get()
}

/// The shared fixed maintenance queue.
pub fn maintenance() -> Option<&'static MaintenanceQueue<MAINTENANCE_CAPACITY>> {
    MAINTENANCE.get()
}

/// The diagnostic bank-event timeline.
pub fn timeline() -> Option<&'static BankTimeline> {
    TIMELINE.get()
}

/// Record one bank event onto the timeline (detour-legal: two atomics and a
/// QPC read; never allocates, locks, or logs). No-op until init or on
/// ordinary boots where no non-identity generation has armed.
#[cfg(windows)]
fn record_bank_event(kind: BankEventKind, file_id: i32, status: u8, path: BankCreatePath) {
    if !super::runtime::rate_recording_active() {
        return;
    }
    if let Some(timeline) = TIMELINE.get() {
        timeline.record(kind, file_id, status, path);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdentityReadiness {
    pub clock: bool,
    pub wavebank_create: bool,
    pub wavebank_unregister: bool,
    /// The streaming binding integration (`binding::integration_available`).
    /// Structurally false until plan Step 4 installs the XACT file-IO
    /// callback detour pair — which keeps `integration_ready()` false and
    /// the SONG SPEED row unregistered through the identity-only base.
    pub binding: bool,
    pub movie_policy: bool,
}

impl IdentityReadiness {
    #[must_use]
    pub const fn is_ready(self) -> bool {
        self.clock
            && self.wavebank_create
            && self.wavebank_unregister
            && self.binding
            && self.movie_policy
    }
}

pub fn call_create_identity<O, P>(file_id: i32, original: O, post: P) -> u8
where
    O: FnOnce(i32) -> u8,
    P: FnOnce(),
{
    let frame = enter_frame(file_id).ok();
    let result = original(file_id);
    if frame.is_some() {
        let _ = std::panic::catch_unwind(AssertUnwindSafe(post));
    }
    drop(frame);
    result
}

pub fn call_unregister_identity<O, P>(file_id: i32, original: O, post: P)
where
    O: FnOnce(i32),
    P: FnOnce(),
{
    original(file_id);
    let _ = std::panic::catch_unwind(AssertUnwindSafe(post));
}

/// Pin on the DELETED LayeredFS song-rate seam: song-rate no longer supplies
/// any dynamic path replacement (the streaming design binds inside
/// `wavebank_create` instead — design req 25). Consumed by the host
/// validator's `identity_no_dynamic_redirect` check.
#[must_use]
pub fn identity_conversion_path(_normalized_path: &str, _effective_source: &str) -> Option<String> {
    None
}

// ── The bind/unbind composition (cfg-agnostic, host-tested) ──────────

/// Everything the pre-original bind step touches, injected so the host
/// suites run the full composition (the windows `create_hook` builds one
/// over the process statics).
pub struct BindContext<'a> {
    pub lifecycle: &'a LifecycleState,
    pub slots: &'a XactSlots,
    pub registry: &'a BindingRegistry,
    pub publication: &'a RatePublication,
    pub fault: FaultSelector,
    pub owner_thread: u64,
    /// Bind-time content pre-shift `(shift_ms, lead_ms)` (R15/training
    /// design §4.5): applied to the fresh binding BEFORE publication, so
    /// the first byte the engine ever reads is already shifted — bank
    /// prepare's buffering reads begin the instant the binding is visible,
    /// so a post-publication mapping call would lose that race by design.
    /// `(0, 0)` = unmapped (every non-training caller).
    pub initial_mapping_ms: (u64, u64),
}

/// A first bind's refusal lands EarlyFailed (design req 24: the original
/// runs unbound, the song plays stock at 100%, one bounded WARN via the
/// drain). Best-effort phase advance — a contended guard cannot make the
/// refusal any more failed than it already is.
fn first_bind_refusal(ctx: &BindContext<'_>, refusal: BindRefusal, file_id: i32, generation: u64) {
    ctx.registry.note_refusal(refusal, file_id);
    let _ = ctx.lifecycle.mark_early_failed(generation);
}

/// A Quick-Restart re-bind refusal keeps the generation Committed (the
/// gameplay-exit boundary completes it normally) but must not leave the
/// committed non-identity Q31 live against the stock audio the re-created
/// bank will now carry — the clock resets to identity (conservative:
/// taint/ledger stay, applied once per generation).
fn quick_restart_refusal(ctx: &BindContext<'_>, refusal: BindRefusal, file_id: i32) {
    ctx.registry.note_refusal(refusal, file_id);
    let _ = ctx.publication.reset_identity();
}

/// The pre-original bind step (design req 23): qualify → preflight →
/// slot expose (the LAST fallible step) → registry publish. Runs inside
/// `call_create`'s pre-original containment on the game thread during the
/// loading screen — allocation is legal, logging is not (refusals go
/// through the registry mailbox to the drain). `song_code` and `source`
/// are the caller-resolved dance-bank identity and FileManager-row view
/// (hosts inject fixtures; windows resolves through the task-01 file-table
/// derivation).
pub fn bind_for_create(
    ctx: &BindContext<'_>,
    file_id: i32,
    song_code: Option<&str>,
    source: Option<&SourceView<'_>>,
) -> BindOutcome {
    let qualification = qualify_bind(ctx.lifecycle, song_code);
    if qualification == BindQualification::Decline {
        return BindOutcome::Stock;
    }
    let generation = ctx.lifecycle.generation();
    let percent = ctx.lifecycle.requested_percent();
    let refuse = |refusal: BindRefusal| {
        match qualification {
            BindQualification::FirstBind => first_bind_refusal(ctx, refusal, file_id, generation),
            BindQualification::QuickRestart => quick_restart_refusal(ctx, refusal, file_id),
            BindQualification::Decline => unreachable!("declined above"),
        }
        BindOutcome::Refused
    };

    if qualification == BindQualification::FirstBind {
        // The generation binds to THIS song at its first bind (arming is
        // song-agnostic); the digest was validated/derived by the caller.
        if let Some(code) = song_code {
            ctx.lifecycle
                .bind_song(super::binding::song_code_digest(code));
        }
        if ctx.lifecycle.begin_binding(generation).is_err() {
            return refuse(BindRefusal::SlotExpose);
        }
    }

    let Some(source) = source else {
        return refuse(BindRefusal::SourceRead);
    };
    let preserve_pitch = ctx.lifecycle.preserve_pitch();
    let binding = match prepare_binding(
        file_id,
        generation,
        percent as u32,
        preserve_pitch,
        source,
        &ctx.fault,
        crate::core::xact::virtual_bank::StretchTarget::Main,
    ) {
        Ok(binding) => binding,
        Err(refusal) => return refuse(refusal),
    };

    // Bind-time pre-shift (R15): the mapping lands on the binding while it
    // is still private to this call — before the slot expose and registry
    // publication make it readable. Failure to apply (out-of-range values)
    // fails open to unmapped: the song simply plays from its start.
    let (shift_ms, lead_ms) = ctx.initial_mapping_ms;
    if shift_ms != 0 || lead_ms != 0 {
        let _ = binding.set_content_mapping(
            binding.ms_to_blocks(shift_ms),
            binding.ms_to_blocks(lead_ms),
        );
    }

    // The expose tail: claim the slot with the live frame identity, expose
    // the token, attach, and advance the lifecycle — each failure retires
    // the just-built binding (stopping its producer) before refusing. The
    // registry publication after a successful tail is infallible.
    let Some(frame) = current_frame().filter(|frame| frame.file_id == file_id) else {
        binding.retire();
        return refuse(BindRefusal::SlotExpose);
    };
    let Some(slot) = ctx
        .slots
        .claim(ctx.owner_thread, frame.nonce, frame.depth, file_id)
    else {
        binding.retire();
        return refuse(BindRefusal::SlotExpose);
    };
    let token = RedirectToken {
        call_nonce: frame.nonce,
        call_depth: frame.depth,
        generation,
        requested_percent: percent,
        participant_mask: ctx.lifecycle.participant_mask(),
        stage_index: ctx.lifecycle.stage_index(),
        effective_rate: binding.rate(),
    };
    let exposed = ctx
        .slots
        .expose(
            slot,
            ctx.owner_thread,
            frame.nonce,
            frame.depth,
            file_id,
            token,
        )
        .and_then(|()| {
            attach_slot_to_current(frame.nonce, slot)
                .map_err(|_| super::xact_runtime::SlotError::IdentityMismatch)
        });
    if exposed.is_err() {
        let _ = ctx.slots.abandon(slot);
        binding.retire();
        return refuse(BindRefusal::SlotExpose);
    }
    let advanced = match qualification {
        BindQualification::FirstBind => ctx.lifecycle.mark_exposed(generation),
        BindQualification::QuickRestart => ctx.lifecycle.mark_reexposed(generation),
        BindQualification::Decline => unreachable!("declined above"),
    };
    if advanced.is_err() {
        let _ = ctx.slots.abandon(slot);
        binding.retire();
        return refuse(BindRefusal::SlotExpose);
    }
    ctx.registry.publish(binding);
    BindOutcome::Bound
}

/// The unregister detour's PRE-ORIGINAL step (design req 26): retire the
/// binding before the original destroys the bank and closes the handle
/// (state → Retired, armed slots cancelled with the EOF-clamp semantics),
/// release the committed transaction slot, and enqueue the reclamation
/// record the drain consumes. Atomics-only — detour-legal. Returns whether
/// a binding was retired.
pub fn unregister_prelude(
    registry: &BindingRegistry,
    slots: &XactSlots,
    maintenance: &MaintenanceQueue<MAINTENANCE_CAPACITY>,
    file_id: i32,
) -> bool {
    let retired = registry.retire_by_file(file_id);
    if let Ok(index) = slots.begin_release_by_file(file_id) {
        // A full queue leaves the slot pinned (bounded leak; the binding
        // itself is still reclaimed by the drain's retired-list sweep).
        let _ = maintenance.push(MaintenanceEvent {
            kind: MaintenanceKind::ReclaimBinding,
            slot_index: index as u8,
        });
    }
    retired
}

/// The create detour's POST-`call_create` cleanup: a create that failed (or
/// fell to conservative recovery) after this call's bind published a
/// binding must retire it — XACT rejected the bank, so nothing will ever
/// read it, and Q31 was never published (design req 23's late-fail leg).
/// Atomics-only — legal right after the post-original phase.
pub fn retire_after_create(
    registry: &BindingRegistry,
    outcome: CreateOutcome,
    bound: bool,
    file_id: i32,
) -> bool {
    if !bound {
        return false;
    }
    match outcome {
        CreateOutcome::LateFailed { .. } | CreateOutcome::RecoveryFailed => {
            registry.retire_by_file(file_id)
        }
        _ => false,
    }
}

#[must_use]
pub fn readiness(movie_policy: bool) -> IdentityReadiness {
    IdentityReadiness {
        clock: clock_patch::is_installed(),
        wavebank_create: CREATE_READY.load(Ordering::Acquire),
        wavebank_unregister: UNREGISTER_READY.load(Ordering::Acquire),
        binding: super::binding::integration_available(),
        movie_policy,
    }
}

pub fn drain_maintenance(mut consume: impl FnMut(MaintenanceEvent)) {
    let Some(queue) = MAINTENANCE.get() else {
        return;
    };
    while let Some(event) = queue.pop() {
        consume(event);
    }
}

#[cfg(windows)]
type WavebankCreateFn = unsafe extern "C" fn(i32) -> u8;
#[cfg(windows)]
type WavebankUnregisterFn = unsafe extern "C" fn(i32);

#[cfg(windows)]
static mut CREATE_HOOK: Option<GenericDetour<WavebankCreateFn>> = None;
#[cfg(windows)]
static mut UNREGISTER_HOOK: Option<GenericDetour<WavebankUnregisterFn>> = None;

/// The audio file-table global (task-01's `song_rate_file_table`
/// derivation), stashed at init for the create detour's source/path row
/// resolution. Zero when the signature did not resolve — every lookup then
/// returns `None` (defensive only: without the full callback signature set
/// task-04's readiness conjunction keeps the feature unarmed anyway).
#[cfg(windows)]
static FILE_TABLE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Dereference the file-table global to the live table object.
///
/// Layout (RE note §6, verified across all four builds): the global holds
/// a pointer; data rows at `*(obj+0x8)` stride 0x40 (buffer ptr +0x8, size
/// u32 +0x14); path rows at `*(obj+0x28)` stride 0xA0 (NUL-terminated path
/// at +0x11 — the exact bytes stock passes to `avs_fs_convert_path`).
#[cfg(windows)]
unsafe fn file_table_object() -> Option<*const u8> {
    let global = FILE_TABLE.load(Ordering::Acquire);
    if global == 0 {
        return None;
    }
    let object = *(global as *const *const u8);
    (!object.is_null()).then_some(object)
}

/// The FileManager row's RAM buffer for `file_id` (the stock bank the
/// game itself loaded at song confirm — the generator's source, req 17).
#[cfg(windows)]
unsafe fn file_table_source(file_id: i32) -> Option<(*const u8, usize)> {
    if file_id < 0 {
        return None;
    }
    let object = file_table_object()?;
    let rows = *(object.add(0x8) as *const *const u8);
    if rows.is_null() {
        return None;
    }
    let row = rows.add(file_id as usize * 0x40);
    let buffer = *(row.add(0x8) as *const *const u8);
    let size = *(row.add(0x14) as *const u32);
    if buffer.is_null() || size == 0 {
        return None;
    }
    Some((buffer, size as usize))
}

/// The file-table row's virtual path for `file_id` (bounded by the row's
/// inline buffer — the SSO flag byte sits at +0x8F, so 0x7E bytes is the
/// hard cap).
#[cfg(windows)]
unsafe fn file_table_path(file_id: i32) -> Option<String> {
    if file_id < 0 {
        return None;
    }
    let object = file_table_object()?;
    let rows = *(object.add(0x28) as *const *const u8);
    if rows.is_null() {
        return None;
    }
    let path = rows.add(file_id as usize * 0xA0).add(0x11);
    let mut bytes = Vec::new();
    for index in 0..0x7E {
        let byte = *path.add(index);
        if byte == 0 {
            break;
        }
        bytes.push(byte);
    }
    if bytes.is_empty() {
        return None;
    }
    String::from_utf8(bytes).ok()
}

/// Windows glue for the selected-song publication: resolve the create's
/// FileManager path + resident bytes and hand them to
/// [`super::selected_song::publish_from_bank`]. Every failure (table
/// unresolved, row empty, non-dance path, parse error) publishes nothing
/// and returns `None`; a dance bank returns its published digest — the
/// same-call "song being created" identity the bind's pre-shift
/// coherence check consumes.
#[cfg(windows)]
unsafe fn publish_selected_song(file_id: i32) -> Option<u64> {
    let path = file_table_path(file_id)?;
    // Per-song judgement-offsets identity observer (D21): dance-bank
    // creates fire once per stage load — normal play, EACH course/dan
    // stage (courses batch-preload SSQs at course start, so the SSQ
    // observer alone misidentifies stages 2+), and training mode. The
    // callout is a no-op when that mod is inactive.
    if let Some(code) = super::binding::dance_bank_song_code(&path) {
        crate::mods::per_song_judgement_offsets::override_hook::on_dance_bank(&code);
    }
    let (buffer, size) = file_table_source(file_id)?;
    let bytes = std::slice::from_raw_parts(buffer, size);
    super::selected_song::publish_from_bank(&path, bytes)
}

/// The file-table row's virtual path for `file_id` (the preview branch's
/// dance-path resolution — same source the bind closure uses).
#[cfg(windows)]
pub(super) fn create_path(file_id: i32) -> Option<String> {
    unsafe { file_table_path(file_id) }
}

/// The FileManager row's resident buffer for `file_id` (the preview
/// branch's source view; valid for the duration of the create call).
#[cfg(windows)]
pub(super) fn create_source(file_id: i32) -> Option<(*const u8, usize)> {
    unsafe { file_table_source(file_id) }
}

/// The file-table row's load-state dword (`row+0x20` — the AudioLoader
/// tick's gate values; preview design §Components 5's restart
/// preconditions consume it in plan Step 5).
#[cfg(windows)]
pub(super) fn file_table_state(file_id: i32) -> Option<u32> {
    if file_id < 0 {
        return None;
    }
    unsafe {
        let object = file_table_object()?;
        let rows = *(object.add(0x8) as *const *const u8);
        if rows.is_null() {
            return None;
        }
        Some(*(rows.add(file_id as usize * 0x40).add(0x20) as *const u32))
    }
}

#[cfg(windows)]
unsafe extern "C" fn create_hook(file_id: i32) -> u8 {
    let Some(hook) = (&*addr_of!(CREATE_HOOK)).as_ref() else {
        return 0;
    };
    // Selected-song publication (training design §4.6): every dance-bank
    // create — armed or not, preview or gameplay, on BOTH the degraded and
    // full paths below — publishes {code_digest, audio_len_ms} from the
    // resident header. Non-dance banks return after two row derefs; the
    // path/parse work matches what the bind closure already does
    // pre-original (game thread, loading screen — allocation legal,
    // logging is not; publish-nothing on any resolution/parse failure).
    // The returned digest is this very bank's identity — the pre-shift
    // coherence check's "fresh" side (a stale-song mapping must not bind
    // into this create; the fast-confirm race).
    let created_digest = publish_selected_song(file_id);
    let original = |id| unsafe { hook.call(id) };
    // The full transaction needs every shared piece; if anything is absent
    // (identity-only boots, partial init) the identity protocol runs instead
    // and no binding is possible.
    let (Some(slots), Some(maintenance), Some(publication)) =
        (SLOTS.get(), MAINTENANCE.get(), clock_patch::publication())
    else {
        let result = call_create_identity(file_id, original, || {});
        record_bank_event(BankEventKind::Create, file_id, result, BankCreatePath::None);
        return result;
    };
    struct GuardTaint;
    impl super::transaction::SessionTaint for GuardTaint {
        fn taint(&self, side: usize) {
            crate::services::score_guard::mark_session_tainted(side);
        }
    }
    static TAINT: GuardTaint = GuardTaint;
    fn confirm_movie() {
        crate::services::movie_policy::set_suppressed(
            crate::services::movie_policy::MovieSuppressor::SongRate,
            true,
        );
    }
    let lifecycle = super::runtime::lifecycle();
    let fault = super::runtime::fault_selector();
    let parts = super::transaction::TransactionParts {
        slots,
        maintenance,
        publication,
        ledger: crate::services::score_guard::rate_ledger(),
        lifecycle,
        confirm_movie: &confirm_movie,
        taint_session: &TAINT,
        fault,
    };
    let owner = u64::from(unsafe { windows::Win32::System::Threading::GetCurrentThreadId() });
    let registry = super::binding::registry();
    // The bind closure runs pre-original inside `call_create`'s panic
    // containment (game thread, loading screen — allocation legal). The
    // one-atomic-load gate keeps every ordinary create at zero extra cost.
    // The song-select PREVIEW branch (preview design §Architecture Flow 1)
    // runs whenever the gameplay path resolves to Stock — including the
    // fast identity-phase exit, which is exactly the song-select case —
    // and never changes the gameplay outcome (design R8: the transaction
    // must not see a preview bind).
    let bound = std::cell::Cell::new(false);
    let bind = |id: i32| {
        let outcome = if !super::binding::bind_may_qualify(lifecycle) {
            BindOutcome::Stock
        } else {
            let song_code = unsafe { file_table_path(id) }
                .and_then(|path| super::binding::dance_bank_song_code(&path));
            let context = BindContext {
                lifecycle,
                slots,
                registry,
                publication,
                fault,
                owner_thread: owner,
                initial_mapping_ms: super::runtime::initial_content_mapping_coherent(
                    created_digest,
                ),
            };
            let source = unsafe { file_table_source(id) }
                .and_then(|(ptr, len)| unsafe { SourceView::from_raw(ptr, len) });
            let outcome = bind_for_create(&context, id, song_code.as_deref(), source.as_ref());
            if outcome == BindOutcome::Bound {
                bound.set(true);
            }
            outcome
        };
        if outcome == BindOutcome::Stock {
            super::preview::maybe_bind_preview(id);
        }
        outcome
    };
    let (result, outcome) = super::transaction::call_create(&parts, file_id, owner, bind, original);
    // A rejected (or conservatively recovered) create retires the binding
    // this call published — atomics-only, detour-legal.
    let _ = retire_after_create(registry, outcome, bound.get(), file_id);
    let path = match outcome {
        super::transaction::CreateOutcome::Stock => BankCreatePath::Stock,
        super::transaction::CreateOutcome::Committed { .. } => BankCreatePath::Committed,
        super::transaction::CreateOutcome::LateFailed { .. } => BankCreatePath::LateFailed,
        super::transaction::CreateOutcome::RecoveryFailed => BankCreatePath::RecoveryFailed,
        super::transaction::CreateOutcome::TlsOverflow => BankCreatePath::TlsOverflow,
    };
    record_bank_event(BankEventKind::Create, file_id, result, path);
    result
}

#[cfg(windows)]
unsafe extern "C" fn unregister_hook(file_id: i32) {
    let Some(hook) = (&*addr_of!(UNREGISTER_HOOK)).as_ref() else {
        return;
    };
    // PRE-ORIGINAL (design req 26): retire the binding before the original
    // destroys the bank and closes the handle; reclamation is the drain's.
    if let (Some(slots), Some(maintenance)) = (SLOTS.get(), MAINTENANCE.get()) {
        let _ = unregister_prelude(super::binding::registry(), slots, maintenance, file_id);
    }
    call_unregister_identity(
        file_id,
        |id| unsafe { hook.call(id) },
        || record_bank_event(BankEventKind::Unregister, file_id, 0, BankCreatePath::None),
    );
}

#[cfg(windows)]
pub fn init(signatures: &SignatureStore) -> bool {
    if CREATE_READY.load(Ordering::Acquire) && UNREGISTER_READY.load(Ordering::Acquire) {
        return true;
    }
    let (Some(create), Some(unregister)) = (
        signatures.get_address("song_rate_wavebank_create"),
        signatures.get_address("song_rate_wavebank_unregister"),
    ) else {
        log_warn!("song_rate: wave-bank signatures unavailable");
        return false;
    };
    let _ = SLOTS.set(XactSlots::new());
    let _ = MAINTENANCE.set(MaintenanceQueue::new());
    let _ = TIMELINE.set(BankTimeline::new());
    // The bind preflight's source/path resolution (absent ⇒ every lookup
    // is None ⇒ SourceRead refusal — and task-04's readiness conjunction
    // keeps the feature unarmed without the full callback signature set).
    if let Some(table) = signatures.get_address("song_rate_file_table") {
        FILE_TABLE.store(table as usize, Ordering::Release);
    }
    let create: WavebankCreateFn = unsafe { std::mem::transmute(create) };
    let unregister: WavebankUnregisterFn = unsafe { std::mem::transmute(unregister) };
    unsafe {
        if let Err(error) = hooks::install_enabled(addr_of_mut!(CREATE_HOOK), create, create_hook) {
            log_warn!("song_rate: wavebank_create hook failed: {}", error);
            return false;
        }
        CREATE_READY.store(true, Ordering::Release);
        if let Err(error) =
            hooks::install_enabled(addr_of_mut!(UNREGISTER_HOOK), unregister, unregister_hook)
        {
            if let Some(hook) = (&*addr_of!(CREATE_HOOK)).as_ref() {
                let _ = hook.disable();
            }
            *addr_of_mut!(CREATE_HOOK) = None;
            CREATE_READY.store(false, Ordering::Release);
            log_warn!("song_rate: wavebank_unregister hook failed: {}", error);
            return false;
        }
    }
    UNREGISTER_READY.store(true, Ordering::Release);
    log_info!("song_rate: identity wave-bank hooks installed");
    true
}
