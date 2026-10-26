//! Is the game running? The tool writes the cache the DLL's index path reads,
//! so it refuses to run while a spice2x process exists. A process snapshot
//! is enough: the game is a single well-known executable name. Non-Windows
//! hosts (tests, the maintainer's machine) always answer "no".

/// Executable names that mean "the game is up".
pub const GAME_PROCESSES: [&str; 2] = ["spice64.exe", "spice.exe"];

/// Pure decision over a list of process image names.
pub fn any_game_process<'a>(names: impl IntoIterator<Item = &'a str>) -> Option<String> {
    names
        .into_iter()
        .find(|n| GAME_PROCESSES.iter().any(|g| g.eq_ignore_ascii_case(n)))
        .map(str::to_string)
}

/// The matching process name when the game is running.
pub fn game_is_running() -> Option<String> {
    let names = process_names();
    any_game_process(names.iter().map(String::as_str))
}

#[cfg(windows)]
fn process_names() -> Vec<String> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    let mut out = Vec::new();
    // SAFETY: plain Win32 calls with a correctly sized, zeroed entry struct.
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return out;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        if Process32FirstW(snap, &mut entry) != 0 {
            loop {
                let len = entry
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szExeFile.len());
                out.push(String::from_utf16_lossy(&entry.szExeFile[..len]));
                if Process32NextW(snap, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snap);
    }
    out
}

#[cfg(not(windows))]
fn process_names() -> Vec<String> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_case_insensitively() {
        assert_eq!(
            any_game_process(["explorer.exe", "SPICE64.EXE"]),
            Some("SPICE64.EXE".to_string())
        );
        assert_eq!(
            any_game_process(["spice.exe"]),
            Some("spice.exe".to_string())
        );
        assert_eq!(any_game_process(["spice2x.exe", "cmd.exe"]), None);
        assert_eq!(any_game_process([]), None);
    }
}
