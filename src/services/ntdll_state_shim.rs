//! ntdll `RtlGetPersistedStateLocation` shim for native quartz (Wine-only).
//!
//! **STATUS: RETAINED BUT UNCALLED.** This module supported the
//! native-quartz bottle experiment (a SetRate-capable filter-graph manager
//! under CrossOver), which was ABANDONED 2026-08-21: the shim itself worked
//! (live-verified `quartz.dll IAT patched ... pre-DllMain`), but native
//! quartz then hard-locked the game building its first graph (intelligent
//! connect → default Video Renderer → wined3d deadlock against the game's
//! live device — beyond safe in-process fixing). See
//! `docs/native_wm_runtime_bottle_setup.md` §2.9 for the full record.
//! The module is kept as the proven implementation of a generally useful
//! pattern: **fixing a Wine `@ stub` import that has NO export to detour,
//! via `LdrRegisterDllNotification` + a pre-DllMain IAT patch on the
//! importing module.**
//!
//! ## The gap this fills
//!
//! Windows-10+ system DLLs (native `quartz.dll` 19041 among them — installed
//! into the CrossOver bottle to replace Wine's builtin quartz, whose
//! `IMediaSeeking::SetRate` is a silent no-op) import
//! `ntdll!RtlGetPersistedStateLocation` and call it during `DllMain`.
//! Wine declares the function as `@ stub` in ntdll.spec: **it has no real
//! export at all** — `GetProcAddress` returns NULL, and at import-snap time
//! the loader patches a *synthesized* abort thunk into the importer's IAT
//! ("Call from ... to unimplemented function", raises EXCEPTION_WINE_STUB).
//! The DllMain exception fails the load: in-process,
//! `CoCreateInstance(CLSID_FilterGraph)` returns failure, BuildGraph exits
//! `0xC0260001`, and fallback mode degrades every movie to no-movie
//! (live-observed 2026-08-21). A conventional export detour is impossible —
//! there is nothing to detour.
//!
//! ## The fix
//!
//! Patch **quartz's own IAT** the moment it loads, before its DllMain runs:
//! `LdrRegisterDllNotification`'s LOADED callback fires after the module is
//! mapped and its imports snapped but before initialization. The callback
//! walks quartz's ntdll import descriptor, finds the
//! `RtlGetPersistedStateLocation` thunk (by name via the original first
//! thunk), and overwrites the IAT slot with a faithful "no persisted state"
//! implementation: zero the caller's out-length and return
//! `STATUS_OBJECT_NAME_NOT_FOUND` (0xC0000034). On real Windows this API
//! queries the Persisted State registry redirection — a servicing feature no
//! Wine bottle has state for — and callers treat "not found" as the normal
//! no-redirection case and proceed. This is the semantically correct answer
//! for a bottle, not a lie.
//!
//! If Wine's notification ordering ever proves to be after-DllMain (it
//! mirrors Windows' before-DllMain contract as of wine-3.16+), the load
//! keeps failing exactly as today (fallback no-movie) and the escalation
//! path is an ntdll export-table augmentation — documented in the planning
//! notes, not built until needed.
//!
//! ## Scope / gating
//!
//! - **Wine-only**: installs only when `ntdll!wine_get_version` resolves.
//! - **quartz.dll only**: the callback ignores every other module. If
//!   quartz is somehow already loaded at install time, it is patched
//!   immediately instead.
//! - **Idempotent + fail-open**: a failed registration/patch just means
//!   native quartz stays unloadable and movie playback degrades exactly as
//!   before (builtin quartz, or fallback mode's no-movie path). Installed
//!   from non-native-os-support's fallback-mode enable, alongside
//!   `mfplat_vih_fix`.
//!
//! Bottle recipe: `docs/native_wm_runtime_bottle_setup.md` §2.9 (file copy +
//! DllOverrides; no regsvr32 for quartz — its CLSIDs already point at
//! `C:\windows\system32\quartz.dll` and the override picks the binary).

use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(windows)]
use crate::{log_info, log_warn};
#[cfg(windows)]
use std::ffi::c_void;

static INSTALLED: AtomicBool = AtomicBool::new(false);
static PATCHED: AtomicBool = AtomicBool::new(false);

#[must_use]
pub fn is_installed() -> bool {
    INSTALLED.load(Ordering::Acquire)
}

/// Whether quartz's IAT slot has actually been patched (i.e. native quartz
/// loaded and the import was found).
#[must_use]
pub fn is_patched() -> bool {
    PATCHED.load(Ordering::Acquire)
}

/// `STATUS_OBJECT_NAME_NOT_FOUND` — the documented "no redirection
/// configured" answer callers already handle on real Windows.
#[cfg(windows)]
const STATUS_OBJECT_NAME_NOT_FOUND: i32 = 0xC000_0034_u32 as i32;

#[cfg(windows)]
const TARGET_IMPORT: &[u8] = b"RtlGetPersistedStateLocation\0";
#[cfg(windows)]
const TARGET_MODULE_UTF16: &[u16] = &[
    b'q' as u16,
    b'u' as u16,
    b'a' as u16,
    b'r' as u16,
    b't' as u16,
    b'z' as u16,
    b'.' as u16,
    b'd' as u16,
    b'l' as u16,
    b'l' as u16,
];

