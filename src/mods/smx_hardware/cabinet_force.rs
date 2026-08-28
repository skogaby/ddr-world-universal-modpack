//! Cabinet-mode force — makes the game drive the **Gold-Cab** light path.
//!
//! ## Why this exists (Ghidra-confirmed, see the feature `progress.md`)
//!
//! gamemdx's per-frame light dispatcher (`FUN_18000fcf0` @ 20260721) picks
//! its light-output API from a cabinet-family classifier
//! (`FUN_1800135e0`) that reads two `arkmdxbio2` exports:
//!
//! - `arkMDXGetMachineType` (== 4 ⇒ GOLD) and
//! - `arkMDXGetPCType` (∈ {2,3,4}).
//!
//! Only when **machineType == 4 AND pcType ≥ 2** does it take the GOLD light
//! state machine (`FUN_180012720`), which emits per-LED arrow tape via
//! `arkMDXChangeTapeled` + stage corners via `arkMDXChangeDimlamp` — the
//! exports our [`super::lights_read`] capture + [`crate::services::smx::light_map`]
//! already decode correctly. Otherwise it falls to the **satellite** path
//! (`arkMDXChangeSatellite`), whose device space is NOT the tape table and
//! which never emits the per-arrow / corner data we need.
//!
//! On this cabinet the ark's *internal* flush (`MdxHWIO::stepUpdate`) already
//! runs the machine-type-4 GOLD branch (it reads the tape + dimlamp buffers
//! and emits `ac_io_bi2a_control_tapeled_bright` — this is what SpiceManiaX
//! consumed over SpiceAPI). But gamemdx's *export* view of the machine type
//! is forced to 1 by `MdxHWIO::GetMachineType` (`FUN_1800c9320`, which returns
//! 1 when the `+0x5ee` "force SD" flag is set) / downgraded to 3 by the
//! backend getter's `DAT_180c47f69` flag — so gamemdx drives satellite and
//! never populates the tape/dimlamp buffers.
//!
//! Setting `<io>bio2</io>` in `app-config.xml` is NOT a fix: spice2x only
//! fakes the BIO2 USB probe (`FUN_1800d0dd0`, PID_804C/8050) when the ea3
//! `spec` starts with `'I'`; on DDR World's spec `F` the probe fails and the
//! ark raises a `specification.i` boot error (the game dies at boot,
//! cabinet-confirmed 2026-08-27).
//!
//! ## The fix
//!
//! Detour the two exports and force the outputs to GOLD
//! (`machineType = 4`, `pcType = max(pcType, 2)`) so gamemdx's dispatcher
//! selects the GOLD light path. This is override-agnostic — it wins over
//! whichever of the two SD/white overrides is active — and it aligns
//! gamemdx's view with the ark's already-GOLD internal flush. On a
//! Gold/Universal cabinet (identical light layout) GOLD is the *correct*
//! answer everywhere, so forcing it is a repair, not a spoof.
//!
//! Detours forward to the original first (preserving all side effects,
//! including `arkMDXGetPCType`'s one-shot processor-type cache), then patch
//! the out-param. Gated on [`set_force_enabled`]; installed once and never
//! removed (repo rule: one detour per target).

use std::ffi::CString;
use std::ptr::addr_of;
use std::sync::atomic::{AtomicBool, Ordering};

use retour::GenericDetour;
use windows::core::PCSTR;
use windows::Win32::System::LibraryLoader::GetProcAddress;

use crate::core::hooks;
use crate::core::module_resolver::resolve_ark_module;
use crate::{log_info, log_warn};

/// `arkMDXGetMachineType(i32* out)` — writes the machine type through `out`.
/// The `arkmdxbio2` export forwards `(singleton, out)` internally; the ABI
/// gamemdx calls is a single out-pointer, void return.
type GetTypeFn = unsafe extern "C" fn(*mut i32);

static mut MACHINE_TYPE_DETOUR: Option<GenericDetour<GetTypeFn>> = None;
static mut PC_TYPE_DETOUR: Option<GenericDetour<GetTypeFn>> = None;

