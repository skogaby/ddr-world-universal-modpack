//! Custom Player Options Persistence — ess.dll save/load bridge.
//!
//! Installs two retour detours on ess.dll's playerdata sender/receiver:
//!   - `sys_playerdata_save_sender`: after the native 29-field option block,
//!     appends `<mod_{id}>` s32 children for each registered custom option.
//!   - `sys_playerdata_load_receiver`: after the native parse, reads back any
//!     `<mod_{id}>` children and pushes them into `custom_options::resolve_from_load`.
//!
//! Resolution strategy (per design Decision 9):
//!   1. Primary: dispatcher-table walk — scan ess.dll's `.data` segment for the
//!      `"playerdata_save"` / `"playerdata_load"` C strings, read the function
//!      pointer at `+0x10` (sender) / `+0x18` (receiver) from the registration slot.
//!   2. Fallback: AOB scan on a distinctive prolog sequence.
//!
//! libavs-win64 ordinals 162/163/175/176 are resolved via `GetProcAddress`
//! with numeric ordinals (MAKEINTRESOURCE pattern); ordinal 164
//! (`property_node_remove`, the logout-save sanitiser's league strip) is
//! resolved non-fatally alongside them.
//!
//! This service also hosts the enforcement half of the score-submission
//! policy (state lives in `score_guard`): the `save_sender` trampoline
//! suppresses tainted per-stage saves, and applies the three-way logout-save
//! policy (forward / sanitise-and-forward / suppress) backed by the EAM_EXIT
//! record sanitiser registered here.
//!
//! Gated by `"custom_options": { "persist_network": true }` in mod-config.json
//! (default true). When false, no network children are emitted; if
//! `persist_json` is also false, no detours are installed and options reset
//! to defaults on each card swipe.

use std::ffi::CString;
use std::ptr::addr_of;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use once_cell::sync::Lazy;
use std::time::Duration;

use retour::GenericDetour;
use windows::core::PCSTR;
use windows::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
use windows::Win32::System::ProcessStatus::{GetModuleInformation, MODULEINFO};
use windows::Win32::System::Threading::GetCurrentProcess;

use crate::core::memory;
use crate::core::scanner;
use crate::core::signatures::SignatureStore;
use crate::mods::config;
use crate::services::custom_options;
use crate::services::scene_manager;
use crate::services::score_guard;
use crate::services::stage_records;
use crate::types::scenes::scene;
use crate::{log_debug, log_error, log_info, log_warn};

// ── FFI types ───────────────────────────────────────────────────────

/// Both sender and receiver share the same signature:
/// `u64 fn(job: *mut u8, kbin_ctx: *mut u8)`
type EssSenderFn = unsafe extern "C" fn(*mut u8, *mut u8) -> u64;
type EssReceiverFn = unsafe extern "C" fn(*mut u8, *mut u8) -> u64;

/// `Ordinal_162`: avs_xml_find_child — `(unused, parent, name) -> child_node`
type FnXmlFindChild = unsafe extern "C" fn(i32, *mut u8, *const i8) -> *mut u8;

/// `Ordinal_163`: avs_xml_add_child_with_value — `(ctx, parent, kbin_type, name, value) -> new_node`
/// The 5th argument is the raw s32 value (zero-extended to 64 bits on the
/// stack), NOT a pointer to it.
type FnXmlAddChild = unsafe extern "C" fn(*mut u8, *mut u8, i32, *const i8, i32) -> *mut u8;

/// `Ordinal_163`, str-typed view: for kbin type 11 (`str`) the variadic
/// value slot carries a POINTER to the NUL-terminated string. Verified in
/// Ghidra against ess.dll's own `ghost` emission (sys_ghostdata_save_sender
/// calls the identical function with type 0xb and `LEA` of the string
/// buffer into the value slot).
type FnXmlAddChildStr =
    unsafe extern "C" fn(*mut u8, *mut u8, i32, *const i8, *const i8) -> *mut u8;

/// `Ordinal_175`: avs_xml_get_context — `(root) -> ctx`
type FnXmlGetCtx = unsafe extern "C" fn(*mut u8) -> *mut u8;

/// `Ordinal_176`: avs_xml_read_value_by_name — `(ctx, parent, name, kbin_type, dest, dest_size) -> status`
type FnXmlReadValue = unsafe extern "C" fn(*mut u8, *mut u8, *const i8, i32, *mut i32, i32) -> i32;

/// `Ordinal_164`: property_node_remove — `(node) -> status`. Unlinks the node
/// from its parent chain and releases it (self-identifying null-guard log:
/// `"%s: %s==NULL", "node_remove"` on the `property` channel). Used by the
/// logout-save sanitiser to strip the `<league>` accumulator from a tainted
/// side's logout request. Resolved non-fatally — a miss only disables the
/// sanitise path (tainted logout saves fall back to full suppression).
type FnXmlRemoveNode = unsafe extern "C" fn(*mut u8) -> i32;

// ── Static state ────────────────────────────────────────────────────

/// Delay before the one-shot JSON-load timer fires, in seconds. Chosen to land
/// well after all mods have registered their options (mod `enable()` runs at
/// lib.rs step 8, after this service's `init()` at step 4i) but well before a
/// player could card in and view the options menu. Tunable if cabinet boot
/// timing differs.
const JSON_LOAD_DELAY_SECS: u64 = 12;

/// Offset of `savekind` within the save-side savedata struct
/// (`*(job+0x10) + 0x74`). The ess `save_sender` emits it as the `savekind`
/// child; gamemdx's `ReflectSavePlayerData(side, kind, stage)` writes the kind
/// here. Used to distinguish per-stage (score) saves from the card-out logout
/// save for score-submission suppression.
const SAVEDATA_SAVEKIND_OFFSET: usize = 0x74;

/// `savekind` enum values passed by `ReflectSavePlayerData`. `FIRST` is the
/// initial card-in checkpoint (option/result fields are sentinels, never a real
/// score); `STAGE` fires after each song (carries that song's `/result`);
/// `LOGOUT` fires at card-out (re-bundles all stages' results). Values are
/// RE-derived; see `research/score-submission-re.md`.
const SAVEKIND_FIRST: i32 = 1;
const SAVEKIND_STAGE: i32 = 2;
const SAVEKIND_LOGOUT: i32 = 3;

/// On the load path, the savedata buffer is reached via `*(job + 0x18)` (the
/// ess `sys_playerdata_load_receiver`'s `param_1 + 0x18`). The incoming player's
/// numeric DDR ID (`ddrcode`) is parsed into `savedata + 0x48`. Unlike the save
/// path, the load job carries no player-side index — the side is recovered by
/// matching this ddrcode to a per-side PlayerWork (see `side_from_ddrcode`).
const LOAD_JOB_SAVEDATA_PTR_OFFSET: usize = 0x18;
const LOAD_SAVEDATA_DDRCODE_OFFSET: usize = 0x48;

/// `ddrcode` field within a `PlayerWork` instance. Live-confirmed: the two
/// per-side PlayerWorks hold the carded-in players' DDR IDs here, so a load's
/// ddrcode matched against this field identifies the owning side.
const PLAYER_WORK_DDRCODE_OFFSET: usize = 0x18;

static INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Base of the 2-slot per-side player-work table (`table[side]` → wrapper,
/// `*wrapper` → PlayerWork). Resolved in `init()` from the signature store;
/// null if unavailable, in which case load-side side resolution falls back to
/// the legacy (P1-only-correct) path.
static mut PLAYER_WORK_TABLE: *const u8 = std::ptr::null();

/// One profile's worth of network-loaded mod option values, captured at
/// `load_receiver` time and keyed by the load's `ddrcode`. The side it belongs
/// to cannot be resolved yet (the game populates `PlayerWork+0x18` only *after*
/// the load completes), so application is deferred until SONG_SELECT entry,
/// when the ddrcode→side join is valid. See `PENDING_LOADS` / `apply_pending_loads`.
struct PendingLoad {
    ddrcode: i32,
    values: Vec<(String, i32)>,
}

/// Network loads awaiting side resolution. Drained on SONG_SELECT entry.
static PENDING_LOADS: Lazy<Mutex<Vec<PendingLoad>>> = Lazy::new(|| Mutex::new(Vec::new()));

/// Successful profile loads (by ddrcode) whose side-matched song-rate ledger
/// reset is deferred to SONG_SELECT entry (the ddrcode→side join is invalid
/// at load-receiver time). Independent of the persistence gates.
static PENDING_RATE_RESETS: Lazy<Mutex<Vec<i32>>> = Lazy::new(|| Mutex::new(Vec::new()));

// ── String-valued wire fields (Per-Song Judgement Offsets extension) ──────

/// Producer for a registered string field: `None` omits the field this save
/// (the un-armed / mod-disabled case); `Some("")` is a real value (the
/// server-clear signal).
pub type StringSaveFn = fn(side: u8) -> Option<String>;
/// Consumer for a loaded string field, invoked at SONG_SELECT entry with the
/// resolved side — strictly AFTER the card-in callbacks.
pub type StringLoadFn = fn(side: u8, value: &str);

struct StringField {
    wire_name: &'static str,
    save: StringSaveFn,
    load: StringLoadFn,
}

static STRING_FIELDS: Lazy<Mutex<Vec<StringField>>> = Lazy::new(|| Mutex::new(Vec::new()));

/// Per-side card-in notifications, fired where `PersistMode::Session`
/// card-in resets happen (side-resolved, fail-closed on unresolved ddrcode).
static CARD_IN_CALLBACKS: Lazy<Mutex<Vec<fn(u8)>>> = Lazy::new(|| Mutex::new(Vec::new()));

/// Deferred string-field loads, ddrcode-keyed like [`PendingLoad`].
struct PendingStringLoad {
    ddrcode: i32,
    values: Vec<(&'static str, String)>,
}

static PENDING_STRING_LOADS: Lazy<Mutex<Vec<PendingStringLoad>>> =
    Lazy::new(|| Mutex::new(Vec::new()));

/// One-shot warn latch for str-emit failures.
static WARNED_STR_EMIT: AtomicBool = AtomicBool::new(false);

/// Register a string-valued wire field carried as a kbin `str` child of
/// `/data/option` in every player-data save, and read back on profile load
/// (applied side-resolved at SONG_SELECT entry). Callable any time before
/// or after the detours install; consulted only when they did.
pub fn register_string_field(wire_name: &'static str, save: StringSaveFn, load: StringLoadFn) {
    STRING_FIELDS.lock().unwrap().push(StringField {
        wire_name,
        save,
        load,
    });
}

/// Register a per-side card-in callback (fires before any string-field load
/// application for that side).
pub fn register_card_in_callback(cb: fn(u8)) {
    CARD_IN_CALLBACKS.lock().unwrap().push(cb);
}

/// Guards one-time registration of the SONG_SELECT drain callback.
static SCENE_CALLBACK_REGISTERED: AtomicBool = AtomicBool::new(false);

/// Guards one-time registration of the EAM_EXIT logout-save sanitiser.
static SANITISER_CALLBACK_REGISTERED: AtomicBool = AtomicBool::new(false);

// The league-strip availability flag (Ordinal 164 resolved) lives in
// `score_guard` — the single source of truth shared by the trampoline's
// three-way logout policy and the full-sanitization readiness conjunction.

/// Gate flags read by the `extern "C"` trampolines. Set once in `init()` from
/// the config; the trampolines branch on these to decide whether to emit/read
/// network children (`PERSIST_NETWORK`) and whether to write/read the offline
/// JSON cache (`PERSIST_JSON`).
static PERSIST_NETWORK: AtomicBool = AtomicBool::new(false);
static PERSIST_JSON: AtomicBool = AtomicBool::new(false);

static mut HOOK_SAVE_SENDER: Option<GenericDetour<EssSenderFn>> = None;
static mut HOOK_LOAD_RECEIVER: Option<GenericDetour<EssReceiverFn>> = None;

static mut FN_XML_FIND_CHILD: Option<FnXmlFindChild> = None;
static mut FN_XML_ADD_CHILD: Option<FnXmlAddChild> = None;
static mut FN_XML_ADD_CHILD_STR: Option<FnXmlAddChildStr> = None;
static mut FN_XML_GET_CTX: Option<FnXmlGetCtx> = None;
static mut FN_XML_READ_VALUE: Option<FnXmlReadValue> = None;
static mut FN_XML_REMOVE_NODE: Option<FnXmlRemoveNode> = None;

// ── Public API ──────────────────────────────────────────────────────

pub fn init(signatures: &SignatureStore) -> bool {
    if INITIALIZED.swap(true, Ordering::SeqCst) {
        return true;
    }

    // Resolve the per-side player-work table so the load path can recover the
    // player side by ddrcode (the load job carries no side index). Optional:
    // if it's unavailable, load-side resolution falls back to the legacy path.
    if let Some(addr) = signatures.get_address("player_work_table") {
        unsafe {
            PLAYER_WORK_TABLE = addr;
        }
    } else {
        log_warn!(
            "custom_options_persistence: player_work_table unresolved — 2-player option load may misroute to side 0"
        );
    }

    // Check config gating. Detours install if EITHER gate is on; inside the
    // trampolines, network emission/read is gated on `persist_network` and the
    // offline JSON write/read is gated on `persist_json`.
    let co = config::get().and_then(|c| c.custom_options.as_ref());
    let persist_network = co.map(|c| c.persist_network).unwrap_or(true);
    let persist_json = co.map(|c| c.persist_json).unwrap_or(true);
    PERSIST_NETWORK.store(persist_network, Ordering::SeqCst);
    PERSIST_JSON.store(persist_json, Ordering::SeqCst);

    // One-time migration of the legacy webui_options offline cache into the
    // custom_options section. Runs before the JSON-load timer reads the file.
    config::migrate_webui_options_to_custom_options();

    if !persist_network && !persist_json {
        log_info!(
            "custom_options_persistence: both gates off (persist_network=false, persist_json=false) — no detours"
        );
        return true;
    }

    if !custom_options::is_available() {
        log_warn!("custom_options_persistence: custom_options service unavailable — skipping");
        return true;
    }

    // Offline JSON *load* runs on a one-shot lazy timer that only reads the file
    // and primes the registry — it does NOT depend on the ess.dll detours.
    // Spawn it before resolving those detours so it survives a hook failure: if
    // ess.dll can't be hooked, JSON *save* (which rides save_sender) is lost but
    // JSON *load* still works (design R12). The timer must fire after all mods
    // register their options (at mod enable(), after this init), so it can't run
    // inline here.
    if persist_json {
        spawn_json_load_timer();
    }

    // The ess.dll save/load detours back the network path AND the JSON save
    // (which piggybacks on save_sender). Install them if either gate is on.
    if !resolve_libavs_ordinals() {
        log_warn!(
            "custom_options_persistence: failed to resolve libavs-win64 ordinals — save/load detours disabled (JSON load timer unaffected)"
        );
        return true;
    }

    if !resolve_and_hook_ess() {
        log_warn!(
            "custom_options_persistence: failed to hook ess.dll — save/load detours disabled (JSON load timer unaffected)"
        );
        return true;
    }

    // The ess save_sender detour is now live. Mark the score-submission guard
    // available so the autoplay mod can fail-closed against it (refuse to
    // enable if score suppression can't be enforced). The detour body consults
    // score_guard to suppress tainted (autoplayed / quick-failed) saves.
    score_guard::mark_hook_installed();

    // Network loads are captured at load_receiver time but can't be routed to a
    // side until the profile is in memory, so they're applied on SONG_SELECT
    // entry. Register the drain callback once. (Harmless if persist_network is
    // off — the pending buffer simply stays empty.)
    register_pending_load_drain();

    // Logout-save sanitiser: on entry to EAM_EXIT (0-idx 34), virginise the
    // play records of each tainted side so the imminent savekind==3 marshal
    // emits an empty stage list. Registered at the same point the save detours
    // install — the sanitise-and-forward policy only exists while the
    // save_sender trampoline is live to enforce its fallback.
    register_logout_sanitiser();

    log_info!(
        "custom_options_persistence: save/load detours installed (network={} json={}); score_guard hook marked available",
        persist_network,
        persist_json
    );

    true
}

/// Register a one-time scene-change callback that applies deferred network
/// loads (`PENDING_LOADS`) when the game reaches SONG_SELECT. By then each
/// player's `PlayerWork` is populated, so the load's ddrcode resolves to the
/// correct side.
///
/// Registering only appends to `scene_manager`'s callback list, which does not
/// require its detour to be installed yet — `scene_manager::init()` runs later
/// in the boot sequence (lib.rs step 5) than this service (step 4i), but well
/// before any real scene change, so the callback is live by the time it fires.
/// (Do NOT gate this on `scene_manager::is_available()`; the hook isn't active
/// at this point in init.)
fn register_pending_load_drain() {
    if SCENE_CALLBACK_REGISTERED.swap(true, Ordering::SeqCst) {
        return;
    }
    scene_manager::on_scene_change(Box::new(|_prev, next| {
        if next == scene::SONG_SELECT {
            apply_pending_card_in_resets();
            apply_pending_loads();
            apply_pending_string_loads();
        }
    }));
}

/// Drain `PENDING_RATE_RESETS` — the per-side card-in reset point: for each
/// successful card-in load, resolve its side by ddrcode and (a) clear ONLY
/// that side's song-rate pending-save ledger, (b) restore that side's
/// [`PersistMode::Session`](custom_options::PersistMode::Session) options to
/// their defaults (`custom_options::reset_session_values` — a new player
/// session must not inherit the previous session's practice-tool state).
/// An unresolved ddrcode resets nothing (fail closed — the design forbids a
/// P2 load from erasing P1 state or any broad reset from consuming rate
/// taint). Runs before `apply_pending_loads` on the same SONG_SELECT entry,
/// so the new session starts clean before any option application.
fn apply_pending_card_in_resets() {
    let pending: Vec<i32> = {
        let mut buf = PENDING_RATE_RESETS.lock().unwrap();
        if buf.is_empty() {
            return;
        }
        std::mem::take(&mut *buf)
    };
    for ddrcode in pending {
        match unsafe { side_from_ddrcode(ddrcode) } {
            Some(side) => {
                score_guard::reset_rate_state_for_side(side as usize);
                custom_options::reset_session_values(side);
                for cb in CARD_IN_CALLBACKS.lock().unwrap().iter() {
                    cb(side);
                }
                log_info!(
                    "custom_options_persistence: card-in reset — song-rate ledger cleared for side {} (ddrcode={})",
                    side,
                    ddrcode
                );
            }
            None => {
                log_warn!(
                    "custom_options_persistence: card-in reset — ddrcode={} unresolved at SONG_SELECT; song-rate ledger untouched",
                    ddrcode
                );
            }
        }
    }
}

/// Drain `PENDING_STRING_LOADS`: side-resolve each buffered profile's
/// string fields by ddrcode and hand them to their registered consumers.
/// Runs strictly after `apply_pending_card_in_resets` (card-in callbacks
/// have reset per-side state) on the same SONG_SELECT entry. Unresolved
/// ddrcodes drop their values (fail closed, same rule as the s32 drain).
fn apply_pending_string_loads() {
    let pending: Vec<PendingStringLoad> = {
        let mut buf = PENDING_STRING_LOADS.lock().unwrap();
        if buf.is_empty() {
            return;
        }
        std::mem::take(&mut *buf)
    };
    let fields = STRING_FIELDS.lock().unwrap();
    for load in pending {
        let Some(side) = (unsafe { side_from_ddrcode(load.ddrcode) }) else {
            log_warn!(
                "custom_options_persistence: string load — ddrcode={} unresolved at SONG_SELECT; {} value(s) dropped",
                load.ddrcode,
                load.values.len()
            );
            continue;
        };
        for (wire_name, value) in &load.values {
            if let Some(field) = fields.iter().find(|f| f.wire_name == *wire_name) {
                (field.load)(side, value);
            }
        }
    }
}

/// Register the one-time EAM_EXIT scene callback that runs the logout-save
/// record sanitiser. Same registration model (and the same "don't gate on
/// `scene_manager::is_available()`" caveat) as `register_pending_load_drain`.
fn register_logout_sanitiser() {
    if SANITISER_CALLBACK_REGISTERED.swap(true, Ordering::SeqCst) {
        return;
    }
    scene_manager::on_scene_change(Box::new(|_prev, next| {
        if next == scene::EAM_EXIT {
            sanitise_tainted_logout_records();
        }
    }));
    // Full-sanitization readiness latch (Song Playback Speed): the EAM_EXIT
    // sanitiser callback is one of its five prerequisites.
    score_guard::mark_sanitiser_registered();
}

/// The record half of the logout-save sanitiser (design D21–D26). Fires inside
/// `createNextSequence(34)` — strictly before `EAmExitRootSequence::onSetup`
/// and several frames before `SavePlayerDataActor` marshals the records; TOTAL
/// RESULTS (scene 32) has already rendered, so the summary is unaffected. For
/// each tainted side, write `mcode = -1` (the marshal's skip key) into all
/// five per-stage play records AND the course record, then mark the side
/// sanitised so the save trampoline forwards (rather than suppresses) its
/// logout save. The records are dead state after this point regardless — the
/// next session start re-initialises them. Any failure leaves the side
/// un-sanitised, and the trampoline falls back to full suppression (FR6).
fn sanitise_tainted_logout_records() {
    for side in 0..2usize {
        if !score_guard::logout_taint(side) {
            continue;
        }
        if !stage_records::is_available() {
            log_warn!(
                "logout sanitiser: stage_records unavailable — P{} logout save will be suppressed",
                side + 1
            );
            continue;
        }
        let mut complete = true;
        for stage in 0..stage_records::MAX_STAGE_RECORDS {
            match stage_records::stage_record(side, stage) {
                Some(rec) => unsafe { memory::write_i32(rec, -1) },
                None => complete = false,
            }
        }
        match stage_records::course_record(side) {
            Some(rec) => unsafe { memory::write_i32(rec, -1) },
            None => complete = false,
        }
        if complete {
            score_guard::mark_logout_sanitised(side);
            log_info!(
                "logout sanitiser: P{} records virginised (tainted session)",
                side + 1
            );
        } else {
            log_warn!(
                "logout sanitiser: P{} record walk failed — logout save will be suppressed",
                side + 1
            );
        }
    }
}

/// Drain `PENDING_LOADS`: for each captured profile load, resolve its side by
/// matching the load's ddrcode to the now-populated per-side PlayerWork, then
/// push each value into the registry via `resolve_from_load`. Runs on the
/// render thread from the SONG_SELECT scene callback.
fn apply_pending_loads() {
    let pending: Vec<PendingLoad> = {
        let mut buf = PENDING_LOADS.lock().unwrap();
        if buf.is_empty() {
            return;
        }
        std::mem::take(&mut *buf)
    };

    for load in pending {
        let side = match unsafe { side_from_ddrcode(load.ddrcode) } {
            Some(s) => s,
            None => {
                log_warn!(
                    "custom_options_persistence: deferred load — ddrcode={} still unresolved at SONG_SELECT; dropping {} value(s)",
                    load.ddrcode,
                    load.values.len()
                );
                continue;
            }
        };
        for (id, value) in &load.values {
            custom_options::resolve_from_load(id, side, *value);
        }
        log_info!(
            "custom_options_persistence: deferred load — applied {} option(s) to side {} (ddrcode={})",
            load.values.len(),
            side,
            load.ddrcode
        );
    }
}

/// Spawn the one-shot background timer that primes the registry from the
/// offline JSON cache `JSON_LOAD_DELAY_SECS` after init. Matches the project's
/// deferred-work idiom (`std::thread::spawn` + sleep; see lib.rs splash timer).
fn spawn_json_load_timer() {
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(JSON_LOAD_DELAY_SECS));
        json_load_once();
    });
}

