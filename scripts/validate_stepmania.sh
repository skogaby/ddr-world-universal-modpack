#!/usr/bin/env bash
# Offline validation for the StepMania pack loader's pure chart + audio
# pipeline (`src/core/sm/` parser, `src/core/ssq/{writer,check}.rs`,
# `src/core/audio/`, `src/core/xact/{song_bank,xsb}.rs`, `src/core/art/
# {source,jacket,thumbnail}.rs`) and the loader's pure service files
# (`src/services/stepmania/{identity,musicdb,model,convert,ceiling,
# scheduler_logic,progressive_logic,sync_state}.rs` and
# `src/services/ssq_path_logic.rs`) and
# the auto-sync estimator (`src/core/sync/`); plan
# `.agents/planning/2026-10-20-stepmania-pack-loader/implementation/plan.md`
# Steps 4, 5, 6, 8, 9, 10 and 12.
#
# Usage:
#   ./scripts/validate_stepmania.sh                    # sibling checkout at ../ddr-chart-tools
#   ./scripts/validate_stepmania.sh --chart-tools <p>  # point at the checkout explicitly
#   DDR_CHART_TOOLS=<p> ./scripts/validate_stepmania.sh
#
# Why a harness: plain `cargo test` cannot build the DLL crate on non-x86
# hosts (`retour`), so this builds a throwaway HOST cargo crate in a temp
# directory that mounts `crate::core::{sm,ssq}` via `#[path]` under a `core`
# module mirror (the same trick `tools/bot_sim` uses) and runs:
#
#   1. every `#[cfg(test)]` suite of the mounted files (`cargo test`);
#   2. BYTE IDENTITY: a generated corpus of `.ssc`/`.sm` songs (taps, jumps,
#      hands, holds, BPM changes, stops, partial mines, full-row shocks,
#      12th/24th/48th rows, offsets, trailing tempo events) is converted by the
#      sibling `ddr-chart-tools` CLI and by `writer::write_song` with
#      `lead_in_ms = 0`, `sync_bias_ms = 0`; `files[0].bytes` must equal the
#      CLI's `.ssq` byte for byte. SKIPPED (with a note) when the sibling
#      checkout or its release binary is absent;
#   3. quad split: `L+R` at t, `D+U` at t+1 for every standard quantisation,
#      both rows in the same colour class under the game's selector
#      (`q = (tick>>6)*3 + ((tick&0x3F)*3+0x20>>6)`), no shock byte leaks;
#   4. self-check negatives: mines-only, holds-only, zero-Δms tempo, END before
#      the last note, shock-byte leak — each reports its specific failure;
#      every corpus file passes;
#   5. delay nudge: a `#DELAYS` entry becomes a stop at `tick − 1`;
#   6. `shift_seconds` round trip;
#   7. per-chart timing split grouping into `<b>_N.ssq` files;
#   8. lead-in / sync-bias shift every anchor and nothing else;
#   9. XSB: `xsb::write_song_profile_indexed(code, TOOL)` is byte-identical
#      to the sibling's `xsb::write(code)` for 4/5/6-char codes, and the
#      stock-order default differs from it in exactly the two wave-index
#      bytes plus the CRC;
#  10. XWB: for the same PCM, our main/preview entries equal the sibling's
#      `build_xwb_bank`-shaped entries byte for byte (metadata row fields,
#      names, ADPCM data), and the whole file equals the sibling container
#      writer's output when fed main-first; a 2 min + 15 s silence bank
#      passes `xwb::parse_song_bank` strictly with main = 0 / preview = 1;
#      our ADPCM decodes identically through the sibling's decoder; the
#      progressive writer's watermark equals `block_offset(0, k)`;
#  11. audio: probe frame counts equal decoded frame counts for a WAV, a
#      Vorbis fixture (sibling `ogg::encode`; also ±1 LSB vs its lewton
#      decoder) and — when `lame` is on PATH — CBR (`Info` → one frame of
#      StepMania silence), VBR (`Xing`) and mono 22.05 kHz MP3s; preview
#      windows decode sample-exactly against a whole decode after a seek;
#      lead-in arithmetic on the Step 1 song's numbers;
#  12. identity: `identity::derive` is deterministic and casefolded, yields a
#      6-byte `z[0-9a-z]{5}` basename and an mcode in 9,000,000..9,999,999;
#      `assign` is input-order independent, collision-free over thousands of
#      keys, probes deterministically, and never hands out a reserved
#      basename/mcode;
#  13. musicdb: `entry_xml` reproduces the cabinet-proven spike entry's shape
#      field for field (types, counts, slot rules: limited_ary 0/-1, diffLv
#      1..19 never 255 / never all-zero, region omitted), clips title/artist to
#      127 bytes at a UTF-8 boundary, escapes `& < >` like the stock file,
#      `transform` inserts before the LAST `</mdb>` and drops base-musicdb
#      mcode/basename collisions, `index.bin` and `lru.bin` round-trip;
#  14. art: the thumbnail DDS is exactly 18,560 bytes with the stock header
#      (flags 0x00081007, DXT1, caps 0x1000, 192x192, linear size 18,432),
#      the jacket square is 512x512 from a jacket, a banner (over a blurred
#      backdrop) and a background (centre crop), and the LRU evicts
#      least-recently-served entries only;
#  15. scheduler + progressive (Step 8): a fake worker drains the job queue in
#      priority order (gameplay-load full bank > highlighted preview > visible
#      previews in binding order > highlighted jacket > visible jackets >
#      speculative full bank), dedupes `(kind, basename)` handing the first
#      ticket back, re-highlights, and cancels only speculative full banks;
#      the read-hook decision waits exactly until the watermark covers the
#      read (or the early-written tail does, or the writer is done), times out
#      at the bound under a fake clock, and the per-fd cursor follows the
#      returned lengths;
#  16. scroll gimmicks (Step 9): `core::sm::scroll` against StepMania-derived
#      vectors — identity tables, a 0-factor region freezing display beat,
#      a negative factor, the Matt Silver `#SCROLLS`/`#SPEEDS` tables
#      (116.0–116.5 flat; speed 0 at 160, 0 → 1 over 6 beats from 160.021,
#      0.5 → 1 over 16 beats from 196.021), a unit-1 ramp in seconds, the
#      renderer remap arithmetic (`d'` ticks + speed scale), the parser's
#      hold-error tolerance (unclosed head → tap, orphan tail ignored, both
#      warned), per-chart table resolution for the committed charts, and
#      the `index.bin` round trip of the tables;
#  17. ssq_path + split declaration (Step 10): the pure resolver chain orders
#      by ascending priority, first answer wins, `None` from every resolver
#      means "the original"; a per-chart-timing song's bundle declares every
#      level from its `<b>_N.ssq` files (`convert::declare`), the index's
#      Rule-A lookup names `_N` per difficulty, a file that failed its
#      self-check costs only its slots, and `index.bin` (FORMAT_VERSION >= 4)
#      round-trips the split table.
#  18. auto-sync (Step 12): `core::sync` (ported from the sibling's
#      `src/sync/`) on a synthetic click track measured through the runtime's
#      own path (`measure::SongAnalysis` over the file the index writes, lead-in
#      included): known offsets → `Apply{round(measured − T)}`, the lead-in
#      subtracted exactly; refusals too_few_events / beyond_cap /
#      halves_disagree / ambiguous_peak / no_audio; the per-song `sync.bin`
#      (v2) round trip, malformed input → empty and the stale-fingerprint
#      drop; the one-delta-per-song pick over split files (+ disagreement
#      flag); the live-map freeze rule (publish before/after the freeze,
#      unmeasured → None, refused → None); `shift_seconds` ±d identity;
#      and — with the sibling checkout — our `chart_events` and estimator
#      against its `sync` module on the same inputs (±1 ms).
#
# Requires: cargo (host toolchain). The identity/parity legs additionally
# need a ddr-chart-tools SOURCE checkout (for its OGG encoder, XWB/XSB
# writers and ADPCM codec) with a built binary at target/release/ddr-chart-tools
# (or target/debug/); the MP3 legs need `lame` (SKIPPED otherwise). Nothing
# else; no game install, no Windows toolchain. This script is validation only
# — it writes nothing into the repository.

set -euo pipefail
cd "$(dirname "$0")/.."
REPO_ROOT="$(pwd)"

die() { echo "error: $*" >&2; exit 1; }
note() { echo "[*] $*"; }

CHART_TOOLS="${DDR_CHART_TOOLS:-}"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --chart-tools) [[ $# -ge 2 ]] || die "--chart-tools needs a path"; CHART_TOOLS="$2"; shift 2 ;;
    -h|--help)     sed -n '2,/^$/p' "$0"; exit 0 ;;
    *)             die "unknown option: $1" ;;
  esac
done
if [[ -z "$CHART_TOOLS" && -f "$REPO_ROOT/../ddr-chart-tools/Cargo.toml" ]]; then
  CHART_TOOLS="$REPO_ROOT/../ddr-chart-tools"
fi
TOOL_BIN=""
if [[ -n "$CHART_TOOLS" && -f "$CHART_TOOLS/Cargo.toml" ]]; then
  CHART_TOOLS="$(cd "$CHART_TOOLS" && pwd)"
  for cand in "$CHART_TOOLS/target/release/ddr-chart-tools" "$CHART_TOOLS/target/debug/ddr-chart-tools"; do
    if [[ -x "$cand" ]]; then TOOL_BIN="$cand"; break; fi
  done
  [[ -n "$TOOL_BIN" ]] || note "ddr-chart-tools binary not built (run 'cargo build --release' in the checkout); identity leg will be SKIPPED"
else
  CHART_TOOLS=""
  note "no ddr-chart-tools checkout (--chart-tools, DDR_CHART_TOOLS, or ../ddr-chart-tools); identity leg will be SKIPPED"
fi
LAME_BIN="$(command -v lame || true)"
[[ -n "$LAME_BIN" ]] || note "lame not on PATH; MP3 fixture legs will be SKIPPED"

for f in mod.rs model.rs msd.rs notes.rs scroll.rs simfile.rs text.rs; do
  [[ -r "$REPO_ROOT/src/core/sm/$f" ]] || die "module source missing: src/core/sm/$f"
done
for f in mod.rs ssq_chunk.rs timing.rs writer.rs check.rs; do
  [[ -r "$REPO_ROOT/src/core/ssq/$f" ]] || die "module source missing: src/core/ssq/$f"
done
for f in mod.rs probe.rs decode.rs preview.rs lead_in.rs; do
  [[ -r "$REPO_ROOT/src/core/audio/$f" ]] || die "module source missing: src/core/audio/$f"
done
for f in mod.rs adpcm.rs resample.rs song_bank.rs xsb.rs xwb.rs; do
  [[ -r "$REPO_ROOT/src/core/xact/$f" ]] || die "module source missing: src/core/xact/$f"
done
for f in source.rs jacket.rs thumbnail.rs; do
  [[ -r "$REPO_ROOT/src/core/art/$f" ]] || die "module source missing: src/core/art/$f"
done
for f in mod.rs onset.rs estimate.rs events.rs timemap.rs measure.rs; do
  [[ -r "$REPO_ROOT/src/core/sync/$f" ]] || die "module source missing: src/core/sync/$f"
done
for f in identity.rs musicdb.rs model.rs convert.rs ceiling.rs scheduler_logic.rs progressive_logic.rs sync_state.rs; do
  [[ -r "$REPO_ROOT/src/services/stepmania/$f" ]] || die "module source missing: src/services/stepmania/$f"
done
[[ -r "$REPO_ROOT/src/services/ssq_path_logic.rs" ]] || die "module source missing: src/services/ssq_path_logic.rs"

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
note "harness dir: $TMP"

{
  cat <<EOF
[package]
name = "stepmania-validate"
version = "0.0.0"
edition = "2021"

[lib]
path = "src/lib.rs"

[[bin]]
name = "legs"
path = "src/main.rs"

[dependencies]
encoding_rs = "0.8"
symphonia = { version = "0.6", default-features = false, features = ["mp3", "vorbis", "wav", "pcm", "ogg"] }
image = { version = "0.25", default-features = false, features = ["png", "jpeg", "gif"] }
texpresso = "2"
rustfft = "6"
EOF
  if [[ -n "$CHART_TOOLS" ]]; then
    echo "ddr-chart-tools = { path = \"$CHART_TOOLS\", optional = true }"
    echo
    echo "[features]"
    echo "identity = [\"dep:ddr-chart-tools\"]"
  else
    echo
    echo "[features]"
    echo "identity = []"
  fi
  echo
  echo "[workspace]"
} >"$TMP/Cargo.toml"

mkdir -p "$TMP/src"
cat >"$TMP/src/lib.rs" <<EOF
//! Throwaway offline-validation harness — generated by
//! scripts/validate_stepmania.sh; never committed.
#![allow(dead_code)]

pub mod core {
    pub mod art {
        #[path = "$REPO_ROOT/src/core/art/jacket.rs"]
        pub mod jacket;
        #[path = "$REPO_ROOT/src/core/art/source.rs"]
        pub mod source;
        #[path = "$REPO_ROOT/src/core/art/thumbnail.rs"]
        pub mod thumbnail;
    }
    #[path = "$REPO_ROOT/src/core/audio/mod.rs"]
    pub mod audio;
    #[path = "$REPO_ROOT/src/core/sm/mod.rs"]
    pub mod sm;
    #[path = "$REPO_ROOT/src/core/ssq/mod.rs"]
    pub mod ssq;
    #[path = "$REPO_ROOT/src/core/sync/mod.rs"]
    pub mod sync;
    #[path = "$REPO_ROOT/src/core/xact/mod.rs"]
    pub mod xact;
}

pub mod services {
    #[path = "$REPO_ROOT/src/services/ssq_path_logic.rs"]
    pub mod ssq_path_logic;
    pub mod stepmania {
        #[path = "$REPO_ROOT/src/services/stepmania/ceiling.rs"]
        pub mod ceiling;
        #[path = "$REPO_ROOT/src/services/stepmania/convert.rs"]
        pub mod convert;
        #[path = "$REPO_ROOT/src/services/stepmania/identity.rs"]
        pub mod identity;
        #[path = "$REPO_ROOT/src/services/stepmania/model.rs"]
        pub mod model;
        #[path = "$REPO_ROOT/src/services/stepmania/musicdb.rs"]
        pub mod musicdb;
        #[path = "$REPO_ROOT/src/services/stepmania/progressive_logic.rs"]
        pub mod progressive_logic;
        #[path = "$REPO_ROOT/src/services/stepmania/scheduler_logic.rs"]
        pub mod scheduler_logic;
        #[path = "$REPO_ROOT/src/services/stepmania/sync_state.rs"]
        pub mod sync_state;
    }
}
EOF

# The legs binary. Quoted heredoc: no shell substitution inside; runtime
# inputs (tool binary, scratch dir) arrive as environment variables.
cat >"$TMP/src/main.rs" <<'EOF'
//! Throwaway offline-validation harness — generated by
//! scripts/validate_stepmania.sh; never committed.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

use stepmania_validate::core::sm::model::{
    Beat, Bpm, Chart, Difficulty, Note, NoteKind, PanelSet, Rational, ShockSide, Style, Timing,
};
use stepmania_validate::core::sm::scroll::{
    self as sm_scroll, ScrollGimmicks, ScrollTable, SpeedTable,
};
use stepmania_validate::core::sm::simfile;
use stepmania_validate::core::ssq::check::{check_ssq, CheckFailure};
use stepmania_validate::core::ssq::timing::TempoConverter;
use stepmania_validate::core::ssq::writer::{
    self, build_file, prepare_chart, QuadPolicy, WriteOptions,
};
use stepmania_validate::core::audio::decode::{decode_whole, Decoder, TargetFormat};
use stepmania_validate::core::audio::lead_in::{choose_lead_in, LeadIn};
use stepmania_validate::core::audio::preview::{PreviewDecoder, PreviewWindow};
use stepmania_validate::core::audio::{probe, Codec, StereoBlock};
use stepmania_validate::core::xact::song_bank::{self, SongBankPlan, SongBankWriter};
use stepmania_validate::core::xact::{adpcm, xsb, xwb};
use stepmania_validate::core::art::jacket::{self as jacket_art, ArtSource};
use stepmania_validate::core::art::source::SourceKind;
use stepmania_validate::core::art::thumbnail;
use stepmania_validate::services::stepmania::identity;
use stepmania_validate::services::stepmania::convert as sm_convert;
use stepmania_validate::services::stepmania::model::{
    self as sm_model, AudioMeta, IndexFile, LruTable, RejectEntry, SongEntry,
};
use stepmania_validate::services::stepmania::musicdb;
use stepmania_validate::services::stepmania::progressive_logic::{
    self, FdCursor, Poll, Progress,
};
use stepmania_validate::services::stepmania::scheduler_logic::{
    Absorbed, Cancelled, JobKind, JobQueue, Urgency,
};
use stepmania_validate::services::ssq_path_logic::Chain;
use stepmania_validate::core::sync::measure::{file_slots, first_anchor, SongAnalysis};
use stepmania_validate::core::sync::{
    self as sync, AudioView, Outcome, Params, Refusal, SyncEvent, TARGET_OFFSET_MS,
};
use stepmania_validate::services::stepmania::sync_state::{
    decide_song, FileResult, LiveEntry, LiveMap, Publish, SyncRecord, SyncState, SyncTable,
};

