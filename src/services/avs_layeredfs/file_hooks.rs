//! File hooks — retour static detours on AVS filesystem functions.
//!
//! Intercepts avs_fs_open, avs_fs_lstat, avs_fs_mount, avs_fs_read, and
//! avs_fs_convert_path to enable transparent file replacement from the
//! data_mods/ folder.
//!
//! Also hooks `kernel32!GetLongPathNameA` to work around a hard-coded 128‑byte
//! buffer AVS uses internally; long ifs‑in‑arc cache paths can blow past it
//! and AVS doesn't fall through to `CreateFileA`. See `hook_get_long_path_name_a`.

use retour::GenericDetour;
use std::ffi::{CStr, CString};
use std::panic::AssertUnwindSafe;
use std::ptr::addr_of;

use crate::{log_info, log_warn};

use super::arc_handler;
use super::avs_resolver::*;
use super::ifs_textures;
use super::mod_paths;
use super::ramfs_demangler;
use super::xml_merger;
use super::{avs_version, config};

// ── Static detours ───────────────────────────────────────────────────

static mut HOOK_FS_OPEN: Option<GenericDetour<FnAvsFsOpen>> = None;
static mut HOOK_FS_LSTAT: Option<GenericDetour<FnAvsFsLstat>> = None;
static mut HOOK_FS_MOUNT: Option<GenericDetour<FnAvsFsMount>> = None;
static mut HOOK_FS_READ: Option<GenericDetour<FnAvsFsRead>> = None;
static mut HOOK_FS_CONVERT_PATH: Option<GenericDetour<FnAvsFsConvertPath>> = None;

type FnGetLongPathNameA = unsafe extern "system" fn(*const i8, *mut i8, u32) -> u32;
static mut HOOK_GET_LONG_PATH_NAME_A: Option<GenericDetour<FnGetLongPathNameA>> = None;

/// Helper to read a static detour without triggering static_mut_refs warnings.
/// Returns `Option<&GenericDetour<_>>` — callers must handle `None` without
/// panicking: every caller runs inside an `extern "C"`/`extern "system"`
/// callback on arbitrary game threads, where an unwind aborts the process.
/// (A `.unwrap()` here, hit during the install race, was the 2026-07
/// non-deterministic boot abort.)
macro_rules! get_hook {
    ($hook:ident) => {
        unsafe { &*addr_of!($hook) }.as_ref()
    };
}

// ── Hook callbacks ───────────────────────────────────────────────────

// ── Original (unhoooked) AVS function accessors ──────────────────────
// These call through the retour trampoline, bypassing our hooks.
// Use these when our code needs to call AVS functions (e.g. loading XML)
// to avoid infinite recursion.

pub unsafe fn orig_fs_open(name: *const i8, mode: u16, flags: i32) -> AvsFile {
    match get_hook!(HOOK_FS_OPEN) {
        Some(hook) => hook.call(name, mode, flags),
        // Hook not installed → the target is unpatched, so the raw AVS
        // function IS the original.
        None => (super::get_avs_fns().avs_fs_open)(name, mode, flags),
    }
}
pub unsafe fn orig_fs_read(handle: AvsFile, buf: *mut u8, nbytes: usize) -> usize {
    match get_hook!(HOOK_FS_READ) {
        Some(hook) => hook.call(handle, buf, nbytes),
        None => (super::get_avs_fns().avs_fs_read)(handle, buf, nbytes),
    }
}
pub unsafe fn orig_fs_close(handle: AvsFile) {
    let avs = super::get_avs_fns();
    (avs.avs_fs_close)(handle)
}
pub unsafe fn orig_fs_fstat(handle: AvsFile, st: *mut AvsStat) -> i32 {
    let avs = super::get_avs_fns();
    (avs.avs_fs_fstat)(handle, st)
}

