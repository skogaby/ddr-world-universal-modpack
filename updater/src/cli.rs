//! Command-line parsing for `ddr_world_hook_updater`.
//!
//! Hand-written (a dozen flags; no CLI crate keeps the dependency tree and
//! the binary small). The contract that matters most is at the edges: any
//! malformed invocation yields [`Parsed::Usage`], which the caller prints and
//! exits 0 on — a mistyped `gamestart.bat` line must never stop the game from
//! launching.

use std::path::PathBuf;

/// Default GitHub repository (`owner/name`) the release feed is read from.
pub const REPO_DEFAULT: &str = "skogaby/ddr-world-universal-modpack";

/// Fully parsed invocation for a normal run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cli {
    /// `--game-dir <DIR>`: game folder override (default: the exe's folder).
    pub game_dir: Option<PathBuf>,
    /// `--check`: report whether an update is available; change nothing.
    pub check: bool,
    /// `--force`: reinstall the latest release even if already current.
    pub force: bool,
    /// `--include-prerelease`: consider pre-releases (newest by `published_at`).
    pub include_prerelease: bool,
    /// `--from-zip <PATH>`: install a local archive instead of downloading.
    pub from_zip: Option<PathBuf>,
    /// `--tag <NAME>`: tag to record with `--from-zip`.
    pub tag: Option<String>,
    /// `--repo <OWNER/NAME>`: GitHub repository override.
    pub repo: String,
}

impl Default for Cli {
    fn default() -> Self {
        Self {
            game_dir: None,
            check: false,
            force: false,
            include_prerelease: false,
            from_zip: None,
            tag: None,
            repo: REPO_DEFAULT.to_string(),
        }
    }
}

/// Result of parsing the argument list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Parsed {
    /// A normal run with the given options.
    Run(Cli),
    /// `--help` / `-h` was present (wins over everything else).
    Help,
    /// `--version` / `-V` was present (wins over everything but `--help`).
    Version,
    /// The invocation was malformed; the payload is the one-line explanation
    /// to print ahead of the usage text.
    Usage(String),
}

/// Usage text printed for `--help` and after a usage error.
pub fn usage() -> String {
    format!(
        "Usage: ddr_world_hook_updater [OPTIONS]\n\
         \n\
         Keeps a DDR World Universal Modpack installation current with the newest\n\
         GitHub release. Run it from gamestart.bat on the line before spice64.exe.\n\
         \n\
         Options:\n\
         \x20 --game-dir <DIR>        Game folder (default: the folder containing this exe)\n\
         \x20 --check                 Report whether an update is available; change nothing\n\
         \x20                         (exit 0 = current, 3 = update available)\n\
         \x20 --force                 Reinstall the latest release even if already current\n\
         \x20 --include-prerelease    Consider pre-releases (newest by published_at)\n\
         \x20 --from-zip <PATH>       Install this local archive instead of downloading\n\
         \x20 --tag <NAME>            Tag to record with --from-zip (default: local:<sha256 prefix>)\n\
         \x20 --repo <OWNER/NAME>     Override the GitHub repository (default: {REPO_DEFAULT})\n\
         \x20 --help                  Show this text\n\
         \x20 --version               Show the updater version\n"
    )
}

