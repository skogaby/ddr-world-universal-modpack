//! Folder Expansion Mod — Config-driven custom genre folders for DDR World.
//!
//! Creates custom FolderProperty objects in the game's folder carousel,
//! hooks the has-songs predicate to ensure custom folders are always visible,
//! and optionally patches difficulty restrictions on all folders.
//!
//! Config: `mod-config.json` `"folder_expansion"` key — see FolderConfig for schema.

use crate::core::memory;
use crate::core::scanner::decode_call_rel32;
use crate::core::{afp, arc, ifs};
use crate::mods::mod_trait::{Mod, ModContext};
use crate::services::afp_patcher;
use crate::services::avs_layeredfs::mod_paths;
use crate::{log_error, log_info, log_warn};
use retour::GenericDetour;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// FolderProperty layout — detected dynamically from folder_init at runtime.
// The struct layout differs between game versions, so nothing is hardcoded.
#[derive(Clone)]
struct FolderPropertyLayout {
    struct_size: usize,
    key_offset: usize,
    voice_key_offset: usize,
    filter_functor_slot: usize,
    mode_flag_offset: usize,
    functor_slot: usize,
    shared_ptr_move: *const u8,
    create_folder_data: *const u8, // FUN_1801448a0 equivalent — creates shared_ptr for [folder+0x1A8]
}

unsafe impl Send for FolderPropertyLayout {}
unsafe impl Sync for FolderPropertyLayout {}

static mut FOLDER_LAYOUT: Option<FolderPropertyLayout> = None;

fn get_layout() -> Option<&'static FolderPropertyLayout> {
    unsafe { (*std::ptr::addr_of!(FOLDER_LAYOUT)).as_ref() }
}

// Difficulty unlock offsets — detected from constructor, applied in hook
struct DifficultyOffsets {
    max_diff: usize,
    enable_flags: Option<usize>, // None if version doesn't have detectable enable flags
    enable_flags_count: usize,   // Number of enable flag bytes to write
    enable_value: u8, // Value to write: 1 for "enable" semantics, 0 for "restriction" semantics
}

unsafe impl Send for DifficultyOffsets {}
unsafe impl Sync for DifficultyOffsets {}

static mut DIFF_OFFSETS: Option<DifficultyOffsets> = None;

// Gameplay sequence object patch — the game allocates a fixed-size object with
// exactly N shared_ptr slots (one per non-ALL_MUSIC folder). Adding custom folders
// overflows this array. We patch the allocation size and zero the extra bytes.
static mut GAMEPLAY_OBJ_ORIGINAL_SIZE: u32 = 0;
static mut GAMEPLAY_OBJ_EXTRA_BYTES: u32 = 0;
static mut GAMEPLAY_OBJ_CTOR_HOOK: Option<GenericDetour<GameplayObjCtorFn>> = None;
type GameplayObjCtorFn = unsafe extern "C" fn(*mut u8) -> *mut u8;

unsafe extern "C" fn gameplay_obj_ctor_hook(this: *mut u8) -> *mut u8 {
    let result = (*std::ptr::addr_of!(GAMEPLAY_OBJ_CTOR_HOOK))
        .as_ref()
        .unwrap()
        .call(this);
    if !result.is_null() && GAMEPLAY_OBJ_EXTRA_BYTES > 0 {
        std::ptr::write_bytes(
            result.add(GAMEPLAY_OBJ_ORIGINAL_SIZE as usize),
            0,
            GAMEPLAY_OBJ_EXTRA_BYTES as usize,
        );
    }
    result
}

// SSO string layout
const STR_LENGTH: usize = 0x10;
const STR_CAPACITY: usize = 0x18;
const SSO_MAX_LEN: usize = 15;
const SSO_CAPACITY: u64 = 0x0F;

// Custom folder type IDs — use values outside the vanilla range (1-10, 99) but within
// the game's valid range. The info banner texture lookup uses type_id to construct
// the texture name, so each custom folder needs a unique type_id.
const CUSTOM_TYPE_ID_BASE: u32 = 0x10;

// ALL MUSIC folder type ID — used to detect insertion point
const ALL_MUSIC_TYPE_ID: u32 = 7;

// Dan Ranking / Dan Course folder type ID. This folder is built via a special
// path in folder_init (not the genre-folder path) and deliberately clears the
// per-difficulty flag cluster at +0x1fc..+0x202 down to {1,0,0,0,0,0,0}, unlike
// genre folders. The flag cluster doubles as Dan Ranking's view-axis / layout
// state, so blanket-writing the difficulty-unlock 1s across it re-enables a
// horizontal scroll axis that doesn't exist in Dan Ranking, letting the player
// scroll left/right to phantom items. Difficulty unlock is meaningless for Dan
// Ranking anyway (courses have fixed difficulties), so we skip it for this folder.
const DAN_RANK_TYPE_ID: u32 = 10;

// Functor output buffer size (derived from stack layout: buffers are 0x40 apart)
const FUNCTOR_BUF_SIZE: usize = 0x40;

// Functor bit_index offset (for Task 4's has-songs hook)
const FUNCTOR_BIT_INDEX: usize = 0x08;

// ARC containing the folder IFS (relative to game directory)
const FOLDER_ARC_PATH: &str = "data/arc/bm2d/select_music_folder_v3.arc";
// IFS filename inside the ARC
const FOLDER_IFS_NAME: &str = "select_music_folder_v3.ifs";
// ARC containing language-specific textures (info banner, subtitle strip)
const LANG_ENG_ARC_PATH: &str = "data/arc/bm2d/select_music_folder_lang_eng_v3.arc";
const LANG_ENG_IFS_NAME: &str = "select_music_folder_lang_eng_v3.ifs";
// Source folder key to clone geo files from
const SOURCE_KEY: &str = "firststep";
// The 6 shape IDs containing folder-specific texture labels
const FOLDER_SHAPE_IDS: &[u32] = &[41, 44, 47, 50, 53, 56];
// The 12 shared shape IDs (no folder-specific labels, copied as-is)
const SHARED_SHAPE_IDS: &[u32] = &[5, 8, 9, 12, 15, 18, 21, 29, 30, 59, 62, 63];
// Mod folder for generated geo files
const GEO_MOD_FOLDER: &str = "./data_mods/custom_folders";
// IFS mod path (used for LayeredFS file lookup)
const IFS_MOD_PATH: &str = "select_music_folder_v3_ifs";
const LANG_ENG_IFS_MOD_PATH: &str = "select_music_folder_lang_eng_v3_ifs";

// ── Config types ────────────────────────────────────────────────────

static mut FOLDER_CONFIG: Option<FolderConfig> = None;

#[derive(Deserialize, Serialize, Clone)]
pub struct FolderConfig {
    pub custom_folders: Vec<CustomFolderEntry>,
    #[serde(default)]
    pub hide_difficulty_pane: bool,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct CustomFolderEntry {
    pub bit_index: u32,
    pub key: String,
    #[serde(default)]
    pub voice_key: String,
}

pub fn get_config() -> Option<&'static FolderConfig> {
    unsafe { (*std::ptr::addr_of!(FOLDER_CONFIG)).as_ref() }
}