// ── Hook callback implementations ───────────────────────────────────
//
// These are `extern "C"` callbacks invoked on arbitrary AVS threads —
// including the AVS boot/config worker (`ess_soft_info_get` reads config
// files through `avs_fs_open` while the game is still initializing). A Rust
// panic that unwinds across the `extern "C"` boundary is undefined behavior;
// on this toolchain it aborts the process with "thread caused non-unwinding
// panic" (observed as a non-deterministic boot crash). So every callback body
// runs inside `catch_unwind` and, on panic, falls through to the original
// unmodified AVS call — the file is served vanilla, and LayeredFS logic for
// that one call is skipped rather than taking down the game. (CLAUDE.md rule 1.)

unsafe extern "C" fn hook_avs_fs_open(name: *const i8, mode: u16, flags: i32) -> AvsFile {
    // `None` is unreachable once installed (the handle is stored before the
    // detour is enabled), but a panic here would abort the process, so bail
    // to a benign failure instead of unwrapping. -1 = AVS open failure.
    let Some(original) = get_hook!(HOOK_FS_OPEN) else {
        return -1;
    };
    std::panic::catch_unwind(AssertUnwindSafe(|| {
        fs_open_body(original, name, mode, flags)
    }))
    .unwrap_or_else(|_| {
        log_warn!("LayeredFS: panic in avs_fs_open hook — serving original file");
        original.call(name, mode, flags)
    })
}

unsafe fn fs_open_body(
    original: &GenericDetour<FnAvsFsOpen>,
    name: *const i8,
    mode: u16,
    flags: i32,
) -> AvsFile {
    if name.is_null() {
        return original.call(name, mode, flags);
    }

    // Only intercept reads
    if mode != avs_open_mode_read(avs_version()) {
        return original.call(name, mode, flags);
    }

    let path = CStr::from_ptr(name).to_string_lossy().to_string();
    let verbose = config().verbose;
    if verbose {
        log_info!("LayeredFS: open {}", path);
    }

    let norm = match mod_paths::normalise_path(&path) {
        Some(n) => n,
        None => return original.call(name, mode, flags),
    };

    // Try to find a mod replacement (with IFS expansion)
    let replacement = find_mod_replacement(&norm, &path);

    let result = if let Some(ref mod_path) = replacement {
        if verbose {
            log_info!("LayeredFS: using {}", mod_path);
        }
        match CString::new(mod_path.as_str()) {
            Ok(cpath) => original.call(cpath.as_ptr(), mode, flags),
            Err(_) => original.call(name, mode, flags),
        }
    } else {
        original.call(name, mode, flags)
    };

    // Feed demangler for .ifs files
    ramfs_demangler::on_fs_open(&path, result);

    // Per-song SSQ-open observer (Per-Song Judgement Offsets, D21): every
    // stage load — normal, course/dan, training — opens the song's chart as
    // `.../ssq/<basename>[_N].ssq`; publishing the basename here gives the
    // override lifecycle a per-STAGE song identity (the wheel latch only
    // knows the course's first song). Cheap suffix check on the normalized
    // path; no allocation unless it IS an SSQ.
    if result >= 0 {
        crate::mods::per_song_judgement_offsets::override_hook::on_ssq_open(&norm);
    }

    result
}

unsafe extern "C" fn hook_avs_fs_lstat(name: *const i8, st: *mut AvsStat) -> i32 {
    let Some(original) = get_hook!(HOOK_FS_LSTAT) else {
        return -1;
    };
    std::panic::catch_unwind(AssertUnwindSafe(|| fs_lstat_body(original, name, st))).unwrap_or_else(
        |_| {
            log_warn!("LayeredFS: panic in avs_fs_lstat hook — serving original file");
            original.call(name, st)
        },
    )
}

