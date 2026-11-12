//! End-to-end: the prep binary against a synthetic game folder.
//!
//! cold run → artefacts + `index.bin`; warm run → nothing rewritten; a
//! simfile edit → that song alone re-converts; a removed song → artefacts
//! gone; an aborted run → a valid checkpoint the next run reuses; the
//! disabled-mod, not-a-game-folder and ceiling-report paths.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ddr_world_hook_prep::services::stepmania::model::{parse_index, IndexFile};

const BIN: &str = env!("CARGO_BIN_EXE_ddr_world_hook_prep");

struct Game {
    root: PathBuf,
}

impl Game {
    fn new(tag: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "ddr_prep_e2e_{tag}_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("stepmania/Songs")).unwrap();
        fs::write(root.join("spice64.exe"), b"").unwrap();
        Game { root }
    }

    fn songs(&self) -> PathBuf {
        self.root.join("stepmania/Songs")
    }

    fn cache(&self) -> PathBuf {
        self.root.join("data_mods/_cache/stepmania")
    }

    fn add_song(&self, pack: &str, folder: &str, title: &str, bpm: u32) -> PathBuf {
        let dir = self.songs().join(pack).join(folder);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("song.ssc"), ssc(title, bpm)).unwrap();
        fs::write(dir.join("song.wav"), wav(4)).unwrap();
        dir
    }

    fn run(&self, extra: &[&str]) -> (i32, String) {
        let out = Command::new(BIN)
            .arg("--game-dir")
            .arg(&self.root)
            .args(extra)
            .env("RUST_BACKTRACE", "0")
            .output()
            .expect("spawn prep");
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        (out.status.code().unwrap_or(-1), text)
    }

    fn index(&self) -> IndexFile {
        parse_index(&fs::read(self.cache().join("index.bin")).expect("index.bin")).expect("parse")
    }

    fn mtime(&self, rel: &str) -> SystemTime {
        fs::metadata(self.cache().join(rel))
            .unwrap()
            .modified()
            .unwrap()
    }
}

impl Drop for Game {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn ssc(title: &str, bpm: u32) -> String {
    format!(
        "#VERSION:0.83;\n#TITLE:{title};\n#ARTIST:e2e;\n#MUSIC:song.wav;\n#OFFSET:0.000;\n\
#SAMPLESTART:0.5;\n#SAMPLELENGTH:2.0;\n#BPMS:0.000={bpm}.000;\n\
#NOTEDATA:;\n#STEPSTYPE:dance-single;\n#DIFFICULTY:Hard;\n#METER:7;\n#NOTES:\n\
1000\n0100\n0010\n0001\n,\n1100\n0000\n0011\n0000\n;\n"
    )
}

/// `secs` of 16-bit stereo 44.1 kHz silence with a click on each beat.
fn wav(secs: u32) -> Vec<u8> {
    let rate = 44_100u32;
    let n = rate * secs;
    let mut data = Vec::with_capacity(n as usize * 4);
    for i in 0..n {
        let s: i16 = if i % (rate / 2) < 200 { 8000 } else { 0 };
        data.extend_from_slice(&s.to_le_bytes());
        data.extend_from_slice(&s.to_le_bytes());
    }
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 4).to_le_bytes());
    out.extend_from_slice(&4u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&data);
    out
}

fn artefacts_of(game: &Game, basename: &str) -> [PathBuf; 3] {
    let c = game.cache();
    [
        c.join("ssq").join(format!("{basename}.ssq")),
        c.join("xsb").join(format!("{basename}.xsb")),
        c.join("tn").join(format!("{basename}.dds")),
    ]
}

fn bump_mtime(path: &Path) {
    // Ensure a visible mtime change even on coarse filesystems.
    std::thread::sleep(Duration::from_millis(1100));
    let text = fs::read_to_string(path).unwrap();
    fs::write(path, text.replace("#ARTIST:e2e;", "#ARTIST:e2e edited;")).unwrap();
}

