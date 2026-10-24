//! `sm_pack_check` — dry-run StepMania → DDR World conversion.
//!
//! Mounts the hook DLL's REAL pure modules (`core::sm` parser,
//! `core::ssq::{writer,check}`, `core::audio`, `core::xact::{adpcm,
//! song_bank,xsb}`) via `#[path]` and runs a song folder — or every song
//! folder of a pack — through the same pipeline the runtime uses:
//! parse → SSQ (+ self-check) → probe audio → lead-in → song bank (XWB) +
//! sound bank (XSB). Artefacts land in `--out` under the song's basename
//! (`<code>.ssq` / `<code>_N.ssq`, `<code>.xsb`, `<code>.xwb`) so they can
//! be vetted offline or dropped into a mod folder for a cabinet check; the
//! verdict names every chart kept or rejected and why, and for a song with
//! per-chart timing lists which `<code>_N.ssq` the runtime's `ssq_path`
//! chain would answer for each difficulty.
//!
//! Basename: `--code` for a single song, otherwise the runtime's own
//! derivation (`services/stepmania/identity.rs`, mounted here): `"z" +
//! base36(fnv1a64(casefold(pack) + "/" + casefold(song)))[0..5]` with
//! deterministic probing across the songs of one run. The folder-discovery,
//! lead-in timing and display-text rules come from the runtime's
//! `services/stepmania/convert.rs` as well, and the `<music>` element each
//! song would inject (series / property placeholders) is printed through
//! `services/stepmania/musicdb.rs`.
//!
//! `--auto-sync` measures every SSQ file against the song's decoded audio
//! through `core::sync::measure` — the exact path the runtime sweep
//! (`services/stepmania/sync_sweep.rs`) uses — and prints, per file,
//! `measured_ms=` / `correction_ms=` / the outcome. The written SSQ is left
//! as converted (this is a report, like the CLI's `--auto-sync report`).
//!
//! `cargo test` here also runs the mounted modules' own `#[cfg(test)]`
//! suites; `scripts/validate_sm_pack_check.sh` is the one-liner for that.

#![allow(dead_code)]

pub mod core;
pub mod services;

use std::collections::HashMap;
use std::fs;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use core::audio::decode::{decode_whole, Decoder, TargetFormat};
use core::audio::lead_in::{choose_lead_in, LeadIn};
use core::audio::preview::{PreviewDecoder, PreviewWindow};
use core::audio::{probe, AudioInfo};
use core::sm::model::Song;
use core::ssq::check::check_ssq;
use core::ssq::writer::{self, WriteOptions};
use core::sync::measure::SongAnalysis;
use core::sync::{AudioView, Outcome, Params};
use core::xact::song_bank::{self, SongBankPlan};
use core::xact::{xsb, xwb};
use services::stepmania::convert::{self, chart_timings, find_audio, find_simfile};
use services::stepmania::identity::{self, Identity};
use services::stepmania::model::{AudioMeta, SongEntry};
use services::stepmania::musicdb;

const USAGE: &str = "\
sm_pack_check — dry-run StepMania → DDR World conversion

USAGE:
  sm_pack_check <song-or-pack dir> --out <dir> [options]