unsafe fn fs_lstat_body(
    original: &GenericDetour<FnAvsFsLstat>,
    name: *const i8,
    st: *mut AvsStat,
) -> i32 {
    if name.is_null() {
        return original.call(name, st);
    }

    let path = CStr::from_ptr(name).to_string_lossy().to_string();

    let norm = match mod_paths::normalise_path(&path) {
        Some(n) => n,
        None => return original.call(name, st),
    };

    let replacement = find_mod_replacement(&norm, &path);

    if let Some(ref mod_path) = replacement {
        if config().verbose {
            log_info!("LayeredFS: lstat using {}", mod_path);
        }
        match CString::new(mod_path.as_str()) {
            Ok(cpath) => original.call(cpath.as_ptr(), st),
            Err(_) => original.call(name, st),
        }
    } else {
        original.call(name, st)
    }
}

unsafe extern "C" fn hook_avs_fs_mount(
    mountpoint: *const i8,
    fsroot: *const i8,
    fstype: *const i8,
    flags: *const i8,
) -> i32 {
    let Some(original) = get_hook!(HOOK_FS_MOUNT) else {
        return -1;
    };
    std::panic::catch_unwind(AssertUnwindSafe(|| {
        fs_mount_body(original, mountpoint, fsroot, fstype, flags)
    }))
    .unwrap_or_else(|_| {
        log_warn!("LayeredFS: panic in avs_fs_mount hook — passing through");
        original.call(mountpoint, fsroot, fstype, flags)
    })
}

unsafe fn fs_mount_body(
    original: &GenericDetour<FnAvsFsMount>,
    mountpoint: *const i8,
    fsroot: *const i8,
    fstype: *const i8,
    flags: *const i8,
) -> i32 {
    // Feed demangler before calling original
    let mp = if !mountpoint.is_null() {
        CStr::from_ptr(mountpoint).to_string_lossy().to_string()
    } else {
        String::new()
    };
    let fr = if !fsroot.is_null() {
        CStr::from_ptr(fsroot).to_string_lossy().to_string()
    } else {
        String::new()
    };
    let ft = if !fstype.is_null() {
        CStr::from_ptr(fstype).to_string_lossy().to_string()
    } else {
        String::new()
    };
    let fl = if !flags.is_null() {
        CStr::from_ptr(flags).to_string_lossy().to_string()
    } else {
        String::new()
    };

    if config().verbose {
        log_info!("LayeredFS: mount {} -> {} type={} flags={}", fr, mp, ft, fl);
    }

    ramfs_demangler::on_fs_mount(&mp, &fr, &ft, &fl);

    original.call(mountpoint, fsroot, fstype, flags)
}

unsafe extern "C" fn hook_avs_fs_read(context: AvsFile, bytes: *mut u8, nbytes: usize) -> usize {
    let Some(original) = get_hook!(HOOK_FS_READ) else {
        return 0;
    };
    std::panic::catch_unwind(AssertUnwindSafe(|| {
        // Feed demangler with buffer address
        ramfs_demangler::on_fs_read(context, bytes as usize);
        original.call(context, bytes, nbytes)
    }))
    .unwrap_or_else(|_| {
        log_warn!("LayeredFS: panic in avs_fs_read hook — serving original read");
        original.call(context, bytes, nbytes)
    })
}

unsafe extern "C" fn hook_avs_fs_convert_path(dest: *mut i8, name: *const i8) -> i32 {
    let Some(original) = get_hook!(HOOK_FS_CONVERT_PATH) else {
        return -1;
    };
    std::panic::catch_unwind(AssertUnwindSafe(|| {
        fs_convert_path_body(original, dest, name)
    }))
    .unwrap_or_else(|_| {
        log_warn!("LayeredFS: panic in avs_fs_convert_path hook — passing through");
        original.call(dest, name)
    })
}

unsafe fn fs_convert_path_body(
    original: &GenericDetour<FnAvsFsConvertPath>,
    dest: *mut i8,
    name: *const i8,
) -> i32 {
    if name.is_null() {
        return original.call(dest, name);
    }

    let path = CStr::from_ptr(name).to_string_lossy().to_string();

    let norm = match mod_paths::normalise_path(&path) {
        Some(n) => n,
        None => return original.call(dest, name),
    };

    let replacement = find_mod_replacement(&norm, &path);

    if let Some(ref mod_path) = replacement {
        match CString::new(mod_path.as_str()) {
            Ok(cpath) => original.call(dest, cpath.as_ptr()),
            Err(_) => original.call(dest, name),
        }
    } else {
        original.call(dest, name)
    }
}

