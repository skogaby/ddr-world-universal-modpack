//! Song-select preview-rate policy (preview design §Components 4–5): the
//! pure qualification the create detour's preview branch runs, the
//! mod-driven feature gate, the scene-exit force-retire, and the restart
//! half's derivation stash + loader-chain resolver (design §Components 5
//! step 1 / §Components 6).
//!
//! Preview bindings are an AUDIO-SERVING concern only (design R8): they
//! never publish Q31, never touch the score ledger or session taint,
//! never set movie suppression, and never enter the lifecycle state
//! machine or the transaction slot table. The gameplay transaction always
//! sees a preview-bound create as `BindOutcome::Stock`. Every failure at
//! every stage fails open to a stock preview (design R9) — refusals go
//! through the registry's preview mailbox to the maintenance drain; the
//! detour branch itself never logs.
//!
//! The live-edit restart executor (design §Components 5 steps 0–5) runs
//! on the input manager's per-frame callback (the render/game thread —
//! the one context the loader walk and the stock stop/unregister/create
//! calls are legal in): [`request_refresh`] is its atomics-only intake,
//! [`RefreshCell`] the 150 ms debounce + supersession policy, and the
//! preview play watchdog (design amendment 2026-08-16) rides the same
//! frame callback. This module also carries the executor's address
//! foundation: the all-or-nothing [`init_restart`] stash (two vftable
//! identity gates + three stock functions) and [`resolve_loader`], the
//! validated walk to the live `sequence::AudioLoader`.

#[cfg(windows)]
use std::sync::atomic::AtomicPtr;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

use super::lifecycle::{is_supported_rate_percent, IDENTITY_PERCENT};
use crate::types::scenes::scene;

/// One qualified preview bind: the controlling side's settings for the
/// create being intercepted (preview design §Data Models).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreviewBindRequest {
    /// The controlling side (the single entered side; P1 in versus).
    pub side: u8,
    /// The desired rate percent (≠ 100, in the supported scalar domain).
    pub percent: i32,
    /// The controlling side's DSP mode (true = WSOLA, false = resample).
    pub preserve_pitch: bool,
}

/// The qualification's inputs, gathered by the detour branch (windows) or
/// injected by host tests. `None`/unreadable anywhere fails closed.
pub struct QualifyInputs<'a> {
    /// The mod-driven gate ([`feature_active`]).
    pub feature_active: bool,
    /// The current scene id (`scene_manager::current_scene()`).
    pub scene: i32,
    /// The create's dance-bank song code (`dance_bank_song_code` on the
    /// FileManager path) — `None` for every non-dance bank (named banks,
    /// `custom_bgm_%04d`, unresolvable rows).
    pub song_code: Option<&'a str>,
    /// Per-side entered flags (`stage_records::side_entered`); `None` =
    /// unreadable.
    pub entered: [Option<bool>; 2],
    /// Per-side desired rate percents (the option rows' atomics).
    pub desired: [i32; 2],
    /// Per-side preserve-pitch flags.
    pub preserve: [bool; 2],
}

/// The preview branch's pure decision (host-tested; design §Components 4):
/// a governed session at song select, on a dance bank, desiring a supported
/// non-100% rate. Solo/doubles: the single entered side governs. Versus:
/// P1 governs (mirroring the gameplay classifier — the SONG SPEED mod
/// mirrors both sides' rows, P1 being the authoritative seed). Empty
/// sessions and ANY unreadable entered flag decline to a stock preview.
#[must_use]
pub fn qualify(inputs: &QualifyInputs<'_>) -> Option<PreviewBindRequest> {
    if !inputs.feature_active || inputs.scene != scene::SONG_SELECT {
        return None;
    }
    inputs.song_code?;
    let side = match (inputs.entered[0]?, inputs.entered[1]?) {
        (true, false) => 0u8,
        (false, true) => 1u8,
        // Versus: the shared rate, P1 governing (gameplay-classifier
        // parity — see `classify_scene26`).
        (true, true) => 0u8,
        // Empty sessions preview stock.
        (false, false) => return None,
    };
    let percent = inputs.desired[usize::from(side)];
    if percent == IDENTITY_PERCENT || !is_supported_rate_percent(percent) {
        return None;
    }
    Some(PreviewBindRequest {
        side,
        percent,
        preserve_pitch: inputs.preserve[usize::from(side)],
    })
}

/// The mod-driven feature gate: true while the `song-playback-speed` mod
/// is enabled (design R11 — no config surface; the mod IS the switch).
static FEATURE_ACTIVE: AtomicBool = AtomicBool::new(false);

pub fn set_feature_active(on: bool) {
    FEATURE_ACTIVE.store(on, Ordering::Release);
    if !on {
        // Hygiene: a pending refresh must not survive a disable/re-enable
        // cycle (the executor's feature gate would suppress it anyway).
        REFRESH_CELL.clear();
    }
}

#[must_use]
pub fn feature_active() -> bool {
    FEATURE_ACTIVE.load(Ordering::Acquire)
}

/// Live-edit refresh intake (design §Components 5/7): stamped by the
/// option rows' change callbacks — atomics only (one seqlock read + three
/// relaxed-class stores; the option-callback contract). Latches the
/// selected-song publication generation for the executor's supersession
/// check (design Flow 2 step 0): a wheel settle between the stamp and the
/// fire already created a bank that qualified with the newest desired
/// values, making the restart redundant.
pub fn request_refresh() {
    if !feature_active() {
        return;
    }
    let settle = super::selected_song::selected_song()
        .map(|info| info.generation)
        .unwrap_or(0);
    REFRESH_CELL.stamp_at(refresh_now_nanos(), settle);
}

