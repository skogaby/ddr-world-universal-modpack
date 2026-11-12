//! Optional content packs: dancers and stages a user opts into, one pack per
//! source folder (design: `.agents/planning/2026-11-11-optional-content-packs/
//! design/detailed-design.md`).
//!
//! The base release carries a catalog of packs ([`catalog`]); each installed
//! pack is recorded by a marker and the user's choice by a selection file
//! ([`state`]).

pub mod catalog;
pub mod chooser;
pub mod install;
pub mod plan;
pub mod resolve;
pub mod space;
pub mod state;
pub mod step;

use std::collections::BTreeMap;

use crate::gamedir::GameDir;
use crate::Outcome;
use catalog::{Catalog, Pack};
use state::{Marker, SelectionFile};

/// How one catalog pack stands on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Installed {
    No,
    /// The marker names the catalog's revision.
    Current,
    /// The marker names another revision.
    OtherRevision,
}

pub fn installed_state(pack: &Pack, markers: &BTreeMap<String, Marker>) -> Installed {
    match markers.get(&pack.id) {
        None => Installed::No,
        Some(m) if m.content_id == pack.content_id => Installed::Current,
        Some(_) => Installed::OtherRevision,
    }
}

/// `12.3 MB` (binary megabytes, one decimal).
pub fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
}

/// The ids that count as selected: the selection file's list, or — while no
/// usable selection file exists — the packs that are installed (a pack
/// unpacked by hand, or a full image).
pub fn selected_ids(selection: &SelectionFile, markers: &BTreeMap<String, Marker>) -> Vec<String> {
    match selection {
        SelectionFile::Loaded(s) => s.packs.clone(),
        SelectionFile::Absent | SelectionFile::Unreadable(_) => markers.keys().cloned().collect(),
    }
}

/// The `--packs-list` text: every catalog pack with its state.
pub fn render_list(
    catalog: &Catalog,
    selection: &SelectionFile,
    markers: &BTreeMap<String, Marker>,
) -> String {
    let selected = selected_ids(selection, markers);
    let id_width = catalog.packs.iter().map(|p| p.id.len()).max().unwrap_or(0);
    let title_width = catalog
        .packs
        .iter()
        .map(|p| p.title.len())
        .max()
        .unwrap_or(0);
    let mut out = format!(
        "Optional packs offered by the installed release ({}); [x] = selected\n",
        catalog.packs.len()
    );
    let mut group: Option<&str> = None;
    for pack in &catalog.packs {
        if group != Some(pack.group.as_str()) {
            out.push_str(&format!("\n {}\n", pack.group.to_uppercase()));
            group = Some(pack.group.as_str());
        }
        let state = match installed_state(pack, markers) {
            Installed::No => "not installed",
            Installed::Current => "installed",
            Installed::OtherRevision => "installed, another revision",
        };
        out.push_str(&format!(
            "  [{}] {:<id_width$}  {:<title_width$}  {:>4} {:<8} {:>9}  {state}\n",
            if selected.iter().any(|s| s == &pack.id) {
                'x'
            } else {
                ' '
            },
            pack.id,
            pack.title,
            pack.items,
            pack.group.to_lowercase(),
            megabytes(pack.zip_size),
        ));
    }
    let strangers: Vec<&str> = markers
        .keys()
        .filter(|id| catalog.get(id).is_none())
        .map(String::as_str)
        .collect();
    if !strangers.is_empty() {
        out.push_str(&format!(
            "\nInstalled but not offered by this release: {}\n",
            strangers.join(", ")
        ));
    }
    out.push_str("\nTo choose packs: ddr_world_hook_updater.exe --packs\n");
    if let SelectionFile::Unreadable(why) = selection {
        out.push_str(&format!(
            "\n{} could not be read ({why}); showing the installed packs as selected.\n",
            state::SELECTION_NAME
        ));
    }
    out
}

