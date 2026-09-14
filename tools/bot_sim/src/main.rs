//! `bot_sim` — offline simulator for the Multiplayer Bot's skill model.
//!
//! Mounts the DLL's REAL pure modules (`skill`, `planner`, `eligibility`, the
//! foot-panel `layout`, the SSQ chunk/tempo parsers) via `#[path]`, so the code
//! exercised here is byte-for-byte what ships, and drives them through a model
//! of `GamePlayActor::judgeNotes`, the freeze judge, `judge_submit` and the
//! NORMAL gauge (`docs/gauge_and_judge_scoring_research.md`) over every chart
//! in an SSQ directory × every SINGLE difficulty × every bot level × N seeds.
//! Output: one self-contained HTML report with per-level overviews and
//! synthesized scorecards, plus optional JSON.
//!
//! `cargo test` here also runs the mounted DLL modules' own `#[cfg(test)]`
//! suites — this crate IS `scripts/validate_multiplayer_bot.sh`'s harness.

// ── Mounted DLL modules (repository-relative `#[path]`s live in the
// `core/ssq/mod.rs` and `bot/mod.rs` files so the `..` chains resolve from
// real directories) ──────────────────────────────────────────────────────
pub mod bot;
pub mod core;
#[path = "../../../src/services/foot_panel_swap/layout.rs"]
pub mod layout;

// ── The simulator's own modules ─────────────────────────────────────────
pub mod chart;
pub mod gauge;
pub mod judge_model;
pub mod report;
pub mod scoring;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use chart::{Chart, Difficulty};
use judge_model::{simulate, SimConfig};
use scoring::Scorecard;

const USAGE: &str = "\
bot_sim — offline Multiplayer Bot simulator

USAGE:
  bot_sim <ssq-dir> [options]

OPTIONS:
  --out <file.html>      Report path (default: bot_sim_report.html)
  --json <file.json>     Also write every scorecard as JSON
  --levels <a-b|list>    Bot levels, e.g. 1-10 or 1,5,10 (default 1-10)
  --diffs <list>         Difficulties: b,B,D,E,C = beginner,basic,difficult,expert,challenge (default all)
  --seeds <n>            Seeds per (chart, difficulty, level) (default 3)
  --filter <substr>      Only charts whose basename contains <substr>
  --fps <n>              Judge frame rate (default 60)
  --smarv-ms <n>         S-Marvelous window for the S-MARV column (default 12)
  --threads <n>          Worker threads (default: available parallelism)
  --sigma-l1 <ms>        Override skill::SIGMA_L1_MS (tuning what-if)
  --sigma-l10 <ms>       Override skill::SIGMA_L10_MS
  --pmiss-l1 <p>         Override skill::P_MISS_L1
  --pmiss-exp <e>        Override skill::P_MISS_EXP (1.0 = linear fall-off)
  --summary              Print the per-level table to stdout (tuning loops)
  --no-html              Skip the HTML report (with --summary / --json)
  -h, --help
";

#[derive(Debug, Clone)]
pub struct Args {
    pub ssq_dir: PathBuf,
    pub out: PathBuf,
    pub json: Option<PathBuf>,
    pub levels: Vec<u8>,
    pub diffs: Vec<Difficulty>,
    pub seeds: u32,
    pub filter: Option<String>,
    pub fps: u32,
    pub smarv_ms: i32,
    pub threads: usize,
    pub curve_override: skill_override::Override,
    pub summary: bool,
    pub html: bool,
}

/// Optional what-if overrides of the skill constants. The DLL's `skill.rs`
/// exposes the constants and `curve()`; overriding here re-derives the same
/// shape with different anchors without touching the mounted source.
pub mod skill_override {
    use crate::bot::skill::{Curve, P_MISS_EXP, P_MISS_L1, SIGMA_L10_MS, SIGMA_L1_MS};

    #[derive(Debug, Clone, Copy, Default)]
    pub struct Override {
        pub sigma_l1: Option<f64>,
        pub sigma_l10: Option<f64>,
        pub pmiss_l1: Option<f64>,
        pub pmiss_exp: Option<f64>,
    }