// ── Asset-generation cache ──────────────────────────────────────────
//
// The mod's `enable()` regenerates per-folder geo, AFP, BSI, and atlas
// files under `./data_mods/custom_folders/`. Those outputs are pure
// functions of (folder config, source ARC bytes), so on a warm boot we
// can skip regeneration if neither has changed.
//
// `.cache_meta.json` lives alongside the generated artifacts and stores
// a hash of the canonical config plus the source ARC's mtime. On
// `enable()`, we compute the current key, compare to the stored one, and
// skip the generation step on a HIT. On any failure (missing file,
// parse error, schema mismatch) we treat as a MISS and regenerate.

const CACHE_META_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct CacheMeta {
    version: u32,
    config_hash: String,
    source_arc_mtime: u64,
}

fn cache_meta_path() -> PathBuf {
    PathBuf::from(GEO_MOD_FOLDER).join(".cache_meta.json")
}

fn compute_cache_key(config: &FolderConfig) -> Option<CacheMeta> {
    // serde_json::to_vec produces deterministic bytes for FolderConfig
    // because the struct fields are typed (no HashMap) and the only
    // collection is `Vec<CustomFolderEntry>`, which preserves order.
    let config_bytes = serde_json::to_vec(config).ok()?;
    let digest = md5::compute(&config_bytes);
    let config_hash = format!("{:x}", digest);

    let arc_mtime = std::fs::metadata(FOLDER_ARC_PATH)
        .ok()?
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();

    Some(CacheMeta {
        version: CACHE_META_VERSION,
        config_hash,
        source_arc_mtime: arc_mtime,
    })
}

fn cache_is_valid(want: &CacheMeta) -> bool {
    let bytes = match std::fs::read(cache_meta_path()) {
        Ok(b) => b,
        Err(_) => return false,
    };
    let got: CacheMeta = match serde_json::from_slice(&bytes) {
        Ok(m) => m,
        Err(_) => return false,
    };
    got.version == want.version
        && got.config_hash == want.config_hash
        && got.source_arc_mtime == want.source_arc_mtime
}

fn write_cache_meta(meta: &CacheMeta) {
    let path = cache_meta_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match serde_json::to_vec_pretty(meta) {
        Ok(bytes) => {
            if let Err(e) = std::fs::write(&path, bytes) {
                log_warn!("FolderExpansion: failed to write cache meta: {}", e);
            }
        }
        Err(e) => log_warn!("FolderExpansion: failed to serialize cache meta: {}", e),
    }
}

// ── Function pointer types ──────────────────────────────────────────

/// FolderProperty constructor: __fastcall(RCX=this) → *mut u8
type FolderPropertyCtorFn = unsafe extern "C" fn(*mut u8) -> *mut u8;
/// Functor constructor: __fastcall(RCX=out_buf, EDX=bit_index) → *mut u8
type FolderFunctorCtorFn = unsafe extern "C" fn(*mut u8, u32) -> *mut u8;
/// Shared_ptr move: __fastcall(RCX=dest_slot, RDX=source) → void
type SharedPtrMoveFn = unsafe extern "C" fn(*mut u8, *mut u8);
/// Store shared_ptr at +0x1a8: __fastcall(RCX=folder_property, RDX=shared_ptr_buf) → *mut u8
type FolderStorePtrFn = unsafe extern "C" fn(*mut u8, *mut u8) -> *mut u8;
/// Create folder data shared_ptr: __fastcall(RCX=out_buf) → *mut u8
type CreateFolderDataFn = unsafe extern "C" fn(*mut u8) -> *mut u8;
/// Register folder: __fastcall(RCX=context, RDX=folder_property)
type FolderRegisterFn = unsafe extern "C" fn(*mut u8, *mut u8);
/// Has-songs predicate: __fastcall(RCX=functor) → bool
type FolderHasSongsFn = unsafe extern "C" fn(*const u8) -> bool;
/// Game's CRT malloc: __fastcall(RCX=size) → *mut u8
type GameMallocFn = unsafe extern "C" fn(usize) -> *mut u8;

// ── Hook statics ────────────────────────────────────────────────────

static mut REGISTER_HOOK: Option<GenericDetour<FolderRegisterFn>> = None;
static mut HAS_SONGS_HOOK: Option<GenericDetour<FolderHasSongsFn>> = None;

// Function pointers accessible from hook callback
static mut FN_PROPERTY_CTOR: Option<FolderPropertyCtorFn> = None;
static mut FN_FUNCTOR_CTOR: Option<FolderFunctorCtorFn> = None;
static mut FN_FILTER_FUNCTOR_CTOR: Option<FolderFunctorCtorFn> = None;
static mut FN_SHARED_PTR_MOVE: Option<SharedPtrMoveFn> = None;
static mut FN_STORE_PTR: Option<FolderStorePtrFn> = None;
static mut FN_GAME_MALLOC: Option<GameMallocFn> = None;
static mut CUSTOM_FOLDERS_CREATED: bool = false;

// ── SSO string helper ───────────────────────────────────────────────

unsafe fn write_sso_string(base: *mut u8, s: &str) {
    let bytes = s.as_bytes();
    let len = bytes.len().min(SSO_MAX_LEN);
    for i in 0..16 {
        *base.add(i) = 0;
    }
    for (i, &b) in bytes.iter().take(len).enumerate() {
        *base.add(i) = b;
    }
    *(base.add(STR_LENGTH) as *mut u64) = len as u64;
    *(base.add(STR_CAPACITY) as *mut u64) = SSO_CAPACITY;
}

// ── folder_register hook ────────────────────────────────────────────

