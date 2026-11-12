//! `--dry-run` — StepMania → DDR World conversion into a scratch directory
//! with a per-song verdict (the former `tools/sm_pack_check`).
//!
//! Runs a song folder — or every song folder of a pack — through the same
//! pipeline the runtime uses: parse → SSQ (+ self-check) → probe audio →
//! lead-in → song bank (XWB) + sound bank (XSB). Artefacts land in `--out`
//! under the song's basename (`<code>.ssq` / `<code>_N.ssq`, `<code>.xsb`,
//! `<code>.xwb`) so they can be vetted offline or dropped into a mod folder
//! for a cabinet check; the verdict names every chart kept or rejected and
//! why, and for a song with per-chart timing lists which `<code>_N.ssq` the
//! runtime's `ssq_path` chain would answer for each difficulty.
//!
//! Basename: `--code` for a single song, otherwise the runtime's own
//! derivation (`services/stepmania/identity.rs`, mounted): `"z" +
//! base36(fnv1a64(casefold(pack) + "/" + casefold(song)))[0..5]` with
//! deterministic probing across the songs of one run. The folder-discovery,
//! lead-in timing and display-text rules come from the runtime's
//! `services/stepmania/convert.rs` as well, and the `<music>` element each
//! song would inject (series / property placeholders) is printed through
//! `services/stepmania/musicdb.rs`.
//!
//! `--auto-sync` measures every SSQ file against the song's decoded audio
//! through `core::sync::measure` and prints, per file, `measured_ms=` /
//! `correction_ms=` / the outcome. The written SSQ is left as converted.
//!
//! The conversion itself is `indexer::convert_song` — the same function the
//! default run uses to fill the game's cache — writing into `--out` through
//! `indexer::FlatPaths`; this module adds the full bank (built once so it
//! can be vetted) and the report. Output is a report, so it goes to stdout
//! directly rather than the log.

use std::collections::HashMap;
use std::fs;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use crate::core;
use crate::core::audio::decode::{decode_whole, Decoder, TargetFormat};
use crate::core::audio::preview::{PreviewDecoder, PreviewWindow};
use crate::core::audio::AudioInfo;
use crate::core::sm::model::Song;
use crate::core::sync::measure::SongAnalysis;
use crate::core::sync::{AudioView, Outcome, Params};
use crate::core::xact::song_bank::{self, SongBankPlan};
use crate::core::xact::{xsb, xwb};
use crate::indexer::{self, ArtefactPaths};
use crate::services::stepmania::convert::find_simfile;
use crate::services::stepmania::identity::{self, Identity};
use crate::services::stepmania::musicdb;

pub const USAGE: &str = "\
ddr_world_hook_prep --dry-run — convert a StepMania song or pack into a scratch directory

USAGE:
  ddr_world_hook_prep --dry-run <song-or-pack dir> --out <dir> [options]

OPTIONS:
  --out <dir>            Where <code>.ssq[_N] / <code>.xsb / <code>.xwb are written (required)
  --code <code>          Basename for a SINGLE song (1-6 chars [a-z0-9_]); default: the runtime's derivation
  --lead-in auto|0|<ms>  auto (default): smallest block multiple with td0 ≥ 60 ms and ≥ 2 s before the
                         first note; 0: none (SSQ byte-identical to the ddr-chart-tools CLI);
                         <ms>: at least that many ms, rounded up to whole ADPCM blocks
  --max-audio-mb <n>     Skip songs whose audio file is larger (default 256, the runtime cap)
  --auto-sync            Measure each SSQ against the audio (the runtime sweep's code path) and print
                         measured_ms / correction_ms / outcome per file; files are left as converted
  -h, --help
";

#[derive(Debug, Clone)]
pub struct Args {
    input: PathBuf,
    out: PathBuf,
    code: Option<String>,
    lead_in: LeadInArg,
    max_audio_mb: u64,
    auto_sync: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LeadInArg {
    Auto,
    Zero,
    AtLeastMs(u32),
}

/// Parse the arguments AFTER `--dry-run`.
pub fn parse_args(argv: &[String]) -> Result<Args, String> {
    let mut input = None;
    let mut out = None;
    let mut code = None;
    let mut lead_in = LeadInArg::Auto;
    let mut max_audio_mb = 256u64;
    let mut auto_sync = false;
    let mut it = argv.iter().cloned();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-h" | "--help" => return Err(String::new()),
            "--out" => out = Some(PathBuf::from(it.next().ok_or("--out needs a path")?)),
            "--code" => code = Some(it.next().ok_or("--code needs a value")?),
            "--lead-in" => {
                let v = it.next().ok_or("--lead-in needs auto|0|<ms>")?;
                lead_in = match v.as_str() {
                    "auto" => LeadInArg::Auto,
                    "0" => LeadInArg::Zero,
                    n => LeadInArg::AtLeastMs(
                        n.parse()
                            .map_err(|_| format!("bad --lead-in value {n:?}"))?,
                    ),
                };
            }
            "--auto-sync" => auto_sync = true,
            "--max-audio-mb" => {
                max_audio_mb = it
                    .next()
                    .ok_or("--max-audio-mb needs a number")?
                    .parse()
                    .map_err(|_| "bad --max-audio-mb".to_string())?;
            }
            other if other.starts_with('-') => return Err(format!("unknown option {other}")),
            other => {
                if input.is_some() {
                    return Err("only one input directory".into());
                }
                input = Some(PathBuf::from(other));
            }
        }
    }
    Ok(Args {
        input: input.ok_or("missing <song-or-pack dir>")?,
        out: out.ok_or("missing --out <dir>")?,
        code,
        lead_in,
        max_audio_mb,
        auto_sync,
    })
}

