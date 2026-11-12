//! What is on disk and what the user wants (optional content packs design
//! §5.2, §5.3).
//!
//! A MARKER (`data_mods/custom_models/.packs/<id>.json`) records one installed
//! pack: its content id and its files with size and hash. It ships inside the
//! pack zip, so a pack unpacked by hand is recognised exactly like one the
//! updater installed. No marker, or an unreadable one, means "not installed".
//!
//! The SELECTION (`ddr_world_hook_updater.packs.json`, game folder) records
//! which packs the user wants. It is written by the chooser and
//! `--packs-install`, and may be edited by hand.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::catalog::{self, MARKER_DIR};
use crate::relpath::RelPath;

pub const SELECTION_NAME: &str = "ddr_world_hook_updater.packs.json";
pub const MARKER_SCHEMA: u32 = 1;
pub const SELECTION_SCHEMA: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRecord {
    pub size: u64,
    /// Lowercase hex.
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Marker {
    pub schema: u32,
    pub id: String,
    pub content_id: String,
    /// Game-folder-relative path → record. Never includes the marker itself.
    pub files: BTreeMap<RelPath, FileRecord>,
}

/// Deserialize a JSON OBJECT. (serde would also accept an array as the
/// struct's fields in order; these files are documented as objects.)
fn from_object<T: serde::de::DeserializeOwned>(text: &str) -> Result<T, String> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    if !value.is_object() {
        return Err("not a JSON object".to_string());
    }
    serde_json::from_value(value).map_err(|e| e.to_string())
}

/// Parse a marker and check it is the marker of `expected_id`.
pub fn parse_marker(text: &str, expected_id: &str) -> Result<Marker, String> {
    let marker: Marker = from_object(text)?;
    if marker.schema != MARKER_SCHEMA {
        return Err(format!("marker schema {} is not supported", marker.schema));
    }
    if marker.id != expected_id {
        return Err(format!(
            "the marker names pack {:?}, not {expected_id:?}",
            marker.id
        ));
    }
    Ok(marker)
}

/// Every readable marker under the game folder, by pack id. Unreadable ones
/// are returned separately so the caller can report them; they count as "not
/// installed".
pub fn read_markers(game_root: &Path) -> (BTreeMap<String, Marker>, Vec<String>) {
    let mut markers = BTreeMap::new();
    let mut problems = Vec::new();
    let dir = RelPath::new(MARKER_DIR)
        .expect("constant is a valid RelPath")
        .to_path(game_root);
    let Ok(entries) = fs::read_dir(&dir) else {
        return (markers, problems);
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(id) = name.strip_suffix(".json") else {
            continue;
        };
        if !catalog::valid_id(id) {
            continue;
        }
        let parsed = fs::read_to_string(entry.path())
            .map_err(|e| e.to_string())
            .and_then(|text| parse_marker(&text, id));
        match parsed {
            Ok(marker) => {
                markers.insert(id.to_string(), marker);
            }
            Err(why) => problems.push(format!("{MARKER_DIR}/{name}: {why}")),
        }
    }
    (markers, problems)
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Selection {
    pub schema: u32,
    /// Pack ids the user wants installed.
    #[serde(default)]
    pub packs: Vec<String>,
    /// Install packs a later release adds, without asking.
    #[serde(default)]
    pub install_new: bool,
    /// Every pack id the user has been shown (or that was adopted through
    /// `install_new`); catalog packs outside it are "new".
    #[serde(default)]
    pub known: Vec<String>,
}

impl Selection {
    pub fn new() -> Self {
        Selection {
            schema: SELECTION_SCHEMA,
            ..Selection::default()
        }
    }

    pub fn is_selected(&self, id: &str) -> bool {
        self.packs.iter().any(|p| p == id)
    }

    pub fn is_known(&self, id: &str) -> bool {
        self.known.iter().any(|p| p == id)
    }
}

/// What the selection file says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectionFile {
    /// No file: nothing was ever chosen.
    Absent,
    Loaded(Selection),
    /// Present but unusable; the payload says why. The file is left alone.
    Unreadable(String),
}

pub fn parse_selection(text: &str) -> Result<Selection, String> {
    let selection: Selection = from_object(text)?;
    if selection.schema != SELECTION_SCHEMA {
        return Err(format!(
            "selection schema {} is not supported",
            selection.schema
        ));
    }
    Ok(selection)
}

