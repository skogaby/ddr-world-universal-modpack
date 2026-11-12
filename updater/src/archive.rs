//! Archive extraction into the staging directory (design §4.5; optional
//! content packs design §4.8).
//!
//! Every entry is confined to the stage root (`enclosed_name`), symlink
//! entries are refused, and entry count and total size are capped. Two kinds
//! of archive are accepted, each with its own gate:
//!
//! * a RELEASE must carry `ddr_world_hook.dll` at its root, or it is "not a
//!   modpack release";
//! * a PACK may hold nothing but files under its catalog `root` and its own
//!   marker, and that marker must name the catalog's pack id and content id
//!   and list exactly the archive's other files with their sizes — so a pack
//!   zip can write only inside its own source folder.
//!
//! Validation runs as a first pass over the whole central directory so a bad
//! archive never leaves a half-extracted stage behind.

use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use crate::packs::catalog::Pack;
use crate::packs::state::{self, Marker};
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

/// An extracted pack: its stage root and the marker it carries, which lists
/// exactly the files extracted beside it.
#[derive(Debug)]
pub struct StagedPack {
    pub root: PathBuf,
    pub marker: Marker,
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
    /// A pack archive that is not the pack the catalog describes.
    NotThePack(String),
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
            ArchiveError::NotThePack(why) => write!(f, "not the expected pack archive: {why}"),
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
    let entries = scan(&mut archive, limits)?;
    if !entries
        .iter()
        .any(|(rel, _)| rel.as_str() == REQUIRED_ROOT_FILE)
    {
        return Err(ArchiveError::NotAModpack);
    }
    unpack(&mut archive, stage_root, limits)?;
    let mut files: Vec<RelPath> = entries.into_iter().map(|(rel, _)| rel).collect();
    files.sort();
    files.dedup();
    Ok(StagedRelease {
        root: stage_root.to_path_buf(),
        files,
    })
}

/// Extract the pack archive `zip_path` into `stage_root` (emptied first),
/// after checking it is exactly the pack `pack` describes.
pub fn extract_pack(
    zip_path: &Path,
    stage_root: &Path,
    pack: &Pack,
) -> Result<StagedPack, ArchiveError> {
    extract_pack_with_limits(zip_path, stage_root, pack, DEFAULT_LIMITS)
}

pub fn extract_pack_with_limits(
    zip_path: &Path,
    stage_root: &Path,
    pack: &Pack,
    limits: Limits,
) -> Result<StagedPack, ArchiveError> {
    let file = File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    let entries = scan(&mut archive, limits)?;
    let not_it = |why: String| Err(ArchiveError::NotThePack(why));

    // Confinement first: nothing outside the pack's root except its marker.
    let marker_rel = pack.marker_path();
    let prefix = format!("{}/", pack.root.as_str());
    let mut files: Vec<(RelPath, u64)> = Vec::new();
    let mut has_marker = false;
    for (rel, size) in entries {
        if rel == marker_rel {
            has_marker = true;
        } else if rel.as_str().starts_with(&prefix) {
            files.push((rel, size));
        } else {
            return not_it(format!("entry {rel} is outside {}", pack.root));
        }
    }
    if !has_marker {
        return not_it(format!("no marker ({marker_rel})"));
    }

    // The marker must describe this archive and the catalog's revision.
    let mut text = String::new();
    archive
        .by_name(marker_rel.as_str())?
        .take(4 * 1024 * 1024)
        .read_to_string(&mut text)?;
    let marker = match state::parse_marker(&text, &pack.id) {
        Ok(m) => m,
        Err(why) => return not_it(format!("unusable marker: {why}")),
    };
    if marker.content_id != pack.content_id {
        return not_it(format!(
            "the archive holds revision {}, the catalog names {}",
            short(&marker.content_id),
            short(&pack.content_id)
        ));
    }
    files.sort();
    if files.windows(2).any(|w| w[0].0 == w[1].0) {
        return not_it("an entry appears twice".to_string());
    }
    if files.len() != marker.files.len() {
        return not_it(format!(
            "the archive holds {} file(s), its marker lists {}",
            files.len(),
            marker.files.len()
        ));
    }
    for (rel, size) in &files {
        match marker.files.get(rel) {
            Some(record) if record.size == *size => {}
            Some(record) => {
                return not_it(format!(
                    "{rel} is {size} bytes, its marker says {}",
                    record.size
                ))
            }
            None => return not_it(format!("{rel} is not listed in the marker")),
        }
    }

    unpack(&mut archive, stage_root, limits)?;
    Ok(StagedPack {
        root: stage_root.to_path_buf(),
        marker,
    })
}

