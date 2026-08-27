//! Shared ownership and contributor policy for `DShowPlayer::BuildGraph`.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(windows)]
use crate::core::{hooks, signatures::SignatureStore};
#[cfg(windows)]
use crate::{log_info, log_warn};
#[cfg(windows)]
use retour::GenericDetour;
#[cfg(windows)]
use std::ptr::{addr_of, addr_of_mut};

const PLAYER_STATE_OFFSET: usize = 0x8;
const PLAYER_STATE_OPENED: u32 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MovieSuppressor {
    NonNativeOs,
    SongRate,
}

/// What `MoviePolicy::call` did with one `BuildGraph` invocation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallOutcome {
    /// Original ran; its result was returned untouched.
    Passthrough,
    /// Original was never called; the success epilogue was faked.
    Suppressed,
    /// Fallback mode: original ran and FAILED — success epilogue faked so the
    /// song proceeds without a movie. Carries the original HRESULT.
    FallbackFaked(u32),
}

pub struct MoviePolicy {
    non_native_os: AtomicBool,
    /// Fallback mode for the NonNativeOs contributor: try the real graph
    /// build first and only fake the success epilogue when it FAILED (e.g.
    /// `VFW_E_CANNOT_RENDER` for VC-1 files with no decoder in the Wine
    /// GStreamer stack). Lets converted (H.264) movies play while
    /// unplayable files degrade to the no-movie behavior instead of the
    /// movie-ready soft-lock. Only meaningful while `non_native_os` is set;
    /// `SongRate` suppression always wins (never builds the graph — the
    /// DirectShow clock cannot follow the XACT rate).
    non_native_fallback: AtomicBool,
    song_rate: AtomicBool,
}

impl Default for MoviePolicy {
    fn default() -> Self {
        Self::new()
    }
}

impl MoviePolicy {
    pub const fn new() -> Self {
        Self {
            non_native_os: AtomicBool::new(false),
            non_native_fallback: AtomicBool::new(false),
            song_rate: AtomicBool::new(false),
        }
    }

    pub fn set(&self, source: MovieSuppressor, suppressed: bool) {
        match source {
            MovieSuppressor::NonNativeOs => self.non_native_os.store(suppressed, Ordering::Release),
            MovieSuppressor::SongRate => self.song_rate.store(suppressed, Ordering::Release),
        }
    }

    /// Sets the NonNativeOs contributor's fallback mode (see the field doc).
    pub fn set_fallback(&self, fallback: bool) {
        self.non_native_fallback.store(fallback, Ordering::Release);
    }

    #[must_use]
    pub fn is_suppressed(&self, source: MovieSuppressor) -> bool {
        match source {
            MovieSuppressor::NonNativeOs => self.non_native_os.load(Ordering::Acquire),
            MovieSuppressor::SongRate => self.song_rate.load(Ordering::Acquire),
        }
    }

    /// True when the next `call` would NOT invoke the original (full
    /// suppression). A fallback-mode NonNativeOs contributor does invoke the
    /// original, so it does not count.
    #[must_use]
    pub fn should_suppress(&self) -> bool {
        if self.song_rate.load(Ordering::Acquire) {
            return true;
        }
        self.non_native_os.load(Ordering::Acquire)
            && !self.non_native_fallback.load(Ordering::Acquire)
    }

    /// True when the NonNativeOs contributor is set AND in fallback mode —
    /// i.e. the next `call` will run the original graph build with the
    /// fail-open fake behind it.
    #[must_use]
    pub fn fallback_active(&self) -> bool {
        self.non_native_os.load(Ordering::Acquire)
            && self.non_native_fallback.load(Ordering::Acquire)
    }