/// The scene-callback registration latch (init is idempotent).
static SCENE_CALLBACK: AtomicUsize = AtomicUsize::new(0);
/// The frame-callback registration latch (independent of the scene one —
/// each half degrades on its own).
static FRAME_CALLBACK: AtomicUsize = AtomicUsize::new(0);

/// Register the scene-exit defense (design R7) and the per-frame restart
/// executor. Leaving SONG_SELECT force-retires any live preview binding —
/// the natural unregister covers the common paths (wheel move, song
/// confirm); this covers redirected exits. The frame callback drives the
/// debounced restart + the preview play watchdog on the render/game
/// thread ([`executor_frame`]). Idempotent; independent of the feature
/// gate (a disabled mod has no preview binding to retire, and the
/// executor's first load declines).
#[cfg(windows)]
pub fn init() -> bool {
    if FRAME_CALLBACK.load(Ordering::Acquire) == 0 {
        let id = crate::services::input_manager::on_frame(std::sync::Arc::new(executor_frame));
        // usize::MAX (poisoned-lock sentinel) still latches: registration
        // will never succeed on a poisoned manager, and the executor's
        // absence only degrades live edits to next-settle (fail-open).
        FRAME_CALLBACK.store(id.max(1), Ordering::Release);
    }
    if SCENE_CALLBACK.load(Ordering::Acquire) != 0 {
        return true;
    }
    if !crate::services::scene_manager::is_available() {
        return false;
    }
    let id = crate::services::scene_manager::on_scene_change(Box::new(|prev, next| {
        if prev == scene::SONG_SELECT && next != scene::SONG_SELECT {
            let _ = super::binding::registry().retire_preview();
        }
    }));
    SCENE_CALLBACK.store(id.max(1), Ordering::Release);
    true
}

/// Force-retire any live preview binding (the mod-disable path).
pub fn retire_now() {
    let _ = super::binding::registry().retire_preview();
}

/// The create detour's preview branch (design §Architecture Flow 1): runs
/// pre-original inside the bind closure's containment AFTER the gameplay
/// path resolved to Stock. Gathers the qualification inputs (cheapest
/// gates first — one atomic for the feature, one for the scene),
/// preflights a `StretchTarget::Side` binding over the resident source,
/// and publishes it into the registry's preview slot. Never changes the
/// gameplay outcome; never logs (refusals go to the preview mailbox, the
/// publish event is reported by the drain's latch). Allocation is legal
/// here (game thread, song select, pre-original) — the same context the
/// gameplay preflight runs in at the loading screen.
#[cfg(windows)]
pub fn maybe_bind_preview(file_id: i32) {
    use super::binding::{self, SourceView};
    use crate::core::xact::virtual_bank::StretchTarget;

    if !feature_active() {
        return;
    }
    let scene = crate::services::scene_manager::current_scene();
    if scene != scene::SONG_SELECT {
        return;
    }
    // Both sides at identity: exit before ANY game-memory read (R2's
    // zero-footprint contract — the row callbacks keep these atomics).
    let desired = [
        super::runtime::desired_percent(0),
        super::runtime::desired_percent(1),
    ];
    if desired.iter().all(|&percent| percent == IDENTITY_PERCENT) {
        return;
    }
    let path = match super::wavebank_hook::create_path(file_id) {
        Some(path) => path,
        None => return,
    };
    let song_code = binding::dance_bank_song_code(&path);
    let inputs = QualifyInputs {
        feature_active: true,
        scene,
        song_code: song_code.as_deref(),
        entered: [
            crate::services::stage_records::side_entered(0),
            crate::services::stage_records::side_entered(1),
        ],
        desired,
        preserve: [
            super::runtime::desired_preserve_pitch(0),
            super::runtime::desired_preserve_pitch(1),
        ],
    };
    let Some(request) = qualify(&inputs) else {
        return;
    };
    let registry = binding::registry();
    let Some(source) = super::wavebank_hook::create_source(file_id) else {
        registry.note_preview_refusal(binding::BindRefusal::SourceRead, file_id);
        return;
    };
    // SAFETY: the FileManager row's buffer is stable for the duration of
    // the create call (the same contract the gameplay preflight relies
    // on); `prepare_binding` copies before returning.
    let view = match unsafe { SourceView::from_raw(source.0, source.1) } {
        Some(view) => view,
        None => {
            registry.note_preview_refusal(binding::BindRefusal::SourceRead, file_id);
            return;
        }
    };
    match binding::prepare_binding(
        file_id,
        binding::next_preview_generation(),
        request.percent as u32,
        request.preserve_pitch,
        &view,
        &super::runtime::fault_selector(),
        StretchTarget::Side,
    ) {
        Ok(preview_binding) => {
            registry.publish_preview(preview_binding);
            // The drain's publish latch reports it; sweeping/reporting
            // needs the maintenance drain even on boots that never armed
            // a gameplay generation.
            super::runtime::ensure_maintenance_drain();
            // Loader-chain probe (plan Step-4 demo): the loader is live
            // mid-load at this instant (its ctor queued the loads whose
            // completion routed this create), and this is the game
            // thread — the one context the walk is legal in. Outcome via
            // atomic; the drain reports it (the detour never logs).
            probe_loader_chain();
        }
        Err(refusal) => {
            if refusal == binding::BindRefusal::UnsupportedProfile {
                // Forensics for the strict parser rejecting a resident
                // row the ENGINE plays fine (live incident 2026-08-16:
                // one row incarnation of a file id refused for ~32 s of
                // restarts, healed on natural release+reload). Re-run
                // the header parse to capture WHY (fails fast; failure
                // path only) — the refusal mailbox alone cannot
                // distinguish garbage bytes from truncation from a real
                // format oddity. Allocation is legal here; logging is
                // not (the drain reports the packet).
                stash_parse_forensics(file_id, &path, &view);
            }
            registry.note_preview_refusal(refusal, file_id);
        }
    }
}

