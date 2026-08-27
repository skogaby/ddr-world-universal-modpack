//! mfplat `MFInitMediaTypeFromVideoInfoHeader` subtype fix (Wine-only).
//!
//! ## The bug this works around
//!
//! Win10 `wmvdecod.dll` (the native WMV/VC-1 decoder DMO installed into the
//! CrossOver bottle for background-movie decode) validates its compressed
//! input type by converting the DirectShow `VIDEOINFOHEADER` to an
//! `IMFMediaType` via `MFInitMediaTypeFromVideoInfoHeader(mt, vih, size,
//! subtype=NULL)`. On Windows, a NULL `subtype` makes mfplat derive the
//! subtype from `bmiHeader.biCompression` — a FOURCC like `'WVC1'` maps to
//! the FOURCC GUID `{57564331-0000-0010-8000-00AA00389B71}`. Wine's builtin
//! mfplat (verified against wine-11.0 `dlls/mfplat/mediatype.c`) instead
//! derives it from `biBitCount` ONLY — a WVC1 header with `biBitCount=24`
//! comes back labeled `MFVideoFormat_RGB24`. The decoder is then asked to
//! accept "RGB24" as its compressed input and refuses
//! (`DMO_E_TYPE_NOT_ACCEPTED`, 0x80040205), so the WM ASF Reader's video
//! output only ever offers the compressed WVC1 stream type, which the game's
//! `MemRenderer` (RGB32/RGB565/RGB555 only) can't connect to →
//! `VFW_E_CANNOT_RENDER` → no movie.
//!
//! ## The fix
//!
//! Detour `MFInitMediaTypeFromVideoInfoHeader`; when the caller passes
//! `subtype == NULL` and `biCompression` holds a real FOURCC (not `BI_RGB`=0
//! or `BI_BITFIELDS`=3), call the original through the trampoline with the
//! FOURCC-derived subtype made explicit. All other calls (explicit subtype,
//! uncompressed RGB headers, undersized buffers) pass through untouched, so
//! behavior is byte-identical to stock Wine for every non-FOURCC caller.
//! This replicates exactly what Windows' mfplat does.
//!
//! Validated live 2026-08-19 (harness `sync-reader-fix` mode,
//! `winmm-repro-harness`): with the fix, `IWMSyncReader::GetOutputFormatCount`
//! on a stock VC-1 movie goes from `0x80040205 / 0 formats` to `S_OK / 13
//! formats` including RGB32, RGB565 and RGB555 — the exact set the game
//! renderer accepts.
//!
//! ## Scope / gating
//!
//! - **Wine-only**: installs only when `ntdll!wine_get_version` resolves.
//!   Real Windows' mfplat doesn't have the bug and is left untouched.
//! - **Idempotent**: `install()` may be called repeatedly; the detour
//!   installs once and stays installed (its semantics are strictly
//!   Windows-correct, so there is nothing to undo on mod disable).
//! - **Fail-open**: any resolution/installation failure logs one warning and
//!   reports `false`; movie playback then degrades exactly as before the fix
//!   (fallback mode's per-song "no movie" INFO).
//!
//! RE record: `docs/native_wm_runtime_bottle_setup.md` (bottle recipe +
//! root-cause trace) and `.agents/planning/20260721-non-native-os-support/`.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(windows)]
use crate::core::hooks;
#[cfg(windows)]
use crate::{log_info, log_warn};
#[cfg(windows)]
use retour::GenericDetour;
#[cfg(windows)]
use std::ptr::{addr_of, addr_of_mut};

static INSTALLED: AtomicBool = AtomicBool::new(false);

#[must_use]
pub fn is_installed() -> bool {
    INSTALLED.load(Ordering::Acquire)
}

/// GUID layout compatible with `windows::core::GUID` / Win32 `GUID`.
#[cfg(windows)]
#[repr(C)]
#[derive(Clone, Copy)]
struct Guid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

/// `HRESULT MFInitMediaTypeFromVideoInfoHeader(IMFMediaType*, const
/// VIDEOINFOHEADER*, UINT32, const GUID*)`.
#[cfg(windows)]
type VihInitFn = unsafe extern "system" fn(*mut c_void, *const u8, u32, *const Guid) -> i32;