/// Parse the argument list (WITHOUT the program name).
pub fn parse<I>(args: I) -> Parsed
where
    I: IntoIterator<Item = String>,
{
    let args: Vec<String> = args.into_iter().collect();

    // Help and version win regardless of anything else on the line, so an
    // operator can always get the text even from a broken bat line.
    if args.iter().any(|a| a == "--help" || a == "-h") {
        return Parsed::Help;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        return Parsed::Version;
    }

    let mut cli = Cli::default();
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        // Split `--flag=value` into its two halves; `--flag` alone has no inline value.
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f.to_string(), Some(v.to_string())),
            _ => (arg.clone(), None),
        };
        match flag.as_str() {
            "--check" => cli.check = true,
            "--force" => cli.force = true,
            "--include-prerelease" => cli.include_prerelease = true,
            "--game-dir" => match take_value(&flag, inline, &mut iter) {
                Ok(v) => cli.game_dir = Some(PathBuf::from(v)),
                Err(msg) => return Parsed::Usage(msg),
            },
            "--from-zip" => match take_value(&flag, inline, &mut iter) {
                Ok(v) => cli.from_zip = Some(PathBuf::from(v)),
                Err(msg) => return Parsed::Usage(msg),
            },
            "--tag" => match take_value(&flag, inline, &mut iter) {
                Ok(v) => cli.tag = Some(v),
                Err(msg) => return Parsed::Usage(msg),
            },
            "--repo" => match take_value(&flag, inline, &mut iter) {
                Ok(v) => cli.repo = v,
                Err(msg) => return Parsed::Usage(msg),
            },
            other if other.starts_with('-') => {
                return Parsed::Usage(format!("unknown option: {other}"));
            }
            other => {
                return Parsed::Usage(format!("unexpected argument: {other}"));
            }
        }
    }

    if cli.tag.is_some() && cli.from_zip.is_none() {
        return Parsed::Usage("--tag only makes sense together with --from-zip".to_string());
    }
    if !repo_is_well_formed(&cli.repo) {
        return Parsed::Usage(format!(
            "--repo must look like OWNER/NAME (got: {})",
            cli.repo
        ));
    }

    Parsed::Run(cli)
}

/// Resolve the value of a valued flag: the inline `--flag=value` half if
/// present, else the next argument. Empty values are rejected so `--game-dir=`
/// cannot silently mean "the current directory".
fn take_value(
    flag: &str,
    inline: Option<String>,
    iter: &mut impl Iterator<Item = String>,
) -> Result<String, String> {
    let value = match inline {
        Some(v) => v,
        None => match iter.next() {
            Some(v) => v,
            None => return Err(format!("{flag} requires a value")),
        },
    };
    if value.is_empty() {
        return Err(format!("{flag} requires a non-empty value"));
    }
    Ok(value)
}