/// Forensic packet for an `UnsupportedProfile` preview refusal: enough
/// of the resident row to classify the failure from the log alone —
/// `head` holds magic/version/header-version/segment-table start (a
/// freed/reused buffer shows garbage magic; a truncated load shows a
/// plausible header with an out-of-bounds segment; a genuine format
/// oddity shows a clean header and a deep parse error).
#[cfg(windows)]
pub(super) struct ParseForensics {
    pub file_id: i32,
    pub path: String,
    pub buffer_ptr: usize,
    pub buffer_len: usize,
    pub head: [u8; 32],
    pub row_state: Option<u32>,
    pub error: String,
}

/// Last-failure forensics cell (single packet — the storm case repeats
/// one file id, so the latest is the story). `try_lock` on both sides:
/// the detour branch must never block, and a dropped packet under
/// contention is fine (the next refusal re-stashes).
#[cfg(windows)]
static PARSE_FORENSICS: std::sync::Mutex<Option<ParseForensics>> = std::sync::Mutex::new(None);

#[cfg(windows)]
fn stash_parse_forensics(file_id: i32, path: &str, view: &super::binding::SourceView<'_>) {
    let bytes = view.bytes();
    let mut head = [0u8; 32];
    let take = bytes.len().min(head.len());
    head[..take].copy_from_slice(&bytes[..take]);
    let error = match crate::core::xact::xwb::parse_song_bank(bytes) {
        // The refusal said the parse failed; a success on re-parse means
        // the buffer CHANGED between the two reads (itself a finding —
        // record it rather than dropping the packet).
        Ok(_) => "reparse-succeeded (buffer changed between reads)".to_string(),
        Err(error) => format!("{error:?}"),
    };
    let packet = ParseForensics {
        file_id,
        path: path.to_string(),
        buffer_ptr: bytes.as_ptr() as usize,
        buffer_len: bytes.len(),
        head,
        row_state: super::wavebank_hook::file_table_state(file_id),
        error,
    };
    if let Ok(mut slot) = PARSE_FORENSICS.try_lock() {
        *slot = Some(packet);
    }
}

/// Take the latest forensic packet (drain consumer only).
#[cfg(windows)]
pub(super) fn take_parse_forensics() -> Option<ParseForensics> {
    PARSE_FORENSICS
        .try_lock()
        .ok()
        .and_then(|mut slot| slot.take())
}

// ── Restart half: derivations + loader-chain resolver ─────────────────
// (design §Components 5 step 1 / §Components 6; RE:
// research/preview-retrigger-re.md §1.1/§1.3/§3/§9)

/// The preview SE slot the game passes to `se_play` (RE §1.2 — slot 5
/// skips the SE mute filter).
pub const PREVIEW_SLOT: i32 = 5;
/// `sequence::AudioLoader` mode 1 = one-shot `se_play` — the preview
/// path (mode 0 is the BGM/loop play; RE §1.3).
pub const LOADER_MODE_ONESHOT: u8 = 1;

/// One live `sequence::AudioLoader`'s field snapshot (RE §1.3 layout).
/// Read on the game thread by [`resolve_loader`]; the pure
/// [`loader_sane`] predicate over it is host-tested.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoaderSnapshot {
    /// Cue handle (+0x10): −1 = the tick has not fired yet (or was
    /// re-armed); the restart sets it back to −1 to replay.
    pub handle: i32,
    /// Failed latch (+0x14): set when a `se_play` returned −1 — the
    /// loader never retries on its own (the watchdog's re-arm target).
    pub failed: bool,
    /// Play mode (+0x15): must be [`LOADER_MODE_ONESHOT`] for previews.
    pub mode: u8,
    /// SE slot (+0x18): must be [`PREVIEW_SLOT`] for previews.
    pub slot: i32,
    /// FileManager XWB / XSB file ids (+0x08/+0x0C): −1 = unresolved.
    pub xwb_id: i32,
    pub xsb_id: i32,
}

/// Pure field-sanity predicate (design §Components 5 step 1, the part
/// that needs no game memory): the snapshot must look like a live
/// PREVIEW loader — slot 5, one-shot mode, both file ids resolved.
/// Anything else means the chain walked to some other AudioLoader use
/// (or a mid-teardown one) and the executor must decline (fail-open).
/// The cue `_s`-suffix check is the executor's (Step 5 — it reads the
/// loader's cue string, which this snapshot deliberately excludes).
#[must_use]
pub fn loader_sane(snapshot: &LoaderSnapshot) -> bool {
    snapshot.slot == PREVIEW_SLOT
        && snapshot.mode == LOADER_MODE_ONESHOT
        && snapshot.xwb_id >= 0
        && snapshot.xsb_id >= 0
}

/// The five restart-half addresses (design §Components 6), stashed
/// all-or-nothing by [`init_restart`] at `Mod::init` time. Null = the
/// restart half is unavailable (Step-3 wheel-settle previews unaffected).
#[cfg(windows)]
static RESTART_VIEW_VFTABLE: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
#[cfg(windows)]
static RESTART_LOADER_VFTABLE: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
#[cfg(windows)]
static RESTART_STOP_FN: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
#[cfg(windows)]
static RESTART_ROUTER_FN: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
/// The PATCHED `wavebank_unregister` game entry (the base signature's
/// match): calling it — unlike `GenericDetour::call`, which is the
/// trampoline and BYPASSES the detour — flows through the installed hook,
/// whose prelude retires the live preview binding (design Flow 2 step 3).
#[cfg(windows)]
static RESTART_UNREGISTER_FN: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());

