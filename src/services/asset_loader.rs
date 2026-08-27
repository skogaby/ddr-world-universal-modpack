//! Asset loader — on-demand customizer texture loading via the engine's
//! FileManager → ResourceManager pipeline.
//!
//! A thin wrapper around the game's own file/resource API, used by the WebUI
//! Options preview overlay to load a customizer asset `.arc` on demand, resolve
//! the inner PNG's GPU texture handle, and release it again.
//!
//! ## Load path (confirmed on cabinet — Step-1 probe #3)
//!
//! ```text
//! file_manager_load(singleton, "data/arc/custom/<cat>/<stem>.arc")   // async
//!    └─ engine unpacks the arc
//!    └─ PngFileCallback registers the inner PNG in the ResourceManager under
//!       its BARE FILENAME STEM (e.g. "appeal_board_0001")
//! [~43 frames later]
//! td = get_texture_data(get_texture_hash_value("appeal_board_0001"))   // gs::TextureData*
//! handle = *(u32*)(td + 0x04)                                          // bindable texture id
//! sprite.set_texture_id(handle)
//! file_manager_free(singleton, fm_handle)   // → OnDetach → ReleaseTextureData(stem)
//! ```
//!
//! This is the SAME pipeline `note_types_expansion` uses for its mine textures
//! (see `mods/note_types_expansion/texture_loader.rs` + `mine_render.rs`); this
//! service is the reusable, release-capable form of it. The originally-planned
//! `arc_load` path is NOT used — that loader only handles sound/shader arcs and
//! does not register loose PNGs in the ResourceManager (probe #1/#2 findings).
//!
//! ## Threading
//!
//! Every function here calls into the live engine and MUST run on the game's
//! render thread — callers route through `widget_renderer::run_on_render_thread`.
//! The service's own mutex is only held for the duration of a single call (never
//! across a `run_on_render_thread` schedule), so it composes safely with the
//! overlay state lock (CLAUDE.md rule 6).
//!
//! Loading is refcounted by the engine: loading an already-resident file bumps
//! its refcount instead of re-reading, so each successful `load` must be paired
//! with exactly one `release`.

use once_cell::sync::Lazy;
use std::ffi::CString;
use std::sync::Mutex;

use crate::core::memory;
use crate::core::signatures::SignatureStore;
use crate::{log_info, log_warn};

/// `agcs::FileManager::Load(this, path) -> i32 handle` (index; -1 on failure).
type FileLoadFn = unsafe extern "C" fn(this: *mut u8, path: *const i8) -> i32;
/// `agcs::FileManager::Free(this, index)` — enqueues the handle for release.
type FileFreeFn = unsafe extern "C" fn(this: *mut u8, index: i32);
/// `ResourceManager::GetTextureHashValue(name) -> u32` (static; lowercases,
/// strips underscores, FNV-hashes). No `this`.
type TextureHashFn = unsafe extern "C" fn(name: *const i8) -> u32;
/// `ResourceManager::GetTextureData(hash) -> gs::TextureData*` (null if the
/// hash isn't registered). No `this` — reads the RM singleton from a global.
type TextureDataFn = unsafe extern "C" fn(hash: u32) -> *mut u8;

/// Offset of the bindable GPU texture handle within `gs::TextureData`. Same
/// slot `note_types_expansion`'s mine render reads before emitting SetTexture.
const TEXTURE_DATA_HANDLE_OFFSET: usize = 0x04;

/// A loaded asset. Carries the RM texture-name hash (for re-lookup) and the
/// FileManager arc handle (so we can `Free` it, which the engine turns into a
/// `ReleaseTextureData(stem)` via the PNG callback's OnDetach).
///
/// Deliberately NOT `Copy`/`Clone`: each successful [`load`] must be paired
/// with exactly one [`release`], and `release` consumes the handle — so a
/// double-release is a compile error instead of an engine refcount underflow.
#[derive(Debug)]
pub struct AssetHandle {
    /// FNV hash of the bare texture stem, as computed by the engine's hasher.
    pub name_hash: u32,
    /// The FileManager file-table index returned by `file_manager_load`.
    pub fm_handle: i32,
}

