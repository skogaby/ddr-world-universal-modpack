//! AVS LayeredFS — transparent file replacement via AVS filesystem hooks.
//!
//! Hooks Konami's AVS filesystem layer (`libavs-win64.dll`) so game file opens are
//! served from the `data_mods/` folder when a mod provides, overlays or extends
//! them — without repacking any container on disk. Always on (not a mod, no menu
//! toggle); unmodified files pass straight through.
//!
//! ## Init order
//!
//! [`init`] runs at `lib.rs` step 0b, right after the config store and BEFORE the
//! gamemdx wait and the signature scan. `Application::onBoot` reads
//! `startup.arc`, `shader.arc` (the only shader read of the session) and
//! `musicdb.xml` within a few hundred ms of gamemdx loading, on the game's own
//! thread; installing any later loses that race and shader synthesis and musicdb
//! merges silently fall back to stock. Never move it below `resolve_all`. It
//! depends only on libavs exports and the mod folder.
//!
//! ## Hooks
//!
//! `file_hooks` detours `avs_fs_open`, `avs_fs_lstat`, `avs_fs_mount`,
//! `avs_fs_read` and `avs_fs_convert_path` all-or-nothing (a failed install
//! rolls the others back and the service stays unavailable). It also detours
//! `kernel32!GetLongPathNameA`, best-effort, to work around AVS's fixed 128-byte
//! path buffer on long `_cache` paths. Hook bodies are panic-contained and pass
//! through on failure.
//!
//! ## Request routing (`file_hooks::find_mod_replacement`)
//!
//! A normalized path is matched against the mod folders (direct file, then with
//! `.ifs` expanded to `_ifs`), then:
//! - `.arc` — `arc_handler` repacks the archive with a sibling `<name>_arc/`
//!   overlay folder plus, for `shader.arc`, the containers `shader_synthesis`
//!   builds from the stock blobs and `data_mods/shader_fixes/blobs/`;
//! - a direct match — served as is;
//! - `.xml` — `xml_merger` applies `.merged.xml` appends (e.g. `musicdb.xml`);
//!   `texturelist.xml` / `afplist.xml` are also parsed (and afplists extended) by
//!   `ifs_textures`;
//! - other IFS members — `ifs_textures` serves converted textures and AFP/geo
//!   replacements by MD5 name;
//! - otherwise — passthrough to the original path.
//!
//! Built output (converted textures, repacked arcs, merged XML) lives in
//! `data_mods/_cache/`; `cache_hasher` input hashes decide when a merge or
//! repack must be rebuilt. It is machine-owned: never hand-edit or commit it.
//!
//! ## Config (`layeredfs` section, operator-only)
//!
//! `verbose` (extra logging), `developer_mode` (no folder-content caching; live
//! filesystem checks), `mod_folder` (default `./data_mods`), `allowlist` /
//! `blocklist` (mod folder names to include / skip).
//!
//! ## Submodules
//!
//! - `avs_resolver` — libavs export resolution across AVS versions.
//! - `file_hooks` — the detours and request routing above.
//! - `mod_paths` — mod folder scan, path normalization, lookup.
//! - `arc_handler` — `.arc` overlay and repack.
//! - `shader_synthesis` / `shader_layout` — runtime shader-container synthesis and
//!   its pure layout rules.
//! - `xml_merger` — `.merged.xml` merging; `kbin` — binary XML decoding.
//! - `ifs_textures`, `atlas_cloner`, `texture_packer`, `afplist_ext` — IFS texture
//!   and AFP replacement, atlas cloning and packing, afplist geo-list extension.
//! - `ramfs_demangler` — maps RAM-mounted IFS virtual paths back to real paths.
//! - `avslz` — AVSLZ compression; `cache_hasher` — cache invalidation hashes.

pub(crate) mod afplist_ext;
pub(super) mod arc_handler;
pub(crate) mod atlas_cloner;
pub(super) mod avs_resolver;
pub(crate) mod avslz;
pub(crate) mod cache_hasher;
pub(super) mod file_hooks;
pub(crate) mod ifs_textures;
pub(crate) mod kbin;
pub(crate) mod mod_paths;
pub(super) mod ramfs_demangler;
pub(crate) mod shader_layout;
pub(crate) mod shader_synthesis;
pub(super) mod texture_packer;
pub(super) mod xml_merger;