/// Hook callback for folder_register. Intercepts ALL MUSIC registration
/// to inject custom folders before it, and unlocks all difficulty levels
/// on every folder.
unsafe extern "C" fn folder_register_hook(context: *mut u8, folder_property: *mut u8) {
    let type_id = *(folder_property as *const u32);

    // Reset the guard when a new folder_init pass begins (non-ALL_MUSIC folder
    // after the flag was set means the game is re-registering all folders).
    if type_id != ALL_MUSIC_TYPE_ID && CUSTOM_FOLDERS_CREATED {
        CUSTOM_FOLDERS_CREATED = false;
    }

    if type_id == ALL_MUSIC_TYPE_ID && !CUSTOM_FOLDERS_CREATED {
        CUSTOM_FOLDERS_CREATED = true;

        if let (Some(config), Some(layout), Some(game_malloc)) =
            (get_config(), get_layout(), FN_GAME_MALLOC)
        {
            let property_ctor = FN_PROPERTY_CTOR.unwrap();
            let functor_ctor = FN_FUNCTOR_CTOR.unwrap();
            let filter_functor_ctor = FN_FILTER_FUNCTOR_CTOR.unwrap();
            let shared_ptr_move = FN_SHARED_PTR_MOVE.unwrap();
            let store_ptr = FN_STORE_PTR.unwrap();

            for (i, entry) in config.custom_folders.iter().enumerate() {
                let custom_type_id = CUSTOM_TYPE_ID_BASE + i as u32;
                log_info!(
                    "FolderExpansion: creating folder '{}' (type_id=0x{:X}, bit_index={})",
                    entry.key,
                    custom_type_id,
                    entry.bit_index
                );

                // 1. Create functor (property bit check)
                let functor_buf = memory::alloc_zeroed(FUNCTOR_BUF_SIZE);
                let functor_ret = functor_ctor(functor_buf, entry.bit_index);

                // 2. Create filter functor
                let filter_buf = memory::alloc_zeroed(FUNCTOR_BUF_SIZE);
                let filter_ret = filter_functor_ctor(filter_buf, entry.bit_index);

                // 3. Allocate FolderProperty using the GAME'S allocator.
                // folder_register creates a shared_ptr whose control block destructor
                // calls the game's free() on this pointer. Using VirtualAlloc here
                // causes a heap mismatch crash in RtlFreeHeap.
                let folder = game_malloc(layout.struct_size);
                if folder.is_null() {
                    log_error!("FolderExpansion: game_malloc failed for FolderProperty");
                    continue;
                }
                std::ptr::write_bytes(folder, 0, layout.struct_size);
                property_ctor(folder);

                // 4. Set fields (max_difficulty left at constructor default of 4)
                *(folder as *mut u32) = custom_type_id;
                write_sso_string(folder.add(layout.key_offset), &entry.key);
                write_sso_string(folder.add(layout.voice_key_offset), &entry.voice_key);

                // 5. Wire filter functor via shared_ptr move
                shared_ptr_move(folder.add(layout.filter_functor_slot), filter_ret);

                // 6. Set mode_flag = 3 (genre folder)
                *(folder.add(layout.mode_flag_offset) as *mut u32) = 3;

                // 7. Wire property functor via shared_ptr move
                shared_ptr_move(folder.add(layout.functor_slot), functor_ret);

                // 8. Create folder data shared_ptr and store it
                let shared_ptr_buf = memory::alloc_zeroed(16);
                if !layout.create_folder_data.is_null() {
                    let create_fn: CreateFolderDataFn =
                        std::mem::transmute(layout.create_folder_data);
                    create_fn(shared_ptr_buf);
                }
                let result = store_ptr(folder, shared_ptr_buf);

                // 9. Register the custom folder
                if let Some(ref hook) = REGISTER_HOOK {
                    hook.call(context, result);
                }

                log_info!("FolderExpansion: registered folder '{}'", entry.key);
            }
        }
    }

    // Unlock all difficulty levels on every folder — except Dan Ranking, whose
    // flag cluster doubles as view-axis state. Writing the unlock 1s into it
    // re-enables a phantom horizontal scroll axis (see DAN_RANK_TYPE_ID).
    if type_id == DAN_RANK_TYPE_ID {
        if let Some(ref hook) = REGISTER_HOOK {
            hook.call(context, folder_property);
        }
        return;
    }
    if let Some(ref offsets) = *std::ptr::addr_of!(DIFF_OFFSETS) {
        let type_id = *(folder_property as *const u32);
        let old_max = *(folder_property.add(offsets.max_diff) as *const u32);
        *(folder_property.add(offsets.max_diff) as *mut u32) = 4;
        if let Some(en_off) = offsets.enable_flags {
            let old_en = *folder_property.add(en_off);
            for i in 0..offsets.enable_flags_count {
                *folder_property.add(en_off + i) = offsets.enable_value;
            }
            log_info!(
                "FolderExpansion: unlock type_id={} max_diff {}→4 enable[0] {}→{} (ptr={:p})",
                type_id,
                old_max,
                old_en,
                offsets.enable_value,
                folder_property
            );
        } else {
            log_info!(
                "FolderExpansion: unlock type_id={} max_diff {}→4 (no enable flags) (ptr={:p})",
                type_id,
                old_max,
                folder_property
            );
        }
    }

    if let Some(ref hook) = REGISTER_HOOK {
        hook.call(context, folder_property);
    }
}

// ── folder_has_songs hook ────────────────────────────────────────────

/// Hook callback for folder_has_songs. The native function returns false for
/// all genre folders when a player profile sets the internal mode flag to 1.
/// We override this by returning true for all folders with valid bit indices
/// (native genre folders 0-9 and configured custom folders).
unsafe extern "C" fn folder_has_songs_hook(functor: *const u8) -> bool {
    if let Some(ref hook) = HAS_SONGS_HOOK {
        if hook.call(functor) {
            return true;
        }
    }
    let bit_index = *(functor.add(FUNCTOR_BIT_INDEX) as *const u32);
    // Native genre folders (bit_index 0-9) should always be visible
    if bit_index <= 9 {
        return true;
    }
    // Custom folders with configured bit_index should also be visible
    if let Some(config) = get_config() {
        return config
            .custom_folders
            .iter()
            .any(|f| f.bit_index == bit_index);
    }
    false
}

// ── Geo + AFP file generation ────────────────────────────────────────

