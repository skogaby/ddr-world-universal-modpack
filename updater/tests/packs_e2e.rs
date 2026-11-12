//! End-to-end tests of optional content packs: a synthetic game folder, a
//! synthetic base release carrying a pack catalog, and synthetic pack zips,
//! driven through the real binary. The base always comes from `--from-zip`,
//! so no test touches GitHub; packs come from `--packs-dir` or from an
//! in-process HTTP listener.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;

static COUNTER: AtomicU32 = AtomicU32::new(0);

struct Temp(PathBuf);

impl Temp {
    fn new() -> Self {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("ddr_updater_packs_e2e_{}_{n}", std::process::id()));
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

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn write_file(root: &Path, rel: &str, bytes: &[u8]) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, bytes).unwrap();
}

fn make_zip(path: &Path, entries: &[(String, Vec<u8>)]) {
    let mut w = zip::ZipWriter::new(File::create(path).unwrap());
    for (name, data) in entries {
        w.start_file(name.as_str(), SimpleFileOptions::default())
            .unwrap();
        w.write_all(data).unwrap();
    }
    w.finish().unwrap();
}

/// Longest any updater run in this file may take. A run that exceeds it is
/// killed and the test fails — a mistake in a test (or a real hang) must
/// never stall the suite.
const RUN_LIMIT: std::time::Duration = std::time::Duration::from_secs(60);

/// Wait for `child` with its output captured, for at most [`RUN_LIMIT`].
fn wait_bounded(mut child: std::process::Child) -> (i32, String) {
    fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> JoinHandle<Vec<u8>> {
        std::thread::spawn(move || {
            let mut data = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut data);
            }
            data
        })
    }
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());
    let started = std::time::Instant::now();
    let status = loop {
        match child.try_wait().expect("poll updater") {
            Some(status) => break status,
            None if started.elapsed() > RUN_LIMIT => {
                let _ = child.kill();
                let _ = child.wait();
                let text = String::from_utf8_lossy(&stdout.join().unwrap()).into_owned();
                panic!("the updater did not exit within {RUN_LIMIT:?}; output so far:\n{text}");
            }
            None => std::thread::sleep(std::time::Duration::from_millis(20)),
        }
    };
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&stdout.join().unwrap()),
        String::from_utf8_lossy(&stderr.join().unwrap())
    );
    (status.code().expect("exit code"), text)
}

const MODELS: &str = "data_mods/custom_models";
const CATALOG: &str = "ddr_world_hook_packs.json";
const SELECTION: &str = "ddr_world_hook_updater.packs.json";
const ALLOW_HTTP: (&str, &str) = ("DDR_UPDATER_ALLOW_LOOPBACK_HTTP", "1");

/// A test world: the game folder, a folder of pack zips, and a folder for
/// base zips.
struct World {
    _t: Temp,
    game: PathBuf,
    packs: PathBuf,
    zips: PathBuf,
    bases: u32,
}

impl World {
    fn new() -> Self {
        let t = Temp::new();
        let (game, packs, zips) = (t.0.join("game"), t.0.join("packs"), t.0.join("zips"));
        for d in [&game, &packs, &zips] {
            fs::create_dir_all(d).unwrap();
        }
        fs::write(game.join("spice64.exe"), b"spice").unwrap();
        World {
            _t: t,
            game,
            packs,
            zips,
            bases: 0,
        }
    }

    /// Build the pack zip for `<kind>/<title>` at revision `rev` holding
    /// `files` (relative to the pack root) and return its catalog entry.
    fn pack(&self, kind: &str, title: &str, rev: &str, files: &[(&str, &[u8])]) -> Value {
        self.pack_at(kind, title, rev, files, "https://example.invalid/")
    }

    fn pack_at(
        &self,
        kind: &str,
        title: &str,
        rev: &str,
        files: &[(&str, &[u8])],
        url_base: &str,
    ) -> Value {
        let id = format!("{kind}-{}", title.to_lowercase().replace(' ', "-"));
        let root = format!("{MODELS}/{kind}/{title}");
        let content_id = sha256(format!("{id}:{rev}").as_bytes());
        let records: serde_json::Map<String, Value> = files
            .iter()
            .map(|(rel, data)| {
                (
                    format!("{root}/{rel}"),
                    json!({"size": data.len(), "sha256": sha256(data)}),
                )
            })
            .collect();
        let marker =
            json!({"schema": 1, "id": id, "content_id": content_id, "files": records}).to_string();
        let mut entries: Vec<(String, Vec<u8>)> = files
            .iter()
            .map(|(rel, data)| (format!("{root}/{rel}"), data.to_vec()))
            .collect();
        entries.push((format!("{MODELS}/.packs/{id}.json"), marker.into_bytes()));
        let asset = format!("ddr-world-pack-{id}-{}.zip", &content_id[..12]);
        let zip = self.packs.join(&asset);
        make_zip(&zip, &entries);
        let bytes = fs::read(&zip).unwrap();
        json!({
            "id": id,
            "group": if kind == "dancers" { "Dancers" } else { "Stages" },
            "title": title,
            "items": files.len(),
            "root": root,
            "content_id": content_id,
            "asset": asset,
            "url": format!("{url_base}{asset}"),
            "zip_size": bytes.len(),
            "zip_sha256": sha256(&bytes),
            "installed_size": files.iter().map(|(_, d)| d.len()).sum::<usize>().max(1),
        })
    }

    /// Write a base release zip carrying `catalog` and return its path. Each
    /// call yields a different DLL, hence a different release.
    fn base(&mut self, catalog: &[Value]) -> PathBuf {
        self.bases += 1;
        let path = self.zips.join(format!("base{}.zip", self.bases));
        make_zip(
            &path,
            &[
                (
                    "ddr_world_hook.dll".to_string(),
                    format!("DLL {}", self.bases).into_bytes(),
                ),
                (
                    CATALOG.to_string(),
                    json!({"schema": 1, "packs": catalog})
                        .to_string()
                        .into_bytes(),
                ),
            ],
        );
        path
    }

    /// Run the updater: install `base` (a no-op when it is already installed),
    /// then the pack step, taking pack zips from this world's folder.
    fn run(&self, base: &Path, args: &[&str]) -> (i32, String) {
        self.run_env(base, args, &[])
    }

    fn run_env(&self, base: &Path, args: &[&str], env: &[(&str, &str)]) -> (i32, String) {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ddr_world_hook_updater"));
        cmd.arg("--game-dir")
            .arg(&self.game)
            .arg("--from-zip")
            .arg(base)
            .arg("--packs-dir")
            .arg(&self.packs)
            .args(args);
        for (k, v) in env {
            cmd.env(k, v);
        }
        // Standard input is the null device: end of input at once.
        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        wait_bounded(cmd.spawn().expect("spawn updater"))
    }