/// Re-read `custom_options.{p1,p2}` from disk and prime the registry via
/// `resolve_from_load` for each cached value (applying each option's
/// `load_transform` + firing its `on_change`).
///
/// Runs on the background timer thread. `resolve_from_load` primes the
/// Mutex-guarded value cache (thread-safe) and fires `on_change`. Pre-login —
/// which is guaranteed by the timer-before-login ordering — a consumer like
/// WebUI's `on_change` early-returns on the null per-player work pointer, so no
/// game-memory write happens off the render thread; the cached value is applied
/// later on scene-20 entry through the mod's existing apply path. (Network
/// values still win: the network `load_receiver` re-applies on every card
/// swipe, which always happens after this one-shot timer.)
fn json_load_once() {
    if !custom_options::is_available() {
        log_warn!("custom_options_persistence: JSON load — custom_options service unavailable");
        return;
    }
    let values = config::read_custom_options_values();
    if values.is_empty() {
        log_info!("custom_options_persistence: JSON load — no cached custom_options values found");
        return;
    }
    let count = values.len();
    for (side, id, wire_value) in values {
        custom_options::resolve_from_load(&id, side, wire_value);
    }
    log_info!(
        "custom_options_persistence: JSON load — primed {} option value(s) from mod-config.json",
        count
    );
}

