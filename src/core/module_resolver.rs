//! Module Resolver — Finds gamemdx.dll and arkmdxbio2.dll in process memory.

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