    impl Override {
        pub fn is_identity(&self) -> bool {
            self.sigma_l1.is_none()
                && self.sigma_l10.is_none()
                && self.pmiss_l1.is_none()
                && self.pmiss_exp.is_none()
        }
        pub fn curve(&self, level: u8) -> Curve {
            if self.is_identity() {
                return crate::bot::skill::curve(level);
            }
            let s1 = self.sigma_l1.unwrap_or(SIGMA_L1_MS);
            let s10 = self.sigma_l10.unwrap_or(SIGMA_L10_MS);
            let p1 = self.pmiss_l1.unwrap_or(P_MISS_L1);
            let pe = self.pmiss_exp.unwrap_or(P_MISS_EXP);
            let l = level.clamp(1, 10) as f64;
            let t = (l - 1.0) / 9.0;
            Curve {
                sigma_ms: s1 * (s10 / s1).powf(t),
                p_miss: p1 * ((10.0 - l) / 9.0).powf(pe),
            }
        }
        pub fn describe(&self) -> String {
            format!(
                "sigma_l1={} sigma_l10={} pmiss_l1={} pmiss_exp={}",
                self.sigma_l1.unwrap_or(SIGMA_L1_MS),
                self.sigma_l10.unwrap_or(SIGMA_L10_MS),
                self.pmiss_l1.unwrap_or(P_MISS_L1),
                self.pmiss_exp.unwrap_or(P_MISS_EXP)
            )
        }
    }
}

fn parse_levels(s: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    for part in s.split(',') {
        let part = part.trim();
        if let Some((a, b)) = part.split_once('-') {
            let a: u8 = a
                .trim()
                .parse()
                .map_err(|_| format!("bad level range {part}"))?;
            let b: u8 = b
                .trim()
                .parse()
                .map_err(|_| format!("bad level range {part}"))?;
            if a < 1 || b > 10 || a > b {
                return Err(format!("level range {part} outside 1..=10"));
            }
            out.extend(a..=b);
        } else {
            let v: u8 = part.parse().map_err(|_| format!("bad level {part}"))?;
            if !(1..=10).contains(&v) {
                return Err(format!("level {v} outside 1..=10"));
            }
            out.push(v);
        }
    }
    out.sort_unstable();
    out.dedup();
    if out.is_empty() {
        return Err("no levels".into());
    }
    Ok(out)
}

fn parse_diffs(s: &str) -> Result<Vec<Difficulty>, String> {
    let mut out = Vec::new();
    for part in s.split(',') {
        let d = match part.trim() {
            "b" | "beginner" => Difficulty::Beginner,
            "B" | "basic" => Difficulty::Basic,
            "D" | "difficult" => Difficulty::Difficult,
            "E" | "expert" => Difficulty::Expert,
            "C" | "challenge" => Difficulty::Challenge,
            other => return Err(format!("unknown difficulty {other:?}")),
        };
        if !out.contains(&d) {
            out.push(d);
        }
    }
    if out.is_empty() {
        return Err("no difficulties".into());
    }
    Ok(out)
}