pub fn is_available() -> bool {
    INITIALIZED.load(Ordering::SeqCst)
}

// ── libavs-win64 ordinal resolution ─────────────────────────────────

fn resolve_libavs_ordinals() -> bool {
    unsafe {
        let dll = CString::new("libavs-win64.dll").unwrap();
        let handle = match GetModuleHandleA(PCSTR(dll.as_ptr() as *const u8)) {
            Ok(h) if !h.is_invalid() => h,
            _ => {
                log_warn!("custom_options_persistence: libavs-win64.dll not loaded");
                return false;
            }
        };

        macro_rules! resolve_ordinal {
            ($ordinal:expr, $ty:ty) => {{
                let addr = GetProcAddress(handle, PCSTR($ordinal as usize as *const u8));
                match addr {
                    Some(f) => {
                        #[allow(clippy::missing_transmute_annotations)]
                        let typed: $ty = std::mem::transmute(f);
                        typed
                    }
                    None => {
                        log_warn!(
                            "custom_options_persistence: Ordinal_{} not found in libavs-win64",
                            $ordinal
                        );
                        return false;
                    }
                }
            }};
        }

        FN_XML_FIND_CHILD = Some(resolve_ordinal!(162u16, FnXmlFindChild));
        FN_XML_ADD_CHILD = Some(resolve_ordinal!(163u16, FnXmlAddChild));
        // Same export, str-typed view (kbin type 11 passes the value by
        // pointer through the variadic slot).
        FN_XML_ADD_CHILD_STR = Some(resolve_ordinal!(163u16, FnXmlAddChildStr));
        FN_XML_GET_CTX = Some(resolve_ordinal!(175u16, FnXmlGetCtx));
        FN_XML_READ_VALUE = Some(resolve_ordinal!(176u16, FnXmlReadValue));

        // Ordinal 164 (property_node_remove) backs the logout-save sanitiser's
        // league strip. Resolved NON-fatally: a miss must not take down the
        // whole persistence bridge — it only forces tainted logout saves back
        // to full suppression (FR6).
        match GetProcAddress(handle, PCSTR(164usize as *const u8)) {
            Some(f) => {
                #[allow(clippy::missing_transmute_annotations)]
                let typed: FnXmlRemoveNode = std::mem::transmute(f);
                FN_XML_REMOVE_NODE = Some(typed);
                score_guard::mark_league_strip_available();
            }
            None => {
                log_warn!(
                    "custom_options_persistence: Ordinal_164 (property_node_remove) not found — league strip unavailable, tainted logout saves will be suppressed"
                );
            }
        }

        log_info!(
            "custom_options_persistence: resolved libavs-win64 ordinals 162/163/175/176 (164 league-strip: {})",
            score_guard::league_strip_available()
        );
        true
    }
}

// ── ess.dll resolution + detour installation ────────────────────────

