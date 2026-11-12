//! The pack catalog (optional content packs design §5.1): which packs the
//! installed modpack release offers, and the exact revision of each.
//!
//! `ddr_world_hook_packs.json` ships inside the base release zip, so it is
//! release-owned and replaced by every base update. The updater never looks
//! for packs anywhere else: a pack is downloaded from the URL its entry gives
//! and must match the size, digest and content id recorded here.

use std::collections::HashSet;
use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

use serde::Deserialize;

use crate::relpath::RelPath;

pub const CATALOG_NAME: &str = "ddr_world_hook_packs.json";
pub const SCHEMA: u32 = 1;
/// Where every pack's marker lives (a dot-directory: the DLL's model scanner
/// skips it).
pub const MARKER_DIR: &str = "data_mods/custom_models/.packs";
/// Pack files may only live under here.
const CONTENT_BASE: &str = "data_mods/";
/// Machine-owned by the DLL and the prep tool; never a pack's.
const CACHE_BASE: &str = "data_mods/_cache";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Pack {
    /// `[a-z0-9-]+`, stable across revisions (`dancers-3rd-mix`).
    pub id: String,
    /// Checklist heading (`Dancers`, `Stages`).
    pub group: String,
    /// Display name (the source folder's name).
    pub title: String,
    /// How many models the pack holds.
    pub items: u32,
    /// The folder every file of the pack lives under.
    pub root: RelPath,
    /// SHA-256 over the pack's files; changes exactly when the pack does.
    pub content_id: String,
    /// File name of the zip.
    pub asset: String,
    pub url: String,
    pub zip_size: u64,
    pub zip_sha256: String,
    /// Bytes on disk once installed.
    pub installed_size: u64,
}

impl Pack {
    /// `data_mods/custom_models/.packs/<id>.json`.
    pub fn marker_path(&self) -> RelPath {
        marker_path(&self.id).expect("a validated pack id forms a valid path")
    }
}

/// The marker path for a pack id, when the id is usable as a file name.
pub fn marker_path(id: &str) -> Option<RelPath> {
    if !valid_id(id) {
        return None;
    }
    RelPath::new(&format!("{MARKER_DIR}/{id}.json"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Catalog {
    /// In display order.
    pub packs: Vec<Pack>,
}

impl Catalog {
    pub fn get(&self, id: &str) -> Option<&Pack> {
        self.packs.iter().find(|p| p.id == id)
    }
}

#[derive(Debug)]
pub enum CatalogError {
    Io(io::Error),
    Parse(serde_json::Error),
    UnknownSchema(u64),
    Invalid(String),
}

impl fmt::Display for CatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CatalogError::Io(e) => write!(f, "cannot read the pack catalog: {e}"),
            CatalogError::Parse(e) => write!(f, "the pack catalog is not valid: {e}"),
            CatalogError::UnknownSchema(s) => {
                write!(
                    f,
                    "pack catalog schema {s} is not supported by this updater"
                )
            }
            CatalogError::Invalid(why) => write!(f, "the pack catalog is not valid: {why}"),
        }
    }
}

/// `[a-z0-9]+(-[a-z0-9]+)*`.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}

fn valid_asset(name: &str) -> bool {
    name.ends_with(".zip")
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.')
}

