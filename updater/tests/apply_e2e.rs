//! End-to-end install tests: a synthetic game folder plus a synthetic release
//! zip, driven through the real binary with `--from-zip`. This harness is the
//! one later plan steps extend (merges, GitHub feed, recovery).

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use zip::write::SimpleFileOptions;

static COUNTER: AtomicU32 = AtomicU32::new(0);

struct Temp(PathBuf);

impl Temp {
    fn new() -> Self {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("ddr_updater_apply_e2e_{}_{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Temp(dir)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Run the updater against `game` with extra args and optional env.
fn run(game: &Path, args: &[&str], env: &[(&str, &str)]) -> (i32, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ddr_world_hook_updater"));
    cmd.arg("--game-dir").arg(game).args(args);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("spawn updater");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.code().expect("exit code"), text)
}

fn write_file(root: &Path, rel: &str, bytes: &[u8]) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, bytes).unwrap();
}

fn make_zip(path: &Path, entries: &[(&str, &[u8])]) {
    let mut w = zip::ZipWriter::new(File::create(path).unwrap());
    for (name, data) in entries {
        w.start_file(*name, SimpleFileOptions::default()).unwrap();
        w.write_all(data).unwrap();
    }
    w.finish().unwrap();
}

/// Every regular file under `root` (relative, forward-slash) → contents,
/// minus the updater's own log/work dir and the fixture zips, so runs compare.
fn snapshot(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let rel = path
                    .strip_prefix(root)
                    .unwrap()
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/");
                // Skip the updater's own log/work dir/manifest (the manifest
                // carries a timestamp) and the test fixtures' zips, so runs
                // can be compared byte for byte.
                if rel == "ddr_world_hook_updater.log"
                    || rel == "ddr_world_hook_updater.manifest.json"
                    || rel.starts_with(".ddr_world_hook_updater/")
                    || rel.ends_with(".zip")
                {
                    continue;
                }
                out.insert(rel, fs::read(&path).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

/// A game folder with an old install: stub spice, old DLL, one shipped
/// texture, a user pack, a cache file, a generated texturelist, and both
/// user-owned files present.
fn synthetic_game() -> Temp {
    let t = Temp::new();
    let g = &t.0;
    write_file(g, "spice64.exe", b"spice");
    write_file(g, "ddr_world_hook.dll", b"OLD DLL");
    write_file(
        g,
        "data_mods/custom_options/select_music_option_v3_ifs/tex/shipped.png",
        b"old png",
    );
    write_file(
        g,
        "data_mods/custom_options/select_music_option_v3_ifs/tex/texturelist.merged.xml",
        b"<generated/>",
    );
    write_file(g, "data_mods/my_song_pack/song.ssq", b"user chart");
    write_file(g, "data_mods/_cache/step_data/v1.bin", b"cache");
    write_file(
        g,
        "mod-config.json",
        b"{\n  \"mods\": {\"user\": true}\n}\n",
    );
    write_file(
        g,
        "judgement_offsets.csv",
        b"code,p1_offset,p2_offset\nuser,5,5\n",
    );
    t
}

const V1: &[(&str, &[u8])] = &[
    ("ddr_world_hook.dll", b"NEW DLL v1"),
    (
        "ddr_world_hook_updater.exe",
        b"updater exe (skipped by the plan)",
    ),
    ("README.md", b"# release readme"),
    (
        "mod-config.json",
        b"{\n  \"mods\": {\"release\": true}\n}\n",
    ),
    (
        "judgement_offsets.csv",
        b"code,p1_offset,p2_offset\nrel,1,1\n",
    ),
    (
        "data_mods/custom_options/select_music_option_v3_ifs/tex/shipped.png",
        b"new png v1",
    ),
];

fn manifest_of(game: &Path) -> serde_json::Value {
    serde_json::from_str(
        &fs::read_to_string(game.join("ddr_world_hook_updater.manifest.json")).unwrap(),
    )
    .unwrap()
}

#[test]
fn a1_first_run_installs_release_owned_files_and_touches_nothing_else() {
    let t = synthetic_game();
    let g = &t.0;
    let zip = t.0.join("v1.zip");
    make_zip(&zip, V1);
    let before = snapshot(g);

    let (code, out) = run(g, &["--from-zip", zip.to_str().unwrap()], &[]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("Installed: 3 files written"), "{out}");
    assert!(out.contains("Updated to local:"), "{out}");

    let after = snapshot(g);
    // Release-owned: replaced / created.
    assert_eq!(after["ddr_world_hook.dll"], b"NEW DLL v1");
    assert_eq!(after["README.md"], b"# release readme");
    assert_eq!(
        after["data_mods/custom_options/select_music_option_v3_ifs/tex/shipped.png"],
        b"new png v1"
    );
    // The updater exe in the zip is installed (first install: plain move, no .old).
    assert_eq!(
        after["ddr_world_hook_updater.exe"],
        b"updater exe (skipped by the plan)"
    );
    assert!(!g.join("ddr_world_hook_updater.exe.old").exists());
    // User-owned merges: user values kept, release additions appended.
    let cfg: serde_json::Value = serde_json::from_slice(&after["mod-config.json"]).unwrap();
    assert_eq!(cfg["mods"]["user"], true);
    assert_eq!(cfg["mods"]["release"], true);
    assert_eq!(
        after["judgement_offsets.csv"],
        b"code,p1_offset,p2_offset\nuser,5,5\nrel,1,1\n"
    );
    // Runtime / user content: byte-identical.
    for keep in [
        "data_mods/my_song_pack/song.ssq",
        "data_mods/_cache/step_data/v1.bin",
        "data_mods/custom_options/select_music_option_v3_ifs/tex/texturelist.merged.xml",
        "spice64.exe",
    ] {
        assert_eq!(after[keep], before[keep], "{keep} must be untouched");
    }
    // Manifest.
    let m = manifest_of(g);
    assert_eq!(m["schema"], 1);
    assert!(m["tag"].as_str().unwrap().starts_with("local:"));
    assert_eq!(m["asset_name"], "v1.zip");
    let files = m["files"].as_object().unwrap();
    assert_eq!(files.len(), 4, "{files:?}");
    assert!(files.contains_key("ddr_world_hook.dll"));
    assert!(files.contains_key("README.md"));
    assert!(files.contains_key("ddr_world_hook_updater.exe"));
    assert!(!files.contains_key("mod-config.json"));
    // Work dir: backup holds originals, download/stage/journal gone.
    let work = g.join(".ddr_world_hook_updater");
    assert_eq!(
        fs::read(work.join("backup/ddr_world_hook.dll")).unwrap(),
        b"OLD DLL"
    );
    assert_eq!(
        fs::read(
            work.join("backup/data_mods/custom_options/select_music_option_v3_ifs/tex/shipped.png")
        )
        .unwrap(),
        b"old png"
    );
    assert!(!work.join("stage").exists());
    assert!(!work.join("download").exists());
    assert!(!work.join("journal.json").exists());
}

#[test]
fn a2_rerun_is_up_to_date_and_force_reinstalls() {
    let t = synthetic_game();
    let g = &t.0;
    let zip = t.0.join("v1.zip");
    make_zip(&zip, V1);
    let z = zip.to_str().unwrap();
    assert_eq!(run(g, &["--from-zip", z], &[]).0, 0);
    let after_first = snapshot(g);

    let (code, out) = run(g, &["--from-zip", z], &[]);
    assert_eq!(code, 0);
    assert!(out.contains("up to date (local:"), "{out}");
    assert!(!out.contains("Installing"), "{out}");
    assert_eq!(snapshot(g), after_first);

    let (code, out) = run(g, &["--from-zip", z, "--force"], &[]);
    assert_eq!(code, 0);
    assert!(out.contains("Installed: 3 files written"), "{out}");
    assert_eq!(snapshot(g), after_first);

    // --check reports the state without touching anything.
    let (code, out) = run(g, &["--from-zip", z, "--check"], &[]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("up to date"), "{out}");
    let zip2 = t.0.join("v2.zip");
    make_zip(&zip2, &[("ddr_world_hook.dll", b"NEW DLL v2")]);
    let (code, out) = run(g, &["--from-zip", zip2.to_str().unwrap(), "--check"], &[]);
    assert_eq!(code, 3, "{out}");
    assert!(out.contains("update available"), "{out}");
    assert_eq!(snapshot(g), after_first);
}

#[test]
fn a3_second_release_prunes_unchanged_and_keeps_modified_dropped_files() {
    let t = synthetic_game();
    let g = &t.0;
    let zip1 = t.0.join("v1.zip");
    make_zip(&zip1, V1);
    assert_eq!(run(g, &["--from-zip", zip1.to_str().unwrap()], &[]).0, 0);

    // Operator edits the shipped README locally.
    write_file(g, "README.md", b"# my notes");

    // v2 drops README.md and shipped.png, adds new.png.
    let zip2 = t.0.join("v2.zip");
    make_zip(
        &zip2,
        &[
            ("ddr_world_hook.dll", b"NEW DLL v2"),
            (
                "data_mods/custom_options/select_music_option_v3_ifs/tex/new.png",
                b"png v2",
            ),
        ],
    );
    let (code, out) = run(
        g,
        &["--from-zip", zip2.to_str().unwrap(), "--tag", "v2"],
        &[],
    );
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains(
            "Installed: 2 files written, 1 obsolete files removed, 1 locally modified file kept"
        ),
        "{out}"
    );
    assert!(
        out.contains("kept locally modified file (no longer shipped): README.md"),
        "{out}"
    );

    let after = snapshot(g);
    assert!(
        !after.contains_key("data_mods/custom_options/select_music_option_v3_ifs/tex/shipped.png")
    );
    assert_eq!(after["README.md"], b"# my notes");
    assert_eq!(
        after["data_mods/custom_options/select_music_option_v3_ifs/tex/new.png"],
        b"png v2"
    );
    assert_eq!(after["ddr_world_hook.dll"], b"NEW DLL v2");
    assert_eq!(after["data_mods/my_song_pack/song.ssq"], b"user chart");

    let m = manifest_of(g);
    assert_eq!(m["tag"], "v2");
    let files = m["files"].as_object().unwrap();
    assert_eq!(files.len(), 2);
    assert!(!files.contains_key("README.md"));
    // Pruned original is in backup/.
    assert_eq!(
        fs::read(g.join(".ddr_world_hook_updater/backup/data_mods/custom_options/select_music_option_v3_ifs/tex/shipped.png")).unwrap(),
        b"new png v1"
    );
}

#[test]
fn a5_injected_failure_rolls_back_to_byte_identical_folder() {
    let t = synthetic_game();
    let g = &t.0;
    let zip = t.0.join("v1.zip");
    make_zip(&zip, V1);
    let before = snapshot(g);

    let (code, out) = run(
        g,
        &["--from-zip", zip.to_str().unwrap()],
        &[("DDR_UPDATER_FAULT", "apply-after:2")],
    );
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("rolled back"), "{out}");
    assert_eq!(snapshot(g), before, "folder must be exactly as before");
    assert!(!g.join("ddr_world_hook_updater.manifest.json").exists());
    assert!(!g.join(".ddr_world_hook_updater/journal.json").exists());
    assert!(
        !g.join(".ddr_world_hook_updater/stage").exists(),
        "stage is cleaned after a rollback"
    );
}