fn resolve_and_hook_ess() -> bool {
    unsafe {
        let dll = CString::new("ess.dll").unwrap();
        let handle = match GetModuleHandleA(PCSTR(dll.as_ptr() as *const u8)) {
            Ok(h) if !h.is_invalid() => h,
            _ => {
                log_warn!("custom_options_persistence: ess.dll not loaded");
                return false;
            }
        };

        let mut info = MODULEINFO::default();
        if GetModuleInformation(
            GetCurrentProcess(),
            handle,
            &mut info,
            std::mem::size_of::<MODULEINFO>() as u32,
        )
        .is_err()
        {
            log_warn!("custom_options_persistence: GetModuleInformation failed for ess.dll");
            return false;
        }

        let ess_base = info.lpBaseOfDll as *const u8;
        let ess_size = info.SizeOfImage as usize;

        // Resolve sender/receiver by finding their unique log strings.
        let save_addr = resolve_by_log_string(
            ess_base,
            ess_size,
            "sys_playerdata_save_sender() start.",
            "save_sender",
        );
        let load_addr = resolve_by_log_string(
            ess_base,
            ess_size,
            "sys_playerdata_load_receiver() start.",
            "load_receiver",
        );

        let save_addr = match save_addr {
            Some(a) => a,
            None => {
                log_warn!("custom_options_persistence: save_sender not found in ess.dll");
                return false;
            }
        };

        let load_addr = match load_addr {
            Some(a) => a,
            None => {
                log_warn!("custom_options_persistence: load_receiver not found in ess.dll");
                return false;
            }
        };

        // Install save sender detour
        let save_target: EssSenderFn = std::mem::transmute(save_addr);
        if let Err(e) = crate::core::hooks::install_enabled(
            std::ptr::addr_of_mut!(HOOK_SAVE_SENDER),
            save_target,
            save_sender_trampoline,
        ) {
            log_warn!(
                "custom_options_persistence: save_sender detour install failed: {:?}",
                e
            );
            return false;
        }

        // Install load receiver detour
        let load_target: EssReceiverFn = std::mem::transmute(load_addr);
        if let Err(e) = crate::core::hooks::install_enabled(
            std::ptr::addr_of_mut!(HOOK_LOAD_RECEIVER),
            load_target,
            load_receiver_trampoline,
        ) {
            log_warn!(
                "custom_options_persistence: load_receiver detour install failed: {:?}",
                e
            );
            return false;
        }

        true
    }
}

/// Find a function in ess.dll by locating a unique log string that the
/// function references in its prologue (via LEA r64, [RIP+disp32]).
///
/// Uses `core::scanner::scan_lea_xrefs_to` to find the LEA instruction,
/// then `core::scanner::find_function_entry` to walk back to the function
/// start. Stable across builds because both 20250805 and 20260324 emit
/// the same log string literals from the same functions.
unsafe fn resolve_by_log_string(
    base: *const u8,
    size: usize,
    log_string: &str,
    label: &str,
) -> Option<*const u8> {
    let str_bytes = log_string.as_bytes();
    let slice = std::slice::from_raw_parts(base, size);

    // 1. Find the null-terminated log string in the module.
    let search_limit = size.saturating_sub(str_bytes.len() + 1);
    let mut string_ptr: Option<*const u8> = None;
    for i in 0..search_limit {
        if &slice[i..i + str_bytes.len()] == str_bytes && slice[i + str_bytes.len()] == 0 {
            string_ptr = Some(base.add(i));
            break;
        }
    }

    let string_ptr = match string_ptr {
        Some(p) => p,
        None => {
            log_warn!(
                "custom_options_persistence: log string for {} not found in ess.dll",
                label
            );
            return None;
        }
    };

    // 2. Find LEA instructions that reference this string.
    let xrefs = scanner::scan_lea_xrefs_to(base, size, string_ptr);
    if xrefs.is_empty() {
        log_warn!(
            "custom_options_persistence: no LEA xref found for {} log string",
            label
        );
        return None;
    }

    // 3. Walk back from the first xref to find the function entry.
    let fn_entry = scanner::find_function_entry(xrefs[0], base);
    log_info!(
        "custom_options_persistence: resolved {} @ {:p} (LEA at {:p}, string at {:p})",
        label,
        fn_entry,
        xrefs[0],
        string_ptr
    );
    Some(fn_entry)
}

// ── Detour trampolines ──────────────────────────────────────────────

