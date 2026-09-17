//! Dependency-free pure logic of the `scene3d` service: arc path resolution
//! and the engine's model-name hash. Nothing here touches the engine or any
//! other crate module, so `scripts/validate_background_dancers.sh` can mount
//! this file into a host crate and run its tests on non-x86 hosts (plain
//! `cargo test` cannot build `retour` there).

use std::path::Path;

/// The `data/`-relative form of a game path (`data/arc/x.arc` → `arc/x.arc`),
/// which is what the LayeredFS mod-path index is keyed on. Leading `./`, `/`
/// and `\` are stripped; paths without the `data/` prefix are returned as-is.
pub fn data_relative(game_rel: &str) -> &str {
    let s = game_rel.trim_start_matches(['.', '/', '\\']);
    if s.len() >= 5
        && (s[..5].eq_ignore_ascii_case("data/") || s[..5].eq_ignore_ascii_case("data\\"))
    {
        &s[5..]
    } else {
        s
    }
}

/// Where a game-relative path resolved to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolved {
    /// A LayeredFS mod-folder override (filesystem path).
    ModOverride(String),
    /// The stock file under the game folder (filesystem path).
    Stock(String),
}

impl Resolved {
    pub fn path(&self) -> &str {
        match self {
            Resolved::ModOverride(p) | Resolved::Stock(p) => p,
        }
    }
    pub fn is_override(&self) -> bool {
        matches!(self, Resolved::ModOverride(_))
    }
}

/// Pure resolution: the mod-folder override wins when it exists, else the
/// stock path when it exists, else `None`. `mod_override` is the LayeredFS
/// lookup result for the `data/`-relative form; `stock_root` is the directory
/// the stock `data/...` path is relative to (the game folder = process cwd).
pub fn resolve_with(
    game_rel: &str,
    mod_override: Option<&str>,
    stock_root: &Path,
    exists: impl Fn(&Path) -> bool,
) -> Option<Resolved> {
    if let Some(m) = mod_override {
        if exists(Path::new(m)) {
            return Some(Resolved::ModOverride(m.to_string()));
        }
    }
    let stock = stock_root.join(game_rel.trim_start_matches(['/', '\\']));
    if exists(&stock) {
        return Some(Resolved::Stock(stock.to_string_lossy().into_owned()));
    }
    None
}

/// FNV-1 (multiply-then-xor, offset basis 0x811C9DC5, prime 0x01000193) over
/// the raw bytes — the engine's model-name hash (`FUN_1802030b0`, applied to
/// the bare `.model` file stem, case- and underscore-sensitive; NOT the
/// texture hasher, which lowercases and strips underscores first).
pub fn fnv1_name_hash(name: &str) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in name.as_bytes() {
        h = h.wrapping_mul(0x0100_0193) ^ (b as u32);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_relative_strips_prefix_variants() {
        assert_eq!(data_relative("data/arc/x.arc"), "arc/x.arc");
        assert_eq!(data_relative("./data/arc/x.arc"), "arc/x.arc");
        assert_eq!(data_relative("DATA/arc/x.arc"), "arc/x.arc");
        assert_eq!(data_relative("arc/x.arc"), "arc/x.arc");
    }

    #[test]
    fn mod_override_wins_when_present() {
        let r = resolve_with(
            "data/arc/x.arc",
            Some("./data_mods/m/arc/x.arc"),
            Path::new("/game"),
            |p| p == Path::new("./data_mods/m/arc/x.arc") || p == Path::new("/game/data/arc/x.arc"),
        );
        assert_eq!(
            r,
            Some(Resolved::ModOverride("./data_mods/m/arc/x.arc".into()))
        );
    }

    #[test]
    fn stock_when_no_override_or_override_missing() {
        let only_stock = |p: &Path| p == Path::new("/game/data/arc/x.arc");
        let expect = Some(Resolved::Stock(
            Path::new("/game/data/arc/x.arc")
                .to_string_lossy()
                .into_owned(),
        ));
        assert_eq!(
            resolve_with("data/arc/x.arc", None, Path::new("/game"), only_stock),
            expect
        );
        assert_eq!(
            resolve_with(
                "data/arc/x.arc",
                Some("./data_mods/m/arc/x.arc"),
                Path::new("/game"),
                only_stock
            ),
            expect
        );
    }

    #[test]
    fn missing_everywhere_is_none() {
        assert!(resolve_with("data/arc/x.arc", None, Path::new("/game"), |_| false).is_none());
    }

    #[test]
    fn fnv1_known_vectors() {
        assert_eq!(fnv1_name_hash(""), 0x811C_9DC5);
        assert_eq!(fnv1_name_hash("a"), 0x050C_5D7E);
        assert_eq!(fnv1_name_hash("gm_boom00_footpanel"), 0x3E7F_AD7A);
    }

    #[test]
    fn fnv1_is_case_and_underscore_sensitive() {
        assert_ne!(
            fnv1_name_hash("gm_boom00_bg"),
            fnv1_name_hash("GM_BOOM00_BG")
        );
        assert_ne!(fnv1_name_hash("gm_boom00_bg"), fnv1_name_hash("gmboom00bg"));
    }
}