/// Generate patched geo and AFP files for all custom folders by extracting
/// firststep's data from the game's ARC/IFS and replacing name strings.
fn generate_custom_assets(config: &FolderConfig) {
    let arc_data = match std::fs::read(FOLDER_ARC_PATH) {
        Ok(d) => d,
        Err(e) => {
            log_warn!("FolderExpansion: can't read {}: {}", FOLDER_ARC_PATH, e);
            return;
        }
    };

    let entries = match arc::parse(&arc_data) {
        Some(e) => e,
        None => {
            log_warn!("FolderExpansion: failed to parse ARC");
            return;
        }
    };

    let ifs_entry = match entries.iter().find(|e| e.path.ends_with(FOLDER_IFS_NAME)) {
        Some(e) => e,
        None => {
            log_warn!("FolderExpansion: {} not found in ARC", FOLDER_IFS_NAME);
            return;
        }
    };

    let ifs_data = match arc::extract(&arc_data, ifs_entry) {
        Some(d) => d,
        None => {
            log_warn!("FolderExpansion: failed to extract IFS from ARC");
            return;
        }
    };
    log_info!("FolderExpansion: extracted IFS ({} bytes)", ifs_data.len());

    // Build list of all firststep geo file names we need
    let all_shape_ids: Vec<u32> = FOLDER_SHAPE_IDS
        .iter()
        .chain(SHARED_SHAPE_IDS.iter())
        .copied()
        .collect();
    let source_names: Vec<String> = all_shape_ids
        .iter()
        .map(|id| format!("folder_{}_shape{}", SOURCE_KEY, id))
        .collect();

    let extracted = ifs::extract_files(&ifs_data, "geo", &source_names);
    if extracted.is_empty() {
        log_warn!("FolderExpansion: no geo files extracted from IFS");
        return;
    }
    log_info!(
        "FolderExpansion: extracted {} geo files from IFS",
        extracted.len()
    );

    // Build source name → data map
    let source_map: std::collections::HashMap<String, Vec<u8>> = extracted.into_iter().collect();

    for entry in &config.custom_folders {
        let geo_dir = format!("{}/{}/geo", GEO_MOD_FOLDER, IFS_MOD_PATH);
        let _ = std::fs::create_dir_all(&geo_dir);

        let mut written = 0;
        for &shape_id in &all_shape_ids {
            let src_name = format!("folder_{}_shape{}", SOURCE_KEY, shape_id);
            let dst_name = format!("folder_{}_shape{}", entry.key, shape_id);

            let src_data = match source_map.get(&src_name) {
                Some(d) => d,
                None => continue,
            };

            // Patch folder-specific shapes; copy shared shapes as-is
            let output = if FOLDER_SHAPE_IDS.contains(&shape_id) {
                patch_ge2d_labels(src_data, &entry.key).unwrap_or_else(|| src_data.clone())
            } else {
                src_data.clone()
            };

            let dst_path = format!("{}/{}", geo_dir, dst_name);
            if let Err(e) = std::fs::write(&dst_path, &output) {
                log_warn!("FolderExpansion: failed to write {}: {}", dst_path, e);
                continue;
            }
            written += 1;
        }
        log_info!(
            "FolderExpansion: generated {} geo files for '{}'",
            written,
            entry.key
        );
    }

    // Extract AFP + BSI for firststep, patch exported name for each custom folder
    let afp_name = format!("folder_{}", SOURCE_KEY);
    let afp_files = ifs::extract_files(&ifs_data, "afp", std::slice::from_ref(&afp_name));
    let bsi_files = ifs::extract_files(&ifs_data, "afp/bsi", std::slice::from_ref(&afp_name));

    let afp_src = afp_files.iter().find(|(n, _)| n == &afp_name);
    let bsi_src = bsi_files.iter().find(|(n, _)| n == &afp_name);

    if let (Some((_, afp_data)), Some((_, bsi_data))) = (afp_src, bsi_src) {
        for entry in &config.custom_folders {
            let new_name = format!("folder_{}", entry.key);

            let patched = match afp::patch_exported_name(afp_data, bsi_data, &new_name) {
                Some(p) => p,
                None => {
                    log_warn!("FolderExpansion: AFP name patch failed for '{}'", entry.key);
                    continue;
                }
            };

            let afp_dir = format!("{}/{}/afp", GEO_MOD_FOLDER, IFS_MOD_PATH);
            let bsi_dir = format!("{}/{}/afp/bsi", GEO_MOD_FOLDER, IFS_MOD_PATH);
            let _ = std::fs::create_dir_all(&afp_dir);
            let _ = std::fs::create_dir_all(&bsi_dir);

            let _ = std::fs::write(format!("{}/{}", afp_dir, new_name), &patched);
            let _ = std::fs::write(format!("{}/{}", bsi_dir, new_name), bsi_data);
            log_info!("FolderExpansion: generated AFP + BSI for '{}'", entry.key);
        }

        // Generate afplist.merged.xml so the game loads our custom AFP/geo entries
        generate_afplist_merged(config);
    } else {
        log_warn!(
            "FolderExpansion: could not extract AFP/BSI for '{}'",
            afp_name
        );
    }

    // Extract texturelist.xml (kbin binary) and generate cloned atlas textures
    if let Some(xml) = crate::services::avs_layeredfs::atlas_cloner::load_stock_texturelist(
        FOLDER_ARC_PATH,
        FOLDER_IFS_NAME,
    ) {
        generate_cloned_atlases(config, &xml, IFS_MOD_PATH);
    }

    // Extract lang_eng IFS and generate cloned atlases for info/subtitle textures
    if let Some(xml) = crate::services::avs_layeredfs::atlas_cloner::load_stock_texturelist(
        LANG_ENG_ARC_PATH,
        LANG_ENG_IFS_NAME,
    ) {
        generate_cloned_atlases(config, &xml, LANG_ENG_IFS_MOD_PATH);
    }
}

/// Generate afplist.merged.xml that injects custom folder AFP entries into the
/// game's afplist.xml via LayeredFS XML merging. This causes the AFP runtime to
/// load our custom AFP, BSI, and geo files at IFS mount time.
fn generate_afplist_merged(config: &FolderConfig) {
    let all_shape_ids: Vec<u32> = FOLDER_SHAPE_IDS
        .iter()
        .chain(SHARED_SHAPE_IDS.iter())
        .copied()
        .collect();
    let geo_text: String = all_shape_ids
        .iter()
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join(" ");

    let mut xml = String::from("<afplist>\n");
    for entry in &config.custom_folders {
        xml.push_str(&format!(
            "  <afp name=\"folder_{}\">\n    <geo __type=\"u16\" __count=\"{}\">{}</geo>\n  </afp>\n",
            entry.key, all_shape_ids.len(), geo_text
        ));
    }
    xml.push_str("</afplist>\n");

    let merged_path = format!("{}/{}/afp/afplist.merged.xml", GEO_MOD_FOLDER, IFS_MOD_PATH);
    let afp_dir = format!("{}/{}/afp", GEO_MOD_FOLDER, IFS_MOD_PATH);
    let _ = std::fs::create_dir_all(&afp_dir);
    if let Err(e) = std::fs::write(&merged_path, &xml) {
        log_warn!("FolderExpansion: failed to write afplist.merged.xml: {}", e);
    } else {
        log_info!("FolderExpansion: generated afplist.merged.xml");
    }
}

/// Build per-folder atlases (each folder gets its own atlas with textures
/// at the donor's original UV positions so GEO files work), then write a
/// single unified texturelist.merged.xml containing all folders' entries.
fn generate_cloned_atlases(config: &FolderConfig, texlist_xml: &str, ifs_mod_path: &str) {
    use crate::services::avs_layeredfs::atlas_cloner::{
        generate_cloned_atlases_xml, write_merged_texturelist, NewTextureSpec,
    };

    let source_prefix = format!("_{}", SOURCE_KEY);
    let donor_names: Vec<String> = extract_donor_image_names(texlist_xml, &source_prefix);
    if donor_names.is_empty() {
        log_warn!(
            "FolderExpansion: no source textures found in texturelist for key '{}'",
            SOURCE_KEY
        );
        return;
    }
    log_info!(
        "FolderExpansion: found {} donor texture entries",
        donor_names.len()
    );

    let tex_dir = format!("{}/{}/tex", GEO_MOD_FOLDER, ifs_mod_path);
    let mut combined_xml = String::new();

    for folder in &config.custom_folders {
        let pairs: Vec<(String, String)> = donor_names
            .iter()
            .map(|donor| {
                let new_name = donor.replace(SOURCE_KEY, &folder.key);
                let png_path = format!("{}/{}.png", tex_dir, new_name);
                (new_name, png_path)
            })
            .collect();
        let specs: Vec<NewTextureSpec> = pairs
            .iter()
            .zip(donor_names.iter())
            .map(|((new_name, png_path), donor)| NewTextureSpec {
                new_name,
                donor_name: donor,
                png_path,
            })
            .collect();

        let prefix = format!("cfolder_{}", folder.key);
        if let Some(xml) = generate_cloned_atlases_xml(
            texlist_xml,
            ifs_mod_path,
            "./data_mods/_cache",
            &prefix,
            &specs,
        ) {
            combined_xml.push_str(&xml);
        }
    }

    if !combined_xml.is_empty() {
        write_merged_texturelist(ifs_mod_path, GEO_MOD_FOLDER, &combined_xml);
    }
}

/// Return every `<image name>` in `xml` whose name contains `substring`,
/// in document order.
fn extract_donor_image_names(xml: &str, substring: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut pos = 0;
    while let Some(start) = xml[pos..].find("<image ") {
        let s = pos + start;
        let e = match xml[s..].find('>') {
            Some(i) => s + i + 1,
            None => break,
        };
        let tag = &xml[s..e];
        if let Some(name) = extract_xml_attr(tag, "name") {
            if name.contains(substring) {
                out.push(name.to_string());
            }
        }
        pos = e;
    }
    out
}

fn extract_xml_attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let search = format!("{}=\"", name);
    let pos = tag.find(&search)?;
    let start = pos + search.len();
    let end = tag[start..].find('"')? + start;
    Some(&tag[start..end])
}