struct Report {
    pass: u32,
    fail: u32,
    skip: u32,
}

impl Report {
    fn check(&mut self, name: &str, ok: bool, detail: String) {
        if ok {
            self.pass += 1;
            println!("PASS  {name}");
        } else {
            self.fail += 1;
            println!("FAIL  {name}: {detail}");
        }
    }
    fn skip(&mut self, name: &str, why: &str) {
        self.skip += 1;
        println!("SKIP  {name}: {why}");
    }
}

fn r(n: i64, d: i64) -> Rational {
    Rational::new(n, d).expect("rational")
}

fn timing(bpm: i64) -> Timing {
    Timing {
        bpms: vec![(Beat::zero(), Bpm::from_rational(r(bpm, 1)))],
        ..Timing::default()
    }
}

fn chart(style: Style, difficulty: Difficulty, notes: Vec<Note>) -> Chart {
    Chart {
        style,
        difficulty,
        meter: 9,
        description: String::new(),
        timing: None,
        scrolls: None,
        speeds: None,
        notes,
        warnings: Vec::new(),
    }
}

fn tap(beat: Rational, bits: u8) -> Note {
    Note {
        beat: Beat::from_rational(beat),
        kind: NoteKind::Tap,
        panels: PanelSet::from_bits(Style::Double, bits),
    }
}

// ---------------------------------------------------------------------
// Deterministic corpus generator (SSC/SM text the tool handles unchanged)
// ---------------------------------------------------------------------

struct Rng(u64);

impl Rng {
    fn next_u32(&mut self) -> u32 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 32) as u32
    }
    fn below(&mut self, n: u32) -> u32 {
        self.next_u32() % n.max(1)
    }
    fn chance(&mut self, per_mille: u32) -> bool {
        self.below(1000) < per_mille
    }
    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len() as u32) as usize]
    }
}

const QUANTS: [u32; 9] = [4, 8, 12, 16, 24, 32, 48, 64, 192];
const OFFSETS: [&str; 5] = ["0.000", "-0.123", "0.050", "-1.500", "0.250"];
const BPMS: [&str; 6] = ["120.000", "174.500", "249.999985", "97.333", "200", "140.25"];
const STOP_LENS: [&str; 5] = ["0.500", "0.125", "0.333333", "1.000", "0.052"];
const ALL_CHARTS: [(&str, &str); 9] = [
    ("dance-single", "Beginner"),
    ("dance-single", "Easy"),
    ("dance-single", "Medium"),
    ("dance-single", "Hard"),
    ("dance-single", "Challenge"),
    ("dance-double", "Easy"),
    ("dance-double", "Medium"),
    ("dance-double", "Hard"),
    ("dance-double", "Challenge"),
];