    /// Dispatches one `BuildGraph` invocation according to the contributor
    /// state: full suppression (never call the original, fake the success
    /// epilogue), fallback (call the original, fake success only on FAILED),
    /// or plain passthrough. The original runs at most once.
    ///
    /// # Safety
    /// A non-null `this` must point to a live game `DShowPlayer` object.
    pub unsafe fn call(
        &self,
        this: *mut c_void,
        request: *mut c_void,
        original: impl FnOnce(*mut c_void, *mut c_void) -> u32,
    ) -> (u32, CallOutcome) {
        if self.should_suppress() {
            Self::fake_opened(this);
            return (0, CallOutcome::Suppressed);
        }
        let fallback = self.non_native_os.load(Ordering::Acquire)
            && self.non_native_fallback.load(Ordering::Acquire);
        let hr = original(this, request);
        if fallback && (hr & 0x8000_0000) != 0 {
            Self::fake_opened(this);
            return (0, CallOutcome::FallbackFaked(hr));
        }
        (hr, CallOutcome::Passthrough)
    }

    /// Fakes `BuildGraph`'s success epilogue's one load-bearing side effect:
    /// player state (+0x8) = 3 ("opened"). The `opened` byte (+0x14) stays 0
    /// so the per-frame get-frame path early-returns before touching any COM
    /// pointer — the movie "plays" silently delivering no frames.
    ///
    /// # Safety
    /// A non-null `this` must point to a live game `DShowPlayer` object.
    unsafe fn fake_opened(this: *mut c_void) {
        if !this.is_null() {
            std::ptr::write_volatile(
                (this as *mut u8).add(PLAYER_STATE_OFFSET) as *mut u32,
                PLAYER_STATE_OPENED,
            );
        }
    }
}

static POLICY: MoviePolicy = MoviePolicy::new();
static AVAILABLE: AtomicBool = AtomicBool::new(false);
#[cfg(windows)]
static LOGGED: AtomicBool = AtomicBool::new(false);

pub fn set_suppressed(source: MovieSuppressor, suppressed: bool) {
    POLICY.set(source, suppressed);
    #[cfg(windows)]
    if !POLICY.should_suppress() {
        LOGGED.store(false, Ordering::Release);
    }
}

/// Sets the NonNativeOs contributor's fallback mode (try the real graph
/// build; fake success only when it FAILED). Only meaningful while the
/// NonNativeOs contributor itself is set.
pub fn set_non_native_fallback(fallback: bool) {
    POLICY.set_fallback(fallback);
    #[cfg(windows)]
    if !POLICY.should_suppress() {
        LOGGED.store(false, Ordering::Release);
    }
}

#[must_use]
pub fn is_suppressed(source: MovieSuppressor) -> bool {
    POLICY.is_suppressed(source)
}

#[must_use]
pub fn is_available() -> bool {
    AVAILABLE.load(Ordering::Acquire)
}

#[cfg(windows)]
type BuildGraphFn = unsafe extern "C" fn(*mut c_void, *mut c_void) -> u32;

#[cfg(windows)]
static mut MOVIE_HOOK: Option<GenericDetour<BuildGraphFn>> = None;

/// BuildGraph request-struct offsets (stable across all four supported
/// builds; Ghidra 20260721 `FUN_18024a780`, 20260616 `0x18023AE40`):
/// `+0x00` = narrow path `char*` (request type 0), `+0x10` = type byte
/// (0 narrow / 1 wide / 2 stream / 3 callback), `+0x14` = flag bits.
#[cfg(windows)]
const REQUEST_PATH_OFFSET: usize = 0x0;
#[cfg(windows)]
const REQUEST_TYPE_OFFSET: usize = 0x10;
#[cfg(windows)]
const REQUEST_TYPE_NARROW: u8 = 0;

/// Scratch buffer for the absolutized movie path. Movie opens are issued
/// sequentially from the game's actor-update thread (double-buffered
/// players, but never two concurrent BuildGraph calls), and the game
/// consumes the pointer synchronously inside the original call, so one
/// static buffer is sufficient.
#[cfg(windows)]
static mut ABS_PATH_BUF: [u8; 1024] = [0; 1024];