#[test]
fn a6_rollback_failure_exits_1_and_names_the_backup_dir() {
    let t = synthetic_game();
    let g = &t.0;
    let zip = t.0.join("v1.zip");
    make_zip(&zip, V1);

    let (code, out) = run(
        g,
        &["--from-zip", zip.to_str().unwrap()],
        &[("DDR_UPDATER_FAULT", "rollback")],
    );
    assert_eq!(code, 1, "{out}");
    assert!(
        out.contains("could not be restored") || out.contains("rollback did not fully succeed"),
        "{out}"
    );
    assert!(out.contains(".ddr_world_hook_updater"), "{out}");
    assert!(out.contains("backup"), "{out}");
    // Journal left as a marker of the suspect state.
    assert!(g.join(".ddr_world_hook_updater/journal.json").exists());
}

#[test]
fn a7_merged_files_copied_only_when_absent() {
    let t = synthetic_game();
    let g = &t.0;
    fs::remove_file(g.join("mod-config.json")).unwrap();
    let zip = t.0.join("v1.zip");
    make_zip(&zip, V1);

    let (code, out) = run(g, &["--from-zip", zip.to_str().unwrap()], &[]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("mod-config.json: not present yet, installing the release copy"),
        "{out}"
    );
    assert!(out.contains("mod-config.json written"), "{out}");
    assert_eq!(
        fs::read(g.join("mod-config.json")).unwrap(),
        b"{\n  \"mods\": {\"release\": true}\n}\n"
    );
    // The existing CSV is merged: user row kept, release row appended.
    assert_eq!(
        fs::read(g.join("judgement_offsets.csv")).unwrap(),
        b"code,p1_offset,p2_offset\nuser,5,5\nrel,1,1\n"
    );
}

