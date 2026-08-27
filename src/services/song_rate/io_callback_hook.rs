//! The XACT file-IO callback detour pair (design req 9–11, 15, 21): the
//! streaming engine's serving surface. gamemdx registers a readFile and a
//! getOverlappedResult callback with the XACT engine (RE note §2); for the
//! rate-bound files — the gameplay generation's bank, and (preview design
//! §Components 3) a song-select preview binding when one is published —
//! these detours serve the synthesized virtual bank through the published
//! [`Binding`]'s pure serve/poll dispatch — everything else takes the
//! trampoline for byte-exact stock behavior (unbound preview banks, every
//! other bank slot, every non-audio user).
//!
//! The pair is MANDATORY as a pair: the stock getOverlappedResult callback
//! reports instant completion for any vector-listed handle, which would
//! corrupt a deferral into a spurious 0-byte completion — install both or
//! neither (a second-hook failure rolls the first back). One detour per
//! target (repository rule); a future consumer of these callbacks turns
//! this module into the shared dispatcher per the `judge_hook` pattern.
//!
//! Detour-context law (design threading table): both bodies run on the game
//! thread (the in-create header read) AND the engine pump threads — they are
//! thread-agnostic, allocation-free, log-free, and panic-free. The only
//! decision logic here is the outcome→ABI mapping; everything with judgment
//! lives in the host-tested serve/poll dispatch.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use super::binding::{self, PollOutcome, ServeOutcome};
use crate::core::{hooks, signatures::SignatureStore};
use crate::{log_info, log_warn};
use retour::GenericDetour;
use std::ffi::c_void;
use std::ptr::{addr_of, addr_of_mut};
use windows::Win32::Foundation::{SetLastError, ERROR_IO_INCOMPLETE, ERROR_IO_PENDING};

/// Minimal `OVERLAPPED` mirror documenting the exact repurposed-field
/// protocol (RE note §2/§7): gamemdx's callbacks use the full 64-bit
/// offset union (`u.Pointer`) as the read offset and `Internal` as the
/// bytes-completed accumulator ("accumulate on serve, report-and-zero on
/// poll"). x64 layout: Internal @0, InternalHigh @8, offset union @16,
/// hEvent @24.
#[repr(C)]
struct Overlapped {
    internal: u64,
    internal_high: u64,
    offset: u64,
    h_event: *mut c_void,
}

/// `BOOL readfile_cb(HANDLE, void* buf, DWORD len, DWORD* bytesRead,
/// OVERLAPPED* ov)`.
type ReadFileCb = unsafe extern "C" fn(*mut c_void, *mut u8, u32, *mut u32, *mut Overlapped) -> i32;
/// `BOOL overlapped_cb(HANDLE, OVERLAPPED* ov, DWORD* bytes, BOOL wait)`.
type OverlappedCb = unsafe extern "C" fn(*mut c_void, *mut Overlapped, *mut u32, i32) -> i32;
/// The stock handle→file_id lookup helper (task-01 Option A): replicates
/// the locked sorted-vector walk exactly (it takes the AVS mutex itself);
/// returns −1 on a miss.
type HandleLookupFn = unsafe extern "C" fn(*mut c_void) -> i32;

static mut READFILE_HOOK: Option<GenericDetour<ReadFileCb>> = None;
static mut OVERLAPPED_HOOK: Option<GenericDetour<OverlappedCb>> = None;
/// The stock lookup helper's address (zero until init publishes it).
static HANDLE_LOOKUP: AtomicUsize = AtomicUsize::new(0);
/// Both detours live — the readiness conjunction's binding leg
/// (`binding::integration_available()` reads this through `installed`).
static INSTALLED: AtomicBool = AtomicBool::new(false);

/// Whether the detour pair is installed (the streaming integration's
/// readiness leg, design req 40).
#[must_use]
pub fn installed() -> bool {
    INSTALLED.load(Ordering::Acquire)
}

/// Resolve `handle` to the game's file id through the stock helper, and to
/// the bound binding's verdict: `Some(..)` only when the handle IS a bound
/// file — the ACTIVE (gameplay) binding first, the song-select PREVIEW
/// binding on miss (the registry's host-tested routing order). Everything
/// else — no binding at all (the common case: at most two Acquire loads,
/// no lookup call), unresolved helper, lookup miss, different file — is
/// `None` and takes the trampoline.
unsafe fn bound_verdict<R>(
    handle: *mut c_void,
    visit: impl FnOnce(&binding::Binding) -> R,
) -> Option<R> {
    let registry = binding::registry();
    if !registry.any_bound() {
        return None;
    }
    // SAFETY: the helper address was validated by the task-01 derivation
    // (fail-closed publish) and the call replicates the stock per-read
    // lookup exactly (RE note §6/§7).
    let file_id = unsafe {
        let lookup = HANDLE_LOOKUP.load(Ordering::Acquire);
        if lookup == 0 {
            return None;
        }
        let lookup: HandleLookupFn = std::mem::transmute(lookup);
        lookup(handle)
    };
    registry.with_bound_for_file(file_id, visit)
}

