//! The install manifest (design §4.9, §5.1): what the updater last installed,
//! and the rule that decides whether a release needs installing.
//!
//! `files` records the SHA-256 of every release-owned file the run wrote, so a
//! later run can tell "shipped by a previous release and unchanged since"
//! (prunable) from "modified locally" or "never ours". The manifest is written
//! LAST in the apply sequence; an interrupted update therefore re-runs.

use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::relpath::RelPath;

pub const SCHEMA: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub schema: u32,
    pub tag: String,
    #[serde(default)]
    pub release_name: Option<String>,
    pub asset_name: String,
    /// Lowercase hex.
    pub asset_sha256: String,
    pub installed_at_unix: u64,
    pub updater_version: String,
    /// Release-owned file → sha256 hex (lowercase). Sorted by construction.
    pub files: BTreeMap<RelPath, String>,
}

#[derive(Debug)]
pub enum ManifestError {
    Io(io::Error),
    Parse(serde_json::Error),
    UnknownSchema(u32),
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ManifestError::Io(e) => write!(f, "cannot read manifest: {e}"),
            ManifestError::Parse(e) => write!(f, "manifest is not valid JSON: {e}"),
            ManifestError::UnknownSchema(s) => write!(f, "manifest schema {s} is not supported"),
        }
    }
}

impl Manifest {
    /// A manifest describing a fresh install of `tag`.
    pub fn new(
        tag: &str,
        release_name: Option<&str>,
        asset_name: &str,
        asset_sha256: &str,
        files: BTreeMap<RelPath, String>,
    ) -> Self {
        Manifest {
            schema: SCHEMA,
            tag: tag.to_string(),
            release_name: release_name.map(str::to_string),
            asset_name: asset_name.to_string(),
            asset_sha256: asset_sha256.to_string(),
            installed_at_unix: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            updater_version: env!("CARGO_PKG_VERSION").to_string(),
            files,
        }
    }
}

/// Read the manifest at `path`. `Ok(None)` when absent; `Err` when present but
/// unusable (the caller logs a WARN and treats it as absent).
pub fn read(path: &Path) -> Result<Option<Manifest>, ManifestError> {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(ManifestError::Io(e)),
    };
    let m: Manifest = serde_json::from_str(&text).map_err(ManifestError::Parse)?;
    if m.schema != SCHEMA {
        return Err(ManifestError::UnknownSchema(m.schema));
    }
    Ok(Some(m))
}

/// Serialise (pretty, trailing newline).
pub fn to_json(m: &Manifest) -> String {
    let mut s = serde_json::to_string_pretty(m).expect("manifest serialises");
    s.push('\n');
    s
}

/// Write via temp file + rename so a crash never leaves a torn manifest.
pub fn write(path: &Path, m: &Manifest) -> io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, to_json(m))?;
    fs::rename(&tmp, path)
}

/// The update rule (design §2.2): install unless the recorded tag AND asset
/// digest both match — equality, never ordering.
pub fn needs_update(
    current: Option<&Manifest>,
    tag: &str,
    asset_sha256: &str,
    force: bool,
) -> bool {
    if force {
        return true;
    }
    match current {
        None => true,
        Some(m) => !(m.tag == tag && m.asset_sha256.eq_ignore_ascii_case(asset_sha256)),
    }
}

/// Lowercase hex SHA-256 of a byte slice.
#[cfg(test)]
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

/// Lowercase hex SHA-256 of a file, streamed.
pub fn sha256_file(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex(&hasher.finalize()))
}

/// Hash every listed file under `root`.
pub fn hash_tree(root: &Path, files: &[RelPath]) -> io::Result<BTreeMap<RelPath, String>> {
    let mut out = BTreeMap::new();
    for rel in files {
        out.insert(rel.clone(), sha256_file(&rel.to_path(root))?);
    }
    Ok(out)
}

pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        // Writing into a String cannot fail.
        let _ = write!(s, "{b:02x}");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    struct Temp(PathBuf);

    impl Temp {
        fn new() -> Self {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "ddr_updater_manifest_test_{}_{n}",
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

    fn sample() -> Manifest {
        let mut files = BTreeMap::new();
        files.insert(RelPath::new("ddr_world_hook.dll").unwrap(), "aa".repeat(32));
        files.insert(RelPath::new("data_mods/x/y.png").unwrap(), "bb".repeat(32));
        Manifest::new(
            "v1.2",
            Some("v1.2 - notes"),
            "ddr-world-universal-modpack-20260903_hotfix.zip",
            &"cd".repeat(32),
            files,
        )
    }

    #[test]
    fn round_trip_and_defaults() {
        let t = Temp::new();
        let path = t.0.join("m.json");
        let m = sample();
        write(&path, &m).unwrap();
        assert!(
            !path.with_extension("json.tmp").exists(),
            "temp file must be renamed away"
        );
        let back = read(&path).unwrap().unwrap();
        assert_eq!(back, m);
        assert_eq!(back.schema, 1);
        assert_eq!(back.updater_version, env!("CARGO_PKG_VERSION"));
        assert!(back.installed_at_unix > 1_700_000_000);
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.ends_with('\n'));
        assert!(text.contains("\"data_mods/x/y.png\""));
    }

    #[test]
    fn absent_is_none_and_garbage_is_err() {
        let t = Temp::new();
        assert!(read(&t.0.join("missing.json")).unwrap().is_none());
        let bad = t.0.join("bad.json");
        fs::write(&bad, "{ not json").unwrap();
        assert!(matches!(read(&bad).unwrap_err(), ManifestError::Parse(_)));
    }

    #[test]
    fn unknown_schema_is_err() {
        let t = Temp::new();
        let path = t.0.join("m.json");
        let mut text = to_json(&sample());
        text = text.replace("\"schema\": 1", "\"schema\": 99");
        fs::write(&path, text).unwrap();
        assert!(matches!(
            read(&path).unwrap_err(),
            ManifestError::UnknownSchema(99)
        ));
    }

    #[test]
    fn needs_update_truth_table() {
        let m = sample();
        let same_tag = "v1.2";
        let same_sha = "cd".repeat(32);
        let other_sha = "ef".repeat(32);
        // (current, tag, sha, force) → expected
        assert!(needs_update(None, same_tag, &same_sha, false));
        assert!(needs_update(None, same_tag, &same_sha, true));
        assert!(needs_update(Some(&m), "v1.3", &same_sha, false));
        assert!(needs_update(Some(&m), same_tag, &other_sha, false));
        assert!(needs_update(Some(&m), "v1.3", &other_sha, false));
        assert!(!needs_update(Some(&m), same_tag, &same_sha, false));
        assert!(needs_update(Some(&m), same_tag, &same_sha, true));
        // digest comparison is case-insensitive
        assert!(!needs_update(
            Some(&m),
            same_tag,
            &same_sha.to_uppercase(),
            false
        ));
    }

    #[test]
    fn sha256_known_vector_and_tree() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let t = Temp::new();
        fs::create_dir_all(t.0.join("d")).unwrap();
        fs::write(t.0.join("d/a.txt"), b"abc").unwrap();
        fs::write(t.0.join("b.txt"), b"").unwrap();
        let files = vec![
            RelPath::new("d/a.txt").unwrap(),
            RelPath::new("b.txt").unwrap(),
        ];
        let tree = hash_tree(&t.0, &files).unwrap();
        assert_eq!(tree.len(), 2);
        assert_eq!(
            tree[&RelPath::new("d/a.txt").unwrap()],
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            tree[&RelPath::new("b.txt").unwrap()],
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_file(&t.0.join("d/a.txt")).unwrap(),
            sha256_hex(b"abc")
        );
    }
}
