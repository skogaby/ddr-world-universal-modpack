//! The pack step of a run (optional content packs design §3.1, §4.5): after
//! the base release is settled, bring every selected pack to the revision the
//! installed catalog names. Also the two ways a selection is set without the
//! chooser's screen (`--packs-install`) and with it.
//!
//! Nothing here can stop the game from starting: every failure is a warning
//! and the pack is retried on the next run. The one exception is a pack
//! transaction whose rollback failed, which is reported upward exactly like
//! a failed rollback of the base release.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use super::catalog::{self, Catalog, Pack};
use super::chooser;
use super::install::{self, PackError, Sources};
use super::megabytes;
use super::resolve::{self, Reason, Work};
use super::state::{self, Marker, Selection, SelectionFile};
use crate::apply::{Session, Summary};
use crate::console;
use crate::fault;
use crate::gamedir::GameDir;
use crate::github;
use crate::manifest;
use crate::relpath::RelPath;
use crate::{log_error, log_info, log_warn};

/// Test-only: lets a catalog name `http://127.0.0.1:<port>/…` URLs, so the
/// integration tests can serve packs from an in-process listener.
pub const ALLOW_LOOPBACK_HTTP_ENV: &str = "DDR_UPDATER_ALLOW_LOOPBACK_HTTP";

pub struct Options<'a> {
    /// `--packs-dir`.
    pub packs_dir: Option<&'a Path>,
    /// `--force`: compare hashes, not just presence and size.
    pub force: bool,
    /// `--check`: report, change nothing.
    pub check_only: bool,
    /// False when the base step already failed to reach GitHub.
    pub network: bool,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Report {
    /// A usable catalog is installed (there is something to choose from).
    pub has_catalog: bool,
    /// Selected packs that still need work after this step.
    pub pending: usize,
    /// A pack transaction's rollback failed.
    pub rollback_failed: bool,
    /// The status line for the prompt.
    pub status: Option<String>,
}

/// Everything the step reads from the game folder.
pub struct Loaded {
    pub catalog: Catalog,
    pub markers: BTreeMap<String, Marker>,
    pub selection: SelectionFile,
}

/// Read the catalog, markers and selection. `None` when the installed release
/// has no (usable) catalog — a base older than packs, or a damaged file.
pub fn load(game: &GameDir) -> Option<Loaded> {
    let allow_http = std::env::var_os(ALLOW_LOOPBACK_HTTP_ENV).is_some();
    let catalog = match catalog::read(&game.catalog_path(), allow_http) {
        Ok(Some(c)) if !c.packs.is_empty() => c,
        Ok(_) => return None,
        Err(e) => {
            log_warn!("{e}; optional packs are skipped this run");
            return None;
        }
    };
    let (markers, problems) = state::read_markers(&game.root);
    for p in problems {
        log_warn!("unreadable pack marker ignored ({p}); that pack counts as not installed");
    }
    let selection = state::read_selection(&game.selection_path());
    if let SelectionFile::Unreadable(why) = &selection {
        log_warn!(
            "{} could not be read ({why}); it was left untouched and the installed packs are treated as the selection",
            state::SELECTION_NAME
        );
    }
    Some(Loaded {
        catalog,
        markers,
        selection,
    })
}

fn size_on_disk(game: &GameDir) -> impl Fn(&RelPath) -> Option<u64> + '_ {
    move |rel| {
        fs::metadata(rel.to_path(&game.root))
            .ok()
            .filter(|m| m.is_file())
            .map(|m| m.len())
    }
}

fn hash_on_disk(game: &GameDir) -> impl Fn(&RelPath) -> Option<String> + '_ {
    move |rel| manifest::sha256_file(&rel.to_path(&game.root)).ok()
}

fn describe(work: &Work) -> String {
    match &work.reason {
        Reason::NotInstalled => "not installed".to_string(),
        Reason::NewRevision => "another revision is installed".to_string(),
        Reason::Damaged { missing, changed } => {
            format!("{missing} file(s) missing, {changed} changed")
        }
    }
}

fn verb(reason: &Reason) -> &'static str {
    match reason {
        Reason::NotInstalled => "Installing",
        Reason::NewRevision => "Updating",
        Reason::Damaged { .. } => "Repairing",
    }
}

fn label(pack: &Pack) -> String {
    format!("{} pack {}", pack.group.to_lowercase(), pack.title)
}