// ── Mod replacement lookup ───────────────────────────────────────────

/// Try to find a mod file for the given normalized path, including IFS expansion.
/// Also handles XML merging, texture replacement, and AFP/geo mapping.
fn find_mod_replacement(norm_path: &str, original_path: &str) -> Option<String> {
    let lower = norm_path.to_lowercase();

    // Direct match first, then iterative IFS expansion (.ifs → _ifs).
    let direct = mod_paths::find_first_modfile(norm_path).or_else(|| {
        let mut expanded = norm_path.to_string();
        loop {
            let pos = expanded.to_lowercase().rfind(".ifs")?;
            expanded.replace_range(pos..pos + 4, "_ifs");
            if let Some(p) = mod_paths::find_first_modfile(&expanded) {
                return Some(p);
            }
        }
    });

    // ARC handling. Runs even when a direct .arc replacement was found —
    // an inner-ifs-only mod folder needs its basenames registered with the
    // demangler regardless of whether anything else replaced the arc.
    // If overlay files exist, the cached repack supersedes the direct match.
    if lower.ends_with(".arc") {
        // Use the direct match (if any) as the source the overlays will apply
        // on top of; otherwise apply on top of the original game arc.
        let arc_source = direct.as_deref().unwrap_or(original_path);
        if let Some(cached) = arc_handler::handle_arc(norm_path, arc_source) {
            return Some(cached);
        }
        if let Some(p) = direct {
            return Some(p);
        }
        return None;
    }

    if let Some(p) = direct {
        return Some(p);
    }

    // XML merging
    if lower.ends_with(".xml") {
        if let Some(cached) = xml_merger::merge_xmls(norm_path, original_path) {
            // After merging, still run AFP/texture list parsing on the merged result
            // so MD5 mappings are registered for any new entries
            if lower.ends_with("texturelist.xml") {
                ifs_textures::parse_texturelist(norm_path, &cached);
            } else if lower.ends_with("afplist.xml") {
                ifs_textures::parse_afplist(norm_path, &cached);
            }
            return Some(cached);
        }
    }

    // IFS texture/AFP list parsing (registers MD5 mappings for future lookups)
    if lower.ends_with("texturelist.xml") {
        ifs_textures::parse_texturelist(norm_path, original_path);
        // If new textures were injected, serve the modified texturelist so the
        // game's AFP runtime knows about them
        let ifs_mod_path = norm_path
            .strip_suffix("/tex/texturelist.xml")
            .or_else(|| norm_path.strip_suffix("\\tex\\texturelist.xml"))
            .map(|p| p.replace(".ifs", "_ifs"));
        if let Some(ref imp) = ifs_mod_path {
            let cached = format!("./data_mods/_cache/{}/texturelist.xml", imp);
            if std::path::Path::new(&cached).exists() {
                return Some(cached);
            }
        }
    } else if lower.ends_with("afplist.xml") {
        ifs_textures::parse_afplist(norm_path, original_path);
    } else {
        // Try texture replacement via MD5 lookup
        if let Some(cached) = ifs_textures::handle_texture(norm_path) {
            return Some(cached);
        }
        // Try AFP/geo replacement via MD5 lookup
        if let Some(mod_file) = ifs_textures::handle_afp(norm_path) {
            return Some(mod_file);
        }
    }

    None
}

// ── GetLongPathNameA hook ────────────────────────────────────────────