/// A `#NOTES` body with taps, jumps, hands, holds, partial mines and
/// full-row shocks, never a full side nibble on a note row, every hold
/// closed, and a tap at beat 0.
fn gen_notes(rng: &mut Rng, panels: usize) -> String {
    let measures = 2 + rng.below(9) as usize;
    let mut open = vec![false; panels];
    let mut out: Vec<String> = Vec::new();
    for m in 0..measures {
        let rows = *rng.pick(&QUANTS) as usize;
        let per_mille = (1800 / rows as u32).clamp(8, 450);
        let mut lines: Vec<String> = Vec::with_capacity(rows);
        for row_idx in 0..rows {
            let mut row = vec!['0'; panels];
            let shock_side = if rng.chance(25) { Some(rng.below(3)) } else { None };
            if let Some(side) = shock_side {
                let range: Vec<usize> = match (panels, side) {
                    (4, _) => (0..4).collect(),
                    (8, 0) => (0..8).collect(),
                    (8, 1) => (0..4).collect(),
                    _ => (4..8).collect(),
                };
                if range.iter().all(|&p| !open[p]) {
                    for p in range {
                        row[p] = 'M';
                    }
                    lines.push(row.into_iter().collect());
                    continue;
                }
            }
            for p in 0..panels {
                if open[p] {
                    if rng.chance(250) {
                        row[p] = '3';
                        open[p] = false;
                    }
                    continue;
                }
                if rng.chance(per_mille) {
                    row[p] = match rng.below(10) {
                        0..=5 => '1',
                        6..=7 => {
                            open[p] = true;
                            '2'
                        }
                        _ => 'M',
                    };
                }
            }
            if m == 0 && row_idx == 0 && !row.contains(&'1') {
                if row[0] == '2' {
                    open[0] = false;
                }
                row[0] = '1';
            }
            // Never a full side nibble of judged panels on a note row.
            for side in 0..(panels / 4) {
                let cells = side * 4..side * 4 + 4;
                let full = cells.clone().all(|p| matches!(row[p], '1' | '2'));
                if full {
                    let p = side * 4 + 1;
                    if row[p] == '2' {
                        open[p] = false;
                    }
                    row[p] = '0';
                }
            }
            lines.push(row.into_iter().collect());
        }
        out.push(lines.join("\n"));
    }
    if open.iter().any(|&o| o) {
        let mut rows = vec![vec!['0'; panels]; 4];
        for (p, o) in open.iter().enumerate() {
            if *o {
                rows[0][p] = '3';
            }
        }
        out.push(
            rows.into_iter()
                .map(|r| r.into_iter().collect::<String>())
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    out.join("\n,\n")
}

fn gen_timing(rng: &mut Rng) -> (String, String) {
    let mut bpms = vec![format!("0.000={}", rng.pick(&BPMS))];
    let mut beat = 0u32;
    for _ in 0..rng.below(4) {
        beat += 4 + rng.below(12);
        let frac = *rng.pick(&["", ".5", ".25", ".75"]);
        bpms.push(format!("{beat}{frac}={}", rng.pick(&BPMS)));
    }
    let mut stops = Vec::new();
    let mut sbeat = 1u32;
    for _ in 0..rng.below(4) {
        sbeat += 1 + rng.below(10);
        let frac = *rng.pick(&["", "", ".5", ".25", ".333333"]);
        stops.push(format!("{sbeat}{frac}={}", rng.pick(&STOP_LENS)));
    }
    if rng.chance(300) {
        // A stop on a BPM-change beat shares its anchor.
        if let Some(b) = bpms.get(1) {
            let beat_s = b.split('=').next().unwrap_or("8");
            stops.push(format!("{beat_s}={}", rng.pick(&STOP_LENS)));
        }
    }
    (bpms.join(","), stops.join(","))
}

struct GenSong {
    stem: String,
    ext: &'static str,
    text: String,
}

fn gen_song(rng: &mut Rng, idx: usize) -> GenSong {
    let (bpms, stops) = gen_timing(rng);
    let offset = *rng.pick(&OFFSETS);
    let mut picked: Vec<(&str, &str)> = ALL_CHARTS
        .iter()
        .copied()
        .filter(|_| rng.chance(450))
        .collect();
    if picked.is_empty() {
        picked.push(ALL_CHARTS[3]);
    }
    let sm = idx % 4 == 3;
    let mut text = String::new();
    if !sm {
        text.push_str("#VERSION:0.83;\n");
    }
    text.push_str(&format!("#TITLE:Corpus {idx};\n#ARTIST:harness;\n"));
    text.push_str(&format!("#OFFSET:{offset};\n#SAMPLESTART:0.000;\n#SAMPLELENGTH:0.100;\n"));
    text.push_str(&format!("#BPMS:{bpms};\n#STOPS:{stops};\n#DELAYS:;\n#WARPS:;\n"));
    for (style, diff) in picked {
        let panels = if style == "dance-single" { 4 } else { 8 };
        let body = gen_notes(rng, panels);
        if sm {
            text.push_str(&format!(
                "#NOTES:\n     {style}:\n     :\n     {diff}:\n     7:\n     0,0,0,0,0:\n{body}\n;\n"
            ));
        } else {
            text.push_str(&format!(
                "#NOTEDATA:;\n#STEPSTYPE:{style};\n#DIFFICULTY:{diff};\n#METER:7;\n#NOTES:\n{body}\n;\n"
            ));
        }
    }
    GenSong {
        stem: format!("c{idx:02}"),
        ext: if sm { "sm" } else { "ssc" },
        text,
    }
}

/// Hand-written edge cases on top of the random corpus.
fn fixed_corpus() -> Vec<GenSong> {
    let mut v = Vec::new();
    let mk = |stem: &str, body: &str| GenSong {
        stem: stem.to_string(),
        ext: "ssc",
        text: body.to_string(),
    };
    v.push(mk(
        "f01",
        "#OFFSET:-0.250;\n#BPMS:0.000=120.000,4.000=150.000;\n#STOPS:8.000=0.500,4.000=0.250;\n#NOTEDATA:;\n#STEPSTYPE:dance-single;\n#DIFFICULTY:Hard;\n#NOTES:\n1000\n0100\n0010\n0001\n,\n1100\n0000\n0011\n0000\n,\n2000\n0000\n0000\n0000\n,\n3000\n0000\n0000\n0000\n;\n",
    ));
    v.push(mk(
        "f02",
        "#OFFSET:0.000;\n#BPMS:0.000=200.000;\n#STOPS:;\n#NOTEDATA:;\n#STEPSTYPE:dance-single;\n#DIFFICULTY:Beginner;\n#NOTES:\n1000\n;\n",
    ));
    v.push(mk(
        "f03",
        "#OFFSET:0.500;\n#BPMS:0.000=100.000,64.000=200.000;\n#STOPS:70.000=1.000;\n#NOTEDATA:;\n#STEPSTYPE:dance-double;\n#DIFFICULTY:Challenge;\n#NOTES:\n10000000\n00000000\nMMMMMMMM\n00000000\n,\nMMMM0000\n0000MMMM\nM0M00000\n00000001\n;\n",
    ));
    v.push(mk(
        "f04",
        "#OFFSET:-0.033;\n#BPMS:0.000=180.000;\n#STOPS:;\n#NOTEDATA:;\n#STEPSTYPE:dance-single;\n#DIFFICULTY:Medium;\n#NOTES:\n1000\n0000\n0000\n0100\n0000\n0000\n0010\n0000\n0000\n0001\n0000\n0000\n,\n2200\n0000\n0000\n3000\n0300\n0000\n0000\n0000\n0000\n0000\n0000\n0000\n0000\n0000\n0000\n0000\n0000\n0000\n0000\n0000\n0000\n0000\n0000\n1000\n;\n#NOTEDATA:;\n#STEPSTYPE:dance-single;\n#DIFFICULTY:Hard;\n#NOTES:\n1002\n0000\n0003\n0000\n,\n4000\n0000\n3000\nM0M0\n;\n",
    ));
    v.push(mk(
        "f05",
        "#OFFSET:0;\n#BPMS:0=120;\n#STOPS:;\n#NOTEDATA:;\n#STEPSTYPE:dance-single;\n#DIFFICULTY:Easy;\n#NOTES:\n1000\n0000\n0000\n0000\n,\n0000\n0000\n0000\n2000\n,\n0000\n0000\n0000\n3000\n;\n",
    ));
    v
}

#[cfg(feature = "identity")]
fn make_ogg() -> Vec<u8> {
    use ddr_chart_tools::model::AudioBuffer;
    let mut rng = Rng(0xD1B5_4A32_D192_ED03);
    let frames = 44_100 / 2;
    let mut samples = Vec::with_capacity(frames * 2);
    for _ in 0..frames {
        let s = (rng.below(2000) as i16) - 1000;
        samples.push(s);
        samples.push(s);
    }
    let audio = AudioBuffer {
        samples,
        sample_rate: 44_100,
        channels: 2,
    };
    let mut out = Vec::new();
    ddr_chart_tools::ogg::encode::encode(&audio, &mut out).expect("ogg encode");
    out
}

fn write_our_ssq(song: &GenSong) -> Result<writer::SsqBundle, String> {
    let parsed = simfile::parse(&format!("x.{}", song.ext), &song.text);
    if !parsed.rejected.is_empty() {
        return Err(format!("parser rejected charts: {:?}", parsed.rejected));
    }
    writer::write_song(&parsed, &WriteOptions::default()).map_err(|e| e.to_string())
}

fn first_diff(a: &[u8], b: &[u8]) -> String {
    match a.iter().zip(b.iter()).position(|(x, y)| x != y) {
        Some(i) => format!(
            "first diff at byte {i} (ours 0x{:02X} vs cli 0x{:02X}); lens {} vs {}",
            a[i],
            b[i],
            a.len(),
            b.len()
        ),
        None => format!("lens {} vs {}", a.len(), b.len()),
    }
}

fn leg_identity(rep: &mut Report, corpus: &[GenSong], scratch: &Path) {
    let Some(bin) = std::env::var_os("SM_TOOL_BIN").filter(|v| !v.is_empty()) else {
        rep.skip("byte identity vs ddr-chart-tools CLI", "no built sibling binary");
        return;
    };
    #[cfg(not(feature = "identity"))]
    {
        let _ = (bin, corpus, scratch);
        rep.skip("byte identity vs ddr-chart-tools CLI", "harness built without the sibling");
    }
    #[cfg(feature = "identity")]
    {
        let bin = PathBuf::from(bin);
        let ogg = make_ogg();
        let in_dir = scratch.join("in");
        let out_dir = scratch.join("out");
        std::fs::create_dir_all(&in_dir).expect("mkdir");
        std::fs::create_dir_all(&out_dir).expect("mkdir");
        let mut all_ok = true;
        let mut detail = String::new();
        let mut compared = 0usize;
        for song in corpus {
            let chart = in_dir.join(format!("{}.{}", song.stem, song.ext));
            let audio = in_dir.join(format!("{}.ogg", song.stem));
            std::fs::write(&chart, &song.text).expect("write ssc");
            std::fs::write(&audio, &ogg).expect("write ogg");
            let output = Command::new(&bin)
                .args(["--from-format", "SM5", "--to-format", "DDR", "--overwrite", "-q"])
                .arg("--chartfile")
                .arg(&chart)
                .arg("--audiofile")
                .arg(&audio)
                .arg("--output-dir")
                .arg(&out_dir)
                .output()
                .expect("run cli");
            if !output.status.success() {
                all_ok = false;
                detail.push_str(&format!(
                    "{}: cli failed: {}\n",
                    song.stem,
                    String::from_utf8_lossy(&output.stderr)
                ));
                continue;
            }
            let cli = std::fs::read(out_dir.join(format!("{}.ssq", song.stem))).expect("read cli ssq");
            match write_our_ssq(song) {
                Err(e) => {
                    all_ok = false;
                    detail.push_str(&format!("{}: ours failed: {e}\n", song.stem));
                }
                Ok(bundle) => {
                    compared += 1;
                    let ours = bundle.files.first();
                    let same = bundle.files.len() == 1
                        && ours.map(|f| f.index) == Some(0)
                        && ours.map(|f| f.bytes.as_slice()) == Some(cli.as_slice());
                    if !same {
                        all_ok = false;
                        let ob = ours.map(|f| f.bytes.clone()).unwrap_or_default();
                        detail.push_str(&format!(
                            "{}: {} files; {}\n",
                            song.stem,
                            bundle.files.len(),
                            first_diff(&ob, &cli)
                        ));
                    }
                }
            }
        }
        rep.check(
            &format!("byte identity vs ddr-chart-tools CLI ({compared} songs, lead_in=0, bias=0)"),
            all_ok && compared == corpus.len(),
            detail,
        );
    }
}

fn leg_corpus_selfcheck(rep: &mut Report, corpus: &[GenSong]) {
    let mut ok = true;
    let mut detail = String::new();
    let mut files = 0usize;
    for song in corpus {
        match write_our_ssq(song) {
            Err(e) => {
                ok = false;
                detail.push_str(&format!("{}: {e}\n", song.stem));
            }
            Ok(b) => {
                for f in &b.files {
                    files += 1;
                    let slots: Vec<u8> = f.charts.iter().map(|c| c.0).collect();
                    if let Err(fails) = check_ssq(&f.bytes, &slots) {
                        ok = false;
                        detail.push_str(&format!("{}: {:?}\n", song.stem, fails));
                    }
                }
            }
        }
    }
    rep.check(
        &format!("self-check passes on every corpus file ({files} files)"),
        ok,
        detail,
    );
}

/// The game's quantisation row selector (`docs/chart_strip_hud_research.md` §4).
fn colour_class(tick: i32) -> (u32, u32) {
    let t = (tick as u32) & 0x3FF;
    let q = (t >> 6) * 3 + (((t & 0x3F) * 3 + 0x20) >> 6);
    let class = if q % 0x30 == 0 {
        1
    } else if q % 0x18 == 0 {
        3
    } else if q % 0x0C == 0 {
        2
    } else {
        4
    };
    (q % 0x30, class)
}

fn leg_quad_split(rep: &mut Report) {
    let mut ok = true;
    let mut detail = String::new();
    let mut checked = 0usize;
    for &q in &QUANTS {
        let mut notes = Vec::new();
        let mut ticks = Vec::new();
        for row in 0..q as i64 {
            let beat = r(row * 4, i64::from(q));
            notes.push(tap(beat, 0x0F));
            ticks.push(Beat::from_rational(beat).to_measure_ticks().expect("tick"));
        }
        let c = chart(Style::Single, Difficulty::Expert, notes);
        let p = prepare_chart(&c, &timing(120), &WriteOptions::default()).expect("prepare");
        for t in ticks {
            checked += 1;
            let has_lr = p.steps.contains(&(t, 0x09));
            let has_du = p.steps.contains(&(t + 1, 0x06));
            let (qa, ca) = colour_class(t);
            let (qb, cb) = colour_class(t + 1);
            if !(has_lr && has_du && ca == cb && qa == qb) {
                ok = false;
                detail.push_str(&format!(
                    "1/{q} tick {t}: lr={has_lr} du={has_du} class {ca}/{cb} q {qa}/{qb}\n"
                ));
            }
        }
        if p.steps.iter().any(|(_, b)| matches!(b, 0x0F | 0xF0 | 0xFF)) {
            ok = false;
            detail.push_str(&format!("1/{q}: shock byte leaked\n"));
        }
    }
    rep.check(
        &format!("quad split: L+R at t, D+U at t+1, same colour class ({checked} rows over 4th..192nd)"),
        ok,
        detail,
    );

    // Double: both sides split independently; a real shock is untouched;
    // ReduceToJump keeps only L+R.
    let notes = vec![
        tap(r(0, 1), 0xFF),
        tap(r(1, 1), 0x1F),
        Note {
            beat: Beat::from_integer(2),
            kind: NoteKind::Shock { side: ShockSide::P2Only },
            panels: PanelSet::empty(),
        },
    ];
    let c = chart(Style::Double, Difficulty::Expert, notes);
    let p = prepare_chart(&c, &timing(120), &WriteOptions::default()).expect("prepare");
    rep.check(
        "quad split on doubles is per side and leaves shocks alone",
        p.steps == vec![(0, 0x99), (1, 0x66), (1024, 0x19), (1025, 0x06), (2048, 0xF0)],
        format!("{:?}", p.steps),
    );
    let opts = WriteOptions {
        quad_policy: QuadPolicy::ReduceToJump,
        ..WriteOptions::default()
    };
    let p = prepare_chart(&c, &timing(120), &opts).expect("prepare");
    rep.check(
        "ReduceToJump keeps L+R only",
        p.steps == vec![(0, 0x99), (1024, 0x19), (2048, 0xF0)],
        format!("{:?}", p.steps),
    );
}

fn good_file() -> Vec<u8> {
    let notes = vec![
        tap(r(0, 1), 0x01),
        Note {
            beat: Beat::from_integer(2),
            kind: NoteKind::HoldHead {
                length: Beat::from_integer(2),
            },
            panels: PanelSet::from_bits(Style::Single, 0x02),
        },
        tap(r(5, 1), 0x0C),
    ];
    let c = chart(Style::Single, Difficulty::Basic, notes);
    let p = prepare_chart(&c, &timing(120), &WriteOptions::default()).expect("prepare");
    build_file(&timing(120), &[p], &WriteOptions::default()).expect("build")
}

fn chunk_offset(bytes: &[u8], kind: u16) -> usize {
    let mut off = 0;
    loop {
        let len = u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap()) as usize;
        let k = u16::from_le_bytes(bytes[off + 4..off + 6].try_into().unwrap());
        if k == kind || len == 0 {
            return off;
        }
        off += len;
    }
}

fn leg_selfcheck_negatives(rep: &mut Report) {
    let good = good_file();
    rep.check("self-check: well-formed file passes", check_ssq(&good, &[1]).is_ok(), String::new());

    // Mines-only (bypass the writer's own rule by editing the prepared chart).
    let c = chart(Style::Single, Difficulty::Basic, vec![tap(r(0, 1), 0x01)]);
    let mut p = prepare_chart(&c, &timing(120), &WriteOptions::default()).expect("prepare");
    p.steps.clear();
    p.freezes.clear();
    p.mines = vec![(0, 0x05), (1024, 0x02)];
    let bytes = build_file(&timing(120), &[p.clone()], &WriteOptions::default()).expect("build");
    let res = check_ssq(&bytes, &[1]);
    rep.check(
        "self-check: mines-only slot fails NoSteps",
        res == Err(vec![CheckFailure::NoSteps { slot: 1 }]),
        format!("{res:?}"),
    );

    // Holds-only.
    p.steps = vec![(0, 0x01), (1024, 0x00), (2048, 0x03), (3072, 0x00)];
    p.freezes = vec![(0x01, 0x01), (0x03, 0x01)];
    p.mines.clear();
    let bytes = build_file(&timing(120), &[p.clone()], &WriteOptions::default()).expect("build");
    let res = check_ssq(&bytes, &[1]);
    rep.check(
        "self-check: holds-only slot fails NoSteps",
        res == Err(vec![CheckFailure::NoSteps { slot: 1 }]),
        format!("{res:?}"),
    );

    // Zero-Δms tempo.
    let mut bytes = good.clone();
    let n = u16::from_le_bytes([bytes[8], bytes[9]]) as usize;
    let s0 = bytes[12 + 4 * n..12 + 4 * n + 4].to_vec();
    bytes[12 + 4 * n + 4..12 + 4 * n + 8].copy_from_slice(&s0);
    let res = check_ssq(&bytes, &[1]);
    rep.check(
        "self-check: zero-Δms tempo segment fails",
        matches!(&res, Err(f) if f.contains(&CheckFailure::TempoZeroDeltaMs { index: 1 })),
        format!("{res:?}"),
    );

    // END before the last note.
    let mut bytes = good.clone();
    let ev = chunk_offset(&bytes, 2);
    bytes[ev + 12 + 20..ev + 12 + 24].copy_from_slice(&4096i32.to_le_bytes());
    let res = check_ssq(&bytes, &[1]);
    rep.check(
        "self-check: END before last note fails",
        matches!(&res, Err(f) if f.iter().any(|x| matches!(x, CheckFailure::EndBeforeLastNote { slot: 1, .. }))),
        format!("{res:?}"),
    );

    // Shock-byte leak on a single chart.
    let mut bytes = good.clone();
    let st = chunk_offset(&bytes, 3);
    let n = u16::from_le_bytes([bytes[st + 8], bytes[st + 9]]) as usize;
    bytes[st + 12 + 4 * n] = 0x0F;
    let res = check_ssq(&bytes, &[1]);
    rep.check(
        "self-check: shock byte on a note row fails",
        matches!(&res, Err(f) if f.iter().any(|x| matches!(x, CheckFailure::ShockByteOnNoteRow { slot: 1, tick: 0, byte: 0x0F }))),
        format!("{res:?}"),
    );

    // Truncated file.
    let res = check_ssq(&good[..good.len() - 4], &[1]);
    rep.check(
        "self-check: truncated chunk list is not walkable",
        matches!(&res, Err(f) if f.iter().any(|x| matches!(x, CheckFailure::ChunkListNotWalkable { .. }))),
        format!("{res:?}"),
    );
}

fn tempo_entries(bytes: &[u8]) -> Vec<(i32, i32)> {
    TempoConverter::from_ssq(bytes).expect("tempo").entries().collect()
}

fn leg_delay_nudge(rep: &mut Report) {
    let base = "#OFFSET:0;\n#BPMS:0=120;\n#NOTEDATA:;\n#STEPSTYPE:dance-single;\n#DIFFICULTY:Hard;\n#NOTES:\n1000\n0000\n0000\n0000\n,\n0100\n0000\n0000\n0000\n,\n0010\n0000\n0000\n0000\n;\n";
    let delayed = format!("#DELAYS:4=0.5;\n{base}");
    let stopped = format!("#STOPS:4=0.5;\n{base}");
    let d = writer::write_song(&simfile::parse_ssc(&delayed), &WriteOptions::default()).expect("write");
    let s = writer::write_song(&simfile::parse_ssc(&stopped), &WriteOptions::default()).expect("write");
    let de = tempo_entries(&d.files[0].bytes);
    let se = tempo_entries(&s.files[0].bytes);
    rep.check(
        "delay nudge: #DELAYS becomes a stop at tick(beat) − 1",
        de == vec![(0, 0), (4095, 2000), (4095, 2500), (16384, 8500)]
            && se == vec![(0, 0), (4096, 2000), (4096, 2500), (16384, 8500)],
        format!("delay {de:?} stop {se:?}"),
    );
    let conv = TempoConverter::from_ssq(&d.files[0].bytes).expect("tempo");
    rep.check(
        "delay nudge: the beat's notes are judged after the pause",
        conv.beat_to_music_count(4096) == 2500 && conv.beat_to_music_count(4094) <= 2000,
        format!("{} {}", conv.beat_to_music_count(4096), conv.beat_to_music_count(4094)),
    );
    rep.check(
        "delay nudge: output passes the self-check",
        check_ssq(&d.files[0].bytes, &[3]).is_ok(),
        String::new(),
    );
    let zero = format!("#DELAYS:0=0.25;\n{base}");
    let z = writer::write_song(&simfile::parse_ssc(&zero), &WriteOptions::default()).expect("write");
    rep.check(
        "delay nudge: a delay at beat 0 folds into the offset",
        tempo_entries(&z.files[0].bytes)[0] == (0, 250),
        format!("{:?}", tempo_entries(&z.files[0].bytes)),
    );
}

fn leg_shift_seconds(rep: &mut Report, corpus: &[GenSong]) {
    let mut ok = true;
    let mut detail = String::new();
    let mut n = 0usize;
    for song in corpus.iter().take(12) {
        let Ok(b) = write_our_ssq(song) else { continue };
        for f in &b.files {
            n += 1;
            let Some(sh) = writer::shift_seconds(&f.bytes, 37) else {
                ok = false;
                detail.push_str(&format!("{}: shift failed\n", song.stem));
                continue;
            };
            let a = tempo_entries(&f.bytes);
            let z = tempo_entries(&sh);
            let moved = a.len() == z.len()
                && a.iter().zip(&z).all(|(x, y)| x.0 == y.0 && y.1 - x.1 == 37);
            // Everything after the tempo chunk is untouched.
            let tl = u32::from_le_bytes(f.bytes[0..4].try_into().unwrap()) as usize;
            let tail_same = f.bytes[tl..] == sh[tl..];
            let back = writer::shift_seconds(&sh, -37) == Some(f.bytes.clone());
            if !(moved && tail_same && back && sh.len() == f.bytes.len()) {
                ok = false;
                detail.push_str(&format!("{}: moved={moved} tail={tail_same} back={back}\n", song.stem));
            }
        }
    }
    rep.check(
        &format!("shift_seconds round trip (+37/−37) on {n} files"),
        ok && n > 0,
        detail,
    );
    rep.check(
        "shift_seconds rejects a file without a tempo chunk",
        writer::shift_seconds(&[0u8; 16], 5).is_none() && writer::shift_seconds(&[], 5).is_none(),
        String::new(),
    );
}

fn leg_timing_split(rep: &mut Report) {
    let text = "\
#OFFSET:0;
#BPMS:0=120;
#NOTEDATA:;
#STEPSTYPE:dance-single;
#DIFFICULTY:Easy;
#METER:3;
#NOTES:
1000
0000
0000
0000
;
#NOTEDATA:;
#STEPSTYPE:dance-double;
#DIFFICULTY:Easy;
#METER:4;
#NOTES:
10000000
00000000
00000000
00000000
;
#NOTEDATA:;
#STEPSTYPE:dance-single;
#DIFFICULTY:Hard;
#METER:12;
#BPMS:0=200;
#NOTES:
0100
0000
0000
0000
;
#NOTEDATA:;
#STEPSTYPE:dance-double;
#DIFFICULTY:Hard;
#METER:13;
#BPMS:0=200;
#NOTES:
01000000
00000000
00000000
00000000
;
#NOTEDATA:;
#STEPSTYPE:dance-single;
#DIFFICULTY:Challenge;
#METER:16;
#BPMS:0=240;
#NOTES:
0010
0000
0000
0000
;
#NOTEDATA:;
#STEPSTYPE:dance-double;
#DIFFICULTY:Challenge;
#METER:17;
#BPMS:0=180;
#NOTES:
00100000
00000000
00000000
00000000
;
";
    let song = simfile::parse_ssc(text);
    let b = writer::write_song(&song, &WriteOptions::default()).expect("write");
    let idx: Vec<u8> = b.files.iter().map(|f| f.index).collect();
    let charts: Vec<Vec<(u8, u8)>> = b.files.iter().map(|f| f.charts.clone()).collect();
    rep.check(
        "timing split: per-level files, conflicting double hidden",
        idx == vec![2, 4, 5]
            && charts == vec![vec![(1, 3), (6, 4)], vec![(3, 12), (8, 13)], vec![(4, 16)]]
            && b.slots == [0, 3, 0, 12, 16, 0, 4, 0, 13, 0]
            && b.rejects.iter().any(|(l, r)| l == "dance-double Challenge" && *r == writer::Reason::TimingConflict),
        format!("idx {idx:?} charts {charts:?} slots {:?} rejects {:?}", b.slots, b.rejects),
    );
    let bpm_of = |bytes: &[u8]| {
        let c = TempoConverter::from_ssq(bytes).expect("tempo");
        // 4 beats → ms at that BPM.
        240_000 / c.beat_to_music_count(4096).max(1)
    };
    let bpms: Vec<i32> = b.files.iter().map(|f| bpm_of(&f.bytes)).collect();
    rep.check(
        "timing split: each file carries its own tempo chunk",
        bpms == vec![120, 200, 240],
        format!("{bpms:?}"),
    );
    let mut all_ok = true;
    for f in &b.files {
        let slots: Vec<u8> = f.charts.iter().map(|c| c.0).collect();
        if check_ssq(&f.bytes, &slots).is_err() {
            all_ok = false;
        }
    }
    rep.check("timing split: every file passes the self-check", all_ok, String::new());

    // Identical overrides fold back into one file.
    let same = text.replace("#BPMS:0=200;", "#BPMS:0=120;").replace("#BPMS:0=240;", "#BPMS:0=120;").replace("#BPMS:0=180;", "#BPMS:0=120;");
    let b = writer::write_song(&simfile::parse_ssc(&same), &WriteOptions::default()).expect("write");
    rep.check(
        "timing split: overrides equal to the song timing stay one file",
        b.files.len() == 1 && b.files[0].index == 0 && b.files[0].charts.len() == 6,
        format!("{:?}", b.files.iter().map(|f| (f.index, f.charts.clone())).collect::<Vec<_>>()),
    );
}

fn leg_lead_in(rep: &mut Report, corpus: &[GenSong]) {
    let mut ok = true;
    let mut detail = String::new();
    for song in corpus.iter().take(8) {
        let parsed = simfile::parse(&format!("x.{}", song.ext), &song.text);
        let a = writer::write_song(&parsed, &WriteOptions::default()).expect("write");
        let opts = WriteOptions {
            lead_in_ms: 1500,
            sync_bias_ms: -12,
            ..WriteOptions::default()
        };
        let b = writer::write_song(&parsed, &opts).expect("write");
        let (fa, fb) = (&a.files[0].bytes, &b.files[0].bytes);
        let ea = tempo_entries(fa);
        let eb = tempo_entries(fb);
        let shifted = ea.len() == eb.len()
            && ea.iter().zip(&eb).all(|(x, y)| x.0 == y.0 && y.1 - x.1 == 1488);
        let tl = u32::from_le_bytes(fa[0..4].try_into().unwrap()) as usize;
        let rest_same = fa[tl..] == fb[tl..];
        let same_as_shift = writer::shift_seconds(fa, 1488) == Some(fb.clone());
        if !(shifted && rest_same && same_as_shift) {
            ok = false;
            detail.push_str(&format!("{}: shifted={shifted} rest={rest_same} eq_shift={same_as_shift}\n", song.stem));
        }
    }
    rep.check(
        "lead-in + sync bias shift every anchor by lead_in+bias and nothing else (== shift_seconds)",
        ok,
        detail,
    );
}

// ---------------------------------------------------------------------
// Step 5: audio + bank legs
// ---------------------------------------------------------------------

fn build_wav(rate: u32, channels: u16, samples: &[i16]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut v = Vec::new();
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data_len).to_le_bytes());
    v.extend_from_slice(b"WAVE");
    v.extend_from_slice(b"fmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&channels.to_le_bytes());
    v.extend_from_slice(&rate.to_le_bytes());
    v.extend_from_slice(&(rate * u32::from(channels) * 2).to_le_bytes());
    v.extend_from_slice(&(channels * 2).to_le_bytes());
    v.extend_from_slice(&16u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        v.extend_from_slice(&s.to_le_bytes());
    }
    v
}

/// Deterministic test signal: two detuned sines plus a little noise, so
/// ADPCM has something to chew on and seeks are unambiguous.
fn signal(frames: usize, channels: usize, seed: u64) -> Vec<i16> {
    let mut rng = Rng(seed);
    let mut v = Vec::with_capacity(frames * channels);
    for i in 0..frames {
        let t = i as f64 / 44_100.0;
        for c in 0..channels {
            let f = 220.0 * (c as f64 + 1.0) + 0.5 * i as f64 / frames as f64;
            let s = 9000.0 * (2.0 * std::f64::consts::PI * f * t).sin()
                + 3000.0 * (2.0 * std::f64::consts::PI * 3.3 * f * t).sin();
            let n = (rng.below(400) as f64) - 200.0;
            v.push((s + n).clamp(-32768.0, 32767.0) as i16);
        }
    }
    v
}