fn summary_line(summary: &Summary) -> String {
    // The marker is one of the written files; report the pack's own.
    let mut s = format!("{} files written", summary.written.saturating_sub(1));
    if summary.pruned > 0 {
        s.push_str(&format!(", {} obsolete files removed", summary.pruned));
    }
    if !summary.kept_modified.is_empty() {
        s.push_str(&format!(
            ", {} locally modified file(s) kept",
            summary.kept_modified.len()
        ));
    }
    s
}

fn report_rollback_failure(game: &GameDir, what: &str, cause: &str, restore_errors: &[String]) {
    log_error!("{what} failed ({cause}) and the rollback did not fully succeed:");
    for e in restore_errors {
        log_error!("  {e}");
    }
    log_error!(
        "the installation may be inconsistent; originals are in {}",
        game.backup_dir().display()
    );
}

/// Remove an installed pack. Returns `Err(())` only for a failed rollback.
fn remove_pack(game: &GameDir, session: &Session, marker: &Marker, why: &str) -> Result<(), ()> {
    match install::remove(game, session, marker, fault::from_env()) {
        Ok(summary) => {
            log_info!(
                "Removed pack {} ({why}): {} files removed",
                marker.id,
                summary.pruned.saturating_sub(1)
            );
            for rel in &summary.kept_modified {
                log_info!("kept locally modified file: {rel}");
            }
            Ok(())
        }
        Err(PackError::RollbackFailed {
            cause,
            restore_errors,
        }) => {
            report_rollback_failure(
                game,
                &format!("removing pack {}", marker.id),
                &cause,
                &restore_errors,
            );
            Err(())
        }
        Err(e) => {
            log_warn!("pack {} was not removed: {e}", marker.id);
            Ok(())
        }
    }
}

/// Delete downloads no pack in `wanted` refers to (superseded revisions,
/// deselected packs).
fn clean_downloads(game: &GameDir, wanted: &[&Pack]) {
    let dir = game.packs_dir();
    let Ok(entries) = fs::read_dir(&dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let asset = name.strip_suffix(".part").unwrap_or(&name);
        if !wanted.iter().any(|p| p.asset == asset) {
            let _ = fs::remove_file(entry.path());
        }
    }
    let _ = fs::remove_dir(&dir); // only succeeds when empty
}