/// Stash the restart half's five addresses. All-or-nothing (design R9):
/// any missing derivation returns false and leaves every pointer null,
/// so [`restart_available`] is a single coherent gate — there is no
/// partially-armed restart. Called from the song-playback-speed mod's
/// `init` (the `real_speed::init` pattern); availability is reported at
/// `enable()`.
#[cfg(windows)]
pub fn init_restart(signatures: &crate::core::signatures::SignatureStore) -> bool {
    let (Some(view_vft), Some(loader_vft), Some(stop_fn), Some(router_fn), Some(unregister_fn)) = (
        signatures.get_address("selectmusic_view_vftable"),
        signatures.get_address("audio_loader_vftable"),
        signatures.get_address("cue_handle_stop"),
        signatures.get_address("sound_bank_create_router"),
        signatures.get_address("song_rate_wavebank_unregister"),
    ) else {
        return false;
    };
    RESTART_VIEW_VFTABLE.store(view_vft as *mut u8, Ordering::Release);
    RESTART_LOADER_VFTABLE.store(loader_vft as *mut u8, Ordering::Release);
    RESTART_STOP_FN.store(stop_fn as *mut u8, Ordering::Release);
    RESTART_ROUTER_FN.store(router_fn as *mut u8, Ordering::Release);
    RESTART_UNREGISTER_FN.store(unregister_fn as *mut u8, Ordering::Release);
    true
}

/// Whether the restart half's derivations all resolved ([`init_restart`]
/// stores all five or none, so one null check would do — checking all
/// five keeps the invariant observable).
#[cfg(windows)]
#[must_use]
pub fn restart_available() -> bool {
    !RESTART_VIEW_VFTABLE.load(Ordering::Acquire).is_null()
        && !RESTART_LOADER_VFTABLE.load(Ordering::Acquire).is_null()
        && !RESTART_STOP_FN.load(Ordering::Acquire).is_null()
        && !RESTART_ROUTER_FN.load(Ordering::Acquire).is_null()
        && !RESTART_UNREGISTER_FN.load(Ordering::Acquire).is_null()
}

/// Offset of the active scene child on the TransitionSequence (the same
/// accessor quick_logout / quick_restart use).
#[cfg(windows)]
const TS_ACTIVE_CHILD_OFFSET: usize = 0x58;
/// Actor tree-flags dword offset + dying/destroyed mask (quick_logout's
/// documented pair).
#[cfg(windows)]
const TREE_FLAGS_OFFSET: usize = 0x20;
#[cfg(windows)]
const TREE_FLAGS_DEAD_MASK: u32 = 0x24;
/// `sequence::selectmusic::View*` on the SelectMusicSequence (RE §1.1).
#[cfg(windows)]
const CHILD_VIEW_OFFSET: usize = 0xB8;
/// The embedded `sequence::AudioPlayer` within the View (RE §1.1 — the
/// signature's load-bearing layout pin) and its loader unique_ptr.
#[cfg(windows)]
const VIEW_AUDIO_PLAYER_OFFSET: usize = 0xC8;
#[cfg(windows)]
const AUDIO_PLAYER_LOADER_OFFSET: usize = 0x08;
/// `sequence::AudioLoader` field offsets (RE §1.3; pinned literal by the
/// `audio_loader_ctor` signature so a layout change fails the MATCH, and
/// gated at runtime by the vftable identity below).
#[cfg(windows)]
const LOADER_XWB_ID_OFFSET: usize = 0x08;
#[cfg(windows)]
const LOADER_XSB_ID_OFFSET: usize = 0x0C;
#[cfg(windows)]
const LOADER_HANDLE_OFFSET: usize = 0x10;
#[cfg(windows)]
const LOADER_FAILED_OFFSET: usize = 0x14;
#[cfg(windows)]
const LOADER_MODE_OFFSET: usize = 0x15;
#[cfg(windows)]
const LOADER_SLOT_OFFSET: usize = 0x18;
/// The loader's cue `std::string` (RE §1.3): preview cues end `_s`.
#[cfg(windows)]
const LOADER_CUE_OFFSET: usize = 0x48;

/// A structurally validated live preview loader: every pointer on the
/// walk identity- or liveness-gated. Game-owned memory — only valid on
/// the game thread, within the frame it was resolved.
#[cfg(windows)]
pub struct LoaderChain {
    /// The live `sequence::AudioLoader`.
    pub loader: *mut u8,
    pub snapshot: LoaderSnapshot,
}

/// Why the loader-chain walk declined (deploy-#2 log-hygiene refinement:
/// the scene-entry profile load seeds the persisted rate through
/// `on_change` → `request_refresh`, so the executor's first fire can
/// legitimately land before ANY preview loader exists — that must not
/// consume the once-per-class chain WARN a real layout drift would need).
#[cfg(windows)]
enum ChainDecline {
    /// No preview machinery to restart: derivations unstashed, wrong
    /// scene, TS/child missing or dying, no View yet, or no loader
    /// installed (nothing is playing — e.g., the first wheel-settle
    /// request still inside its 0.4 s deferral). Expected states —
    /// silent.
    Absent,
    /// A vftable identity gate failed: the walk reached a non-null
    /// object whose first qword is not the derived vftable — layout
    /// drift on this build. Actionable — worth the latched WARN.
    IdentityMismatch,
}