/// Run the dry-run; returns the process exit code (0 all converted, 1 some
/// rejected, 2 usage / I/O error).
pub fn run(argv: &[String]) -> i32 {
    let args = match parse_args(argv) {
        Ok(a) => a,
        Err(e) if e.is_empty() => {
            print!("{USAGE}");
            return 0;
        }
        Err(e) => {
            eprintln!("error: {e}\n\n{USAGE}");
            return 2;
        }
    };
    if let Err(e) = fs::create_dir_all(&args.out) {
        eprintln!("error: cannot create {}: {e}", args.out.display());
        return 2;
    }
    let songs = match discover_songs(&args.input) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {e}");
            return 2;
        }
    };
    if songs.is_empty() {
        eprintln!(
            "error: no .ssc/.sm found in {} or its subfolders",
            args.input.display()
        );
        return 1;
    }
    if args.code.is_some() && songs.len() > 1 {
        eprintln!(
            "error: --code applies to a single song folder; {} songs found",
            songs.len()
        );
        return 2;
    }
    let identities = assign_identities(&songs);
    let mut ok = 0usize;
    let mut failed = 0usize;
    for song_dir in &songs {
        let code = match &args.code {
            Some(c) => c.clone(),
            None => identities
                .get(song_dir)
                .map(|id| id.basename.clone())
                .unwrap_or_default(),
        };
        let mcode = identities.get(song_dir).map(|id| id.mcode).unwrap_or(0);
        match convert_song(song_dir, &args, &code, mcode) {
            Ok(report) => {
                print!("{report}");
                if report.verdict_ok {
                    ok += 1;
                } else {
                    failed += 1;
                }
            }
            Err(e) => {
                failed += 1;
                println!("== {}\n   REJECTED: {e}\n", song_dir.display());
            }
        }
    }
    println!("{ok} song(s) converted, {failed} rejected");
    if failed > 0 {
        1
    } else {
        0
    }
}

/// A song folder is one containing a `.ssc` or `.sm`; a pack is a folder
/// whose subfolders are songs.
fn discover_songs(input: &Path) -> Result<Vec<PathBuf>, String> {
    if !input.is_dir() {
        return Err(format!("{} is not a directory", input.display()));
    }
    if find_simfile(input).is_some() {
        return Ok(vec![input.to_path_buf()]);
    }
    let mut songs: Vec<PathBuf> = fs::read_dir(input)
        .map_err(|e| format!("cannot read {}: {e}", input.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir() && find_simfile(p).is_some())
        .collect();
    songs.sort();
    Ok(songs)
}

/// The runtime's identities for every discovered song (sorted-key probing
/// across the whole run, exactly as the index does).
fn assign_identities(songs: &[PathBuf]) -> HashMap<PathBuf, Identity> {
    let name = |p: Option<&Path>| {
        p.and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string()
    };
    let keyed: Vec<(String, PathBuf)> = songs
        .iter()
        .map(|dir| {
            (
                identity::key(&name(dir.parent()), &name(Some(dir))),
                dir.clone(),
            )
        })
        .collect();
    let keys: Vec<String> = keyed.iter().map(|(k, _)| k.clone()).collect();
    let assigned: HashMap<String, Identity> = identity::assign(&keys).into_iter().collect();
    keyed
        .into_iter()
        .filter_map(|(k, dir)| assigned.get(&k).cloned().map(|id| (dir, id)))
        .collect()
}

// ---------------------------------------------------------------------
// Conversion
// ---------------------------------------------------------------------

struct Report {
    lines: Vec<String>,
    verdict_ok: bool,
}

impl std::fmt::Display for Report {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for l in &self.lines {
            writeln!(f, "{l}")?;
        }
        writeln!(f)
    }
}