/// Fallback-mode path canonicalization: the game passes the movie path
/// RELATIVE (`.\data\mdb_apx\movie\<code>.wmv`). Wine's `RenderFile`
/// source-filter selection fails its media byte-pattern probe on relative
/// paths and silently falls back to the async file source + winegstreamer
/// (no VC-1 decoder on macOS) instead of the ASF Media-Type mapping's
/// native WM ASF Reader (live-diagnosed 2026-08-19: identical file renders
/// hr=0 absolute, VFW_E_CANNOT_RENDER relative). Rewriting the narrow
/// request path to absolute costs nothing on stock Windows (same filters
/// resolve) and lets a native-WM-runtime-equipped Wine bottle decode VC-1.
///
/// Best-effort: any anomaly (non-narrow request, already-absolute path,
/// oversized path, GetCurrentDirectoryA failure) leaves the request
/// untouched.
///
/// # Safety
/// `request` must point to a live BuildGraph request struct (validated
/// non-null by the caller); called only from the BuildGraph hook thread.
#[cfg(windows)]
unsafe fn absolutize_request_path(request: *mut c_void) {
    use windows::Win32::System::Environment::GetCurrentDirectoryA;

    if request.is_null() {
        return;
    }
    let req = request as *mut u8;
    if *(req.add(REQUEST_TYPE_OFFSET)) != REQUEST_TYPE_NARROW {
        return;
    }
    let path_ptr = *(req.add(REQUEST_PATH_OFFSET) as *mut *const u8);
    if path_ptr.is_null() {
        return;
    }
    // read the narrow path (bounded)
    let mut len = 0usize;
    while len < 512 && *path_ptr.add(len) != 0 {
        len += 1;
    }
    if len == 0 || len >= 512 {
        return;
    }
    let path = std::slice::from_raw_parts(path_ptr, len);
    // already absolute? (drive letter "X:\..." or UNC "\\...")
    let already_absolute = (len >= 3 && path[1] == b':' && (path[2] == b'\\' || path[2] == b'/'))
        || (len >= 2 && path[0] == b'\\' && path[1] == b'\\');
    if already_absolute {
        return;
    }
    // strip a leading ".\" / "./"
    let rel = if len >= 2 && path[0] == b'.' && (path[1] == b'\\' || path[1] == b'/') {
        &path[2..]
    } else {
        path
    };
    let buf = &mut *addr_of_mut!(ABS_PATH_BUF);
    let cwd_len = GetCurrentDirectoryA(Some(&mut buf[..512])) as usize;
    if cwd_len == 0 || cwd_len + 1 + rel.len() + 1 > buf.len() {
        return;
    }
    let mut pos = cwd_len;
    if buf[pos - 1] != b'\\' {
        buf[pos] = b'\\';
        pos += 1;
    }
    buf[pos..pos + rel.len()].copy_from_slice(rel);
    buf[pos + rel.len()] = 0;
    // point the request at the absolutized copy (consumed synchronously by
    // the original call; the game rebuilds the request on every open)
    *(req.add(REQUEST_PATH_OFFSET) as *mut *const u8) = buf.as_ptr();
}

/// One-shot in-process diagnostic (fallback mode): checks the two inputs of
/// Wine quartz's source-filter probe — the file's leading bytes (expect the
/// ASF header GUID 3026B275...) and the `Media Type` registry mapping — as
/// seen from INSIDE the game process (spice2x trampolines kernel32
/// CreateFileW/ReadFile process-wide, so this view can differ from a clean
/// harness). Logged once; pure logging, no behavior change.
#[cfg(windows)]
unsafe fn log_probe_diagnostic(abs_path: &[u8]) {
    use std::io::Read;
    use windows::core::PCSTR;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExA, RegQueryValueExA, HKEY, HKEY_CLASSES_ROOT, KEY_READ,
    };

    // 1. file bytes as read through whatever CreateFile/ReadFile hooks are
    // live (std::fs uses kernel32 CreateFileW/ReadFile underneath)
    let path = String::from_utf8_lossy(abs_path).into_owned();
    match std::fs::File::open(&path) {
        Ok(mut f) => {
            let mut head = [0u8; 16];
            let read = f.read(&mut head).unwrap_or(0);
            let hex: String = head.iter().map(|b| format!("{:02X}", b)).collect();
            log_info!("movie_policy[diag]: file head read={} bytes={}", read, hex);
        }
        Err(e) => {
            log_info!("movie_policy[diag]: file open failed: {} ({})", e, path);
        }
    }

    // 2. registry mapping as seen in-process
    let key_path =
        b"Media Type\\{E436EB83-524F-11CE-9F53-0020AF0BA770}\\{3026B275-8E66-CF11-A6D9-00AA0062CE6C}\0";
    let mut hkey = HKEY::default();
    let status = RegOpenKeyExA(
        HKEY_CLASSES_ROOT,
        PCSTR(key_path.as_ptr()),
        0,
        KEY_READ,
        &mut hkey,
    );
    if status.is_ok() {
        let mut buf = [0u8; 64];
        let mut len = buf.len() as u32;
        let q = RegQueryValueExA(
            hkey,
            PCSTR(b"Source Filter\0".as_ptr()),
            None,
            None,
            Some(buf.as_mut_ptr()),
            Some(&mut len),
        );
        let val = std::str::from_utf8(&buf[..len.saturating_sub(1).min(63) as usize])
            .unwrap_or("<non-utf8>");
        log_info!(
            "movie_policy[diag]: Media Type key open OK, Source Filter query={:?} value={}",
            q,
            val
        );
        let _ = RegCloseKey(hkey);
    } else {
        log_info!(
            "movie_policy[diag]: Media Type key open FAILED: {:?}",
            status
        );
    }
}

