//! The two things the tool needs from `mod-config.json`: whether the
//! `stepmania-packs` mod is on, and `stepmania_packs.auto_sync`. Defaults
//! match the DLL (`mod_enabled_in_config`: absent toggle = on;
//! `StepmaniaPacksConfig::default()`: `auto_sync: true`). A missing or
//! unreadable file yields the defaults — the DLL behaves the same.

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

pub const MOD_ID: &str = "stepmania-packs";
pub const SONG_LIMIT_MOD_ID: &str = "song-limit-expansion";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfigView {
    pub stepmania_packs_enabled: bool,
    pub song_limit_expansion_enabled: bool,
    pub auto_sync: bool,
}

impl Default for ConfigView {
    fn default() -> Self {
        Self {
            stepmania_packs_enabled: true,
            song_limit_expansion_enabled: true,
            auto_sync: true,
        }
    }
}

#[derive(Deserialize, Default)]
struct Raw {
    #[serde(default)]
    mods: HashMap<String, bool>,
    #[serde(default)]
    stepmania_packs: Section,
}

#[derive(Deserialize)]
struct Section {
    #[serde(default = "default_true")]
    auto_sync: bool,
}

impl Default for Section {
    fn default() -> Self {
        Self { auto_sync: true }
    }
}

fn default_true() -> bool {
    true
}

/// Parse the file's text. Unknown keys and sections are ignored.
pub fn parse(text: &str) -> Result<ConfigView, serde_json::Error> {
    let raw: Raw = serde_json::from_str(text)?;
    Ok(ConfigView {
        stepmania_packs_enabled: raw.mods.get(MOD_ID).copied().unwrap_or(true),
        song_limit_expansion_enabled: raw.mods.get(SONG_LIMIT_MOD_ID).copied().unwrap_or(true),
        auto_sync: raw.stepmania_packs.auto_sync,
    })
}

/// Read `path`; a missing or malformed file yields the defaults and the
/// reason, so the caller can log it.
pub fn load(path: &Path) -> (ConfigView, Option<String>) {
    match std::fs::read_to_string(path) {
        Ok(text) => match parse(&text) {
            Ok(v) => (v, None),
            Err(e) => (
                ConfigView::default(),
                Some(format!(
                    "{} is not valid JSON ({e}); using defaults",
                    path.display()
                )),
            ),
        },
        Err(e) => (
            ConfigView::default(),
            Some(format!("{} not read ({e}); using defaults", path.display())),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_real_shape() {
        let v = parse(
            r#"{ "mods": { "fast-bootup": true, "stepmania-packs": false, "song-limit-expansion": true },
                 "stepmania_packs": { "exclude_from_all_music": true, "audio_cache_mb": 1024, "auto_sync": false,
                                      "index_timeout_s": 30, "gimmick_policy": "hide_chart" },
                 "layeredfs": { "verbose": false } }"#,
        )
        .unwrap();
        assert_eq!(
            v,
            ConfigView {
                stepmania_packs_enabled: false,
                song_limit_expansion_enabled: true,
                auto_sync: false
            }
        );
    }

    #[test]
    fn absent_keys_default_on() {
        assert_eq!(parse("{}").unwrap(), ConfigView::default());
        assert_eq!(
            parse(r#"{ "mods": { "stepmania-packs": true } }"#).unwrap(),
            ConfigView::default()
        );
        assert!(parse("not json").is_err());
    }

    #[test]
    fn missing_file_is_defaults_with_a_reason() {
        let (v, why) = load(Path::new("/definitely/not/here/mod-config.json"));
        assert_eq!(v, ConfigView::default());
        assert!(why.unwrap().contains("using defaults"));
    }
}