    fn list(&self) -> String {
        let out = Command::new(env!("CARGO_BIN_EXE_ddr_world_hook_updater"))
            .arg("--game-dir")
            .arg(&self.game)
            .arg("--packs-list")
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(0));
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn file(&self, rel: &str) -> PathBuf {
        self.game.join(rel)
    }

    fn read(&self, rel: &str) -> Vec<u8> {
        fs::read(self.file(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
    }

    fn selection(&self) -> Value {
        serde_json::from_slice(&self.read(SELECTION)).unwrap()
    }

    /// Every file under `data_mods/custom_models`, relative to it.
    fn models(&self) -> BTreeMap<String, Vec<u8>> {
        fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
            let Ok(entries) = fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
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
                    out.insert(rel, fs::read(&path).unwrap());
                }
            }
        }
        let root = self.game.join(MODELS);
        let mut out = BTreeMap::new();
        walk(&root, &root, &mut out);
        out
    }

    fn model_names(&self) -> Vec<String> {
        self.models().into_keys().collect()
    }

    fn work(&self, rel: &str) -> PathBuf {
        self.game.join(".ddr_world_hook_updater").join(rel)
    }
}

const MIX_V1: [(&str, &[u8]); 3] = [
    ("Afro/pl_afro00.arc", b"afro arc v1"),
    ("Afro/chara_resources.rlist.txt", b"afro00,0\n"),
    ("Lady/pl_lady00.arc", b"lady arc v1"),
];
const HALL_V1: [(&str, &[u8]); 2] = [
    ("Stage 01/mapset_hall01.arc", b"hall arc v1"),
    ("Stage 01/map_resources.rlist.txt", b"hall01,ffffff\n"),
];

/// Two packs at revision 1 and a base that offers them.
fn world_v1() -> (World, PathBuf) {
    let mut w = World::new();
    let catalog = [
        w.pack("dancers", "Mix", "1", &MIX_V1),
        w.pack("stages", "Hall", "1", &HALL_V1),
    ];
    let base = w.base(&catalog);
    (w, base)
}

#[test]
fn p01_list_without_a_catalog_explains_and_creates_nothing() {
    let w = World::new();
    let text = w.list();
    assert!(
        text.contains("No pack catalog in this game folder"),
        "{text}"
    );
    assert!(!w.file("ddr_world_hook_updater.log").exists());
    assert!(!w.work("").exists());
}

