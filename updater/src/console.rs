//! Console ownership (design R19): when the updater is the ONLY process
//! attached to its console it was double-clicked from Explorer, and the window
//! would vanish with the output the moment we exit — so we wait for Enter.
//! Launched from `gamestart.bat`, `cmd.exe` shares the console (count ≥ 2) and
//! we never wait; on non-Windows hosts we never wait either. The wait is
//! bounded (`WAIT_CAP`) so an unattended machine that somehow lands here can
//! never be held — the game must start regardless (design R20).

use std::io::{self, BufRead, Write};
use std::sync::mpsc;
use std::time::Duration;

/// Upper bound on the Enter-wait.
pub const WAIT_CAP: Duration = Duration::from_secs(60);

/// Pure decision: wait iff this process is alone on the console.
pub fn should_wait(process_count: u32) -> bool {
    process_count == 1
}

#[cfg(windows)]
fn console_process_count() -> u32 {
    use windows_sys::Win32::System::Console::GetConsoleProcessList;
    // A buffer of 2 is enough: we only care whether the count is 1.
    let mut ids = [0u32; 2];
    // SAFETY: `ids` is a valid, writable buffer of the length passed.
    let n = unsafe { GetConsoleProcessList(ids.as_mut_ptr(), ids.len() as u32) };
    // 0 = failure (no console at all, e.g. redirected/detached): don't wait.
    n
}

#[cfg(not(windows))]
fn console_process_count() -> u32 {
    0
}

/// True when the updater owns its console alone (double-click launch).
pub fn owns_console_alone() -> bool {
    should_wait(console_process_count())
}

/// Print the prompt and block until Enter, EOF, or [`WAIT_CAP`] elapses.
pub fn wait_for_enter() {
    print!(
        "Press Enter to close (closes by itself in {} s)...",
        WAIT_CAP.as_secs()
    );
    let _ = io::stdout().flush();
    // stdin has no timed read; park the blocking read on a thread that dies
    // with the process.
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let _ = io::stdin().lock().read_line(&mut line);
        let _ = tx.send(());
    });
    let _ = rx.recv_timeout(WAIT_CAP);
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wait_only_when_alone() {
        assert!(!should_wait(0), "no console / query failed");
        assert!(should_wait(1));
        assert!(!should_wait(2), "cmd.exe shares the console");
        assert!(!should_wait(7));
    }
}
