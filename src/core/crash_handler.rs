//! Crash handler — bulletproof last-resort crash diagnostics.
//!
//! Two boot crashes were seen (non-deterministic, on an AVS worker thread) that
//! left NO trace: `./log.txt` (spice2x's `OutputDebugStringA` capture) is
//! line-buffered, and when the process `abort()`s or faults immediately after
//! the offending call, the tail never flushes. Worse, a hard fault (access
//! violation) isn't a Rust panic at all, so the panic hook can't see it.
//!
//! This module closes both gaps:
//!   1. [`crash_log`] writes to a dedicated `./ddr_hook_crash.log` with an
//!      explicit flush + `sync_all` per line, so a record survives an immediate
//!      abort/fault. Also mirrored to the normal log channel.
//!   2. [`install`] registers a top-level unhandled-exception filter
//!      (`SetUnhandledExceptionFilter`) that catches HARD FAULTS — access
//!      violations, illegal instructions, etc. — which the Rust panic hook
//!      cannot. It logs the exception code, the faulting address, and whether
//!      that address lies inside our own DLL (so we know instantly whether the
//!      fault is our code or the game's), then lets the default handler run.
//!
//! Everything here is best-effort and panic-free: the filter runs in a broken
//! process, so it touches only raw pointers + a file handle, never allocates
//! through paths that could re-fault, and never itself panics across the
//! `extern "system"` boundary.

use std::ffi::c_void;
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::core::logger::{self, LogLevel};

/// Path of the dedicated crash log (next to the game exe / working dir).
const CRASH_LOG_PATH: &str = "./ddr_hook_crash.log";

/// Bounds of our own DLL, filled in at [`install`] time, so the exception
/// filter can classify a faulting address as "ours" vs "the game's" without
/// allocating or calling into module APIs from the broken-process context.
static DLL_BASE: AtomicUsize = AtomicUsize::new(0);
static DLL_END: AtomicUsize = AtomicUsize::new(0);

// ── Minimal SEH FFI (avoid pulling the Win32_System_Kernel/CONTEXT feature) ──

#[repr(C)]
struct ExceptionRecord {
    exception_code: u32,
    exception_flags: u32,
    exception_record: *mut ExceptionRecord,
    exception_address: *mut c_void,
    number_parameters: u32,
    // The real struct has [usize; 15] here; we never read it, so we stop short.
}

#[repr(C)]
struct ExceptionPointers {
    exception_record: *mut ExceptionRecord,
    context_record: *mut c_void,
}

type TopLevelFilter = Option<unsafe extern "system" fn(info: *const ExceptionPointers) -> i32>;

const EXCEPTION_CONTINUE_SEARCH: i32 = 0;

extern "system" {
    fn SetUnhandledExceptionFilter(filter: TopLevelFilter) -> TopLevelFilter;
}

/// Write one line to the dedicated crash log with an explicit flush + fsync, so
/// it survives an immediate `abort()`/fault. Also mirrors to the normal log
/// channel (which may or may not flush in time). Best-effort — never panics.
pub fn crash_log(msg: &str) {
    // Mirror to the normal channel first (cheap, may be lost to buffering).
    logger::log(LogLevel::Error, msg);

    // Then the durable path: append + flush + sync so it can't be lost.
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(CRASH_LOG_PATH)
    {
        let _ = f.write_all(msg.as_bytes());
        let _ = f.write_all(b"\n");
        let _ = f.flush();
        let _ = f.sync_all();
    }
}

/// Classify a faulting address relative to our DLL's mapped range.
fn in_our_dll(addr: usize) -> bool {
    let base = DLL_BASE.load(Ordering::Relaxed);
    let end = DLL_END.load(Ordering::Relaxed);
    base != 0 && addr >= base && addr < end
}

/// Human-readable name for the common SEH exception codes.
fn exception_name(code: u32) -> &'static str {
    match code {
        0xC0000005 => "ACCESS_VIOLATION",
        0xC000001D => "ILLEGAL_INSTRUCTION",
        0xC0000094 => "INTEGER_DIVIDE_BY_ZERO",
        0xC0000096 => "PRIVILEGED_INSTRUCTION",
        0xC00000FD => "STACK_OVERFLOW",
        0x80000003 => "BREAKPOINT",
        0xC0000025 => "NONCONTINUABLE_EXCEPTION",
        0xE06D7363 => "C++_EXCEPTION (MSVC throw)",
        _ => "UNKNOWN",
    }
}

/// Top-level unhandled-exception filter. Logs the fault, then returns
/// CONTINUE_SEARCH so the previously-registered handler (spice2x's / the OS
/// default) still runs — we only add a durable record, we don't swallow.
unsafe extern "system" fn exception_filter(info: *const ExceptionPointers) -> i32 {
    // The process is broken; touch only raw pointers, no unwrap/alloc-heavy work.
    if info.is_null() || (*info).exception_record.is_null() {
        crash_log("[DDR-Hook] HARD FAULT: unhandled exception (no record)");
        return EXCEPTION_CONTINUE_SEARCH;
    }
    let rec = &*(*info).exception_record;
    let code = rec.exception_code;
    let addr = rec.exception_address as usize;
    let base = DLL_BASE.load(Ordering::Relaxed);
    let where_ = if in_our_dll(addr) {
        let rva = addr.wrapping_sub(base);
        format!("INSIDE our DLL (base+0x{rva:X})")
    } else {
        "in game / other module".to_string()
    };
    let thread = std::thread::current();
    crash_log(&format!(
        "[DDR-Hook] HARD FAULT: {} (0x{:08X}) at address 0x{:016X} [{}] on thread '{}' ({:?})",
        exception_name(code),
        code,
        addr,
        where_,
        thread.name().unwrap_or("<unnamed>"),
        thread.id(),
    ));
    EXCEPTION_CONTINUE_SEARCH
}

/// Install the unhandled-exception filter and record our DLL's address range.
/// Call once, early in init. Idempotent.
pub fn install() {
    use std::sync::Once;
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        record_dll_bounds();
        unsafe {
            SetUnhandledExceptionFilter(Some(exception_filter));
        }
        // Announce via the durable log so a crash file always has a header line
        // with a timestamp-adjacent marker (the normal log carries the wall clock).
        crash_log("[DDR-Hook] crash handler installed (SEH filter + durable crash log)");
    });
}

/// Resolve our own module's mapped [base, base+size) using an address known to
/// live inside this DLL (the `install` fn itself) via GetModuleHandleExW with
/// the FROM_ADDRESS flag, then GetModuleInformation for the size.
fn record_dll_bounds() {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::HMODULE;
    use windows::Win32::System::LibraryLoader::{
        GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
        GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
    };
    use windows::Win32::System::ProcessStatus::{GetModuleInformation, MODULEINFO};
    use windows::Win32::System::Threading::GetCurrentProcess;

    unsafe {
        let anchor = record_dll_bounds as *const () as *const u16;
        let mut module = HMODULE::default();
        if GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            PCWSTR(anchor),
            &mut module,
        )
        .is_err()
        {
            return;
        }
        let mut mi = MODULEINFO::default();
        if GetModuleInformation(
            GetCurrentProcess(),
            module,
            &mut mi,
            std::mem::size_of::<MODULEINFO>() as u32,
        )
        .is_ok()
        {
            let base = mi.lpBaseOfDll as usize;
            DLL_BASE.store(base, Ordering::Relaxed);
            DLL_END.store(base + mi.SizeOfImage as usize, Ordering::Relaxed);
        }
    }
}
