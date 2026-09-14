//! Release-archive extraction into the staging directory (design §4.5).
//!
//! Every entry is confined to the stage root (`enclosed_name`), symlink
//! entries are refused, entry count and total size are capped, and an archive
//! without `ddr_world_hook.dll` at its root is rejected as "not a modpack
//! release" before anything is written. Validation runs as a first pass over
//! the whole central directory so a bad archive never leaves a half-extracted
//! stage behind.

use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use crate::relpath::RelPath;

/// The file every release must contain at its root.
pub const REQUIRED_ROOT_FILE: &str = "ddr_world_hook.dll";

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_entries: usize,
    pub max_total_bytes: u64,
}

pub const DEFAULT_LIMITS: Limits = Limits {
    max_entries: 100_000,
    max_total_bytes: 2 * 1024 * 1024 * 1024,
};

/// An extracted release: its stage root and every regular file it contains.
#[derive(Debug)]
pub struct StagedRelease {
    pub root: PathBuf,
    pub files: Vec<RelPath>,
}

#[derive(Debug)]
pub enum ArchiveError {
    Io(io::Error),
    Zip(zip::result::ZipError),
    /// An entry name escapes the stage root or is otherwise not a clean
    /// relative path.
    UnsafePath(String),
    /// A symlink entry (never present in a release; refused on principle).
    SymlinkEntry(String),
    TooManyEntries(usize),
    TooLarge(u64),
    /// No `ddr_world_hook.dll` at the archive root.
    NotAModpack,
}

impl fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ArchiveError::Io(e) => write!(f, "I/O error while extracting: {e}"),
            ArchiveError::Zip(e) => write!(f, "invalid zip archive: {e}"),
            ArchiveError::UnsafePath(n) => write!(f, "unsafe entry name in archive: {n:?}"),
            ArchiveError::SymlinkEntry(n) => write!(f, "symlink entry in archive: {n:?}"),
            ArchiveError::TooManyEntries(n) => write!(f, "archive has too many entries ({n})"),
            ArchiveError::TooLarge(n) => write!(f, "archive is too large ({n} bytes uncompressed)"),
            ArchiveError::NotAModpack => write!(
                f,
                "archive does not contain {REQUIRED_ROOT_FILE} at its root; not a modpack release"
            ),
        }
    }
}

impl From<io::Error> for ArchiveError {
    fn from(e: io::Error) -> Self {
        ArchiveError::Io(e)
    }
}

impl From<zip::result::ZipError> for ArchiveError {
    fn from(e: zip::result::ZipError) -> Self {
        ArchiveError::Zip(e)
    }
}

/// Extract `zip_path` into `stage_root` (emptied first) with the default caps.
pub fn extract(zip_path: &Path, stage_root: &Path) -> Result<StagedRelease, ArchiveError> {
    extract_with_limits(zip_path, stage_root, DEFAULT_LIMITS)
}

pub fn extract_with_limits(
    zip_path: &Path,
    stage_root: &Path,
    limits: Limits,
) -> Result<StagedRelease, ArchiveError> {
    let file = File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(file)?;

    // Pass 1: validate everything before touching the stage.
    if archive.len() > limits.max_entries {
        return Err(ArchiveError::TooManyEntries(archive.len()));
    }
    let mut total: u64 = 0;
    let mut files: Vec<RelPath> = Vec::new();
    let mut has_required = false;
    for i in 0..archive.len() {
        let entry = archive.by_index_raw(i)?;
        let raw_name = entry.name().to_string();
        if entry.enclosed_name().is_none() {
            return Err(ArchiveError::UnsafePath(raw_name));
        }
        if entry.is_symlink() {
            return Err(ArchiveError::SymlinkEntry(raw_name));
        }
        if entry.is_dir() {
            continue;
        }
        total = total.saturating_add(entry.size());
        if total > limits.max_total_bytes {
            return Err(ArchiveError::TooLarge(total));
        }
        let rel =
            RelPath::new(&raw_name).ok_or_else(|| ArchiveError::UnsafePath(raw_name.clone()))?;
        if rel.as_str() == REQUIRED_ROOT_FILE {
            has_required = true;
        }
        files.push(rel);
    }
    if !has_required {
        return Err(ArchiveError::NotAModpack);
    }

    // Pass 2: fresh stage, then extract.
    if stage_root.exists() {
        fs::remove_dir_all(stage_root)?;
    }
    fs::create_dir_all(stage_root)?;
    let mut written: u64 = 0;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        if entry.is_dir() {
            continue;
        }
        let rel = RelPath::new(entry.name())
            .ok_or_else(|| ArchiveError::UnsafePath(entry.name().to_string()))?;
        let dest = rel.to_path(stage_root);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out = File::create(&dest)?;
        // Count bytes actually produced too, so a header that under-declares
        // its size cannot bypass the cap.
        let n = io::copy(
            &mut LimitedReader::new(&mut entry, limits.max_total_bytes - written),
            &mut out,
        )?;
        written = written.saturating_add(n);
        if written > limits.max_total_bytes {
            return Err(ArchiveError::TooLarge(written));
        }
    }
    files.sort();
    files.dedup();
    Ok(StagedRelease {
        root: stage_root.to_path_buf(),
        files,
    })
}

/// Reader that stops after `remaining + 1` bytes so the caller can detect an
/// over-cap stream without reading it all.
struct LimitedReader<R> {
    inner: R,
    remaining: u64,
}

impl<R: Read> LimitedReader<R> {
    fn new(inner: R, remaining: u64) -> Self {
        Self {
            inner,
            remaining: remaining.saturating_add(1),
        }
    }
}