#[test]
fn a8_bad_archives_are_skipped_without_changes() {
    let t = synthetic_game();
    let g = &t.0;
    let before = snapshot(g);

    let notpack = t.0.join("np.zip");
    make_zip(&notpack, &[("README.md", b"x")]);
    let (code, out) = run(g, &["--from-zip", notpack.to_str().unwrap()], &[]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("skipped") && out.contains("not a modpack"),
        "{out}"
    );

    let (code, out) = run(
        g,
        &["--from-zip", t.0.join("missing.zip").to_str().unwrap()],
        &[],
    );
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("skipped"), "{out}");

    let corrupt = t.0.join("c.zip");
    fs::write(&corrupt, b"nope").unwrap();
    let (code, out) = run(g, &["--from-zip", corrupt.to_str().unwrap()], &[]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("skipped"), "{out}");

    assert_eq!(snapshot(g), before);
    assert!(!g.join(".ddr_world_hook_updater/stage").exists());
}

#[test]
fn a9_real_io_conflict_rolls_back() {
    let t = synthetic_game();
    let g = &t.0;
    // The release wants data_mods/blocker/x.png but data_mods/blocker is a FILE.
    write_file(g, "data_mods/blocker", b"i am a file");
    let zip = t.0.join("v1.zip");
    make_zip(
        &zip,
        &[
            ("ddr_world_hook.dll", b"NEW DLL"),
            ("data_mods/blocker/x.png", b"png"),
        ],
    );
    let before = snapshot(g);
    let (code, out) = run(g, &["--from-zip", zip.to_str().unwrap()], &[]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("rolled back"), "{out}");
    assert!(
        out.contains("cannot create directory for data_mods/blocker/x.png"),
        "{out}"
    );
    assert_eq!(snapshot(g), before);
}