#[test]
fn p02_fresh_folder_installs_the_base_and_no_pack() {
    let (w, base) = world_v1();
    let (code, out) = w.run(&base, &[]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(w.read("ddr_world_hook.dll"), b"DLL 1");
    assert!(w.file(CATALOG).is_file());
    assert!(w.models().is_empty(), "{:?}", w.model_names());
    assert!(
        !w.file(SELECTION).exists(),
        "nothing chosen, nothing recorded"
    );
    assert!(
        out.contains("Optional packs: none installed (2 available"),
        "{out}"
    );
    let list = w.list();
    assert!(
        list.contains("  [ ] dancers-mix") && list.contains("not installed"),
        "{list}"
    );
}

#[test]
fn p03_packs_install_installs_the_named_packs() {
    let (w, base) = world_v1();
    let (code, out) = w.run(&base, &["--packs-install", "dancers-mix,stages-hall,typo"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("unknown pack id \"typo\" ignored"), "{out}");
    assert!(out.contains("Installing dancers pack Mix"), "{out}");
    assert!(
        out.contains("Installed dancers pack Mix: 3 files written"),
        "{out}"
    );
    assert!(
        out.contains("Installed stages pack Hall: 2 files written"),
        "{out}"
    );
    assert!(out.contains("Optional packs: 2 of 2 installed"), "{out}");
    for (rel, data) in MIX_V1 {
        assert_eq!(w.read(&format!("{MODELS}/dancers/Mix/{rel}")), data);
    }
    for (rel, data) in HALL_V1 {
        assert_eq!(w.read(&format!("{MODELS}/stages/Hall/{rel}")), data);
    }
    assert!(w
        .file(&format!("{MODELS}/.packs/dancers-mix.json"))
        .is_file());
    let sel = w.selection();
    assert_eq!(sel["packs"], json!(["dancers-mix", "stages-hall"]));
    assert_eq!(sel["install_new"], false);
    assert_eq!(sel["known"], json!(["dancers-mix", "stages-hall"]));
    let list = w.list();
    assert!(
        list.contains("  [x] dancers-mix") && list.contains("  [x] stages-hall"),
        "{list}"
    );
    assert!(!list.contains("not installed"), "{list}");
    // Transactions are closed, and a fresh install leaves no pack backup.
    assert!(!w.work("journal.json").exists() && !w.work("stage").exists());
    assert!(!w.work("backup/dancers-mix").exists());
    // The user's own zips are never consumed.
    assert_eq!(fs::read_dir(&w.packs).unwrap().count(), 2);

    // A second run has nothing to do.
    let (code, out) = w.run(&base, &[]);
    assert_eq!(code, 0);
    assert!(
        !out.contains("Installing") && !out.contains("Repairing"),
        "{out}"
    );
}

#[test]
fn p04_a_new_release_updates_a_changed_pack_in_the_same_run() {
    let (mut w, base) = world_v1();
    w.run(&base, &["--packs-install", "all"]);
    let before_hall = w.read(&format!("{MODELS}/stages/Hall/Stage 01/mapset_hall01.arc"));

    // The user edits one shipped file, edits another that the next revision
    // drops, and adds a model of their own inside the source folder.
    write_file(
        &w.game,
        &format!("{MODELS}/dancers/Mix/Afro/pl_afro00.arc"),
        b"my afro edit!!",
    );
    write_file(
        &w.game,
        &format!("{MODELS}/dancers/Mix/Lady/pl_lady00.arc"),
        b"my lady",
    );
    write_file(
        &w.game,
        &format!("{MODELS}/dancers/Mix/Mine/pl_mine00.arc"),
        b"my own model",
    );

    // Revision 2: Afro changed, Lady dropped, Emi added. Hall unchanged.
    let mix_v2: [(&str, &[u8]); 3] = [
        ("Afro/pl_afro00.arc", b"afro arc v2"),
        ("Afro/chara_resources.rlist.txt", b"afro00,0\n"),
        ("Emi/pl_emi00.arc", b"emi arc"),
    ];
    let catalog = [
        w.pack("dancers", "Mix", "2", &mix_v2),
        w.pack("stages", "Hall", "1", &HALL_V1),
    ];
    let base2 = w.base(&catalog);
    let (code, out) = w.run(&base2, &["--tag", "v2"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("Updated to v2."), "{out}");
    assert!(out.contains("Updating dancers pack Mix"), "{out}");
    assert!(
        !out.contains("stages pack Hall"),
        "an unchanged pack is not touched: {out}"
    );
    assert!(
        out.contains("kept locally modified file (no longer part of the pack): data_mods/custom_models/dancers/Mix/Lady/pl_lady00.arc"),
        "{out}"
    );

    assert_eq!(w.read("ddr_world_hook.dll"), b"DLL 2");
    assert_eq!(
        w.read(&format!("{MODELS}/dancers/Mix/Afro/pl_afro00.arc")),
        b"afro arc v2"
    );
    assert_eq!(
        w.read(&format!("{MODELS}/dancers/Mix/Emi/pl_emi00.arc")),
        b"emi arc"
    );
    assert_eq!(
        w.read(&format!("{MODELS}/dancers/Mix/Lady/pl_lady00.arc")),
        b"my lady"
    );
    assert_eq!(
        w.read(&format!("{MODELS}/dancers/Mix/Mine/pl_mine00.arc")),
        b"my own model"
    );
    assert_eq!(
        w.read(&format!("{MODELS}/stages/Hall/Stage 01/mapset_hall01.arc")),
        before_hall
    );

    // The edited shipped file is in the pack's backup; the unmodified ones
    // it replaced are not. The base update's own backup survived the pack
    // transaction that followed it in the same run.
    assert_eq!(
        fs::read(w.work(&format!(
            "backup/dancers-mix/{MODELS}/dancers/Mix/Afro/pl_afro00.arc"
        )))
        .unwrap(),
        b"my afro edit!!"
    );
    assert!(!w
        .work(&format!(
            "backup/dancers-mix/{MODELS}/dancers/Mix/Afro/chara_resources.rlist.txt"
        ))
        .exists());
    assert!(!w
        .work(&format!("backup/dancers-mix/{MODELS}/.packs"))
        .exists());
    assert_eq!(
        fs::read(w.work("backup/base/ddr_world_hook.dll")).unwrap(),
        b"DLL 1"
    );
}

#[test]
fn p05_missing_and_resized_files_are_repaired_same_size_edits_only_with_force() {
    let (w, base) = world_v1();
    w.run(&base, &["--packs-install", "all"]);
    let afro = format!("{MODELS}/dancers/Mix/Afro/pl_afro00.arc");
    let lady = format!("{MODELS}/dancers/Mix/Lady/pl_lady00.arc");
    let hall = format!("{MODELS}/stages/Hall/Stage 01/mapset_hall01.arc");

    fs::remove_file(w.file(&afro)).unwrap();
    write_file(&w.game, &hall, b"cut");
    let (code, out) = w.run(&base, &[]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("Repairing dancers pack Mix")
            && out.contains("(1 file(s) missing, 0 changed)"),
        "{out}"
    );
    assert!(
        out.contains("Repairing stages pack Hall")
            && out.contains("(0 file(s) missing, 1 changed)"),
        "{out}"
    );
    assert_eq!(w.read(&afro), b"afro arc v1");
    assert_eq!(w.read(&hall), b"hall arc v1");
    // The damaged copy is kept until the next update, like any replaced file.
    assert_eq!(
        fs::read(w.work(&format!("backup/stages-hall/{hall}"))).unwrap(),
        b"cut"
    );

    // Same length, other bytes: invisible to the per-launch check...
    write_file(&w.game, &lady, b"LADY ARC V1");
    let (_, out) = w.run(&base, &[]);
    assert!(!out.contains("Repairing"), "{out}");
    assert_eq!(w.read(&lady), b"LADY ARC V1");
    // ...and found by --force.
    let (code, out) = w.run(&base, &["--force"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("Repairing dancers pack Mix"), "{out}");
    assert!(!out.contains("stages pack Hall"), "{out}");
    assert_eq!(w.read(&lady), b"lady arc v1");
}

#[test]
fn p06_deselecting_removes_only_recorded_unmodified_files() {
    let (w, base) = world_v1();
    w.run(&base, &["--packs-install", "all"]);
    // Net-new user content: inside a pack's source folder, in a source of
    // its own, and at the kind root.
    write_file(
        &w.game,
        &format!("{MODELS}/dancers/Mix/Mine/pl_mine00.arc"),
        b"mine 1",
    );
    write_file(
        &w.game,
        &format!("{MODELS}/dancers/My Source/Robot/pl_robot00/pl_robot00.model"),
        b"mine 2",
    );
    write_file(
        &w.game,
        &format!("{MODELS}/stages/mapset_room00.arc"),
        b"mine 3",
    );
    // And one recorded file edited in place.
    write_file(
        &w.game,
        &format!("{MODELS}/dancers/Mix/Lady/pl_lady00.arc"),
        b"edited lady",
    );

    let (code, out) = w.run(&base, &["--packs-install", "stages-hall"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("Removed pack dancers-mix (deselected): 2 files removed"),
        "{out}"
    );
    assert!(
        out.contains(
            "kept locally modified file: data_mods/custom_models/dancers/Mix/Lady/pl_lady00.arc"
        ),
        "{out}"
    );
    assert_eq!(
        w.model_names(),
        [
            ".packs/stages-hall.json",
            "dancers/Mix/Lady/pl_lady00.arc",
            "dancers/Mix/Mine/pl_mine00.arc",
            "dancers/My Source/Robot/pl_robot00/pl_robot00.model",
            "stages/Hall/Stage 01/map_resources.rlist.txt",
            "stages/Hall/Stage 01/mapset_hall01.arc",
            "stages/mapset_room00.arc",
        ]
    );
    assert!(
        !w.file(&format!("{MODELS}/dancers/Mix/Afro")).exists(),
        "emptied folders go"
    );
    assert_eq!(w.selection()["packs"], json!(["stages-hall"]));

    let (code, out) = w.run(&base, &["--packs-install", "none"]);
    assert_eq!(code, 0, "{out}");
    let models = w.models();
    assert_eq!(
        models.keys().map(String::as_str).collect::<Vec<_>>(),
        [
            "dancers/Mix/Lady/pl_lady00.arc",
            "dancers/Mix/Mine/pl_mine00.arc",
            "dancers/My Source/Robot/pl_robot00/pl_robot00.model",
            "stages/mapset_room00.arc",
        ]
    );
    assert_eq!(models["dancers/Mix/Mine/pl_mine00.arc"], b"mine 1");
    assert_eq!(
        models["dancers/My Source/Robot/pl_robot00/pl_robot00.model"],
        b"mine 2"
    );
    assert_eq!(models["stages/mapset_room00.arc"], b"mine 3");
    assert!(!w.file(&format!("{MODELS}/.packs")).exists());
    assert!(!w.file(&format!("{MODELS}/stages/Hall")).exists());
    assert_eq!(w.selection()["packs"], json!([]));
    assert!(out.contains("Optional packs: none installed"), "{out}");
}

/// Unpack a pack zip into the game folder, as a user would by hand.
fn unzip_into(zip: &Path, game: &Path) {
    let mut archive = zip::ZipArchive::new(File::open(zip).unwrap()).unwrap();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).unwrap();
        let dest = game.join(entry.name());
        fs::create_dir_all(dest.parent().unwrap()).unwrap();
        let mut data = Vec::new();
        entry.read_to_end(&mut data).unwrap();
        fs::write(dest, data).unwrap();
    }
}

#[test]
fn p07_a_pack_unzipped_by_hand_is_recognised_and_kept_current() {
    let (mut w, base) = world_v1();
    w.run(&base, &[]);
    let mix_zip = fs::read_dir(&w.packs)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| p.to_string_lossy().contains("dancers-mix"))
        .unwrap();
    unzip_into(&mix_zip, &w.game);

    let (code, out) = w.run(&base, &[]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("Optional packs: 1 of 2 installed"), "{out}");
    assert!(!out.contains("Installing"), "{out}");
    assert_eq!(
        w.selection()["packs"],
        json!(["dancers-mix"]),
        "adopted as the selection"
    );

    // From then on it is kept at the catalog's revision like any other.
    let mix_v2: [(&str, &[u8]); 1] = [("Afro/pl_afro00.arc", b"afro arc v2")];
    let catalog = [
        w.pack("dancers", "Mix", "2", &mix_v2),
        w.pack("stages", "Hall", "1", &HALL_V1),
    ];
    let base2 = w.base(&catalog);
    let (_, out) = w.run(&base2, &[]);
    assert!(out.contains("Updating dancers pack Mix"), "{out}");
    assert_eq!(
        w.model_names(),
        [".packs/dancers-mix.json", "dancers/Mix/Afro/pl_afro00.arc"]
    );
}

#[test]
fn p08_an_unselected_installed_pack_is_left_alone() {
    let (mut w, base) = world_v1();
    w.run(&base, &["--packs-install", "stages-hall"]);
    // Unpacked by hand AFTER a selection exists: installed, not selected.
    let mix_zip = fs::read_dir(&w.packs)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| p.to_string_lossy().contains("dancers-mix"))
        .unwrap();
    unzip_into(&mix_zip, &w.game);
    fs::remove_file(w.file(&format!("{MODELS}/dancers/Mix/Lady/pl_lady00.arc"))).unwrap();
    let before = w.models();

    let catalog = [
        w.pack("dancers", "Mix", "2", &[("Afro/pl_afro00.arc", b"v2")]),
        w.pack("stages", "Hall", "1", &HALL_V1),
    ];
    let base2 = w.base(&catalog);
    let (code, out) = w.run(&base2, &[]);
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains("dancers pack Mix"), "{out}");
    assert_eq!(w.models(), before);
    assert_eq!(w.selection()["packs"], json!(["stages-hall"]));
    let list = w.list();
    assert!(
        list.contains("  [ ] dancers-mix") && list.contains("installed, another revision"),
        "{list}"
    );
}