fn convert_song(song_dir: &Path, args: &Args, code: &str, mcode: u32) -> Result<Report, String> {
    let code = code.to_string();
    if !xsb::is_valid_name(&code) || code.len() > 6 {
        return Err(format!("code {code:?} must be 1-6 chars of [a-z0-9_]"));
    }
    let job = indexer::Job::for_dir(
        song_dir,
        Identity {
            basename: code.clone(),
            mcode,
            probe: 0,
        },
    )
    .ok_or("no simfile")?;
    // `root` only shapes the relative paths recorded in the entry.
    let root = job
        .pack_dir
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| job.pack_dir.clone());
    let opts = indexer::ConvertOptions {
        lead_in: match args.lead_in {
            LeadInArg::Auto => indexer::LeadInPolicy::Auto,
            LeadInArg::Zero => indexer::LeadInPolicy::Zero,
            LeadInArg::AtLeastMs(ms) => indexer::LeadInPolicy::AtLeastMs(ms),
        },
        max_audio_bytes: args.max_audio_mb * 1024 * 1024,
        strict_identity: false,
    };
    let paths = indexer::FlatPaths(args.out.clone());

    // The shared pipeline: SSQ (+ self-check), XSB and thumbnail into --out.
    let c = indexer::convert_song(&root, &job, &opts, &paths)?;
    let song = &c.song;
    let info = &c.info;
    let rate = info.target_rate();
    let hint = job
        .simfile
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("x.sm");

    let mut lines = vec![
        format!("== {}", song_dir.display()),
        format!("   title: {} — {}", c.entry.title, c.entry.artist),
        format!(
            "   simfile: {} ({} charts parsed, {} rejected by the parser)",
            hint,
            song.charts.len(),
            song.rejected.len()
        ),
    ];
    for r in &song.rejected {
        lines.push(format!(
            "     parser rejected {} {}: {}",
            r.stepstype, r.difficulty, r.reason
        ));
    }
    for w in &song.warnings {
        lines.push(format!("     warning: {w}"));
    }
    lines.push(format!(
        "   audio: {} — {:?} {} Hz {} ch, {} frames ({:.3} s){}; bank rate {} Hz, {} frames",
        c.audio_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?"),
        info.codec,
        info.rate,
        info.channels,
        info.frames_exact,
        info.duration_s(),
        if info.leading_silence_frames > 0 {
            format!(
                " incl. {} frames of StepMania Info-frame silence",
                info.leading_silence_frames
            )
        } else {
            String::new()
        },
        rate,
        info.target_frames()
    ));
    let lead_in = c.lead_in;
    lines.push(format!(
        "   lead-in: {} ms = {} frames ({} blocks{}); tempo_data[0] {} → {} ms; first note at {} → {} ms",
        lead_in.ms,
        lead_in.frames,
        lead_in.frames / 128,
        if lead_in.exact { ", exact" } else { ", ±0.5 ms" },
        c.td0_ms,
        c.td0_ms + lead_in.ms as i32,
        c.first_note_ms,
        c.first_note_ms + lead_in.ms as i32
    ));

    // Charts.
    let mut verdict_ok = true;
    let mut kept = 0usize;
    for file in &c.files {
        lines.push(format!(
            "   {}: {} bytes, charts {}, self-check {}",
            file.name,
            file.bytes.len(),
            file.charts
                .iter()
                .map(|(slot, level)| format!("{} lv{level}", slot_name(*slot)))
                .collect::<Vec<_>>()
                .join(", "),
            match &file.check {
                Ok(()) => "OK".to_string(),
                Err(f) => {
                    verdict_ok = false;
                    format!("FAILED {f:?}")
                }
            }
        ));
        kept += file.charts.len();
    }
    for (label, reason) in &c.bundle_rejects {
        lines.push(format!("     rejected {label}: {reason}"));
    }
    if let Some(why) = &c.scroll_conflict {
        lines.push(format!("     scroll gimmicks: {why}"));
    }
    let levels = c.entry.levels;
    let split_levels = c.entry.split_levels;
    if split_levels.iter().any(|&m| m != 0) {
        let answers: Vec<String> = (0..5)
            .map(|d| {
                format!(
                    "d={d}→{}",
                    match c.entry.split_file_for(d) {
                        Some(n) => format!("{code}_{n}.ssq"),
                        None => format!("{code}.ssq"),
                    }
                )
            })
            .collect();
        lines.push(format!(
            "   per-chart timing: split files served through ssq_path ({})",
            answers.join(", ")
        ));
    }

    // Auto-sync report (the runtime sweep's measurement, per file).
    if args.auto_sync {
        lines.extend(auto_sync_lines(
            song,
            &c.audio,
            info,
            &c.files,
            &code,
            lead_in.frames,
        ));
    }

    // Banks (the runtime builds these on demand; the dry run builds the full
    // bank once so it can be vetted).
    let window = PreviewWindow::for_audio(song.sample_start_s, song.sample_length_s, info);
    let frames_main = lead_in.frames + info.target_frames();
    let plan = SongBankPlan::new(&code, rate, frames_main, window.len_frames)
        .map_err(|e| format!("bank plan: {e}"))?;
    let target = TargetFormat {
        rate,
        lead_in_frames: lead_in.frames,
        total_frames: plan.frames(0),
    };
    let mut main_dec =
        Decoder::open(&c.audio, info, target).map_err(|e| format!("open main: {e}"))?;
    let mut preview_dec =
        PreviewDecoder::open(&c.audio, info, &window).map_err(|e| format!("open preview: {e}"))?;
    let xwb_path = args.out.join(format!("{code}.xwb"));
    let file =
        fs::File::create(&xwb_path).map_err(|e| format!("create {}: {e}", xwb_path.display()))?;
    let started = std::time::Instant::now();
    let sink = song_bank::write_complete(
        BufWriter::new(file),
        &plan,
        main_dec.by_ref(),
        preview_dec.by_ref(),
    )
    .map_err(|e| format!("xwb: {e}"))?;
    drop(sink);
    let encode_time = started.elapsed();
    if let Some(e) = main_dec.failed() {
        verdict_ok = false;
        lines.push(format!(
            "   main decode FAILED mid-stream (rest silent): {e}"
        ));
    }
    if let Some(e) = preview_dec.failed() {
        verdict_ok = false;
        lines.push(format!(
            "   preview decode FAILED mid-stream (rest silent): {e}"
        ));
    }
    let xsb_len = fs::metadata(paths.xsb(&code)).map(|m| m.len()).unwrap_or(0);

    // Bank self-check: read back through the strict stock parser.
    let xwb_bytes = fs::read(&xwb_path).map_err(|e| e.to_string())?;
    let bank_check = match xwb::parse_song_bank(&xwb_bytes) {
        Ok(b) if b.main_entry_index() == 0 && b.preview_entry_index() == 1 && b.name() == code => {
            "OK".to_string()
        }
        Ok(b) => {
            verdict_ok = false;
            format!(
                "FAILED (identity main={} preview={} name={})",
                b.main_entry_index(),
                b.preview_entry_index(),
                b.name()
            )
        }
        Err(e) => {
            verdict_ok = false;
            format!("FAILED {e}")
        }
    };
    lines.push(format!(
        "   {code}.xwb: {} bytes (main {} blocks = {:.3} s, preview {} blocks = {:.3} s from {:.3} s), encoded in {:.2} s, strict parse {bank_check}",
        xwb_bytes.len(),
        plan.blocks(0),
        plan.frames(0) as f64 / f64::from(rate),
        plan.blocks(1),
        window.len_s(),
        window.start_s(),
        encode_time.as_secs_f64(),
    ));
    lines.push(format!(
        "   {code}.xsb: {xsb_len} bytes (cues {code} → wave 0, {code}_s → wave 1)"
    ));
    lines.push(format!(
        "   {code}.dds: thumbnail from {}",
        match &c.entry.art.source {
            Some(src) => src.as_str(),
            None => "the placeholder jacket",
        }
    ));
    if levels.iter().any(|&l| l != 0) {
        lines.push(
            "   musicdb (series/property are placeholders -- the runtime picks them at boot):"
                .to_string(),
        );
        for l in musicdb::entry_xml(&c.entry, 0, None).lines() {
            lines.push(format!("   {l}"));
        }
    } else {
        verdict_ok = false;
        lines.push("   NO DECLARABLE SLOT: every SSQ file failed its self-check".to_string());
    }
    lines.push(format!(
        "   verdict: {} — {kept} chart(s) kept, {} rejected",
        if verdict_ok && kept > 0 {
            "OK"
        } else {
            "FAILED"
        },
        c.bundle_rejects.len() + song.rejected.len()
    ));
    Ok(Report {
        lines,
        verdict_ok: verdict_ok && kept > 0,
    })
}