/// Patch an AFP template to have an empty root frame, making it load but render nothing.
/// Receives already-descrambled AFP data from the AFP patcher hook.
fn patch_empty_root_frame(data: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    if data.len() < 56 {
        return None;
    }

    let tags_offset = u32::from_le_bytes(data[36..40].try_into().ok()?) as usize;
    let tags_frame_off =
        u32::from_le_bytes(data[tags_offset + 16..tags_offset + 20].try_into().ok()?) as usize;
    let abs_frame_off = tags_offset + tags_frame_off;
    if abs_frame_off + 4 > data.len() {
        return None;
    }

    let mut patched = data.to_vec();
    let frame0_info =
        u32::from_le_bytes(patched[abs_frame_off..abs_frame_off + 4].try_into().ok()?);
    let frame0_start = frame0_info & 0xFFFFF;
    // Set count to 0, keep start — frame exists but places nothing
    patched[abs_frame_off..abs_frame_off + 4].copy_from_slice(&frame0_start.to_le_bytes());

    Some((patched, vec![0u8, 0u8]))
}

/// Patch GE2D label strings: replace "firststep" with the custom key in texture names.
///
/// Thin wrapper over the promoted [`crate::core::geo::rewrite_labels`]
/// (this used to be the rewriter's home). Same contract as before: `None`
/// when the file is structurally unrecognized or no label contained the
/// source key (callers copy the donor bytes verbatim). Equal/shorter keys
/// produce byte-identical output to the original in-place implementation;
/// keys LONGER than "firststep" — which the old code silently truncated —
/// now rebuild correctly via the promoted helper's append path.
fn patch_ge2d_labels(data: &[u8], custom_key: &str) -> Option<Vec<u8>> {
    crate::core::geo::rewrite_labels(data, |label| {
        if !label.contains(SOURCE_KEY) {
            return None;
        }
        let new_label = label.replace(SOURCE_KEY, custom_key);
        log_info!("FolderExpansion: GE2D label '{}' → '{}'", label, new_label);
        Some(new_label)
    })
}

// ── Mod implementation ──────────────────────────────────────────────

pub struct FolderExpansionMod {
    // Resolved function addresses
    folder_init_addr: *const u8,
    folder_init_size: usize,
    property_ctor: *const u8,
    functor_ctor: *const u8,
    filter_functor_ctor: *const u8,
    store_ptr: *const u8,
    register_fn: *const u8,
    has_songs_fn: *const u8,
    game_malloc: *const u8,
    // Gameplay object patch addresses (for expanding the fixed-size shared_ptr array)
    gameplay_obj_alloc_size: *const u8, // points to the imm32 in MOV ECX,<size>
    gameplay_obj_ctor: *const u8,       // constructor function address
}

unsafe impl Send for FolderExpansionMod {}

impl FolderExpansionMod {
    pub fn new() -> Self {
        Self {
            folder_init_addr: std::ptr::null(),
            folder_init_size: 0,
            property_ctor: std::ptr::null(),
            functor_ctor: std::ptr::null(),
            filter_functor_ctor: std::ptr::null(),
            store_ptr: std::ptr::null(),
            register_fn: std::ptr::null(),
            has_songs_fn: std::ptr::null(),
            game_malloc: std::ptr::null(),
            gameplay_obj_alloc_size: std::ptr::null(),
            gameplay_obj_ctor: std::ptr::null(),
        }
    }

    fn load_config() -> Option<FolderConfig> {
        match super::config::get() {
            Some(cfg) => match cfg.folder_expansion.clone() {
                Some(config) => {
                    if let Some(err) = Self::validate_config(&config) {
                        log_warn!("FolderExpansion: config validation failed — {}", err);
                        return None;
                    }
                    log_info!(
                        "FolderExpansion: loaded {} custom folder(s)",
                        config.custom_folders.len()
                    );
                    Some(config)
                }
                None => {
                    log_warn!("FolderExpansion: no folder_expansion config — mod disabled");
                    None
                }
            },
            None => {
                log_warn!("FolderExpansion: config store not available — mod disabled");
                None
            }
        }
    }

    fn validate_config(config: &FolderConfig) -> Option<String> {
        for (i, entry) in config.custom_folders.iter().enumerate() {
            if entry.key.len() > SSO_MAX_LEN {
                return Some(format!(
                    "entry[{}].key '{}' exceeds {} chars",
                    i, entry.key, SSO_MAX_LEN
                ));
            }
            if entry.voice_key.len() > SSO_MAX_LEN {
                return Some(format!(
                    "entry[{}].voice_key exceeds {} chars",
                    i, SSO_MAX_LEN
                ));
            }
            if entry.bit_index > 31 {
                return Some(format!("entry[{}].bit_index {} > 31", i, entry.bit_index));
            }
        }
        let mut seen = std::collections::HashSet::new();
        for (i, entry) in config.custom_folders.iter().enumerate() {
            if !seen.insert(entry.bit_index) {
                return Some(format!(
                    "entry[{}] duplicate bit_index {}",
                    i, entry.bit_index
                ));
            }
        }
        None
    }