#[test]
fn p09_future_packs_are_installed_for_all_and_dropped_packs_are_removed() {
    let (mut w, base) = world_v1();
    w.run(&base, &["--packs-install", "all"]);
    assert_eq!(w.selection()["install_new"], true);
    write_file(
        &w.game,
        &format!("{MODELS}/stages/Hall/Mine/mapset_mine00.arc"),
        b"mine",
    );

    // The next release adds a pack and no longer offers Hall.
    let catalog = [
        w.pack("dancers", "Mix", "1", &MIX_V1),
        w.pack("dancers", "Party", "1", &[("Rena/pl_rena00.arc", b"rena")]),
    ];
    let base2 = w.base(&catalog);
    let (code, out) = w.run(&base2, &[]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("Installing dancers pack Party"), "{out}");
    assert!(
        out.contains(
            "Removed pack stages-hall (no longer offered by this release): 2 files removed"
        ),
        "{out}"
    );
    assert_eq!(
        w.read(&format!("{MODELS}/dancers/Party/Rena/pl_rena00.arc")),
        b"rena"
    );
    assert!(!w.file(&format!("{MODELS}/stages/Hall/Stage 01")).exists());
    assert_eq!(
        w.read(&format!("{MODELS}/stages/Hall/Mine/mapset_mine00.arc")),
        b"mine"
    );
    let sel = w.selection();
    assert_eq!(sel["packs"], json!(["dancers-mix", "dancers-party"]));
    assert_eq!(
        sel["known"],
        json!(["dancers-mix", "stages-hall", "dancers-party"])
    );
}

#[test]
fn p10_a_new_pack_is_only_announced_to_a_user_with_an_explicit_list() {
    let (mut w, base) = world_v1();
    w.run(&base, &["--packs-install", "dancers-mix"]);
    let catalog = [
        w.pack("dancers", "Mix", "1", &MIX_V1),
        w.pack("stages", "Hall", "1", &HALL_V1),
        w.pack("dancers", "Party", "1", &[("Rena/pl_rena00.arc", b"rena")]),
    ];
    let base2 = w.base(&catalog);
    let (_, out) = w.run(&base2, &[]);
    assert!(
        !out.contains("pack Party") && !out.contains("pack Hall"),
        "{out}"
    );
    assert!(
        out.contains("Optional packs: 1 of 3 installed; new in this release: Party dancers"),
        "{out}"
    );
    assert!(!w.file(&format!("{MODELS}/dancers/Party")).exists());
}

#[test]
fn p11_check_reports_pending_pack_work_and_changes_nothing() {
    let (w, base) = world_v1();
    w.run(&base, &["--packs-install", "all"]);
    let (code, out) = w.run(&base, &["--check"]);
    assert_eq!(code, 0, "nothing pending: {out}");

    fs::remove_file(w.file(&format!("{MODELS}/dancers/Mix/Afro/pl_afro00.arc"))).unwrap();
    let before = w.models();
    let (code, out) = w.run(&base, &["--check"]);
    assert_eq!(code, 3, "{out}");
    assert!(
        out.contains("pack dancers-mix needs installing (1 file(s) missing, 0 changed)"),
        "{out}"
    );
    assert_eq!(w.models(), before);
    // --check with --packs-install does not change the selection either.
    let (code, _) = w.run(&base, &["--check", "--packs-install", "none"]);
    assert_eq!(code, 3);
    assert_eq!(
        w.selection()["packs"],
        json!(["dancers-mix", "stages-hall"])
    );
}