/// The replacement implementation quartz's IAT slot is pointed at.
/// `NTSTATUS RtlGetPersistedStateLocation(PCWSTR SourceID, PCWSTR
/// CustomValue, PCWSTR DefaultPath, STATE_LOCATION_TYPE StateLocationType,
/// PWCHAR TargetPath, ULONG BufferLengthIn, PULONG BufferLengthOut)`.
#[cfg(windows)]
unsafe extern "system" fn persisted_state_shim(
    _source_id: *const u16,
    _custom_value: *const u16,
    _default_path: *const u16,
    _location_type: u32,
    _target_path: *mut u16,
    _buffer_length_in: u32,
    buffer_length_out: *mut u32,
) -> i32 {
    if !buffer_length_out.is_null() {
        *buffer_length_out = 0;
    }
    STATUS_OBJECT_NAME_NOT_FOUND
}

// ── Ldr notification plumbing ────────────────────────────────────────────

#[cfg(windows)]
#[repr(C)]
struct UnicodeString {
    length: u16,
    maximum_length: u16,
    buffer: *const u16,
}

/// `LDR_DLL_LOADED_NOTIFICATION_DATA` / `..._UNLOADED_...` share this shape.
#[cfg(windows)]
#[repr(C)]
struct LdrDllNotificationData {
    flags: u32,
    full_dll_name: *const UnicodeString,
    base_dll_name: *const UnicodeString,
    dll_base: *mut c_void,
    size_of_image: u32,
}

#[cfg(windows)]
const LDR_DLL_NOTIFICATION_REASON_LOADED: u32 = 1;

#[cfg(windows)]
type LdrRegisterDllNotificationFn = unsafe extern "system" fn(
    u32,
    unsafe extern "system" fn(u32, *const LdrDllNotificationData, *mut c_void),
    *mut c_void,
    *mut *mut c_void,
) -> i32;

#[cfg(windows)]
unsafe extern "system" fn dll_notification(
    reason: u32,
    data: *const LdrDllNotificationData,
    _context: *mut c_void,
) {
    if reason != LDR_DLL_NOTIFICATION_REASON_LOADED || data.is_null() {
        return;
    }
    let name = (*data).base_dll_name;
    if name.is_null() || (*data).dll_base.is_null() {
        return;
    }
    let len = usize::from((*name).length) / 2;
    if len != TARGET_MODULE_UTF16.len() || (*name).buffer.is_null() {
        return;
    }
    let chars = std::slice::from_raw_parts((*name).buffer, len);
    let matches = chars
        .iter()
        .zip(TARGET_MODULE_UTF16)
        .all(|(&c, &t)| to_lower_u16(c) == t);
    if !matches {
        return;
    }
    // quartz.dll just mapped + snapped; DllMain has not run yet. Patch.
    patch_module_iat((*data).dll_base as *mut u8);
}

#[cfg(windows)]
fn to_lower_u16(c: u16) -> u16 {
    if (b'A' as u16..=b'Z' as u16).contains(&c) {
        c + 32
    } else {
        c
    }
}