fn parse_args() -> Result<Args, String> {
    let mut it = std::env::args().skip(1);
    let mut ssq_dir: Option<PathBuf> = None;
    let mut out = PathBuf::from("bot_sim_report.html");
    let mut json = None;
    let mut levels = (1..=10).collect::<Vec<u8>>();
    let mut diffs = Difficulty::ALL.to_vec();
    let mut seeds = 3u32;
    let mut filter = None;
    let mut fps = 60u32;
    let mut smarv_ms = 12i32;
    let mut threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    let mut ov = skill_override::Override::default();
    let mut summary = false;
    let mut html = true;

    fn need(it: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
        it.next().ok_or_else(|| format!("{flag} needs a value"))
    }
    while let Some(a) = it.next() {
        match a.as_str() {
            "-h" | "--help" => {
                print!("{USAGE}");
                std::process::exit(0);
            }
            "--out" => out = PathBuf::from(need(&mut it, "--out")?),
            "--json" => json = Some(PathBuf::from(need(&mut it, "--json")?)),
            "--levels" => levels = parse_levels(&need(&mut it, "--levels")?)?,
            "--diffs" => diffs = parse_diffs(&need(&mut it, "--diffs")?)?,
            "--seeds" => {
                seeds = need(&mut it, "--seeds")?
                    .parse()
                    .map_err(|_| "bad --seeds".to_string())?;
                if seeds == 0 {
                    return Err("--seeds must be >= 1".into());
                }
            }
            "--filter" => filter = Some(need(&mut it, "--filter")?),
            "--fps" => {
                fps = need(&mut it, "--fps")?
                    .parse()
                    .map_err(|_| "bad --fps".to_string())?;
                if !(10..=1000).contains(&fps) {
                    return Err("--fps outside 10..=1000".into());
                }
            }
            "--smarv-ms" => {
                smarv_ms = need(&mut it, "--smarv-ms")?
                    .parse()
                    .map_err(|_| "bad --smarv-ms".to_string())?;
                if !(1..=16).contains(&smarv_ms) {
                    return Err("--smarv-ms outside 1..=16".into());
                }
            }
            "--threads" => {
                threads = need(&mut it, "--threads")?
                    .parse()
                    .map_err(|_| "bad --threads".to_string())?;
                threads = threads.max(1);
            }
            "--sigma-l1" => {
                ov.sigma_l1 = Some(
                    need(&mut it, "--sigma-l1")?
                        .parse()
                        .map_err(|_| "bad --sigma-l1".to_string())?,
                )
            }
            "--sigma-l10" => {
                ov.sigma_l10 = Some(
                    need(&mut it, "--sigma-l10")?
                        .parse()
                        .map_err(|_| "bad --sigma-l10".to_string())?,
                )
            }
            "--pmiss-l1" => {
                ov.pmiss_l1 = Some(
                    need(&mut it, "--pmiss-l1")?
                        .parse()
                        .map_err(|_| "bad --pmiss-l1".to_string())?,
                )
            }
            "--pmiss-exp" => {
                ov.pmiss_exp = Some(
                    need(&mut it, "--pmiss-exp")?
                        .parse()
                        .map_err(|_| "bad --pmiss-exp".to_string())?,
                )
            }
            "--summary" => summary = true,
            "--no-html" => html = false,
            other if other.starts_with('-') => return Err(format!("unknown option {other}")),
            other => {
                if ssq_dir.is_some() {
                    return Err(format!("unexpected positional argument {other}"));
                }
                ssq_dir = Some(PathBuf::from(other));
            }
        }
    }
    let ssq_dir = ssq_dir.ok_or_else(|| "missing <ssq-dir>".to_string())?;
    Ok(Args {
        ssq_dir,
        out,
        json,
        levels,
        diffs,
        seeds,
        filter,
        fps,
        smarv_ms,
        threads,
        curve_override: ov,
        summary,
        html,
    })
}

/// Display form of a path with the home directory collapsed to `~` (the
/// report is a generated artifact that may be shared — never embed a
/// machine-specific absolute path in it).
fn tilde_path(p: &Path) -> String {
    let shown = p.display().to_string();
    if let Some(home) = std::env::var_os("HOME") {
        let home = home.to_string_lossy();
        if !home.is_empty() {
            if let Some(rest) = shown.strip_prefix(home.as_ref()) {
                return format!("~{rest}");
            }
        }
    }
    shown
}

/// Every `*.ssq` in `dir` (non-recursive), sorted by file name.
fn list_ssq(dir: &Path, filter: Option<&str>) -> std::io::Result<Vec<PathBuf>> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("ssq"))
        })
        .filter(|p| match filter {
            Some(f) => p
                .file_stem()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.contains(f)),
            None => true,
        })
        .collect();
    v.sort();
    Ok(v)
}