/// Walk `scene==SONG_SELECT → TS → *(TS+0x58) child (live) → View =
/// *(child+0xB8) (vftable identity) → loader = *(View+0xC8+0x08)
/// (vftable identity)` and snapshot the loader's fields (design
/// §Components 5 step 1). The vftable identity gates are what make the
/// compile-time offsets fail-closed across builds: a future layout drift
/// walks to a pointer whose first qword is not the derived vftable and
/// declines cleanly. GAME THREAD ONLY (the objects die on scene/wheel
/// transitions that only the game thread serializes against).
///
/// Fail-open, no logging (callers latch outcomes). Field SANITY is
/// deliberately the caller's ([`loader_sane`]): the resolver answers "is
/// this walk structurally the preview chain", not "is this loader
/// restartable".
#[cfg(windows)]
fn resolve_loader_detail() -> Result<LoaderChain, ChainDecline> {
    use crate::core::memory;
    use crate::services::scene_manager;

    let view_vft = RESTART_VIEW_VFTABLE.load(Ordering::Acquire) as *const u8;
    let loader_vft = RESTART_LOADER_VFTABLE.load(Ordering::Acquire) as *const u8;
    if view_vft.is_null() || loader_vft.is_null() {
        return Err(ChainDecline::Absent);
    }
    if scene_manager::current_scene() != scene::SONG_SELECT {
        return Err(ChainDecline::Absent);
    }
    let Some(ts) = scene_manager::current_transition_sequence() else {
        return Err(ChainDecline::Absent);
    };
    // SAFETY: every dereference below is null-checked and identity-gated
    // before use; the objects are game-owned and this function's contract
    // restricts callers to the game thread (no concurrent teardown).
    unsafe {
        let child = memory::read_ptr(ts.add(TS_ACTIVE_CHILD_OFFSET)) as *mut u8;
        if child.is_null() {
            return Err(ChainDecline::Absent);
        }
        if memory::read_u32(child.add(TREE_FLAGS_OFFSET)) & TREE_FLAGS_DEAD_MASK != 0 {
            return Err(ChainDecline::Absent);
        }
        let view = memory::read_ptr(child.add(CHILD_VIEW_OFFSET)) as *mut u8;
        if view.is_null() {
            return Err(ChainDecline::Absent);
        }
        if memory::read_ptr(view) != view_vft {
            return Err(ChainDecline::IdentityMismatch);
        }
        let loader =
            memory::read_ptr(view.add(VIEW_AUDIO_PLAYER_OFFSET + AUDIO_PLAYER_LOADER_OFFSET))
                as *mut u8;
        if loader.is_null() {
            return Err(ChainDecline::Absent);
        }
        if memory::read_ptr(loader) != loader_vft {
            return Err(ChainDecline::IdentityMismatch);
        }
        Ok(LoaderChain {
            loader,
            snapshot: LoaderSnapshot {
                handle: memory::read_i32(loader.add(LOADER_HANDLE_OFFSET)),
                failed: memory::read_u8(loader.add(LOADER_FAILED_OFFSET)) != 0,
                mode: memory::read_u8(loader.add(LOADER_MODE_OFFSET)),
                slot: memory::read_i32(loader.add(LOADER_SLOT_OFFSET)),
                xwb_id: memory::read_i32(loader.add(LOADER_XWB_ID_OFFSET)),
                xsb_id: memory::read_i32(loader.add(LOADER_XSB_ID_OFFSET)),
            },
        })
    }
}

/// [`resolve_loader_detail`] without the decline reason (the watchdog and
/// the chain probe treat every decline the same).
#[cfg(windows)]
#[must_use]
pub fn resolve_loader() -> Option<LoaderChain> {
    resolve_loader_detail().ok()
}

/// Loader-chain probe outcomes (plan Step-4 demo). 0 = never probed;
/// the drain reports each distinct outcome once (latched on its side).
#[cfg(windows)]
pub const CHAIN_PROBE_OK: usize = 1;
#[cfg(windows)]
pub const CHAIN_PROBE_UNRESOLVED: usize = 2;
#[cfg(windows)]
pub const CHAIN_PROBE_INSANE: usize = 3;

#[cfg(windows)]
static CHAIN_PROBE: AtomicUsize = AtomicUsize::new(0);

/// Probe the loader chain right after a preview publish (game thread —
/// the loader is live mid-load at that instant) and stash the outcome
/// for the drain. No-op while the restart half is unavailable (the mod's
/// enable-time WARN already covers that state).
#[cfg(windows)]
fn probe_loader_chain() {
    if !restart_available() {
        return;
    }
    let outcome = match resolve_loader() {
        Some(chain) if loader_sane(&chain.snapshot) => CHAIN_PROBE_OK,
        Some(_) => CHAIN_PROBE_INSANE,
        None => CHAIN_PROBE_UNRESOLVED,
    };
    CHAIN_PROBE.store(outcome, Ordering::Release);
}

/// Take the latest probe outcome (0 = none since the last take). Drain
/// consumer only.
#[cfg(windows)]
pub(super) fn take_chain_probe() -> usize {
    CHAIN_PROBE.swap(0, Ordering::AcqRel)
}

// ── Live-edit restart: debounce, sequence, executor, watchdog ──────────
// (design §Architecture Flow 2, §Components 5 steps 0–5 + the watchdog
// amendment, §Components 7, §Data Models)

/// The debounce window: the restart fires once per quiet-150 ms window
/// after the LAST edit tick (design R4/D4).
pub const REFRESH_DEBOUNCE_NANOS: u64 = 150_000_000;

/// The module's monotonic timebase (the repo idiom: an `Instant` epoch +
/// elapsed nanos through atomics — std's `Instant` is QPC-backed on
/// windows). Shared by the stamp side (option callbacks) and the poll
/// side (frame executor).
static REFRESH_EPOCH: OnceLock<Instant> = OnceLock::new();