#[test]
fn p12_a_tampered_or_foreign_pack_zip_is_rejected_and_the_game_still_starts() {
    let (w, base) = world_v1();
    // Corrupt the Mix zip in the user's folder: the digest no longer matches.
    let mix_zip = fs::read_dir(&w.packs)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| p.to_string_lossy().contains("dancers-mix"))
        .unwrap();
    let mut bytes = fs::read(&mix_zip).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xFF;
    fs::write(&mix_zip, &bytes).unwrap();

    let (code, out) = w.run(&base, &["--packs-install", "all"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("dancers pack Mix was not installed")
            && out.contains("does not match the catalog's digest"),
        "{out}"
    );
    assert!(
        out.contains("Installed stages pack Hall"),
        "one bad pack does not stop the next: {out}"
    );
    assert!(
        out.contains("Optional packs: 1 of 2 installed; 1 waiting to download"),
        "{out}"
    );
    assert_eq!(w.read("ddr_world_hook.dll"), b"DLL 1");
    assert!(!w.file(&format!("{MODELS}/dancers/Mix")).exists());
}

#[test]
fn p13_a_pack_zip_that_reaches_outside_its_root_is_rejected() {
    let mut w = World::new();
    let mut entry = w.pack("dancers", "Mix", "1", &MIX_V1);
    // The catalog says the pack lives elsewhere than its zip writes to.
    entry["root"] = json!(format!("{MODELS}/dancers/Other"));
    let base = w.base(&[entry]);
    let (code, out) = w.run(&base, &["--packs-install", "all"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("not the expected pack archive") && out.contains("is outside"),
        "{out}"
    );
    assert!(w.models().is_empty());
    assert!(!w.work("stage").exists());
}

#[test]
fn p14_a_failed_pack_transaction_rolls_back_alone() {
    let (w, base) = world_v1();
    w.run(&base, &[]);
    // A foreign file at a path the pack ships: it must come back on rollback.
    write_file(
        &w.game,
        &format!("{MODELS}/dancers/Mix/Afro/pl_afro00.arc"),
        b"foreign",
    );
    let before = w.models();

    let (code, out) = w.run_env(
        &base,
        &["--packs-install", "dancers-mix"],
        &[("DDR_UPDATER_FAULT", "apply-after:2")],
    );
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("dancers pack Mix was not installed") && out.contains("rolled back"),
        "{out}"
    );
    assert_eq!(w.models(), before, "the folder is as it was");
    assert_eq!(w.read("ddr_world_hook.dll"), b"DLL 1");
    assert!(!w.work("journal.json").exists());
    assert_eq!(
        w.selection()["packs"],
        json!(["dancers-mix"]),
        "still wanted"
    );

    // The next ordinary run installs it; the foreign file is replaced and kept
    // in the backup.
    let (code, out) = w.run(&base, &[]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(
        w.read(&format!("{MODELS}/dancers/Mix/Afro/pl_afro00.arc")),
        b"afro arc v1"
    );
    assert_eq!(
        fs::read(w.work(&format!(
            "backup/dancers-mix/{MODELS}/dancers/Mix/Afro/pl_afro00.arc"
        )))
        .unwrap(),
        b"foreign"
    );
}

#[test]
fn p15_a_failed_pack_rollback_exits_1() {
    let (w, base) = world_v1();
    w.run(&base, &[]);
    write_file(
        &w.game,
        &format!("{MODELS}/dancers/Mix/Afro/pl_afro00.arc"),
        b"foreign",
    );
    let (code, out) = w.run_env(
        &base,
        &["--packs-install", "dancers-mix"],
        &[("DDR_UPDATER_FAULT", "rollback")],
    );
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("the rollback did not fully succeed"), "{out}");
    assert!(out.contains("backup"), "{out}");
    assert!(
        w.work("journal.json").exists(),
        "the journal marks the folder as suspect"
    );
}

#[test]
fn p16_a_crash_mid_pack_is_recovered_and_the_install_repeated() {
    let (w, base) = world_v1();
    w.run(&base, &[]);
    write_file(
        &w.game,
        &format!("{MODELS}/dancers/Mix/Afro/pl_afro00.arc"),
        b"foreign",
    );
    let before = w.models();
    let (code, _) = w.run_env(
        &base,
        &["--packs-install", "dancers-mix"],
        &[("DDR_UPDATER_FAULT", "crash-after:2")],
    );
    assert_eq!(code, 70, "the injected crash");
    assert!(w.work("journal.json").exists());
    assert_ne!(w.models(), before, "half applied");

    let (code, out) = w.run(&base, &[]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("the previous run was interrupted mid-update"),
        "{out}"
    );
    assert!(out.contains("Installed dancers pack Mix"), "{out}");
    for (rel, data) in MIX_V1 {
        assert_eq!(w.read(&format!("{MODELS}/dancers/Mix/{rel}")), data);
    }
    assert!(!w.work("journal.json").exists());
}