/// The pack step.
pub fn run(game: &GameDir, session: &Session, opts: &Options<'_>) -> Report {
    let Some(loaded) = load(game) else {
        return Report::default();
    };
    let Loaded {
        catalog,
        markers,
        selection,
    } = loaded;
    let resolution = resolve::resolve(
        &catalog,
        &selection,
        &markers,
        &size_on_disk(game),
        &hash_on_disk(game),
        opts.force,
    );
    for id in &resolution.unknown {
        log_warn!("selected pack {id:?} is not offered by the installed release; ignored");
    }
    let status_for = |markers: &BTreeMap<String, Marker>, pending: usize| {
        resolve::status(&catalog, &resolution.selection, markers, pending).line()
    };

    if opts.check_only {
        for work in &resolution.install {
            log_info!("pack {} needs installing ({})", work.id, describe(work));
        }
        for id in &resolution.remove {
            log_info!("pack {id} would be removed (no longer offered by the installed release)");
        }
        let pending = resolution.install.len() + resolution.remove.len();
        return Report {
            has_catalog: true,
            pending,
            rollback_failed: false,
            status: Some(status_for(&markers, resolution.install.len())),
        };
    }

    if resolution.save_selection {
        if let Err(e) = state::write_selection(&game.selection_path(), &resolution.selection) {
            log_warn!("could not write {}: {e}", state::SELECTION_NAME);
        }
    }

    let mut report = Report {
        has_catalog: true,
        ..Report::default()
    };
    for id in &resolution.remove {
        let Some(marker) = markers.get(id) else {
            continue;
        };
        if remove_pack(game, session, marker, "no longer offered by this release").is_err() {
            report.rollback_failed = true;
            return report;
        }
    }

    let wanted: Vec<&Pack> = resolution
        .install
        .iter()
        .filter_map(|w| catalog.get(&w.id))
        .collect();
    clean_downloads(game, &wanted);

    let agent = opts.network.then(github::agent);
    let mut network_ok = opts.network;
    let mut not_attempted = 0usize;
    let mut failed = 0usize;
    for work in &resolution.install {
        let Some(pack) = catalog.get(&work.id) else {
            continue;
        };
        let in_packs_dir = opts
            .packs_dir
            .is_some_and(|dir| dir.join(&pack.asset).is_file());
        if !network_ok && !in_packs_dir {
            not_attempted += 1;
            continue;
        }
        let detail = match &work.reason {
            Reason::Damaged { .. } => format!(" ({})", describe(work)),
            _ => String::new(),
        };
        log_info!(
            "{} {} ({}){detail} ...",
            verb(&work.reason),
            label(pack),
            megabytes(pack.zip_size)
        );
        let sources = Sources {
            packs_dir: opts.packs_dir,
            agent: if network_ok { agent.as_ref() } else { None },
        };
        let mut progress = console::Progress::stdout();
        let obtained = install::obtain(game, pack, &sources, &mut |done, total| {
            progress.update(done, total)
        });
        progress.finish();
        let obtained = match obtained {
            Ok(o) => o,
            Err(e) => {
                if matches!(e, PackError::Unavailable { network: true, .. }) {
                    network_ok = false;
                }
                log_warn!(
                    "{} was not installed: {e}; it will be retried on the next run",
                    label(pack)
                );
                failed += 1;
                continue;
            }
        };
        if obtained.resumed_from > 0 {
            log_info!(
                "  resumed an interrupted download at {}",
                megabytes(obtained.resumed_from)
            );
        }
        let result = install::install(
            game,
            session,
            pack,
            &obtained.path,
            markers.get(&pack.id),
            fault::from_env(),
        );
        if obtained.downloaded {
            let _ = fs::remove_file(&obtained.path);
        }
        match result {
            Ok(summary) => {
                log_info!("Installed {}: {}", label(pack), summary_line(&summary));
                for rel in &summary.kept_modified {
                    log_info!("kept locally modified file (no longer part of the pack): {rel}");
                }
                let (new_markers, _) = state::read_markers(&game.root);
                if let Some(marker) = new_markers.get(&pack.id) {
                    for path in install::name_collisions(&game.root, pack, marker) {
                        log_warn!(
                            "{path} has the same model name as a model of the {}; the game will use only one of them",
                            label(pack)
                        );
                    }
                }
            }
            Err(PackError::RollbackFailed {
                cause,
                restore_errors,
            }) => {
                report_rollback_failure(
                    game,
                    &format!("installing the {}", label(pack)),
                    &cause,
                    &restore_errors,
                );
                report.rollback_failed = true;
                return report;
            }
            Err(e) => {
                log_warn!(
                    "{} was not installed: {e}; it will be retried on the next run",
                    label(pack)
                );
                failed += 1;
            }
        }
    }
    if not_attempted > 0 {
        log_warn!(
            "{not_attempted} pack(s) were not downloaded because GitHub could not be reached; they will be retried on the next run"
        );
    }
    clean_downloads(game, &wanted);

    let (markers_now, _) = state::read_markers(&game.root);
    report.pending = failed + not_attempted;
    report.status = Some(status_for(&markers_now, report.pending));
    report
}

/// What `--packs-install` asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// Every pack, including ones later releases add.
    All,
    None,
    List(Vec<String>),
}

/// `all`, `none`, or a comma-separated id list (blank items ignored).
pub fn parse_target(spec: &str) -> Target {
    match spec.trim().to_ascii_lowercase().as_str() {
        "all" => Target::All,
        "none" => Target::None,
        list => Target::List(
            list.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect(),
        ),
    }
}

/// The selection `target` describes, given what the installed release offers.
/// Returns it with the ids that are not offered.
pub fn selection_for(
    catalog: &Catalog,
    previous: &SelectionFile,
    target: &Target,
) -> (Selection, Vec<String>) {
    let mut selection = match previous {
        SelectionFile::Loaded(s) => s.clone(),
        _ => Selection::new(),
    };
    let mut unknown = Vec::new();
    let wanted: Vec<&str> = match target {
        Target::All => catalog.packs.iter().map(|p| p.id.as_str()).collect(),
        Target::None => Vec::new(),
        Target::List(ids) => {
            unknown = ids
                .iter()
                .filter(|id| catalog.get(id).is_none())
                .cloned()
                .collect();
            ids.iter().map(String::as_str).collect()
        }
    };
    // Catalog order, each id once.
    selection.packs = catalog
        .packs
        .iter()
        .filter(|p| wanted.contains(&p.id.as_str()))
        .map(|p| p.id.clone())
        .collect();
    selection.install_new = matches!(target, Target::All);
    for pack in &catalog.packs {
        if !selection.is_known(&pack.id) {
            selection.known.push(pack.id.clone());
        }
    }
    (selection, unknown)
}