/// Save-side detour. First enforces score-submission suppression: for a tainted
/// (autoplayed / quick-failed) side it returns a pretend-success without calling
/// the original sender, so no profile save — score or options — reaches the
/// server. Otherwise it calls the original (which builds the native request
/// including /option) and then appends `<mod_{id}>` children for each registered
/// custom option.
unsafe extern "C" fn save_sender_trampoline(job: *mut u8, kbin_ctx: *mut u8) -> u64 {
    let hook = &*addr_of!(HOOK_SAVE_SENDER);
    let original = hook.as_ref().unwrap();
    log_debug!(
        "custom_options_persistence: save_sender entered — job={:p} kbin_ctx={:p}",
        job,
        kbin_ctx
    );

    // Determine which player side this save is for, and which kind of save it
    // is. Both are read from the save-side savedata struct (pointer at
    // job+0x10): `playside` at +0x90 (0=P1, 1=P2), `savekind` at +0x74. These
    // are derived up front because the score-submission guard (below) decides
    // whether to suppress this save *before* the original sender builds the
    // request. Confirmed via Ghidra 20260324: *(job+0x10)+0x90 = playside.
    let savedata = *(job.add(0x10) as *const *const u8);
    let (playside_raw, savekind) = if !savedata.is_null() {
        (
            *(savedata.add(0x90) as *const i32),
            *(savedata.add(SAVEDATA_SAVEKIND_OFFSET) as *const i32),
        )
    } else {
        log_warn!("custom_options_persistence: save — savedata ptr at job+0x10 is null");
        (0, 0)
    };
    let decoded_side: Option<u8> =
        (playside_raw == 0 || playside_raw == 1).then_some(playside_raw as u8);

    // Song-rate election for per-stage saves runs BEFORE any side default:
    // pending rate-tainted stages are claimed/consumed by exact (side, stage)
    // identity, and any ambiguity (unknown side while rate state exists,
    // unknown/mismatched stage while the side has pending entries, ring
    // overflow, duplicate retries) suppresses fail-closed WITHOUT consuming —
    // never defaulting to P1 (design "Pending Rate Saves").
    if savekind == SAVEKIND_STAGE {
        let stage = stage_records::stage_counter();
        match score_guard::elect_rate_save_policy(decoded_side.map(|side| side as usize), stage) {
            score_guard::RateSavePolicy::NoRateOpinion => {}
            score_guard::RateSavePolicy::SuppressConsume {
                generation,
                stage_index,
            } => {
                // Belt-and-suspenders: commit already latched session taint
                // for the participating side; re-latching is idempotent.
                if let Some(side) = decoded_side {
                    score_guard::mark_session_tainted(side as usize);
                }
                log_warn!(
                    "score_guard: side={:?} savekind=2 rate-tainted stage save SUPPRESSED (generation={}, stage={})",
                    decoded_side,
                    generation,
                    stage_index
                );
                return 1;
            }
            score_guard::RateSavePolicy::SuppressNoConsume(reason) => {
                if let Some(side) = decoded_side {
                    score_guard::mark_session_tainted(side as usize);
                }
                log_warn!(
                    "score_guard: side={:?} savekind=2 stage save SUPPRESSED fail-closed ({:?}; decoded stage={:?}) — no pending entry consumed",
                    decoded_side,
                    reason,
                    stage
                );
                return 1;
            }
        }
    }

    let side: u8 = match decoded_side {
        Some(side) => side,
        None => {
            log_warn!(
                "custom_options_persistence: save — unexpected playside_raw={} (expected 0 or 1), defaulting to side 0",
                playside_raw
            );
            0
        }
    };

    // Score-submission policy (score_guard + design D21–D26):
    //
    //   * Per-stage save (savekind == 2): carries this song's `/result` score
    //     block — suppressed outright when this side's song is tainted
    //     (autoplay / quick-fail). Returning a pretend-success without calling
    //     the original sender means nothing is emitted for this side; the
    //     game's save state machine treats a nonzero return as success.
    //   * Logout save (savekind == 3): the only save carrying the
    //     profile/customize write-back, so a tainted side's is sanitised, not
    //     suppressed — three-way policy:
    //       clean                       -> forward unchanged;
    //       sanitised + league strip ok -> strip <data><league> from the built
    //                                      request (the records were already
    //                                      virginised at EAM_EXIT entry), then
    //                                      forward — profile data persists;
    //       otherwise                   -> fail closed: suppress outright, as
    //                                      the pre-sanitiser policy did.
    //   * `FIRST` saves carry no real score and are never touched.
    let mut strip_league = false;
    let suppress = match savekind {
        SAVEKIND_STAGE => score_guard::is_stage_suppressed(side as usize),
        SAVEKIND_LOGOUT => {
            if !score_guard::logout_taint(side as usize) {
                false
            } else if score_guard::was_logout_sanitised(side as usize)
                && score_guard::league_strip_available()
            {
                strip_league = true;
                false
            } else {
                true
            }
        }
        _ => false,
    };
    if suppress {
        if savekind == SAVEKIND_STAGE {
            // A suppressed per-stage save means this side actually produced a
            // tainted score this session, so latch the session-sticky flag —
            // the card-out logout save (which re-bundles every stage) must be
            // sanitised or suppressed too.
            score_guard::mark_session_tainted(side as usize);
            log_warn!(
                "score_guard: side={} savekind={} save SUPPRESSED (stage_taint={}, logout_taint={})",
                side,
                savekind,
                score_guard::is_stage_suppressed(side as usize),
                score_guard::logout_taint(side as usize)
            );
        } else {
            log_warn!(
                "score_guard: P{} logout save SUPPRESSED (sanitiser unavailable)",
                side + 1
            );
        }
        return 1;
    }
    if !strip_league {
        log_info!(
            "score_guard: side={} savekind={} save allowed (stage_taint={}, logout_taint={})",
            side,
            savekind,
            score_guard::is_stage_suppressed(side as usize),
            score_guard::logout_taint(side as usize)
        );
    }

    let result = original.call(job, kbin_ctx);

    if result == 0 {
        log_debug!("custom_options_persistence: save — original returned 0 (failure), skipping");
        return result;
    }

    // Sanitised logout save: the original sender has built the request tree —
    // remove the <data><league> accumulator (a PlayerWork-sourced client-side
    // score the record virginising cannot cover; the backend no-ops when the
    // node is absent, preserving the pre-session value) and forward the rest.
    // The AVS removal status is never ignored (tri-state semantics): a
    // removal FAILURE means the built tree still carries the league data, so
    // the trampoline signals sender failure instead of forwarding it — the
    // only fail-closed lever that exists after the tree is built.
    if strip_league {
        let outcome = strip_league_node(kbin_ctx);
        if score_guard::logout_league_forward_allowed(outcome) {
            log_warn!(
                "score_guard: P{} logout save SANITISED — scores stripped ({:?}), profile forwarded",
                side + 1,
                outcome
            );
        } else {
            log_error!(
                "score_guard: P{} logout league strip FAILED — signalling sender failure, save not forwarded",
                side + 1
            );
            return 0;
        }
    }

    // Snapshot registered options once; shared by both persistence paths.
    let snapshot = custom_options::snapshot_for_save();

    // ── Per-song judgement-offset leak fix (belt-and-braces layer) ──────────
    // Should be unreachable: the mod's scene-timed restore precedes every
    // marshal. If an override nonetheless survived into the built tree,
    // rewrite <timing_music> with the cached stock value so the profile is
    // never clobbered (design: Per-Song Judgement Offsets, override
    // lifecycle layer 2).
    if let Some(stock) =
        crate::mods::per_song_judgement_offsets::override_hook::leaked_stock(side as usize)
    {
        log_warn!(
            "judgement_offsets: P{} override LEAKED into a save -- rewriting <timing_music> to stock {}",
            side + 1,
            stock
        );
        if !replace_option_s32(kbin_ctx, b"timing_music\0", stock) {
            log_error!(
                "judgement_offsets: P{} <timing_music> tree fix FAILED -- profile may carry the override",
                side + 1
            );
        }
    }

    // ── Network persistence: append <mod_{id}> kbin children to the request ──
    if PERSIST_NETWORK.load(Ordering::SeqCst) {
        emit_network_children(kbin_ctx, &snapshot, side, playside_raw);
        emit_string_fields(kbin_ctx, side);
    }

    // ── Offline JSON persistence: write this side's values to mod-config.json ─
    if PERSIST_JSON.load(Ordering::SeqCst) {
        write_json_cache(&snapshot, side);
    }

    result
}

/// Persist the current side's option values to the `custom_options.{p1,p2}`
/// block in `mod-config.json` (the offline persistence path). Stores the same
/// post-`save_transform` wire values the network path emits, filtered to the
/// options whose `PersistMode` includes the JSON cache (`Full` — `SaveOnly`
/// options ride the network save only and never enter the offline cache).
/// Dirty-checked and per-side inside `config::save_custom_options_values`.
fn write_json_cache(snapshot: &[(String, [i32; 2])], side: u8) {
    let mut values = serde_json::Map::new();
    for (id, vals) in snapshot {
        if !custom_options::json_persisted(id) {
            continue;
        }
        values.insert(id.clone(), serde_json::json!(vals[side as usize]));
    }
    let count = values.len();
    let wrote = config::save_custom_options_values(side, serde_json::Value::Object(values));
    if wrote {
        log_info!(
            "custom_options_persistence: save — wrote {} option(s) to mod-config.json custom_options.{}",
            count,
            if side == 0 { "p1" } else { "p2" }
        );
    } else {
        log_debug!(
            "custom_options_persistence: save — JSON cache unchanged for side {}, skipped write",
            side
        );
    }
}

/// Remove the `<data><league>` node from a sanitised logout save's built
/// request tree via Ordinal 164 (`property_node_remove`). Null-safe at every
/// hop: an absent node (some sessions never build one) is normal — the
/// backend no-ops on a missing `<league>`, preserving the pre-session score.
/// Returns the tri-state outcome (design R6 semantics): `NodeAbsent` and
/// `Removed` are safe to forward; `RemovalFailed` — including an unresolvable
/// removal function while a strip was elected — must fail the save closed.
unsafe fn strip_league_node(kbin_ctx: *mut u8) -> score_guard::LeagueStripOutcome {
    use score_guard::LeagueStripOutcome;
    let Some(xml_find_child) = *addr_of!(FN_XML_FIND_CHILD) else {
        return LeagueStripOutcome::RemovalFailed;
    };
    let Some(remove_node) = *addr_of!(FN_XML_REMOVE_NODE) else {
        return LeagueStripOutcome::RemovalFailed;
    };
    let data_node = xml_find_child(0, kbin_ctx, b"data\0".as_ptr() as *const i8);
    if data_node.is_null() {
        return LeagueStripOutcome::NodeAbsent;
    }
    let league_node = xml_find_child(0, data_node, b"league\0".as_ptr() as *const i8);
    if league_node.is_null() {
        return LeagueStripOutcome::NodeAbsent;
    }
    if remove_node(league_node) >= 0 {
        LeagueStripOutcome::Removed
    } else {
        LeagueStripOutcome::RemovalFailed
    }
}

