//! Mine texture loading via the engine's file pipeline.
//!
//! Loads mine PNG textures by calling the agcs::FileManager singleton's
//! file-load member (resolved via the `file_manager_load` signature).
//! The engine's PngFileCallback (RTTI: `.?AVPngFileCallback@@`) handles
//! `.png` extensions and registers the resulting gs::TextureData in the
//! resource system under the filename stem (e.g. `"note_types_mine00_l"`).
//!
//! Three size variants (s/m/l) exist to match the arrow-shape option's
//! shock-effect size mapping. The active variant is selected per-frame
//! based on the player's current arrow shape.
//!
//! Texture availability is checked lazily — if the PNG hasn't finished
//! loading yet (async worker thread), the mine render pass is skipped
//! that frame. No crash, no blocking.

use std::cell::Cell;
use std::ffi::CString;

use crate::{log_info, log_warn};

/// Function-pointer types for the engine's file/resource API.
type FileLoadFn = unsafe extern "C" fn(this: *mut u8, path: *const i8) -> i32;
type TextureHashFn = unsafe extern "C" fn(name: *const i8) -> u32;
type TextureDataFn = unsafe extern "C" fn(hash: u32) -> *mut u8;

/// Arrow-shape index (0–7) → shock-effect size variant.
/// The engine picks s/m/l shock-effect textures based on the player's
/// arrow shape option. This table mirrors that mapping, observable from
/// the gameplay actor's asset-loading path where it formats a size
/// suffix character ('s', 'm', 'l') from the shape index.
const SHOCK_SIZE_TABLE: [u8; 8] = [2, 2, 2, 2, 1, 0, 0, 2]; // 0=SMALL, 1=MEDIUM, 2=LARGE

/// Texture name stems for the three mine variants.
const MINE_TEX_STEMS: [&str; 3] = [
    "note_types_mine00_s",
    "note_types_mine00_m",
    "note_types_mine00_l",
];

/// Disk paths relative to the game's working directory.
const MINE_TEX_PATHS: [&str; 3] = [
    "./data_mods/note_types_expansion/tex/note_types_mine00_s.png",
    "./data_mods/note_types_expansion/tex/note_types_mine00_m.png",
    "./data_mods/note_types_expansion/tex/note_types_mine00_l.png",
];

/// Holds resolved function pointers and cached texture state.
pub struct MineTextureLoader {
    file_load: FileLoadFn,
    file_manager_singleton: *mut u8,
    texture_hash: TextureHashFn,
    texture_data: TextureDataFn,
    /// Cached hash values for the three mine texture stems (s, m, l).
    /// Initially all 0; populated lazily by `ensure_hashes` on the first
    /// per-frame texture lookup so the texture-hash global the function
    /// reads has time to initialize on the game side before we touch it.
    /// `Cell` to allow lazy mutation through `&self` (single-threaded
    /// access from the render thread).
    hashes: [Cell<u32>; 3],
    /// Set once the lazy hash compute has succeeded.
    hashes_computed: Cell<bool>,
    /// Whether the load request has been issued for each variant.
    load_requested: [bool; 3],
}

unsafe impl Send for MineTextureLoader {}

impl MineTextureLoader {
    /// Construct from resolved signature addresses. All pointers must be
    /// non-null and valid — the caller has already verified this via
    /// `required_signatures`.
    pub unsafe fn new(
        file_load_addr: *const u8,
        file_manager_singleton_addr: *const u8,
        texture_hash_addr: *const u8,
        texture_data_addr: *const u8,
    ) -> Self {
        let file_load: FileLoadFn = std::mem::transmute(file_load_addr);
        let texture_hash: TextureHashFn = std::mem::transmute(texture_hash_addr);
        let texture_data: TextureDataFn = std::mem::transmute(texture_data_addr);

        // The singleton address points to a global pointer — dereference
        // once to get the actual FileManager object pointer.
        let singleton_ptr = *(file_manager_singleton_addr as *const *mut u8);

        Self {
            file_load,
            file_manager_singleton: singleton_ptr,
            texture_hash,
            texture_data,
            hashes: [Cell::new(0), Cell::new(0), Cell::new(0)],
            hashes_computed: Cell::new(false),
            load_requested: [false; 3],
        }
    }