/// Save `selection` and remove every installed pack of the catalog it does
/// not name. (An installed pack the release no longer offers was not on
/// offer to be deselected, and is left alone.) The installs follow in
/// [`run`]. Returns false only for a failed rollback.
pub fn apply_selection(
    game: &GameDir,
    session: &Session,
    catalog: &Catalog,
    markers: &BTreeMap<String, Marker>,
    selection: &Selection,
) -> bool {
    if let Err(e) = state::write_selection(&game.selection_path(), selection) {
        log_warn!(
            "could not write {}: {e}; the selection was not changed",
            state::SELECTION_NAME
        );
        return true;
    }
    log_info!(
        "Pack selection saved: {} pack(s){}",
        selection.packs.len(),
        if selection.install_new {
            ", plus packs added by future releases"
        } else {
            ""
        }
    );
    for (id, marker) in markers {
        if catalog.get(id).is_some()
            && !selection.is_selected(id)
            && remove_pack(game, session, marker, "deselected").is_err()
        {
            return false;
        }
    }
    true
}

/// `--packs-install`: set the selection, remove what it no longer names.
/// Returns false only for a failed rollback.
pub fn set_target(game: &GameDir, session: &Session, target: &Target) -> bool {
    let Some(loaded) = load(game) else {
        log_warn!(
            "there is no pack catalog in this game folder ({}); nothing to select",
            catalog::CATALOG_NAME
        );
        return true;
    };
    let (selection, unknown) = selection_for(&loaded.catalog, &loaded.selection, target);
    for id in unknown {
        log_warn!("unknown pack id {id:?} ignored (see --packs-list)");
    }
    apply_selection(game, session, &loaded.catalog, &loaded.markers, &selection)
}

/// Whether, and how, a run offers the chooser (design P13–P15).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Prompt {
    /// Not this run.
    None,
    /// The status line, then a countdown that Enter interrupts.
    Countdown,
    /// Straight into the chooser.
    Chooser,
}

/// What the invocation says about the prompt.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Invocation {
    /// `--check`.
    pub check: bool,
    /// `--packs-install` was given: the choice was made on the command line.
    pub scripted: bool,
    /// `--packs`.
    pub asked: bool,
    /// The updater is alone on its console (double-clicked).
    pub alone_on_console: bool,
}

/// The prompt is unconditional for a normal run: it is skipped only where it
/// cannot apply.
pub fn prompt_for(invocation: Invocation, report: &Report) -> Prompt {
    if invocation.check || invocation.scripted || report.rollback_failed || !report.has_catalog {
        Prompt::None
    } else if invocation.asked || invocation.alone_on_console {
        Prompt::Chooser
    } else {
        Prompt::Countdown
    }
}

/// How the chooser ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chosen {
    Unchanged,
    /// The selection was saved and deselected packs removed; [`run`] should
    /// follow to install the rest.
    Applied,
    RollbackFailed,
}