/// Replace an s32 child of `/data/option` in the BUILT save tree with a new
/// value: find (Ordinal 162) → remove (164) → re-add (163, kbin type 6).
/// There is no set-value-in-place AVS ordinal; the re-added node lands at
/// the end of `<option>`'s children, which is immaterial for name-keyed
/// readers (bemani-buddy). Returns false when any leg is unavailable or
/// fails — the caller decides how loudly to escalate. `name` must be
/// NUL-terminated. Fires only post-`original.call` (the tree exists then).
pub(crate) unsafe fn replace_option_s32(kbin_ctx: *mut u8, name: &[u8], value: i32) -> bool {
    debug_assert!(name.ends_with(b"\0"));
    let Some(xml_find_child) = *addr_of!(FN_XML_FIND_CHILD) else {
        return false;
    };
    let Some(remove_node) = *addr_of!(FN_XML_REMOVE_NODE) else {
        return false;
    };
    let Some(xml_add_child) = *addr_of!(FN_XML_ADD_CHILD) else {
        return false;
    };
    let Some(xml_get_ctx) = *addr_of!(FN_XML_GET_CTX) else {
        return false;
    };
    let data_node = xml_find_child(0, kbin_ctx, b"data\0".as_ptr() as *const i8);
    if data_node.is_null() {
        return false;
    }
    let option_node = xml_find_child(0, data_node, b"option\0".as_ptr() as *const i8);
    if option_node.is_null() {
        return false;
    }
    let existing = xml_find_child(0, option_node, name.as_ptr() as *const i8);
    if !existing.is_null() && remove_node(existing) < 0 {
        return false;
    }
    let ctx = xml_get_ctx(kbin_ctx);
    if ctx.is_null() {
        return false;
    }
    let new_node = xml_add_child(
        ctx,
        option_node,
        6, // kbin type s32
        name.as_ptr() as *const i8,
        value,
    );
    !new_node.is_null()
}

/// Append every registered string field with a `Some` value as a kbin
/// `str` child of `/data/option` in the built save tree. Same navigation as
/// `emit_network_children`; failures skip the field (warn-once) — the CSV /
/// local persistence paths are unaffected.
unsafe fn emit_string_fields(kbin_ctx: *mut u8, side: u8) {
    let fields = STRING_FIELDS.lock().unwrap();
    if fields.is_empty() {
        return;
    }
    let Some(xml_find_child) = *addr_of!(FN_XML_FIND_CHILD) else {
        return;
    };
    let Some(xml_add_child_str) = *addr_of!(FN_XML_ADD_CHILD_STR) else {
        return;
    };
    let Some(xml_get_ctx) = *addr_of!(FN_XML_GET_CTX) else {
        return;
    };
    let data_node = xml_find_child(0, kbin_ctx, b"data\0".as_ptr() as *const i8);
    if data_node.is_null() {
        return;
    }
    let option_node = xml_find_child(0, data_node, b"option\0".as_ptr() as *const i8);
    if option_node.is_null() {
        return;
    }
    let ctx = xml_get_ctx(kbin_ctx);
    if ctx.is_null() {
        return;
    }
    for field in fields.iter() {
        let Some(value) = (field.save)(side) else {
            continue; // omitted this save (un-armed / disabled)
        };
        let Ok(c_value) = std::ffi::CString::new(value) else {
            warn_str_emit_once("interior NUL in value");
            continue;
        };
        let name = format!("{}\0", field.wire_name);
        let new_node = xml_add_child_str(
            ctx,
            option_node,
            11, // kbin type str
            name.as_ptr() as *const i8,
            c_value.as_ptr(),
        );
        if new_node.is_null() {
            warn_str_emit_once("Ordinal_163 (str) returned null");
        } else {
            log_info!(
                "custom_options_persistence: save — emitted <{}> ({} bytes, side {})",
                field.wire_name,
                c_value.as_bytes().len(),
                side
            );
        }
    }
}

fn warn_str_emit_once(reason: &str) {
    if !WARNED_STR_EMIT.swap(true, Ordering::AcqRel) {
        log_warn!(
            "custom_options_persistence: string-field emit failed ({}) — network persistence of string fields skipped",
            reason
        );
    }
}

/// Append one `<mod_{id}>` s32 child per registered option to the built
/// playerdata-save XML tree (the network persistence path). Internal
/// early-returns bail out of network emission only — they do not affect any
/// other persistence path in the calling trampoline.
unsafe fn emit_network_children(
    kbin_ctx: *mut u8,
    snapshot: &[(String, [i32; 2])],
    side: u8,
    playside_raw: i32,
) {
    let xml_find_child = match *addr_of!(FN_XML_FIND_CHILD) {
        Some(f) => f,
        None => return,
    };
    let xml_add_child = match *addr_of!(FN_XML_ADD_CHILD) {
        Some(f) => f,
        None => return,
    };

    // Navigate to the /data/option node in the built XML tree. The save sender
    // has already closed the option block, but the tree is still in memory —
    // we can add more children.
    let data_node = xml_find_child(0, kbin_ctx, b"data\0".as_ptr() as *const i8);
    if data_node.is_null() {
        log_warn!("custom_options_persistence: save — data node not found in XML tree");
        return;
    }
    let option_node = xml_find_child(0, data_node, b"option\0".as_ptr() as *const i8);
    if option_node.is_null() {
        log_warn!("custom_options_persistence: save — option node not found under data");
        return;
    }

    // Get the XML context for writing
    let xml_get_ctx = match *addr_of!(FN_XML_GET_CTX) {
        Some(f) => f,
        None => return,
    };
    let ctx = xml_get_ctx(kbin_ctx);
    if ctx.is_null() {
        log_warn!("custom_options_persistence: save — failed to get XML context");
        return;
    }

    log_info!(
        "custom_options_persistence: save — emitting {} mod options for side {} (playside_raw={})",
        snapshot.len(),
        side,
        playside_raw
    );
    for (id, values) in snapshot {
        let wire_name = format!("mod_{}\0", id);
        let value = values[side as usize];
        let new_node = xml_add_child(
            ctx,
            option_node,
            6, // kbin type s32
            wire_name.as_ptr() as *const i8,
            value,
        );
        if new_node.is_null() {
            log_warn!(
                "custom_options_persistence: save — Ordinal_163 returned null for mod_{} (value={})",
                id, value
            );
        }
    }
}

/// Resolve a load's player side by matching its `ddrcode` against the per-side
/// `PlayerWork` profiles. The load job carries no side index (it reuses one
/// savedata buffer for both players), so the side is recovered here: for each
/// side, walk `player_work_table[side]` → wrapper → PlayerWork and compare
/// `*(PlayerWork + 0x18)` (the profile's ddrcode) to the incoming `ddrcode`.
///
/// Returns the matching side, or `None` if the table is unresolved, a slot is
/// empty, or no profile matches (e.g. the ddrcode hasn't been committed to a
/// PlayerWork yet) — callers fall back to the legacy behavior in that case.
unsafe fn side_from_ddrcode(ddrcode: i32) -> Option<u8> {
    let table = *addr_of!(PLAYER_WORK_TABLE);
    if table.is_null() || ddrcode == 0 {
        return None;
    }
    let table = table as *const *const u8;
    for side in 0u8..2 {
        let wrapper = *table.add(side as usize);
        if wrapper.is_null() {
            continue;
        }
        let player_work = *(wrapper as *const *const u8);
        if player_work.is_null() {
            continue;
        }
        let pw_ddrcode = *(player_work.add(PLAYER_WORK_DDRCODE_OFFSET) as *const i32);
        if pw_ddrcode == ddrcode {
            return Some(side);
        }
    }
    None
}