    /// Detect difficulty-related field offsets from the constructor.
    /// These offsets are used by the folder_register hook to overwrite
    /// restrictive values after each folder is registered.
    fn detect_difficulty_offsets(&self) {
        if self.property_ctor.is_null() {
            return;
        }

        let ctor_bytes = unsafe { std::slice::from_raw_parts(self.property_ctor, 0x120) };

        // max_difficulty: C7 8x [disp32] 04 00 00 00 (MOV dword [Rxx+disp32], 4)
        let mut max_diff: Option<usize> = None;
        let mut max_diff_pos: usize = 0;
        for i in 0..ctor_bytes.len().saturating_sub(10) {
            if ctor_bytes[i] == 0xC7
                && (ctor_bytes[i + 1] & 0xC0) == 0x80
                && ctor_bytes[i + 4] == 0x00
                && ctor_bytes[i + 5] == 0x00
                && ctor_bytes[i + 6] == 0x04
                && ctor_bytes[i + 7] == 0x00
                && ctor_bytes[i + 8] == 0x00
                && ctor_bytes[i + 9] == 0x00
            {
                max_diff = Some(u32::from_le_bytes([
                    ctor_bytes[i + 2],
                    ctor_bytes[i + 3],
                    ctor_bytes[i + 4],
                    ctor_bytes[i + 5],
                ]) as usize);
                max_diff_pos = i;
                break;
            }
        }

        let max_diff = match max_diff {
            Some(o) => o,
            None => {
                log_warn!("FolderExpansion: could not detect max_difficulty offset");
                return;
            }
        };
        log_info!(
            "FolderExpansion: detected max_difficulty offset +0x{:X}",
            max_diff
        );

        // Detect enable flags. Two known patterns:
        //
        // 20260324: MOV dword [Rxx+X], 0x01010101 (flags default to enabled)
        //   C7 8x [disp32] 01 01 01 01 → followed by MOV word + MOV byte = 7 bytes total
        //
        // 20250805: MOV word [Rxx+X], 0; MOV byte [Rxx+Y], 0 (flags default to disabled)
        //   66 89 Bx [disp32] 40 88 Bx [disp32] → right after max_difficulty MOV
        //   Enable flags at X, count = Y - X + 1

        let mut enable_flags: Option<usize> = None;
        let mut enable_count: usize = 7; // default for 20260324
        let mut enable_value: u8 = 1; // default: 1 = enabled (20260324 semantics)

        // Strategy 1: Look for MOV dword [Rxx+X], 0x01010101 (20260324 pattern)
        // Flags default to enabled; folder_init clears them for restricted folders.
        // Unlock = write 1s.
        for i in 0..ctor_bytes.len().saturating_sub(10) {
            if ctor_bytes[i] == 0xC7
                && (ctor_bytes[i + 1] & 0xC0) == 0x80
                && ctor_bytes[i + 6] == 0x01
                && ctor_bytes[i + 7] == 0x01
                && ctor_bytes[i + 8] == 0x01
                && ctor_bytes[i + 9] == 0x01
            {
                enable_flags = Some(u32::from_le_bytes([
                    ctor_bytes[i + 2],
                    ctor_bytes[i + 3],
                    ctor_bytes[i + 4],
                    ctor_bytes[i + 5],
                ]) as usize);
                enable_count = 7;
                enable_value = 1;
                break;
            }
        }

        // Strategy 2: Look for MOV word [Rxx+disp32], reg16 right after max_difficulty (20250805 pattern)
        // These are restriction flags (1=restricted); folder_init sets them for restricted folders.
        // Unlock = write 0s.
        if enable_flags.is_none() {
            let search_start = max_diff_pos + 10; // skip past the max_diff MOV
            for i in
                search_start..std::cmp::min(search_start + 16, ctor_bytes.len().saturating_sub(7))
            {
                if ctor_bytes[i] == 0x66
                    && ctor_bytes[i + 1] == 0x89
                    && (ctor_bytes[i + 2] & 0xC0) == 0x80
                {
                    let word_off = u32::from_le_bytes([
                        ctor_bytes[i + 3],
                        ctor_bytes[i + 4],
                        ctor_bytes[i + 5],
                        ctor_bytes[i + 6],
                    ]) as usize;

                    // Look for the following MOV byte [Rxx+disp32], reg8
                    let after_word = i + 7;
                    for j in after_word
                        ..std::cmp::min(after_word + 10, ctor_bytes.len().saturating_sub(7))
                    {
                        let is_mov_byte = ctor_bytes[j] == 0x40
                            && ctor_bytes[j + 1] == 0x88
                            && (ctor_bytes[j + 2] & 0xC0) == 0x80;
                        if is_mov_byte {
                            let byte_off = u32::from_le_bytes([
                                ctor_bytes[j + 3],
                                ctor_bytes[j + 4],
                                ctor_bytes[j + 5],
                                ctor_bytes[j + 6],
                            ]) as usize;
                            enable_flags = Some(word_off);
                            enable_count = byte_off - word_off + 1;
                            enable_value = 0; // restriction semantics: 0 = unrestricted
                            break;
                        }
                    }
                    break;
                }
            }
        }

        if let Some(off) = enable_flags {
            log_info!(
                "FolderExpansion: detected enable flags offset +0x{:X} ({} bytes, unlock_value={})",
                off,
                enable_count,
                enable_value
            );
        }

        unsafe {
            DIFF_OFFSETS = Some(DifficultyOffsets {
                max_diff,
                enable_flags,
                enable_flags_count: enable_count,
                enable_value,
            });
        }
    }