// ---------------------------------------------------------------------------
// Merges (Step 3)
// ---------------------------------------------------------------------------

const USER_CONFIG: &str = r#"{
  "mods": {
    "premium-free": false,
    "my-custom-mod": true
  },
  "timing_offsets": {
    "sound_offset": 87
  },
  "custom_options": {
    "persist_json": true,
    "option_menu_settings": [
      { "id": "header_training_options", "overlay": true, "in_game": true },
      { "id": "autoplay", "overlay": true, "in_game": true },
      { "id": "song_speed", "overlay": false, "in_game": true },
      { "id": "header_power_user_options", "overlay": true, "in_game": true },
      { "id": "premium_free", "overlay": true, "in_game": true },
      { "id": "removed_by_release", "overlay": true, "in_game": true }
    ]
  }
}
"#;

const RELEASE_CONFIG: &str = r#"{
  "mods": {
    "premium-free": true,
    "brand-new-mod": true
  },
  "timing_offsets": {
    "sound_offset": 0,
    "input_offset": 28
  },
  "custom_options": {
    "persist_json": true,
    "option_menu_settings": [
      { "id": "header_power_user_options", "overlay": true, "in_game": true },
      { "id": "premium_free", "overlay": true, "in_game": true },
      { "id": "timing_stats", "overlay": true, "in_game": true },
      { "id": "header_training_options", "overlay": true, "in_game": true },
      { "id": "autoplay", "overlay": true, "in_game": true },
      { "id": "song_speed", "overlay": true, "in_game": true },
      { "id": "preserve_pitch", "overlay": true, "in_game": true },
      { "id": "header_new_section", "overlay": true, "in_game": true },
      { "id": "new_row", "overlay": true, "in_game": true }
    ]
  },
  "s_marvelous": { "window_ms": 12 }
}
"#;