use once_cell::sync::Lazy;
use serde::Deserialize;
use std::sync::Mutex;

use crate::log_info;
use crate::log_warn;

use self::avs_resolver::AvsFunctions;

// ── Configuration ────────────────────────────────────────────────────

#[derive(Deserialize, Clone)]
pub struct LayeredFsConfig {
    #[serde(default)]
    pub verbose: bool,
    #[serde(default)]
    pub developer_mode: bool,
    #[serde(default = "default_mod_folder")]
    pub mod_folder: String,
    #[serde(default)]
    pub allowlist: Vec<String>,
    #[serde(default)]
    pub blocklist: Vec<String>,
}

fn default_mod_folder() -> String {
    "./data_mods".to_string()
}

impl Default for LayeredFsConfig {
    fn default() -> Self {
        Self {
            verbose: false,
            developer_mode: false,
            mod_folder: default_mod_folder(),
            allowlist: Vec::new(),
            blocklist: Vec::new(),
        }
    }
}

// ── Service state ────────────────────────────────────────────────────

struct LayeredFsInner {
    available: bool,
    conversion_ready: bool,
    source_read_ready: bool,
    config: LayeredFsConfig,
    /// `config.mod_folder` with `/` rewritten to `\` — what Win32 sees when AVS
    /// passes mod-cache paths through `GetLongPathNameA`. Computed once in init.
    mod_folder_native: String,
    avs: Option<AvsFunctions>,
    avs_version: u16,
    avs_version_name: &'static str,
}

static STATE: Lazy<Mutex<LayeredFsInner>> = Lazy::new(|| {
    Mutex::new(LayeredFsInner {
        available: false,
        conversion_ready: false,
        source_read_ready: false,
        config: LayeredFsConfig::default(),
        mod_folder_native: String::new(),
        avs: None,
        avs_version: 0,
        avs_version_name: "",
    })
});

// ── Public API ───────────────────────────────────────────────────────

/// Initialize the LayeredFS service: load config, resolve AVS, scan mods.
/// Returns true if AVS was resolved and hooks can be installed.
///
/// Runs BEFORE the gamemdx wait / signature scan (lib.rs step 0b): the
/// game's `Application::onBoot` opens `shader.arc` exactly once, within a
/// few hundred ms of gamemdx loading, and the shader-fixes / mod-menu theme
/// synthesis rides that open. Nothing here depends on gamemdx.
pub fn init() -> bool {
    let config = load_config();
    let verbose = config.verbose;

    avs_resolver::wait_for_avs_dll();
    let resolution = match avs_resolver::resolve_avs() {
        Some(r) => r,
        None => {
            log_warn!("LayeredFS: AVS DLL not found or exports don't match — disabled");
            return false;
        }
    };

    log_info!(
        "LayeredFS: AVS {} detected (version {})",
        resolution.version_name,
        resolution.version
    );

    if verbose {
        log_info!("LayeredFS: verbose logging enabled");
        log_info!("LayeredFS: mod_folder = {}", config.mod_folder);
        if config.developer_mode {
            log_info!("LayeredFS: developer mode ON (no caching, live filesystem checks)");
        }
        if !config.allowlist.is_empty() {
            log_info!("LayeredFS: allowlist = {:?}", config.allowlist);
        }
        if !config.blocklist.is_empty() {
            log_info!("LayeredFS: blocklist = {:?}", config.blocklist);
        }
    }

    let mut state = STATE.lock().unwrap();
    state.avs_version = resolution.version;
    state.avs_version_name = resolution.version_name;
    state.mod_folder_native = config.mod_folder.replace('/', "\\");
    state.config = config;
    state.avs = Some(resolution.functions);
    state.available = false;
    state.conversion_ready = false;
    state.source_read_ready = false;

    // Scan mod folders and cache contents
    drop(state); // release lock before scanning (init_mod_paths acquires its own)
    mod_paths::init_mod_paths();

    // Index the on-disk texture cache once, so the texture hot path resolves
    // cache hits via an in-memory set instead of a per-open filesystem stat
    // (scene-21 preloads thousands of textures; the per-open `exists()` made
    // load time scale with OS file-cache warmth). Cloned atlases / converted
    // textures written later at mod enable() keep the index live themselves.
    ifs_textures::build_cache_index();

    // Install AVS filesystem hooks
    if !file_hooks::install_hooks() {
        let mut state = STATE.lock().unwrap();
        state.available = false;
        state.conversion_ready = false;
        state.source_read_ready = false;
        log_warn!("LayeredFS: hook installation failed — file replacement disabled");
        return false;
    }

    let mut state = STATE.lock().unwrap();
    state.available = true;
    state.conversion_ready = true;
    state.source_read_ready = true;
    drop(state);

    log_info!("LayeredFS: initialized successfully");
    true
}