fn is_hex64(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// `http://127.0.0.1:…` is accepted only when the caller says so (tests
/// serving packs from an in-process listener).
fn valid_url(url: &str, allow_loopback_http: bool) -> bool {
    url.starts_with("https://") || (allow_loopback_http && url.starts_with("http://127.0.0.1:"))
}

fn check(pack: &Pack, allow_loopback_http: bool) -> Result<(), String> {
    let id = &pack.id;
    if !valid_id(id) {
        return Err(format!(
            "pack id {id:?} is not lower-case letters, digits and dashes"
        ));
    }
    let fail = |what: &str| Err(format!("pack {id}: {what}"));
    if pack.group.is_empty() || pack.title.is_empty() {
        return fail("group and title must not be empty");
    }
    let root = pack.root.as_str();
    if !root.starts_with(CONTENT_BASE)
        || root == CACHE_BASE
        || root.starts_with(&format!("{CACHE_BASE}/"))
        || root == MARKER_DIR
        || root.starts_with(&format!("{MARKER_DIR}/"))
    {
        return fail("root must be a content folder under data_mods/");
    }
    if !is_hex64(&pack.content_id) {
        return fail("content_id must be 64 lower-case hex digits");
    }
    if !valid_asset(&pack.asset) {
        return fail("asset must be a lower-case .zip file name");
    }
    if !valid_url(&pack.url, allow_loopback_http) {
        return fail("url must be https");
    }
    if pack.zip_size == 0 || pack.installed_size == 0 {
        return fail("zip_size and installed_size must be positive");
    }
    if !is_hex64(&pack.zip_sha256) {
        return fail("zip_sha256 must be 64 lower-case hex digits");
    }
    Ok(())
}

/// Parse and validate a catalog document.
pub fn parse(text: &str, allow_loopback_http: bool) -> Result<Catalog, CatalogError> {
    let doc: serde_json::Value = serde_json::from_str(text).map_err(CatalogError::Parse)?;
    match doc.get("schema").and_then(serde_json::Value::as_u64) {
        Some(s) if s == u64::from(SCHEMA) => {}
        Some(s) => return Err(CatalogError::UnknownSchema(s)),
        None => return Err(CatalogError::Invalid("no schema number".to_string())),
    }
    let entries = match doc.get("packs").and_then(serde_json::Value::as_array) {
        Some(list) => list,
        None => return Err(CatalogError::Invalid("no packs list".to_string())),
    };
    // serde would also accept an array as a pack's fields in order.
    if !entries.iter().all(serde_json::Value::is_object) {
        return Err(CatalogError::Invalid(
            "a pack entry is not an object".to_string(),
        ));
    }
    let packs: Vec<Pack> = serde_json::from_value(serde_json::Value::Array(entries.clone()))
        .map_err(CatalogError::Parse)?;
    let mut ids = HashSet::new();
    let mut roots = HashSet::new();
    for pack in &packs {
        check(pack, allow_loopback_http).map_err(CatalogError::Invalid)?;
        if !ids.insert(pack.id.clone()) {
            return Err(CatalogError::Invalid(format!(
                "duplicate pack id {}",
                pack.id
            )));
        }
        // Windows paths are case-insensitive: two roots differing only in case
        // are one folder.
        if !roots.insert(pack.root.as_str().to_ascii_lowercase()) {
            return Err(CatalogError::Invalid(format!(
                "pack {}: root is shared with another pack",
                pack.id
            )));
        }
    }
    Ok(Catalog { packs })
}

/// Read the catalog at `path`. `Ok(None)` when absent (a base release older
/// than packs); `Err` when present but unusable.
pub fn read(path: &Path, allow_loopback_http: bool) -> Result<Option<Catalog>, CatalogError> {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(CatalogError::Io(e)),
    };
    parse(&text, allow_loopback_http).map(Some)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use serde_json::{json, Value};

    /// The catalog `scripts/build_packs.py` wrote for the committed fixture.
    pub const FIXTURE_CATALOG: &str = include_str!("../../tests/fixtures/packs/out/catalog.json");

    pub fn entry(id: &str, root: &str) -> Value {
        json!({
            "id": id,
            "group": "Dancers",
            "title": "Test",
            "items": 2,
            "root": root,
            "content_id": "a".repeat(64),
            "asset": format!("ddr-world-pack-{id}-aaaaaaaaaaaa.zip"),
            "url": format!("https://example.invalid/ddr-world-pack-{id}-aaaaaaaaaaaa.zip"),
            "zip_size": 100,
            "zip_sha256": "b".repeat(64),
            "installed_size": 300,
        })
    }

    /// A valid pack rooted at `data_mods/custom_models/dancers/<title>`.
    pub fn test_pack(id: &str, title: &str, content_id: &str) -> Pack {
        let mut e = entry(id, &format!("data_mods/custom_models/dancers/{title}"));
        e["title"] = json!(title);
        e["content_id"] = json!(content_id);
        serde_json::from_value(e).unwrap()
    }

    fn doc(packs: Vec<Value>) -> String {
        json!({"schema": 1, "packs": packs}).to_string()
    }

    fn one(edit: impl FnOnce(&mut Value)) -> Result<Catalog, CatalogError> {
        let mut e = entry("dancers-x", "data_mods/custom_models/dancers/X");
        edit(&mut e);
        parse(&doc(vec![e]), false)
    }

    #[test]
    fn the_fixture_catalog_from_the_build_tool_parses() {
        let c = parse(FIXTURE_CATALOG, false).unwrap();
        assert_eq!(
            c.packs.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            ["dancers-fixture-mix", "stages-fixture-hall"]
        );
        let p = c.get("dancers-fixture-mix").unwrap();
        assert_eq!(
            (p.group.as_str(), p.title.as_str(), p.items),
            ("Dancers", "Fixture Mix", 2)
        );
        assert_eq!(
            p.root.as_str(),
            "data_mods/custom_models/dancers/Fixture Mix"
        );
        assert_eq!(
            p.marker_path().as_str(),
            "data_mods/custom_models/.packs/dancers-fixture-mix.json"
        );
        assert!(p.asset.ends_with(&format!("{}.zip", &p.content_id[..12])));
        assert!(p.url.ends_with(&p.asset));
        assert!(c.get("nope").is_none());
    }

    #[test]
    fn unknown_fields_are_ignored_and_empty_catalogs_are_fine() {
        let c = one(|e| e["future_field"] = json!(1)).unwrap();
        assert_eq!(c.packs.len(), 1);
        assert_eq!(parse(&doc(vec![]), false).unwrap().packs.len(), 0);
    }

    #[test]
    fn schema_is_checked_before_anything_else() {
        assert!(matches!(
            parse(
                r#"{"schema": 2, "packs": "whatever a newer format holds"}"#,
                false
            ),
            Err(CatalogError::UnknownSchema(2))
        ));
        assert!(matches!(
            parse(r#"{"packs": []}"#, false),
            Err(CatalogError::Invalid(_))
        ));
        assert!(matches!(
            parse(r#"{"schema": 1}"#, false),
            Err(CatalogError::Invalid(_))
        ));
        assert!(matches!(
            parse("not json", false),
            Err(CatalogError::Parse(_))
        ));
        assert!(matches!(
            parse(r#"{"schema": 1, "packs": [["a", "b"]]}"#, false),
            Err(CatalogError::Invalid(_))
        ));
        assert!(matches!(
            parse(r#"{"schema": 1, "packs": {}}"#, false),
            Err(CatalogError::Invalid(_))
        ));
    }

    #[test]
    fn each_invalid_field_is_rejected() {
        let cases: Vec<(&str, Box<dyn FnOnce(&mut Value)>)> = vec![
            ("id upper", Box::new(|e| e["id"] = json!("Dancers-X"))),
            ("id path", Box::new(|e| e["id"] = json!("../x"))),
            ("id empty part", Box::new(|e| e["id"] = json!("a--b"))),
            ("title", Box::new(|e| e["title"] = json!(""))),
            (
                "root escapes",
                Box::new(|e| e["root"] = json!("data_mods/../x")),
            ),
            (
                "root outside data_mods",
                Box::new(|e| e["root"] = json!("backup/x")),
            ),
            (
                "root is the cache",
                Box::new(|e| e["root"] = json!("data_mods/_cache/x")),
            ),
            (
                "root is the marker dir",
                Box::new(|e| e["root"] = json!(MARKER_DIR)),
            ),
            ("content id", Box::new(|e| e["content_id"] = json!("abc"))),
            (
                "content id case",
                Box::new(|e| e["content_id"] = json!("A".repeat(64))),
            ),
            (
                "asset path",
                Box::new(|e| e["asset"] = json!("../evil.zip")),
            ),
            ("asset ext", Box::new(|e| e["asset"] = json!("pack.exe"))),
            (
                "url scheme",
                Box::new(|e| e["url"] = json!("http://example.invalid/a.zip")),
            ),
            (
                "url loopback",
                Box::new(|e| e["url"] = json!("http://127.0.0.1:9/a.zip")),
            ),
            ("zip size", Box::new(|e| e["zip_size"] = json!(0))),
            ("zip sha", Box::new(|e| e["zip_sha256"] = json!("zz"))),
            (
                "installed size",
                Box::new(|e| e["installed_size"] = json!(0)),
            ),
            (
                "missing field",
                Box::new(|e| {
                    e.as_object_mut().unwrap().remove("url");
                }),
            ),
            ("negative items", Box::new(|e| e["items"] = json!(-1))),
        ];
        for (name, edit) in cases {
            assert!(one(edit).is_err(), "{name} should be rejected");
        }
    }

    #[test]
    fn loopback_http_only_when_allowed() {
        let mut e = entry("dancers-x", "data_mods/custom_models/dancers/X");
        e["url"] = json!("http://127.0.0.1:4000/a.zip");
        assert!(parse(&doc(vec![e.clone()]), true).is_ok());
        e["url"] = json!("http://example.invalid/a.zip");
        assert!(parse(&doc(vec![e]), true).is_err());
    }

    #[test]
    fn duplicate_ids_and_shared_roots_are_rejected() {
        let a = entry("dancers-x", "data_mods/custom_models/dancers/X");
        let dup = parse(&doc(vec![a.clone(), a.clone()]), false);
        assert!(matches!(dup, Err(CatalogError::Invalid(m)) if m.contains("duplicate pack id")));
        let b = entry("dancers-y", "data_mods/custom_models/dancers/x");
        let shared = parse(&doc(vec![a, b]), false);
        assert!(matches!(shared, Err(CatalogError::Invalid(m)) if m.contains("shared")));
    }

    #[test]
    fn read_distinguishes_absent_from_unusable() {
        let dir = std::env::temp_dir().join(format!("ddr_updater_catalog_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(CATALOG_NAME);
        assert!(matches!(read(&path, false), Ok(None)));
        fs::write(&path, FIXTURE_CATALOG).unwrap();
        assert_eq!(read(&path, false).unwrap().unwrap().packs.len(), 2);
        fs::write(&path, "{").unwrap();
        assert!(matches!(read(&path, false), Err(CatalogError::Parse(_))));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn id_rules() {
        for ok in ["a", "dancers-3rd-mix", "stages-supernova-1-2", "x2"] {
            assert!(valid_id(ok), "{ok}");
        }
        for bad in ["", "-a", "a-", "a--b", "A", "a_b", "a/b", "a.b", "é"] {
            assert!(!valid_id(bad), "{bad:?}");
        }
        assert!(marker_path("../x").is_none());
    }
}