fn refresh_now_nanos() -> u64 {
    u64::try_from(REFRESH_EPOCH.get_or_init(Instant::now).elapsed().as_nanos()).unwrap_or(u64::MAX)
}

/// The refresh debounce cell (design §Data Models): single writer set
/// (option callbacks, whatever thread the options framework uses), single
/// consumer (the frame executor, game thread). Every tick overwrites the
/// stamp, so the executor fires once per quiet window.
///
/// Benign race (accepted; fail-open): an edit tick landing between the
/// consumer's decision and its `requested` clear loses that one stamp —
/// at most one missed restart, corrected by the next edit or wheel
/// settle. The alternative (CAS token protocols) buys nothing a player
/// can perceive.
pub struct RefreshCell {
    requested: AtomicBool,
    stamp_nanos: AtomicU64,
    /// Selected-song publication generation latched at stamp time
    /// (design Flow 2 step 0 — the supersession check's baseline).
    settle_generation: AtomicU32,
}

/// One debounce-poll decision (design Flow 2 + §Components 5 step 0–1's
/// scene half; the loader-chain half is the executor's).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefreshPoll {
    /// No request pending.
    Idle,
    /// Request pending, quiet window not yet elapsed.
    Pending,
    /// Request cleared: the scene left SONG_SELECT (C9 — a stale edit
    /// must never restart after returning to select).
    SceneCleared,
    /// Request cleared: a wheel settle re-published the selected song
    /// after the stamp — the fresh create already qualified with the
    /// newest desired values (restarting would fight the game's own
    /// 0.4 s deferred request).
    Superseded,
    /// Fire the restart now (request cleared).
    Fire,
}

impl RefreshCell {
    pub const fn new() -> Self {
        Self {
            requested: AtomicBool::new(false),
            stamp_nanos: AtomicU64::new(0),
            settle_generation: AtomicU32::new(0),
        }
    }

    /// Stamp an edit tick. Stamp/generation first, `requested` last, so a
    /// concurrent poll observing `requested` sees a coherent stamp.
    pub fn stamp_at(&self, now_nanos: u64, settle_generation: u32) {
        self.stamp_nanos.store(now_nanos, Ordering::Release);
        self.settle_generation
            .store(settle_generation, Ordering::Release);
        self.requested.store(true, Ordering::Release);
    }

    pub fn clear(&self) {
        self.requested.store(false, Ordering::Release);
    }

    /// Cheap idle-path gate (one Acquire): whether a stamp is pending.
    /// The executor consults this before paying for the selected-song
    /// seqlock read [`poll_at`]'s supersession check needs.
    pub fn has_request(&self) -> bool {
        self.requested.load(Ordering::Acquire)
    }

    /// The consumer's per-frame decision. Injectable time/scene/generation
    /// for host tests; the windows executor feeds the live values.
    pub fn poll_at(
        &self,
        now_nanos: u64,
        current_scene: i32,
        current_settle_generation: u32,
    ) -> RefreshPoll {
        if !self.requested.load(Ordering::Acquire) {
            return RefreshPoll::Idle;
        }
        if current_scene != scene::SONG_SELECT {
            self.requested.store(false, Ordering::Release);
            return RefreshPoll::SceneCleared;
        }
        let stamp = self.stamp_nanos.load(Ordering::Acquire);
        if now_nanos.saturating_sub(stamp) < REFRESH_DEBOUNCE_NANOS {
            return RefreshPoll::Pending;
        }
        if self.settle_generation.load(Ordering::Acquire) != current_settle_generation {
            self.requested.store(false, Ordering::Release);
            return RefreshPoll::Superseded;
        }
        self.requested.store(false, Ordering::Release);
        RefreshPoll::Fire
    }
}

/// The one live cell ([`request_refresh`] stamps it, the frame executor
/// polls it).
static REFRESH_CELL: RefreshCell = RefreshCell::new();

/// FileManager row load-state values the AudioLoader tick accepts as
/// "loaded" (RE §1.3 — the tick's own gate set; the restart's rows must
/// pass it or the re-created banks would race the loads).
#[must_use]
pub fn row_state_loaded(state: u32) -> bool {
    matches!(state, 0 | 5 | 6 | 8)
}

/// Preview cues are `<code>_s` (RE §1.2). Byte-wise — the cue is read
/// raw from game memory.
#[must_use]
pub fn cue_is_preview(cue: &[u8]) -> bool {
    cue.ends_with(b"_s")
}

/// The engine's fixed first ADPCM stream read (RE §8 — `FUN_004265d0`
/// rounds the initial read to `min(stream buffer capacity, remaining)`,
/// the full 64 KiB packet for these banks).
pub const INITIAL_PACKET_BYTES: u64 = 0x10000;

/// The produced-watermark threshold at which a preview's first engine
/// read can complete: the target entry's data start plus the initial
/// packet, clamped to the entry's end (short entries prepare on less).
#[must_use]
pub fn watchdog_cover(target_data_start: u64, target_data_end: u64) -> u64 {
    (target_data_start + INITIAL_PACKET_BYTES).min(target_data_end)
}