#[test]
fn p17_a_same_named_model_elsewhere_is_reported_not_changed() {
    let (w, base) = world_v1();
    write_file(
        &w.game,
        &format!("{MODELS}/dancers/Custom/My Afro/pl_afro00.arc"),
        b"my afro",
    );
    let (code, out) = w.run(&base, &["--packs-install", "dancers-mix"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains(
            "data_mods/custom_models/dancers/Custom/My Afro/pl_afro00.arc has the same model name"
        ),
        "{out}"
    );
    assert_eq!(
        w.read(&format!("{MODELS}/dancers/Custom/My Afro/pl_afro00.arc")),
        b"my afro"
    );
}

#[test]
fn p18_an_unreadable_selection_file_is_left_alone() {
    let (w, base) = world_v1();
    w.run(&base, &["--packs-install", "dancers-mix"]);
    fs::write(w.file(SELECTION), b"{ not json").unwrap();
    let (code, out) = w.run(&base, &[]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("could not be read"), "{out}");
    assert_eq!(w.read(SELECTION), b"{ not json");
    assert!(out.contains("Optional packs: 1 of 2 installed"), "{out}");
}

#[test]
fn p19_the_committed_fixture_from_the_build_tool_installs() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/packs/out");
    let mut w = World::new();
    w.packs = fixture.clone();
    let catalog: Value =
        serde_json::from_slice(&fs::read(fixture.join("catalog.json")).unwrap()).unwrap();
    let base = w.base(catalog["packs"].as_array().unwrap());
    let (code, out) = w.run(&base, &["--packs-install", "all"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("Optional packs: 2 of 2 installed"), "{out}");
    assert_eq!(
        w.model_names(),
        [
            ".packs/dancers-fixture-mix.json",
            ".packs/stages-fixture-hall.json",
            "dancers/Fixture Mix/Alpha/chara_resources.rlist.txt",
            "dancers/Fixture Mix/Alpha/pl_fixalpha00.arc",
            "dancers/Fixture Mix/Beta/chara_resources.rlist.txt",
            "dancers/Fixture Mix/Beta/pl_fixbeta00.arc",
            "stages/Fixture Hall/Stage 01/map_resources.rlist.txt",
            "stages/Fixture Hall/Stage 01/mapset_fixhall01.arc",
        ]
    );
}

// ---- the prompt and the chooser --------------------------------------------

/// What the updater's standard input is.
enum Stdin<'a> {
    /// These bytes, then end of input.
    Script(&'a str),
    /// Open and silent until the updater exits (a console nobody types on).
    Silent,
}

impl World {
    fn run_stdin(
        &self,
        base: &Path,
        args: &[&str],
        stdin: Stdin<'_>,
    ) -> (i32, String, std::time::Duration) {
        use std::process::Stdio;
        let started = std::time::Instant::now();
        let mut child = Command::new(env!("CARGO_BIN_EXE_ddr_world_hook_updater"))
            .arg("--game-dir")
            .arg(&self.game)
            .arg("--from-zip")
            .arg(base)
            .arg("--packs-dir")
            .arg(&self.packs)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn updater");
        let mut pipe = child.stdin.take().unwrap();
        let held = match stdin {
            Stdin::Script(text) => {
                pipe.write_all(text.as_bytes()).unwrap();
                // Close it NOW: the updater must see end of input after the
                // script. (Left to scope, the pipe would stay open until
                // this function returns — after the wait below.)
                drop(pipe);
                None
            }
            Stdin::Silent => Some(pipe),
        };
        let (code, text) = wait_bounded(child);
        drop(held);
        (code, text, started.elapsed())
    }
}

const PROMPT: &str = "Press Enter to choose packs ... starting the game in 5 seconds";

#[test]
fn c01_every_normal_run_prompts_and_closed_input_does_not_wait() {
    let (w, base) = world_v1();
    let started = std::time::Instant::now();
    let (code, out) = w.run(&base, &[]); // stdin is the null device
    assert_eq!(code, 0, "{out}");
    assert!(out.contains(PROMPT), "{out}");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(3),
        "no countdown without a console"
    );
    assert!(!out.contains("OPTIONAL PACKS"), "{out}");
    // Again on the next run, with everything settled: the prompt is unconditional.
    w.run(&base, &["--packs-install", "all"]);
    let (_, out) = w.run(&base, &[]);
    assert!(
        out.contains("Optional packs: 2 of 2 installed") && out.contains(PROMPT),
        "{out}"
    );
}

#[test]
fn c02_the_prompt_is_skipped_only_where_it_cannot_apply() {
    let (mut w, base) = world_v1();
    w.run(&base, &[]);
    for args in [&["--check"][..], &["--packs-install", "none"][..]] {
        let (_, out) = w.run(&base, args);
        assert!(
            !out.contains("Press Enter to choose packs"),
            "{args:?}: {out}"
        );
    }
    // A base without a catalog (older than packs): nothing to choose from.
    let old = w.zips.join("old.zip");
    make_zip(&old, &[("ddr_world_hook.dll".to_string(), b"OLD".to_vec())]);
    let fresh = World::new();
    w.game = fresh.game.clone();
    let (code, out) = w.run(&old, &[]);
    assert_eq!(code, 0, "{out}");
    assert!(
        !out.contains("Press Enter") && !out.contains("Optional packs"),
        "{out}"
    );
}

#[test]
fn c03_enter_at_the_prompt_opens_the_chooser_and_the_choice_is_installed() {
    let (w, base) = world_v1();
    // Enter (the prompt), tick row 1, Enter (apply), y (confirm).
    let (code, out, _) = w.run_stdin(&base, &[], Stdin::Script("\n1\n\ny\n"));
    assert_eq!(code, 0, "{out}");
    assert!(out.contains(PROMPT), "{out}");
    assert!(out.contains("Opening the pack chooser"), "{out}");
    assert!(
        out.contains(" OPTIONAL PACKS            0 of 2 selected"),
        "{out}"
    );
    assert!(
        out.contains("   1 [x] Mix "),
        "the list is redrawn with the tick: {out}"
    );
    assert!(out.contains(" Download 1 pack(s)"), "{out}");
    assert!(out.contains("Pack selection saved: 1 pack(s)"), "{out}");
    assert!(
        out.contains("Installed dancers pack Mix: 3 files written"),
        "{out}"
    );
    assert!(out.contains("Optional packs: 1 of 2 installed"), "{out}");
    let sel = w.selection();
    assert_eq!(sel["packs"], json!(["dancers-mix"]));
    assert_eq!(sel["known"], json!(["dancers-mix", "stages-hall"]));
    assert!(w
        .file(&format!("{MODELS}/dancers/Mix/Afro/pl_afro00.arc"))
        .is_file());
    assert!(!w.file(&format!("{MODELS}/stages/Hall")).exists());
}

#[test]
fn c04_packs_flag_opens_the_chooser_directly_and_unticking_removes() {
    let (w, base) = world_v1();
    w.run(&base, &["--packs-install", "all"]);
    write_file(
        &w.game,
        &format!("{MODELS}/dancers/Mix/Mine/pl_mine00.arc"),
        b"mine",
    );
    let (code, out, _) = w.run_stdin(&base, &["--packs"], Stdin::Script("1 f\n\n\n"));
    assert_eq!(code, 0, "{out}");
    assert!(
        !out.contains("Press Enter to choose packs"),
        "no countdown with --packs: {out}"
    );
    assert!(
        out.contains("   1 [x] Mix ") && out.contains("installed"),
        "{out}"
    );
    assert!(out.contains(" Remove   1 pack(s): Mix"), "{out}");
    assert!(out.contains(" Future packs: ask first"), "{out}");
    assert!(
        out.contains("Removed pack dancers-mix (deselected): 3 files removed"),
        "{out}"
    );
    assert_eq!(
        w.model_names(),
        [
            ".packs/stages-hall.json",
            "dancers/Mix/Mine/pl_mine00.arc",
            "stages/Hall/Stage 01/map_resources.rlist.txt",
            "stages/Hall/Stage 01/mapset_hall01.arc",
        ]
    );
    let sel = w.selection();
    assert_eq!(sel["packs"], json!(["stages-hall"]));
    assert_eq!(sel["install_new"], false);
}