/// `OWNER/NAME` with exactly one slash and non-empty halves.
fn repo_is_well_formed(repo: &str) -> bool {
    match repo.split_once('/') {
        Some((owner, name)) => !owner.is_empty() && !name.is_empty() && !name.contains('/'),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Parsed {
        parse(args.iter().map(|s| s.to_string()))
    }

    fn run(args: &[&str]) -> Cli {
        match p(args) {
            Parsed::Run(cli) => cli,
            other => panic!("expected Run, got {other:?}"),
        }
    }

    fn usage_msg(args: &[&str]) -> String {
        match p(args) {
            Parsed::Usage(msg) => msg,
            other => panic!("expected Usage, got {other:?}"),
        }
    }

    #[test]
    fn t1_no_args_is_default_run() {
        assert_eq!(p(&[]), Parsed::Run(Cli::default()));
        assert_eq!(Cli::default().repo, REPO_DEFAULT);
        assert_eq!(Cli::default().tag, None);
    }

    #[test]
    fn t2_boolean_flags_alone() {
        assert!(run(&["--check"]).check);
        assert!(run(&["--force"]).force);
        assert!(run(&["--include-prerelease"]).include_prerelease);
        let c = run(&["--check"]);
        assert!(!c.force && !c.include_prerelease && c.game_dir.is_none());
    }

    #[test]
    fn t3_game_dir_both_syntaxes() {
        assert_eq!(run(&["--game-dir", "X"]).game_dir, Some(PathBuf::from("X")));
        assert_eq!(run(&["--game-dir=X"]).game_dir, Some(PathBuf::from("X")));
    }

    #[test]
    fn t4_from_zip_both_syntaxes() {
        assert_eq!(
            run(&["--from-zip", "p.zip"]).from_zip,
            Some(PathBuf::from("p.zip"))
        );
        assert_eq!(
            run(&["--from-zip=p.zip"]).from_zip,
            Some(PathBuf::from("p.zip"))
        );
    }

    #[test]
    fn t5_from_zip_with_tag() {
        let c = run(&["--from-zip", "p.zip", "--tag", "v9"]);
        assert_eq!(c.from_zip, Some(PathBuf::from("p.zip")));
        assert_eq!(c.tag, Some("v9".to_string()));
    }

    #[test]
    fn t6_repo_override() {
        assert_eq!(run(&["--repo", "a/b"]).repo, "a/b");
    }

    #[test]
    fn t7_all_flags_combined() {
        let c = run(&[
            "--game-dir",
            "G",
            "--check",
            "--force",
            "--include-prerelease",
            "--from-zip=r.zip",
            "--tag=v1",
            "--repo=o/n",
        ]);
        assert_eq!(
            c,
            Cli {
                game_dir: Some(PathBuf::from("G")),
                check: true,
                force: true,
                include_prerelease: true,
                from_zip: Some(PathBuf::from("r.zip")),
                tag: Some("v1".to_string()),
                repo: "o/n".to_string(),
            }
        );
    }

    #[test]
    fn t8_tag_without_from_zip_is_usage_error() {
        let msg = usage_msg(&["--tag", "v9"]);
        assert!(msg.contains("--tag") && msg.contains("--from-zip"), "{msg}");
    }

    #[test]
    fn t9_valued_flag_without_value_is_usage_error() {
        for flag in ["--game-dir", "--from-zip", "--tag", "--repo"] {
            let msg = usage_msg(&[flag]);
            assert!(msg.contains(flag), "{flag}: {msg}");
        }
    }

    #[test]
    fn t10_unknown_flag_is_usage_error() {
        assert!(usage_msg(&["--bogus"]).contains("--bogus"));
        assert!(usage_msg(&["-x"]).contains("-x"));
    }

    #[test]
    fn t11_stray_positional_is_usage_error() {
        assert!(usage_msg(&["stray"]).contains("stray"));
    }

    #[test]
    fn t12_malformed_repo_is_usage_error() {
        for repo in ["ab", "/b", "a/", "a/b/c"] {
            assert!(
                matches!(p(&["--repo", repo]), Parsed::Usage(_)),
                "{repo} should be rejected"
            );
        }
    }

    #[test]
    fn t13_help_wins_over_everything() {
        assert_eq!(p(&["--check", "--help"]), Parsed::Help);
        assert_eq!(p(&["--bogus", "--help"]), Parsed::Help);
        assert_eq!(p(&["--version", "--help"]), Parsed::Help);
        assert_eq!(p(&["-h"]), Parsed::Help);
    }

    #[test]
    fn t14_version_wins_over_run_and_errors() {
        assert_eq!(p(&["--check", "--version"]), Parsed::Version);
        assert_eq!(p(&["--bogus", "--version"]), Parsed::Version);
        assert_eq!(p(&["-V"]), Parsed::Version);
    }

    #[test]
    fn t15_repeated_valued_flag_last_wins() {
        assert_eq!(
            run(&["--game-dir", "A", "--game-dir", "B"]).game_dir,
            Some(PathBuf::from("B"))
        );
    }

    #[test]
    fn t16_empty_inline_value_is_usage_error() {
        assert!(usage_msg(&["--game-dir="]).contains("--game-dir"));
    }

    #[test]
    fn usage_text_documents_every_flag_as_an_option_line() {
        let text = usage();
        for flag in [
            "--game-dir",
            "--check",
            "--force",
            "--include-prerelease",
            "--from-zip",
            "--tag",
            "--repo",
            "--help",
            "--version",
        ] {
            // Each option is introduced on its own indented line; a flag may
            // additionally be mentioned inside another option's description.
            assert!(text.contains(&format!("\n  {flag}")), "{flag}");
        }
        assert!(text.starts_with("Usage:"));
    }
}