#[cfg(windows)]
static PROBE_DIAG_LOGGED: AtomicBool = AtomicBool::new(false);

#[cfg(windows)]
unsafe extern "C" fn build_graph_hook(this: *mut c_void, request: *mut c_void) -> u32 {
    let Some(hook) = (&*addr_of!(MOVIE_HOOK)).as_ref() else {
        return 0;
    };
    let (result, outcome) = POLICY.call(this, request, |this, request| unsafe {
        if POLICY.fallback_active() {
            absolutize_request_path(request);
            if !PROBE_DIAG_LOGGED.swap(true, Ordering::AcqRel) && !request.is_null() {
                let req = request as *mut u8;
                if *(req.add(REQUEST_TYPE_OFFSET)) == REQUEST_TYPE_NARROW {
                    let path_ptr = *(req.add(REQUEST_PATH_OFFSET) as *mut *const u8);
                    if !path_ptr.is_null() {
                        let mut len = 0usize;
                        while len < 512 && *path_ptr.add(len) != 0 {
                            len += 1;
                        }
                        log_probe_diagnostic(std::slice::from_raw_parts(path_ptr, len));
                    }
                }
            }
        }
        hook.call(this, request)
    });
    match outcome {
        CallOutcome::Suppressed => {
            if !LOGGED.swap(true, Ordering::AcqRel) {
                log_info!(
                    "movie_policy: suppressed DirectShow graph build (player faked as opened)"
                );
            }
        }
        // Once per song start — names the files that still need conversion.
        CallOutcome::FallbackFaked(hr) => {
            log_info!(
                "movie_policy: graph build failed (hr={:#010x}) -- faked opened, no movie this song",
                hr
            );
        }
        // A REAL successful graph build — the movie is actually playing.
        // Hand the live player to the sync engine (no-op unless it
        // initialized; failed builds and fakes never reach it).
        CallOutcome::Passthrough => {
            if result == 0 {
                crate::services::movie_sync::on_graph_opened(this);
            }
        }
    }
    result
}

#[cfg(windows)]
pub fn init(signatures: &SignatureStore) -> bool {
    if AVAILABLE.load(Ordering::Acquire) {
        return true;
    }
    let Some(target) = signatures.get_address("movie_build_graph") else {
        log_warn!("movie_policy: movie_build_graph signature unavailable");
        return false;
    };
    let target: BuildGraphFn = unsafe { std::mem::transmute(target) };
    if let Err(error) =
        unsafe { hooks::install_enabled(addr_of_mut!(MOVIE_HOOK), target, build_graph_hook) }
    {
        log_warn!("movie_policy: hook installation failed: {}", error);
        return false;
    }
    AVAILABLE.store(true, Ordering::Release);
    log_info!("movie_policy: shared BuildGraph hook installed");
    true
}
