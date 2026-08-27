//! Scene Manager — Tracks the current game scene via hook on
//! TransitionSequence::createNextSequence.
//!
//! Uses `retour::GenericDetour` for the hook. The scene ID is in RDX (arg 2),
//! 1-indexed. We subtract 1 for 0-indexed scene IDs.

use once_cell::sync::Lazy;
use retour::GenericDetour;
use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::{Arc, Mutex};

use crate::core::signatures::SignatureStore;
use crate::types::scenes::get_scene_name;
use crate::{log_info, log_warn};

pub type SceneChangeCallback = Box<dyn Fn(i32, i32) + Send + Sync>;

/// Internal storage: `Arc` so the hook can SNAPSHOT the list and fire
/// the callbacks with the manager mutex RELEASED. Firing under the lock
/// deadlocks any callback that touches the scene manager again —
/// `current_scene()` being the landmine that froze the cabinet on
/// 2026-08-14 (training's threshold-restore condition evaluated it from
/// inside the gameplay-exit callback; the frame thread parked forever on
/// the reentrant lock, game unresponsive to everything incl. the test
/// menu).
type StoredCallback = Arc<dyn Fn(i32, i32) + Send + Sync>;

pub(crate) struct SceneManagerInner {
    current_scene: i32,
    previous_scene: i32,
    callbacks: Vec<(usize, StoredCallback)>,
    redirects: HashMap<i32, i32>,
    /// `from` keys whose redirect should be removed after firing once.
    /// Used by Quick Restart to redirect STAGE_RESULT→GAMEPLAY for one
    /// transition without affecting later natural song-end flows.
    one_shot_redirects: HashSet<i32>,
    next_callback_id: usize,
}

pub(crate) static SCENE_MANAGER: Lazy<Mutex<SceneManagerInner>> = Lazy::new(|| {
    Mutex::new(SceneManagerInner {
        current_scene: -1,
        previous_scene: -1,
        callbacks: Vec::new(),
        redirects: HashMap::new(),
        one_shot_redirects: HashSet::new(),
        next_callback_id: 0,
    })
});

static SCENE_HOOK_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Latched when the `advance_to_scene` hook (the post-redirect `m_currentID`
/// repair) installs successfully. Load-bearing for mods whose redirects feed
/// the game's automatic `getNextID` tail (quick-logout): without the repair, a
/// redirect leaves the stale pre-redirect id in `TS+0x68` and the tail
/// mis-routes.
static REDIRECT_REPAIR_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Snapshot of the most-recent `TransitionSequence*` observed by the
/// `createNextSequence` hook. Mods that need to initiate scene
/// transitions themselves (e.g. QuickRestartOrFailMod) read this to
/// get the `this` argument they must pass to the transition trigger.
/// Null until the first scene transition is observed.
static CURRENT_TRANSITION_SEQUENCE: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());

/// The function signature: (this: *mut u8, scene_id_1indexed: i32, ...) -> *mut u8
type CreateNextSequenceFn = unsafe extern "C" fn(*mut u8, i32) -> *mut u8;

/// TS::advanceToScene signature: (this: *mut u8, scene_id_1indexed: i32) -> void
/// (actually returns void via tail-call, but we model as no-return-value)
type AdvanceToSceneFn = unsafe extern "C" fn(*mut u8, i32);

static mut SCENE_HOOK: Option<GenericDetour<CreateNextSequenceFn>> = None;
static mut ADVANCE_TO_SCENE_HOOK: Option<GenericDetour<AdvanceToSceneFn>> = None;

thread_local! {
    static REDIRECTED_SCENE_ID: Cell<Option<i32>> = const { Cell::new(None) };
}