fn blocks_of(pcm: &[i16]) -> Vec<StereoBlock> {
    pcm.chunks(256)
        .map(|c| {
            let mut b = [0i16; 256];
            b[..c.len()].copy_from_slice(c);
            b
        })
        .collect()
}

fn leg_xsb(rep: &mut Report) {
    let codes = ["zab1", "zab12", "zab12c"];
    let mut ok = true;
    let mut detail = String::new();
    for code in codes {
        let ours = xsb::write_song_profile(code).expect("xsb");
        let tool_order =
            xsb::write_song_profile_indexed(code, xsb::WaveIndices::TOOL).expect("xsb");
        let diff: Vec<usize> = (0..ours.len()).filter(|&i| ours[i] != tool_order[i]).collect();
        // CRC (8, 9), complex track wave index (0xCA + 31), simple wave index (0xCA + 39 + 9).
        let expected = vec![8, 9, 0xCA + 31, 0xCA + 39 + 9];
        if diff != expected || ours[0xCA + 31] != 1 || ours[0xCA + 48] != 0 {
            ok = false;
            detail.push_str(&format!("{code}: diff {diff:?}\n"));
        }
        if ours.len() != 318 + 2 * code.len() {
            ok = false;
            detail.push_str(&format!("{code}: len {}\n", ours.len()));
        }
    }
    rep.check(
        "XSB: stock order (main=0, preview=1) differs from tool order in the two wave indices + CRC only",
        ok,
        detail,
    );
    #[cfg(feature = "identity")]
    {
        let mut ok = true;
        let mut detail = String::new();
        for code in codes {
            let mut tool = Vec::new();
            ddr_chart_tools::xsb::write(code, &mut tool).expect("tool xsb");
            let ours =
                xsb::write_song_profile_indexed(code, xsb::WaveIndices::TOOL).expect("xsb");
            if ours != tool {
                ok = false;
                detail.push_str(&format!("{code}: {}\n", first_diff(&ours, &tool)));
            }
        }
        rep.check(
            "XSB golden: write_song_profile_indexed(code, TOOL) == ddr-chart-tools xsb::write(code) for 4/5/6-char codes",
            ok,
            detail,
        );
    }
    #[cfg(not(feature = "identity"))]
    rep.skip("XSB golden vs ddr-chart-tools", "harness built without the sibling");
}

#[cfg(feature = "identity")]
fn tool_bank(
    code: &str,
    fmt: ddr_chart_tools::xwb::WaveFormat,
    entries: &[(&str, &[u8])],
) -> ddr_chart_tools::xwb::XwbBank {
    use ddr_chart_tools::xwb::{XwbBank, XwbEntry};
    let spb = fmt.samples_per_block() as usize;
    let ba = fmt.block_align() as usize;
    let mut name = [0u8; 64];
    name[..code.len()].copy_from_slice(code.as_bytes());
    let entries = entries
        .iter()
        .map(|(n, data)| {
            let duration = data.len() / ba * spb;
            let mut name_bytes = vec![0u8; 64];
            name_bytes[..n.len()].copy_from_slice(n.as_bytes());
            XwbEntry {
                flags_and_duration: (duration as u32) << 4,
                format: fmt,
                data: data.to_vec(),
                loop_start: 0,
                loop_length: duration.saturating_sub(1) as u32,
                name_bytes,
            }
        })
        .collect();
    XwbBank {
        header_version: 42,
        flags: 0x0009_0001,
        name,
        entry_name_element_size: 64,
        alignment: 2048,
        compact_format: 0,
        build_time: 0,
        entries,
    }
}

fn leg_xwb(rep: &mut Report) {
    let code = "zab12c";
    let main_pcm = signal(2 * 44_100, 2, 0x1234);
    let preview_pcm = signal(44_100, 2, 0x5678);
    let plan = SongBankPlan::new(code, 44_100, 2 * 44_100, 44_100).expect("plan");
    let ours = song_bank::write_complete(
        std::io::Cursor::new(Vec::new()),
        &plan,
        blocks_of(&main_pcm),
        blocks_of(&preview_pcm),
    )
    .expect("write")
    .into_inner();
    let bank = xwb::parse_song_bank(&ours).expect("parse");
    rep.check(
        "XWB: generated bank parses strictly, main=0 / preview=1, names, durations",
        bank.main_entry_index() == 0
            && bank.preview_entry_index() == 1
            && bank.entries[0].name() == code
            && bank.entries[1].name() == "zab12c_s"
            && bank.entries[0].duration as u64 == plan.frames(0)
            && bank.entries[1].duration as u64 == plan.frames(1)
            && ours.len() as u64 == plan.total_len(),
        format!(
            "{} {} {:?}",
            bank.main_entry_index(),
            bank.preview_entry_index(),
            bank.entries.iter().map(|e| e.name().to_string()).collect::<Vec<_>>()
        ),
    );

    #[cfg(feature = "identity")]
    {
        use ddr_chart_tools::xwb::{
            adpcm as sib_adpcm, container as sib_container, WaveFormat as SibFmt,
        };
        let fmt = SibFmt::from_packed(plan.wave_format().packed());
        // The tool pads a partial last block with zeros; so does blocks_of.
        let main_enc = sib_adpcm::encode::encode(&main_pcm, &fmt).expect("tool encode");
        let preview_enc = sib_adpcm::encode::encode(&preview_pcm, &fmt).expect("tool encode");
        rep.check(
            "XWB parity: our ADPCM entry bytes == tool encoder output for the same PCM",
            bank.entries[0].data == &main_enc[..] && bank.entries[1].data == &preview_enc[..],
            format!(
                "main {} preview {}",
                first_diff(bank.entries[0].data, &main_enc),
                first_diff(bank.entries[1].data, &preview_enc)
            ),
        );
        // Tool order (preview first): per-entry bytes equal modulo offsets.
        let tool_pf = tool_bank(code, fmt, &[("zab12c_s", &preview_enc), (code, &main_enc)]);
        let mut tool_bytes = Vec::new();
        sib_container::write(&tool_pf, &mut tool_bytes).expect("tool write");
        let parsed = sib_container::parse(&tool_bytes).expect("tool parse");
        // Metadata row without the data-offset field (bytes 8..12).
        let row = |b: &[u8], i: usize| -> Vec<u8> {
            let r = &b[52 + 96 + 24 * i..52 + 96 + 24 * (i + 1)];
            [&r[0..8], &r[12..24]].concat()
        };
        let names = |b: &[u8], i: usize| -> Vec<u8> {
            b[52 + 96 + 48 + 64 * i..52 + 96 + 48 + 64 * (i + 1)].to_vec()
        };
        rep.check(
            "XWB parity: entry rows/names/data equal the tool's preview-first bank per entry (order differs by design)",
            row(&ours, 0) == row(&tool_bytes, 1)
                && row(&ours, 1) == row(&tool_bytes, 0)
                && names(&ours, 0) == names(&tool_bytes, 1)
                && names(&ours, 1) == names(&tool_bytes, 0)
                && parsed.entries[1].data == bank.entries[0].data
                && parsed.entries[0].data == bank.entries[1].data
                && parsed.flags == bank.flags
                && parsed.header_version == bank.header_version
                && parsed.alignment == bank.alignment
                && parsed.name[..] == ours[52 + 8..52 + 72],
            String::new(),
        );
        // Main-first through the tool's container writer: whole-file identity.
        let tool_mf = tool_bank(code, fmt, &[(code, &main_enc), ("zab12c_s", &preview_enc)]);
        let mut mf_bytes = Vec::new();
        sib_container::write(&tool_mf, &mut mf_bytes).expect("tool write");
        rep.check(
            "XWB parity: whole file == tool container writer fed main-first",
            ours == mf_bytes,
            first_diff(&ours, &mf_bytes),
        );
        // ADPCM round trip through the tool's decoder.
        let tool_dec = sib_adpcm::decode::decode(bank.entries[0].data, &fmt).expect("tool decode");
        let our_dec = adpcm::decode_interleaved(
            bank.entries[0].data,
            bank.entries[0].format,
            bank.entries[0].duration,
        )
        .expect("our decode");
        let max_err = main_pcm
            .iter()
            .zip(&our_dec)
            .map(|(a, b)| (i32::from(*a) - i32::from(*b)).abs())
            .max()
            .unwrap_or(0);
        rep.check(
            "ADPCM round trip: tool decoder == our decoder on our blocks, close to the source",
            tool_dec == our_dec && max_err < 2048,
            format!("max abs error {max_err}, lens {} {}", tool_dec.len(), our_dec.len()),
        );
    }
    #[cfg(not(feature = "identity"))]
    rep.skip("XWB parity vs ddr-chart-tools", "harness built without the sibling");

    // Progressive contract.
    let mut w = SongBankWriter::new(std::io::Cursor::new(Vec::new()), &plan).expect("writer");
    let mut ok = w.written_watermark() == plan.wave_data_offset();
    let mut enc = Vec::new();
    for (k, b) in blocks_of(&main_pcm).iter().enumerate().take(50) {
        enc.clear();
        adpcm::encode_block(b, plan.wave_format(), &mut enc).expect("encode");
        w.append_encoded(&enc).expect("append");
        ok &= w.written_watermark() == plan.block_offset(0, k as u64 + 1).unwrap();
    }
    rep.check(
        "progressive contract: watermark after k blocks == block_offset(0, k)",
        ok,
        String::new(),
    );

    // 2 min @ 44.1 kHz + 15 s preview of silence (the spec's acceptance case).
    let plan = SongBankPlan::new(code, 44_100, 120 * 44_100, 15 * 44_100).expect("plan");
    let silence = std::iter::repeat_n([0i16; 256], plan.blocks(0) as usize);
    let preview = std::iter::repeat_n([0i16; 256], plan.blocks(1) as usize);
    let big = song_bank::write_complete(std::io::Cursor::new(Vec::new()), &plan, silence, preview)
        .expect("write")
        .into_inner();
    let parsed = xwb::parse_song_bank(&big);
    rep.check(
        "XWB: 2 min + 15 s silence bank parses strictly (song_rate would bind it)",
        matches!(&parsed, Ok(b) if b.main_entry_index() == 0 && b.preview_entry_index() == 1 && b.entries[0].duration as u64 == plan.frames(0)),
        format!("{:?} len {}", parsed.as_ref().err(), big.len()),
    );
}

fn frames_of(pcm: &[i16]) -> u64 {
    (pcm.len() / 2) as u64
}

fn leg_audio(rep: &mut Report, scratch: &Path) {
    // WAV.
    let pcm = signal(44_100 + 777, 2, 1);
    let wav = build_wav(44_100, 2, &pcm);
    let info = probe::probe(&wav).expect("probe");
    let dec = decode_whole(&wav, &info).expect("decode");
    rep.check(
        "audio probe: WAV frames_exact == decoded frames",
        info.codec == Codec::Wav && info.frames_exact == frames_of(&dec) && dec == pcm,
        format!("{} vs {}", info.frames_exact, frames_of(&dec)),
    );

    // Normalisation shape: mono 22.05 kHz, lead-in 1280, total N.
    let mono: Vec<i16> = signal(22_050, 1, 2);
    let wav = build_wav(22_050, 1, &mono);
    let info = probe::probe(&wav).expect("probe");
    let target = TargetFormat::for_source(&info, 1280);
    let mut d = Decoder::open(&wav, &info, target).expect("open");
    let flat: Vec<i16> = d.by_ref().flat_map(|b| b.into_iter()).collect();
    rep.check(
        "audio normalise: mono 22.05 kHz → stereo 44.1 kHz, exactly total frames, first 1280 silent",
        d.failed().is_none()
            && flat.len() as u64 == target.total_frames * 2
            && flat[..2560].iter().all(|&s| s == 0)
            && flat[2560] == mono[0]
            && flat[2561] == mono[0]
            && info.target_frames() == 44_100,
        format!("{:?} len {} target {:?}", d.failed(), flat.len(), target),
    );

    // Vorbis via the sibling's encoder (lewton on the other side).
    #[cfg(feature = "identity")]
    {
        use ddr_chart_tools::model::AudioBuffer;
        let pcm = signal(5 * 44_100 + 333, 2, 3);
        let audio = AudioBuffer {
            samples: pcm.clone(),
            sample_rate: 44_100,
            channels: 2,
        };
        let mut ogg = Vec::new();
        ddr_chart_tools::ogg::encode::encode(&audio, &mut ogg).expect("ogg encode");
        let info = probe::probe(&ogg).expect("probe");
        let ours = decode_whole(&ogg, &info).expect("decode");
        let lewton = ddr_chart_tools::ogg::decode::decode(&ogg).expect("lewton");
        let same_len = frames_of(&ours) == frames_of(&lewton.samples);
        let max_diff = ours
            .iter()
            .zip(&lewton.samples)
            .map(|(a, b)| (i32::from(*a) - i32::from(*b)).abs())
            .max()
            .unwrap_or(i32::MAX);
        rep.check(
            "audio probe: Vorbis frames_exact == decoded frames (sibling ogg::encode fixture)",
            info.codec == Codec::Vorbis
                && info.frames_exact == frames_of(&ours)
                && info.rate == 44_100
                && info.channels == 2,
            format!("{} vs {}", info.frames_exact, frames_of(&ours)),
        );
        rep.check(
            "audio decode: symphonia Vorbis within ±1 LSB of lewton, same length",
            same_len && max_diff <= 1,
            format!(
                "same_len={same_len} max_diff={max_diff} ({} vs {})",
                frames_of(&ours),
                frames_of(&lewton.samples)
            ),
        );
        // Seek: preview window == whole-decode slice (outside the fades).
        let w = PreviewWindow::for_audio(Some(1.234), Some(5.0), &info);
        let mut pd = PreviewDecoder::open(&ogg, &info, &w).expect("preview");
        let got: Vec<i16> = pd.by_ref().flat_map(|b| b.into_iter()).collect();
        let (fi, fo) = ((0.25 * 44_100.0) as usize, (1.5 * 44_100.0) as usize);
        let s = w.start_frame as usize;
        let len = w.len_frames as usize;
        let exact = (fi..len - fo)
            .all(|i| got[i * 2] == ours[(s + i) * 2] && got[i * 2 + 1] == ours[(s + i) * 2 + 1]);
        rep.check(
            "audio seek: Vorbis preview window is sample-exact against the whole decode",
            pd.failed().is_none() && exact && got[0] == 0 && w.len_frames % 128 == 0,
            format!("{:?} start {} len {}", pd.failed(), s, len),
        );
    }
    #[cfg(not(feature = "identity"))]
    rep.skip("audio probe/decode: Vorbis fixture", "harness built without the sibling");

    // MP3 via lame.
    let lame = std::env::var_os("SM_LAME").filter(|v| !v.is_empty());
    match lame {
        None => rep.skip("audio probe/decode: MP3 fixtures", "lame not on PATH"),
        Some(lame) => {
            let pcm = signal(6 * 44_100 + 100, 2, 4);
            let wav_path = scratch.join("fx.wav");
            std::fs::write(&wav_path, build_wav(44_100, 2, &pcm)).expect("write wav");
            let run = |args: &[&str], out: &str| -> Option<Vec<u8>> {
                let out_path = scratch.join(out);
                let st = Command::new(&lame)
                    .arg("--quiet")
                    .args(args)
                    .arg(&wav_path)
                    .arg(&out_path)
                    .status()
                    .ok()?;
                st.success().then(|| std::fs::read(&out_path).ok()).flatten()
            };
            let cbr = run(&["-b", "128", "--add-id3v2", "--tt", "Fixture", "--ta", "harness"], "cbr.mp3");
            let vbr = run(&["-V", "4"], "vbr.mp3");
            let mono = run(&["-b", "64", "--resample", "22.05", "-m", "m"], "mono22.mp3");
            match (cbr, vbr, mono) {
                (Some(cbr), Some(vbr), Some(mono)) => {
                    let ic = probe::probe(&cbr).expect("probe cbr");
                    let dc = decode_whole(&cbr, &ic).expect("decode cbr");
                    rep.check(
                        "audio probe: CBR MP3 (Info tag, ID3v2) frames_exact == decoded, one frame of StepMania silence first",
                        ic.codec == Codec::Mp3
                            && ic.leading_silence_frames == 1152
                            && ic.frames_exact == frames_of(&dc)
                            && dc[..1152 * 2].iter().all(|&s| s == 0)
                            && dc[1152 * 2..].iter().any(|&s| s != 0),
                        format!("{ic:?} decoded {}", frames_of(&dc)),
                    );
                    let iv = probe::probe(&vbr).expect("probe vbr");
                    let dv = decode_whole(&vbr, &iv).expect("decode vbr");
                    rep.check(
                        "audio probe: VBR MP3 (Xing tag) frames_exact == decoded, no leading silence",
                        iv.codec == Codec::Mp3
                            && iv.leading_silence_frames == 0
                            && iv.frames_exact == frames_of(&dv),
                        format!("{iv:?} decoded {}", frames_of(&dv)),
                    );
                    let im = probe::probe(&mono).expect("probe mono");
                    let dm = decode_whole(&mono, &im).expect("decode mono");
                    rep.check(
                        "audio normalise: mono 22.05 kHz MP3 → stereo 44.1 kHz, target_frames == decoded",
                        im.rate == 22_050
                            && im.channels == 1
                            && im.target_rate() == 44_100
                            && im.target_frames() == frames_of(&dm)
                            && dm.chunks_exact(2).all(|p| p[0] == p[1]),
                        format!("{im:?} decoded {}", frames_of(&dm)),
                    );
                    // Seek exactness on the CBR file (bit reservoir + filterbank warm-up).
                    let w = PreviewWindow::for_audio(Some(0.9), Some(5.0), &ic);
                    let mut pd = PreviewDecoder::open(&cbr, &ic, &w).expect("preview");
                    let got: Vec<i16> = pd.by_ref().flat_map(|b| b.into_iter()).collect();
                    let (fi, fo) = ((0.25 * 44_100.0) as usize, (1.5 * 44_100.0) as usize);
                    let s = w.start_frame as usize;
                    let len = w.len_frames as usize;
                    let exact = (fi..len - fo).all(|i| {
                        got[i * 2] == dc[(s + i) * 2] && got[i * 2 + 1] == dc[(s + i) * 2 + 1]
                    });
                    rep.check(
                        "audio seek: MP3 preview window is sample-exact against the whole decode",
                        pd.failed().is_none() && exact,
                        format!("{:?}", pd.failed()),
                    );
                }
                _ => rep.skip(
                    "audio probe/decode: MP3 fixtures",
                    "lame failed to encode the fixtures",
                ),
            }
        }
    }

    // Lead-in arithmetic on the Step 1 song's numbers (#OFFSET −0.241,
    // 140 BPM, first note at beat 8) and the beat-0 variant.
    let big_money = choose_lead_in(-0.241, 0.241 + 8.0 * 60.0 / 140.0, 44_100);
    let beat0 = choose_lead_in(-0.241, 0.241, 44_100);
    let beat0_48 = choose_lead_in(-0.241, 0.241, 48_000);
    rep.check(
        "lead-in: Big Money needs none; a beat-0 first note gets 606 blocks (1759 ms) @44.1k, 660 blocks (1760 ms, exact) @48k",
        big_money == LeadIn::ZERO
            && beat0.frames == 606 * 128
            && beat0.ms == 1759
            && beat0_48.frames == 660 * 128
            && beat0_48.ms == 1760
            && beat0_48.exact,
        format!("{big_money:?} {beat0:?} {beat0_48:?}"),
    );
    // The writer honours the chosen ms: td0 == −offset·1000 + ms.
    let text = "#OFFSET:-0.241;\n#BPMS:0=140;\n#NOTEDATA:;\n#STEPSTYPE:dance-single;\n#DIFFICULTY:Hard;\n#NOTES:\n1000\n0000\n0000\n0000\n;\n";
    let song = simfile::parse_ssc(text);
    let opts = WriteOptions {
        lead_in_ms: beat0.ms,
        ..WriteOptions::default()
    };
    let b = writer::write_song(&song, &opts).expect("write");
    let td0 = tempo_entries(&b.files[0].bytes)[0].1;
    rep.check(
        "lead-in: SSQ tempo_data[0] == −#OFFSET·1000 + lead_in_ms (≥ 60, first note ≥ 2000 ms)",
        td0 == 241 + beat0.ms as i32 && td0 >= 2000,
        format!("td0 {td0}"),
    );
}

