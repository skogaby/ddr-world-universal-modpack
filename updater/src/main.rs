//! `ddr_world_hook_updater` — keeps a DDR World Universal Modpack installation
//! current with the newest GitHub release. Invoked from `gamestart.bat` on the
//! line before spice2x; a bare `.exe` call blocks the batch file until we exit.
//!
//! Exit-code contract (design §2.6): 0 for everything that leaves the game
//! startable — including usage errors, offline runs and updates that were
//! rolled back — 1 only when a rollback itself failed, 3 for `--check` when an
//! update is available. `main` is deliberately thin: parse → run → exit.

mod apply;
mod archive;
mod changelog;
mod cli;
mod console;
mod download;
mod fault;
mod gamedir;
mod github;
mod log;
mod manifest;
mod merge;
mod plan;
mod relpath;
mod selfupdate;

use std::panic::{self, AssertUnwindSafe, UnwindSafe};
use std::path::Path;
use std::process;

use cli::{Cli, Parsed};

/// How many cleaned release-notes lines to show before downloading.
const CHANGELOG_LINES: usize = 15;
use gamedir::GameDir;
use plan::{DiskState, MergedFiles};
use relpath::RelPath;

/// Everything a run can end in. Mapped to a process exit code by
/// [`exit_code`] — the ONLY place exit codes are decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// An update was installed.
    Ok,
    /// Nothing needed doing (already current, or no implemented mode requested).
    NothingToDo,
    /// `--check` found a newer release (changed nothing).
    UpdateAvailable,
    /// The run was skipped (offline, API error, refusal, rolled-back failure).
    Skipped,
    /// A rollback failed; the installation may be inconsistent.
    RollbackFailed,
    /// Malformed command line; usage was printed.
    Usage,
    /// `--help` printed.
    Help,
    /// `--version` printed.
    Version,
    /// The run panicked; the message was printed and the game may start.
    Crashed,
}

/// The single exit-code mapping.
pub fn exit_code(outcome: Outcome) -> i32 {
    match outcome {
        Outcome::RollbackFailed => 1,
        Outcome::UpdateAvailable => 3,
        Outcome::Ok
        | Outcome::NothingToDo
        | Outcome::Skipped
        | Outcome::Usage
        | Outcome::Help
        | Outcome::Version
        | Outcome::Crashed => 0,
    }
}

/// Run `f`, containing any panic. Returns the outcome plus the panic message
/// when one occurred. The default panic hook is replaced beforehand by
/// [`install_quiet_panic_hook`] so the console shows one clean line instead of
/// a backtrace.
pub fn run_guarded<F>(f: F) -> (Outcome, Option<String>)
where
    F: FnOnce() -> Outcome + UnwindSafe,
{
    match panic::catch_unwind(f) {
        Ok(outcome) => (outcome, None),
        Err(payload) => {
            let msg = payload
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic payload".to_string());
            (Outcome::Crashed, Some(msg))
        }
    }
}

/// Suppress the default panic output (message + backtrace hint); the guarded
/// runner reports the payload itself.
fn install_quiet_panic_hook() {
    panic::set_hook(Box::new(|_| {}));
}

fn version_line() -> String {
    format!("ddr_world_hook_updater {}", env!("CARGO_PKG_VERSION"))
}

fn run(parsed: Parsed, argv: &[String]) -> Outcome {
    match parsed {
        Parsed::Help => {
            print!("{}", cli::usage());
            Outcome::Help
        }
        Parsed::Version => {
            println!("{}", version_line());
            Outcome::Version
        }
        Parsed::Usage(msg) => {
            println!("{msg}");
            println!();
            print!("{}", cli::usage());
            Outcome::Usage
        }
        Parsed::Run(cli) => run_in_game_folder(cli, argv),
    }
}