/// Detour for `kernel32!GetLongPathNameA`. AVS calls this with a fixed 128‑byte
/// buffer when normalizing paths internally; ifs‑in‑arc cache paths (e.g.
/// `./data_mods/_cache/arc/.../inner_ifs/<md5>`) routinely exceed that. When
/// the real Win32 call returns "buffer too small" (`ret > cchBuffer`) for one
/// of *our* mod-cache paths, we lie and return `ERROR_FILE_NOT_FOUND` so AVS
/// falls through to `CreateFileA` with the original (still valid) path
/// instead of strcmp-failing and bailing out.
unsafe extern "system" fn hook_get_long_path_name_a(
    short_path: *const i8,
    long_path: *mut i8,
    cch_buffer: u32,
) -> u32 {
    // This fires on arbitrary threads (AVS boot workers, Win32 callers) the
    // instant the detour is enabled. The handle is stored before enable, so
    // `None` is unreachable in practice — but if it ever isn't, report "not
    // found"
    // (AVS's documented fall-through to CreateFileA) instead of panicking:
    // an unwind out of `extern "system"` aborts the process, and this exact
    // unwrap-during-install was the 2026-07 non-deterministic boot crash.
    let Some(original) = get_hook!(HOOK_GET_LONG_PATH_NAME_A) else {
        use windows::Win32::Foundation::{SetLastError, ERROR_FILE_NOT_FOUND};
        SetLastError(ERROR_FILE_NOT_FOUND);
        return 0;
    };
    let ret = original.call(short_path, long_path, cch_buffer);

    // Panic-isolate the mod-path check (runs on arbitrary AVS/Win32 callers).
    // On panic, return the real Win32 result unmodified. `extern "system"` has
    // the same no-unwind-across-FFI rule as `extern "C"` (CLAUDE.md rule 1).
    std::panic::catch_unwind(AssertUnwindSafe(|| {
        if cch_buffer == 0x80 && ret > cch_buffer && !short_path.is_null() {
            let path = match CStr::from_ptr(short_path).to_str() {
                Ok(s) => s,
                Err(_) => return ret,
            };
            let mod_folder_native = super::mod_folder_native();
            if !mod_folder_native.is_empty() && path.contains(&mod_folder_native) {
                use windows::Win32::Foundation::{SetLastError, ERROR_FILE_NOT_FOUND};
                SetLastError(ERROR_FILE_NOT_FOUND);
                return 0;
            }
        }
        ret
    }))
    .unwrap_or(ret)
}

fn install_get_long_path_name_a() -> bool {
    use windows::core::PCSTR;
    use windows::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};

    unsafe {
        let dll = CString::new("kernel32.dll").unwrap();
        let handle = match GetModuleHandleA(PCSTR(dll.as_ptr() as *const u8)) {
            Ok(h) if !h.is_invalid() => h,
            _ => {
                log_warn!("LayeredFS: kernel32.dll not loaded — skipping GetLongPathNameA hook");
                return false;
            }
        };

        let cname = CString::new("GetLongPathNameA").unwrap();
        let target_addr = match GetProcAddress(handle, PCSTR(cname.as_ptr() as *const u8)) {
            Some(f) => f,
            None => {
                log_warn!("LayeredFS: GetLongPathNameA not found in kernel32");
                return false;
            }
        };

        #[allow(clippy::missing_transmute_annotations)]
        let target: FnGetLongPathNameA = std::mem::transmute(target_addr);
        match GenericDetour::new(target, hook_get_long_path_name_a) {
            Ok(hook) => {
                // Store BEFORE enable: the moment `enable()` patches the
                // target, any thread (AVS boot workers included) can land in
                // our callback, which reads this static. Storing after
                // enable() left a window where the callback saw `None` —
                // the 2026-07 non-deterministic boot abort.
                HOOK_GET_LONG_PATH_NAME_A = Some(hook);
                let enable_result = (*addr_of!(HOOK_GET_LONG_PATH_NAME_A))
                    .as_ref()
                    .map(|h| h.enable());
                if let Some(Err(e)) = enable_result {
                    log_warn!("LayeredFS: failed to enable GetLongPathNameA hook: {}", e);
                    HOOK_GET_LONG_PATH_NAME_A = None;
                    return false;
                }
                log_info!("LayeredFS: hooked GetLongPathNameA @ {:p}", target_addr);
                true
            }
            Err(e) => {
                log_warn!("LayeredFS: failed to create GetLongPathNameA hook: {}", e);
                false
            }
        }
    }
}