#[test]
fn c05_declining_quitting_and_end_of_input_change_nothing() {
    let (w, base) = world_v1();
    w.run(&base, &[]);
    for script in ["1\n\nn\nq\n", "q\n", "1 2\n", ""] {
        let (code, out, _) = w.run_stdin(&base, &["--packs"], Stdin::Script(script));
        assert_eq!(code, 0, "{script:?}: {out}");
        assert!(
            out.contains("Pack chooser closed without changes"),
            "{script:?}: {out}"
        );
        assert!(w.models().is_empty(), "{script:?}");
        assert!(!w.file(SELECTION).exists(), "{script:?}");
    }
}

#[test]
fn c06_a_silent_console_waits_five_seconds_then_lets_the_game_start() {
    let (w, base) = world_v1();
    let (code, out, took) = w.run_stdin(&base, &[], Stdin::Silent);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains(PROMPT), "{out}");
    assert!(!out.contains("OPTIONAL PACKS"), "{out}");
    assert!(
        took >= std::time::Duration::from_secs(5) && took < std::time::Duration::from_secs(15),
        "{took:?}"
    );
    assert!(w.models().is_empty());
}

#[test]
fn c07_seeing_the_list_once_stops_packs_being_announced_as_new() {
    let (mut w, base) = world_v1();
    w.run(&base, &["--packs-install", "dancers-mix"]);
    let catalog = [
        w.pack("dancers", "Mix", "1", &MIX_V1),
        w.pack("stages", "Hall", "1", &HALL_V1),
        w.pack("dancers", "Party", "1", &[("Rena/pl_rena00.arc", b"rena")]),
    ];
    let base2 = w.base(&catalog);
    let (_, out, _) = w.run_stdin(&base2, &["--packs"], Stdin::Script("q\n"));
    assert!(out.contains("new in this release: Party dancers"), "{out}");
    assert!(
        out.contains("   3 [ ] Party") && out.contains("NEW"),
        "{out}"
    );
    assert_eq!(
        w.selection()["known"],
        json!(["dancers-mix", "stages-hall", "dancers-party"])
    );
    assert_eq!(
        w.selection()["packs"],
        json!(["dancers-mix"]),
        "the choice itself is untouched"
    );
    let (_, out) = w.run(&base2, &[]);
    assert!(out.contains("Optional packs: 1 of 3 installed"), "{out}");
    assert!(!out.contains("new in this release"), "{out}");
}

// ---- downloads --------------------------------------------------------------

/// A one-thread HTTP server for pack zips: serves `files` by path, honours
/// `Range`, and can close a connection early (`cut`) for one named file.
struct Server {
    addr: SocketAddr,
    seen: Arc<Mutex<Vec<(String, Option<String>)>>>,
    cut: Arc<Mutex<Option<(String, usize)>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Server {
    fn start(dir: &Path) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let cut = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let (dir, seen_t, cut_t, stop_t) = (
            dir.to_path_buf(),
            Arc::clone(&seen),
            Arc::clone(&cut),
            Arc::clone(&stop),
        );
        let thread = std::thread::spawn(move || {
            for stream in listener.incoming() {
                if stop_t.load(Ordering::SeqCst) {
                    break;
                }
                if let Ok(stream) = stream {
                    serve(stream, &dir, &seen_t, &cut_t);
                }
            }
        });
        Server {
            addr,
            seen,
            cut,
            stop,
            thread: Some(thread),
        }
    }

    fn url_base(&self) -> String {
        format!("http://{}/", self.addr)
    }

    fn requests_for(&self, needle: &str) -> Vec<Option<String>> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter(|(path, _)| path.contains(needle))
            .map(|(_, range)| range.clone())
            .collect()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.addr);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn serve(
    mut stream: TcpStream,
    dir: &Path,
    seen: &Mutex<Vec<(String, Option<String>)>>,
    cut: &Mutex<Option<(String, usize)>>,
) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).unwrap_or(0) == 0 {
        return;
    }
    let path = request_line
        .split_whitespace()
        .nth(1)
        .unwrap_or("")
        .to_string();
    let mut range = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("range") {
                range = Some(value.trim().to_string());
            }
        }
    }
    seen.lock().unwrap().push((path.clone(), range.clone()));
    let name = path.trim_start_matches('/');
    let Ok(body) = fs::read(dir.join(name)) else {
        let _ = stream
            .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        return;
    };
    let from = range
        .as_deref()
        .and_then(|r| r.strip_prefix("bytes="))
        .and_then(|r| r.strip_suffix('-'))
        .and_then(|n| n.parse::<usize>().ok())
        .filter(|n| *n < body.len());
    let (status, extra, payload) = match from {
        Some(n) => (
            "206 Partial Content",
            format!(
                "Content-Range: bytes {n}-{}/{}\r\n",
                body.len() - 1,
                body.len()
            ),
            &body[n..],
        ),
        None => ("200 OK", String::new(), &body[..]),
    };
    // Decide under ONE lock acquisition. (A `match` on `cut.lock()…` keeps
    // the guard alive through its arms, so locking again inside one
    // deadlocks this thread.)
    let limit = {
        let mut pending = cut.lock().unwrap();
        match pending.as_ref() {
            Some((which, at)) if name.contains(which.as_str()) => {
                let at = (*at).min(payload.len());
                *pending = None;
                at
            }
            _ => payload.len(),
        }
    };
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n",
        payload.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&payload[..limit]);
    let _ = stream.flush();
}

/// A world whose packs are served over HTTP rather than found in
/// `--packs-dir` (which points at an empty folder).
struct NetWorld {
    w: World,
    server: Server,
    served: PathBuf,
    base: PathBuf,
}

/// 300 kB that deflate cannot shrink (xorshift noise), so the pack's zip is
/// big enough for an interrupted transfer to have something to resume.
fn big(seed: u8) -> Vec<u8> {
    let mut x = 0x9E37_79B9u32 ^ u32::from(seed);
    (0..300_000)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            (x >> 24) as u8
        })
        .collect()
}