// ---------------------------------------------------------------------
// Leg 12: identity
// ---------------------------------------------------------------------

fn leg_identity_derivation(rep: &mut Report) {
    let k = identity::key("Stamina RPG 5", "[11] Big Money (Medium)");
    let a = identity::derive(&k, 0);
    let b = identity::derive(&identity::key("STAMINA rpg 5", "[11] BIG MONEY (medium)"), 0);
    rep.check(
        "identity: derive is deterministic, casefolded, z+5 base36, mcode in the 9M window",
        a == b
            && a.basename.len() == 6
            && a.basename.starts_with('z')
            && identity::is_well_formed_basename(&a.basename)
            && xsb::is_valid_name(&a.basename)
            && (9_000_000..10_000_000).contains(&a.mcode)
            && a.probe == 0,
        format!("{a:?} vs {b:?}"),
    );
    // The Step 5 placeholder's exact digits (the tool's test vector).
    let digits = format!("{:0>13}", identity::base36(identity::fnv1a64(k.as_bytes())));
    rep.check(
        "identity: basename is the first five of the 13-digit zero-padded base36 of fnv1a64(key)",
        a.basename == format!("z{}", &digits[..5]),
        format!("{} vs z{}", a.basename, &digits[..5]),
    );
    let keys: Vec<String> = (0..3000)
        .map(|i| identity::key(&format!("Pack {}", i % 7), &format!("Song {i}")))
        .collect();
    let mut reversed = keys.clone();
    reversed.reverse();
    let fwd = identity::assign(&keys);
    let rev = identity::assign(&reversed);
    let mut basenames: Vec<&str> = fwd.iter().map(|(_, id)| id.basename.as_str()).collect();
    basenames.sort_unstable();
    let unique_b = {
        let mut v = basenames.clone();
        v.dedup();
        v.len()
    };
    let mut mcodes: Vec<u32> = fwd.iter().map(|(_, id)| id.mcode).collect();
    mcodes.sort_unstable();
    mcodes.dedup();
    rep.check(
        "identity: assign is order independent and collision free over 3000 keys (all well formed, none reserved)",
        fwd == rev
            && fwd.len() == 3000
            && unique_b == 3000
            && mcodes.len() == 3000
            && fwd.iter().all(|(_, id)| {
                identity::is_well_formed_basename(&id.basename) && !identity::is_reserved(id)
            }),
        format!("len {} unique_b {} unique_m {}", fwd.len(), unique_b, mcodes.len()),
    );
    // Probing contract: derive(k, i) re-hashes `k#i`, differs from the plain
    // derivation and is stable; a forced collision (two keys, one of which
    // is given the other's plain identity by a reserved-set injection) is
    // exercised through `assign` by making a key whose plain mcode is
    // reserved: it must come back probed.
    let synthetic_key = "a/b".to_string();
    let p0 = identity::derive(&synthetic_key, 0);
    let p1 = identity::derive(&synthetic_key, 1);
    let p1_again = identity::derive(&synthetic_key, 1);
    rep.check(
        "identity: probe i re-hashes key#i deterministically and changes the identity",
        p0 != p1 && p1 == p1_again && p1.probe == 1 && p0.probe == 0,
        format!("{p0:?} {p1:?}"),
    );
    // Brute-force a pair of keys colliding on basename (5 base36 digits →
    // ~60M space; birthday over ~12k keys gives a collision with high
    // probability) and check `assign` resolves it by probing the LATER key.
    let mut seen: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut pair: Option<(String, String)> = None;
    for i in 0..40_000u32 {
        let k = format!("c/{i}");
        let id = identity::derive(&k, 0);
        if let Some(prev) = seen.insert(id.basename.clone(), k.clone()) {
            pair = Some((prev, k));
            break;
        }
    }
    match pair {
        Some((k1, k2)) => {
            let assigned = identity::assign(&[k1.clone(), k2.clone()]);
            let (first, second) = if k1 < k2 { (&k1, &k2) } else { (&k2, &k1) };
            let id_first = assigned.iter().find(|(k, _)| k == first).map(|(_, id)| id.clone());
            let id_second = assigned.iter().find(|(k, _)| k == second).map(|(_, id)| id.clone());
            rep.check(
                "identity: a basename collision keeps the sorted-earlier key's plain identity and probes the later one",
                assigned.len() == 2
                    && id_first.as_ref().is_some_and(|id| id.probe == 0)
                    && id_second.as_ref().is_some_and(|id| id.probe >= 1)
                    && id_first.as_ref().map(|i| &i.basename) != id_second.as_ref().map(|i| &i.basename),
                format!("{k1} {k2} {id_first:?} {id_second:?}"),
            );
        }
        None => rep.skip(
            "identity: basename collision probing",
            "no collision found in 40k keys",
        ),
    }
    rep.check(
        "identity: reserved basenames/mcodes are excluded",
        identity::RESERVED_BASENAMES.iter().all(|b| {
            identity::is_reserved(&identity::Identity {
                basename: b.to_string(),
                mcode: 9_000_000,
                probe: 0,
            })
        }) && identity::RESERVED_MCODES.iter().all(|m| {
            identity::is_reserved(&identity::Identity {
                basename: "zzzzzz".into(),
                mcode: *m,
                probe: 0,
            })
        }) && identity::RESERVED_MCODES.iter().all(|m| *m < identity::MCODE_BASE),
        String::new(),
    );
}

// ---------------------------------------------------------------------
// Leg 13: musicdb + index/lru tables
// ---------------------------------------------------------------------

fn spike_entry() -> SongEntry {
    SongEntry {
        pack: "Stamina RPG 5".into(),
        folder: "[11] Big Money (Medium)".into(),
        basename: "zab12c".into(),
        mcode: 9_000_001,
        title: "[11] [140] Big Money (Medium)".into(),
        artist: "The Quick Brown Fox".into(),
        bpm_min: 140,
        bpm_max: 140,
        levels: [0, 0, 0, 0, 11, 0, 0, 0, 0, 0],
        audio: AudioMeta {
            rel_path: "big money.ogg".into(),
            rate: 44_100,
            frames: 7_000_000,
            lead_in_frames: 0,
            lead_in_ms: 0,
            preview_start: 2_000_000,
            preview_len: 661_504,
        },
        ..SongEntry::default()
    }
}

const STOCK_DOC: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<mdb>\n  <music>\n    <mcode __type=\"u32\">38660</mcode>\n    <basename>drat</basename>\n    <title>DanRevoAthlon</title>\n    <title_yomi>danrevoathlon</title_yomi>\n    <artist>アツムワンダフル</artist>\n    <bpmmax __type=\"u16\">133</bpmmax>\n    <series __type=\"u8\">21</series>\n    <property __type=\"u32\">512</property>\n    <limited_ary __type=\"s32\" __count=\"10\">0 0 0 0 -1 -1 0 0 0 -1</limited_ary>\n    <diffLv __type=\"u8\" __count=\"10\">1 4 8 12 0 0 4 8 12 0</diffLv>\n  </music>\n</mdb>\n";

/// The opening tag's attribute string for `tag` in `doc` (`None` if absent).
fn open_tag_attrs(doc: &str, tag: &str) -> Option<String> {
    doc.lines()
        .map(str::trim_start)
        .find(|l| l.starts_with(&format!("<{tag}>")) || l.starts_with(&format!("<{tag} ")))
        .map(|l| {
            let open_end = l.find('>').unwrap_or(l.len());
            l[1 + tag.len()..open_end].trim().to_string()
        })
}

fn leg_musicdb(rep: &mut Report) {
    let xml = musicdb::entry_xml(&spike_entry(), 30, None);
    // The cabinet-proven spike entry (runs #1-#4), plus bpmmin and property
    // which the design adds.
    let expected = "  <music>\n    <mcode __type=\"u32\">9000001</mcode>\n    <basename>zab12c</basename>\n    <title>[11] [140] Big Money (Medium)</title>\n    <title_yomi>11140bigmoney'medium'</title_yomi>\n    <artist>The Quick Brown Fox</artist>\n    <bpmmax __type=\"u16\">140</bpmmax>\n    <bpmmin __type=\"u16\">140</bpmmin>\n    <series __type=\"u8\">30</series>\n    <property __type=\"u32\">0</property>\n    <limited_ary __type=\"s32\" __count=\"10\">-1 -1 -1 -1 0 -1 -1 -1 -1 -1</limited_ary>\n    <diffLv __type=\"u8\" __count=\"10\">0 0 0 0 11 0 0 0 0 0</diffLv>\n  </music>\n";
    rep.check(
        "musicdb: entry_xml reproduces the spike entry (series 30, slot 4 = 11, limited -1/0)",
        xml == expected,
        format!("\n{xml}\nvs\n{expected}"),
    );
    let same_attrs = [
        "mcode", "basename", "title", "title_yomi", "artist", "bpmmax", "series", "property",
        "limited_ary", "diffLv",
    ]
    .iter()
    .all(|t| open_tag_attrs(STOCK_DOC, t) == open_tag_attrs(&xml, t));
    let full = musicdb::entry_xml(&spike_entry(), 22, Some(10));
    rep.check(
        "musicdb: __type/__count attributes match the stock entry tag for tag; region/movie/eventno/voice/bgstage/genreflag/bemaniflag omitted; property = 1<<bit",
        same_attrs
            && ["<region", "<movie", "<eventno", "<voice", "<bgstage", "<genreflag", "<bemaniflag"]
                .iter()
                .all(|t| !xml.contains(t))
            && full.contains("<property __type=\"u32\">1024</property>")
            && full.contains("<series __type=\"u8\">22</series>"),
        format!("same_attrs={same_attrs}\n{full}"),
    );
    let mut e = spike_entry();
    e.levels = [1, 4, 8, 12, 25, 0, 4, 8, 12, 0];
    let xml = musicdb::entry_xml(&e, 30, None);
    rep.check(
        "musicdb: limited_ary is 0 where a level exists and -1 elsewhere; diffLv clamps to 19, never 255",
        xml.contains("<limited_ary __type=\"s32\" __count=\"10\">0 0 0 0 0 -1 0 0 0 -1</limited_ary>")
            && xml.contains("<diffLv __type=\"u8\" __count=\"10\">1 4 8 12 19 0 4 8 12 0</diffLv>")
            && !xml.contains("255"),
        xml.clone(),
    );
    let mut e = spike_entry();
    e.title = format!("{}<&>\"'", "予".repeat(50)); // 150 bytes of CJK, then specials
    e.artist = "Robert \"RAab\" & <friends>".into();
    let xml = musicdb::entry_xml(&e, 30, None);
    let title_line = xml.lines().find(|l| l.contains("<title>")).unwrap_or("");
    let title_text = title_line
        .trim()
        .trim_start_matches("<title>")
        .trim_end_matches("</title>");
    rep.check(
        "musicdb: title clipped to ≤127 UTF-8 bytes at a character boundary (42 × 3-byte chars = 126); & < > escaped, quotes literal like stock",
        title_text.len() == 126
            && title_text.chars().count() == 42
            && xml.contains("<artist>Robert \"RAab\" &amp; &lt;friends&gt;</artist>"),
        format!(
            "len {} chars {}\n{xml}",
            title_text.len(),
            title_text.chars().count()
        ),
    );
    let long_ascii = "x".repeat(300);
    rep.check(
        "musicdb: clip_utf8 at exactly 127 for ASCII; yomi rules (parentheticals quoted, non-ASCII → empty → tag omitted)",
        musicdb::clip_utf8(&long_ascii, 127).len() == 127
            && musicdb::clip_utf8("short", 127) == "short"
            && musicdb::title_yomi("Put Your Faith In Me (Jazzy Groove)")
                == "putyourfaithinme'jazzygroove'"
            && musicdb::title_yomi("予").is_empty()
            && !musicdb::entry_xml(
                &SongEntry {
                    title: "予".into(),
                    ..spike_entry()
                },
                30,
                None
            )
            .contains("<title_yomi>"),
        String::new(),
    );
    // Transform: insertion before the LAST </mdb>, collision drops.
    let a = spike_entry();
    let mut b = spike_entry();
    b.basename = "zbbbbb".into();
    b.mcode = 38660;
    let mut c = spike_entry();
    c.basename = "drat".into();
    c.mcode = 9_000_003;
    let mut d = spike_entry();
    d.basename = "zddddd".into();
    d.mcode = 9_000_004;
    let body = STOCK_DOC.trim_end_matches("</mdb>\n");
    let doc_prefix = format!("{body}<!-- a stray </mdb> inside a comment -->\n");
    let doc = format!("{doc_prefix}</mdb>\n");
    let report = musicdb::transform_report(doc.as_bytes(), &[a, b, c, d], 30, Some(10), None);
    let out = report
        .xml
        .clone()
        .map(|v| String::from_utf8_lossy(&v).to_string())
        .unwrap_or_default();
    rep.check(
        "musicdb: transform drops base mcode/basename collisions and inserts the survivors before the FINAL </mdb>",
        report.injected == vec![0, 3]
            && report.dropped.len() == 2
            && out.ends_with("  </music>\n</mdb>\n")
            && out.matches("<music>").count() == 3
            && out.starts_with(&doc_prefix)
            && musicdb::transform(STOCK_DOC.as_bytes(), &[], 30, None, None).is_none(),
        format!("{:?} {:?}\n{out}", report.injected, report.dropped),
    );
    let file = IndexFile {
        songs: vec![spike_entry()],
        rejects: vec![RejectEntry {
            pack: "P".into(),
            folder: "S".into(),
            fingerprint: 9,
            reason: "no playable chart".into(),
        }],
    };
    let bytes = sm_model::serialize_index(&file);
    let mut lru = LruTable::new();
    lru.insert("xwb/a.xwb", 10, 100);
    rep.check(
        "model: index.bin and lru.bin round-trip; truncation parses to None; digest is stable",
        sm_model::parse_index(&bytes) == Some(file.clone())
            && sm_model::parse_index(&bytes[..bytes.len() - 3]).is_none()
            && LruTable::parse(&lru.serialize()) == Some(lru.clone())
            && sm_model::index_digest(&file.songs) == sm_model::index_digest(&[spike_entry()]),
        String::new(),
    );
}