/// Check if the LayeredFS service initialized successfully.
pub fn is_available() -> bool {
    STATE.lock().unwrap().available
}

pub fn conversion_ready() -> bool {
    STATE.lock().unwrap().conversion_ready
}

pub fn source_read_ready() -> bool {
    STATE.lock().unwrap().source_read_ready
}

/// Access the resolved AVS functions. Panics if not available.
/// NOTE: The closure must NOT call AVS filesystem functions (open/read/close etc.)
/// because they are hooked and will try to re-acquire this lock, causing deadlock.
/// Use `get_avs_fns()` instead when you need to call AVS functions.
pub(super) fn with_avs<F, R>(f: F) -> R
where
    F: FnOnce(&AvsFunctions) -> R,
{
    let state = STATE.lock().unwrap();
    f(state.avs.as_ref().expect("LayeredFS AVS not initialized"))
}

/// Get a copy of the AVS function pointers without holding any lock.
/// Safe to call AVS functions through these pointers (they go through our hooks,
/// which acquire their own short-lived locks on STATE).
pub(super) fn get_avs_fns() -> AvsFunctions {
    let state = STATE.lock().unwrap();
    let avs = state.avs.as_ref().expect("LayeredFS AVS not initialized");
    AvsFunctions {
        avs_fs_open: avs.avs_fs_open,
        avs_fs_close: avs.avs_fs_close,
        avs_fs_read: avs.avs_fs_read,
        avs_fs_lstat: avs.avs_fs_lstat,
        avs_fs_fstat: avs.avs_fs_fstat,
        avs_fs_lseek: avs.avs_fs_lseek,
        avs_fs_mount: avs.avs_fs_mount,
        avs_fs_convert_path: avs.avs_fs_convert_path,
        property_read_query_memsize: avs.property_read_query_memsize,
        property_read_query_memsize_long: avs.property_read_query_memsize_long,
        property_create: avs.property_create,
        property_insert_read: avs.property_insert_read,
        property_mem_write: avs.property_mem_write,
        property_query_size: avs.property_query_size,
        property_destroy: avs.property_destroy,
        cstream_create: avs.cstream_create,
        cstream_operate: avs.cstream_operate,
        cstream_finish: avs.cstream_finish,
        cstream_destroy: avs.cstream_destroy,
    }
}

/// Access the config.
pub(super) fn config() -> LayeredFsConfig {
    STATE.lock().unwrap().config.clone()
}

/// Mod folder string in native (backslash) form — used by the
/// `GetLongPathNameA` long-path workaround.
pub(super) fn mod_folder_native() -> String {
    STATE.lock().unwrap().mod_folder_native.clone()
}

/// Get the detected AVS version number (bemanitools form, e.g. 1700 = 2.17.x).
pub(super) fn avs_version() -> u16 {
    STATE.lock().unwrap().avs_version
}

// ── Config loading ───────────────────────────────────────────────────

fn load_config() -> LayeredFsConfig {
    match crate::mods::config::get() {
        Some(cfg) => cfg.layeredfs.clone().unwrap_or_default(),
        None => LayeredFsConfig::default(),
    }
}