/// A resolved, bindable texture. Write `handle` into a sprite's texture-id
/// field (`ImageWidget::set_texture_id`). (Native dimensions can be added here
/// later if overlay sizing needs them — see the layout note in `resolve`.)
#[derive(Clone, Copy, Debug)]
pub struct TextureHandle {
    /// The engine's bindable texture id (`gs::TextureData + 0x04`).
    pub handle: u32,
}

struct Inner {
    file_load: FileLoadFn,
    file_free: FileFreeFn,
    /// Address of the FileManager singleton *global pointer* (not the object).
    /// Dereferenced once per call so we never cache a stale/uninitialized ptr.
    file_manager_singleton: *const u8,
    texture_hash: TextureHashFn,
    texture_data: TextureDataFn,
}

// The raw pointer is a fixed global in the game's address space, valid for the
// process lifetime and only touched from the render thread (codebase norm).
unsafe impl Send for Inner {}

impl Inner {
    /// Dereference the singleton global to the live FileManager object, or
    /// `None` if the global (or the pointer within it) is still null.
    fn manager(&self) -> Option<*mut u8> {
        if self.file_manager_singleton.is_null() {
            return None;
        }
        let obj = unsafe { *(self.file_manager_singleton as *const *mut u8) };
        if obj.is_null() {
            None
        } else {
            Some(obj)
        }
    }
}

static LOADER: Lazy<Mutex<Option<Inner>>> = Lazy::new(|| Mutex::new(None));

/// Cache the FileManager / ResourceManager addresses. Call once during init.
/// Returns `false` (and disables the service) if any address is missing — the
/// overlay path then degrades to chrome-only (see `is_available`).
///
/// All four load/resolve signatures already exist for `note_types_expansion`;
/// `file_manager_free` is the release counterpart added for this service.
pub fn init(signatures: &SignatureStore) -> bool {
    let file_load = signatures.get_address("file_manager_load");
    let file_free = signatures.get_address("file_manager_free");
    let singleton = signatures.get_address("file_manager_singleton");
    let tex_hash = signatures.get_address("resource_manager_get_texture_hash_value");
    let tex_data = signatures.get_address("resource_manager_get_texture_data");

    match (file_load, file_free, singleton, tex_hash, tex_data) {
        (Some(fl), Some(ff), Some(sg), Some(th), Some(td)) => {
            let inner = Inner {
                file_load: unsafe { std::mem::transmute::<*const u8, FileLoadFn>(fl) },
                file_free: unsafe { std::mem::transmute::<*const u8, FileFreeFn>(ff) },
                file_manager_singleton: sg,
                texture_hash: unsafe { std::mem::transmute::<*const u8, TextureHashFn>(th) },
                texture_data: unsafe { std::mem::transmute::<*const u8, TextureDataFn>(td) },
            };
            match LOADER.lock() {
                Ok(mut g) => *g = Some(inner),
                Err(_) => {
                    log_warn!("AssetLoader: state mutex poisoned during init — disabled");
                    return false;
                }
            }
            log_info!(
                "AssetLoader: initialized (file_manager load/free + resource_manager hash/data resolved)"
            );
            true
        }
        _ => {
            log_warn!(
                "AssetLoader: required signatures missing (load={} free={} singleton={} hash={} data={}) — on-demand previews disabled",
                file_load.is_some(),
                file_free.is_some(),
                singleton.is_some(),
                tex_hash.is_some(),
                tex_data.is_some()
            );
            false
        }
    }
}

/// Whether the loader resolved its addresses and can service requests. Callers
/// check this before wiring the overlay path (graceful degradation, R-8).
pub fn is_available() -> bool {
    LOADER.lock().map(|g| g.is_some()).unwrap_or(false)
}

/// Compute the engine's texture-name hash for a bare stem. Returns `None` if
/// the service isn't available or the string can't be made a C string.
/// (Callers that only need the hash — e.g. to build an `AssetHandle` key
/// without loading — can use this; `load`/`resolve` hash internally.)
pub fn hash_name(tex_name: &str) -> Option<u32> {
    let guard = LOADER.lock().ok()?;
    let inner = guard.as_ref()?;
    let c_name = CString::new(tex_name).ok()?;
    Some(unsafe { (inner.texture_hash)(c_name.as_ptr()) })
}