/// Load-side detour: call original (parses native response including /option),
/// then read back `<mod_{id}>` children and push into custom_options.
unsafe extern "C" fn load_receiver_trampoline(job: *mut u8, kbin_ctx: *mut u8) -> u64 {
    let hook = &*addr_of!(HOOK_LOAD_RECEIVER);
    let original = hook.as_ref().unwrap();

    log_debug!(
        "custom_options_persistence: load_receiver entered — job={:p} kbin_ctx={:p}",
        job,
        kbin_ctx
    );

    // Card-in marks the start of a new player session. Clear the score-guard's
    // session-sticky taint so a clean session uploads normally even if the
    // previous session was tainted (design R8). Runs before the early-returns
    // so it always fires on a load, regardless of persist gating.
    score_guard::reset_session();

    let result = original.call(job, kbin_ctx);

    if result == 0 {
        log_debug!("custom_options_persistence: load — original returned 0 (failure), skipping");
        return result;
    }

    // Song-rate per-side reset ownership: a SUCCESSFUL profile load marks a
    // new session for exactly the side this profile belongs to — but the side
    // cannot be resolved yet (`PlayerWork+0x18` populates only after the load
    // completes), so capture the ddrcode and defer the positively matched
    // reset to SONG_SELECT entry alongside the pending-load drain. Failed or
    // unidentified loads clear nothing, and a P2 load can never erase P1
    // state. Deliberately independent of the persist_network gate — the reset
    // ownership is score policy, not persistence.
    {
        let savedata = *(job.add(LOAD_JOB_SAVEDATA_PTR_OFFSET) as *const *const u8);
        if !savedata.is_null() {
            let ddrcode = *(savedata.add(LOAD_SAVEDATA_DDRCODE_OFFSET) as *const i32);
            if ddrcode != 0 {
                PENDING_RATE_RESETS.lock().unwrap().push(ddrcode);
            }
        }
    }

    // Network read is gated independently of JSON load (which rides a lazy
    // timer, not this detour). With persist_network off, the detour is still
    // installed (because persist_json may be on) but reads nothing here.
    if !PERSIST_NETWORK.load(Ordering::SeqCst) {
        return result;
    }

    // Retrieve function pointers
    let xml_find_child = match *addr_of!(FN_XML_FIND_CHILD) {
        Some(f) => f,
        None => return result,
    };
    let xml_read_value = match *addr_of!(FN_XML_READ_VALUE) {
        Some(f) => f,
        None => return result,
    };
    let xml_get_ctx = match *addr_of!(FN_XML_GET_CTX) {
        Some(f) => f,
        None => return result,
    };

    // Navigate to the option node in the response
    let option_node = xml_find_child(0, kbin_ctx, b"option\0".as_ptr() as *const i8);
    if option_node.is_null() {
        log_debug!("custom_options_persistence: load — option node not found (may be expected for non-profile responses)");
        return result;
    }

    let ctx = xml_get_ctx(kbin_ctx);
    if ctx.is_null() {
        log_warn!("custom_options_persistence: load — xml_get_ctx returned null");
        return result;
    }

    // Capture the ddrcode that identifies which profile this load is for. The
    // load job reuses one savedata buffer for both players and carries no side
    // index; the side is recovered later by matching this ddrcode to a per-side
    // PlayerWork. We cannot resolve it here because the game populates
    // `PlayerWork+0x18` only *after* this load completes — so application is
    // deferred to SONG_SELECT entry (see `apply_pending_loads`).
    let savedata = *(job.add(LOAD_JOB_SAVEDATA_PTR_OFFSET) as *const *const u8);
    let ddrcode = if savedata.is_null() {
        0
    } else {
        *(savedata.add(LOAD_SAVEDATA_DDRCODE_OFFSET) as *const i32)
    };

    // Read every registered option's value from the response, stashing the raw
    // wire values for deferred, side-resolved application.
    let snapshot = custom_options::snapshot_for_save();
    let mut values: Vec<(String, i32)> = Vec::new();
    for (id, _) in &snapshot {
        let wire_name = format!("mod_{}\0", id);
        let mut value: i32 = 0;
        let status = xml_read_value(
            ctx,
            option_node,
            wire_name.as_ptr() as *const i8,
            6, // kbin type s32
            &mut value as *mut i32,
            4, // dest_size
        );
        if status >= 0 {
            values.push((id.clone(), value));
        }
    }

    // String-valued wire fields (str reads via the same ordinal-176 call
    // with kbin type 11; dest = byte buffer, capacity in the size slot —
    // the ess ghost-read convention). Buffered alongside the s32 values,
    // same ddrcode-keyed deferral.
    let mut string_values: Vec<(&'static str, String)> = Vec::new();
    {
        let fields = STRING_FIELDS.lock().unwrap();
        if !fields.is_empty() {
            // Comfortably above the producers' caps (judge-offsets encodes
            // <= ~26 KB) while bounding a hostile/corrupt response.
            const STR_READ_CAP: usize = 0x10000;
            let mut buf = vec![0u8; STR_READ_CAP];
            for field in fields.iter() {
                buf[0] = 0;
                let name = format!("{}\0", field.wire_name);
                let status = xml_read_value(
                    ctx,
                    option_node,
                    name.as_ptr() as *const i8,
                    11, // kbin type str
                    buf.as_mut_ptr() as *mut i32,
                    STR_READ_CAP as i32,
                );
                if status < 0 {
                    continue; // absent — the normal un-persisted case
                }
                let len = buf.iter().position(|&b| b == 0).unwrap_or(0);
                match std::str::from_utf8(&buf[..len]) {
                    Ok(s) => string_values.push((field.wire_name, s.to_string())),
                    Err(_) => log_warn!(
                        "custom_options_persistence: load — <{}> is not valid UTF-8, ignored",
                        field.wire_name
                    ),
                }
            }
        }
    }
    if !string_values.is_empty() {
        log_info!(
            "custom_options_persistence: load — captured {} string field(s) for ddrcode={} (deferred to SONG_SELECT)",
            string_values.len(),
            ddrcode
        );
        PENDING_STRING_LOADS
            .lock()
            .unwrap()
            .push(PendingStringLoad {
                ddrcode,
                values: string_values,
            });
    }

    if values.is_empty() {
        log_info!(
            "custom_options_persistence: load — no mod options in response (ddrcode={})",
            ddrcode
        );
        return result;
    }

    let count = values.len();
    PENDING_LOADS
        .lock()
        .unwrap()
        .push(PendingLoad { ddrcode, values });
    log_info!(
        "custom_options_persistence: load — captured {}/{} mod option(s) for ddrcode={} (deferred to SONG_SELECT)",
        count,
        snapshot.len(),
        ddrcode
    );

    result
}
