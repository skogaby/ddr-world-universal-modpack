//! `RelPath`: a release-relative file path — the key form shared by the
//! archive listing, the install manifest and the plan.
//!
//! Always forward-slash separated, relative, and free of `.`/`..`/empty
//! components, so it can be stored in JSON portably and joined onto the game
//! folder without ever escaping it.

use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct RelPath(String);

impl RelPath {
    /// Validate and normalise a path string. Backslashes are NOT accepted as
    /// separators (zip entries and manifests are always forward-slash); a
    /// Windows path must be converted by the caller.
    pub fn new(s: &str) -> Option<RelPath> {
        if s.is_empty() || s.starts_with('/') || s.contains('\\') {
            return None;
        }
        if s.contains(':') {
            // Drive letters (`C:/x`) and NTFS alternate streams — never legal
            // in a release-relative path.
            return None;
        }
        for component in s.split('/') {
            if component.is_empty() || component == "." || component == ".." {
                return None;
            }
        }
        Some(RelPath(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Join onto `root`, component by component.
    pub fn to_path(&self, root: &Path) -> PathBuf {
        let mut p = root.to_path_buf();
        for component in self.0.split('/') {
            p.push(component);
        }
        p
    }

    /// The path's parent components (`a/b/c.png` → `Some("a/b")`).
    pub fn parent(&self) -> Option<RelPath> {
        self.0
            .rsplit_once('/')
            .map(|(head, _)| RelPath(head.to_string()))
    }
}

impl fmt::Display for RelPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for RelPath {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        RelPath::new(&s).ok_or_else(|| serde::de::Error::custom(format!("invalid RelPath: {s:?}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_plain_relative_paths() {
        for ok in [
            "x",
            "a/b.png",
            "data_mods/custom_options/select_music_option_v3_ifs/tex/x.png",
        ] {
            assert!(RelPath::new(ok).is_some(), "{ok}");
        }
    }

    #[test]
    fn rejects_unsafe_or_non_normalised_paths() {
        for bad in [
            "", "/a", "a//b", "a/../b", "./a", "..", "a/.", "a\\b", "C:/a", "a:b", "a/",
        ] {
            assert!(RelPath::new(bad).is_none(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn to_path_joins_components() {
        let r = RelPath::new("a/b/c.txt").unwrap();
        assert_eq!(
            r.to_path(Path::new("root")),
            Path::new("root").join("a").join("b").join("c.txt")
        );
        assert_eq!(r.parent().unwrap().as_str(), "a/b");
        assert_eq!(RelPath::new("top").unwrap().parent(), None);
    }

    #[test]
    fn serde_round_trip_and_validation() {
        let r = RelPath::new("a/b").unwrap();
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(json, "\"a/b\"");
        let back: RelPath = serde_json::from_str(&json).unwrap();
        assert_eq!(back, r);
        assert!(serde_json::from_str::<RelPath>("\"../x\"").is_err());
    }
}