/// GOLD machine type (classifier requires exactly 4).
const MACHINE_TYPE_GOLD: i32 = 4;
/// Minimum PC type the classifier accepts for the GOLD path.
const PC_TYPE_MIN_GOLD: i32 = 2;

static INSTALLED: AtomicBool = AtomicBool::new(false);
/// Gates the force: false ⇒ the detour bodies are pure passthrough.
static FORCE_ENABLED: AtomicBool = AtomicBool::new(false);

/// True while the GOLD-cabinet force is actively rewriting the getters.
/// [`super::lights_read`] consults this to suppress satellite→tape capture
/// (in GOLD mode gamemdx's boot-clear satellite fills would otherwise wipe
/// the legitimate tapeled arrow data).
#[inline]
pub fn is_forcing() -> bool {
    INSTALLED.load(Ordering::Acquire) && FORCE_ENABLED.load(Ordering::Acquire)
}

unsafe extern "C" fn machine_type_detour(out: *mut i32) {
    let result = std::panic::catch_unwind(|| {
        if let Some(hook) = (*addr_of!(MACHINE_TYPE_DETOUR)).as_ref() {
            hook.call(out);
        }
        if FORCE_ENABLED.load(Ordering::Acquire) && !out.is_null() {
            *out = MACHINE_TYPE_GOLD;
        }
    });
    let _ = result;
}

unsafe extern "C" fn pc_type_detour(out: *mut i32) {
    let result = std::panic::catch_unwind(|| {
        if let Some(hook) = (*addr_of!(PC_TYPE_DETOUR)).as_ref() {
            hook.call(out);
        }
        if FORCE_ENABLED.load(Ordering::Acquire) && !out.is_null() && *out < PC_TYPE_MIN_GOLD {
            *out = PC_TYPE_MIN_GOLD;
        }
    });
    let _ = result;
}

/// Resolve `arkMDXGetMachineType` / `arkMDXGetPCType` and install the force
/// detours (once). Returns true when both are hooked. On any miss/failure the
/// mod degrades to whatever cabinet mode the game auto-detects (SD/satellite),
/// with one WARN — never a hard failure (repo rule 2).
pub fn install() -> bool {
    if INSTALLED.load(Ordering::Acquire) {
        return true;
    }
    let Some(ark_module) = resolve_ark_module() else {
        log_warn!("SmxHardware: ark module unavailable -- cannot force GOLD cabinet mode");
        return false;
    };
    let resolve = |name: &str| -> Option<*const ()> {
        let cname = CString::new(name).ok()?;
        let addr =
            unsafe { GetProcAddress(ark_module.handle, PCSTR(cname.as_ptr() as *const u8)) }?;
        Some(addr as *const ())
    };

    let (Some(machine_type), Some(pc_type)) =
        (resolve("arkMDXGetMachineType"), resolve("arkMDXGetPCType"))
    else {
        log_warn!(
            "SmxHardware: arkMDXGetMachineType/PCType exports unavailable -- GOLD force disabled"
        );
        return false;
    };

    unsafe {
        let machine_type: GetTypeFn = std::mem::transmute(machine_type);
        if let Err(e) = hooks::install_enabled(
            std::ptr::addr_of_mut!(MACHINE_TYPE_DETOUR),
            machine_type,
            machine_type_detour as GetTypeFn,
        ) {
            log_warn!("SmxHardware: arkMDXGetMachineType detour failed: {}", e);
            return false;
        }
        let pc_type: GetTypeFn = std::mem::transmute(pc_type);
        if let Err(e) = hooks::install_enabled(
            std::ptr::addr_of_mut!(PC_TYPE_DETOUR),
            pc_type,
            pc_type_detour as GetTypeFn,
        ) {
            log_warn!("SmxHardware: arkMDXGetPCType detour failed: {}", e);
            return false;
        }
    }

    INSTALLED.store(true, Ordering::Release);
    log_info!("SmxHardware: GOLD cabinet-mode force detours installed");
    true
}

/// Enable/disable the force (the detours themselves stay installed).
pub fn set_force_enabled(enabled: bool) {
    FORCE_ENABLED.store(enabled, Ordering::Release);
}