impl<R: Read> Read for LimitedReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.remaining == 0 {
            return Ok(0);
        }
        let max = buf.len().min(self.remaining as usize);
        let n = self.inner.read(&mut buf[..max])?;
        self.remaining -= n as u64;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::{AtomicU32, Ordering};
    use zip::write::SimpleFileOptions;

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    struct Temp(PathBuf);

    impl Temp {
        fn new() -> Self {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "ddr_updater_archive_test_{}_{n}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Temp(dir)
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Build a zip at `path` from (name, bytes) pairs. Names are written raw,
    /// so traversal names can be planted for the negative tests.
    fn make_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let file = File::create(path).unwrap();
        let mut w = zip::ZipWriter::new(file);
        for (name, data) in entries {
            if name.ends_with('/') {
                w.add_directory(*name, SimpleFileOptions::default())
                    .unwrap();
            } else {
                w.start_file(*name, SimpleFileOptions::default()).unwrap();
                w.write_all(data).unwrap();
            }
        }
        w.finish().unwrap();
    }

    const DLL: (&str, &[u8]) = ("ddr_world_hook.dll", b"MZ-dll");

    #[test]
    fn extracts_nested_files_and_lists_them() {
        let t = Temp::new();
        let zip = t.0.join("r.zip");
        make_zip(
            &zip,
            &[
                DLL,
                ("data_mods/", b""),
                ("data_mods/a/", b""),
                ("data_mods/a/x.png", b"png"),
                ("README.md", b"# hi"),
            ],
        );
        let stage = t.0.join("stage");
        fs::create_dir_all(stage.join("junk")).unwrap();
        fs::write(stage.join("junk/old.txt"), b"old").unwrap();
        let staged = extract(&zip, &stage).unwrap();
        assert_eq!(
            staged.files.iter().map(|r| r.as_str()).collect::<Vec<_>>(),
            vec!["README.md", "data_mods/a/x.png", "ddr_world_hook.dll"]
        );
        assert_eq!(fs::read(stage.join("data_mods/a/x.png")).unwrap(), b"png");
        assert_eq!(
            fs::read(stage.join("ddr_world_hook.dll")).unwrap(),
            b"MZ-dll"
        );
        assert!(!stage.join("junk").exists(), "stage must be emptied first");
    }

    #[test]
    fn traversal_and_absolute_names_are_rejected_before_writing() {
        for bad in ["../evil.txt", "/abs.txt", "C:\\x.txt", "a/../../b"] {
            let t = Temp::new();
            let zip = t.0.join("r.zip");
            make_zip(&zip, &[DLL, (bad, b"x")]);
            let stage = t.0.join("stage");
            let err = extract(&zip, &stage).unwrap_err();
            assert!(matches!(err, ArchiveError::UnsafePath(_)), "{bad}: {err}");
            assert!(!stage.exists(), "{bad}: nothing may be written");
            assert!(!t.0.join("evil.txt").exists());
        }
    }

    #[test]
    fn symlink_entries_are_rejected() {
        let t = Temp::new();
        let zip = t.0.join("r.zip");
        let file = File::create(&zip).unwrap();
        let mut w = zip::ZipWriter::new(file);
        w.start_file(DLL.0, SimpleFileOptions::default()).unwrap();
        w.write_all(DLL.1).unwrap();
        w.add_symlink("link", "ddr_world_hook.dll", SimpleFileOptions::default())
            .unwrap();
        w.finish().unwrap();
        let err = extract(&zip, &t.0.join("stage")).unwrap_err();
        assert!(matches!(err, ArchiveError::SymlinkEntry(_)), "{err}");
        assert!(!t.0.join("stage").exists());
    }

    #[test]
    fn missing_dll_is_not_a_modpack() {
        let t = Temp::new();
        let zip = t.0.join("r.zip");
        make_zip(&zip, &[("README.md", b"x"), ("data_mods/a.png", b"y")]);
        let err = extract(&zip, &t.0.join("stage")).unwrap_err();
        assert!(matches!(err, ArchiveError::NotAModpack), "{err}");
    }

    #[test]
    fn entry_cap() {
        let t = Temp::new();
        let zip = t.0.join("r.zip");
        make_zip(&zip, &[DLL, ("a", b"1"), ("b", b"2")]);
        let limits = Limits {
            max_entries: 2,
            max_total_bytes: 1 << 20,
        };
        let err = extract_with_limits(&zip, &t.0.join("stage"), limits).unwrap_err();
        assert!(matches!(err, ArchiveError::TooManyEntries(3)), "{err}");
    }

    #[test]
    fn size_cap_on_declared_sizes() {
        let t = Temp::new();
        let zip = t.0.join("r.zip");
        make_zip(&zip, &[DLL, ("big", &[0u8; 100])]);
        let limits = Limits {
            max_entries: 100,
            max_total_bytes: 50,
        };
        let err = extract_with_limits(&zip, &t.0.join("stage"), limits).unwrap_err();
        assert!(matches!(err, ArchiveError::TooLarge(_)), "{err}");
        assert!(!t.0.join("stage").exists());
    }

    #[test]
    fn corrupt_file_is_a_zip_error() {
        let t = Temp::new();
        let zip = t.0.join("r.zip");
        fs::write(&zip, b"this is not a zip").unwrap();
        let err = extract(&zip, &t.0.join("stage")).unwrap_err();
        assert!(matches!(err, ArchiveError::Zip(_)), "{err}");
    }
}
