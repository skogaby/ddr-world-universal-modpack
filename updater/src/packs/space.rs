//! Free-space check before a pack is fetched and unpacked (optional content
//! packs design §4.7). The largest pack is several hundred megabytes; running
//! a cabinet's disk full half-way through would only produce a rollback.

use std::path::Path;

/// Head-room kept beyond what the pack itself needs.
pub const MARGIN: u64 = 64 * 1024 * 1024;

/// How many bytes are missing to hold `need` (plus the margin), or `None`
/// when there is room — or when the free space is unknown, in which case the
/// install is attempted.
pub fn shortfall(free: Option<u64>, need: u64) -> Option<u64> {
    let free = free?;
    let wanted = need.saturating_add(MARGIN);
    (free < wanted).then(|| wanted - free)
}

/// Bytes available to this user on the volume holding `dir`. `None` when the
/// platform call fails or (off Windows) is not made.
#[cfg(windows)]
pub fn free_bytes(dir: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut available: u64 = 0;
    // SAFETY: `wide` is a NUL-terminated UTF-16 string that outlives the
    // call; `available` is a valid out-pointer; the two optional
    // out-parameters are null, which the API permits.
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut available,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    (ok != 0).then_some(available)
}

#[cfg(not(windows))]
pub fn free_bytes(_dir: &Path) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortfall_rules() {
        assert_eq!(
            shortfall(None, u64::MAX),
            None,
            "unknown free space never blocks"
        );
        assert_eq!(shortfall(Some(MARGIN + 100), 100), None);
        assert_eq!(shortfall(Some(MARGIN + 99), 100), Some(1));
        assert_eq!(shortfall(Some(0), 100), Some(MARGIN + 100));
        assert_eq!(shortfall(Some(5), u64::MAX), Some(u64::MAX - 5));
    }

    #[test]
    fn free_bytes_is_some_only_on_windows() {
        let here = std::env::temp_dir();
        assert_eq!(free_bytes(&here).is_some(), cfg!(windows));
    }
}