/// `--packs-list`: print the catalog and each pack's state from local files
/// only. Changes nothing and never touches the network.
pub fn list(game: &GameDir) -> Outcome {
    let catalog = match catalog::read(&game.catalog_path(), false) {
        Ok(Some(c)) => c,
        Ok(None) => {
            println!(
                "No pack catalog in this game folder ({} is missing): optional packs arrive with modpack v1.5 and later.",
                catalog::CATALOG_NAME
            );
            return Outcome::NothingToDo;
        }
        Err(e) => {
            println!("{e}");
            return Outcome::NothingToDo;
        }
    };
    let (markers, problems) = state::read_markers(&game.root);
    let selection = state::read_selection(&game.selection_path());
    print!("{}", render_list(&catalog, &selection, &markers));
    for p in problems {
        println!("unreadable marker ignored: {p}");
    }
    Outcome::NothingToDo
}

/// A selection holding exactly `ids` (test and scripting helper).
#[cfg(test)]
pub fn selection_of(ids: &[&str]) -> state::Selection {
    let mut s = state::Selection::new();
    s.packs = ids.iter().map(|s| s.to_string()).collect();
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use catalog::tests::FIXTURE_CATALOG;
    use state::tests::marker_json;

    fn fixture() -> Catalog {
        catalog::parse(FIXTURE_CATALOG, false).unwrap()
    }

    fn marker(id: &str, content_id: &str) -> Marker {
        state::parse_marker(&marker_json(id, content_id, &[]), id).unwrap()
    }

    #[test]
    fn megabytes_formatting() {
        assert_eq!(megabytes(0), "0.0 MB");
        assert_eq!(megabytes(1024 * 1024), "1.0 MB");
        assert_eq!(megabytes(22_963_814), "21.9 MB");
    }

    #[test]
    fn installed_state_compares_content_ids() {
        let c = fixture();
        let pack = &c.packs[0];
        let mut markers = BTreeMap::new();
        assert_eq!(installed_state(pack, &markers), Installed::No);
        markers.insert(pack.id.clone(), marker(&pack.id, &pack.content_id));
        assert_eq!(installed_state(pack, &markers), Installed::Current);
        markers.insert(pack.id.clone(), marker(&pack.id, &"0".repeat(64)));
        assert_eq!(installed_state(pack, &markers), Installed::OtherRevision);
    }

    #[test]
    fn selection_defaults_to_the_installed_packs_until_a_file_exists() {
        let mut markers = BTreeMap::new();
        markers.insert("a".to_string(), marker("a", "c"));
        assert_eq!(selected_ids(&SelectionFile::Absent, &markers), ["a"]);
        assert_eq!(
            selected_ids(&SelectionFile::Unreadable("x".into()), &markers),
            ["a"]
        );
        let loaded = SelectionFile::Loaded(selection_of(&["b"]));
        assert_eq!(selected_ids(&loaded, &markers), ["b"]);
        let none = SelectionFile::Loaded(selection_of(&[]));
        assert!(selected_ids(&none, &markers).is_empty());
    }

    #[test]
    fn list_shows_groups_states_selection_and_strangers() {
        let c = fixture();
        let mut markers = BTreeMap::new();
        markers.insert(
            "stages-fixture-hall".to_string(),
            marker("stages-fixture-hall", &"0".repeat(64)),
        );
        markers.insert("dancers-gone".to_string(), marker("dancers-gone", "c"));
        let text = render_list(
            &c,
            &SelectionFile::Loaded(selection_of(&["dancers-fixture-mix"])),
            &markers,
        );
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines[0].contains("(2)"), "{text}");
        assert_eq!(lines[2], " DANCERS");
        assert!(
            lines[3].starts_with("  [x] dancers-fixture-mix  Fixture Mix ")
                && lines[3].contains("   2 dancers")
                && lines[3].ends_with("not installed"),
            "{text}"
        );
        assert_eq!(lines[5], " STAGES");
        assert!(
            lines[6].starts_with("  [ ] stages-fixture-hall ")
                && lines[6].ends_with("installed, another revision"),
            "{text}"
        );
        assert!(
            text.contains("Installed but not offered by this release: dancers-gone"),
            "{text}"
        );

        let unreadable = render_list(&c, &SelectionFile::Unreadable("bad json".into()), &markers);
        assert!(
            unreadable.contains("could not be read (bad json)"),
            "{unreadable}"
        );
        assert!(
            unreadable.contains("  [x] stages-fixture-hall"),
            "{unreadable}"
        );
    }
}