/// Open the chooser on `input`.
pub fn choose(game: &GameDir, session: &Session, input: &console::LineReader) -> Chosen {
    let Some(loaded) = load(game) else {
        log_warn!(
            "there is no pack catalog in this game folder ({}); nothing to choose from",
            catalog::CATALOG_NAME
        );
        return Chosen::Unchanged;
    };
    log_info!("Opening the pack chooser");
    let screen = chooser::Screen::open(&loaded.catalog, &loaded.selection, &loaded.markers);
    let seen = screen.selection(&loaded.selection);
    let decision = chooser::run(
        screen,
        &loaded.selection,
        input,
        &mut std::io::stdout(),
        chooser::IDLE_CAP,
    );
    match decision {
        chooser::Decision::Apply(screen) => {
            let selection = screen.selection(&loaded.selection);
            if apply_selection(game, session, &loaded.catalog, &loaded.markers, &selection) {
                Chosen::Applied
            } else {
                Chosen::RollbackFailed
            }
        }
        chooser::Decision::Unchanged => {
            log_info!("Pack chooser closed without changes");
            // The user has now seen every pack on offer: none of them is
            // "new" any longer. Only that is recorded.
            if let SelectionFile::Loaded(saved) = &loaded.selection {
                if saved.known != seen.known {
                    let mut noted = saved.clone();
                    noted.known = seen.known;
                    let _ = state::write_selection(&game.selection_path(), &noted);
                }
            }
            Chosen::Unchanged
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packs::catalog::tests::test_pack;
    use crate::packs::selection_of;

    fn catalog() -> Catalog {
        Catalog {
            packs: ["a", "b", "c"]
                .iter()
                .map(|id| test_pack(id, &id.to_uppercase(), &id.repeat(64)))
                .collect(),
        }
    }

    #[test]
    fn target_parsing() {
        assert_eq!(parse_target("all"), Target::All);
        assert_eq!(parse_target(" ALL "), Target::All);
        assert_eq!(parse_target("none"), Target::None);
        assert_eq!(
            parse_target("dancers-3rd-mix, stages-x-x2,,"),
            Target::List(vec!["dancers-3rd-mix".into(), "stages-x-x2".into()])
        );
        assert_eq!(parse_target(""), Target::List(vec![]));
    }

    #[test]
    fn selections_for_targets() {
        let c = catalog();
        let (all, unknown) = selection_for(&c, &SelectionFile::Absent, &Target::All);
        assert_eq!(all.packs, ["a", "b", "c"]);
        assert!(all.install_new && unknown.is_empty());
        assert_eq!(all.known, ["a", "b", "c"]);
        assert_eq!(all.schema, state::SELECTION_SCHEMA);

        let mut previous = selection_of(&["a"]);
        previous.install_new = true;
        previous.known = vec!["old".into()];
        let (none, _) = selection_for(&c, &SelectionFile::Loaded(previous.clone()), &Target::None);
        assert!(none.packs.is_empty() && !none.install_new);
        assert_eq!(none.known, ["old", "a", "b", "c"], "known only ever grows");

        let list = Target::List(vec!["c".into(), "nope".into(), "a".into(), "c".into()]);
        let (some, unknown) = selection_for(&c, &SelectionFile::Loaded(previous), &list);
        assert_eq!(some.packs, ["a", "c"], "catalog order, once each");
        assert_eq!(unknown, ["nope"]);
        assert!(!some.install_new);
    }

    #[test]
    fn the_prompt_is_unconditional_except_where_it_cannot_apply() {
        let ready = Report {
            has_catalog: true,
            ..Report::default()
        };
        let plain = Invocation::default();
        assert_eq!(prompt_for(plain, &ready), Prompt::Countdown);
        // Pending work, a status line, nothing selected: still asked.
        let busy = Report {
            pending: 3,
            status: Some("x".into()),
            ..ready.clone()
        };
        assert_eq!(prompt_for(plain, &busy), Prompt::Countdown);

        let with = |edit: fn(&mut Invocation)| {
            let mut i = Invocation::default();
            edit(&mut i);
            i
        };
        assert_eq!(
            prompt_for(with(|i| i.asked = true), &ready),
            Prompt::Chooser
        );
        assert_eq!(
            prompt_for(with(|i| i.alone_on_console = true), &ready),
            Prompt::Chooser
        );
        assert_eq!(prompt_for(with(|i| i.check = true), &ready), Prompt::None);
        assert_eq!(
            prompt_for(with(|i| i.scripted = true), &ready),
            Prompt::None
        );
        // --check wins over --packs.
        assert_eq!(
            prompt_for(
                with(|i| {
                    i.check = true;
                    i.asked = true
                }),
                &ready
            ),
            Prompt::None
        );
        // No catalog (a base older than packs), or a suspect folder.
        assert_eq!(
            prompt_for(with(|i| i.asked = true), &Report::default()),
            Prompt::None
        );
        let broken = Report {
            rollback_failed: true,
            ..ready
        };
        assert_eq!(prompt_for(plain, &broken), Prompt::None);
    }

    #[test]
    fn work_descriptions() {
        let w = |reason| Work {
            id: "a".into(),
            reason,
        };
        assert_eq!(describe(&w(Reason::NotInstalled)), "not installed");
        assert_eq!(
            describe(&w(Reason::Damaged {
                missing: 2,
                changed: 1
            })),
            "2 file(s) missing, 1 changed"
        );
        assert_eq!(verb(&Reason::NewRevision), "Updating");
        assert_eq!(label(&catalog().packs[0]), "dancers pack A");
    }
}