/// Walk `module`'s import descriptors; for the ntdll descriptor, find the
/// `RtlGetPersistedStateLocation` thunk by name (via the original first
/// thunk / INT) and point the corresponding IAT slot at the shim.
///
/// # Safety
/// `module` is the base of a fully MAPPED PE image whose imports are
/// snapped (guaranteed inside the LOADED notification).
#[cfg(windows)]
unsafe fn patch_module_iat(module: *mut u8) {
    use windows::Win32::System::Memory::{VirtualProtect, PAGE_PROTECTION_FLAGS, PAGE_READWRITE};

    // PE walk: DOS → NT headers → DataDirectory[1] (imports).
    let e_lfanew = std::ptr::read_unaligned(module.add(0x3C) as *const u32) as usize;
    let nt = module.add(e_lfanew);
    if std::ptr::read_unaligned(nt as *const u32) != 0x0000_4550 {
        log_warn!("ntdll_state_shim: quartz PE signature mismatch -- not patched");
        return;
    }
    // OptionalHeader64 starts at nt+0x18; DataDirectory at +0x70 within it.
    let import_dir_rva = std::ptr::read_unaligned(nt.add(0x18 + 0x78) as *const u32) as usize;
    if import_dir_rva == 0 {
        log_warn!("ntdll_state_shim: quartz has no import directory -- not patched");
        return;
    }
    let mut desc = module.add(import_dir_rva);
    loop {
        // IMAGE_IMPORT_DESCRIPTOR: +0x00 OriginalFirstThunk, +0x0C Name,
        // +0x10 FirstThunk (all RVAs); all-zero terminator.
        let original_first_thunk = std::ptr::read_unaligned(desc as *const u32) as usize;
        let name_rva = std::ptr::read_unaligned(desc.add(0x0C) as *const u32) as usize;
        let first_thunk = std::ptr::read_unaligned(desc.add(0x10) as *const u32) as usize;
        if name_rva == 0 && first_thunk == 0 {
            break;
        }
        if name_rva != 0 && first_thunk != 0 && dll_name_is_ntdll(module.add(name_rva)) {
            // Prefer the INT (unmodified names); fall back to the IAT RVA
            // if the linker omitted it (rare).
            let int_rva = if original_first_thunk != 0 {
                original_first_thunk
            } else {
                first_thunk
            };
            let mut index = 0usize;
            loop {
                let entry = std::ptr::read_unaligned(module.add(int_rva + index * 8) as *const u64);
                if entry == 0 {
                    break;
                }
                // Skip ordinal imports (bit 63).
                if entry & (1 << 63) == 0 {
                    // IMAGE_IMPORT_BY_NAME: u16 hint, then the name.
                    let import_name = module.add(entry as u32 as usize + 2);
                    if name_matches(import_name, TARGET_IMPORT) {
                        let slot = module.add(first_thunk + index * 8) as *mut u64;
                        let mut old = PAGE_PROTECTION_FLAGS(0);
                        if VirtualProtect(slot as *const c_void, 8, PAGE_READWRITE, &mut old)
                            .is_err()
                        {
                            log_warn!(
                                "ntdll_state_shim: VirtualProtect on quartz IAT failed -- not patched"
                            );
                            return;
                        }
                        *slot = persisted_state_shim as *const () as u64;
                        let _ = VirtualProtect(slot as *const c_void, 8, old, &mut old);
                        PATCHED.store(true, Ordering::Release);
                        log_info!(
                            "ntdll_state_shim: quartz.dll IAT patched (RtlGetPersistedStateLocation -> shim) pre-DllMain"
                        );
                        return;
                    }
                }
                index += 1;
            }
        }
        desc = desc.add(20); // sizeof(IMAGE_IMPORT_DESCRIPTOR)
    }
    log_warn!(
        "ntdll_state_shim: quartz.dll loaded but RtlGetPersistedStateLocation import not found \
         (different quartz build? nothing patched)"
    );
}

/// # Safety
/// `name` points at a NUL-terminated ASCII string inside a mapped image.
#[cfg(windows)]
unsafe fn dll_name_is_ntdll(name: *const u8) -> bool {
    const NTDLL: &[u8] = b"ntdll.dll\0";
    for (i, &expect) in NTDLL.iter().enumerate() {
        let c = *name.add(i);
        let c = if c.is_ascii_uppercase() { c + 32 } else { c };
        if c != expect {
            return false;
        }
        if expect == 0 {
            break;
        }
    }
    true
}

/// # Safety
/// `name` points at a NUL-terminated ASCII string inside a mapped image;
/// `expect` includes the trailing NUL.
#[cfg(windows)]
unsafe fn name_matches(name: *const u8, expect: &[u8]) -> bool {
    for (i, &e) in expect.iter().enumerate() {
        if *name.add(i) != e {
            return false;
        }
    }
    true
}

/// Register the loader notification (patching quartz immediately if it is
/// somehow already resident). Wine-gated, idempotent, fail-open.
#[cfg(windows)]
pub fn install() -> bool {
    use windows::core::PCSTR;
    use windows::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};

    if INSTALLED.load(Ordering::Acquire) {
        return true;
    }
    if !crate::core::platform::running_under_wine() {
        log_info!("ntdll_state_shim: not running under Wine -- shim not needed, skipping");
        return false;
    }
    unsafe {
        let Ok(ntdll) = GetModuleHandleA(PCSTR(b"ntdll.dll\0".as_ptr())) else {
            log_warn!("ntdll_state_shim: GetModuleHandle(ntdll.dll) failed");
            return false;
        };
        let Some(register) = GetProcAddress(ntdll, PCSTR(b"LdrRegisterDllNotification\0".as_ptr()))
        else {
            log_warn!("ntdll_state_shim: LdrRegisterDllNotification unavailable -- native quartz stays unloadable");
            return false;
        };
        let register: LdrRegisterDllNotificationFn = std::mem::transmute::<
            *const c_void,
            LdrRegisterDllNotificationFn,
        >(register as *const c_void);
        let mut cookie: *mut c_void = std::ptr::null_mut();
        let status = register(0, dll_notification, std::ptr::null_mut(), &mut cookie);
        if status < 0 {
            log_warn!(
                "ntdll_state_shim: LdrRegisterDllNotification failed ({:#010x})",
                status
            );
            return false;
        }
        // Already resident (defensive; normal flow loads it at the first
        // movie open, well after install).
        if let Ok(quartz) = GetModuleHandleA(PCSTR(b"quartz.dll\0".as_ptr())) {
            if !quartz.is_invalid() {
                patch_module_iat(quartz.0 as *mut u8);
            }
        }
    }
    INSTALLED.store(true, Ordering::Release);
    log_info!(
        "ntdll_state_shim: DLL-load notification registered (quartz.dll IAT patch armed for \
         RtlGetPersistedStateLocation)"
    );
    true
}

#[cfg(not(windows))]
pub fn install() -> bool {
    false
}
