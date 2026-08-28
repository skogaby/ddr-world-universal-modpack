//! Light-output capture — detours on the `arkmdxbio2.dll` light-out exports.
//!
//! Ghidra-confirmed hook set (see the feature `progress.md`):
//!
//! - **`arkMDXChangeTapeled(off1, off2, r, g, b)`** — per-LED RGB for the 11
//!   tape devices (feet 25 each, top panel / monitor strips 50 each), linear
//!   index `off1 * 50 + off2`, identical to spice2x's
//!   `ac_io_bi2a_control_tapeled_bright` decode.
//! - **`arkMDXChangeDimlamp(id, value)`** — the 29 dimmable lamps; ids
//!   21..=24 are P1's stage corners and 25..=28 P2's (mdxf order
//!   `[UP_RIGHT, DOWN_LEFT, UP_LEFT, DOWN_RIGHT]`), values 0..255. The
//!   woofer corners / menu lamps ride the same array (Step 2 reads them
//!   from the same capture — no new hook needed).
//! - **`arkMDXChangeSatellite(device, r, g, b, led_mask_u64)`** — masked
//!   color set: bit N of the 64-bit mask = LED N of the device (all-ones =
//!   whole-device fill). Cabinet-caught 2026-08-27: this is the export the
//!   game actually drives tape effects through on this build (Tapeled and
//!   Dimlamp never fire in-game) — the earlier capture misread the mask as
//!   an LED index (garble), and removing it killed all lights. Channel
//!   values ≥ 0x100 mean "leave unchanged".
//!
//! First-calls diagnostics: each export logs its first few raw calls at
//! INFO so a cabinet log empirically maps which exports carry the GOLD
//! light traffic (validation aid; near-zero steady-state cost).
//!
//! Detour bodies forward to the original first, then copy the arguments
//! into the transport's shared `DdrLightFrame` (hot-path tight: a bounds
//! check + a few byte writes under an uncontended mutex). Capture is gated
//! on [`set_capture_enabled`] so a disabled mod costs one atomic load.
//! Detours are installed once and never removed (repo rule: one detour per
//! target, never uninstalled at runtime).

use std::ffi::CString;
use std::ptr::addr_of;
use std::sync::atomic::{AtomicBool, Ordering};

use retour::GenericDetour;
use windows::core::PCSTR;
use windows::Win32::System::LibraryLoader::GetProcAddress;

use crate::core::hooks;
use crate::core::module_resolver::resolve_ark_module;
use crate::services::smx::transport;
use crate::{log_info, log_warn};

/// `arkMDXChangeTapeled(off1, off2, r, g, b)`.
type TapeledFn = unsafe extern "C" fn(i32, i32, u32, u32, u32) -> u64;
/// `arkMDXChangeDimlamp(id, value)`.
type DimlampFn = unsafe extern "C" fn(i32, u32) -> u64;
/// `arkMDXChangeSatellite(device, r, g, b, led_mask)`.
type SatelliteFn = unsafe extern "C" fn(i32, u32, u32, u32, u64) -> u64;
/// `arkMDXSetLamp(id, on)` — observed only (first-calls diagnostics).
type SetLampFn = unsafe extern "C" fn(i32, u8) -> u64;

static mut TAPELED_DETOUR: Option<GenericDetour<TapeledFn>> = None;
static mut DIMLAMP_DETOUR: Option<GenericDetour<DimlampFn>> = None;
static mut SATELLITE_DETOUR: Option<GenericDetour<SatelliteFn>> = None;
static mut SETLAMP_DETOUR: Option<GenericDetour<SetLampFn>> = None;

/// First-calls diagnostics: log the first N raw calls per export so a
/// cabinet log maps the live light-out traffic. Cheap after the budget
/// empties (one atomic load).
const DIAG_CALL_BUDGET: usize = 6;

