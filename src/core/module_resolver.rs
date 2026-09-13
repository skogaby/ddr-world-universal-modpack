//! Module Resolver — Finds gamemdx.dll, arkmdxbio2.dll and the libavs DLLs in
//! process memory.

use std::ffi::CString;
use windows::core::PCSTR;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::System::LibraryLoader::GetModuleHandleA;
use windows::Win32::System::ProcessStatus::{GetModuleInformation, MODULEINFO};
use windows::Win32::System::Threading::GetCurrentProcess;

pub struct GameModule {
    pub name: String,
    pub base: *const u8,
    pub size: usize,
    pub handle: HMODULE,
}

unsafe impl Send for GameModule {}
unsafe impl Sync for GameModule {}

const GAME_MODULE_NAME: &str = "gamemdx.dll";
const ARK_DLL_NAMES: &[&str] = &["arkmdxbio2.dll", "arkmdxp3.dll", "arkmdxp4.dll"];
/// AVS core (fs + property API). `avs2-core.dll` is the legacy name.
const LIBAVS_DLL_NAMES: &[&str] = &["libavs-win64.dll", "libavs-win32.dll", "avs2-core.dll"];
/// AVS eamuse library (`ea3_boot`, xrpc, eacoin). `avs2-ea3.dll` is the
/// legacy name spice2x also probes for.
const LIBAVS_EA3_DLL_NAMES: &[&str] = &[
    "libavs-win64-ea3.dll",
    "libavs-win32-ea3.dll",
    "avs2-ea3.dll",
];

pub fn wait_for_game_module() -> GameModule {
    loop {
        if let Some(m) = resolve_module(GAME_MODULE_NAME) {
            return m;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

pub fn get_game_module() -> Option<GameModule> {
    resolve_module(GAME_MODULE_NAME)
}

pub fn resolve_ark_module() -> Option<GameModule> {
    for name in ARK_DLL_NAMES {
        if let Some(m) = resolve_module(name) {
            return Some(m);
        }
    }
    None
}

/// The loaded AVS core DLL (`libavs-win64.dll`), if any.
pub fn resolve_libavs_module() -> Option<GameModule> {
    LIBAVS_DLL_NAMES
        .iter()
        .find_map(|name| resolve_module(name))
}

/// The loaded AVS eamuse DLL (`libavs-win64-ea3.dll`), if any.
pub fn resolve_libavs_ea3_module() -> Option<GameModule> {
    LIBAVS_EA3_DLL_NAMES
        .iter()
        .find_map(|name| resolve_module(name))
}

fn resolve_module(name: &str) -> Option<GameModule> {
    let cname = CString::new(name).ok()?;
    unsafe {
        let handle = GetModuleHandleA(PCSTR(cname.as_ptr() as *const u8)).ok()?;
        if handle.is_invalid() {
            return None;
        }
        let mut info = MODULEINFO::default();
        let ok = GetModuleInformation(
            GetCurrentProcess(),
            handle,
            &mut info,
            std::mem::size_of::<MODULEINFO>() as u32,
        );
        if ok.is_err() {
            return None;
        }
        Some(GameModule {
            name: name.to_string(),
            base: info.lpBaseOfDll as *const u8,
            size: info.SizeOfImage as usize,
            handle,
        })
    }
}