unsafe extern "C" fn readfile_hook(
    handle: *mut c_void,
    buffer: *mut u8,
    len: u32,
    bytes_read: *mut u32,
    overlapped: *mut Overlapped,
) -> i32 {
    let Some(hook) = (&*addr_of!(READFILE_HOOK)).as_ref() else {
        return 0;
    };
    if overlapped.is_null() || buffer.is_null() {
        // Never in the streaming path (the engine always passes both);
        // whatever this is, it is stock's problem.
        return hook.call(handle, buffer, len, bytes_read, overlapped);
    }
    let verdict = bound_verdict(handle, |active| {
        // SAFETY: the engine keeps `buffer` valid for `len` and the
        // OVERLAPPED alive until the request is consumed (the stock
        // contract the serve dispatch documents).
        unsafe {
            active.serve(
                (*overlapped).offset,
                len,
                buffer,
                std::ptr::addr_of_mut!((*overlapped).internal),
            )
        }
    });
    match verdict {
        // Unbound — byte-exact stock behavior via the trampoline.
        None => hook.call(handle, buffer, len, bytes_read, overlapped),
        Some(ServeOutcome::Served(copied)) if copied != 0 => {
            if !bytes_read.is_null() {
                *bytes_read = copied;
            }
            1
        }
        // Stock returns TRUE iff copied != 0 (the defensive-EOF leg).
        Some(ServeOutcome::Served(_)) => {
            if !bytes_read.is_null() {
                *bytes_read = 0;
            }
            0
        }
        // The engine's native polled-async contract: FALSE +
        // ERROR_IO_PENDING is tolerated at issue time (RE note §3).
        Some(ServeOutcome::Pending) => {
            SetLastError(ERROR_IO_PENDING);
            0
        }
        // Refused = the binding retired under us (unregister teardown in
        // progress; engine Destroy follows within the same call). Byte
        // authority returns to stock — the FileManager RAM copy is still
        // loaded and the stock EOF clamp owns the size difference.
        Some(ServeOutcome::Refused) => hook.call(handle, buffer, len, bytes_read, overlapped),
    }
}

unsafe extern "C" fn overlapped_hook(
    handle: *mut c_void,
    overlapped: *mut Overlapped,
    bytes: *mut u32,
    wait: i32,
) -> i32 {
    let Some(hook) = (&*addr_of!(OVERLAPPED_HOOK)).as_ref() else {
        return 0;
    };
    if overlapped.is_null() {
        return hook.call(handle, overlapped, bytes, wait);
    }
    let verdict = bound_verdict(handle, |active| {
        // SAFETY: `overlapped` is the request's live OVERLAPPED (the same
        // accumulator pointer its serve call registered).
        unsafe { active.poll(std::ptr::addr_of_mut!((*overlapped).internal)) }
    });
    match verdict {
        None => hook.call(handle, overlapped, bytes, wait),
        // A deferred read completed: the dispatch reported and zeroed the
        // accumulator and freed its slot; packets are ≤ 64 KiB so the u32
        // report is exact.
        Some(PollOutcome::Complete(reported)) => {
            if !bytes.is_null() {
                *bytes = reported as u32;
            }
            1
        }
        // Armed but not yet complete (`bWait` is always 0 at the engine's
        // single call site — never block).
        Some(PollOutcome::Incomplete) => {
            SetLastError(ERROR_IO_INCOMPLETE);
            0
        }
        // No pending slot: synchronous serves accumulated into `Internal`;
        // report-and-zero exactly like the stock callback.
        Some(PollOutcome::NotPending) => {
            let reported = (*overlapped).internal;
            (*overlapped).internal = 0;
            if !bytes.is_null() {
                *bytes = reported as u32;
            }
            1
        }
    }
}

/// Install the detour pair from task-01's derived addresses. Fail-open: any
/// missing signature or hook failure leaves BOTH detours uninstalled (one
/// WARN), `installed()` stays false, the readiness conjunction never goes
/// true, and the SONG SPEED row never registers — everything stock.
pub fn init(signatures: &SignatureStore) -> bool {
    if installed() {
        return true;
    }
    let (Some(readfile), Some(overlapped), Some(lookup)) = (
        signatures.get_address("song_rate_readfile_callback"),
        signatures.get_address("song_rate_overlapped_callback"),
        signatures.get_address("song_rate_handle_lookup"),
    ) else {
        log_warn!(
            "song_rate: IO-callback signatures unavailable -- streaming integration disabled (stock playback)"
        );
        return false;
    };
    HANDLE_LOOKUP.store(lookup as usize, Ordering::Release);
    let readfile: ReadFileCb = unsafe { std::mem::transmute(readfile) };
    let overlapped: OverlappedCb = unsafe { std::mem::transmute(overlapped) };
    unsafe {
        if let Err(error) =
            hooks::install_enabled(addr_of_mut!(READFILE_HOOK), readfile, readfile_hook)
        {
            log_warn!("song_rate: readFile callback hook failed: {}", error);
            return false;
        }
        if let Err(error) =
            hooks::install_enabled(addr_of_mut!(OVERLAPPED_HOOK), overlapped, overlapped_hook)
        {
            // The pair is mandatory: a lone readFile detour would let the
            // stock getOverlappedResult callback corrupt a deferral into a
            // spurious 0-byte completion. Roll back to neither.
            if let Some(hook) = (&*addr_of!(READFILE_HOOK)).as_ref() {
                let _ = hook.disable();
            }
            *addr_of_mut!(READFILE_HOOK) = None;
            log_warn!(
                "song_rate: getOverlappedResult callback hook failed ({}) -- pair rolled back",
                error
            );
            return false;
        }
    }
    INSTALLED.store(true, Ordering::Release);
    log_info!("song_rate: XACT IO-callback detour pair installed (streaming integration live)");
    true
}
