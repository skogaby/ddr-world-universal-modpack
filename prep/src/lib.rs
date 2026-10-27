//! `ddr_world_hook_prep` — indexes StepMania packs for the DDR World hook
//! before the game starts (library half; `main.rs` is the entry point).
//!
//! The hook DLL loads StepMania songs only from the cache this tool writes
//! (`data_mods/_cache/stepmania/`): the DLL never converts. The tool mounts
//! the DLL's real pure pipeline from `src/` (`core/`, `services/stepmania/`)
//! so the cache is byte-for-byte what the game validates against.
//!
//! The default run indexes into the game folder's cache, prints what the
//! game will inject (the ceiling), and measures auto-sync for exactly those
//! songs. `--dry-run` converts a song or pack into a scratch folder with a
//! report.

#![allow(dead_code)]

pub mod log;

pub mod cli;
pub mod config;
pub mod console;
pub mod core;
pub mod dry_run;
pub mod gamedir;
pub mod indexer;
pub mod measure;
pub mod report;
pub mod running;
pub mod services;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Exit codes (design §4.1).
pub const EXIT_OK: u8 = 0;
pub const EXIT_GAME_RUNNING: u8 = 2;
pub const EXIT_NOT_A_GAME_FOLDER: u8 = 3;

/// The default run. Returns the exit code; everything that can go wrong
/// after the two gates is logged and still exits 0 (the game must start).
pub fn prep(argv: &[String], args: &cli::PrepArgs) -> u8 {
    let game = match gamedir::resolve(args.game_dir.as_deref()) {
        Ok(g) => g,
        Err(refusal) => {
            eprintln!("{refusal}");
            return EXIT_NOT_A_GAME_FOLDER;
        }
    };
    log::set_verbose(args.verbose);
    if let Err(e) = log::attach_file(&game.log_path()) {
        log_warn!(
            "could not open {} for writing ({e}); console output only",
            game.log_path().display()
        );
    }
    log::header(VERSION, argv, &game.root);

    if let Some(name) = running::game_is_running() {
        log_error!(
            "{name} is running -- the hook reads the cache this tool writes; close the game and run again. Nothing done."
        );
        return EXIT_GAME_RUNNING;
    }

    let (cfg, why) = config::load(&game.config_path());
    if let Some(why) = why {
        log_warn!("{why}");
    }
    if !cfg.stepmania_packs_enabled {
        log_info!("StepMania Packs is disabled in mod-config.json -- nothing to do");
        return EXIT_OK;
    }
    log_info!(
        "config: auto_sync={}{}",
        cfg.auto_sync,
        if cfg.song_limit_expansion_enabled {
            ""
        } else {
            " (Song Limit Expansion is OFF: the game injects no StepMania songs until it is on)"
        }
    );

    // The DLL's cache paths (`cache.rs`) and the songs root are relative to
    // the game folder, exactly as inside the game process.
    if let Err(e) = std::env::set_current_dir(&game.root) {
        log_error!("cannot enter the game folder ({e}); nothing done");
        return EXIT_OK;
    }
    let root = std::path::PathBuf::from(SONGS_ROOT);
    if let Err(e) = std::fs::create_dir_all(&root) {
        log_error!("cannot create {SONGS_ROOT} ({e}); nothing done");
        return EXIT_OK;
    }

    let mut settings = indexer::Settings::new(root);
    settings.workers = args.workers;
    if let Some(n) = args.checkpoint_every {
        settings.checkpoint_every = n;
    }
    let sink = report::ConsoleSink::new(args.abort_after);
    log_info!("indexing {SONGS_ROOT} ...");
    let outcome = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        indexer::run(&settings, &sink)
    })) {
        Ok(Ok(o)) => o,
        Ok(Err(e)) => {
            log_error!("index failed: {e}");
            return EXIT_OK;
        }
        Err(_) => {
            log_error!("index panicked -- see the log; whatever was checkpointed stays valid");
            return EXIT_OK;
        }
    };
    log_info!("{}", report::index_summary(&outcome));

    let mut index_file = outcome.file;
    let (lines, selection) =
        report::ceiling_report(&index_file.songs, cfg.song_limit_expansion_enabled);
    for l in &lines {
        log_info!("{l}");
    }

    if args.no_sync {
        log_info!("auto-sync: skipped (--no-sync); unmeasured songs play as authored");
    } else if !cfg.auto_sync {
        log_info!("auto-sync: off in mod-config.json; songs play as authored");
    } else if outcome.aborted {
        log_info!("auto-sync: skipped because the index was interrupted; run again");
    } else {
        let started = std::time::Instant::now();
        let msettings = measure::Settings {
            workers: args.workers.unwrap_or_else(indexer::default_workers),
            checkpoint_every: args.checkpoint_every.unwrap_or(250),
        };
        let msink = report::MeasureConsole::default();
        let stats = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            measure::run(
                &settings.root,
                &mut index_file,
                &selection.kept,
                &msettings,
                &msink,
            )
        })) {
            Ok(st) => st,
            Err(_) => {
                log_error!("auto-sync panicked -- see the log; sync.bin checkpoints stay valid");
                return EXIT_OK;
            }
        };
        log_info!("{}", report::measure_summary(&stats, started.elapsed()));
        if stats.failed > 0 {
            // Decode failures are recorded so the game rejects those songs.
            if !indexer::write_index_file(&index_file) {
                log_warn!("could not rewrite index.bin with the audio_failed flags");
            }
        }
    }
    // The series thumbnail arc for exactly the songs the game will inject
    // (after the measurement pass: a decode failure changes the set) — the
    // game would otherwise build it inside its first file open.
    {
        let budget = services::stepmania::ceiling::Budget::expanded();
        let sel = services::stepmania::ceiling::select_loadable(&index_file.songs, &budget);
        let kept: Vec<services::stepmania::model::SongEntry> = sel
            .kept
            .iter()
            .filter_map(|&i| index_file.songs.get(i).cloned())
            .collect();
        match services::stepmania::thumbnail_arc::ensure_logged(&kept, "thumbnail") {
            services::stepmania::thumbnail_arc::Outcome::UpToDate => {
                log_info!("thumbnail arc up to date ({} member(s))", kept.len());
            }
            services::stepmania::thumbnail_arc::Outcome::Failed => {
                log_warn!("could not write the thumbnail arc -- the game will build it at boot");
            }
            services::stepmania::thumbnail_arc::Outcome::Built { .. } => {}
        }
    }
    log_info!("done");
    EXIT_OK
}

/// Songs root, relative to the game folder (the DLL's `SONGS_ROOT`).
pub const SONGS_ROOT: &str = "./stepmania/Songs";