/// One `auto-sync <file>: …` line per SSQ file of `bundle`.
fn auto_sync_lines(
    song: &Song,
    audio: &[u8],
    info: &AudioInfo,
    files: &[indexer::FileReport],
    _code: &str,
    lead_in_frames: u64,
) -> Vec<String> {
    let rate = info.target_rate();
    let started = std::time::Instant::now();
    let pcm = match decode_whole(audio, info) {
        Ok(p) => p,
        Err(e) => {
            return vec![format!(
                "   auto-sync: decode failed ({e}) -- outcome=refused reason=decode_failed"
            )]
        }
    };
    let analysis = SongAnalysis::new(&AudioView {
        samples: &pcm,
        sample_rate: rate,
        channels: 2,
    });
    let analysed = started.elapsed();
    let lead_in_s = lead_in_frames as f64 / f64::from(rate);
    let mut out = Vec::new();
    for file in files {
        let m = analysis.measure(song, &file.bytes, lead_in_s, &Params::default());
        let e = &m.estimate;
        let mut l = format!("   auto-sync {}:", file.name);
        if let Some(v) = e.measured_ms {
            l.push_str(&format!(" measured_ms={v:.2}"));
        }
        if let Some(v) = e.correction_ms {
            l.push_str(&format!(" correction_ms={v:.2}"));
        }
        match e.outcome {
            Outcome::Apply { delta_ms } => {
                l.push_str(&format!(" outcome=apply delta_ms={delta_ms}"))
            }
            Outcome::Unchanged => l.push_str(" outcome=unchanged delta_ms=0"),
            Outcome::Refused(r) => l.push_str(&format!(" outcome=refused reason={}", r.key())),
        }
        if let (Some(r), Some(s)) = (e.rival, e.split_ms) {
            l.push_str(&format!(" rival={r:.2} split_ms={s:.2}"));
        }
        l.push_str(&format!(" events={}", e.events_used));
        out.push(l);
    }
    out.push(format!(
        "   auto-sync: decode + onset analysis {:.2} s, slots {:?}",
        analysed.as_secs_f64(),
        files
            .iter()
            .map(|f| core::sync::measure::file_slots(&f.bytes))
            .collect::<Vec<_>>()
    ));
    out
}

fn slot_name(slot: u8) -> &'static str {
    match slot {
        0 => "SP-Beginner",
        1 => "SP-Basic",
        2 => "SP-Difficult",
        3 => "SP-Expert",
        4 => "SP-Challenge",
        5 => "DP-Beginner",
        6 => "DP-Basic",
        7 => "DP-Difficult",
        8 => "DP-Expert",
        9 => "DP-Challenge",
        _ => "?",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identities_follow_the_runtime_derivation() {
        let a = Path::new("Packs/Stamina RPG 5/[11] Big Money (Medium)");
        let b = Path::new("Other/STAMINA rpg 5/[11] BIG MONEY (Medium)");
        let ids = assign_identities(&[a.to_path_buf()]);
        let ids_b = assign_identities(&[b.to_path_buf()]);
        let ia = &ids[a];
        assert_eq!(ia.basename, ids_b[b].basename, "casefolded pack/song key");
        assert_eq!(ia.basename.len(), 6);
        assert!(ia.basename.starts_with('z'));
        assert!(xsb::is_valid_name(&ia.basename));
        assert_eq!(
            *ia,
            identity::derive(
                &identity::key("Stamina RPG 5", "[11] Big Money (Medium)"),
                0
            )
        );
    }
}