/// The part of a run that needs a game folder: resolve and gate it, attach the
/// log, then do the work. A refusal prints one line and creates nothing in the
/// untrusted folder.
fn run_in_game_folder(cli: Cli, argv: &[String]) -> Outcome {
    let game = match gamedir::resolve(cli.game_dir.as_deref()) {
        Ok(game) => game,
        Err(refusal) => {
            println!("{refusal}");
            return Outcome::Skipped;
        }
    };
    if let Err(e) = log::attach_file(&game.log_path()) {
        log_warn!(
            "could not open {} for writing ({e}); continuing with console output only",
            game.log_path().display()
        );
    }
    log::header(env!("CARGO_PKG_VERSION"), argv, &game.root);
    log_info!("DDR World Hook updater {}", env!("CARGO_PKG_VERSION"));
    log_info!("Game folder: {}", game.root.display());

    // Leftover from a previous self-update (the old image could not be deleted
    // while it was still running).
    if selfupdate::cleanup_stale(&game.root.join(plan::UPDATER_EXE)) {
        log_info!("removed the previous updater image left by a self-update");
    }
    // A journal means the previous run died mid-apply: put the folder back
    // before doing anything else (design §6.4).
    match apply::recover_if_interrupted(&game) {
        apply::Recovery::Clean => {}
        apply::Recovery::Recovered {
            restored,
            removed,
            errors,
        } => {
            log_warn!(
                "the previous run was interrupted mid-update; restored {restored} file(s), removed {removed} partial file(s)"
            );
            for e in &errors {
                log_warn!("  recovery: {e}");
            }
        }
    }

    if cli.from_zip.is_some() {
        return run_from_zip(&cli, &game);
    }
    run_default(&cli, &game)
}

/// The normal run (design §3.1): ask GitHub for the latest release, decide via
/// the manifest, download + verify, then the shared install pipeline. Every
/// network or API failure is one "skipped" line and exit 0 — the game must
/// start regardless.
fn run_default(cli: &Cli, game: &GameDir) -> Outcome {
    log_info!(
        "Checking github.com/{}{} ...",
        cli.repo,
        if cli.include_prerelease {
            " (including pre-releases)"
        } else {
            ""
        }
    );
    let agent = github::agent();
    let release = match github::fetch_latest(&agent, &cli.repo, cli.include_prerelease) {
        Ok(r) => r,
        Err(e) => {
            log_warn!("skipped ({e}) -- starting the game with the installed version");
            return Outcome::Skipped;
        }
    };
    let Some(asset) = github::select_asset(&release) else {
        log_warn!(
            "skipped (release {} has no {}*.zip asset) -- starting the game with the installed version",
            release.tag_name,
            github::ASSET_PREFIX
        );
        return Outcome::Skipped;
    };
    if github::matching_asset_count(&release) > 1 {
        log_warn!(
            "release {} has several zip assets; using {}",
            release.tag_name,
            asset.name
        );
    }
    let digest = asset.digest.as_deref().and_then(github::parse_digest);
    if digest.is_none() {
        log_warn!(
            "release {} publishes no sha256 digest for {}; only the size will be verified",
            release.tag_name,
            asset.name
        );
    }

    // Decide before downloading anything.
    let current = match manifest::read(&game.manifest_path()) {
        Ok(m) => m,
        Err(e) => {
            log_warn!("ignoring unusable manifest: {e}");
            None
        }
    };
    let up_to_date = match (&current, &digest) {
        (Some(m), Some(d)) => m.tag == release.tag_name && m.asset_sha256.eq_ignore_ascii_case(d),
        // Without a published digest the tag is all we can compare.
        (Some(m), None) => m.tag == release.tag_name,
        (None, _) => false,
    };
    if up_to_date && !cli.force {
        log_info!("up to date ({})", release.tag_name);
        return Outcome::NothingToDo;
    }
    let installed = current
        .as_ref()
        .map(|m| m.tag.as_str())
        .unwrap_or("nothing");
    if cli.check {
        log_info!(
            "update available: {} (installed: {installed})",
            release.tag_name
        );
        return Outcome::UpdateAvailable;
    }
    log_info!(
        "update available: {} (installed: {installed})",
        release.tag_name
    );
    if let Some(name) = &release.name {
        log_info!("  {name}");
    }
    if let Some(body) = &release.body {
        for line in changelog::preview(body, CHANGELOG_LINES) {
            log_info!("    {line}");
        }
    }
    if let Some(url) = &release.html_url {
        log_info!("  full notes: {url}");
    }

    let mb = asset.size as f64 / (1024.0 * 1024.0);
    log_info!("Downloading {} ({mb:.1} MB) ...", asset.name);
    let mut last_pct: i64 = -1;
    let mut progress = |done: u64, total: u64| {
        if total == 0 {
            return;
        }
        let pct = (done * 100 / total) as i64;
        if pct / 10 != last_pct / 10 || pct == 100 && last_pct != 100 {
            log_info!("  {pct}%");
            last_pct = pct;
        }
    };
    let downloaded = match download::fetch_asset(&agent, asset, &game.download_dir(), &mut progress)
    {
        Ok(d) => d,
        Err(e) => {
            log_warn!(
                "skipped (download failed: {e}) -- starting the game with the installed version"
            );
            let _ = std::fs::remove_dir_all(game.download_dir());
            return Outcome::Skipped;
        }
    };
    log_info!(
        "Verified {} bytes{}",
        downloaded.bytes,
        if downloaded.size_only {
            " (size only; no published digest)"
        } else {
            " against the published sha256"
        }
    );

    install(
        game,
        &release.tag_name,
        release.name.as_deref(),
        &asset.name,
        &downloaded.sha256_hex,
        &downloaded.path,
        cli.force,
        false,
    )
}

