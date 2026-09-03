//! AVS LayeredFS — Transparent file replacement via AVS filesystem hooks.
//!
//! Hooks Konami's AVS filesystem layer (`libavs-win64.dll`) to intercept file
//! accesses at runtime, enabling transparent replacement and injection of game
//! assets from a `data_mods/` folder without repacking container files.
//!
//! This is an always-on service (not toggleable via mod menu). Once initialized,
//! hooks are transparent — unmodified files pass through with negligible overhead.

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