    /// Detect the FolderProperty struct layout by analyzing the first folder
    /// registration block in folder_init. Returns None if detection fails.
    ///
    /// Scans from the first property_ctor CALL to the first folder_register CALL,
    /// extracting field offsets from LEA RCX,[RDI+X] patterns and MOV [RDI+X],imm patterns.
    fn detect_layout(&self) -> Option<FolderPropertyLayout> {
        if self.folder_init_addr.is_null() || self.folder_init_size == 0 {
            return None;
        }
        let bytes =
            unsafe { std::slice::from_raw_parts(self.folder_init_addr, self.folder_init_size) };

        // Find the first property_ctor CALL and first folder_register CALL
        let mut ctor_pos: Option<usize> = None;
        let mut register_pos: Option<usize> = None;
        #[allow(clippy::needless_range_loop)]
        // cursor-style scan; i used for pointer arithmetic + slice index together
        for i in 0..bytes.len().saturating_sub(5) {
            if bytes[i] != 0xE8 {
                continue;
            }
            let target = unsafe { decode_call_rel32(self.folder_init_addr.add(i)) };
            if target == self.property_ctor && ctor_pos.is_none() {
                ctor_pos = Some(i);
            }
            if target == self.register_fn && ctor_pos.is_some() && register_pos.is_none() {
                register_pos = Some(i);
                break;
            }
        }

        let block_start = ctor_pos? + 5; // skip past the CALL instruction
        let block_end = register_pos?;
        log_info!(
            "FolderExpansion: analyzing first folder block [{:X}..{:X}] in folder_init",
            block_start,
            block_end
        );

        // Find struct_size: MOV ECX, imm32 (B9 XX XX XX XX) before the property_ctor CALL.
        // Scan backwards from ctor_pos.
        let mut struct_size: usize = 0;
        for back in 5..64usize {
            if ctor_pos.unwrap() < back {
                break;
            }
            let j = ctor_pos.unwrap() - back;
            if bytes[j] == 0xB9 && j + 5 <= bytes.len() {
                struct_size =
                    u32::from_le_bytes([bytes[j + 1], bytes[j + 2], bytes[j + 3], bytes[j + 4]])
                        as usize;
                break;
            }
        }
        if struct_size == 0 {
            log_warn!("FolderExpansion: could not detect struct size");
            return None;
        }
        log_info!(
            "FolderExpansion: detected struct_size = 0x{:X}",
            struct_size
        );

        // Collect all LEA RCX,[RDI+X] in the block and their following CALL targets.
        // Two forms: 48 8D 4F XX (disp8) and 48 8D 8F XX XX XX XX (disp32)
        struct LeaInfo {
            offset: usize,
            call_target: *const u8,
        }
        let mut leas: Vec<LeaInfo> = Vec::new();

        let mut i = block_start;
        while i < block_end.saturating_sub(7) {
            let field_offset: Option<usize>;
            let lea_len: usize;

            if bytes[i] == 0x48
                && bytes[i + 1] == 0x8D
                && bytes[i + 2] == 0x8F
                && i + 7 <= block_end
            {
                // LEA RCX,[RDI+disp32]
                field_offset = Some(u32::from_le_bytes([
                    bytes[i + 3],
                    bytes[i + 4],
                    bytes[i + 5],
                    bytes[i + 6],
                ]) as usize);
                lea_len = 7;
            } else if bytes[i] == 0x48
                && bytes[i + 1] == 0x8D
                && bytes[i + 2] == 0x4F
                && i + 4 <= block_end
            {
                // LEA RCX,[RDI+disp8]
                field_offset = Some(bytes[i + 3] as usize);
                lea_len = 4;
            } else {
                i += 1;
                continue;
            }

            let fo = field_offset.unwrap();
            // Find the next E8 CALL within 16 bytes after the LEA
            let search_start = i + lea_len;
            let mut found_call = false;
            #[allow(clippy::needless_range_loop)] // cursor-style forward scan for CALL byte
            for j in search_start..std::cmp::min(search_start + 16, block_end.saturating_sub(4)) {
                if bytes[j] == 0xE8 {
                    let target = unsafe { decode_call_rel32(self.folder_init_addr.add(j)) };
                    leas.push(LeaInfo {
                        offset: fo,
                        call_target: target,
                    });
                    i = j + 5;
                    found_call = true;
                    break;
                }
            }
            if !found_call {
                i += 1;
            }
        }

        if leas.len() < 4 {
            log_warn!(
                "FolderExpansion: expected >=4 LEA RCX,[RDI+X] patterns, found {}",
                leas.len()
            );
            return None;
        }

        // Group by call target to identify shared_ptr_move (appears exactly 2x)
        // vs string_assign (appears 2x or more, but is a different target).
        let mut target_counts: std::collections::HashMap<usize, usize> =
            std::collections::HashMap::new();
        for lea in &leas {
            *target_counts.entry(lea.call_target as usize).or_insert(0) += 1;
        }

        // shared_ptr_move target: appears exactly 2 times in this block
        // string_assign target: also appears 2 times but is a different function
        // Distinguish by order: string fields come first (key, voice_key), then functor slots
        let mut string_offsets: Vec<usize> = Vec::new();
        let mut spm_offsets: Vec<usize> = Vec::new();
        let mut spm_target: *const u8 = std::ptr::null();

        // The first two LEAs call string_assign; the last two call shared_ptr_move.
        // We identify them by noting that the first LEA's target is string_assign.
        let first_target = leas[0].call_target;
        for lea in &leas {
            if lea.call_target == first_target {
                string_offsets.push(lea.offset);
            } else if spm_target.is_null() || lea.call_target == spm_target {
                spm_target = lea.call_target;
                spm_offsets.push(lea.offset);
            }
        }

        if string_offsets.len() < 2 || spm_offsets.len() < 2 || spm_target.is_null() {
            log_warn!(
                "FolderExpansion: layout detection failed — strings:{}, spm:{}",
                string_offsets.len(),
                spm_offsets.len()
            );
            return None;
        }

        // Find mode_flag: MOV dword [RDI+X], 3 — C7 87 [disp32] 03 00 00 00
        let mut mode_flag_offset: Option<usize> = None;
        for j in block_start..block_end.saturating_sub(10) {
            if bytes[j] == 0xC7
                && bytes[j + 1] == 0x87
                && bytes[j + 6] == 0x03
                && bytes[j + 7] == 0x00
                && bytes[j + 8] == 0x00
                && bytes[j + 9] == 0x00
            {
                mode_flag_offset = Some(u32::from_le_bytes([
                    bytes[j + 2],
                    bytes[j + 3],
                    bytes[j + 4],
                    bytes[j + 5],
                ]) as usize);
                break;
            }
        }

        let mode_flag = match mode_flag_offset {
            Some(o) => o,
            None => {
                log_warn!("FolderExpansion: could not detect mode_flag offset");
                return None;
            }
        };

        // Detect create_folder_data: it's called BEFORE property_ctor in the first folder block.
        // Pattern: CALL create_folder_data; MOV RCX,[RAX] (48 8B 08) — reads shared_ptr from return.
        // Scan backwards from ctor_pos for an E8 CALL followed (within a few bytes) by 48 8B 08.
        let mut create_folder_data: *const u8 = std::ptr::null();
        for j in (5..ctor_pos.unwrap()).rev() {
            if bytes[j] != 0xE8 || j + 5 >= bytes.len() {
                continue;
            }
            // Check if 48 8B 08 (MOV RCX,[RAX]) appears within 8 bytes after the CALL
            let after = j + 5;
            let search_end = std::cmp::min(after + 8, bytes.len().saturating_sub(2));
            let mut found_mov = false;
            for k in after..search_end {
                if bytes[k] == 0x48 && bytes[k + 1] == 0x8B && bytes[k + 2] == 0x08 {
                    found_mov = true;
                    break;
                }
            }
            if !found_mov {
                continue;
            }
            let target = unsafe { decode_call_rel32(self.folder_init_addr.add(j)) };
            create_folder_data = target;
            break;
        }
        if create_folder_data.is_null() {
            log_warn!("FolderExpansion: could not detect create_folder_data function");
        } else {
            log_info!(
                "FolderExpansion: detected create_folder_data={:p}",
                create_folder_data
            );
        }

        let layout = FolderPropertyLayout {
            struct_size,
            key_offset: string_offsets[0],
            voice_key_offset: string_offsets[1],
            filter_functor_slot: spm_offsets[0],
            functor_slot: spm_offsets[1],
            mode_flag_offset: mode_flag,
            shared_ptr_move: spm_target,
            create_folder_data,
        };

        log_info!("FolderExpansion: detected layout — size=0x{:X} key=+0x{:X} voice=+0x{:X} filter=+0x{:X} functor=+0x{:X} mode=+0x{:X} spm={:p}",
            layout.struct_size, layout.key_offset, layout.voice_key_offset,
            layout.filter_functor_slot, layout.functor_slot, layout.mode_flag_offset, layout.shared_ptr_move);

        Some(layout)
    }
}

impl Mod for FolderExpansionMod {
    fn id(&self) -> &str {
        "folder-expansion"
    }
    fn name(&self) -> &str {
        "Folder Expansion"
    }
    fn description(&self) -> &str {
        "Custom genre folders and difficulty unlock"
    }
    fn required_signatures(&self) -> &[&str] {
        &[
            "folder_init",
            "folder_property_ctor",
            "folder_functor_ctor",
            "folder_filter_functor_ctor",
            "folder_store_ptr",
            "folder_register",
            "folder_has_songs",
        ]
    }

    fn init(&mut self, ctx: &ModContext) -> bool {
        let config = match Self::load_config() {
            Some(c) => c,
            None => return false,
        };

        // Resolve all function addresses
        self.folder_init_addr = ctx.signatures.require_address("folder_init");
        self.property_ctor = ctx.signatures.require_address("folder_property_ctor");
        self.functor_ctor = ctx.signatures.require_address("folder_functor_ctor");
        self.filter_functor_ctor = ctx.signatures.require_address("folder_filter_functor_ctor");
        self.store_ptr = ctx.signatures.require_address("folder_store_ptr");
        self.register_fn = ctx.signatures.require_address("folder_register");
        self.has_songs_fn = ctx.signatures.require_address("folder_has_songs");

        // Optional: gameplay object patch addresses (for custom folder support)
        self.gameplay_obj_alloc_size = ctx
            .signatures
            .get_address("gameplay_obj_alloc_size")
            .unwrap_or(std::ptr::null());
        self.gameplay_obj_ctor = ctx
            .signatures
            .get_address("gameplay_obj_ctor")
            .unwrap_or(std::ptr::null());
        self.game_malloc = ctx
            .signatures
            .get_address("game_malloc")
            .unwrap_or(std::ptr::null());

        if self.game_malloc.is_null() {
            log_warn!("FolderExpansion: game_malloc not found — custom folders disabled");
        }

        // Estimate folder_init size for scanning
        self.folder_init_size = estimate_function_size(self.folder_init_addr, 0x3000);
        log_info!(
            "FolderExpansion: folder_init size ~0x{:X}",
            self.folder_init_size
        );

        // Detect FolderProperty struct layout from folder_init disassembly
        match self.detect_layout() {
            Some(layout) => unsafe {
                FOLDER_LAYOUT = Some(layout);
            },
            None => {
                log_warn!("FolderExpansion: layout detection failed — custom folders disabled");
            }
        }

        // Detect difficulty offsets for hook-based unlock
        self.detect_difficulty_offsets();

        unsafe {
            FOLDER_CONFIG = Some(config);
        }
        log_info!("FolderExpansion: initialized");
        true
    }