unsafe extern "C" fn scene_hook(this: *mut u8, scene_id_1indexed: i32) -> *mut u8 {
    let original_scene_id = scene_id_1indexed - 1;
    let mut scene_id = original_scene_id;

    // Snapshot the TransitionSequence pointer so consumers
    // (QuickRestartOrFailMod) can initiate transitions of their own.
    CURRENT_TRANSITION_SEQUENCE.store(this, Ordering::Release);

    {
        let mut mgr = SCENE_MANAGER.lock().unwrap();

        if let Some(&redirect) = mgr.redirects.get(&scene_id) {
            log_info!(
                "SceneManager: redirecting scene {} -> {}",
                scene_id,
                redirect
            );
            let from = scene_id;
            scene_id = redirect;
            if mgr.one_shot_redirects.remove(&from) {
                mgr.redirects.remove(&from);
                log_info!(
                    "SceneManager: one-shot redirect {} -> {} consumed",
                    from,
                    redirect
                );
            }
        }

        mgr.previous_scene = mgr.current_scene;
        mgr.current_scene = scene_id;
    }

    // Snapshot state + callbacks under the lock, then fire OUTSIDE it —
    // callbacks may legitimately re-enter the scene manager
    // (`current_scene()`, redirect registration); firing under the lock
    // self-deadlocks the frame thread (cabinet freeze, 2026-08-14).
    let (prev, next, callbacks) = {
        let mgr = SCENE_MANAGER.lock().unwrap();
        (
            mgr.previous_scene,
            mgr.current_scene,
            mgr.callbacks
                .iter()
                .map(|(_, cb)| Arc::clone(cb))
                .collect::<Vec<_>>(),
        )
    };

    log_info!(
        "Scene change hook: prev={} prev_name={} next={} next_name={} raw_next={}",
        prev,
        get_scene_name(prev),
        next,
        get_scene_name(next),
        scene_id_1indexed
    );

    for cb in callbacks {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            cb(prev, next);
        }));
    }

    // Signal to the outer advance_to_scene_hook that a redirect occurred.
    if scene_id != original_scene_id {
        REDIRECTED_SCENE_ID.with(|cell| cell.set(Some(scene_id)));
    }

    // Call original with potentially redirected scene ID.
    if let Some(ref hook) = SCENE_HOOK {
        hook.call(this, scene_id + 1)
    } else {
        std::ptr::null_mut()
    }
}

/// Detour for TS::advanceToScene. Wraps the entire function so we can
/// fix m_currentID AFTER the framework's own `MOV [RDI+0x68], EBX`
/// clobber has already executed.
unsafe extern "C" fn advance_to_scene_hook(this: *mut u8, scene_id_1indexed: i32) {
    // Clear any stale redirect signal before calling original.
    REDIRECTED_SCENE_ID.with(|cell| cell.set(None));

    // Call the original advanceToScene — this triggers:
    //   1. createNextSequence (our scene_hook fires, may redirect)
    //   2. installSequence (installs new gosub child)
    //   3. MOV [this+0x68], original_scene_id (the clobber)
    //   4. tail-call notification
    if let Some(ref hook) = ADVANCE_TO_SCENE_HOOK {
        hook.call(this, scene_id_1indexed);
    }

    // If scene_hook redirected, fix m_currentID to match the actual scene.
    let redirected = REDIRECTED_SCENE_ID.with(|cell| cell.take());
    if let Some(scene_id) = redirected {
        let m_current_id = this.add(0x68) as *mut i32;
        let correct_value = scene_id + 1;
        log_info!(
            "SceneManager: fixing m_currentID after redirect: {} -> {}",
            *m_current_id,
            correct_value
        );
        *m_current_id = correct_value;
    }
}