/// Request-load the `.arc` at `arc_path`; the engine unpacks it and registers
/// the inner PNG under `tex_name` (its bare stem). Non-blocking — poll
/// [`resolve`] for readiness. Returns an [`AssetHandle`] to pass to [`release`],
/// or `None` if the service is unavailable or the load request failed.
///
/// MUST run on the game thread.
pub fn load(arc_path: &str, tex_name: &str) -> Option<AssetHandle> {
    let guard = LOADER.lock().ok()?;
    let inner = guard.as_ref()?;
    let manager = inner.manager()?;

    // Build BOTH C strings before calling into the engine: a successful
    // file_manager_load bumps an engine refcount that only `release` can
    // undo, so no fallible work may sit between the load call and returning
    // the handle (a late bail here would leak the load).
    let c_path = CString::new(arc_path).ok()?;
    let c_name = CString::new(tex_name).ok()?;

    let fm_handle = unsafe { (inner.file_load)(manager, c_path.as_ptr()) };
    if fm_handle < 0 {
        log_warn!(
            "AssetLoader: file_manager_load(\"{}\") returned {}",
            arc_path,
            fm_handle
        );
        return None;
    }

    let name_hash = unsafe { (inner.texture_hash)(c_name.as_ptr()) };
    Some(AssetHandle {
        name_hash,
        fm_handle,
    })
}

/// Resolve the bindable texture handle for a loaded asset by its bare stem, or
/// `None` if it isn't registered yet (still loading) or has been evicted.
///
/// MUST run on the game thread. Cheap enough to poll per-frame — do not log
/// here (callers log the Loading→Resolved transition once).
pub fn resolve(tex_name: &str) -> Option<TextureHandle> {
    let guard = LOADER.lock().ok()?;
    let inner = guard.as_ref()?;
    let c_name = CString::new(tex_name).ok()?;
    let hash = unsafe { (inner.texture_hash)(c_name.as_ptr()) };
    resolve_inner(inner, hash)
}

/// Resolve directly from a previously-computed hash (avoids re-hashing the
/// stem each poll). Same semantics as [`resolve`]. MUST run on the game thread.
pub fn resolve_hash(name_hash: u32) -> Option<TextureHandle> {
    let guard = LOADER.lock().ok()?;
    let inner = guard.as_ref()?;
    resolve_inner(inner, name_hash)
}

/// Shared body for `resolve`/`resolve_hash`: look up the TextureData by hash and
/// read the bindable handle at `+0x04`. Caller holds the loader guard.
fn resolve_inner(inner: &Inner, name_hash: u32) -> Option<TextureHandle> {
    let td = unsafe { (inner.texture_data)(name_hash) };
    if td.is_null() {
        return None;
    }
    // gs::TextureData + 0x04 = the engine's bindable texture id.
    let handle = unsafe { memory::read_u32(td.add(TEXTURE_DATA_HANDLE_OFFSET)) };
    Some(TextureHandle { handle })
}

/// Release a loaded asset: `FileManager::Free(fm_handle)`, which the engine
/// drains asynchronously and turns into `ReleaseTextureData(stem)` via the PNG
/// callback's OnDetach. Skips a bad (`< 0`) handle. Consumes the handle —
/// exactly-once release is enforced by the type system.
///
/// MUST run on the game thread. Never release a handle whose texture is still
/// bound to a *visible* overlay — swap/hide the sprite first (see the overlay's
/// release ordering).
pub fn release(handle: AssetHandle) {
    if handle.fm_handle < 0 {
        return;
    }
    let guard = match LOADER.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    let inner = match guard.as_ref() {
        Some(i) => i,
        None => return,
    };
    let manager = match inner.manager() {
        Some(m) => m,
        None => return,
    };
    unsafe { (inner.file_free)(manager, handle.fm_handle) };
}