// ── Hook installation ────────────────────────────────────────────────

/// Install all AVS filesystem hooks plus the GetLongPathNameA workaround.
/// Call after AVS resolution and mod path scanning.
pub fn install_hooks() -> bool {
    let avs = super::get_avs_fns();
    let success = crate::core::hook_transaction::install_all_or_rollback(
        5,
        |index| unsafe {
            match index {
                0 => install_one(
                    "avs_fs_open",
                    avs.avs_fs_open,
                    hook_avs_fs_open,
                    &raw mut HOOK_FS_OPEN,
                ),
                1 => install_one(
                    "avs_fs_lstat",
                    avs.avs_fs_lstat,
                    hook_avs_fs_lstat,
                    &raw mut HOOK_FS_LSTAT,
                ),
                2 => install_one(
                    "avs_fs_mount",
                    avs.avs_fs_mount,
                    hook_avs_fs_mount,
                    &raw mut HOOK_FS_MOUNT,
                ),
                3 => install_one(
                    "avs_fs_read",
                    avs.avs_fs_read,
                    hook_avs_fs_read,
                    &raw mut HOOK_FS_READ,
                ),
                4 => install_one(
                    "avs_fs_convert_path",
                    avs.avs_fs_convert_path,
                    hook_avs_fs_convert_path,
                    &raw mut HOOK_FS_CONVERT_PATH,
                ),
                _ => false,
            }
        },
        |index| unsafe { disable_avs_hook(index) },
    );

    // Install best-effort: failure here doesn't break LayeredFS, just leaves
    // very long arc cache paths broken under AVS.
    install_get_long_path_name_a();

    if success {
        log_info!("LayeredFS: all AVS hooks installed");
    } else {
        log_warn!("LayeredFS: some hooks failed to install");
    }

    success
}

unsafe fn disable_one<F: retour::Function>(storage: *mut Option<GenericDetour<F>>) {
    if let Some(hook) = (&*storage).as_ref() {
        let _ = hook.disable();
    }
    *storage = None;
}

unsafe fn disable_avs_hook(index: usize) {
    match index {
        0 => disable_one(&raw mut HOOK_FS_OPEN),
        1 => disable_one(&raw mut HOOK_FS_LSTAT),
        2 => disable_one(&raw mut HOOK_FS_MOUNT),
        3 => disable_one(&raw mut HOOK_FS_READ),
        4 => disable_one(&raw mut HOOK_FS_CONVERT_PATH),
        _ => {}
    }
}

unsafe fn install_one<F: retour::Function>(
    name: &str,
    target: F,
    detour: F,
    storage: *mut Option<GenericDetour<F>>,
) -> bool {
    match GenericDetour::new(target, detour) {
        Ok(hook) => {
            // Store BEFORE enable: once `enable()` patches the target, any
            // thread can land in our callback, which reads `storage`. The
            // old enable-then-store order left a window where the callback
            // saw `None` (the 2026-07 non-deterministic boot abort).
            *storage = Some(hook);
            let enable_result = (*storage).as_ref().map(|h| h.enable());
            if let Some(Err(e)) = enable_result {
                log_warn!("LayeredFS: failed to enable {} hook: {}", name, e);
                *storage = None;
                return false;
            }
            log_info!(
                "LayeredFS: hooked {} @ {:p}",
                name,
                *(&target as *const F as *const *const ())
            );
            true
        }
        Err(e) => {
            log_warn!("LayeredFS: failed to create {} hook: {}", name, e);
            false
        }
    }
}
