//! End-to-end checks of the stdout / exit-code contract on the real binary.
//!
//! Every invocation here must exit 0: the updater runs from `gamestart.bat`
//! before spice2x, and nothing it prints may stop the game from launching.

use std::process::Command;

fn run(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_ddr_world_hook_updater"))
        .args(args)
        .output()
        .expect("spawn updater binary");
    (
        out.status.code().expect("exit code"),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn e1_version() {
    let (code, stdout, _) = run(&["--version"]);
    assert_eq!(code, 0);
    assert_eq!(
        stdout,
        format!("ddr_world_hook_updater {}\n", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn e2_help() {
    let (code, stdout, _) = run(&["--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("Usage:"), "{stdout}");
    assert!(stdout.contains("--game-dir"), "{stdout}");
}

#[test]
fn e3_unknown_flag_prints_message_and_usage_exit_0() {
    let (code, stdout, _) = run(&["--bogus"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("--bogus"), "{stdout}");
    assert!(stdout.contains("Usage:"), "{stdout}");
}

#[test]
fn e4_bare_run_against_unknown_repo_is_skipped_exit_0() {
    // A run needs a game folder (Task 02's gate); point at a valid temp one and
    // at a repository that cannot exist, so the outcome is deterministic whether
    // the host is online (HTTP 404) or offline (transport error).
    let t = gate::Temp::new();
    std::fs::write(t.0.join("spice64.exe"), b"").unwrap();
    let (code, stdout, stderr) = run(&[
        "--game-dir",
        &t.arg(),
        "--repo",
        "no-such-owner-ddr/no-such-repo-ddr",
    ]);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(
        stdout.contains("Checking github.com/no-such-owner-ddr/no-such-repo-ddr"),
        "{stdout}"
    );
    assert!(
        stderr.contains("skipped (")
            && stderr.contains("starting the game with the installed version"),
        "{stderr}"
    );
    assert!(!t.0.join("ddr_world_hook_updater.manifest.json").exists());
    assert!(!t.0.join(".ddr_world_hook_updater/download").exists());
}

#[test]
fn e5_check_against_unknown_repo_is_skipped_exit_0() {
    let t = gate::Temp::new();
    std::fs::write(t.0.join("spice64.exe"), b"").unwrap();
    let (code, _, stderr) = run(&[
        "--game-dir",
        &t.arg(),
        "--check",
        "--repo",
        "no-such-owner-ddr/no-such-repo-ddr",
    ]);
    assert_eq!(code, 0, "{stderr}");
    assert!(stderr.contains("skipped ("), "{stderr}");
}

#[test]
fn e6_tag_without_from_zip_is_usage_error_exit_0() {
    let (code, stdout, _) = run(&["--tag", "v9"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("--from-zip"), "{stdout}");
    assert!(stdout.contains("Usage:"), "{stdout}");
}

// ---------------------------------------------------------------------------
// Game-folder gate + log file (Task 02)
// ---------------------------------------------------------------------------

mod gate {
    use super::run;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    pub struct Temp(pub PathBuf);

    impl Temp {
        pub fn new() -> Self {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let dir =
                std::env::temp_dir().join(format!("ddr_updater_e2e_{}_{n}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Temp(dir)
        }
        pub fn arg(&self) -> String {
            self.0.to_string_lossy().into_owned()
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn prefixed(line: &str) -> bool {
        line.starts_with("[+") && line.contains("s] ")
    }

    #[test]
    fn e7_refusal_outside_game_folder_creates_nothing() {
        let t = Temp::new();
        let (code, stdout, _) = run(&["--game-dir", &t.arg()]);
        assert_eq!(code, 0);
        assert!(stdout.contains("spice64.exe"), "{stdout}");
        assert!(stdout.contains("ddr_world_hook.dll"), "{stdout}");
        assert!(stdout.contains("nothing done"), "{stdout}");
        assert!(!t.0.join("ddr_world_hook_updater.log").exists());
        assert!(!t.0.join(".ddr_world_hook_updater").exists());
        assert_eq!(
            fs::read_dir(&t.0).unwrap().count(),
            0,
            "folder must stay empty"
        );
    }

    #[test]
    fn e8_valid_folder_logs_header_and_truncates_old_log() {
        let t = Temp::new();
        fs::write(t.0.join("spice64.exe"), b"").unwrap();
        let log = t.0.join("ddr_world_hook_updater.log");
        fs::write(&log, "OLD\n").unwrap();

        let (code, stdout, _) = run(&[
            "--game-dir",
            &t.arg(),
            "--repo",
            "no-such-owner-ddr/no-such-repo-ddr",
        ]);
        assert_eq!(code, 0);
        assert!(
            stdout.contains(&format!(
                "DDR World Hook updater {}",
                env!("CARGO_PKG_VERSION")
            )),
            "{stdout}"
        );
        assert!(stdout.contains("Game folder: "), "{stdout}");
        assert!(stdout.contains("Checking github.com/"), "{stdout}");

        let text = fs::read_to_string(&log).unwrap();
        assert!(!text.contains("OLD"), "{text}");
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines.len() >= 6, "{text}");
        assert!(
            lines[0].ends_with(&format!(
                "ddr_world_hook_updater {}",
                env!("CARGO_PKG_VERSION")
            )),
            "{text}"
        );
        assert!(lines[1].contains("unix time: "), "{text}");
        assert!(lines[2].contains("game folder: "), "{text}");
        assert!(lines[3].contains("arguments: "), "{text}");
        assert!(text.contains("Checking github.com/"), "{text}");
        for line in &lines {
            assert!(prefixed(line), "{line}");
        }
        // Every INFO console line is also in the file.
        for line in stdout.lines().filter(|l| l.contains("INFO ")) {
            assert!(text.contains(line), "missing from log: {line}");
        }
    }

    #[test]
    fn e9_run_lines_reach_console_and_log() {
        let t = Temp::new();
        fs::write(t.0.join("ddr_world_hook.dll"), b"").unwrap();
        let (code, stdout, stderr) = run(&[
            "--game-dir",
            &t.arg(),
            "--check",
            "--repo",
            "no-such-owner-ddr/no-such-repo-ddr",
        ]);
        assert_eq!(code, 0, "{stdout}{stderr}");
        assert!(stdout.contains("Checking github.com/"), "{stdout}");
        let text = fs::read_to_string(t.0.join("ddr_world_hook_updater.log")).unwrap();
        assert!(text.contains("Checking github.com/"), "{text}");
        assert!(text.contains("skipped ("), "{text}");
    }
}