pub fn init(signatures: &SignatureStore) -> bool {
    let addr = match signatures.get_address("scene_transition") {
        Some(a) => a,
        None => {
            log_warn!("SceneManager: scene_transition signature not resolved");
            return false;
        }
    };

    unsafe {
        let target: CreateNextSequenceFn = std::mem::transmute(addr);
        match crate::core::hooks::install_enabled(
            std::ptr::addr_of_mut!(SCENE_HOOK),
            target,
            scene_hook,
        ) {
            Ok(()) => {
                SCENE_HOOK_ACTIVE.store(true, Ordering::Release);
                log_info!("SceneManager: hooked scene_transition @ {:p}", addr);
            }
            Err(e) => {
                log_warn!("SceneManager: failed to install hook: {}", e);
                return false;
            }
        }
    }

    // Hook advanceToScene to fix m_currentID after the framework's clobber.
    let ats_addr = match signatures.get_address("advance_to_scene") {
        Some(a) => a,
        None => {
            log_warn!("SceneManager: advance_to_scene signature not resolved — m_currentID fix unavailable");
            return true;
        }
    };

    unsafe {
        let target: AdvanceToSceneFn = std::mem::transmute(ats_addr);
        match crate::core::hooks::install_enabled(
            std::ptr::addr_of_mut!(ADVANCE_TO_SCENE_HOOK),
            target,
            advance_to_scene_hook,
        ) {
            Ok(()) => {
                REDIRECT_REPAIR_ACTIVE.store(true, Ordering::Release);
                log_info!("SceneManager: hooked advance_to_scene @ {:p}", ats_addr);
            }
            Err(e) => {
                log_warn!(
                    "SceneManager: failed to install advance_to_scene hook: {}",
                    e
                );
            }
        }
    }

    true
}

pub fn current_scene() -> i32 {
    SCENE_MANAGER.lock().unwrap().current_scene
}

pub fn current_scene_name() -> String {
    let id = current_scene();
    if id >= 0 {
        get_scene_name(id)
    } else {
        "UNKNOWN".into()
    }
}

/// Returns the most-recent `TransitionSequence*` captured by the
/// `createNextSequence` hook, or `None` if no transition has been
/// observed yet (e.g. before the first scene change after DLL load).
pub fn current_transition_sequence() -> Option<*mut u8> {
    let p = CURRENT_TRANSITION_SEQUENCE.load(Ordering::Acquire);
    if p.is_null() {
        None
    } else {
        Some(p)
    }
}

pub fn on_scene_change(callback: SceneChangeCallback) -> usize {
    let mut mgr = SCENE_MANAGER.lock().unwrap();
    let id = mgr.next_callback_id;
    mgr.next_callback_id += 1;
    // Box -> Arc so the dispatch can snapshot-and-fire outside the lock.
    mgr.callbacks.push((id, Arc::from(callback)));
    id
}

/// Remove a scene callback. NOTE: the dispatch fires a SNAPSHOT of the
/// list taken at the scene-change instant, so a callback removed from
/// another thread may still run one final time — callbacks must tolerate
/// firing after removal (every mod gates on its own ACTIVE latch).
pub fn remove_callback(id: usize) {
    let mut mgr = SCENE_MANAGER.lock().unwrap();
    mgr.callbacks.retain(|(cid, _)| *cid != id);
}

pub fn add_redirect(from: i32, to: i32) {
    let mut mgr = SCENE_MANAGER.lock().unwrap();
    mgr.redirects.insert(from, to);
    mgr.one_shot_redirects.remove(&from);
    log_info!("SceneManager: redirect registered {} -> {}", from, to);
}

/// Register a redirect that fires exactly once and is then removed.
/// Used by Quick Restart to redirect STAGE_RESULT→GAMEPLAY for the
/// next transition only.
pub fn add_redirect_once(from: i32, to: i32) {
    let mut mgr = SCENE_MANAGER.lock().unwrap();
    mgr.redirects.insert(from, to);
    mgr.one_shot_redirects.insert(from);
    log_info!(
        "SceneManager: one-shot redirect registered {} -> {}",
        from,
        to
    );
}

pub fn remove_redirect(from: i32) {
    let mut mgr = SCENE_MANAGER.lock().unwrap();
    mgr.redirects.remove(&from);
    mgr.one_shot_redirects.remove(&from);
}

pub fn is_available() -> bool {
    SCENE_HOOK_ACTIVE.load(Ordering::Acquire)
}

/// True when the `advance_to_scene` hook (post-redirect `m_currentID` repair)
/// installed. Mods whose redirects hand control to the game's automatic
/// `getNextID` tail (quick-logout) must refuse to enable when this is false —
/// a redirect without the repair leaves `TS+0x68` at the pre-redirect id and
/// the tail after the redirected scene runs the wrong successor.
pub fn redirect_repair_available() -> bool {
    REDIRECT_REPAIR_ACTIVE.load(Ordering::Acquire)
}