/// Install a local release archive through the full pipeline (design R27):
/// identity → needs-update → extract → merge inputs → plan → transactional apply.
fn run_from_zip(cli: &Cli, game: &GameDir) -> Outcome {
    let zip = cli.from_zip.as_deref().expect("caller checked from_zip");
    if !zip.is_file() {
        log_warn!("skipped: {} is not a file", zip.display());
        return Outcome::Skipped;
    }
    let asset_sha256 = match manifest::sha256_file(zip) {
        Ok(h) => h,
        Err(e) => {
            log_warn!("skipped: cannot read {}: {e}", zip.display());
            return Outcome::Skipped;
        }
    };
    let tag = cli
        .tag
        .clone()
        .unwrap_or_else(|| format!("local:{}", &asset_sha256[..12]));
    let asset_name = zip
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "local.zip".to_string());

    install(
        game,
        &tag,
        None,
        &asset_name,
        &asset_sha256,
        zip,
        cli.force,
        cli.check,
    )
}

/// The shared install pipeline from "we have a verified archive on disk".
#[allow(clippy::too_many_arguments)]
fn install(
    game: &GameDir,
    tag: &str,
    release_name: Option<&str>,
    asset_name: &str,
    asset_sha256: &str,
    zip: &Path,
    force: bool,
    check_only: bool,
) -> Outcome {
    let current = match manifest::read(&game.manifest_path()) {
        Ok(m) => m,
        Err(e) => {
            log_warn!("ignoring unusable manifest: {e}");
            None
        }
    };
    if !manifest::needs_update(current.as_ref(), tag, asset_sha256, force) {
        log_info!("up to date ({tag})");
        return Outcome::NothingToDo;
    }
    if check_only {
        log_info!(
            "update available: {tag} (installed: {})",
            current.map(|m| m.tag).unwrap_or_else(|| "nothing".into())
        );
        return Outcome::UpdateAvailable;
    }

    log_info!("Extracting {asset_name} ...");
    let staged = match archive::extract(zip, &game.stage_dir()) {
        Ok(s) => s,
        Err(e) => {
            log_warn!("skipped: {e}");
            let _ = std::fs::remove_dir_all(game.stage_dir());
            return Outcome::Skipped;
        }
    };
    log_info!("Extracted {} files", staged.files.len());

    let stage_hashes = match manifest::hash_tree(&staged.root, &staged.files) {
        Ok(h) => h,
        Err(e) => {
            log_warn!("skipped: cannot hash the extracted release: {e}");
            let _ = std::fs::remove_dir_all(game.stage_dir());
            return Outcome::Skipped;
        }
    };

    let merged = merge_inputs(game, &staged.root);

    let probe = |rel: &RelPath| -> DiskState {
        let path = rel.to_path(&game.root);
        if !path.is_file() {
            return DiskState::Missing;
        }
        match manifest::sha256_file(&path) {
            Ok(sha256) => DiskState::Present { sha256 },
            // Unreadable ⇒ report a hash that matches nothing, so the plan can
            // only ever KEEP it (never prune).
            Err(_) => DiskState::Present {
                sha256: String::new(),
            },
        }
    };
    let plan = plan::build(
        &staged.files,
        &stage_hashes,
        current.as_ref(),
        &probe,
        &merged,
    );
    let new_manifest = manifest::Manifest::new(
        tag,
        release_name,
        asset_name,
        asset_sha256,
        plan.new_manifest_files.clone(),
    );

    log_info!("Installing ...");
    match apply::execute(
        game,
        &plan,
        &staged.root,
        &merged,
        &new_manifest,
        fault::from_env(),
    ) {
        Ok(summary) => {
            log_info!("Installed: {}", summary.install_line());
            for rel in &summary.kept_modified {
                log_info!("kept locally modified file (no longer shipped): {rel}");
            }
            if summary.dirs_removed > 0 {
                log_info!("removed {} emptied folder(s)", summary.dirs_removed);
            }
            if summary.self_updated {
                log_info!(
                    "The updater itself was replaced; the new version runs from the next launch."
                );
            }
            log_info!("Updated to {tag}.");
            Outcome::Ok
        }
        Err(apply::ApplyError::RolledBack(cause)) => {
            log_warn!(
                "update failed and was rolled back ({cause}); the installed version is unchanged"
            );
            Outcome::Skipped
        }
        Err(apply::ApplyError::RollbackFailed {
            cause,
            restore_errors,
        }) => {
            log_error!("update failed ({cause}) and the rollback did not fully succeed:");
            for e in &restore_errors {
                log_error!("  {e}");
            }
            log_error!(
                "the installation may be inconsistent; originals are in {}",
                game.backup_dir().display()
            );
            Outcome::RollbackFailed
        }
    }
}