// ---------------------------------------------------------------------
// Leg 14: art + LRU
// ---------------------------------------------------------------------

fn leg_art(rep: &mut Report) {
    use image::{Rgba, RgbaImage};
    let jacket = RgbaImage::from_pixel(600, 600, Rgba([200, 30, 30, 255]));
    let banner = RgbaImage::from_pixel(418, 164, Rgba([30, 200, 30, 255]));
    let background = RgbaImage::from_pixel(640, 480, Rgba([30, 30, 200, 128]));
    let png = |img: &RgbaImage| jacket_art::encode_png(img).expect("png");
    let (pj, pb, pg) = (png(&jacket), png(&banner), png(&background));
    let one = |kind: SourceKind, bytes: &[u8]| {
        jacket_art::compose_square(&[ArtSource { kind, bytes }])
    };
    let sq_j = one(SourceKind::Jacket, &pj).expect("jacket");
    let sq_b = one(SourceKind::Banner, &pb).expect("banner");
    let sq_g = one(SourceKind::Background, &pg).expect("bg");
    let first_decodable = jacket_art::compose_square(&[
        ArtSource {
            kind: SourceKind::Jacket,
            bytes: b"not an image",
        },
        ArtSource {
            kind: SourceKind::Background,
            bytes: &pg,
        },
    ]);
    rep.check(
        "art: jacket/banner/background compose to opaque 512x512 squares (banner over a darker blurred backdrop, alpha flattened onto black); first DECODABLE source wins; none → None",
        sq_j.dimensions() == (512, 512)
            && sq_b.dimensions() == (512, 512)
            && sq_g.dimensions() == (512, 512)
            && sq_j.get_pixel(256, 256)[0] == 200
            && sq_b.get_pixel(256, 256)[1] == 200
            && sq_b.get_pixel(256, 10)[1] < 200
            && sq_b.get_pixel(256, 10)[3] == 255
            && sq_g.get_pixel(256, 256)[2] == 100
            && sq_g.get_pixel(256, 256)[3] == 255
            && first_decodable.is_some()
            && one(SourceKind::Jacket, b"").is_none(),
        format!(
            "j {:?} b {:?} top {:?} g {:?}",
            sq_j.get_pixel(256, 256),
            sq_b.get_pixel(256, 256),
            sq_b.get_pixel(256, 10),
            sq_g.get_pixel(256, 256)
        ),
    );
    let dds = thumbnail::thumbnail_dds(&sq_j);
    let u32_at = |off: usize| u32::from_le_bytes([dds[off], dds[off + 1], dds[off + 2], dds[off + 3]]);
    rep.check(
        "art: thumbnail DDS is 18,560 bytes: 'DDS ', size 0x7C, flags 0x00081007, 192x192, linear 18,432, mips 1, pf 0x20/FOURCC/DXT1, caps 0x1000, tail zero",
        dds.len() == 18_560
            && &dds[0..4] == b"DDS "
            && u32_at(4) == 0x7C
            && u32_at(8) == 0x0008_1007
            && u32_at(12) == 192
            && u32_at(16) == 192
            && u32_at(20) == 18_432
            && u32_at(24) == 0
            && u32_at(28) == 1
            && u32_at(76) == 0x20
            && u32_at(80) == 0x4
            && &dds[84..88] == b"DXT1"
            && u32_at(108) == 0x1000
            && dds[112..128].iter().all(|&b| b == 0)
            && thumbnail::is_thumbnail_dds(&dds),
        format!("len {} flags {:#x}", dds.len(), u32_at(8)),
    );
    let mut px = vec![0u8; 64];
    texpresso::Format::Bc1.decompress(&dds[128..136], 4, 4, &mut px);
    rep.check(
        "art: BC1 payload is standard D3D block order (block 0 decodes back to the solid red)",
        (i32::from(px[0]) - 200).abs() <= 8
            && (i32::from(px[1]) - 30).abs() <= 8
            && (i32::from(px[2]) - 30).abs() <= 8
            && px[3] == 255,
        format!("px0 {:?} block {:02x?}", &px[..4], &dds[128..136]),
    );
    let mut lru = LruTable::new();
    lru.insert("xwb/a.xwb", 40, 100);
    lru.insert("xwb/b.xwb", 40, 100);
    lru.touch("xwb/a.xwb");
    let ev1 = lru.insert("xwb/c.xwb", 40, 100);
    let ev2 = lru.insert("jacket/big_jk.arc", 500, 100);
    rep.check(
        "lru: evicts the least-recently-served OTHER entries until under budget; the new entry survives even when oversize",
        ev1 == vec!["xwb/b.xwb".to_string()]
            && ev2 == vec!["xwb/a.xwb".to_string(), "xwb/c.xwb".to_string()]
            && lru.len() == 1
            && lru.total_bytes() == 500,
        format!("{ev1:?} {ev2:?} {}", lru.total_bytes()),
    );
}

// Leg 15: scheduler queue + progressive read decision (Step 8)

fn leg_scheduler_progressive(rep: &mut Report) {
    // A fake worker: pops in order, records what ran, "completes" tickets.
    let mut q: JobQueue<u32> = JobQueue::new();
    let mut next = 0u32;
    let mut mk = || {
        next += 1;
        next
    };
    // A wheel row binds three songs (visible), the highlight lands on the
    // second, its jacket too; a speculative bank for an earlier dwell; then
    // the gameplay load asks for a different song's bank.
    q.enqueue(JobKind::Preview, "za", Urgency::VisiblePreview, false, &mut mk);
    q.enqueue(JobKind::Jacket, "za", Urgency::VisibleJacket, false, &mut mk);
    q.enqueue(JobKind::Preview, "zb", Urgency::VisiblePreview, false, &mut mk);
    q.enqueue(JobKind::Jacket, "zb", Urgency::VisibleJacket, false, &mut mk);
    q.enqueue(JobKind::Preview, "zc", Urgency::VisiblePreview, false, &mut mk);
    q.enqueue(JobKind::Jacket, "zc", Urgency::VisibleJacket, false, &mut mk);
    q.enqueue(JobKind::FullBank, "zz", Urgency::SpeculativeFullBank, true, &mut mk);
    q.rehighlight("zb");
    let (t_dup, absorbed) = q.enqueue(JobKind::Preview, "zb", Urgency::HighlightedPreview, false, &mut mk);
    q.enqueue(JobKind::FullBank, "zq", Urgency::GameplayLoad, false, &mut mk);
    let mut ran = Vec::new();
    let mut done = std::collections::HashSet::new();
    while let Some(job) = q.pop() {
        ran.push(format!("{}:{}", job.kind, job.basename));
        done.insert(job.payload);
        q.finish(job.kind, &job.basename);
    }
    rep.check(
        "scheduler: fake worker drains in priority order (gameplay bank, highlighted preview, visible previews in binding order, highlighted jacket, visible jackets, speculative bank)",
        ran == vec![
            "full-bank:zq", "preview:zb", "preview:za", "preview:zc", "jacket:zb", "jacket:za",
            "jacket:zc", "full-bank:zz",
        ],
        format!("{ran:?}"),
    );
    rep.check(
        "scheduler: a second request for a pending (kind, basename) merges and gets the first ticket",
        absorbed == Absorbed::MergedPending && t_dup == 3 && done.contains(&3) && q.running_len() == 0,
        format!("{absorbed:?} ticket {t_dup}"),
    );

    // Cancel rules.
    let mut q: JobQueue<u32> = JobQueue::new();
    q.enqueue(JobKind::FullBank, "za", Urgency::SpeculativeFullBank, true, || 1);
    q.enqueue(JobKind::FullBank, "zb", Urgency::SpeculativeFullBank, true, || 2);
    q.enqueue(JobKind::Preview, "za", Urgency::VisiblePreview, false, || 3);
    let running = q.pop().map(|j| (j.kind, j.basename.clone(), j.payload));
    let c_running = q.cancel_speculative("za");
    let (_, merged) = q.enqueue(JobKind::FullBank, "zb", Urgency::GameplayLoad, false, || 9);
    let c_committed = q.cancel_speculative("zb");
    rep.check(
        "scheduler: cancel_speculative drops a pending speculative bank, reports a running one, never touches a committed (gameplay-load) or non-bank job",
        running == Some((JobKind::Preview, "za".to_string(), 3))
            && c_running == Cancelled::Dropped(1)
            && merged == Absorbed::MergedPending
            && c_committed == Cancelled::None
            && q.contains(JobKind::FullBank, "zb")
            && q.pending_len() == 1,
        format!("{running:?} {c_running:?} {merged:?} {c_committed:?}"),
    );
    let mut q: JobQueue<u32> = JobQueue::new();
    q.enqueue(JobKind::FullBank, "zc", Urgency::SpeculativeFullBank, true, || 4);
    let _ = q.pop();
    rep.check(
        "scheduler: a running speculative bank is reported for cooperative cancel",
        q.cancel_speculative("zc") == Cancelled::Running(4),
        String::new(),
    );

    // Progressive read decision with a fake clock.
    const MIB: usize = 1 << 20;
    let planned = 12 * MIB as u64;
    let tail_start = 11 * MIB as u64;
    let mut pr = Progress {
        planned_len: planned,
        watermark: 2048,
        tail_start,
        tail_ready: false,
        done: false,
    };
    let header_ok = !progressive_logic::should_wait(0, 2048, &pr);
    let first_chunk_waits = progressive_logic::should_wait(0, MIB, &pr);
    let tail_waits = progressive_logic::should_wait(tail_start, 64 * 1024, &pr);
    pr.tail_ready = true;
    let tail_ok = !progressive_logic::should_wait(tail_start, 64 * 1024, &pr);
    let straddle_waits = progressive_logic::should_wait(tail_start - 4096, 64 * 1024, &pr);
    rep.check(
        "progressive: header served at once; first 1 MiB waits; the early-written tail serves reads inside it only",
        header_ok && first_chunk_waits && tail_waits && tail_ok && straddle_waits,
        format!("{header_ok} {first_chunk_waits} {tail_waits} {tail_ok} {straddle_waits}"),
    );
    // Writer advances 256 KiB every 10 ms; the reader wants [0, 1 MiB).
    let mut clock = 0u64;
    let mut polls = 0u32;
    let outcome = loop {
        pr.watermark = 2048 + (clock / 10) * 256 * 1024;
        match progressive_logic::poll(0, MIB, &pr, clock, 5000) {
            Poll::Wait => {
                polls += 1;
                clock += 2;
            }
            other => break other,
        }
    };
    rep.check(
        "progressive: fake clock -- the read is released the moment the watermark covers it (40 ms at 256 KiB / 10 ms)",
        outcome == Poll::Ready && clock == 40 && polls == 20,
        format!("{outcome:?} at {clock} ms after {polls} polls"),
    );
    pr.watermark = 2048;
    let mut clock = 0u64;
    let outcome = loop {
        match progressive_logic::poll(MIB as u64, MIB, &pr, clock, 5000) {
            Poll::Wait => clock += 5,
            other => break other,
        }
    };
    pr.done = true;
    let done_serves = !progressive_logic::should_wait(MIB as u64, MIB, &pr);
    rep.check(
        "progressive: a stalled writer times the read out at the 5 s bound; done serves everything",
        outcome == Poll::TimedOut && clock == 5000 && done_serves,
        format!("{outcome:?} at {clock} ms, done_serves={done_serves}"),
    );
    // Cursor tracking: 1 MiB chunks, a short tail read, an error return.
    let mut c = FdCursor::new();
    for _ in 0..11 {
        c.advance(MIB, MIB);
    }
    c.advance(usize::MAX, MIB);
    let before_tail = c.pos;
    c.advance(MIB, MIB);
    c.note_wait(7, false);
    let finished = progressive_logic::entry_finished(&c, &pr);
    rep.check(
        "progressive: the fd cursor follows returned lengths (errors ignored) and the entry finishes when done and fully read",
        before_tail == 11 * MIB as u64 && c.pos == planned && c.waits == 1 && c.waited_ms == 7 && finished,
        format!("pos {} waits {} finished {finished}", c.pos, c.waits),
    );
}

// ---------------------------------------------------------------------
// Leg 16: scroll gimmicks (Step 9)
// ---------------------------------------------------------------------

const MATT_SILVER_SCROLLS: &str = "0.000=1.000,116.000=0.000,116.500=1.000,171.500=0.000,171.750=1.000,264.167=0.000,265.000=1.500,266.000=0.000,267.000=1.500,282.667=1.000,293.000=0.000";
const MATT_SILVER_SPEEDS: &str = "0.000=1.000=0.000=0,160.000=0.000=0.000=0,160.021=1.000=6.000=0,196.000=0.500=0.000=0,196.021=1.000=16.000=0";

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