    fn enable(&mut self) {
        let config = match get_config() {
            Some(c) => c,
            None => return,
        };

        // Install folder_register hook (for custom folders and difficulty unlock)
        unsafe {
            // Store function pointers in statics for hook callback access (may be null if layout detection failed)
            if let Some(layout) = get_layout() {
                FN_PROPERTY_CTOR = Some(std::mem::transmute::<
                    *const u8,
                    unsafe extern "C" fn(*mut u8) -> *mut u8,
                >(self.property_ctor));
                FN_FUNCTOR_CTOR = Some(std::mem::transmute::<
                    *const u8,
                    unsafe extern "C" fn(*mut u8, u32) -> *mut u8,
                >(self.functor_ctor));
                FN_FILTER_FUNCTOR_CTOR = Some(std::mem::transmute::<
                    *const u8,
                    unsafe extern "C" fn(*mut u8, u32) -> *mut u8,
                >(self.filter_functor_ctor));
                FN_SHARED_PTR_MOVE = Some(std::mem::transmute::<
                    *const u8,
                    unsafe extern "C" fn(*mut u8, *mut u8),
                >(layout.shared_ptr_move));
                FN_STORE_PTR = Some(std::mem::transmute::<
                    *const u8,
                    unsafe extern "C" fn(*mut u8, *mut u8) -> *mut u8,
                >(self.store_ptr));
            }
            if !self.game_malloc.is_null() {
                FN_GAME_MALLOC = Some(std::mem::transmute::<
                    *const u8,
                    unsafe extern "C" fn(usize) -> *mut u8,
                >(self.game_malloc));
            }
            CUSTOM_FOLDERS_CREATED = false;

            let target: FolderRegisterFn = std::mem::transmute(self.register_fn);
            match crate::core::hooks::install_enabled(
                std::ptr::addr_of_mut!(REGISTER_HOOK),
                target,
                folder_register_hook,
            ) {
                Ok(()) => {
                    log_info!("FolderExpansion: folder_register hook installed");
                }
                Err(e) => {
                    log_error!("FolderExpansion: failed to hook folder_register: {}", e);
                }
            }
        }

        // Install has_songs predicate hook
        if !config.custom_folders.is_empty() {
            unsafe {
                let target: FolderHasSongsFn = std::mem::transmute(self.has_songs_fn);
                match crate::core::hooks::install_enabled(
                    std::ptr::addr_of_mut!(HAS_SONGS_HOOK),
                    target,
                    folder_has_songs_hook,
                ) {
                    Ok(()) => {
                        log_info!("FolderExpansion: has_songs hook installed");
                    }
                    Err(e) => {
                        log_error!("FolderExpansion: failed to hook has_songs: {}", e);
                    }
                }
            }
        }

        // Generate custom geo + AFP files for LayeredFS serving
        if !config.custom_folders.is_empty() {
            // Patch gameplay sequence object to accommodate extra folders.
            // The game has a fixed-size shared_ptr array (one slot per non-ALL_MUSIC folder).
            // Adding custom folders overflows it. Enlarge the allocation and zero extra bytes.
            if !self.gameplay_obj_alloc_size.is_null() && !self.gameplay_obj_ctor.is_null() {
                let extra_bytes = (config.custom_folders.len() as u32) * 0x10;
                unsafe {
                    let size_ptr = self.gameplay_obj_alloc_size as *mut u32;
                    let original_size = size_ptr.read_unaligned();
                    let new_size = original_size + extra_bytes;

                    let old_prot = memory::make_writable(size_ptr as *const u8, 4);
                    size_ptr.write_unaligned(new_size);
                    memory::restore_protection(size_ptr as *const u8, 4, old_prot);

                    GAMEPLAY_OBJ_ORIGINAL_SIZE = original_size;
                    GAMEPLAY_OBJ_EXTRA_BYTES = extra_bytes;
                    let ctor_fn: GameplayObjCtorFn = std::mem::transmute(self.gameplay_obj_ctor);
                    match crate::core::hooks::install_enabled(
                        std::ptr::addr_of_mut!(GAMEPLAY_OBJ_CTOR_HOOK),
                        ctor_fn,
                        gameplay_obj_ctor_hook,
                    ) {
                        Ok(()) => {
                            log_info!(
                                "FolderExpansion: patched gameplay obj 0x{:X}→0x{:X}",
                                original_size,
                                new_size
                            );
                        }
                        Err(e) => {
                            log_error!("FolderExpansion: failed to hook gameplay obj ctor: {}", e)
                        }
                    }
                }
            } else {
                log_warn!(
                    "FolderExpansion: gameplay obj addresses not found — custom folders may crash"
                );
            }

            // Asset generation is the slowest part of enable() (~1.3s
            // cold). The output is a pure function of (config, source
            // ARC), so on a warm boot with neither changed, skip it.
            // Hooks above must always run — they patch live game memory.
            let need_regen = match compute_cache_key(config) {
                Some(want) => {
                    if cache_is_valid(&want) {
                        log_info!("FolderExpansion: cache HIT, skipping asset regeneration");
                        false
                    } else {
                        log_info!("FolderExpansion: cache MISS, regenerating assets");
                        true
                    }
                }
                None => {
                    log_warn!("FolderExpansion: cache key unavailable, regenerating assets");
                    true
                }
            };

            if need_regen {
                generate_custom_assets(config);
                // Re-scan mod folders so LayeredFS finds the newly generated files
                mod_paths::init_mod_paths();
                if let Some(meta) = compute_cache_key(config) {
                    write_cache_meta(&meta);
                }
            }
        }

        // Hide difficulty limit pane via AFP patch (empty root frame → renders nothing)
        if config.hide_difficulty_pane {
            afp_patcher::register_patch(
                "difficulty_limit",
                Box::new(|afp_data, _bsi| patch_empty_root_frame(afp_data)),
            );
            log_info!("FolderExpansion: difficulty pane hidden");
        }

        log_info!("FolderExpansion: enabled");
    }

    fn disable(&mut self) {
        // Remove hooks
        unsafe {
            REGISTER_HOOK = None;
            HAS_SONGS_HOOK = None;
            CUSTOM_FOLDERS_CREATED = false;
            FN_PROPERTY_CTOR = None;
            FN_FUNCTOR_CTOR = None;
            FN_FILTER_FUNCTOR_CTOR = None;
            FN_SHARED_PTR_MOVE = None;
            FN_STORE_PTR = None;
            FN_GAME_MALLOC = None;
            GAMEPLAY_OBJ_CTOR_HOOK = None;
        }

        log_info!("FolderExpansion: disabled");
    }
}

/// Estimate function size by scanning for the epilogue pattern.
fn estimate_function_size(addr: *const u8, max_search: usize) -> usize {
    let bytes = unsafe { std::slice::from_raw_parts(addr, max_search) };
    for i in (0..max_search.saturating_sub(2)).rev() {
        if bytes[i] == 0x5D && bytes[i + 1] == 0xC3 {
            return i + 2;
        }
    }
    max_search
}
