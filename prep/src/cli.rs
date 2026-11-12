//! Argument parsing. Two commands share one binary: the default prep run
//! (index + sync into the game folder's cache) and `--dry-run` (the former
//! `sm_pack_check`, whose own options are parsed by `dry_run::parse_args`).

use std::path::PathBuf;

pub const USAGE: &str = "\
ddr_world_hook_prep — index StepMania packs for the DDR World hook before the game starts

USAGE:
  ddr_world_hook_prep [--game-dir <dir>] [--no-sync] [--workers <n>] [--verbose]
  ddr_world_hook_prep --dry-run <song-or-pack dir> --out <dir> [dry-run options]
  ddr_world_hook_prep --help

The default run reads stepmania/Songs/ under the game folder (the folder this exe
is in, or --game-dir), converts every new or changed song into the hook's cache
(data_mods/_cache/stepmania/), measures auto-sync for the songs the game will
inject, and prints what the game will load. A run with nothing to do takes about
a second. It refuses to run while the game is running.

OPTIONS:
  --game-dir <dir>   Game folder (must contain spice64.exe or ddr_world_hook.dll)
  --no-sync          Skip the auto-sync measurement pass
  --workers <n>      Conversion / measurement threads (default: CPU count, 2..6)
  --verbose          Also show per-song detail on the console (always in the log)
  --dry-run          Convert into a scratch directory and report; see --dry-run --help
  -h, --help

EXIT CODES: 0 done or nothing to do; 2 the game is running; 3 not a game folder.
Other failures are logged and exit 0 so gamestart.bat still starts the game.
";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Prep(PrepArgs),
    /// Arguments after `--dry-run`, handed to `dry_run::parse_args`.
    DryRun(Vec<String>),
    Help,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PrepArgs {
    pub game_dir: Option<PathBuf>,
    pub no_sync: bool,
    pub workers: Option<usize>,
    pub verbose: bool,
    /// Test hook (undocumented): stop taking jobs after this many conversions.
    pub abort_after: Option<usize>,
    /// Test hook (undocumented): checkpoint `index.bin` every n conversions.
    pub checkpoint_every: Option<usize>,
}

/// Parse `argv` (without the program name).
pub fn parse(argv: &[String]) -> Result<Command, String> {
    if let Some(pos) = argv.iter().position(|a| a == "--dry-run") {
        let mut rest: Vec<String> = argv[..pos].to_vec();
        rest.extend_from_slice(&argv[pos + 1..]);
        return Ok(Command::DryRun(rest));
    }
    let mut args = PrepArgs::default();
    let mut it = argv.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-h" | "--help" => return Ok(Command::Help),
            "--game-dir" => {
                args.game_dir = Some(PathBuf::from(it.next().ok_or("--game-dir needs a path")?));
            }
            "--no-sync" => args.no_sync = true,
            "--verbose" => args.verbose = true,
            "--workers" => {
                let n: usize = it
                    .next()
                    .ok_or("--workers needs a number")?
                    .parse()
                    .map_err(|_| "bad --workers value".to_string())?;
                if n == 0 {
                    return Err("--workers must be at least 1".into());
                }
                args.workers = Some(n);
            }
            "--abort-after" => {
                args.abort_after = Some(
                    it.next()
                        .ok_or("--abort-after needs a number")?
                        .parse()
                        .map_err(|_| "bad --abort-after value".to_string())?,
                );
            }
            "--checkpoint-every" => {
                let n: usize = it
                    .next()
                    .ok_or("--checkpoint-every needs a number")?
                    .parse()
                    .map_err(|_| "bad --checkpoint-every value".to_string())?;
                if n == 0 {
                    return Err("--checkpoint-every must be at least 1".into());
                }
                args.checkpoint_every = Some(n);
            }
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(Command::Prep(args))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &[&str]) -> Vec<String> {
        s.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn default_run_and_options() {
        assert_eq!(parse(&[]).unwrap(), Command::Prep(PrepArgs::default()));
        let c = parse(&v(&[
            "--no-sync",
            "--workers",
            "3",
            "--verbose",
            "--game-dir",
            "G",
        ]))
        .unwrap();
        assert_eq!(
            c,
            Command::Prep(PrepArgs {
                game_dir: Some(PathBuf::from("G")),
                no_sync: true,
                workers: Some(3),
                verbose: true,
                ..PrepArgs::default()
            })
        );
        assert!(parse(&v(&["--workers", "0"])).is_err());
        assert!(parse(&v(&["--bogus"])).is_err());
        assert_eq!(parse(&v(&["-h"])).unwrap(), Command::Help);
    }

    #[test]
    fn dry_run_takes_the_rest_wherever_the_flag_sits() {
        let c = parse(&v(&["Pack", "--dry-run", "--out", "o"])).unwrap();
        assert_eq!(c, Command::DryRun(v(&["Pack", "--out", "o"])));
        let c = parse(&v(&["--dry-run", "--help"])).unwrap();
        assert_eq!(c, Command::DryRun(v(&["--help"])));
    }
}