fn leg_scroll_gimmicks(rep: &mut Report) {
    let tm = timing(120);
    let mut w = Vec::new();
    let scrolls = sm_scroll::parse_scrolls(MATT_SILVER_SCROLLS, &mut w);
    let speeds = sm_scroll::parse_speeds(MATT_SILVER_SPEEDS, &mut w);
    rep.check(
        "scroll: Matt Silver tables parse cleanly",
        scrolls.len() == 11 && speeds.len() == 5 && w.is_empty(),
        format!("{} scrolls, {} speeds, warnings {w:?}", scrolls.len(), speeds.len()),
    );

    // Identity.
    let id = ScrollTable::new(&[]);
    let one = ScrollTable::new(&sm_scroll::parse_scrolls("0=1", &mut Vec::new()));
    rep.check(
        "scroll: empty and `0=1` tables are identity",
        id.is_identity()
            && one.is_identity()
            && near(id.display_beat(37.25), 37.25, 1e-12)
            && near(one.display_beat(37.25), 37.25, 1e-12)
            && SpeedTable::new(&[], &tm).is_identity()
            && near(SpeedTable::new(&[], &tm).speed_at(10.0, 5.0), 1.0, 1e-12),
        String::new(),
    );

    // 0-factor region flattens display beat; negative factor reverses.
    let hold = ScrollTable::new(&sm_scroll::parse_scrolls("0=1,8=0,12=1", &mut Vec::new()));
    rep.check(
        "scroll: a 0-factor segment freezes display beat",
        near(hold.display_beat(8.0), 8.0, 1e-12)
            && near(hold.display_beat(10.0), 8.0, 1e-12)
            && near(hold.display_beat(12.0), 8.0, 1e-12)
            && near(hold.display_beat(13.5), 9.5, 1e-12),
        format!("{} {} {}", hold.display_beat(10.0), hold.display_beat(12.0), hold.display_beat(13.5)),
    );
    let neg = ScrollTable::new(&sm_scroll::parse_scrolls("0=1,4=-1,6=1", &mut Vec::new()));
    rep.check(
        "scroll: a negative factor scrolls backwards",
        near(neg.display_beat(5.0), 3.0, 1e-12) && near(neg.display_beat(7.0), 3.0, 1e-12),
        format!("{} {}", neg.display_beat(5.0), neg.display_beat(7.0)),
    );

    // Matt Silver #SCROLLS.
    let ms = ScrollTable::new(&scrolls);
    let d116 = ms.display_beat(116.0);
    rep.check(
        "scroll: Matt Silver 116.0–116.5 is flat and resumes after",
        near(ms.display_beat(116.25), d116, 1e-9)
            && near(ms.display_beat(116.5), d116, 1e-9)
            && near(ms.display_beat(117.25), d116 + 0.75, 1e-9)
            && near(ms.display_beat(100.0), 100.0, 1e-9)
            && near(ms.display_beat(400.0), ms.display_beat(293.0), 1e-9),
        format!("{} {} {}", ms.display_beat(116.25), ms.display_beat(116.5), ms.display_beat(117.25)),
    );

    // Matt Silver #SPEEDS (120 BPM tempo map for the time-domain ramps).
    let sp = SpeedTable::new(&speeds, &tm);
    let at = |b: f64| sp.speed_at(b, sm_scroll::elapsed_seconds(&tm, b));
    rep.check(
        "scroll: Matt Silver speed 0 at 160, 0→1 over 6 beats from 160.021",
        near(at(159.9), 1.0, 1e-9)
            && near(at(160.0), 0.0, 1e-9)
            && near(at(160.021), 0.0, 1e-9)
            && near(at(163.021), 0.5, 1e-9)
            && near(at(163.0), 0.5, 0.01)
            && near(at(166.021), 1.0, 1e-9)
            && near(at(170.0), 1.0, 1e-9),
        format!("{} {} {} {} {}", at(159.9), at(160.0), at(163.021), at(163.0), at(166.021)),
    );
    rep.check(
        "scroll: Matt Silver 196 steps to 0.5 then ramps 0.5→1 over 16 beats",
        near(at(196.0), 0.5, 1e-9)
            && near(at(204.021), 0.75, 1e-9)
            && near(at(212.021), 1.0, 1e-9)
            && near(at(300.0), 1.0, 1e-9),
        format!("{} {} {}", at(196.0), at(204.021), at(212.021)),
    );

    // Unit 1 ramps in seconds.
    let s1 = SpeedTable::new(&sm_scroll::parse_speeds("0=1=0=0,8=2=1.5=1", &mut Vec::new()), &tm);
    rep.check(
        "scroll: unit-1 ramp runs in seconds",
        near(s1.speed_at(8.0, 4.0), 1.0, 1e-9)
            && near(s1.speed_at(9.5, 4.75), 1.5, 1e-9)
            && near(s1.speed_at(11.0, 5.5), 2.0, 1e-9)
            && near(s1.speed_at(20.0, 10.0), 2.0, 1e-9),
        format!("{} {} {}", s1.speed_at(8.0, 4.0), s1.speed_at(9.5, 4.75), s1.speed_at(11.0, 5.5)),
    );

    // Remap arithmetic (what the get_offset_y detour computes).
    let g = ScrollGimmicks::new(&scrolls, &speeds, &tm);
    let tpb = Beat::TICKS_PER_BEAT;
    let ticks = |b: f64| (b * tpb as f64).round() as i32;
    let el = |b: f64| sm_scroll::elapsed_seconds(&tm, b);
    let (d_plain, s_plain) = g.remap(ticks(100.0), 2 * tpb, el(100.0));
    let (d_freeze, _) = g.remap(ticks(115.0), 2 * tpb, el(115.0));
    let (d_inside, _) = g.remap(ticks(116.25), (0.2 * tpb as f64) as i64, el(116.25));
    let (d_passed, _) = g.remap(ticks(117.0), -2 * tpb, el(117.0));
    let (d_sq, s_sq) = g.remap(ticks(160.0), 4 * tpb, el(160.0));
    let (_, s_half) = g.remap(ticks(163.021), 4 * tpb, el(163.021));
    rep.check(
        "scroll: remap d' and speed scale",
        d_plain == 2 * tpb
            && near(s_plain, 1.0, 1e-12)
            && d_freeze == (1.5 * tpb as f64) as i64
            && d_inside == 0
            && d_passed == -((1.5 * tpb as f64) as i64)
            && d_sq == 4 * tpb
            && near(s_sq, 0.0, 1e-12)
            && near(s_half, 0.5, 1e-3)
            && ScrollGimmicks::default().remap(12345, 777, 0.0) == (777, 1.0),
        format!("{d_plain} {s_plain} {d_freeze} {d_inside} {d_passed} {d_sq} {s_sq} {s_half}"),
    );

    // Parser tolerance (StepMania hold-error rules).
    let mut w = Vec::new();
    let orphan = stepmania_validate::core::sm::notes::parse_notes_body(
        "1000\n0000\n3000\n0000",
        Style::Single,
        &mut w,
    );
    let orphan_ok = matches!(&orphan, Ok(n) if n.len() == 1 && n[0].kind == NoteKind::Tap)
        && w.len() == 1
        && w[0].0.contains("without a matching head");
    let mut w2 = Vec::new();
    let unclosed = stepmania_validate::core::sm::notes::parse_notes_body(
        "2000\n0000\n0000\n0000",
        Style::Single,
        &mut w2,
    );
    let unclosed_ok = matches!(&unclosed, Ok(n) if n.len() == 1 && n[0].kind == NoteKind::Tap)
        && w2.len() == 1
        && w2[0].0.contains("never closed");
    rep.check(
        "scroll: parser keeps unclosed heads as taps and ignores orphan tails (warned)",
        orphan_ok && unclosed_ok,
        format!("{orphan:?} {w:?} / {unclosed:?} {w2:?}"),
    );

    // Per-chart resolution for the committed charts + index round trip.
    let text = format!(
        "#BPMS:0=120;\n#SCROLLS:{MATT_SILVER_SCROLLS};\n#SPEEDS:{MATT_SILVER_SPEEDS};\n{}{}",
        "#NOTEDATA:;\n#STEPSTYPE:dance-single;\n#DIFFICULTY:Hard;\n#METER:9;\n#NOTES:\n1000\n0000\n0000\n0000\n;\n",
        "#NOTEDATA:;\n#STEPSTYPE:dance-single;\n#DIFFICULTY:Challenge;\n#METER:12;\n#NOTES:\n0100\n0000\n0000\n0000\n;\n"
    );
    let song = simfile::parse_ssc(&text);
    let bundle = writer::write_song(&song, &WriteOptions::default());
    let resolved = bundle.as_ref().ok().and_then(|b| {
        b.files
            .iter()
            .find(|f| f.index == 0)
            .map(|f0| (sm_convert::committed_scroll_gimmicks(&song, f0), sm_convert::beat0_ms(f0)))
    });
    let ok = match &resolved {
        Some((r, beat0)) => {
            r.conflict.is_none()
                && *beat0 == 0
                && r
                    .gimmicks
                    .as_ref()
                    .is_some_and(|g| g.scroll.len() == 11 && g.speed.len() == 5 && !g.is_identity())
        }
        None => false,
    };
    rep.check(
        "scroll: committed charts resolve to the song's tables",
        ok,
        format!("{resolved:?}"),
    );
    if let Some((r, beat0)) = resolved {
        let entry = SongEntry {
            scroll: r.gimmicks.clone(),
            beat0_ms: beat0,
            ..spike_entry()
        };
        let file = IndexFile {
            songs: vec![entry.clone(), spike_entry()],
            rejects: Vec::new(),
        };
        let bytes = sm_model::serialize_index(&file);
        let back = sm_model::parse_index(&bytes);
        let round = back.as_ref().is_some_and(|b| {
            b.songs.len() == 2
                && b.songs[0] == entry
                && b.songs[1].scroll.is_none()
                && b.songs[0].scroll.as_ref().is_some_and(|g| {
                    near(g.scroll.display_beat(116.25), g.scroll.display_beat(116.0), 1e-9)
                        && near(g.speed.speed_at(163.021, el(163.021)), 0.5, 1e-9)
                })
        });
        rep.check(
            "scroll: index.bin round-trips the tables (FORMAT_VERSION >= 4)",
            round && sm_model::FORMAT_VERSION >= 4,
            format!("{:?}", back.map(|b| b.songs.len())),
        );
    }
}

// ---------------------------------------------------------------------
// Leg 17: ssq_path chain + split declaration (Step 10)
// ---------------------------------------------------------------------

fn chain_stepmania(b: &[u8], d: usize) -> Option<u8> {
    // "Injected" basenames start with z: Expert from _4, the rest base (0).
    if !b.starts_with(b"z") {
        return None;
    }
    Some(if d == 3 { 4 } else { 0 })
}

fn chain_split_discovery(b: &[u8], _d: usize) -> Option<u8> {
    // Rule-A index: knows `acef` (every level split) and answers base for
    // every other non-injected basename — never `None` while active.
    Some(if b == b"acef" { 7 } else { 0 })
}

fn chain_split_discovery_disabled(_b: &[u8], _d: usize) -> Option<u8> {
    None
}

fn leg_ssq_path_split(rep: &mut Report) {
    // Chain semantics with both consumers registered (in the "wrong" order).
    let mut chain: Chain<u8> = Chain::new();
    chain.register(20, chain_split_discovery);
    chain.register(10, chain_stepmania);
    rep.check(
        "ssq_path: injected → StepMania (10), stock split → discovery (20), chain sorted ascending",
        chain.priorities() == vec![10, 20]
            && chain.resolve(b"zab12c", 3) == Some(4)
            && chain.resolve(b"zab12c", 4) == Some(0)
            && chain.resolve(b"acef", 2) == Some(7)
            && chain.resolve(b"toho3", 1) == Some(0),
        format!("{:?}", chain.priorities()),
    );
    // Discovery disabled → its resolver answers None → unknown basenames
    // reach the original; injected ones still resolve.
    let mut chain: Chain<u8> = Chain::new();
    chain.register(10, chain_stepmania);
    chain.register(20, chain_split_discovery_disabled);
    rep.check(
        "ssq_path: None from every resolver → the original; a consumer standing down never blocks the next",
        chain.resolve(b"acef", 3).is_none()
            && chain.resolve(b"zzzzzz", 3) == Some(4)
            && Chain::<u8>::new().resolve(b"acef", 0).is_none(),
        String::new(),
    );

    // A per-chart-timing song through the index's declaration fold.
    let text = "\
#OFFSET:-0.1;
#BPMS:0=110;
#NOTEDATA:;
#STEPSTYPE:dance-single;
#DIFFICULTY:Beginner;
#METER:1;
#NOTES:
1000
0000
0000
0000
;
#NOTEDATA:;
#STEPSTYPE:dance-single;
#DIFFICULTY:Hard;
#METER:8;
#OFFSET:-0.1;
#BPMS:0=220;
#NOTES:
0100
0000
,
0000
0000
;
#NOTEDATA:;
#STEPSTYPE:dance-double;
#DIFFICULTY:Hard;
#METER:9;
#NOTES:
01000000
00000000
00000000
00000000
;
#NOTEDATA:;
#STEPSTYPE:dance-single;
#DIFFICULTY:Challenge;
#METER:10;
#NOTES:
0010
0000
0000
0000
;
";
    let song = simfile::parse_ssc(text);
    let b = writer::write_song(&song, &WriteOptions::default()).expect("write");
    let passed: Vec<&writer::SsqFile> = b
        .files
        .iter()
        .filter(|f| {
            let slots: Vec<u8> = f.charts.iter().map(|c| c.0).collect();
            check_ssq(&f.bytes, &slots).is_ok()
        })
        .collect();
    let d = sm_convert::declare(&passed);
    let entry = SongEntry {
        levels: d.levels,
        split_levels: d.split_levels,
        ..spike_entry()
    };
    let answers: Vec<Option<u8>> = (0..5).map(|s| entry.split_file_for(s)).collect();
    rep.check(
        "split declaration: _1/_4/_5 declared (double Expert hidden: TimingConflict), Rule A names _N per difficulty",
        passed.len() == 3
            && b.files.iter().all(|f| f.index != 0)
            && d.levels == [1, 0, 0, 8, 10, 0, 0, 0, 0, 0]
            && d.split_levels == [1, 0, 0, 1 << 3, 1 << 4]
            && d.slots == vec![0, 3, 4]
            && entry.has_split_files()
            && answers == vec![Some(1), None, None, Some(4), Some(5)]
            && b.rejects.iter().any(|(l, r)| l == "dance-double Expert" && *r == writer::Reason::TimingConflict),
        format!("{:?} {:?} {:?} {:?}", d.levels, d.split_levels, answers, b.rejects),
    );
    // The Expert file's notes land at the same time as they would on the
    // song timing (2x BPM, beats doubled) — the fixture generator's contract.
    let expert = passed.iter().find(|f| f.index == 4).expect("_4");
    let conv = TempoConverter::from_ssq(&expert.bytes).expect("tempo");
    let beat4_ms = conv.beat_to_music_count(4096 * 2); // 8 beats at 220 == 4 beats at 110
    let base_ms = 100 + 4 * 60_000 / 110;
    rep.check(
        "split declaration: the 2x-BPM file keeps wall-clock note times (8 beats @220 == 4 beats @110)",
        (beat4_ms - base_ms).abs() <= 1,
        format!("{beat4_ms} vs {base_ms}"),
    );
    // A failed file costs only its slots.
    let without_challenge: Vec<&writer::SsqFile> =
        passed.iter().copied().filter(|f| f.index != 5).collect();
    let d2 = sm_convert::declare(&without_challenge);
    rep.check(
        "split declaration: a file that fails its self-check drops its slot and its _N entry only",
        d2.levels == [1, 0, 0, 8, 0, 0, 0, 0, 0, 0] && d2.split_levels == [1, 0, 0, 1 << 3, 0],
        format!("{:?} {:?}", d2.levels, d2.split_levels),
    );
    // index.bin carries the split table.
    let file = IndexFile {
        songs: vec![entry.clone(), spike_entry()],
        rejects: Vec::new(),
    };
    let back = sm_model::parse_index(&sm_model::serialize_index(&file));
    rep.check(
        "split declaration: index.bin round-trips split_levels (FORMAT_VERSION >= 4)",
        back.as_ref().is_some_and(|f| f.songs[0] == entry && !f.songs[1].has_split_files())
            && sm_model::FORMAT_VERSION >= 4,
        format!("{:?}", back.map(|f| f.songs[0].split_levels)),
    );
}

// ---------------------------------------------------------------------
// Leg 18: auto-sync (Step 12)
// ---------------------------------------------------------------------

const SYNC_RATE: u32 = 44_100;

/// xorshift64* in [-1, 1) (the estimator tests' generator).
fn sync_noise(state: &mut u64) -> f64 {
    *state ^= *state >> 12;
    *state ^= *state << 25;
    *state ^= *state >> 27;
    let x = state.wrapping_mul(0x2545_F491_4F6C_DD1D);
    (x >> 11) as f64 / (1u64 << 52) as f64 - 1.0
}

/// 150 BPM, `#OFFSET:-0.250`, one Hard chart: an irregular 16th pattern
/// over `measures` measures (tap at beat 0), plus an Edit the writer drops.
fn sync_fixture(measures: usize) -> String {
    let mut st = 0x1234_5678_9ABC_DEF1u64;
    let mut out = Vec::new();
    for m in 0..measures {
        let rows: Vec<&str> = (0..16)
            .map(|r| {
                if (m == 0 && r == 0) || sync_noise(&mut st) > 0.45 {
                    "1000"
                } else {
                    "0000"
                }
            })
            .collect();
        out.push(rows.join("\n"));
    }
    format!(
        "#TITLE:sync;\n#OFFSET:-0.250;\n#BPMS:0=150;\n\
         #NOTEDATA:;\n#STEPSTYPE:dance-single;\n#DIFFICULTY:Hard;\n#METER:9;\n#NOTES:\n{}\n;\n\
         #NOTEDATA:;\n#STEPSTYPE:dance-single;\n#DIFFICULTY:Edit;\n#METER:9;\n#NOTES:\n1111\n;\n",
        out.join("\n,\n")
    )
}

/// Audio-time of every note of the first chart (`0.25 + 0.4·beat` s).
fn sync_note_times(song: &stepmania_validate::core::sm::model::Song) -> Vec<f64> {
    song.charts
        .first()
        .map(|c| {
            c.notes
                .iter()
                .map(|n| 0.25 + n.beat.as_rational().as_f64() * 0.4)
                .collect()
        })
        .unwrap_or_default()
}

/// Stereo PCM: a 20 ms decaying noise burst at every time + `shift_ms`
/// (the second half a further `split_ms`) over low-level noise.
fn sync_bursts(times: &[f64], shift_ms: f64, split_ms: f64, seconds: f64) -> Vec<i16> {
    let rate = f64::from(SYNC_RATE);
    let frames = (seconds * rate) as usize;
    let mut st = 0xD1B5_4A32_D192_ED03u64;
    let mut mono: Vec<f64> = (0..frames).map(|_| 0.01 * sync_noise(&mut st)).collect();
    let burst = (0.020 * rate) as usize;
    let half = times.len() / 2;
    for (i, &t) in times.iter().enumerate() {
        let extra = if i >= half { split_ms } else { 0.0 };
        let start = ((t + (shift_ms + extra) / 1000.0) * rate).round() as usize;
        for k in 0..burst {
            if let Some(s) = mono.get_mut(start + k) {
                *s += 0.6 * (-(k as f64) / (0.005 * rate)).exp() * sync_noise(&mut st);
            }
        }
    }
    mono.iter()
        .flat_map(|&s| {
            let v = (s.clamp(-1.0, 1.0) * 32_767.0) as i16;
            [v, v]
        })
        .collect()
}

fn sync_view(pcm: &[i16]) -> AudioView<'_> {
    AudioView {
        samples: pcm,
        sample_rate: SYNC_RATE,
        channels: 2,
    }
}