fn ids_of(v: &serde_json::Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|e| e["id"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn m1_config_and_csv_merged_in_place() {
    let t = synthetic_game();
    let g = &t.0;
    write_file(g, "mod-config.json", USER_CONFIG.as_bytes());
    write_file(
        g,
        "judgement_offsets.csv",
        b"code,p1_offset,p2_offset\nuser,5,5\nhalf,,-3\nblank,,\n",
    );
    let zip = t.0.join("v1.zip");
    make_zip(
        &zip,
        &[
            ("ddr_world_hook.dll", b"NEW DLL"),
            ("mod-config.json", RELEASE_CONFIG.as_bytes()),
            (
                "judgement_offsets.csv",
                b"code,p1_offset,p2_offset\nuser,9,9\nhalf,4,4\nblank,7,\nnewsong,-2,-2\n",
            ),
        ],
    );
    let (code, out) = run(g, &["--from-zip", zip.to_str().unwrap()], &[]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains(
            "Merging mod-config.json ... 3 keys added (mods.brand-new-mod, timing_offsets.input_offset, s_marvelous), 4 menu rows placed (timing_stats, preserve_pitch, header_new_section, new_row)"
        ),
        "{out}"
    );
    assert!(
        out.contains("Merging judgement_offsets.csv ... 2 offsets filled, 1 song added"),
        "{out}"
    );
    assert!(
        out.contains("mod-config.json written, judgement_offsets.csv written"),
        "{out}"
    );

    let cfg: serde_json::Value =
        serde_json::from_slice(&fs::read(g.join("mod-config.json")).unwrap()).unwrap();
    // User values win; new keys added.
    assert_eq!(cfg["mods"]["premium-free"], false);
    assert_eq!(cfg["mods"]["my-custom-mod"], true);
    assert_eq!(cfg["mods"]["brand-new-mod"], true);
    assert_eq!(cfg["timing_offsets"]["sound_offset"], 87);
    assert_eq!(cfg["timing_offsets"]["input_offset"], 28);
    assert_eq!(cfg["s_marvelous"]["window_ms"], 12);
    // Key order: user's first, new keys appended.
    let top: Vec<&String> = cfg.as_object().unwrap().keys().collect();
    assert_eq!(
        top,
        ["mods", "timing_offsets", "custom_options", "s_marvelous"]
    );
    // Menu: user's section order kept (training first), new rows under their
    // release headers, removed id kept, user flag on song_speed untouched. The
    // NEW section follows the section that precedes it in the release
    // (training), wherever the user placed that section — design §4.7.
    let ids = ids_of(&cfg["custom_options"]["option_menu_settings"]);
    assert_eq!(
        ids,
        [
            "header_training_options",
            "autoplay",
            "song_speed",
            "preserve_pitch",
            "header_new_section",
            "new_row",
            "header_power_user_options",
            "premium_free",
            "timing_stats",
            "removed_by_release",
        ]
    );
    assert_eq!(
        cfg["custom_options"]["option_menu_settings"][2]["overlay"],
        false
    );

    let csv = fs::read_to_string(g.join("judgement_offsets.csv")).unwrap();
    assert_eq!(
        csv,
        "code,p1_offset,p2_offset\nuser,5,5\nhalf,4,-3\nblank,7,\nnewsong,-2,-2\n"
    );
    // Originals backed up.
    assert_eq!(
        fs::read(g.join(".ddr_world_hook_updater/backup/mod-config.json")).unwrap(),
        USER_CONFIG.as_bytes()
    );
}

#[test]
fn m2_identical_files_are_not_rewritten() {
    let t = synthetic_game();
    let g = &t.0;
    write_file(g, "mod-config.json", RELEASE_CONFIG.as_bytes());
    let csv = b"code,p1_offset,p2_offset\nuser,5,5\n";
    write_file(g, "judgement_offsets.csv", csv);
    let zip = t.0.join("v1.zip");
    make_zip(
        &zip,
        &[
            ("ddr_world_hook.dll", b"NEW DLL"),
            ("mod-config.json", RELEASE_CONFIG.as_bytes()),
            ("judgement_offsets.csv", csv),
        ],
    );
    let cfg_mtime = fs::metadata(g.join("mod-config.json"))
        .unwrap()
        .modified()
        .unwrap();
    let csv_mtime = fs::metadata(g.join("judgement_offsets.csv"))
        .unwrap()
        .modified()
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));

    let (code, out) = run(g, &["--from-zip", zip.to_str().unwrap()], &[]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("Merging mod-config.json ... no changes"),
        "{out}"
    );
    assert!(
        out.contains("Merging judgement_offsets.csv ... no changes"),
        "{out}"
    );
    assert!(!out.contains("mod-config.json written"), "{out}");
    assert_eq!(
        fs::metadata(g.join("mod-config.json"))
            .unwrap()
            .modified()
            .unwrap(),
        cfg_mtime
    );
    assert_eq!(
        fs::metadata(g.join("judgement_offsets.csv"))
            .unwrap()
            .modified()
            .unwrap(),
        csv_mtime
    );
    assert!(!g
        .join(".ddr_world_hook_updater/backup/mod-config.json")
        .exists());
}