OPTIONS:
  --out <dir>            Where <code>.ssq[_N] / <code>.xsb / <code>.xwb are written (required)
  --code <code>          Basename for a SINGLE song (1-6 chars [a-z0-9_]); default: design §5.2 placeholder
  --lead-in auto|0|<ms>  auto (default): smallest block multiple with td0 ≥ 60 ms and ≥ 2 s before the
                         first note; 0: none (SSQ byte-identical to the ddr-chart-tools CLI);
                         <ms>: at least that many ms, rounded up to whole ADPCM blocks
  --max-audio-mb <n>     Skip songs whose audio file is larger (default 256, the runtime cap)
  --auto-sync            Measure each SSQ against the audio (the runtime sweep's code path) and print
                         measured_ms / correction_ms / outcome per file; files are left as converted
  -h, --help
";

#[derive(Debug, Clone)]
struct Args {
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

fn parse_args() -> Result<Args, String> {
    let mut input = None;
    let mut out = None;
    let mut code = None;
    let mut lead_in = LeadInArg::Auto;
    let mut max_audio_mb = 256u64;
    let mut auto_sync = false;
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "-h" | "--help" => {
                print!("{USAGE}");
                std::process::exit(0);
            }
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

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: {e}\n\n{USAGE}");
            std::process::exit(2);
        }
    };
    if let Err(e) = fs::create_dir_all(&args.out) {
        eprintln!("error: cannot create {}: {e}", args.out.display());
        std::process::exit(2);
    }
    let songs = match discover_songs(&args.input) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(2);
        }
    };
    if songs.is_empty() {
        eprintln!(
            "error: no .ssc/.sm found in {} or its subfolders",
            args.input.display()
        );
        std::process::exit(1);
    }
    if args.code.is_some() && songs.len() > 1 {
        eprintln!(
            "error: --code applies to a single song folder; {} songs found",
            songs.len()
        );
        std::process::exit(2);
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
        std::process::exit(1);
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
    let simfile = find_simfile(song_dir).ok_or("no simfile")?;
    let bytes = fs::read(&simfile).map_err(|e| format!("read {}: {e}", simfile.display()))?;
    let hint = simfile
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("x.sm");
    let song: Song = core::sm::parse_bytes(hint, &bytes).ok_or("simfile exceeds the 4 MiB cap")?;
    let code = code.to_string();
    if !xsb::is_valid_name(&code) || code.len() > 6 {
        return Err(format!("code {code:?} must be 1-6 chars of [a-z0-9_]"));
    }
    if !song.selectable {
        return Err("#SELECTABLE:NO".into());
    }

    let title = convert::display_title(&song);
    let artist = convert::display_artist(&song);
    let mut lines = vec![
        format!("== {}", song_dir.display()),
        format!("   title: {title} — {artist}"),
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

    // SSQ with no lead-in first (the CLI-identical output; also what the
    // lead-in rule reads its timings from).
    let bundle0 = writer::write_song(&song, &WriteOptions::default()).map_err(|e| {
        let mut why = format!("no playable chart ({e})");
        let writer::WriteError::NothingPlayable(rejects) = &e;
        for (label, reason) in rejects {
            why.push_str(&format!("\n     {label}: {reason}"));
        }
        why
    })?;

    // Audio.
    let audio_path = find_audio(song_dir, &song.music)
        .ok_or_else(|| format!("audio not found (#MUSIC {:?})", song.music))?;
    let audio_len = fs::metadata(&audio_path).map(|m| m.len()).unwrap_or(0);
    if audio_len > args.max_audio_mb * 1024 * 1024 {
        return Err(format!(
            "audio {} is {} MiB > --max-audio-mb {}",
            audio_path.display(),
            audio_len / (1024 * 1024),
            args.max_audio_mb
        ));
    }
    let audio = fs::read(&audio_path).map_err(|e| format!("read {}: {e}", audio_path.display()))?;
    let info: AudioInfo = probe::probe(&audio).map_err(|e| {
        format!(
            "audio {}: {e}",
            audio_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("?")
        )
    })?;
    lines.push(format!(
        "   audio: {} — {:?} {} Hz {} ch, {} frames ({:.3} s){}; bank rate {} Hz, {} frames",
        audio_path
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
        info.target_rate(),
        info.target_frames()
    ));

    // Lead-in.
    let rate = info.target_rate();
    let (td0_min, first_note_ms) = chart_timings(&bundle0);
    let lead_in = match args.lead_in {
        LeadInArg::Zero => LeadIn::ZERO,
        LeadInArg::AtLeastMs(ms) => LeadIn::at_least_ms(ms, rate),
        LeadInArg::Auto => choose_lead_in(
            -(td0_min as f64) / 1000.0,
            first_note_ms as f64 / 1000.0,
            rate,
        ),
    };
    lines.push(format!(
        "   lead-in: {} ms = {} frames ({} blocks{}); tempo_data[0] {} → {} ms; first note at {} → {} ms",
        lead_in.ms,
        lead_in.frames,
        lead_in.frames / 128,
        if lead_in.exact { ", exact" } else { ", ±0.5 ms" },
        td0_min,
        td0_min + lead_in.ms as i32,
        first_note_ms,
        first_note_ms + lead_in.ms as i32
    ));
    let bundle = if lead_in.ms == 0 {
        bundle0
    } else {
        writer::write_song(
            &song,
            &WriteOptions {
                lead_in_ms: lead_in.ms,
                ..WriteOptions::default()
            },
        )
        .map_err(|e| e.to_string())?
    };

    // Charts.
    let mut verdict_ok = true;
    let mut kept = 0usize;
    let mut passed: Vec<&writer::SsqFile> = Vec::new();
    for file in &bundle.files {
        let name = convert::ssq_file_name(&code, file.index);
        let slots: Vec<u8> = file.charts.iter().map(|c| c.0).collect();
        let check = check_ssq(&file.bytes, &slots);
        let path = args.out.join(&name);
        fs::write(&path, &file.bytes).map_err(|e| format!("write {}: {e}", path.display()))?;
        lines.push(format!(
            "   {name}: {} bytes, charts {}, self-check {}",
            file.bytes.len(),
            file.charts
                .iter()
                .map(|(slot, level)| format!("{} lv{level}", slot_name(*slot)))
                .collect::<Vec<_>>()
                .join(", "),
            match &check {
                Ok(()) => "OK".to_string(),
                Err(f) => {
                    verdict_ok = false;
                    format!("FAILED {f:?}")
                }
            }
        ));
        // The runtime declares a slot only when its file passed.
        if check.is_ok() {
            passed.push(file);
        }
        kept += file.charts.len();
    }
    for (label, reason) in &bundle.rejects {
        lines.push(format!("     rejected {label}: {reason}"));
    }
    let declared = convert::declare(&passed);
    let levels = declared.levels;
    let split_levels = declared.split_levels;
    if split_levels.iter().any(|&m| m != 0) {
        let probe = SongEntry {
            split_levels,
            ..SongEntry::default()
        };
        let answers: Vec<String> = (0..5)
            .map(|d| {
                format!(
                    "d={d}→{}",
                    match probe.split_file_for(d) {
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
            &song,
            &audio,
            &info,
            &bundle,
            &code,
            lead_in.frames,
        ));
    }

    // Banks.
    let window = PreviewWindow::for_audio(song.sample_start_s, song.sample_length_s, &info);
    let frames_main = lead_in.frames + info.target_frames();
    let plan = SongBankPlan::new(&code, rate, frames_main, window.len_frames)
        .map_err(|e| format!("bank plan: {e}"))?;
    let target = TargetFormat {
        rate,
        lead_in_frames: lead_in.frames,
        total_frames: plan.frames(0),
    };
    let mut main_dec =
        Decoder::open(&audio, &info, target).map_err(|e| format!("open main: {e}"))?;
    let mut preview_dec =
        PreviewDecoder::open(&audio, &info, &window).map_err(|e| format!("open preview: {e}"))?;
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
    let xsb_bytes = xsb::write_song_profile(&code).map_err(|e| e.to_string())?;
    let xsb_path = args.out.join(format!("{code}.xsb"));
    fs::write(&xsb_path, &xsb_bytes).map_err(|e| format!("write {}: {e}", xsb_path.display()))?;

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
        "   {code}.xsb: {} bytes (cues {code} → wave 0, {code}_s → wave 1)",
        xsb_bytes.len()
    ));
    if levels.iter().any(|&l| l != 0) {
        let (bpm_min, bpm_max) = convert::bpm_range(&song);
        let entry = SongEntry {
            basename: code.clone(),
            mcode,
            title: title.clone(),
            title_sort: convert::sort_title(&song),
            artist: artist.clone(),
            bpm_min,
            bpm_max,
            levels,
            audio: AudioMeta {
                rate,
                frames: info.target_frames(),
                lead_in_frames: lead_in.frames,
                lead_in_ms: lead_in.ms,
                ..AudioMeta::default()
            },
            split_levels,
            ..SongEntry::default()
        };
        lines.push(
            "   musicdb (series/property are placeholders -- the runtime picks them at boot):"
                .to_string(),
        );
        for l in musicdb::entry_xml(&entry, 0, None).lines() {
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
        bundle.rejects.len() + song.rejected.len()
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
    bundle: &writer::SsqBundle,
    code: &str,
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
    for file in &bundle.files {
        let m = analysis.measure(song, &file.bytes, lead_in_s, &Params::default());
        let e = &m.estimate;
        let mut l = format!("   auto-sync {}:", convert::ssq_file_name(code, file.index));
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
        bundle
            .files
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