    /// Request the engine to load all three mine PNG variants. Non-blocking —
    /// the actual texture registration happens asynchronously on a worker
    /// thread. Call once at mod enable time.
    pub fn request_load_all(&mut self) {
        if self.file_manager_singleton.is_null() {
            log_warn!("MineTextureLoader: file manager singleton is null — skipping load");
            return;
        }

        for (i, path) in MINE_TEX_PATHS.iter().enumerate() {
            if self.load_requested[i] {
                continue;
            }
            let c_path = match CString::new(*path) {
                Ok(s) => s,
                Err(_) => {
                    log_warn!("MineTextureLoader: invalid path string for variant {}", i);
                    continue;
                }
            };
            let handle = unsafe { (self.file_load)(self.file_manager_singleton, c_path.as_ptr()) };
            if handle == -1 {
                log_warn!("MineTextureLoader: load returned -1 for {}", path);
            } else {
                log_info!(
                    "MineTextureLoader: requested load for {} (handle={})",
                    path,
                    handle,
                );
            }
            self.load_requested[i] = true;
        }
        // NOTE: hashes are computed lazily on first texture lookup —
        // see `ensure_hashes`. Calling `texture_hash` here at enable time
        // can crash because the game's resource-manager global it reads
        // may not be initialized yet during very-early DLL init.
    }

    /// Lazy-compute texture-name hashes on first use. Idempotent. Called
    /// from `get_texture_data_for_shape` so the hashes are populated
    /// when the render thread first asks for a mine texture — by which
    /// point the game's resource-manager state is fully initialized.
    fn ensure_hashes(&self) {
        if self.hashes_computed.get() {
            return;
        }
        for (i, stem) in MINE_TEX_STEMS.iter().enumerate() {
            let c_stem = match CString::new(*stem) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let h = unsafe { (self.texture_hash)(c_stem.as_ptr()) };
            self.hashes[i].set(h);
            log_info!("MineTextureLoader: hash for \"{}\" = {:#010x}", stem, h,);
        }
        self.hashes_computed.set(true);
    }

    /// Look up the gs::TextureData pointer for the mine variant matching
    /// the given arrow shape index (0–7). Returns null if the texture
    /// hasn't finished loading yet or the PNG is missing.
    pub fn get_texture_data_for_shape(&self, arrow_shape: u32) -> *mut u8 {
        self.ensure_hashes();
        let size_idx = if (arrow_shape as usize) < SHOCK_SIZE_TABLE.len() {
            SHOCK_SIZE_TABLE[arrow_shape as usize] as usize
        } else {
            2 // default to LARGE
        };
        unsafe { (self.texture_data)(self.hashes[size_idx].get()) }
    }

    /// Return the size-variant index (0=s, 1=m, 2=l) for a given arrow
    /// shape.
    pub fn size_index_for_shape(arrow_shape: u32) -> usize {
        if (arrow_shape as usize) < SHOCK_SIZE_TABLE.len() {
            SHOCK_SIZE_TABLE[arrow_shape as usize] as usize
        } else {
            2
        }
    }

    /// Check whether all three textures have been registered in the
    /// resource system (texture data lookup returns non-null for each).
    pub fn verify_all_loaded(&self) -> bool {
        self.ensure_hashes();
        let mut all_ok = true;
        for (i, stem) in MINE_TEX_STEMS.iter().enumerate() {
            let td = unsafe { (self.texture_data)(self.hashes[i].get()) };
            if td.is_null() {
                log_warn!("MineTextureLoader: \"{}\" not yet registered", stem);
                all_ok = false;
            } else {
                log_info!("MineTextureLoader: \"{}\" registered @ {:p}", stem, td);
            }
        }
        all_ok
    }
}