#[test]
fn m3_unparseable_user_config_is_left_alone_and_a_copy_is_kept() {
    let t = synthetic_game();
    let g = &t.0;
    write_file(g, "mod-config.json", b"{ this is not json");
    let zip = t.0.join("v1.zip");
    make_zip(
        &zip,
        &[
            ("ddr_world_hook.dll", b"NEW DLL"),
            ("mod-config.json", RELEASE_CONFIG.as_bytes()),
        ],
    );
    let (code, out) = run(g, &["--from-zip", zip.to_str().unwrap()], &[]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("WARN") && out.contains("could not be parsed"),
        "{out}"
    );
    assert_eq!(
        fs::read(g.join("mod-config.json")).unwrap(),
        b"{ this is not json"
    );
    assert_eq!(
        fs::read(g.join(".ddr_world_hook_updater/unparseable/mod-config.json")).unwrap(),
        b"{ this is not json"
    );
    // The rest of the update still landed.
    assert_eq!(fs::read(g.join("ddr_world_hook.dll")).unwrap(), b"NEW DLL");
    assert!(g.join("ddr_world_hook_updater.manifest.json").exists());
}

#[test]
fn m4_user_csv_with_garbage_lines_still_merges_and_warns() {
    let t = synthetic_game();
    let g = &t.0;
    write_file(
        g,
        "judgement_offsets.csv",
        b"code,p1_offset,p2_offset\nok,1,1\nbad,xx,3\n",
    );
    let zip = t.0.join("v1.zip");
    make_zip(
        &zip,
        &[
            ("ddr_world_hook.dll", b"NEW DLL"),
            (
                "judgement_offsets.csv",
                b"code,p1_offset,p2_offset\nok,1,1\nnew,2,2\n",
            ),
        ],
    );
    let (code, out) = run(g, &["--from-zip", zip.to_str().unwrap()], &[]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("1 line(s) skipped"), "{out}");
    assert_eq!(
        fs::read_to_string(g.join("judgement_offsets.csv")).unwrap(),
        "code,p1_offset,p2_offset\nok,1,1\nnew,2,2\n"
    );
}

// ---------------------------------------------------------------------------
// Hardening (Step 5): crash recovery, self-update, emptied-dir cleanup
// ---------------------------------------------------------------------------

#[test]
fn h1_crash_mid_apply_is_recovered_on_the_next_run_which_then_completes() {
    let t = synthetic_game();
    let g = &t.0;
    let zip = t.0.join("v1.zip");
    make_zip(&zip, V1);
    let before = snapshot(g);

    // Simulated power loss after 2 actions: no rollback, journal left behind.
    let (code, out) = run(
        g,
        &["--from-zip", zip.to_str().unwrap()],
        &[("DDR_UPDATER_FAULT", "crash-after:2")],
    );
    assert_eq!(code, 70, "{out}");
    assert!(g.join(".ddr_world_hook_updater/journal.json").exists());
    assert_ne!(snapshot(g), before, "the crash left the folder dirty");
    assert!(!g.join("ddr_world_hook_updater.manifest.json").exists());

    // Next run: recovers first, then performs the update normally.
    let (code, out) = run(g, &["--from-zip", zip.to_str().unwrap()], &[]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("previous run was interrupted mid-update; restored"),
        "{out}"
    );
    assert!(out.contains("Installed:"), "{out}");
    assert!(!g.join(".ddr_world_hook_updater/journal.json").exists());
    assert_eq!(
        fs::read(g.join("ddr_world_hook.dll")).unwrap(),
        b"NEW DLL v1"
    );
    assert!(g.join("ddr_world_hook_updater.manifest.json").exists());
    // User content survived both runs.
    assert_eq!(
        fs::read(g.join("data_mods/my_song_pack/song.ssq")).unwrap(),
        b"user chart"
    );
}