/// The restart sequence's side-effect seam (host-testable; the windows
/// executor implements it over the stashed stock functions).
pub trait RestartIo {
    /// `cue_handle_stop(handle)` — the game's own teardown stop.
    fn stop_cue(&mut self, handle: i32);
    /// The PATCHED `wavebank_unregister` entry (the detour prelude
    /// retires the live preview binding).
    fn unregister(&mut self, file_id: i32);
    /// `sound_bank_create_router(file_id)` — `.xsb` ⇒ soundbank create,
    /// else the patched `wavebank_create` (re-qualifies the preview
    /// bind). Returns the create's status.
    fn create(&mut self, file_id: i32) -> bool;
    /// `loader.handle = −1; loader.failed = 0` — the game's tick replays
    /// the cue next frame (design Flow 2 step 5).
    fn rearm_loader(&mut self);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestartOutcome {
    Restarted,
    /// A create returned failure: the sequence aborted WITHOUT re-arming
    /// (the loader keeps its stopped state — silent preview until the
    /// next wheel settle re-runs the stock pipeline; design step 4).
    CreateFailed {
        file_id: i32,
    },
}

/// The restart's exact step order (design Flow 2 steps 2–5; stock order
/// per the 2026-08-05 timeline: unregister XSB then XWB, create XWB then
/// XSB). Pure orchestration over the seam — host tests pin the ordering
/// and the abort semantics.
pub fn run_restart_sequence(snapshot: &LoaderSnapshot, io: &mut dyn RestartIo) -> RestartOutcome {
    if snapshot.handle != -1 {
        io.stop_cue(snapshot.handle);
    }
    io.unregister(snapshot.xsb_id);
    io.unregister(snapshot.xwb_id);
    if !io.create(snapshot.xwb_id) {
        return RestartOutcome::CreateFailed {
            file_id: snapshot.xwb_id,
        };
    }
    if !io.create(snapshot.xsb_id) {
        return RestartOutcome::CreateFailed {
            file_id: snapshot.xsb_id,
        };
    }
    io.rearm_loader();
    RestartOutcome::Restarted
}

/// Once-per-class WARN latch bits for the executor's precondition
/// failures (design §Error Handling — latched WARN per class; the
/// preview keeps playing as-is).
#[cfg(windows)]
static RESTART_WARNED: AtomicU32 = AtomicU32::new(0);
#[cfg(windows)]
const WARN_CHAIN: u32 = 1 << 0;
#[cfg(windows)]
const WARN_ROWS: u32 = 1 << 1;
#[cfg(windows)]
const WARN_CUE: u32 = 1 << 2;
#[cfg(windows)]
const WARN_CREATE: u32 = 1 << 3;

#[cfg(windows)]
fn warn_once(bit: u32, message: &str) {
    if RESTART_WARNED.fetch_or(bit, Ordering::AcqRel) & bit == 0 {
        crate::log_warn!("song_rate: preview restart declined — {message} (fail-open; the preview keeps playing as-is)");
    }
}

/// Read an MSVC `std::string` at `addr` (buf/ptr at +0, len at +0x10,
/// cap at +0x18; heap pointer when cap ≥ 0x10). `None` on insane fields
/// (the song_reset pattern).
#[cfg(windows)]
unsafe fn read_msvc_string(addr: *const u8) -> Option<Vec<u8>> {
    use crate::core::memory;
    let len = memory::read_u64(addr.add(0x10)) as usize;
    let cap = memory::read_u64(addr.add(0x18)) as usize;
    if len > cap || cap > 0x1000 {
        return None;
    }
    let buf = if cap >= 0x10 {
        let p = memory::read_ptr(addr);
        if p.is_null() {
            return None;
        }
        p
    } else {
        addr
    };
    Some(std::slice::from_raw_parts(buf, len).to_vec())
}

/// The windows [`RestartIo`]: the three stashed stock functions + the
/// loader re-arm writes. Only constructed behind [`restart_available`]
/// (the pointers are non-null by the all-or-nothing stash).
#[cfg(windows)]
struct GameRestartIo {
    loader: *mut u8,
}

#[cfg(windows)]
impl RestartIo for GameRestartIo {
    fn stop_cue(&mut self, handle: i32) {
        let f = RESTART_STOP_FN.load(Ordering::Acquire);
        if f.is_null() {
            return;
        }
        // SAFETY: the address is the validated cue_handle_stop entry; a
        // dead/stale handle is a safe no-op inside it (RE §1.3).
        let f: unsafe extern "C" fn(i32) = unsafe { std::mem::transmute(f) };
        unsafe { f(handle) };
    }
    fn unregister(&mut self, file_id: i32) {
        let f = RESTART_UNREGISTER_FN.load(Ordering::Acquire);
        if f.is_null() {
            return;
        }
        // SAFETY: the PATCHED game entry — the call flows through the
        // installed detour (prelude retires the preview binding), then
        // the original unregisters the bank.
        let f: unsafe extern "C" fn(i32) = unsafe { std::mem::transmute(f) };
        unsafe { f(file_id) };
    }
    fn create(&mut self, file_id: i32) -> bool {
        let f = RESTART_ROUTER_FN.load(Ordering::Acquire);
        if f.is_null() {
            return false;
        }
        // SAFETY: the validated create-router entry; its XWB arm lands on
        // the patched wavebank_create (the preview branch re-qualifies).
        let f: unsafe extern "C" fn(i32) -> u8 = unsafe { std::mem::transmute(f) };
        unsafe { f(file_id) != 0 }
    }
    fn rearm_loader(&mut self) {
        use crate::core::memory;
        // SAFETY: the loader pointer was identity-gated by resolve_loader
        // this same frame (game thread — no concurrent teardown).
        unsafe {
            memory::write_i32(self.loader.add(LOADER_HANDLE_OFFSET), -1);
            memory::write_u8(self.loader.add(LOADER_FAILED_OFFSET), 0);
        }
    }
}

/// The per-frame executor (registered on the input manager's frame
/// callback by [`init`]; the render/game thread). Idle cost: one
/// feature-gate load + one `has_request` load + the watchdog's preview
/// Acquire — no allocation, no locks. The game-facing halves are
/// individually panic-contained (design §Components 5; the frame
/// dispatch adds an outer net).
#[cfg(windows)]
fn executor_frame() {
    if !feature_active() {
        return;
    }
    if REFRESH_CELL.has_request() {
        let settle = super::selected_song::selected_song()
            .map(|info| info.generation)
            .unwrap_or(0);
        let poll = REFRESH_CELL.poll_at(
            refresh_now_nanos(),
            crate::services::scene_manager::current_scene(),
            settle,
        );
        if poll == RefreshPoll::Fire {
            let _ = std::panic::catch_unwind(execute_restart);
        }
    }
    let _ = std::panic::catch_unwind(watchdog_step);
}

/// Fire-time restart (design §Components 5 steps 1–5): re-validate the
/// loader chain + field sanity + row states + cue shape, then run the
/// stock sequence. Every decline is fail-open with one latched WARN per
/// class; a success is one INFO (user-triggered, bounded by human input).
#[cfg(windows)]
fn execute_restart() {
    if !restart_available() {
        return;
    }
    let chain = match resolve_loader_detail() {
        Ok(chain) => chain,
        // No preview machinery to restart (nothing playing — e.g., the
        // profile load's seeding stamped a refresh at scene entry before
        // the first wheel-settle request ever built a loader). Expected;
        // silent (the next settle qualifies with the new values anyway).
        Err(ChainDecline::Absent) => return,
        Err(ChainDecline::IdentityMismatch) => {
            warn_once(WARN_CHAIN, "a loader-chain vftable identity gate failed");
            return;
        }
    };
    if !loader_sane(&chain.snapshot) {
        warn_once(WARN_CHAIN, "loader chain resolved to a non-preview shape");
        return;
    }
    let rows_loaded = [chain.snapshot.xwb_id, chain.snapshot.xsb_id]
        .iter()
        .all(|&id| {
            super::wavebank_hook::file_table_state(id)
                .map(row_state_loaded)
                .unwrap_or(false)
        });
    if !rows_loaded {
        warn_once(WARN_ROWS, "file rows not in a loaded state");
        return;
    }
    // SAFETY: the loader pointer was identity-gated by resolve_loader
    // this same frame.
    let cue_ok = unsafe { read_msvc_string(chain.loader.add(LOADER_CUE_OFFSET)) }
        .map(|cue| cue_is_preview(&cue))
        .unwrap_or(false);
    if !cue_ok {
        warn_once(WARN_CUE, "loader cue is not a preview cue");
        return;
    }
    let mut io = GameRestartIo {
        loader: chain.loader,
    };
    match run_restart_sequence(&chain.snapshot, &mut io) {
        RestartOutcome::Restarted => {
            crate::log_info!(
                "song_rate: preview restarted at the desired settings (xwb {}, xsb {}) — the game's tick replays the cue",
                chain.snapshot.xwb_id,
                chain.snapshot.xsb_id,
            );
        }
        RestartOutcome::CreateFailed { file_id } => {
            warn_once(
                WARN_CREATE,
                // The specific id is in the latched message's context via
                // the drain's bank timeline; the class is what matters.
                "a bank create returned failure during the restart",
            );
            let _ = file_id;
        }
    }
}

/// The last preview generation the watchdog re-armed (ONE retry per
/// preview generation — a latch against retry storms; design §Components
/// 5 amendment).
#[cfg(windows)]
static WATCHDOG_RETRIED_GENERATION: AtomicU64 = AtomicU64::new(0);

/// The preview play watchdog (deploy-#1 fix, design amendment
/// 2026-08-16): the game's loader fires `se_play` as soon as the file
/// rows are resident, never waiting for XACT stream prepare; a WSOLA
/// preview's first packet takes ~0.6 s to synthesize, and a Play landing
/// in that window fails and latches the loader's `failed` flag forever.
/// When the live preview binding has produced its initial packet range
/// and the loader sits failed-latched, clear `failed` + re-arm
/// `handle = −1` — the game's own tick retries the play ("slightly late
/// but reliable").
#[cfg(windows)]
fn watchdog_step() {
    // Preview-slot Acquire first — the cheapest gate for the common
    // no-preview frame.
    let Some((generation, file_id, ready)) = super::binding::registry().with_preview(|binding| {
        (
            binding.generation(),
            binding.file_id(),
            binding.state() == super::binding::BindingState::Active
                && binding.ring_produced()
                    >= watchdog_cover(binding.target_data_start(), binding.target_data_end()),
        )
    }) else {
        return;
    };
    if !ready
        || !restart_available()
        || WATCHDOG_RETRIED_GENERATION.load(Ordering::Acquire) == generation
    {
        return;
    }
    let Some(chain) = resolve_loader() else {
        return;
    };
    // Only the exact bank this binding serves, only when the loader
    // actually latched a failure. `handle == −1 && !failed` needs no help
    // — the tick is still armed and will fire on its own.
    if !loader_sane(&chain.snapshot) || chain.snapshot.xwb_id != file_id || !chain.snapshot.failed {
        return;
    }
    WATCHDOG_RETRIED_GENERATION.store(generation, Ordering::Release);
    use crate::core::memory;
    // SAFETY: identity-gated loader, game thread, same frame.
    unsafe {
        memory::write_u8(chain.loader.add(LOADER_FAILED_OFFSET), 0);
        memory::write_i32(chain.loader.add(LOADER_HANDLE_OFFSET), -1);
    }
    crate::log_info!(
        "song_rate: preview play watchdog re-armed the loader (preview generation {}, xwb {}) — the game's tick retries se_play",
        generation,
        file_id,
    );
}