/// Bytes for the two merged files (design §4.6–§4.8): the release copy when
/// the user has no file yet, the merged document when the merge changed
/// anything, `None` when nothing needs writing. An unparseable user file is
/// copied to `backup/` and left alone (R15).
fn merge_inputs(game: &GameDir, stage_root: &Path) -> MergedFiles {
    MergedFiles {
        config: merge_one(game, stage_root, "mod-config.json", merge_config_bytes),
        csv: merge_one(game, stage_root, "judgement_offsets.csv", merge_csv_bytes),
    }
}

/// Outcome of merging one file's bytes.
enum MergeOutcome {
    /// New bytes to write, plus the report line to log.
    Changed(Vec<u8>, String),
    /// Nothing to write.
    Unchanged,
    /// The user's file could not be parsed; leave it alone.
    UserUnparseable(String),
    /// The release's copy could not be parsed; leave the user's file alone.
    ReleaseUnparseable(String),
}

fn merge_one(
    game: &GameDir,
    stage_root: &Path,
    name: &str,
    merge: fn(&[u8], &[u8]) -> MergeOutcome,
) -> Option<Vec<u8>> {
    let user_path = game.root.join(name);
    let release_bytes = match std::fs::read(stage_root.join(name)) {
        Ok(b) => b,
        Err(_) => return None, // the release does not ship it: nothing to merge
    };
    let user_bytes = match std::fs::read(&user_path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            log_info!("{name}: not present yet, installing the release copy");
            return Some(release_bytes);
        }
        Err(e) => {
            log_warn!("{name}: cannot read ({e}); leaving it untouched");
            return None;
        }
    };
    match merge(&user_bytes, &release_bytes) {
        MergeOutcome::Changed(bytes, line) => {
            log_info!("Merging {name} ... {line}");
            Some(bytes)
        }
        MergeOutcome::Unchanged => {
            log_info!("Merging {name} ... no changes");
            None
        }
        MergeOutcome::UserUnparseable(why) => {
            // Not under backup/ (which every apply resets): a sibling dir that
            // keeps the last unparseable copy of each file.
            let dir = game.work.join("unparseable");
            let copy = dir.join(name);
            let copied = std::fs::create_dir_all(&dir)
                .and_then(|_| std::fs::copy(&user_path, &copy))
                .is_ok();
            log_warn!(
                "{name}: your file could not be parsed ({why}); it was left untouched{} and NOT merged with the release copy",
                if copied {
                    format!(" (copy saved to {})", copy.display())
                } else {
                    String::new()
                }
            );
            None
        }
        MergeOutcome::ReleaseUnparseable(why) => {
            log_warn!("{name}: the release copy could not be parsed ({why}); your file was left untouched");
            None
        }
    }
}