#[test]
fn h2_recovery_alone_restores_the_pre_crash_folder() {
    let t = synthetic_game();
    let g = &t.0;
    let zip = t.0.join("v1.zip");
    make_zip(&zip, V1);
    let before = snapshot(g);
    let (code, _) = run(
        g,
        &["--from-zip", zip.to_str().unwrap()],
        &[("DDR_UPDATER_FAULT", "crash-after:3")],
    );
    assert_eq!(code, 70);
    // A run that cannot proceed past recovery (unknown repo) still recovers.
    let (code, out) = run(g, &["--repo", "no-such-owner-ddr/no-such-repo-ddr"], &[]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("previous run was interrupted"), "{out}");
    assert_eq!(
        snapshot(g),
        before,
        "recovery must restore the pre-crash folder exactly"
    );
    assert!(!g.join(".ddr_world_hook_updater/journal.json").exists());
    assert!(!g.join(".ddr_world_hook_updater/stage").exists());
}

#[test]
fn h3_self_update_swaps_the_exe_and_cleans_the_old_image_next_run() {
    let t = synthetic_game();
    let g = &t.0;
    write_file(g, "ddr_world_hook_updater.exe", b"OLD UPDATER");
    let zip = t.0.join("v1.zip");
    make_zip(&zip, V1); // carries "updater exe (skipped by the plan)" bytes — different from OLD UPDATER
    let (code, out) = run(g, &["--from-zip", zip.to_str().unwrap()], &[]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("updater replaced"), "{out}");
    assert!(out.contains("The updater itself was replaced"), "{out}");
    assert_eq!(
        fs::read(g.join("ddr_world_hook_updater.exe")).unwrap(),
        b"updater exe (skipped by the plan)"
    );
    assert_eq!(
        fs::read(g.join("ddr_world_hook_updater.exe.old")).unwrap(),
        b"OLD UPDATER"
    );
    let m = manifest_of(g);
    assert!(m["files"]
        .as_object()
        .unwrap()
        .contains_key("ddr_world_hook_updater.exe"));

    // Next run: stale .old removed; exe unchanged → no swap (up to date anyway).
    let (code, out) = run(g, &["--from-zip", zip.to_str().unwrap()], &[]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("removed the previous updater image"), "{out}");
    assert!(!g.join("ddr_world_hook_updater.exe.old").exists());

    // --force with an identical exe: no SelfUpdate action.
    let (code, out) = run(g, &["--from-zip", zip.to_str().unwrap(), "--force"], &[]);
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains("updater replaced"), "{out}");
    assert!(!g.join("ddr_world_hook_updater.exe.old").exists());
}

#[test]
fn h4_rollback_undoes_a_self_update() {
    let t = synthetic_game();
    let g = &t.0;
    write_file(g, "ddr_world_hook_updater.exe", b"OLD UPDATER");
    let zip = t.0.join("v1.zip");
    make_zip(&zip, V1);
    let before = snapshot(g);
    // SelfUpdate is the last action before WriteManifest: with 3 release files +
    // the swap, failing after 4 actions rolls the swap back too.
    let (code, out) = run(
        g,
        &["--from-zip", zip.to_str().unwrap()],
        &[("DDR_UPDATER_FAULT", "apply-after:4")],
    );
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("rolled back"), "{out}");
    assert_eq!(snapshot(g), before);
    assert!(!g.join("ddr_world_hook_updater.exe.old").exists());
}

#[test]
fn h5_prune_removes_emptied_dirs_but_keeps_dirs_with_user_files() {
    let t = synthetic_game();
    let g = &t.0;
    let zip1 = t.0.join("v1.zip");
    make_zip(
        &zip1,
        &[
            ("ddr_world_hook.dll", b"DLL1"),
            ("data_mods/retired_mod/tex/a.png", b"a"),
            ("data_mods/shared_mod/tex/b.png", b"b"),
        ],
    );
    assert_eq!(run(g, &["--from-zip", zip1.to_str().unwrap()], &[]).0, 0);
    write_file(g, "data_mods/shared_mod/tex/user_note.txt", b"mine");

    let zip2 = t.0.join("v2.zip");
    make_zip(&zip2, &[("ddr_world_hook.dll", b"DLL2")]);
    let (code, out) = run(g, &["--from-zip", zip2.to_str().unwrap()], &[]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("2 obsolete files removed"), "{out}");
    assert!(out.contains("removed 2 emptied folder(s)"), "{out}");
    assert!(
        !g.join("data_mods/retired_mod").exists(),
        "emptied tree removed"
    );
    assert!(
        g.join("data_mods/shared_mod/tex/user_note.txt").exists(),
        "user file keeps its dir"
    );
    assert!(g.join("data_mods").exists());
}