macro_rules! diag_first_calls {
    ($counter:ident, $($fmtargs:tt)*) => {{
        static $counter: std::sync::atomic::AtomicUsize =
            std::sync::atomic::AtomicUsize::new(0);
        let n = $counter.load(Ordering::Relaxed);
        if n < DIAG_CALL_BUDGET {
            $counter.store(n + 1, Ordering::Relaxed);
            log_info!($($fmtargs)*);
        }
    }};
}

/// True once the detours are installed (never reset — hooks stay for the
/// process lifetime).
static INSTALLED: AtomicBool = AtomicBool::new(false);
/// Gates capture: false ⇒ the detour bodies are pure passthrough.
static CAPTURE_ENABLED: AtomicBool = AtomicBool::new(false);

unsafe extern "C" fn tapeled_detour(off1: i32, off2: i32, r: u32, g: u32, b: u32) -> u64 {
    let result = std::panic::catch_unwind(|| {
        let ret = match (*addr_of!(TAPELED_DETOUR)).as_ref() {
            Some(hook) => hook.call(off1, off2, r, g, b),
            None => 0,
        };
        diag_first_calls!(
            TAPELED_CALLS,
            "SMX diag: tapeled(group={}, led={}, r={:#x}, g={:#x}, b={:#x})",
            off1,
            off2,
            r,
            g,
            b
        );
        if CAPTURE_ENABLED.load(Ordering::Acquire) && off1 >= 0 && off2 >= 0 {
            // Linear index off1*50+off2 → (device, led), spice2x's table:
            // off1 0..=3 = foot pairs split at led 25; 5..=7 = 50-LED strips.
            let (device, led) = match off1 {
                0..=3 => {
                    if off2 < 25 {
                        ((off1 * 2) as usize, off2 as usize)
                    } else {
                        ((off1 * 2 + 1) as usize, (off2 - 25) as usize)
                    }
                }
                5..=7 => ((off1 + 3) as usize, off2 as usize),
                _ => return ret,
            };
            transport::write_tape_led(device, led, r, g, b);
        }
        ret
    });
    result.unwrap_or(0)
}

unsafe extern "C" fn dimlamp_detour(id: i32, value: u32) -> u64 {
    let result = std::panic::catch_unwind(|| {
        let ret = match (*addr_of!(DIMLAMP_DETOUR)).as_ref() {
            Some(hook) => hook.call(id, value),
            None => 0,
        };
        diag_first_calls!(
            DIMLAMP_CALLS,
            "SMX diag: dimlamp(id={}, value={:#x})",
            id,
            value
        );
        if CAPTURE_ENABLED.load(Ordering::Acquire) && id >= 0 {
            transport::write_dimlamp(id as usize, value.min(255) as u8);
        }
        ret
    });
    result.unwrap_or(0)
}

unsafe extern "C" fn satellite_detour(device: i32, r: u32, g: u32, b: u32, mask: u64) -> u64 {
    let result = std::panic::catch_unwind(|| {
        let ret = match (*addr_of!(SATELLITE_DETOUR)).as_ref() {
            Some(hook) => hook.call(device, r, g, b, mask),
            None => 0,
        };
        diag_first_calls!(
            SATELLITE_CALLS,
            "SMX diag: satellite(device={}, r={:#x}, g={:#x}, b={:#x}, mask={:#x})",
            device,
            r,
            g,
            b,
            mask
        );
        // In GOLD cabinet mode (the cabinet-force default) the game drives
        // arrows/corners through Tapeled + Dimlamp, and satellite carries
        // only cabinet-light effects + boot-clear whole-device fills — writing
        // those into the tape frame would wipe the legitimate tapeled arrow
        // data. Only fold satellite into the tape when the GOLD force is NOT
        // active (i.e. an operator turned it off and the game is genuinely on
        // the SD/satellite light path).
        if CAPTURE_ENABLED.load(Ordering::Acquire)
            && device >= 0
            && !super::cabinet_force::is_forcing()
        {
            transport::fill_tape_device_masked(device as usize, r, g, b, mask);
        }
        ret
    });
    result.unwrap_or(0)
}

