//! `scene3d` — the engine-facing 3D scene service for the Background Dancers
//! feature (design §4.2; RE record `docs/background_dancers_research.md` §1).
//!
//! World's engine still runs the full 3D pipeline every frame (KTMDL model
//! loader, `MODEL:*` render passes, `agcs::scene::SceneGraph`) with an EMPTY
//! item list — Konami deleted only the game-side scene layer. This service is
//! the DLL's replacement for that layer's engine contact points:
//!
//! * [`arc_set`] — register / free `.arc` files through the game's own
//!   `FileManager` (whose `ModelFileCallback` converts `.model` members into
//!   GPU model resources), with LayeredFS mod-folder overrides honoured.
//! * [`model_registry`] — look a converted model up by name in the
//!   `ResourceManager`'s model map (A3's lookup-by-hash has no World twin, so
//!   the red-black-tree walk is ours, read-only, under the map's own mutex).
//!
//! Later steps add the render-item builder, the mod-owned scene-graph node,
//! root attach / deferred destroy and the camera-slot writer.
//!
//! ## Threading
//!
//! Every function that touches the engine (`FileManager`, the registry walk)
//! is GAME/RENDER-THREAD ONLY — callers route through
//! `widget_renderer::run_on_render_thread` or a frame callback. Pure path
//! helpers and `read_bytes` are thread-agnostic.
//!
//! ## Addresses
//!
//! Every engine address and struct offset comes from
//! [`SignatureStore::scene3d_sites`] — the all-or-nothing `derive_scene3d`
//! group. Nothing here is hardcoded; when the group is missing on a build the
//! service reports unavailable and every consumer stays inert.

pub mod arc_set;
pub mod frame_board;
pub mod model_registry;
pub mod node;
pub mod node_layout;
pub mod pure;
pub mod render_item;
pub mod render_item_layout;
pub mod scene_graph;
pub mod texture;

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

use crate::core::memory;
use crate::core::signatures::{Scene3dSites, SignatureStore};
use crate::{log_info, log_warn};

/// `agcs::FileManager::Load(this, path) -> i32 handle` (index; -1 on failure).
pub(crate) type FileLoadFn = unsafe extern "C" fn(this: *mut u8, path: *const i8) -> i32;
/// `agcs::FileManager::Free(this, index)` — enqueues the handle for release.
pub(crate) type FileFreeFn = unsafe extern "C" fn(this: *mut u8, index: i32);
/// libavs-win64 `avs_mutex_lock` / `avs_mutex_unlock` (ordinals 16 / 17):
/// `void fn(i32 mutex_id)` — the pointer is read out of gamemdx's IAT slot at
/// call time, exactly what the engine's own callers do.
type AvsMutexFn = unsafe extern "C" fn(id: i32);

pub(crate) struct Inner {
    pub sites: Scene3dSites,
    pub file_load: FileLoadFn,
    pub file_free: FileFreeFn,
    /// Address of the FileManager singleton GLOBAL (not the object) —
    /// dereferenced per call so a late-created manager is picked up.
    pub file_manager_singleton: *const u8,
}

// Fixed game-module addresses, valid for the process lifetime.
unsafe impl Send for Inner {}
unsafe impl Sync for Inner {}

static INNER: OnceLock<Inner> = OnceLock::new();

/// Capture the `scene3d` group + the FileManager trio. Call once at init
/// (after `resolve_derived`). `false` ⇒ the service is unavailable for the
/// boot (one WARN naming what was missing; nothing panics).
pub fn init(signatures: &SignatureStore) -> bool {
    let sites = signatures.scene3d_sites();
    let file_load = signatures.get_address("file_manager_load");
    let file_free = signatures.get_address("file_manager_free");
    let singleton = signatures.get_address("file_manager_singleton");
    let (Some(sites), Some(fl), Some(ff), Some(sg)) = (sites, file_load, file_free, singleton)
    else {
        log_warn!(
            "scene3d: unavailable (scene3d group={} file_manager load/free/singleton={}/{}/{})",
            sites.is_some(),
            file_load.is_some(),
            file_free.is_some(),
            singleton.is_some()
        );
        return false;
    };
    let inner = Inner {
        sites,
        // SAFETY: both addresses were AOB-resolved to the engine functions
        // whose prototypes the aliases describe (asset_loader uses the same).
        file_load: unsafe { std::mem::transmute::<*const u8, FileLoadFn>(fl) },
        file_free: unsafe { std::mem::transmute::<*const u8, FileFreeFn>(ff) },
        file_manager_singleton: sg,
    };
    if INNER.set(inner).is_err() {
        log_warn!("scene3d: init called twice");
        return false;
    }
    let base = signatures.module_base() as usize;
    let rel = |p: *const u8| (p as usize).wrapping_sub(base);
    log_info!(
        "scene3d: available (mgr=+0x{:X} rm=+0x{:X} tex create/release=+0x{:X}/+0x{:X} bgmovie=+0x{:X} pool=+0x{:X})",
        rel(sites.scene_graph_manager),
        rel(sites.resource_manager),
        rel(sites.texture_create),
        rel(sites.texture_release),
        rel(sites.bgmovie_actor),
        rel(sites.cmovieclip_pool)
    );
    true
}

