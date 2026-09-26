//! Cabinet identity — the ark's machine-type, licence and language queries.
//!
//! `arkMDXGetMachineType(i32* out)` (exported by the loaded ark DLL) reports
//! the cabinet class: 0 / 1 = SD cabinets, 4 = the gold cabinet (A3 picked
//! its gold-cabinet UI and menu movie on exactly that value). The export is
//! resolved on first success (retried while the ark is not loaded yet) and
//! CALLED on every query, so a later detour on it — the SMX hardware mod's
//! GOLD force — is honoured.
//!
//! `arkMDXGetLicenceKeyVersion` and `arkMDXGetGameOptionsLanguage` are the
//! game's region and language inputs (World's boot table `FUN_1800042c0`
//! binds them to the slots its area-name function `FUN_1801ae0d0` and its
//! language init `FUN_180001060` call — RE
//! `docs/ddr_selection_theme_score_sets.md` §3).
//!
//! Callers: `custom_resolution::debug_ui` (SD vs HD debug-UI tables) and
//! DDR SELECTION (AUTO: A3's own UI, gold or white; the theme stage panel's
//! area texture and area package). Any thread; fail-open (`None` when the ark
//! or its export is unavailable).

use std::ffi::CString;
use std::sync::atomic::{AtomicUsize, Ordering};

use windows::core::PCSTR;
use windows::Win32::System::LibraryLoader::GetProcAddress;

use crate::core::module_resolver::resolve_ark_module;

/// `arkMDXGet*(i32* out)`.
type GetI32Fn = unsafe extern "C" fn(*mut i32);

/// The gold cabinet's machine type.
pub const MACHINE_TYPE_GOLD: i32 = 4;

/// Resolved export addresses (0 = not resolved yet).
static MACHINE_TYPE: AtomicUsize = AtomicUsize::new(0);
static LICENCE_KEY_VERSION: AtomicUsize = AtomicUsize::new(0);
static GAME_LANGUAGE: AtomicUsize = AtomicUsize::new(0);

fn getter(slot: &AtomicUsize, export: &str) -> Option<GetI32Fn> {
    let mut addr = slot.load(Ordering::Acquire);
    if addr == 0 {
        let ark = resolve_ark_module()?;
        let name = CString::new(export).ok()?;
        addr = unsafe { GetProcAddress(ark.handle, PCSTR(name.as_ptr() as *const u8)) }? as usize;
        slot.store(addr, Ordering::Release);
    }
    Some(unsafe { std::mem::transmute::<usize, GetI32Fn>(addr) })
}

/// Call an `arkMDXGet*(i32*)` export; `None` when unavailable or negative.
fn query(slot: &AtomicUsize, export: &str) -> Option<i32> {
    let get = getter(slot, export)?;
    let mut out: i32 = -1;
    unsafe { get(&mut out) };
    (out >= 0).then_some(out)
}

/// The cabinet's machine type, or `None` when the ark export is unavailable
/// (or reports a negative value).
pub fn machine_type() -> Option<i32> {
    query(&MACHINE_TYPE, "arkMDXGetMachineType")
}

/// Whether this is the gold cabinet (machine type 4, incl. a forced GOLD).
/// `false` when the machine type is unreadable.
pub fn is_gold_cabinet() -> bool {
    machine_type() == Some(MACHINE_TYPE_GOLD)
}

/// The licence key version — the region the game's area names group by
/// (1 / 4 / 6 group the Japanese prefectures, everything but 1 / 6 the US
/// states). `None` when the export is unavailable.
pub fn licence_key_version() -> Option<i32> {
    query(&LICENCE_KEY_VERSION, "arkMDXGetLicenceKeyVersion")
}

/// The operator's game language (0 Japanese, 1 English, 2 Korean, 3 / 4
/// Chinese). `None` when the export is unavailable.
pub fn game_language() -> Option<i32> {
    query(&GAME_LANGUAGE, "arkMDXGetGameOptionsLanguage")
}