#[test]
fn cold_warm_edit_remove() {
    let game = Game::new("cwer");
    game.add_song("Pack A", "Alpha", "Alpha", 150);
    game.add_song("Pack A", "Beta", "Beta", 120);
    game.add_song("Pack B", "Gamma", "Gamma", 180);

    // Cold.
    let (code, text) = game.run(&[]);
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("3 to convert"), "{text}");
    assert!(
        text.contains(
            "index complete -- 3 song(s), 2 pack(s), 0 rejected (cold; 3 converted, 0 reused)"
        ),
        "{text}"
    );
    assert!(
        text.contains("the game will inject all 3 indexed song(s)"),
        "{text}"
    );
    let idx = game.index();
    assert_eq!(idx.songs.len(), 3);
    for s in &idx.songs {
        for p in artefacts_of(&game, &s.basename) {
            assert!(p.is_file(), "{}", p.display());
        }
    }
    let alpha = idx
        .songs
        .iter()
        .find(|s| s.folder == "Alpha")
        .unwrap()
        .clone();
    let gamma = idx
        .songs
        .iter()
        .find(|s| s.folder == "Gamma")
        .unwrap()
        .clone();
    let index_bytes = fs::read(game.cache().join("index.bin")).unwrap();
    let alpha_ssq_mtime = game.mtime(&format!("ssq/{}.ssq", alpha.basename));
    let index_mtime = game.mtime("index.bin");

    // Warm: nothing rewritten.
    std::thread::sleep(Duration::from_millis(1100));
    let (code, text) = game.run(&[]);
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("everything is already cached"), "{text}");
    assert!(text.contains("(warm; 0 converted, 3 reused)"), "{text}");
    assert_eq!(
        fs::read(game.cache().join("index.bin")).unwrap(),
        index_bytes
    );
    assert_eq!(
        game.mtime("index.bin"),
        index_mtime,
        "index.bin rewritten on a warm run"
    );
    assert_eq!(
        game.mtime(&format!("ssq/{}.ssq", alpha.basename)),
        alpha_ssq_mtime
    );

    // Edit Alpha's simfile: Alpha alone re-converts.
    bump_mtime(&game.songs().join("Pack A/Alpha/song.ssc"));
    let (code, text) = game.run(&[]);
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("1 to convert"), "{text}");
    assert!(text.contains("(warm; 1 converted, 2 reused)"), "{text}");
    let idx2 = game.index();
    let alpha2 = idx2.songs.iter().find(|s| s.folder == "Alpha").unwrap();
    assert_eq!(
        alpha2.basename, alpha.basename,
        "identity is a function of the path"
    );
    assert_ne!(alpha2.fingerprint, alpha.fingerprint);
    assert_eq!(alpha2.artist, "e2e edited");

    // Remove Gamma: its artefacts go, its record goes.
    fs::remove_dir_all(game.songs().join("Pack B")).unwrap();
    let (code, text) = game.run(&[]);
    assert_eq!(code, 0, "{text}");
    assert!(
        text.contains("'Pack B/Gamma' removed -- artefacts deleted"),
        "{text}"
    );
    let idx3 = game.index();
    assert_eq!(idx3.songs.len(), 2);
    assert!(idx3.songs.iter().all(|s| s.folder != "Gamma"));
    for p in artefacts_of(&game, &gamma.basename) {
        assert!(!p.exists(), "{} should be gone", p.display());
    }
}

#[test]
fn rejections_are_cached_and_reported() {
    let game = Game::new("rej");
    game.add_song("P", "Good", "Good", 140);
    let bad = game.songs().join("P/Bad");
    fs::create_dir_all(&bad).unwrap();
    fs::write(bad.join("bad.sm"), "#TITLE:Bad;\n#MUSIC:nope.ogg;\n").unwrap();

    let (code, text) = game.run(&[]);
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("rejected 'P/Bad':"), "{text}");
    assert!(
        text.contains("1 rejected (cold; 1 converted, 0 reused)"),
        "{text}"
    );
    let idx = game.index();
    assert_eq!(idx.songs.len(), 1);
    assert_eq!(idx.rejects.len(), 1);
    assert_eq!(idx.rejects[0].folder, "Bad");

    // Warm: the rejection is re-reported from the cache (no re-parse needed).
    let (_, text) = game.run(&[]);
    assert!(text.contains("rejected 'P/Bad':"), "{text}");
    assert!(text.contains("everything is already cached"), "{text}");
}

#[test]
fn an_aborted_run_leaves_a_checkpoint_the_next_run_reuses() {
    let game = Game::new("abort");
    for i in 0..6 {
        game.add_song("P", &format!("S{i}"), &format!("S{i}"), 100 + i * 10);
    }
    // One worker, checkpoint after every conversion, stop after two.
    let (code, text) = game.run(&[
        "--workers",
        "1",
        "--checkpoint-every",
        "1",
        "--abort-after",
        "2",
    ]);
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("index INTERRUPTED"), "{text}");
    let idx = game.index();
    assert!(
        idx.songs.len() >= 2 && idx.songs.len() < 6,
        "{} songs",
        idx.songs.len()
    );
    let kept = idx.songs.len();

    let (code, text) = game.run(&[]);
    assert_eq!(code, 0, "{text}");
    assert!(text.contains(&format!("{} reused)", kept)), "{text}");
    assert_eq!(game.index().songs.len(), 6);
}

#[test]
fn disabled_mod_and_bad_folder() {
    let game = Game::new("gates");
    game.add_song("P", "S", "S", 150);
    fs::write(
        game.root.join("mod-config.json"),
        r#"{ "mods": { "stepmania-packs": false } }"#,
    )
    .unwrap();
    let (code, text) = game.run(&[]);
    assert_eq!(code, 0);
    assert!(
        text.contains("disabled in mod-config.json -- nothing to do"),
        "{text}"
    );
    assert!(!game.cache().join("index.bin").exists());

    let not_game = std::env::temp_dir();
    let out = Command::new(BIN)
        .arg("--game-dir")
        .arg(&not_game)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3));
}

#[test]
fn song_limit_off_is_called_out() {
    let game = Game::new("sle");
    game.add_song("P", "S", "S", 150);
    fs::write(
        game.root.join("mod-config.json"),
        r#"{ "mods": { "song-limit-expansion": false } }"#,
    )
    .unwrap();
    let (_, text) = game.run(&[]);
    assert!(text.contains("Song Limit Expansion is OFF"), "{text}");
    assert_eq!(game.index().songs.len(), 1);
}