#[cfg(windows)]
static mut VIH_HOOK: Option<GenericDetour<VihInitFn>> = None;

/// `sizeof(VIDEOINFOHEADER)` — rcSource(16) rcTarget(16) dwBitRate(4)
/// dwBitErrorRate(4) AvgTimePerFrame(8) BITMAPINFOHEADER(40).
#[cfg(windows)]
const VIH_SIZE: u32 = 88;
/// `offsetof(VIDEOINFOHEADER, bmiHeader.biCompression)` = 48 + 16.
#[cfg(windows)]
const BI_COMPRESSION_OFFSET: usize = 64;
#[cfg(windows)]
const BI_RGB: u32 = 0;
#[cfg(windows)]
const BI_BITFIELDS: u32 = 3;

#[cfg(windows)]
static SUBTYPE_LOGGED: AtomicBool = AtomicBool::new(false);

#[cfg(windows)]
unsafe extern "system" fn vih_init_hook(
    media_type: *mut c_void,
    vih: *const u8,
    size: u32,
    subtype: *const Guid,
) -> i32 {
    let Some(hook) = (&*addr_of!(VIH_HOOK)).as_ref() else {
        return -2147467259; // E_FAIL — unreachable (store precedes enable)
    };
    // Only the NULL-subtype + FOURCC-compression case is broken in Wine;
    // everything else passes through untouched.
    if subtype.is_null() && !vih.is_null() && size >= VIH_SIZE {
        let compression = std::ptr::read_unaligned(vih.add(BI_COMPRESSION_OFFSET) as *const u32);
        if compression != BI_RGB && compression != BI_BITFIELDS {
            let fourcc_subtype = Guid {
                data1: compression,
                data2: 0x0000,
                data3: 0x0010,
                data4: [0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71],
            };
            if !SUBTYPE_LOGGED.swap(true, Ordering::AcqRel) {
                let bytes = compression.to_le_bytes();
                log_info!(
                    "mfplat_vih_fix: injected FOURCC subtype {:?} ({:#010x}) for NULL-subtype VIDEOINFOHEADER",
                    core::str::from_utf8(&bytes).unwrap_or("????"),
                    compression
                );
            }
            return hook.call(media_type, vih, size, &fourcc_subtype);
        }
    }
    hook.call(media_type, vih, size, subtype)
}

/// Resolve mfplat (loading it if necessary — the decoder delay-loads it, so
/// it is usually absent at boot) and install the detour. Wine-gated,
/// idempotent, fail-open.
#[cfg(windows)]
pub fn install() -> bool {
    use windows::core::PCSTR;
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryA};

    if INSTALLED.load(Ordering::Acquire) {
        return true;
    }
    if !crate::core::platform::running_under_wine() {
        log_info!("mfplat_vih_fix: not running under Wine -- fix not needed, skipping");
        return false;
    }
    let target = unsafe {
        let Ok(mfplat) = LoadLibraryA(PCSTR(b"mfplat.dll\0".as_ptr())) else {
            log_warn!("mfplat_vih_fix: LoadLibrary(mfplat.dll) failed");
            return false;
        };
        let Some(addr) = GetProcAddress(
            mfplat,
            PCSTR(b"MFInitMediaTypeFromVideoInfoHeader\0".as_ptr()),
        ) else {
            log_warn!("mfplat_vih_fix: MFInitMediaTypeFromVideoInfoHeader export missing");
            return false;
        };
        std::mem::transmute::<_, VihInitFn>(addr)
    };
    if let Err(error) =
        unsafe { hooks::install_enabled(addr_of_mut!(VIH_HOOK), target, vih_init_hook) }
    {
        log_warn!("mfplat_vih_fix: detour installation failed: {}", error);
        return false;
    }
    INSTALLED.store(true, Ordering::Release);
    log_info!(
        "mfplat_vih_fix: MFInitMediaTypeFromVideoInfoHeader detour installed (WVC1/FOURCC subtype fix)"
    );
    true
}

#[cfg(not(windows))]
pub fn install() -> bool {
    false
}
