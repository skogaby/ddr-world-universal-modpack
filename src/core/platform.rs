//! Host-platform detection.
//!
//! One shared answer to "are we on real Windows or under Wine?" — several
//! services gate behavior on it (mfplat_vih_fix installs only under Wine;
//! ntdll_state_shim likewise; song_rate's movie-rate directive is
//! real-Windows-only per the background-movie-sync design's D14). The
//! detection is boot-static, so the first probe is cached.

/// True when the process is running under Wine (CrossOver included):
/// Wine's `ntdll` exports `wine_get_version`, real Windows' never does.
/// Cached after the first call — the answer cannot change at runtime.
#[cfg(windows)]
pub fn running_under_wine() -> bool {
    use std::sync::OnceLock;
    static WINE: OnceLock<bool> = OnceLock::new();
    *WINE.get_or_init(|| {
        use windows::core::PCSTR;
        use windows::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
        unsafe {
            let Ok(ntdll) = GetModuleHandleA(PCSTR(b"ntdll.dll\0".as_ptr())) else {
                return false;
            };
            GetProcAddress(ntdll, PCSTR(b"wine_get_version\0".as_ptr())).is_some()
        }
    })
}

/// Host builds (test harnesses) are never Wine.
#[cfg(not(windows))]
pub fn running_under_wine() -> bool {
    false
}