/// Whether the group resolved on this build. Consumers check this before
/// wiring anything (graceful degradation).
pub fn is_available() -> bool {
    INNER.get().is_some()
}

pub(crate) fn inner() -> Option<&'static Inner> {
    INNER.get()
}

/// The derived site bundle, or `None` when unavailable.
pub(crate) fn sites() -> Option<&'static Scene3dSites> {
    INNER.get().map(|i| &i.sites)
}

/// The live `FileManager` object (the singleton global dereferenced now), or
/// `None` while it is still null.
pub(crate) fn file_manager() -> Option<*mut u8> {
    let inner = INNER.get()?;
    if inner.file_manager_singleton.is_null() {
        return None;
    }
    // SAFETY: the global is a fixed game data address inside the module.
    let obj = unsafe { *(inner.file_manager_singleton as *const *mut u8) };
    if obj.is_null() {
        None
    } else {
        Some(obj)
    }
}

// ── The engine's avs mutex protocol ─────────────────────────────────

/// Tri-state cache of the IAT-slot validation: 0 unchecked, 1 OK, 2 bad.
static MUTEX_STATE: AtomicU8 = AtomicU8::new(0);

/// Read the two loader-patched `avs_mutex_lock/unlock` pointers out of the
/// IAT slots the derivation published. `None` if either slot is unreadable
/// or holds null (fail-open: callers then skip the lock AND the work).
fn avs_mutex_fns() -> Option<(AvsMutexFn, AvsMutexFn)> {
    let sites = sites()?;
    match MUTEX_STATE.load(Ordering::Acquire) {
        2 => return None,
        1 => {}
        _ => {
            let ok = memory::is_readable(sites.mutex_lock_iat, 8)
                && memory::is_readable(sites.mutex_unlock_iat, 8)
                && unsafe {
                    !memory::read_ptr(sites.mutex_lock_iat).is_null()
                        && !memory::read_ptr(sites.mutex_unlock_iat).is_null()
                };
            if !ok {
                log_warn!(
                    "scene3d: avs mutex IAT slots unreadable/null -- locked engine walks disabled"
                );
            }
            MUTEX_STATE.store(if ok { 1 } else { 2 }, Ordering::Release);
            if !ok {
                return None;
            }
        }
    }
    // SAFETY: the slots were validated readable and non-null above; they are
    // gamemdx's own import thunks for the two libavs exports.
    unsafe {
        let lock = memory::read_ptr(sites.mutex_lock_iat);
        let unlock = memory::read_ptr(sites.mutex_unlock_iat);
        Some((
            std::mem::transmute::<*const u8, AvsMutexFn>(lock),
            std::mem::transmute::<*const u8, AvsMutexFn>(unlock),
        ))
    }
}

/// Run `f` under one of the engine's avs mutexes, replicating the exact
/// sequence `FUN_180024250` (SceneGraphManager) and `FUN_180203b60`
/// (ResourceManager) use:
///
/// ```text
/// if (*mutex_field > 0) avs_mutex_lock(*mutex_field);
/// (*depth_field)++;   … f …   (*depth_field)--;
/// if (*mutex_field > 0) avs_mutex_unlock(*mutex_field);
/// ```
///
/// `mutex_field` = the object's i32 mutex id (created at its ctor; `<= 0`
/// means "no lock" and the engine itself runs unlocked), `depth_field` = the
/// i32 right after it. Returns `None` without running `f` when the IAT slots
/// could not be validated (never runs an engine walk unprotected).
///
/// # Safety
/// `mutex_field`/`depth_field` must point at the live object's fields; the
/// caller must be on a thread where the engine itself takes this lock (the
/// game thread for both consumers here).
pub(crate) unsafe fn with_avs_mutex<R>(
    mutex_field: *const i32,
    depth_field: *mut i32,
    f: impl FnOnce() -> R,
) -> Option<R> {
    let (lock, unlock) = avs_mutex_fns()?;
    let id = mutex_field.read_volatile();
    if id > 0 {
        lock(id);
    }
    depth_field.write_volatile(depth_field.read_volatile() + 1);
    let r = f();
    depth_field.write_volatile(depth_field.read_volatile() - 1);
    if id > 0 {
        unlock(id);
    }
    Some(r)
}