/// One unit of work: a chart × level × seed.
struct Job {
    chart_idx: usize,
    level: u8,
    seed_idx: u32,
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: {e}\n\n{USAGE}");
            std::process::exit(2);
        }
    };

    let started = Instant::now();
    let files = match list_ssq(&args.ssq_dir, args.filter.as_deref()) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("error: cannot list {}: {e}", args.ssq_dir.display());
            std::process::exit(1);
        }
    };
    if files.is_empty() {
        eprintln!("error: no .ssq files in {}", args.ssq_dir.display());
        std::process::exit(1);
    }

    // Parse every file once; keep the SINGLE charts of the requested
    // difficulties. Parse failures are reported and skipped (fail-open).
    let mut charts: Vec<Chart> = Vec::new();
    let mut parse_errors: Vec<(String, String)> = Vec::new();
    for f in &files {
        let name = f
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string();
        match std::fs::read(f) {
            Ok(blob) => match chart::parse_single_charts(&name, &blob) {
                Ok(list) => {
                    for c in list {
                        if args.diffs.contains(&c.difficulty) && !c.notes.is_empty() {
                            charts.push(c);
                        }
                    }
                }
                Err(e) => parse_errors.push((name, e)),
            },
            Err(e) => parse_errors.push((name, e.to_string())),
        }
    }
    eprintln!(
        "[*] {} files, {} single charts selected, {} parse errors ({:.1}s)",
        files.len(),
        charts.len(),
        parse_errors.len(),
        started.elapsed().as_secs_f64()
    );
    if charts.is_empty() {
        eprintln!("error: nothing to simulate");
        std::process::exit(1);
    }

    let cfg = SimConfig {
        fps: args.fps,
        smarv_ms: args.smarv_ms,
    };
    let jobs: Vec<Job> = (0..charts.len())
        .flat_map(|ci| {
            args.levels.iter().flat_map(move |&lvl| {
                (0..args.seeds).map(move |si| Job {
                    chart_idx: ci,
                    level: lvl,
                    seed_idx: si,
                })
            })
        })
        .collect();
    eprintln!(
        "[*] {} simulations on {} threads",
        jobs.len(),
        args.threads.min(jobs.len().max(1))
    );

    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Scorecard>> = Mutex::new(Vec::with_capacity(jobs.len()));
    let done = AtomicUsize::new(0);
    let ov = args.curve_override;
    std::thread::scope(|s| {
        for _ in 0..args.threads.min(jobs.len().max(1)) {
            s.spawn(|| {
                let mut local: Vec<Scorecard> = Vec::new();
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(job) = jobs.get(i) else { break };
                    let chart = &charts[job.chart_idx];
                    let curve = ov.curve(job.level);
                    // Deterministic per (chart, difficulty, level, seed index).
                    let seed = bot::skill::seed(
                        0x5EED_0000 + job.seed_idx as u64,
                        chart.mcode_hash(),
                        chart.difficulty as i32,
                        job.level,
                    );
                    let card = simulate(chart, job.level, &curve, seed, job.seed_idx, &cfg);
                    local.push(card);
                    let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                    if n % 5000 == 0 {
                        eprintln!("    {n}/{} …", jobs.len());
                    }
                }
                results.lock().unwrap().extend(local);
            });
        }
    });
    let mut results = results.into_inner().unwrap();
    results.sort_by(|a, b| {
        (a.chart.as_str(), a.difficulty as u8, a.level, a.seed_idx).cmp(&(
            b.chart.as_str(),
            b.difficulty as u8,
            b.level,
            b.seed_idx,
        ))
    });
    eprintln!(
        "[*] simulated {} songs in {:.1}s",
        results.len(),
        started.elapsed().as_secs_f64()
    );

    let meta = report::Meta {
        ssq_dir: tilde_path(&args.ssq_dir),
        chart_count: charts.len(),
        file_count: files.len(),
        parse_errors,
        seeds: args.seeds,
        fps: args.fps,
        smarv_ms: args.smarv_ms,
        levels: args.levels.clone(),
        curves: args.levels.iter().map(|&l| (l, ov.curve(l))).collect(),
        curve_override: !ov.is_identity(),
        curve_description: ov.describe(),
        elapsed_secs: started.elapsed().as_secs_f64(),
    };

    if args.summary {
        print!("{}", report::summary_table(&meta, &results));
    }
    if let Some(j) = &args.json {
        if let Err(e) = std::fs::write(j, report::json_dump(&meta, &results)) {
            eprintln!("error: writing {}: {e}", j.display());
            std::process::exit(1);
        }
        eprintln!("[*] wrote {}", j.display());
    }
    if args.html {
        match std::fs::write(&args.out, report::render_html(&meta, &results)) {
            Ok(()) => eprintln!("[*] wrote {}", args.out.display()),
            Err(e) => {
                eprintln!("error: writing {}: {e}", args.out.display());
                std::process::exit(1);
            }
        }
    }
}