pub fn read_selection(path: &Path) -> SelectionFile {
    match fs::read_to_string(path) {
        Ok(text) => match parse_selection(&text) {
            Ok(s) => SelectionFile::Loaded(s),
            Err(why) => SelectionFile::Unreadable(why),
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => SelectionFile::Absent,
        Err(e) => SelectionFile::Unreadable(e.to_string()),
    }
}

/// Write via temp file + rename so a crash never leaves a torn file.
pub fn write_selection(path: &Path, selection: &Selection) -> io::Result<()> {
    let mut text = serde_json::to_string_pretty(selection).map_err(io::Error::other)?;
    text.push('\n');
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    pub struct Temp(pub PathBuf);

    impl Temp {
        pub fn new() -> Self {
            static N: AtomicU32 = AtomicU32::new(0);
            let dir = std::env::temp_dir().join(format!(
                "ddr_updater_packs_{}_{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
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

    pub fn marker_json(id: &str, content_id: &str, files: &[(&str, u64, &str)]) -> String {
        let files: serde_json::Map<String, serde_json::Value> = files
            .iter()
            .map(|(path, size, sha)| {
                (
                    path.to_string(),
                    serde_json::json!({"size": size, "sha256": sha}),
                )
            })
            .collect();
        serde_json::json!({"schema": 1, "id": id, "content_id": content_id, "files": files})
            .to_string()
    }

    fn put_marker(root: &Path, name: &str, text: &str) {
        let dir = root.join("data_mods/custom_models/.packs");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(name), text).unwrap();
    }

    #[test]
    fn marker_round_trip_and_identity_check() {
        let text = marker_json(
            "dancers-x",
            &"c".repeat(64),
            &[("data_mods/custom_models/dancers/X/A/pl_a00.arc", 12, "ab")],
        );
        let m = parse_marker(&text, "dancers-x").unwrap();
        assert_eq!(m.content_id, "c".repeat(64));
        assert_eq!(m.files.len(), 1);
        let (path, rec) = m.files.iter().next().unwrap();
        assert_eq!(
            path.as_str(),
            "data_mods/custom_models/dancers/X/A/pl_a00.arc"
        );
        assert_eq!((rec.size, rec.sha256.as_str()), (12, "ab"));
        assert!(parse_marker(&text, "dancers-y")
            .unwrap_err()
            .contains("names pack"));
        assert!(parse_marker(&text.replace("\"schema\":1", "\"schema\":9"), "dancers-x").is_err());
        // An escaping path makes the whole marker unusable.
        let evil = marker_json("dancers-x", "c", &[("../outside", 1, "ab")]);
        assert!(parse_marker(&evil, "dancers-x").is_err());
        assert!(parse_marker(r#"[1, "dancers-x", "c", {}]"#, "dancers-x").is_err());
    }

    #[test]
    fn the_marker_inside_the_fixture_pack_parses() {
        let zip = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/packs/out/ddr-world-pack-stages-fixture-hall-058030494441.zip"
        );
        let mut archive = zip::ZipArchive::new(fs::File::open(zip).unwrap()).unwrap();
        let mut text = String::new();
        io::Read::read_to_string(
            &mut archive
                .by_name("data_mods/custom_models/.packs/stages-fixture-hall.json")
                .unwrap(),
            &mut text,
        )
        .unwrap();
        let m = parse_marker(&text, "stages-fixture-hall").unwrap();
        assert!(m.content_id.starts_with("058030494441"));
        assert_eq!(m.files.len(), 2);
        assert!(m.files.keys().all(|p| p
            .as_str()
            .starts_with("data_mods/custom_models/stages/Fixture Hall/")));
    }

    #[test]
    fn read_markers_collects_good_ones_and_reports_bad_ones() {
        let t = Temp::new();
        assert_eq!(read_markers(&t.0), (BTreeMap::new(), vec![]));
        put_marker(&t.0, "dancers-x.json", &marker_json("dancers-x", "c1", &[]));
        put_marker(
            &t.0,
            "stages-y.json",
            &marker_json("stages-other", "c2", &[]),
        );
        put_marker(&t.0, "broken.json", "{");
        put_marker(&t.0, "Not An Id.json", "{}");
        put_marker(&t.0, "notes.txt", "ignored");
        let (markers, problems) = read_markers(&t.0);
        assert_eq!(markers.keys().collect::<Vec<_>>(), ["dancers-x"]);
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(problems.iter().any(|p| p.contains("stages-y.json")));
        assert!(problems.iter().any(|p| p.contains("broken.json")));
    }

    #[test]
    fn selection_round_trip_defaults_and_states() {
        let t = Temp::new();
        let path = t.0.join(SELECTION_NAME);
        assert_eq!(read_selection(&path), SelectionFile::Absent);

        let mut s = Selection::new();
        s.packs = vec!["dancers-x".into()];
        s.install_new = true;
        s.known = vec!["dancers-x".into(), "stages-y".into()];
        write_selection(&path, &s).unwrap();
        assert_eq!(read_selection(&path), SelectionFile::Loaded(s.clone()));
        assert!(s.is_selected("dancers-x") && !s.is_selected("stages-y"));
        assert!(s.is_known("stages-y") && !s.is_known("stages-z"));
        let text = fs::read_to_string(&path).unwrap();
        assert!(
            text.ends_with("}\n") && text.contains("\n  \"packs\": ["),
            "{text}"
        );
        assert!(!t.0.join("ddr_world_hook_updater.packs.json.tmp").exists());

        // A hand-written file may leave the optional keys out.
        fs::write(&path, r#"{"schema": 1, "packs": ["a"]}"#).unwrap();
        let SelectionFile::Loaded(hand) = read_selection(&path) else {
            panic!("expected Loaded");
        };
        assert_eq!(
            (hand.packs.len(), hand.install_new, hand.known.len()),
            (1, false, 0)
        );

        for bad in [
            "{",
            r#"{"schema": 2, "packs": []}"#,
            r#"{"packs": []}"#,
            r#"[1]"#,
        ] {
            fs::write(&path, bad).unwrap();
            assert!(
                matches!(read_selection(&path), SelectionFile::Unreadable(_)),
                "{bad}"
            );
            assert_eq!(fs::read_to_string(&path).unwrap(), bad, "left untouched");
        }
    }
}