fn leg_auto_sync(rep: &mut Report) {
    let song = simfile::parse_ssc(&sync_fixture(28));
    let times = sync_note_times(&song);
    let seconds = 0.25 + 28.0 * 4.0 * 0.4 + 3.0;
    // The runtime's lead-in for this song (td0 250 ms, first note at beat 0).
    let b0 = writer::write_song(&song, &WriteOptions::default()).expect("write");
    let (td0, first) = sm_convert::chart_timings(&b0);
    let lead = choose_lead_in(-f64::from(td0) / 1000.0, f64::from(first) / 1000.0, SYNC_RATE);
    let bundle = writer::write_song(
        &song,
        &WriteOptions {
            lead_in_ms: lead.ms,
            ..WriteOptions::default()
        },
    )
    .expect("write");
    let file = bundle.files[0].bytes.clone();
    let lead_s = lead.frames as f64 / f64::from(SYNC_RATE);
    rep.check(
        "auto-sync: the measured file carries the runtime lead-in and only the Hard chart (Edit dropped)",
        lead.ms >= 1750
            && first_anchor(&file) == Some(250 + lead.ms as i32)
            && file_slots(&file) == vec![3],
        format!("lead {} ms, td0 {:?}, slots {:?}", lead.ms, first_anchor(&file), file_slots(&file)),
    );

    // Synthetic click track at known offsets → Apply with the right delta.
    let measure = |pcm: &[i16], bytes: &[u8], lead_in_s: f64, params: &Params| {
        SongAnalysis::new(&sync_view(pcm)).measure(&song, bytes, lead_in_s, params)
    };
    let base_pcm = sync_bursts(&times, 0.0, 0.0, seconds);
    let base = measure(&base_pcm, &file, lead_s, &Params::default());
    let m0 = base.estimate.measured_ms.unwrap_or(f64::NAN);
    let unled = measure(&base_pcm, &b0.files[0].bytes, 0.0, &Params::default());
    rep.check(
        "auto-sync: lead-in is subtracted from the file's anchors (same measurement with and without it)",
        unled
            .estimate
            .measured_ms
            .is_some_and(|m| (m - m0).abs() <= (lead.ms as f64 - lead_s * 1000.0).abs() + 0.05),
        format!("{:?} vs {m0}", unled.estimate.measured_ms),
    );
    let mut ok = m0.is_finite() && m0.abs() < 10.0;
    let mut detail = format!("baseline {m0:.2} ms; ");
    for x in [-41.0, -17.0, 0.0, 9.0, 23.0, 44.0] {
        let m = measure(&sync_bursts(&times, x, 0.0, seconds), &file, lead_s, &Params::default());
        let measured = m.estimate.measured_ms.unwrap_or(f64::NAN);
        let want = (measured - TARGET_OFFSET_MS).round() as i32;
        let want_outcome = if want == 0 {
            Outcome::Unchanged
        } else {
            Outcome::Apply { delta_ms: want }
        };
        let good = (measured - m0 - x).abs() <= 0.5 && m.estimate.outcome == want_outcome;
        ok &= good;
        detail.push_str(&format!("x={x}: {measured:.2} {:?}; ", m.estimate.outcome));
    }
    rep.check(
        "auto-sync: synthetic click track at known offsets → Apply{round(measured − T)}, measured tracks the offset ±0.5 ms",
        ok,
        detail,
    );

    // Refusals.
    let few = simfile::parse_ssc(&sync_fixture(1));
    let few_b = writer::write_song(&few, &WriteOptions::default()).expect("write");
    let few_m = SongAnalysis::new(&sync_view(&sync_bursts(&sync_note_times(&few), 0.0, 0.0, 6.0)))
        .measure(&few, &few_b.files[0].bytes, 0.0, &Params::default());
    rep.check(
        "auto-sync: refusal too_few_events (one measure of notes)",
        few_m.estimate.outcome == Outcome::Refused(Refusal::TooFewEvents),
        format!("{:?}", few_m.estimate),
    );
    let capped = measure(
        &sync_bursts(&times, 35.0 + TARGET_OFFSET_MS - m0, 0.0, seconds),
        &file,
        lead_s,
        &Params { max_correction_ms: 30 },
    );
    rep.check(
        "auto-sync: refusal beyond_cap (35 ms against a 30 ms cap)",
        capped.estimate.outcome == Outcome::Refused(Refusal::BeyondCap)
            && capped.estimate.correction_ms.is_some_and(|c| (c - 35.0).abs() <= 0.5),
        format!("{:?}", capped.estimate),
    );
    let halves = measure(&sync_bursts(&times, 0.0, 10.0, seconds), &file, lead_s, &Params::default());
    rep.check(
        "auto-sync: refusal halves_disagree (second half 10 ms later)",
        halves.estimate.outcome == Outcome::Refused(Refusal::HalvesDisagree),
        format!("{:?}", halves.estimate),
    );
    let events: Vec<SyncEvent> = times.iter().map(|&t| SyncEvent { time_s: t, weight: 1.0 }).collect();
    let mut doubled = events.clone();
    doubled.extend(events.iter().map(|e| SyncEvent { time_s: e.time_s + 0.030, weight: e.weight }));
    let amb = sync::estimate(&sync_view(&base_pcm), &doubled, &Params::default());
    rep.check(
        "auto-sync: refusal ambiguous_peak (every event duplicated 30 ms later)",
        amb.outcome == Outcome::Refused(Refusal::AmbiguousPeak),
        format!("{amb:?}"),
    );
    let silent = SongAnalysis::new(&sync_view(&[])).measure(&song, &file, lead_s, &Params::default());
    rep.check(
        "auto-sync: refusal no_audio (empty PCM)",
        silent.estimate.outcome == Outcome::Refused(Refusal::NoAudio),
        format!("{:?}", silent.estimate),
    );

    // sync.bin (per-song ledger) round trip + malformed + stale drop.
    let mut table = SyncTable::new();
    table.insert(
        "zab12c",
        SyncRecord {
            source_fingerprint: 0xFEED,
            state: SyncState::Measured { delta_ms: 9 },
        },
    );
    table.insert(
        "zab12d",
        SyncRecord {
            source_fingerprint: 0xFEED,
            state: SyncState::Refused { reason: "ambiguous_peak".into() },
        },
    );
    table.insert(
        "zab12e",
        SyncRecord {
            source_fingerprint: 0xBEEF,
            state: SyncState::Measured { delta_ms: -60 },
        },
    );
    let bytes = table.serialize();
    let mut flipped = bytes.clone();
    flipped[9] ^= 0xFF;
    rep.check(
        "auto-sync: sync.bin (per song, v2) round-trips; truncated / trailing / bad-version input parses as empty",
        SyncTable::parse(&bytes).as_ref() == Some(&table)
            && &bytes[..8] == b"DDRSMSY2"
            && (0..bytes.len()).all(|n| SyncTable::parse(&bytes[..n]).is_none())
            && SyncTable::parse(&[bytes.clone(), vec![0]].concat()).is_none()
            && SyncTable::parse_or_empty(Some(&flipped)).is_empty()
            && table.current("zab12c", 0xFEED).map(SyncRecord::live_delta) == Some(Some(9))
            && table.current("zab12d", 0xFEED).map(SyncRecord::live_delta) == Some(None)
            && table.current("zab12e", 0xBEEF).map(SyncRecord::live_delta) == Some(Some(-60))
            && table.current("zab12c", 0xBEEF).is_none(),
        format!("{} bytes", bytes.len()),
    );
    let mut stale = table.clone();
    let fingerprints: std::collections::HashMap<String, u64> =
        [("zab12c".to_string(), 0xFEEDu64), ("zab12e".to_string(), 0x1234)].into_iter().collect();
    let dropped = stale.drop_stale(&fingerprints);
    rep.check(
        "auto-sync: stale records (song removed / fingerprint changed) are dropped at load",
        dropped == 2 && stale.len() == 1 && stale.get("zab12c").is_some(),
        format!("dropped {dropped}, left {}", stale.len()),
    );

    // Split-file decision: base file wins, else the lowest _N; > 2 ms apart flags.
    let fr = |index: u8, o: Result<i32, &str>| FileResult {
        index,
        outcome: o.map_err(str::to_string),
    };
    let base = decide_song(&[fr(3, Ok(13)), fr(0, Ok(10))]).expect("decision");
    let split = decide_song(&[fr(4, Ok(-3)), fr(2, Ok(-1)), fr(5, Ok(-2))]).expect("decision");
    let refused_pick = decide_song(&[fr(0, Err("too_few_events")), fr(3, Ok(5))]).expect("decision");
    rep.check(
        "auto-sync: one delta per song -- <b>.ssq's (else the lowest _N); files > 2 ms apart are flagged; a refused pick refuses the song",
        base.state == SyncState::Measured { delta_ms: 10 }
            && base.picked == 0
            && base.disagree
            && split.state == SyncState::Measured { delta_ms: -1 }
            && split.picked == 2
            && !split.disagree
            && refused_pick.state == SyncState::Refused { reason: "too_few_events".into() }
            && decide_song(&[]).is_none(),
        format!("{base:?} / {split:?} / {refused_pick:?}"),
    );

    // Freeze rule.
    let mut live = LiveMap::new();
    live.seed("a", Some(9));
    let before = live.publish("a", Some(11));
    let first_play = live.freeze("a");
    let late = live.publish("a", Some(-4));
    let replay = live.freeze("a");
    let unmeasured = live.freeze("b");
    let late_b = live.publish("b", Some(7));
    let refused = live.publish("c", None);
    rep.check(
        "auto-sync: freeze rule -- publish before the freeze changes the value, after it does not (Late); unmeasured freezes as None; refused is None",
        before == Publish::Applied
            && first_play == Some(11)
            && late == Publish::Late
            && replay == Some(11)
            && live.get("a") == Some(LiveEntry { delta: Some(11), frozen: true })
            && unmeasured.is_none()
            && late_b == Publish::Late
            && live.freeze("b").is_none()
            && refused == Publish::Applied
            && live.is_measured("c")
            && live.freeze("c").is_none()
            && !live.is_measured("d"),
        format!("{live:?}"),
    );

    // The writer's shift primitive stays (tooling); the runtime never calls it.
    rep.check(
        "auto-sync: shift_seconds +d then −d is the identity (d = 9, −60, 37)",
        [9, -60, 37].iter().all(|&d| {
            writer::shift_seconds(&file, d).and_then(|s| writer::shift_seconds(&s, -d)).as_deref()
                == Some(file.as_slice())
        }),
        String::new(),
    );
    // The 60 ms auto-sync headroom: the worst correction keeps td0 >= 0.
    rep.check(
        "auto-sync: lead-in headroom keeps tempo_data[0] >= 0 after a -60 ms correction",
        writer::shift_seconds(&file, -60).and_then(|b| first_anchor(&b)).is_some_and(|t| t >= 0),
        String::new(),
    );

    leg_auto_sync_tool(rep, &song, &b0.files[0].bytes, &base_pcm, &times);
}

/// The estimator and the event extraction against the sibling CLI's own
/// `sync` module on the same inputs.
#[cfg(feature = "identity")]
fn leg_auto_sync_tool(
    rep: &mut Report,
    song: &stepmania_validate::core::sm::model::Song,
    file_no_lead: &[u8],
    base_pcm: &[i16],
    times: &[f64],
) {
    use ddr_chart_tools::model::AudioBuffer;
    use ddr_chart_tools::sync as tool;
    let pairs = tempo_entries(file_no_lead);
    let tps = TempoConverter::from_ssq(file_no_lead).map(|c| c.tps() as u32).unwrap_or(1000);
    let ours_map = sync::TimeMap::from_tempo_pairs(&pairs, tps).expect("map");
    let tool_map = tool::TimeMap::from_tempo_pairs(&pairs, tps).expect("map");
    let charts: Vec<&Chart> = file_slots(file_no_lead)
        .iter()
        .filter_map(|&s| stepmania_validate::core::sync::measure::declared_chart(song, s))
        .collect();
    let ours_ev = sync::chart_events(&charts, &song.timing, &ours_map);
    let tool_song = ddr_chart_tools::ssc::parse(&sync_fixture(28)).expect("tool parse");
    // The tool's parser skips the Edit chart; the writer (and so we) drop it.
    let tool_ev = tool::chart_events(&tool_song.charts, &tool_map);
    rep.check(
        "auto-sync: chart_events equal the CLI's on the same file pairs (times ±1e-9 s, weights)",
        ours_ev.len() == tool_ev.len()
            && ours_ev.iter().zip(&tool_ev).all(|(a, b)| (a.time_s - b.time_s).abs() < 1e-9 && a.weight == b.weight),
        format!("{} vs {} events", ours_ev.len(), tool_ev.len()),
    );
    let mut worst = 0.0f64;
    let mut same_outcome = true;
    for x in [-37.0, 0.0, 12.0, 41.0] {
        let pcm = if x == 0.0 { base_pcm.to_vec() } else { sync_bursts(times, x, 0.0, 0.25 + 28.0 * 1.6 + 3.0) };
        let ev: Vec<SyncEvent> = ours_ev.clone();
        let ours = sync::estimate(&sync_view(&pcm), &ev, &Params::default());
        let tev: Vec<tool::SyncEvent> = ev.iter().map(|e| tool::SyncEvent { time_s: e.time_s, weight: e.weight }).collect();
        let buf = AudioBuffer { samples: pcm.clone(), sample_rate: SYNC_RATE, channels: 2 };
        let theirs = tool::estimate(&buf, &tev, &tool::Params::default());
        if let (Some(a), Some(b)) = (ours.correction_ms, theirs.correction_ms) {
            worst = worst.max((a - b).abs());
        } else {
            worst = f64::INFINITY;
        }
        same_outcome &= format!("{:?}", ours.outcome) == format!("{:?}", theirs.outcome);
    }
    rep.check(
        &format!("auto-sync: estimator vs CLI sync::estimate on synthetic fixtures, max |Δcorrection| = {worst:.4} ms (≤ 1 ms), same outcomes"),
        worst <= 1.0 && same_outcome,
        format!("worst {worst}, same_outcome {same_outcome}"),
    );
}

#[cfg(not(feature = "identity"))]
fn leg_auto_sync_tool(
    rep: &mut Report,
    _song: &stepmania_validate::core::sm::model::Song,
    _file: &[u8],
    _pcm: &[i16],
    _times: &[f64],
) {
    rep.skip("auto-sync: estimator / events vs the CLI's sync module", "no ddr-chart-tools checkout");
}

fn print_corpus_stats(corpus: &[GenSong]) {
    let (mut charts, mut taps, mut holds, mut rolls, mut mines, mut shocks) = (0, 0, 0, 0, 0, 0);
    let (mut bpm_changes, mut stops, mut offsets, mut offgrid) = (0, 0, 0, 0);
    for song in corpus {
        let s = simfile::parse(&format!("x.{}", song.ext), &song.text);
        bpm_changes += s.timing.bpms.len().saturating_sub(1);
        stops += s.timing.stops.len();
        offsets += usize::from(!s.timing.offset_s.is_zero());
        charts += s.charts.len();
        for c in &s.charts {
            for n in &c.notes {
                match n.kind {
                    NoteKind::Tap => taps += 1,
                    NoteKind::HoldHead { .. } => holds += 1,
                    NoteKind::RollHead { .. } => rolls += 1,
                    NoteKind::Mine => mines += 1,
                    NoteKind::Shock { .. } => shocks += 1,
                    _ => {}
                }
                if n.beat.as_rational().den() % 2 == 1 && n.beat.as_rational().den() != 1 {
                    offgrid += 1;
                }
            }
        }
    }
    println!(
        "corpus features: {charts} charts, {taps} taps/jumps, {holds} holds, {rolls} rolls, {mines} mines, {shocks} shocks, {bpm_changes} BPM changes, {stops} stops, {offsets} non-zero offsets, {offgrid} triplet-grid notes"
    );
}

fn main() {
    let scratch = PathBuf::from(std::env::var("SM_SCRATCH").unwrap_or_else(|_| ".".into()));
    let mut rep = Report {
        pass: 0,
        fail: 0,
        skip: 0,
    };

    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut corpus = fixed_corpus();
    for i in 0..40 {
        corpus.push(gen_song(&mut rng, i));
    }
    println!(
        "corpus: {} songs ({} hand-written, {} generated; {} .sm)",
        corpus.len(),
        fixed_corpus().len(),
        corpus.len() - fixed_corpus().len(),
        corpus.iter().filter(|s| s.ext == "sm").count()
    );

    print_corpus_stats(&corpus);

    leg_identity(&mut rep, &corpus, &scratch);
    leg_corpus_selfcheck(&mut rep, &corpus);
    leg_quad_split(&mut rep);
    leg_selfcheck_negatives(&mut rep);
    leg_delay_nudge(&mut rep);
    leg_shift_seconds(&mut rep, &corpus);
    leg_timing_split(&mut rep);
    leg_lead_in(&mut rep, &corpus);
    leg_xsb(&mut rep);
    leg_xwb(&mut rep);
    leg_audio(&mut rep, &scratch);
    leg_identity_derivation(&mut rep);
    leg_musicdb(&mut rep);
    leg_art(&mut rep);
    leg_scheduler_progressive(&mut rep);
    leg_scroll_gimmicks(&mut rep);
    leg_ssq_path_split(&mut rep);
    leg_auto_sync(&mut rep);

    println!();
    println!("legs: {} PASS, {} FAIL, {} SKIP", rep.pass, rep.fail, rep.skip);
    if rep.fail > 0 {
        println!("VALIDATION FAILED");
        std::process::exit(1);
    }
    println!("ALL CHECKS PASSED");
}
EOF

FEATURES=()
if [[ -n "$CHART_TOOLS" ]]; then FEATURES=(--features identity); fi

note "leg 1: running the mounted #[cfg(test)] suites (core::sm, core::ssq, core::audio, core::sync, core::xact, core::art, services::stepmania, services::ssq_path_logic)"
(cd "$TMP" && cargo test --quiet --lib ${FEATURES[@]+"${FEATURES[@]}"} 2>&1 | grep -v '^$' | grep -v 'running 0 tests')
echo "PASS  mounted unit-test suites"

note "legs 2-18: building + running the harness binary (host toolchain, release: the bank legs encode minutes of ADPCM)"
mkdir -p "$TMP/scratch"
(cd "$TMP" && SM_TOOL_BIN="$TOOL_BIN" SM_LAME="$LAME_BIN" SM_SCRATCH="$TMP/scratch" cargo run --quiet --release --bin legs ${FEATURES[@]+"${FEATURES[@]}"})
