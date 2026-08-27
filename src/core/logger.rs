//! Logger — OutputDebugStringA backend with [DDR-Hook] prefix.
//!
//! Output visible in DebugView or x64dbg's log window.

use std::ffi::CString;
use std::sync::atomic::{AtomicU8, Ordering};
use windows::core::PCSTR;
use windows::Win32::System::Diagnostics::Debug::OutputDebugStringA;

const PREFIX: &str = "[DDR-Hook]";

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Debug = 0,
    Info = 1,
    Warn = 2,
    Error = 3,
}

static CURRENT_LEVEL: AtomicU8 = AtomicU8::new(LogLevel::Info as u8);

pub fn set_log_level(level: LogLevel) {
    CURRENT_LEVEL.store(level as u8, Ordering::Relaxed);
}

pub fn log(level: LogLevel, message: &str) {
    if (level as u8) < CURRENT_LEVEL.load(Ordering::Relaxed) {
        return;
    }
    let tag = match level {
        LogLevel::Debug => "DEBUG",
        LogLevel::Info => "INFO",
        LogLevel::Warn => "WARN",
        LogLevel::Error => "ERROR",
    };
    let formatted = format!("{PREFIX}[{tag}] {message}\n");
    if let Ok(cstr) = CString::new(formatted) {
        unsafe { OutputDebugStringA(PCSTR(cstr.as_ptr() as *const u8)) };
    }
}

/// Install a process-wide panic hook that routes panic details through our
/// `OutputDebugStringA` channel at ERROR level. Without this, a panic's message
/// goes to stderr, which spice2x doesn't capture — so panics (whether they
/// abort at an FFI boundary or are caught by a hook's `catch_unwind`) leave no
/// trace in the log. With it, every panic logs its message, source location,
/// and thread name/id *before* any `catch_unwind` swallows the payload,
/// pinpointing latent panic sites in hook callbacks. Idempotent — installs once.
pub fn install_panic_hook() {
    use std::sync::Once;
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        // Preserve the previous hook (default) and chain to it, so behavior on
        // an uncaught panic (abort) is unchanged aside from our added logging.
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let location = info
                .location()
                .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
                .unwrap_or_else(|| "<unknown location>".to_string());
            let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
                (*s).to_string()
            } else if let Some(s) = info.payload().downcast_ref::<String>() {
                s.clone()
            } else {
                "<non-string panic payload>".to_string()
            };
            let thread = std::thread::current();
            let thread_name = thread.name().unwrap_or("<unnamed>").to_string();
            // Route through the durable crash log (flush + fsync per line) so the
            // message survives an immediate abort at an extern "C" boundary — the
            // normal OutputDebugStringA path is buffered and loses the tail.
            crate::core::crash_handler::crash_log(&format!(
                "[DDR-Hook] PANIC at {location} on thread '{thread_name}' ({:?}): {msg}",
                thread.id()
            ));
            prev(info);
        }));
    });
}

#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => { $crate::core::logger::log($crate::core::logger::LogLevel::Info, &format!($($arg)*)) };
}
#[macro_export]
macro_rules! log_warn {
    ($($arg:tt)*) => { $crate::core::logger::log($crate::core::logger::LogLevel::Warn, &format!($($arg)*)) };
}
#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => { $crate::core::logger::log($crate::core::logger::LogLevel::Error, &format!($($arg)*)) };
}
#[macro_export]
macro_rules! log_debug {
    ($($arg:tt)*) => { $crate::core::logger::log($crate::core::logger::LogLevel::Debug, &format!($($arg)*)) };
}