fn merge_config_bytes(user: &[u8], release: &[u8]) -> MergeOutcome {
    let user_doc: serde_json::Value = match serde_json::from_slice(user) {
        Ok(v) => v,
        Err(e) => return MergeOutcome::UserUnparseable(e.to_string()),
    };
    let release_doc: serde_json::Value = match serde_json::from_slice(release) {
        Ok(v) => v,
        Err(e) => return MergeOutcome::ReleaseUnparseable(e.to_string()),
    };
    let (merged, report) = merge::json::merge_config(&user_doc, &release_doc);
    if report.is_empty() {
        return MergeOutcome::Unchanged;
    }
    let mut parts = Vec::new();
    if !report.added.is_empty() {
        let shown: Vec<&str> = report.added.iter().take(3).map(String::as_str).collect();
        let more = if report.added.len() > 3 { ", …" } else { "" };
        parts.push(format!(
            "{} key{} added ({}{more})",
            report.added.len(),
            if report.added.len() == 1 { "" } else { "s" },
            shown.join(", ")
        ));
    }
    if !report.menu_rows_added.is_empty() {
        parts.push(format!(
            "{} menu row{} placed ({})",
            report.menu_rows_added.len(),
            if report.menu_rows_added.len() == 1 {
                ""
            } else {
                "s"
            },
            report.menu_rows_added.join(", ")
        ));
    }
    MergeOutcome::Changed(
        merge::json::to_pretty_json(&merged).into_bytes(),
        parts.join(", "),
    )
}

fn merge_csv_bytes(user: &[u8], release: &[u8]) -> MergeOutcome {
    let Ok(user_text) = std::str::from_utf8(user) else {
        return MergeOutcome::UserUnparseable("not UTF-8".to_string());
    };
    let Ok(release_text) = std::str::from_utf8(release) else {
        return MergeOutcome::ReleaseUnparseable("not UTF-8".to_string());
    };
    let (mut user_doc, user_stats) = merge::csv_grammar::parse(user_text);
    if !user_stats.is_clean() {
        // The DLL tolerates and drops bad lines on its own rewrite; the updater
        // still reports them so an operator can see what would be lost.
        log_warn!(
            "judgement_offsets.csv: {} line(s) skipped, {} duplicate(s), {} value(s) clamped while reading your file (lines {:?})",
            user_stats.skipped,
            user_stats.duplicates,
            user_stats.clamped,
            user_stats.bad_lines
        );
    }
    let (release_doc, _) = merge::csv_grammar::parse(release_text);
    let report = merge::csv::merge_csv(&mut user_doc, &release_doc);
    if !report.changed() {
        return MergeOutcome::Unchanged;
    }
    MergeOutcome::Changed(
        merge::csv_grammar::serialize(&user_doc).into_bytes(),
        format!(
            "{} offset{} filled, {} song{} added",
            report.cells_filled,
            if report.cells_filled == 1 { "" } else { "s" },
            report.rows_appended,
            if report.rows_appended == 1 { "" } else { "s" }
        ),
    )
}

fn main() {
    install_quiet_panic_hook();
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let parsed = cli::parse(argv.iter().cloned());
    let (outcome, panic_msg) = run_guarded(AssertUnwindSafe(move || run(parsed, &argv)));
    if let Some(msg) = panic_msg {
        eprintln!("updater crashed internally: {msg}");
    }
    // Double-clicked from Explorer? Keep the window until the operator has read it.
    if console::owns_console_alone() {
        console::wait_for_enter();
    }
    process::exit(exit_code(outcome));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn m1_exit_codes() {
        assert_eq!(exit_code(Outcome::RollbackFailed), 1);
        assert_eq!(exit_code(Outcome::UpdateAvailable), 3);
        for o in [
            Outcome::Ok,
            Outcome::NothingToDo,
            Outcome::Skipped,
            Outcome::Usage,
            Outcome::Help,
            Outcome::Version,
            Outcome::Crashed,
        ] {
            assert_eq!(exit_code(o), 0, "{o:?}");
        }
    }

    #[test]
    fn m2_panic_is_contained_and_reported() {
        install_quiet_panic_hook();
        let (outcome, msg) = run_guarded(|| -> Outcome { panic!("boom {}", 42) });
        assert_eq!(outcome, Outcome::Crashed);
        assert!(msg.as_deref().unwrap_or("").contains("boom 42"), "{msg:?}");
        assert_eq!(exit_code(outcome), 0);
    }

    #[test]
    fn m3_outcome_passes_through() {
        let (outcome, msg) = run_guarded(|| Outcome::NothingToDo);
        assert_eq!(outcome, Outcome::NothingToDo);
        assert_eq!(msg, None);
    }

    #[test]
    fn m5_version_line() {
        assert_eq!(
            version_line(),
            format!("ddr_world_hook_updater {}", env!("CARGO_PKG_VERSION"))
        );
    }
}