unsafe extern "C" fn setlamp_detour(id: i32, on: u8) -> u64 {
    let result = std::panic::catch_unwind(|| {
        let ret = match (*addr_of!(SETLAMP_DETOUR)).as_ref() {
            Some(hook) => hook.call(id, on),
            None => 0,
        };
        // Observation only for now (maps the lamp-check / corner traffic).
        diag_first_calls!(SETLAMP_CALLS, "SMX diag: setlamp(id={}, on={})", id, on);
        ret
    });
    result.unwrap_or(0)
}

/// Resolve the light-out exports and install the capture detours (once).
/// Returns true when the load-bearing pair (Tapeled + Dimlamp) is hooked.
pub fn install() -> bool {
    if INSTALLED.load(Ordering::Acquire) {
        return true;
    }
    let Some(ark_module) = resolve_ark_module() else {
        log_warn!("SmxHardware: ark module unavailable -- lights capture disabled");
        return false;
    };
    let resolve = |name: &str| -> Option<*const ()> {
        let cname = CString::new(name).ok()?;
        let addr =
            unsafe { GetProcAddress(ark_module.handle, PCSTR(cname.as_ptr() as *const u8)) }?;
        Some(addr as *const ())
    };

    let (Some(tapeled), Some(dimlamp)) = (
        resolve("arkMDXChangeTapeled"),
        resolve("arkMDXChangeDimlamp"),
    ) else {
        log_warn!(
            "SmxHardware: arkMDXChangeTapeled/Dimlamp exports unavailable -- lights capture disabled"
        );
        return false;
    };

    unsafe {
        let tapeled: TapeledFn = std::mem::transmute(tapeled);
        if let Err(e) = hooks::install_enabled(
            std::ptr::addr_of_mut!(TAPELED_DETOUR),
            tapeled,
            tapeled_detour as TapeledFn,
        ) {
            log_warn!("SmxHardware: arkMDXChangeTapeled detour failed: {}", e);
            return false;
        }
        let dimlamp: DimlampFn = std::mem::transmute(dimlamp);
        if let Err(e) = hooks::install_enabled(
            std::ptr::addr_of_mut!(DIMLAMP_DETOUR),
            dimlamp,
            dimlamp_detour as DimlampFn,
        ) {
            log_warn!("SmxHardware: arkMDXChangeDimlamp detour failed: {}", e);
            return false;
        }
        // Satellite: the masked-set path the game actually drives tape
        // effects through on current builds. Best-effort.
        if let Some(satellite) = resolve("arkMDXChangeSatellite") {
            let satellite: SatelliteFn = std::mem::transmute(satellite);
            if let Err(e) = hooks::install_enabled(
                std::ptr::addr_of_mut!(SATELLITE_DETOUR),
                satellite,
                satellite_detour as SatelliteFn,
            ) {
                log_warn!(
                    "SmxHardware: arkMDXChangeSatellite detour failed (tape effects uncaptured): {}",
                    e
                );
            }
        }
        // SetLamp: observation-only diagnostics (maps lamp-check traffic).
        if let Some(setlamp) = resolve("arkMDXSetLamp") {
            let setlamp: SetLampFn = std::mem::transmute(setlamp);
            if let Err(e) = hooks::install_enabled(
                std::ptr::addr_of_mut!(SETLAMP_DETOUR),
                setlamp,
                setlamp_detour as SetLampFn,
            ) {
                log_warn!("SmxHardware: arkMDXSetLamp observer failed: {}", e);
            }
        }
    }

    INSTALLED.store(true, Ordering::Release);
    log_info!("SmxHardware: light-output capture detours installed");
    true
}

/// Gate the capture (the detours themselves stay installed).
pub fn set_capture_enabled(enabled: bool) {
    CAPTURE_ENABLED.store(enabled, Ordering::Release);
}

/// Whether the load-bearing detours are installed.
pub fn is_installed() -> bool {
    INSTALLED.load(Ordering::Acquire)
}