fn short(content_id: &str) -> &str {
    content_id.get(..12).unwrap_or(content_id)
}

/// Pass 1: validate every entry before touching the stage. Returns the
/// regular files with their declared sizes, in archive order.
fn scan(
    archive: &mut zip::ZipArchive<File>,
    limits: Limits,
) -> Result<Vec<(RelPath, u64)>, ArchiveError> {
    if archive.len() > limits.max_entries {
        return Err(ArchiveError::TooManyEntries(archive.len()));
    }
    let mut total: u64 = 0;
    let mut files = Vec::new();
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
        files.push((rel, entry.size()));
    }
    Ok(files)
}

/// Pass 2: fresh stage, then extract everything.
fn unpack(
    archive: &mut zip::ZipArchive<File>,
    stage_root: &Path,
    limits: Limits,
) -> Result<(), ArchiveError> {
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
    Ok(())
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

    // ---- pack archives ----------------------------------------------------

    use crate::manifest::sha256_hex;
    use crate::packs::catalog::tests::test_pack;
    use crate::packs::state::tests::marker_json;

    const ROOT: &str = "data_mods/custom_models/dancers/Mix";
    const MARKER: &str = "data_mods/custom_models/.packs/dancers-mix.json";

    fn cid() -> String {
        "c".repeat(64)
    }

    fn pack() -> Pack {
        test_pack("dancers-mix", "Mix", &cid())
    }

    /// The marker text for `files` (paths relative to the pack root).
    fn marker_for(id: &str, content_id: &str, files: &[(&str, &[u8])]) -> String {
        let rows: Vec<(String, u64, String)> = files
            .iter()
            .map(|(rel, data)| (format!("{ROOT}/{rel}"), data.len() as u64, sha256_hex(data)))
            .collect();
        let refs: Vec<(&str, u64, &str)> = rows
            .iter()
            .map(|(p, n, h)| (p.as_str(), *n, h.as_str()))
            .collect();
        marker_json(id, content_id, &refs)
    }

    /// A pack zip of `files` (relative to the pack root) plus `marker` and any
    /// `extra` raw entries.
    fn pack_zip(
        path: &Path,
        files: &[(&str, &[u8])],
        marker: Option<&str>,
        extra: &[(&str, &[u8])],
    ) {
        let named: Vec<(String, &[u8])> = files
            .iter()
            .map(|(rel, data)| (format!("{ROOT}/{rel}"), *data))
            .collect();
        let mut entries: Vec<(&str, &[u8])> = named.iter().map(|(n, d)| (n.as_str(), *d)).collect();
        entries.extend_from_slice(extra);
        if let Some(m) = marker {
            entries.push((MARKER, m.as_bytes()));
        }
        make_zip(path, &entries);
    }

    const FILES: [(&str, &[u8]); 2] = [("A/pl_a00.arc", b"arc bytes"), ("A/side.txt", b"row")];

    fn reject(files: &[(&str, &[u8])], marker: Option<&str>, extra: &[(&str, &[u8])]) -> String {
        let t = Temp::new();
        let zip = t.0.join("p.zip");
        pack_zip(&zip, files, marker, extra);
        let stage = t.0.join("stage");
        let err = extract_pack(&zip, &stage, &pack())
            .err()
            .expect("must be rejected");
        assert!(
            !stage.exists(),
            "nothing may be written for a rejected pack"
        );
        assert!(matches!(err, ArchiveError::NotThePack(_)), "{err}");
        err.to_string()
    }

    #[test]
    fn pack_extracts_its_files_and_returns_its_marker() {
        let t = Temp::new();
        let zip = t.0.join("p.zip");
        pack_zip(
            &zip,
            &FILES,
            Some(&marker_for("dancers-mix", &cid(), &FILES)),
            &[],
        );
        let stage = t.0.join("stage");
        let staged = extract_pack(&zip, &stage, &pack()).unwrap();
        let names: Vec<&str> = staged.marker.files.keys().map(RelPath::as_str).collect();
        assert_eq!(
            names,
            [
                "data_mods/custom_models/dancers/Mix/A/pl_a00.arc",
                "data_mods/custom_models/dancers/Mix/A/side.txt"
            ]
        );
        assert_eq!(staged.marker.content_id, cid());
        assert_eq!(
            fs::read(stage.join(ROOT).join("A/pl_a00.arc")).unwrap(),
            b"arc bytes"
        );
        assert!(stage.join(MARKER).is_file());
    }

    #[test]
    fn the_committed_fixture_pack_extracts() {
        let catalog =
            crate::packs::catalog::parse(crate::packs::catalog::tests::FIXTURE_CATALOG, false)
                .unwrap();
        for pack in &catalog.packs {
            let t = Temp::new();
            let zip = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/packs/out")
                .join(&pack.asset);
            let staged = extract_pack(&zip, &t.0.join("stage"), pack).unwrap();
            for (rel, record) in &staged.marker.files {
                let data = fs::read(rel.to_path(&staged.root)).unwrap();
                assert_eq!(
                    (data.len() as u64, sha256_hex(&data)),
                    (record.size, record.sha256.clone())
                );
            }
        }
    }

    #[test]
    fn pack_entries_outside_its_root_are_rejected() {
        let good = marker_for("dancers-mix", &cid(), &FILES);
        for stray in [
            "ddr_world_hook.dll",
            "data_mods/custom_models/dancers/Other/x.arc",
            "data_mods/custom_models/dancers/Mixed/x.arc",
            "data_mods/custom_models/.packs/stages-other.json",
            "data_mods/custom_models/dancers/Mix",
        ] {
            let why = reject(&FILES, Some(&good), &[(stray, b"x")]);
            assert!(why.contains("is outside"), "{stray}: {why}");
        }
    }

    #[test]
    fn pack_marker_must_match_the_catalog_and_the_archive() {
        assert!(reject(&FILES, None, &[]).contains("no marker"));
        assert!(reject(&FILES, Some("{"), &[]).contains("unusable marker"));
        let other_id = marker_for("dancers-other", &cid(), &FILES);
        assert!(reject(&FILES, Some(&other_id), &[]).contains("unusable marker"));
        let other_rev = marker_for("dancers-mix", &"d".repeat(64), &FILES);
        assert!(reject(&FILES, Some(&other_rev), &[]).contains("holds revision"));
        // The marker lists a file the archive lacks, and the reverse.
        let fewer = marker_for("dancers-mix", &cid(), &FILES[..1]);
        assert!(reject(&FILES, Some(&fewer), &[]).contains("lists 1"));
        let more = marker_for("dancers-mix", &cid(), &FILES);
        assert!(reject(&FILES[..1], Some(&more), &[]).contains("lists 2"));
        // Same count, different names; and a size that disagrees.
        let renamed: [(&str, &[u8]); 2] = [("A/pl_a00.arc", b"arc bytes"), ("A/other.txt", b"row")];
        let why = reject(
            &renamed,
            Some(&marker_for("dancers-mix", &cid(), &FILES)),
            &[],
        );
        assert!(why.contains("not listed in the marker"), "{why}");
        let resized: [(&str, &[u8]); 2] =
            [("A/pl_a00.arc", b"arc bytes!!"), ("A/side.txt", b"row")];
        let why = reject(
            &resized,
            Some(&marker_for("dancers-mix", &cid(), &FILES)),
            &[],
        );
        assert!(why.contains("its marker says 9"), "{why}");
    }

    #[test]
    fn pack_archives_keep_the_generic_safety_checks() {
        let t = Temp::new();
        let zip = t.0.join("p.zip");
        let good = marker_for("dancers-mix", &cid(), &FILES);
        pack_zip(&zip, &FILES, Some(&good), &[("../evil.txt", b"x")]);
        assert!(matches!(
            extract_pack(&zip, &t.0.join("stage"), &pack()),
            Err(ArchiveError::UnsafePath(_))
        ));
        pack_zip(&zip, &FILES, Some(&good), &[]);
        let tiny = Limits {
            max_entries: 2,
            max_total_bytes: u64::MAX,
        };
        assert!(matches!(
            extract_pack_with_limits(&zip, &t.0.join("stage"), &pack(), tiny),
            Err(ArchiveError::TooManyEntries(3))
        ));
    }
}