fn net_world() -> NetWorld {
    let mut w = World::new();
    let served = w.packs.clone();
    let server = Server::start(&served);
    let (mix, hall) = (big(1), big(2));
    let catalog = [
        w.pack_at(
            "dancers",
            "Mix",
            "1",
            &[("Afro/pl_afro00.arc", &mix)],
            &server.url_base(),
        ),
        w.pack_at(
            "stages",
            "Hall",
            "1",
            &[("Stage 01/mapset_hall01.arc", &hall)],
            &server.url_base(),
        ),
    ];
    let base = w.base(&catalog);
    // `--packs-dir` now names a folder without the zips, so they are fetched.
    w.packs = w.zips.join("empty");
    fs::create_dir_all(&w.packs).unwrap();
    NetWorld {
        w,
        server,
        served,
        base,
    }
}

#[test]
fn n01_packs_are_downloaded_verified_and_the_zip_is_not_kept() {
    let n = net_world();
    let (code, out) =
        n.w.run_env(&n.base, &["--packs-install", "all"], &[ALLOW_HTTP]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("Installed dancers pack Mix") && out.contains("Installed stages pack Hall"),
        "{out}"
    );
    assert!(
        !out.contains('%'),
        "progress is a live console line, never a log line: {out}"
    );
    assert_eq!(
        n.w.read(&format!("{MODELS}/dancers/Mix/Afro/pl_afro00.arc")),
        big(1)
    );
    assert_eq!(
        n.w.read(&format!("{MODELS}/stages/Hall/Stage 01/mapset_hall01.arc")),
        big(2)
    );
    assert!(
        !n.w.work("packs").exists(),
        "downloads are deleted once installed"
    );
    assert_eq!(n.server.requests_for("dancers-mix"), [None]);
}

#[test]
fn n02_loopback_http_is_refused_without_the_test_switch() {
    let n = net_world();
    let (code, out) = n.w.run(&n.base, &["--packs-install", "all"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("url must be https"), "{out}");
    assert!(n.w.models().is_empty());
    assert!(n.server.requests_for("").is_empty());
}

#[test]
fn n03_an_interrupted_download_resumes_on_the_next_run() {
    let n = net_world();
    *n.server.cut.lock().unwrap() = Some(("dancers-mix".to_string(), 100_000));
    let (code, out) =
        n.w.run_env(&n.base, &["--packs-install", "dancers-mix"], &[ALLOW_HTTP]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("dancers pack Mix was not installed"), "{out}");
    assert!(out.contains("it will be retried on the next run"), "{out}");
    let part = fs::read_dir(n.w.work("packs"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| p.to_string_lossy().ends_with(".zip.part"))
        .expect("the partial download is kept");
    let kept = fs::metadata(&part).unwrap().len();
    assert!(kept > 0 && kept <= 100_000, "{kept}");
    assert!(n.w.models().is_empty());

    let (code, out) = n.w.run_env(&n.base, &[], &[ALLOW_HTTP]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("resumed an interrupted download"), "{out}");
    assert!(out.contains("Installed dancers pack Mix"), "{out}");
    assert_eq!(
        n.w.read(&format!("{MODELS}/dancers/Mix/Afro/pl_afro00.arc")),
        big(1)
    );
    assert_eq!(
        n.server.requests_for("dancers-mix"),
        [None, Some(format!("bytes={kept}-"))]
    );
    assert!(!n.w.work("packs").exists());
}

#[test]
fn n04_a_missing_asset_fails_one_pack_and_the_next_still_installs() {
    let n = net_world();
    let mix_zip = fs::read_dir(&n.served)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| p.to_string_lossy().contains("dancers-mix"))
        .unwrap();
    fs::remove_file(mix_zip).unwrap();
    let (code, out) =
        n.w.run_env(&n.base, &["--packs-install", "all"], &[ALLOW_HTTP]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("dancers pack Mix was not installed") && out.contains("404"),
        "{out}"
    );
    assert!(out.contains("Installed stages pack Hall"), "{out}");
    assert!(
        out.contains("Optional packs: 1 of 2 installed; 1 waiting to download"),
        "{out}"
    );
}

#[test]
fn n05_a_download_that_is_not_the_catalogs_bytes_is_discarded() {
    let n = net_world();
    let mix_zip = fs::read_dir(&n.served)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| p.to_string_lossy().contains("dancers-mix"))
        .unwrap();
    let mut bytes = fs::read(&mix_zip).unwrap();
    bytes[10] ^= 0xFF;
    fs::write(&mix_zip, &bytes).unwrap();
    let (code, out) =
        n.w.run_env(&n.base, &["--packs-install", "dancers-mix"], &[ALLOW_HTTP]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("does not match the published"), "{out}");
    assert!(!n.w.work("packs").exists(), "nothing kept to resume from");
    assert!(n.w.models().is_empty());
}

#[test]
fn n06_an_unreachable_server_stops_further_downloads_for_the_run() {
    let mut w = World::new();
    // Nothing listens on port 9 of the loopback interface.
    let catalog = [
        w.pack_at("dancers", "Mix", "1", &MIX_V1, "http://127.0.0.1:9/"),
        w.pack_at("stages", "Hall", "1", &HALL_V1, "http://127.0.0.1:9/"),
        w.pack("dancers", "Local", "1", &[("A/pl_a00.arc", b"local")]),
    ];
    let base = w.base(&catalog);
    // Only the third pack's zip is available locally.
    for entry in fs::read_dir(&w.packs).unwrap().flatten() {
        if !entry
            .file_name()
            .to_string_lossy()
            .contains("dancers-local")
        {
            fs::remove_file(entry.path()).unwrap();
        }
    }
    let (code, out) = w.run_env(&base, &["--packs-install", "all"], &[ALLOW_HTTP]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(
        out.matches("was not installed").count(),
        1,
        "one attempt, not one per pack: {out}"
    );
    assert!(
        out.contains("1 pack(s) were not downloaded because GitHub could not be reached"),
        "{out}"
    );
    assert!(
        out.contains("Installed dancers pack Local"),
        "a local zip needs no network: {out}"
    );
    assert!(
        out.contains("Optional packs: 1 of 3 installed; 2 waiting to download"),
        "{out}"
    );
}

#[test]
fn n07_downloads_of_packs_no_longer_wanted_are_cleaned_up() {
    let n = net_world();
    fs::create_dir_all(n.w.work("packs")).unwrap();
    fs::write(
        n.w.work("packs/ddr-world-pack-old-000000000000.zip.part"),
        b"stale",
    )
    .unwrap();
    let (code, out) =
        n.w.run_env(&n.base, &["--packs-install", "stages-hall"], &[ALLOW_HTTP]);
    assert_eq!(code, 0, "{out}");
    assert!(!n.w.work("packs").exists());
}
