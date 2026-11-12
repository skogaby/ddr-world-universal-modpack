//! End-to-end: the measurement pass against a synthetic song whose audio
//! clicks sit a known offset away from its chart.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use ddr_world_hook_prep::services::stepmania::model::parse_index;
use ddr_world_hook_prep::services::stepmania::sync_state::{SyncState, SyncTable};

const BIN: &str = env!("CARGO_BIN_EXE_ddr_world_hook_prep");
const BPM: u32 = 120;
const MEASURES: u32 = 16;

struct Game {
    root: PathBuf,
}

impl Game {
    fn new(tag: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "ddr_prep_measure_{tag}_{}_{}",
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

    fn add_song(&self, folder: &str, click_offset_ms: f64) {
        let dir = self.root.join("stepmania/Songs/Pack").join(folder);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("song.ssc"), ssc(folder)).unwrap();
        fs::write(dir.join("song.wav"), wav(click_offset_ms)).unwrap();
    }

    fn run(&self, extra: &[&str]) -> (i32, String) {
        let out = Command::new(BIN)
            .arg("--game-dir")
            .arg(&self.root)
            .args(extra)
            .output()
            .expect("spawn prep");
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        (out.status.code().unwrap_or(-1), text)
    }

    fn table(&self) -> SyncTable {
        SyncTable::parse(&fs::read(self.root.join("data_mods/_cache/stepmania/sync.bin")).unwrap())
            .unwrap()
    }
}

impl Drop for Game {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// One tap on every beat (4 per measure) for `MEASURES` measures at `BPM`,
/// offset 0: beat k lands at k × 0.5 s.
fn ssc(title: &str) -> String {
    let measure = "1000\n0100\n0010\n0001\n";
    let notes: Vec<&str> = (0..MEASURES).map(|_| measure).collect();
    format!(
        "#VERSION:0.83;\n#TITLE:{title};\n#ARTIST:e2e;\n#MUSIC:song.wav;\n#OFFSET:0.000;\n\
#SAMPLESTART:1.0;\n#SAMPLELENGTH:4.0;\n#BPMS:0.000={BPM}.000;\n\
#NOTEDATA:;\n#STEPSTYPE:dance-single;\n#DIFFICULTY:Hard;\n#METER:7;\n#NOTES:\n{}\n;\n",
        notes.join(",\n")
    )
}

/// 16-bit stereo 44.1 kHz: a sharp click 2 ms long on every beat, shifted by
/// `click_offset_ms`, over the chart's length plus two seconds.
fn wav(click_offset_ms: f64) -> Vec<u8> {
    let rate = 44_100u32;
    let beat_s = 60.0 / f64::from(BPM);
    let secs = f64::from(MEASURES * 4) * beat_s + 2.0;
    let n = (f64::from(rate) * secs) as u32;
    let mut data = vec![0i16; n as usize * 2];
    let click_len = (f64::from(rate) * 0.002) as i64;
    for beat in 0..(MEASURES * 4) {
        let t = f64::from(beat) * beat_s + click_offset_ms / 1000.0;
        let start = (t * f64::from(rate)).round() as i64;
        for i in start.max(0)..(start + click_len).min(i64::from(n)) {
            let amp = if (i - start) % 2 == 0 {
                12_000
            } else {
                -12_000
            };
            data[i as usize * 2] = amp;
            data[i as usize * 2 + 1] = amp;
        }
    }
    let mut bytes = Vec::with_capacity(44 + data.len() * 2);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data.len() as u32 * 2).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&(rate * 4).to_le_bytes());
    bytes.extend_from_slice(&4u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&(data.len() as u32 * 2).to_le_bytes());
    for s in data {
        bytes.extend_from_slice(&s.to_le_bytes());
    }
    bytes
}

#[test]
fn measures_kept_songs_and_remembers_them() {
    let game = Game::new("plant");
    game.add_song("InSync", 0.0);
    game.add_song("Late", 20.0);

    let (code, text) = game.run(&[]);
    assert_eq!(code, 0, "{text}");
    assert!(
        text.contains("auto-sync: measuring 2 song(s) (0 already measured)"),
        "{text}"
    );
    assert!(
        text.contains("auto-sync complete -- 2 measured, 0 refused"),
        "{text}"
    );

    let idx =
        parse_index(&fs::read(game.root.join("data_mods/_cache/stepmania/index.bin")).unwrap())
            .unwrap();
    let table = game.table();
    assert_eq!(table.len(), 2);
    let by_folder = |f: &str| {
        let e = idx.songs.iter().find(|s| s.folder == f).unwrap();
        table
            .current(&e.basename, e.fingerprint)
            .unwrap()
            .state
            .clone()
    };
    let SyncState::Measured { delta_ms: d0 } = by_folder("InSync") else {
        panic!("InSync not measured");
    };
    let SyncState::Measured { delta_ms: d20 } = by_folder("Late") else {
        panic!("Late not measured");
    };
    // The two songs differ only by the planted 20 ms; the estimator's own
    // convention offset cancels in the difference.
    let diff = i32::from(d20) - i32::from(d0);
    assert!(
        (18..=22).contains(&diff),
        "delta difference {diff} (d0 {d0}, d20 {d20})"
    );

    // Second run: nothing to measure.
    let (_, text) = game.run(&[]);
    assert!(
        text.contains("every injected song is measured already (2 record(s))"),
        "{text}"
    );

    // --no-sync leaves sync.bin alone.
    let before = fs::read(game.root.join("data_mods/_cache/stepmania/sync.bin")).unwrap();
    let (_, text) = game.run(&["--no-sync"]);
    assert!(text.contains("auto-sync: skipped (--no-sync)"), "{text}");
    assert_eq!(
        fs::read(game.root.join("data_mods/_cache/stepmania/sync.bin")).unwrap(),
        before
    );
}

#[test]
fn auto_sync_off_in_config_skips_the_pass() {
    let game = Game::new("off");
    game.add_song("S", 0.0);
    fs::write(
        game.root.join("mod-config.json"),
        r#"{ "stepmania_packs": { "auto_sync": false } }"#,
    )
    .unwrap();
    let (code, text) = game.run(&[]);
    assert_eq!(code, 0);
    assert!(text.contains("auto-sync: off in mod-config.json"), "{text}");
    assert!(!game
        .root
        .join("data_mods/_cache/stepmania/sync.bin")
        .exists());
}
