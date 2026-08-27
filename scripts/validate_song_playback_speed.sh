#!/usr/bin/env bash
# Host-only validation for the song playback speed audio pipeline.
#
# Ordinary use runs synthetic validation only:
#   ./scripts/validate_song_playback_speed.sh
#
# Release-corpus use requires supported local XWBs in both entry orders and a
# custom bank whose relative filename contains "custom":
#   DDR_SONG_RATE_CORPUS_DIR=/path/to/corpus \
#     ./scripts/validate_song_playback_speed.sh --require-corpus
#
# Inputs are read in place and represented only by digest in the report. Demo
# outputs and the stable JSON report are written under
# target/song-rate-validation/ and never under source/data directories.

set -euo pipefail
cd "$(dirname "$0")/.."
REPO_ROOT="$(pwd)"
OUTPUT_DIR="$REPO_ROOT/target/song-rate-validation"
# Never let a failed argument/precondition check leave a stale successful report.
rm -f "$OUTPUT_DIR/report.json"

die() { echo "error: $*" >&2; exit 1; }
note() { echo "[*] $*"; }

CHART_TOOLS="${DDR_CHART_TOOLS_DIR:-}"
CORPUS_DIR="${DDR_SONG_RATE_CORPUS_DIR:-}"
PLATFORM="${DDR_SONG_RATE_PLATFORM:-development}"
REQUIRE_CORPUS=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --chart-tools)
      [[ $# -ge 2 ]] || die "--chart-tools needs a path"
      CHART_TOOLS="$2"
      shift 2
      ;;
    --corpus)
      [[ $# -ge 2 ]] || die "--corpus needs a path"
      CORPUS_DIR="$2"
      shift 2
      ;;
    --require-corpus)
      REQUIRE_CORPUS=1
      shift
      ;;
    --platform)
      [[ $# -ge 2 ]] || die "--platform needs native-windows, crossover, or development"
      PLATFORM="$2"
      shift 2
      ;;
    -h|--help)
      sed -n '2,/^$/p' "$0"
      exit 0
      ;;
    *) die "unknown option: $1" ;;
  esac
done

case "$PLATFORM" in
  native-windows|crossover|development) ;;
  *) die "invalid platform '$PLATFORM' (expected native-windows, crossover, or development)" ;;
esac

if [[ -z "$CHART_TOOLS" && -f "$REPO_ROOT/../ddr-chart-tools/Cargo.toml" ]]; then
  CHART_TOOLS="$REPO_ROOT/../ddr-chart-tools"
fi
[[ -n "$CHART_TOOLS" && -f "$CHART_TOOLS/Cargo.toml" ]] \
  || die "need a ddr-chart-tools source checkout (--chart-tools, DDR_CHART_TOOLS_DIR, or ../ddr-chart-tools)"
CHART_TOOLS="$(cd "$CHART_TOOLS" && pwd)"

if [[ -n "$CORPUS_DIR" ]]; then
  [[ -d "$CORPUS_DIR" ]] || die "corpus directory does not exist: $CORPUS_DIR"
  CORPUS_DIR="$(cd "$CORPUS_DIR" && pwd)"
elif [[ "$REQUIRE_CORPUS" == 1 ]]; then
  die "--require-corpus needs --corpus or DDR_SONG_RATE_CORPUS_DIR"
fi
if [[ "$REQUIRE_CORPUS" == 1 && "$PLATFORM" == development ]]; then
  die "--require-corpus needs --platform native-windows or --platform crossover"
fi
if [[ -n "$CORPUS_DIR" && "$CORPUS_DIR/" == "$OUTPUT_DIR/"* ]]; then
  die "corpus must be external to target/song-rate-validation"
fi

SIBLING_REV="$(git -C "$CHART_TOOLS" rev-parse HEAD 2>/dev/null)" \
  || die "could not resolve ddr-chart-tools git revision"

XACT_SRC="$REPO_ROOT/src/core/xact"
MEMORY_PATCH_SRC="$REPO_ROOT/src/core"
SONG_RATE_SRC="$REPO_ROOT/src/services/song_rate"
for file in mod.rs adpcm.rs digest.rs rate.rs resample.rs stretch.rs virtual_bank.rs xwb.rs; do
  [[ -r "$XACT_SRC/$file" ]] || die "module source missing: src/core/xact/$file"
done
for file in hook_transaction.rs hook_transaction_tests.rs memory_patch.rs memory_patch_tests.rs; do
  [[ -r "$MEMORY_PATCH_SRC/$file" ]] || die "module source missing: src/core/$file"
done
for file in mod.rs clock_patch.rs clock_patch_tests.rs wavebank_hook.rs wavebank_hook_tests.rs xact_runtime.rs xact_runtime_tests.rs lifecycle.rs lifecycle_tests.rs transaction.rs transaction_tests.rs binding.rs binding_tests.rs generator.rs generator_tests.rs io_callback_hook.rs preview.rs preview_tests.rs tick_domain.rs tick_domain_tests.rs real_speed.rs real_speed_tests.rs; do
  [[ -r "$SONG_RATE_SRC/$file" ]] || die "module source missing: src/services/song_rate/$file"
done
for file in score_guard.rs score_guard_tests.rs; do
  [[ -r "$REPO_ROOT/src/services/$file" ]] || die "module source missing: src/services/$file"
done
for file in api.rs registry.rs availability_tests.rs; do
  [[ -r "$REPO_ROOT/src/services/custom_options/$file" ]] || die "module source missing: src/services/custom_options/$file"
done
[[ -r "$REPO_ROOT/src/types/scenes.rs" ]] || die "module source missing: src/types/scenes.rs"

rm -rf "$OUTPUT_DIR/demo" "$OUTPUT_DIR/corpus" "$OUTPUT_DIR/fixtures"
mkdir -p "$OUTPUT_DIR/demo" "$OUTPUT_DIR/corpus" "$OUTPUT_DIR/fixtures"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
note "harness dir: $TMP"

cat >"$TMP/Cargo.toml" <<EOF
[package]
name = "song-rate-validate"
version = "0.0.0"
edition = "2021"

[dependencies]
ddr-chart-tools = { path = "$CHART_TOOLS" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
libc = "0.2"
once_cell = "1"

[workspace]
EOF

mkdir -p "$TMP/src"
cat >"$TMP/src/memory.rs" <<'EOF_MEMORY'
#[cfg(target_os = "linux")]
pub fn current_working_set_bytes() -> Option<u64> {
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    let resident_pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    (page_size > 0).then(|| resident_pages.saturating_mul(page_size as u64))
}

#[cfg(target_os = "macos")]
pub fn current_working_set_bytes() -> Option<u64> {
    #[repr(C)]
    struct TimeValue {
        seconds: i32,
        microseconds: i32,
    }
    #[repr(C)]
    struct MachTaskBasicInfo {
        virtual_size: u64,
        resident_size: u64,
        resident_size_max: u64,
        user_time: TimeValue,
        system_time: TimeValue,
        policy: i32,
        suspend_count: i32,
    }
    #[link(name = "System")]
    unsafe extern "C" {
        static mach_task_self_: u32;
        fn task_info(task: u32, flavor: u32, info: *mut i32, count: *mut u32) -> i32;
    }
    let mut info = std::mem::MaybeUninit::<MachTaskBasicInfo>::zeroed();
    let mut count = (std::mem::size_of::<MachTaskBasicInfo>() / std::mem::size_of::<u32>()) as u32;
    let result = unsafe {
        task_info(
            mach_task_self_,
            20,
            info.as_mut_ptr().cast::<i32>(),
            &mut count,
        )
    };
    (result == 0).then(|| unsafe { info.assume_init() }.resident_size)
}

#[cfg(windows)]
pub fn current_working_set_bytes() -> Option<u64> {
    use std::ffi::c_void;

    #[repr(C)]
    struct ProcessMemoryCounters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut c_void;
    }
    #[link(name = "psapi")]
    unsafe extern "system" {
        fn GetProcessMemoryInfo(
            process: *mut c_void,
            counters: *mut ProcessMemoryCounters,
            size: u32,
        ) -> i32;
    }

    let mut counters = ProcessMemoryCounters {
        cb: std::mem::size_of::<ProcessMemoryCounters>() as u32,
        page_fault_count: 0,
        peak_working_set_size: 0,
        working_set_size: 0,
        quota_peak_paged_pool_usage: 0,
        quota_paged_pool_usage: 0,
        quota_peak_non_paged_pool_usage: 0,
        quota_non_paged_pool_usage: 0,
        pagefile_usage: 0,
        peak_pagefile_usage: 0,
    };
    let result = unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            &mut counters,
            std::mem::size_of::<ProcessMemoryCounters>() as u32,
        )
    };
    (result != 0).then_some(counters.working_set_size as u64)
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub fn current_working_set_bytes() -> Option<u64> {
    None
}
EOF_MEMORY

cat >"$TMP/src/main.rs" <<EOF
mod memory;

mod core {
    #[path = "$XACT_SRC/mod.rs"]
    pub mod xact;
    #[path = "$MEMORY_PATCH_SRC/memory_patch.rs"]
    pub mod memory_patch;
    #[path = "$MEMORY_PATCH_SRC/hook_transaction.rs"]
    pub mod hook_transaction;
    #[cfg(test)]
    #[path = "$MEMORY_PATCH_SRC/memory_patch_tests.rs"]
    mod memory_patch_tests;
    #[cfg(test)]
    #[path = "$MEMORY_PATCH_SRC/hook_transaction_tests.rs"]
    mod hook_transaction_tests;
}

mod types {
    #[path = "$REPO_ROOT/src/types/scenes.rs"]
    pub mod scenes;
}

mod services {
    #[path = "$REPO_ROOT/src/services/movie_policy.rs"]
    pub mod movie_policy;
    #[cfg(test)]
    #[path = "$REPO_ROOT/src/services/movie_policy_tests.rs"]
    mod movie_policy_tests;
    #[path = "$REPO_ROOT/src/services/score_guard.rs"]
    pub mod score_guard;
    #[cfg(test)]
    #[path = "$REPO_ROOT/src/services/score_guard_tests.rs"]
    mod score_guard_tests;
    #[path = "$SONG_RATE_SRC/mod.rs"]
    pub mod song_rate;
    // Pure custom-options kernel (api + registry only — the windows-heavy
    // framework glue stays out): hosts the option-availability semantics
    // behind set_option_available / the builder hook's injection filter.
    pub mod custom_options {
        #[path = "$REPO_ROOT/src/services/custom_options/api.rs"]
        pub mod api;
        #[path = "$REPO_ROOT/src/services/custom_options/registry.rs"]
        pub(crate) mod registry;
        #[cfg(test)]
        #[path = "$REPO_ROOT/src/services/custom_options/availability_tests.rs"]
        mod availability_tests;
    }
}

use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use core::xact::rate::RateRatio;
use core::xact::stretch::StretchState;
use core::xact::{adpcm, digest, resample, stretch, virtual_bank, xwb, WaveFormat as SharedWaveFormat};
use ddr_chart_tools::xwb::adpcm::{decode as sibling_decode, encode as sibling_encode};
use ddr_chart_tools::xwb::{container as sibling_xwb, WaveFormat, XwbBank, XwbEntry};
use serde::Serialize;
use memory::current_working_set_bytes;
use services::movie_policy::{MoviePolicy, MovieSuppressor};
use services::song_rate::clock_patch::{scale_music_count_q31, RatePublication, IDENTITY_Q31};
use services::song_rate::wavebank_hook::{call_create_identity, identity_conversion_path};
use services::song_rate::xact_runtime::{enter_frame, MaintenanceQueue, XactSlots};

const SAMPLE_RATE: u32 = 8_000;
const SOURCE_FRAMES: usize = 8_192;
const PREVIEW_DURATION_FRAMES: usize = 8_100;
const PITCH_LIMIT_PERCENT: f64 = 0.25;
const SNR_MIN_DB: f64 = 30.0;
const SEAM_TOLERANCE: i32 = 2_048;
const NATIVE_WINDOWS_LATENCY_LIMIT_MS: u128 = 15_000;
const CROSSOVER_LATENCY_LIMIT_MS: u128 = 25_000;

#[derive(Serialize)]
struct Report {
    schema: &'static str,
    overall_pass: bool,
    mode: &'static str,
    platform: String,
    sibling_revision: String,
    thresholds: Thresholds,
    checks: Vec<Check>,
    synthetic: Vec<RateResult>,
    resample: Vec<ResampleResult>,
    corpus: Vec<CorpusResult>,
    identity_runtime: IdentityRuntimeReport,
    streaming: StreamingReport,
}

/// One preserve-pitch-OFF (resample) validation cell: the pitch expectation
/// is INVERTED relative to the stretch legs — the output frequency must
/// track `f_source × source_frames/output_frames` (the plan's exact
/// effective ratio), not `f_source`.
#[derive(Serialize)]
struct ResampleResult {
    percent: u32,
    source_frames: u64,
    output_frames: u64,
    rate_numerator: u64,
    rate_denominator: u64,
    source_frequency_hz: f64,
    output_frequency_hz: f64,
    expected_output_frequency_hz: f64,
    pitch_tracking_error_percent: f64,
    codec_snr_db: f64,
    deterministic: bool,
    exact_output_length: bool,
    generation_latency_ms: u128,
    passed: bool,
}


#[derive(Serialize)]
struct IdentityRuntimeReport {
    passed: bool,
    checks: Vec<Check>,
}

#[derive(Serialize)]
struct StreamingReport {
    passed: bool,
    /// Informational only — never part of any pass criterion (the real
    /// throughput gate is the plan Step 5 live cabinet benchmark).
    synthetic_frames_per_second: f64,
    rates: Vec<StreamingRateResult>,
    /// Plan Step 3 demo: full synthetic engine replays at the demo rates.
    replays: Vec<StreamingReplayResult>,
    checks: Vec<Check>,
}

#[derive(Serialize)]
struct StreamingRateResult {
    percent: u32,
    loop_shape: String,
    source_frames: u64,
    output_frames: u64,
    byte_equality: bool,
    counters_match: bool,
    chunking_independent: bool,
    checkpoint_restore: bool,
    passed: bool,
}

#[derive(Serialize)]
struct StreamingReplayResult {
    percent: u32,
    entry_order: String,
    /// Data-packet reads issued (the header and defensive EOF reads are
    /// pattern-checked but not counted).
    packet_count: usize,
    reassembled_len: u64,
    /// The RE-pinned read pattern held: full 0x1000 header serve, packets
    /// served in full, nothing past the virtual size, EOF read serves 0.
    read_pattern: bool,
    reparsed: bool,
    /// Reassembled bytes byte-equal the whole-buffer transform oracle.
    matches_reference: bool,
    /// Both entries of the reassembled bank decode equal to the oracle's.
    decode_equality: bool,
    passed: bool,
}

#[derive(Serialize)]
struct Thresholds {
    pitch_error_percent_max: f64,
    codec_snr_db_min: f64,
    clipping_samples_max: usize,
    stereo_lag_samples_max: i32,
    seam_over_source_max: i32,
    generation_latency_ms_max: u128,
}

#[derive(Serialize)]
struct Check {
    name: String,
    passed: bool,
    detail: String,
}

#[derive(Serialize)]
struct RateResult {
    label: String,
    percent: u32,
    source_frames: u64,
    output_frames: u64,
    rate_numerator: u64,
    rate_denominator: u64,
    pitch_error_percent: f64,
    codec_snr_db: f64,
    clipping_samples: usize,
    source_stereo_lag_samples: i32,
    stereo_lag_samples: i32,
    stereo_lag_delta_samples: i32,
    preview_seam_delta: i32,
    preview_source_seam_delta: i32,
    deterministic: bool,
    identity_preserved: bool,
    peak_working_set_bytes: u64,
    additional_peak_working_set_bytes: u64,
    generation_latency_ms: u128,
    input_digest: String,
    output_digest: String,
    demo_path: String,
    passed: bool,
}

#[derive(Serialize)]
struct CorpusResult {
    relative_path: String,
    input_digest: String,
    entry_order: String,
    custom_named: bool,
    rates: Vec<CorpusRateResult>,
    passed: bool,
    error: Option<String>,
}

#[derive(Serialize)]
struct CorpusRateResult {
    percent: u32,
    output_digest: String,
    source_frames: u64,
    output_frames: u64,
    rate_numerator: u64,
    rate_denominator: u64,
    clipping_samples: usize,
    preview_seam_delta: Option<i32>,
    preview_source_seam_delta: Option<i32>,
    pitch_error_percent: f64,
    codec_snr_db: f64,
    source_stereo_lag_samples: i32,
    stereo_lag_samples: i32,
    stereo_lag_delta_samples: i32,
    peak_working_set_bytes: u64,
    additional_peak_working_set_bytes: u64,
    generation_latency_ms: u128,
    deterministic: bool,
    identity_preserved: bool,
    demo_path: String,
    passed: bool,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 6 {
        fail("internal harness argument mismatch");
    }
    let output_dir = PathBuf::from(&args[1]);
    let sibling_revision = args[2].clone();
    let corpus_dir = if args[3].is_empty() {
        None
    } else {
        Some(PathBuf::from(&args[3]))
    };
    let require_corpus = args[4] == "1";
    let platform = args[5].clone();

    let mut checks = vec![Check {
        name: "pure_module_tests".into(),
        passed: true,
        detail: "cargo test completed before demo execution".into(),
    }];
    let source = build_source(false);
    let reverse_order = build_source(true);
    let generated_fixture_digests = [
        digest::md5_bytes(&source).to_hex(),
        digest::md5_bytes(&reverse_order).to_hex(),
    ];
    let fixture_dir = output_dir.join("fixtures");
    fs::create_dir_all(&fixture_dir).expect("create synthetic fixture directory");
    fs::write(fixture_dir.join("main-preview.xwb"), &source).expect("write main-first fixture");
    fs::write(fixture_dir.join("preview-main.xwb"), &reverse_order).expect("write preview-first fixture");
    fs::write(fixture_dir.join("custom-main-preview.xwb"), &source).expect("write custom fixture");
    let shared_source = xwb::parse_song_bank(&source).expect("shared parser accepts synthetic source");
    let sibling_source = sibling_xwb::parse(&source).expect("sibling parser accepts synthetic source");
    checks.push(check(
        "cross_repository_parser",
        parsers_agree(&shared_source, &sibling_source),
        "shared and sibling parsers agree on complete bank/entry metadata and payloads",
    ));
    let reverse = xwb::parse_song_bank(&reverse_order).expect("reverse-order fixture");
    checks.push(check(
        "both_entry_orders",
        shared_source.entries[0].name() == "synt"
            && reverse.entries[0].name() == "synt_s",
        "synthetic main/preview and preview/main fixtures parsed",
    ));

    let (codec_snr_db, codec_match) = codec_comparison();
    checks.push(check(
        "cross_repository_codec",
        codec_match,
        &format!(
            "shared/sibling mono+stereo encode and stereo decode match; SNR={codec_snr_db:.3} dB"
        ),
    ));

    let mut synthetic = Vec::new();
    for percent in [75, 125] {
        let result = validate_rate(
            &source,
            percent,
            codec_snr_db,
            &output_dir.join("demo"),
            "synthetic",
            &platform,
        );
        checks.push(check(
            &format!("synthetic_{percent}"),
            result.passed,
            &format!(
                "ratio={}/{}, pitch_error={:.4}%, latency={}ms, peak_working_set={}B, additional={}B",
                result.rate_numerator,
                result.rate_denominator,
                result.pitch_error_percent,
                result.generation_latency_ms,
                result.peak_working_set_bytes,
                result.additional_peak_working_set_bytes
            ),
        ));
        synthetic.push(result);
    }

    let mut resample_results = Vec::new();
    for percent in [50, 175] {
        let result = validate_resample(&source, percent, codec_snr_db, &platform);
        checks.push(check(
            &format!("resample_{percent}"),
            result.passed,
            &format!(
                "ratio={}/{}, f_src={:.2}Hz, f_out={:.2}Hz (expected {:.2}Hz), tracking_error={:.4}%, latency={}ms",
                result.rate_numerator,
                result.rate_denominator,
                result.source_frequency_hz,
                result.output_frequency_hz,
                result.expected_output_frequency_hz,
                result.pitch_tracking_error_percent,
                result.generation_latency_ms
            ),
        ));
        resample_results.push(result);
    }

    let mut corpus = Vec::new();
    if let Some(root) = corpus_dir.as_deref() {
        let mut files = Vec::new();
        collect_xwbs(root, root, &mut files);
        files.sort();
        for relative in files {
            corpus.push(validate_corpus(root, &relative, &output_dir.join("corpus"), &platform));
        }
        let has_main_first = corpus.iter().any(|entry| entry.entry_order == "main-preview" && entry.passed);
        let has_preview_first = corpus.iter().any(|entry| entry.entry_order == "preview-main" && entry.passed);
        let has_custom = corpus.iter().any(|entry| entry.custom_named && entry.passed);
        let has_generated_fixture = corpus
            .iter()
            .any(|entry| generated_fixture_digests.contains(&entry.input_digest));
        let corpus_complete = !corpus.is_empty()
            && corpus.iter().all(|entry| entry.passed)
            && !has_generated_fixture
            && (!require_corpus || (has_main_first && has_preview_first && has_custom));
        checks.push(check(
            "corpus_profiles",
            corpus_complete,
            &format!(
                "files={}, main-first={}, preview-first={}, custom={}, generated-fixture={}",
                corpus.len(), has_main_first, has_preview_first, has_custom, has_generated_fixture
            ),
        ));
    } else if require_corpus {
        checks.push(check("corpus_profiles", false, "release corpus missing"));
    } else {
        checks.push(check(
            "corpus_profiles",
            true,
            "synthetic-only development mode; release corpus not requested",
        ));
    }

    let identity_runtime = validate_identity_runtime();
    let streaming = validate_streaming(&source, &reverse_order);
    let overall_pass =
        checks.iter().all(|entry| entry.passed) && identity_runtime.passed && streaming.passed;
    let report = Report {
        schema: "song-rate-validation/v1",
        overall_pass,
        mode: if require_corpus { "release-corpus" } else { "development" },
        platform: platform.clone(),
        sibling_revision,
        thresholds: Thresholds {
            pitch_error_percent_max: PITCH_LIMIT_PERCENT,
            codec_snr_db_min: SNR_MIN_DB,
            clipping_samples_max: 0,
            stereo_lag_samples_max: 0,
            seam_over_source_max: SEAM_TOLERANCE,
            generation_latency_ms_max: latency_limit_ms(&platform),
        },
        checks,
        synthetic,
        resample: resample_results,
        corpus,
        identity_runtime,
        streaming,
    };
    let report_path = output_dir.join("report.json");
    fs::write(
        &report_path,
        serde_json::to_vec_pretty(&report).expect("serialize report"),
    )
    .expect("write report");
    println!("report: target/song-rate-validation/report.json");
    for rate in &report.synthetic {
        println!("demo {}%: {}", rate.percent, rate.demo_path);
    }
    if !overall_pass {
        std::process::exit(1);
    }
}

fn check(name: &str, passed: bool, detail: &str) -> Check {
    println!("{}  {}: {}", if passed { "PASS" } else { "FAIL" }, name, detail);
    Check {
        name: name.into(),
        passed,
        detail: detail.into(),
    }
}

fn fail(message: &str) -> ! {
    eprintln!("error: {message}");
    std::process::exit(2)
}

fn shared_format() -> SharedWaveFormat {
    SharedWaveFormat::from_packed(2 | (2 << 2) | (SAMPLE_RATE << 5) | (48 << 23))
}

fn sibling_format() -> WaveFormat {
    WaveFormat::from_packed(2 | (2 << 2) | (SAMPLE_RATE << 5) | (48 << 23))
}

fn shared_mono_format() -> SharedWaveFormat {
    SharedWaveFormat::from_packed(2 | (1 << 2) | (SAMPLE_RATE << 5) | (48 << 23))
}

fn sibling_mono_format() -> WaveFormat {
    WaveFormat::from_packed(2 | (1 << 2) | (SAMPLE_RATE << 5) | (48 << 23))
}

fn source_pcm(frequency: f64) -> Vec<i16> {
    let mut pcm = Vec::with_capacity(SOURCE_FRAMES * 2);
    for frame in 0..SOURCE_FRAMES {
        let sample = (16_000.0
            * (std::f64::consts::TAU * frequency * frame as f64 / SAMPLE_RATE as f64).sin())
            as i16;
        pcm.push(sample);
        pcm.push(sample);
    }
    pcm
}

fn build_source(preview_first: bool) -> Vec<u8> {
    build_source_with_frequencies(preview_first, 250.0, 200.0)
}

/// Synthesize a strict-profile bank at chosen sine frequencies. The on-demand
/// demo uses a second frequency pair as a different-content effective source
/// (same song code, different digest) to prove source-key invalidation.
fn build_source_with_frequencies(preview_first: bool, main_hz: f64, preview_hz: f64) -> Vec<u8> {
    let format = sibling_format();
    let main = sibling_encode::encode(&source_pcm(main_hz), &format).expect("encode main");
    let preview = sibling_encode::encode(&source_pcm(preview_hz), &format).expect("encode preview");
    let make_entry = |name: &str, duration: usize, data: Vec<u8>| XwbEntry {
        flags_and_duration: (duration as u32) << 4,
        format,
        data,
        loop_start: 0,
        loop_length: duration as u32,
        name_bytes: fixed_name(name),
    };
    // The preview's declared duration sits INSIDE its whole-block payload
    // (stock shape): real banks never land on block boundaries — the
    // 2026-08-10 HeaderSynth refusal hid behind block-exact fixtures.
    let entries = if preview_first {
        vec![
            make_entry("synt_s", PREVIEW_DURATION_FRAMES, preview),
            make_entry("synt", SOURCE_FRAMES, main),
        ]
    } else {
        vec![
            make_entry("synt", SOURCE_FRAMES, main),
            make_entry("synt_s", PREVIEW_DURATION_FRAMES, preview),
        ]
    };
    let bank = XwbBank {
        header_version: 42,
        flags: 0x0009_0001,
        name: fixed_bank_name("synt"),
        entry_name_element_size: 64,
        alignment: 2_048,
        compact_format: 0,
        build_time: 0,
        entries,
    };
    let mut bytes = Vec::new();
    sibling_xwb::write(&bank, &mut bytes).expect("write synthetic bank");
    bytes
}

fn fixed_name(name: &str) -> Vec<u8> {
    let mut bytes = vec![0; 64];
    bytes[..name.len()].copy_from_slice(name.as_bytes());
    bytes
}

fn fixed_bank_name(name: &str) -> [u8; 64] {
    let mut bytes = [0; 64];
    bytes[..name.len()].copy_from_slice(name.as_bytes());
    bytes
}

fn parsers_agree(shared: &xwb::SongBank<'_>, sibling: &XwbBank) -> bool {
    shared.header_version == sibling.header_version
        && shared.flags == sibling.flags
        && shared.name() == sibling.name_str()
        && sibling.entry_name_element_size == 64
        && shared.alignment == sibling.alignment
        && shared.compact_format == sibling.compact_format
        && shared.build_time == sibling.build_time
        && shared.entries.len() == sibling.entries.len()
        && shared.entries.iter().enumerate().all(|(index, entry)| {
            let sibling_entry = &sibling.entries[index];
            sibling_entry.flags_and_duration & 0xf == 0
                && sibling_entry.flags_and_duration >> 4 == entry.duration
                && sibling_entry.format.packed() == entry.format.packed()
                && sibling_entry.name_str() == entry.name()
                && sibling_entry.loop_start == entry.loop_start
                && sibling_entry.loop_length == entry.loop_length
                && sibling_entry.data.as_slice() == entry.data
        })
}

fn codec_comparison() -> (f64, bool) {
    let pcm = source_pcm(250.0);
    let shared = adpcm::encode_interleaved(&pcm, shared_format()).expect("shared encode");
    let sibling = sibling_encode::encode(&pcm, &sibling_format()).expect("sibling encode");
    let decoded = adpcm::decode_interleaved(&shared, shared_format(), SOURCE_FRAMES as u32)
        .expect("shared decode");
    let sibling_decoded = sibling_decode::decode(&sibling, &sibling_format()).expect("sibling decode");
    let mono: Vec<i16> = pcm.chunks_exact(2).map(|frame| frame[0]).collect();
    let shared_mono = adpcm::encode_interleaved(&mono, shared_mono_format()).expect("shared mono encode");
    let sibling_mono = sibling_encode::encode(&mono, &sibling_mono_format()).expect("sibling mono encode");
    (
        snr_db(&pcm, &decoded),
        shared == sibling && decoded == sibling_decoded && shared_mono == sibling_mono,
    )
}

/// One entry's whole-bank transform evidence (validation-only summary).
struct EntrySummary {
    source_frames: u64,
    output_frames: u64,
    clipped_samples: usize,
    /// Encoded loop-seam delta, recomputed from the GENERATED bank.
    seam_max_delta: Option<i32>,
    source_seam_max_delta: Option<i32>,
}

struct TransformSummary {
    entries: [EntrySummary; 2],
    main_entry_index: usize,
    effective_rate: RateRatio,
    output_digest: core::xact::digest::Digest128,
    /// The PLANNED serialized length (xwb::serialized_song_bank_len) —
    /// compared against the actually written bytes as a postcondition.
    output_length: u64,
}

fn seam_max_delta_pcm(samples: &[i16], channels: usize, loop_start: usize, loop_end: usize) -> i32 {
    (0..channels)
        .map(|channel| {
            let first = i32::from(samples[loop_start * channels + channel]);
            let last = i32::from(samples[(loop_end - 1) * channels + channel]);
            (first - last).abs()
        })
        .max()
        .unwrap_or(0)
}

/// Whole-bank rate transform assembled from the crate's surviving pure
/// primitives (parse -> plan -> decode -> stretch -> encode -> stream-write).
/// Validation-only: the shipped engine streams incrementally; this keeps an
/// end-to-end whole-file oracle for the DSP checks (pitch, SNR, seams,
/// determinism, identity preservation).
fn transform_bank(
    source: &[u8],
    song_code: &str,
    percent: u32,
) -> Result<(Vec<u8>, TransformSummary), String> {
    let bank = xwb::parse_song_bank(source).map_err(|error| error.to_string())?;
    if bank.name() != song_code {
        return Err(format!(
            "song code {song_code:?} does not match bank {:?}",
            bank.name()
        ));
    }
    let main_entry_index = bank
        .entries
        .iter()
        .position(|entry| entry.name() == song_code)
        .ok_or("main entry missing")?;
    let plans = [
        virtual_bank::plan_entry(0, &bank.entries[0], percent).map_err(|error| error.to_string())?,
        virtual_bank::plan_entry(1, &bank.entries[1], percent).map_err(|error| error.to_string())?,
    ];
    let streamed = [plans[0].streamed, plans[1].streamed];
    let output_length = xwb::serialized_song_bank_len(&bank, &streamed)
        .map_err(|error| error.to_string())? as u64;

    let mut encoded: Vec<Vec<u8>> = Vec::new();
    let mut clipped = [0usize; 2];
    let mut source_seams: [Option<i32>; 2] = [None, None];
    for (index, entry) in bank.entries.iter().enumerate() {
        let plan = &plans[index];
        let decoded = adpcm::decode_interleaved(entry.data, entry.format, entry.duration)
            .map_err(|error| error.to_string())?;
        let channels = entry.format.channels() as usize;
        let stretched = stretch::stretch_interleaved(
            &decoded,
            channels,
            entry.format.sample_rate(),
            plan.streamed.duration as usize,
            plan.loop_context,
        )
        .map_err(|error| format!("{error:?}"))?;
        clipped[index] = stretched.clipped_samples;
        source_seams[index] = plan
            .loop_context
            .map(|context| seam_max_delta_pcm(&decoded, channels, context.source_start, context.source_end));
        encoded.push(
            adpcm::encode_interleaved(&stretched.samples, entry.format)
                .map_err(|error| error.to_string())?,
        );
    }

    let mut output = Cursor::new(Vec::new());
    xwb::write_song_bank_streaming(&bank, &streamed, &mut output, |index, entry_output| {
        entry_output
            .write_all(&encoded[index])
            .map_err(|error| error.to_string())
    })
    .map_err(|error| format!("{error:?}"))?;
    let output_bytes = output.into_inner();

    // Recompute the encoded loop seams from the generated bank (the retired
    // transformer's ValidateOutput postcondition, preserved here).
    let generated = xwb::parse_song_bank(&output_bytes).map_err(|error| error.to_string())?;
    let mut seams: [Option<i32>; 2] = [None, None];
    for index in 0..2 {
        if plans[index].loop_context.is_none() {
            continue;
        }
        let entry = &generated.entries[index];
        let decoded = adpcm::decode_interleaved(entry.data, entry.format, entry.duration)
            .map_err(|error| error.to_string())?;
        let loop_end = entry.loop_start as usize + entry.loop_length as usize;
        seams[index] = Some(seam_max_delta_pcm(
            &decoded,
            entry.format.channels() as usize,
            entry.loop_start as usize,
            loop_end,
        ));
    }

    let summary = TransformSummary {
        entries: [
            EntrySummary {
                source_frames: u64::from(bank.entries[0].duration),
                output_frames: u64::from(streamed[0].duration),
                clipped_samples: clipped[0],
                seam_max_delta: seams[0],
                source_seam_max_delta: source_seams[0],
            },
            EntrySummary {
                source_frames: u64::from(bank.entries[1].duration),
                output_frames: u64::from(streamed[1].duration),
                clipped_samples: clipped[1],
                seam_max_delta: seams[1],
                source_seam_max_delta: source_seams[1],
            },
        ],
        main_entry_index,
        effective_rate: plans[main_entry_index].rate,
        output_digest: digest::md5_bytes(&output_bytes),
        output_length,
    };
    Ok((output_bytes, summary))
}

fn validate_rate(
    source: &[u8],
    percent: u32,
    codec_snr_db: f64,
    output_dir: &Path,
    label: &str,
    platform: &str,
) -> RateResult {
    let source_bank = xwb::parse_song_bank(source).expect("parse validation source");
    let main_index = source_bank
        .entries
        .iter()
        .position(|entry| entry.name() == source_bank.name())
        .expect("main entry");
    let preview_index = 1 - main_index;
    let source_main = adpcm::decode_interleaved(
        source_bank.entries[main_index].data,
        source_bank.entries[main_index].format,
        source_bank.entries[main_index].duration,
    )
    .expect("decode source main");

    let ((first, report), latency, peak_working_set_bytes, additional_peak_working_set_bytes) =
        measure_operation(|| {
            transform_bank(source, source_bank.name(), percent).expect("transform synthetic bank")
        });
    let (second, second_report) =
        transform_bank(source, source_bank.name(), percent).expect("repeat transform");
    let deterministic = first == second && report.output_digest == second_report.output_digest;
    let generated = xwb::parse_song_bank(&first).expect("parse generated bank");
    let sibling_generated = sibling_xwb::parse(&first).expect("sibling parses generated bank");
    let generated_main = adpcm::decode_interleaved(
        generated.entries[main_index].data,
        generated.entries[main_index].format,
        generated.entries[main_index].duration,
    )
    .expect("decode generated main");
    let source_frequency = estimate_frequency(&source_main, 2, 0, SAMPLE_RATE);
    let output_frequency = estimate_frequency(&generated_main, 2, 0, SAMPLE_RATE);
    let pitch_error_percent = ((output_frequency - source_frequency) / source_frequency).abs() * 100.0;
    let source_stereo_lag_samples = stereo_lag(&source_main, 2);
    let stereo_lag_samples = stereo_lag(&generated_main, 2);
    let stereo_lag_delta_samples = (stereo_lag_samples - source_stereo_lag_samples).abs();
    let clipping_samples = report.entries.iter().map(|entry| entry.clipped_samples).sum();
    let preview_seam_delta = report.entries[preview_index].seam_max_delta.unwrap_or(0);
    let preview_source_seam_delta = report.entries[preview_index]
        .source_seam_max_delta
        .unwrap_or(0);
    let identity_preserved = generated.name() == source_bank.name()
        && generated.flags == source_bank.flags
        && generated.alignment == source_bank.alignment
        && generated
            .entries
            .iter()
            .zip(&source_bank.entries)
            .all(|(left, right)| left.name() == right.name() && left.format == right.format)
        && parsers_agree(&generated, &sibling_generated);
    let passed = pitch_error_percent <= PITCH_LIMIT_PERCENT
        && codec_snr_db >= SNR_MIN_DB
        && clipping_samples == 0
        && stereo_lag_delta_samples == 0
        && preview_seam_delta <= preview_source_seam_delta + SEAM_TOLERANCE
        && deterministic
        && identity_preserved
        && latency <= latency_limit_ms(platform)
        && report.output_length == first.len() as u64;

    let filename = format!("{label}-{percent}.xwb");
    fs::write(output_dir.join(&filename), &first).expect("write demo XWB");
    RateResult {
        label: label.into(),
        percent,
        source_frames: report.entries[main_index].source_frames,
        output_frames: report.entries[main_index].output_frames,
        rate_numerator: report.effective_rate.source_frames,
        rate_denominator: report.effective_rate.output_frames,
        pitch_error_percent,
        codec_snr_db,
        clipping_samples,
        source_stereo_lag_samples,
        stereo_lag_samples,
        stereo_lag_delta_samples,
        preview_seam_delta,
        preview_source_seam_delta,
        deterministic,
        identity_preserved,
        peak_working_set_bytes,
        additional_peak_working_set_bytes,
        generation_latency_ms: latency,
        input_digest: digest::md5_bytes(source).to_hex(),
        output_digest: report.output_digest.to_hex(),
        demo_path: format!("target/song-rate-validation/demo/{filename}"),
        passed,
    }
}

/// Preserve-pitch-OFF leg: decode the main entry, resample it through the
/// PLAN's output geometry (block-quantized frames + loop context — the same
/// contract the streaming producer follows), round-trip through the codec,
/// and assert the inverted pitch expectation. The streaming state's
/// byte-identity to this reference is proven by the mounted cargo tests
/// (`resample_*`, `resample_mode_*`); this leg validates the acoustic
/// contract and determinism at the report level.
fn validate_resample(source: &[u8], percent: u32, codec_snr_db: f64, platform: &str) -> ResampleResult {
    let bank = xwb::parse_song_bank(source).expect("parse resample source");
    let main_index = bank
        .entries
        .iter()
        .position(|entry| entry.name() == bank.name())
        .expect("main entry");
    let entry = &bank.entries[main_index];
    let source_main =
        adpcm::decode_interleaved(entry.data, entry.format, entry.duration).expect("decode main");
    let layout = virtual_bank::plan_virtual_bank(&bank, percent, virtual_bank::StretchTarget::Main).expect("plan resample bank");
    let plan = &layout.entries[main_index];
    let output_frames = plan.streamed.duration as usize;
    let channels = entry.format.channels() as usize;

    let (first, latency, _, _) = measure_operation(|| {
        resample::resample_interleaved(&source_main, channels, output_frames, plan.loop_context)
            .expect("reference resample")
    });
    let second =
        resample::resample_interleaved(&source_main, channels, output_frames, plan.loop_context)
            .expect("repeat resample");
    let deterministic = first == second;
    let exact_output_length = first.len() == output_frames * channels;

    // Codec round trip: the served stream is ADPCM, so measure through it.
    let encoded = adpcm::encode_interleaved(&first, entry.format).expect("encode resample");
    let roundtrip = adpcm::decode_interleaved(&encoded, entry.format, output_frames as u32)
        .expect("decode resample roundtrip");

    let rate = plan.rate;
    let source_frequency_hz = zero_crossing_frequency(&source_main, channels, 0, SAMPLE_RATE);
    let output_frequency_hz = zero_crossing_frequency(&roundtrip, channels, 0, SAMPLE_RATE);
    let expected_output_frequency_hz =
        source_frequency_hz * rate.source_frames as f64 / rate.output_frames as f64;
    let pitch_tracking_error_percent =
        ((output_frequency_hz - expected_output_frequency_hz) / expected_output_frequency_hz).abs()
            * 100.0;
    let passed = pitch_tracking_error_percent <= PITCH_LIMIT_PERCENT
        && codec_snr_db >= SNR_MIN_DB
        && deterministic
        && exact_output_length
        && latency <= latency_limit_ms(platform);

    ResampleResult {
        percent,
        source_frames: rate.source_frames,
        output_frames: rate.output_frames,
        rate_numerator: rate.source_frames,
        rate_denominator: rate.output_frames,
        source_frequency_hz,
        output_frequency_hz,
        expected_output_frequency_hz,
        pitch_tracking_error_percent,
        codec_snr_db,
        deterministic,
        exact_output_length,
        generation_latency_ms: latency,
        passed,
    }
}

fn validate_corpus(
    root: &Path,
    relative: &Path,
    output_dir: &Path,
    platform: &str,
) -> CorpusResult {
    let full_path = root.join(relative);
    let relative_string = relative.to_string_lossy().replace('\\\\', "/");
    let custom_named = relative_string.to_ascii_lowercase().contains("custom");
    let bytes = match fs::read(&full_path) {
        Ok(bytes) => bytes,
        Err(error) => {
            return CorpusResult {
                relative_path: relative_string,
                input_digest: String::new(),
                entry_order: "unknown".into(),
                custom_named,
                rates: Vec::new(),
                passed: false,
                error: Some(error.to_string()),
            }
        }
    };
    let input_digest = digest::md5_bytes(&bytes).to_hex();
    let bank = match xwb::parse_song_bank(&bytes) {
        Ok(bank) => bank,
        Err(error) => {
            return CorpusResult {
                relative_path: relative_string,
                input_digest,
                entry_order: "unsupported".into(),
                custom_named,
                rates: Vec::new(),
                passed: false,
                error: Some(error.to_string()),
            }
        }
    };
    let sibling_bank = match sibling_xwb::parse(&bytes) {
        Ok(bank) => bank,
        Err(_) => {
            return CorpusResult {
                relative_path: relative_string,
                input_digest,
                entry_order: "unknown".into(),
                custom_named,
                rates: Vec::new(),
                passed: false,
                error: Some("sibling parser rejected supported bank".into()),
            }
        }
    };
    if !parsers_agree(&bank, &sibling_bank) {
        return CorpusResult {
            relative_path: relative_string,
            input_digest,
            entry_order: "unknown".into(),
            custom_named,
            rates: Vec::new(),
            passed: false,
            error: Some("shared and sibling parsers disagree on source bank".into()),
        };
    }
    let entry_order = if bank.entries[0].name() == bank.name() {
        "main-preview"
    } else {
        "preview-main"
    };
    let main_index = bank
        .entries
        .iter()
        .position(|entry| entry.name() == bank.name())
        .expect("corpus main entry");
    let source_names = [
        bank.entries[0].name().to_owned(),
        bank.entries[1].name().to_owned(),
    ];
    let source_formats = [bank.entries[0].format, bank.entries[1].format];
    let source_main = adpcm::decode_interleaved(
        bank.entries[main_index].data,
        bank.entries[main_index].format,
        bank.entries[main_index].duration,
    )
    .expect("decode corpus main");
    let source_frequency = estimate_frequency(
        &source_main,
        bank.entries[main_index].format.channels() as usize,
        0,
        bank.entries[main_index].format.sample_rate(),
    );
    let codec_snr_db = codec_snr_for_pcm(&source_main, bank.entries[main_index].format);
    let source_stereo_lag_samples = stereo_lag(
        &source_main,
        bank.entries[main_index].format.channels() as usize,
    );
    let safe_name: String = relative_string
        .chars()
        .map(|character| if character.is_ascii_alphanumeric() { character } else { '_' })
        .collect();
    let code = bank.name().to_owned();
    drop(bank);
    let mut rates = Vec::new();
    for percent in [75, 125] {
        let (transform_result, latency, peak_working_set_bytes, additional_peak_working_set_bytes) =
            measure_operation(|| transform_bank(&bytes, &code, percent));
        let (first, report) = match transform_result {
            Ok(result) => result,
            Err(error) => {
                return CorpusResult {
                    relative_path: relative_string,
                    input_digest,
                    entry_order: entry_order.into(),
                    custom_named,
                    rates,
                    passed: false,
                    error: Some(error),
                }
            }
        };
        let deterministic = transform_bank(&bytes, &code, percent)
            .map(|(second, second_report)| {
                first == second && report.output_digest == second_report.output_digest
            })
            .unwrap_or(false);
        let generated = xwb::parse_song_bank(&first).expect("generated corpus output reparses");
        let sibling_generated = sibling_xwb::parse(&first).expect("sibling parses generated corpus output");
        let identity_preserved = generated.name() == code
            && generated.entries.iter().enumerate().all(|(index, entry)| {
                entry.name() == source_names[index] && entry.format == source_formats[index]
            })
            && parsers_agree(&generated, &sibling_generated);
        let generated_main = adpcm::decode_interleaved(
            generated.entries[main_index].data,
            generated.entries[main_index].format,
            generated.entries[main_index].duration,
        )
        .expect("decode generated corpus main");
        let generated_frequency = estimate_frequency(
            &generated_main,
            generated.entries[main_index].format.channels() as usize,
            0,
            generated.entries[main_index].format.sample_rate(),
        );
        let pitch_error_percent = if source_frequency > 0.0 {
            ((generated_frequency - source_frequency) / source_frequency).abs() * 100.0
        } else {
            f64::INFINITY
        };
        let stereo_lag_samples = stereo_lag(
            &generated_main,
            generated.entries[main_index].format.channels() as usize,
        );
        let stereo_lag_delta_samples =
            (stereo_lag_samples - source_stereo_lag_samples).abs();
        let clipping_samples = report.entries.iter().map(|entry| entry.clipped_samples).sum();
        let preview_index = generated
            .entries
            .iter()
            .position(|entry| entry.name().ends_with("_s"))
            .unwrap_or(0);
        let seam = report.entries[preview_index].seam_max_delta;
        let source_seam = report.entries[preview_index].source_seam_max_delta;
        let passed = deterministic
            && identity_preserved
            && clipping_samples == 0
            && seam.zip(source_seam).is_none_or(|(output, source)| output <= source + SEAM_TOLERANCE)
            && pitch_error_percent <= PITCH_LIMIT_PERCENT
            && codec_snr_db >= SNR_MIN_DB
            && stereo_lag_delta_samples == 0
            && latency <= latency_limit_ms(platform);
        let filename = format!("{safe_name}-{}-{percent}.xwb", &input_digest[..12]);
        fs::write(output_dir.join(&filename), &first).expect("write corpus demo");
        rates.push(CorpusRateResult {
            percent,
            output_digest: report.output_digest.to_hex(),
            source_frames: report.entries[report.main_entry_index].source_frames,
            output_frames: report.entries[report.main_entry_index].output_frames,
            rate_numerator: report.effective_rate.source_frames,
            rate_denominator: report.effective_rate.output_frames,
            clipping_samples,
            preview_seam_delta: seam,
            preview_source_seam_delta: source_seam,
            pitch_error_percent,
            codec_snr_db,
            source_stereo_lag_samples,
            stereo_lag_samples,
            stereo_lag_delta_samples,
            peak_working_set_bytes,
            additional_peak_working_set_bytes,
            generation_latency_ms: latency,
            deterministic,
            identity_preserved,
            demo_path: format!("target/song-rate-validation/corpus/{filename}"),
            passed,
        });
    }
    let passed = rates.len() == 2 && rates.iter().all(|rate| rate.passed);
    CorpusResult {
        relative_path: relative_string,
        input_digest,
        entry_order: entry_order.into(),
        custom_named,
        rates,
        passed,
        error: None,
    }
}

fn collect_xwbs(root: &Path, directory: &Path, output: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(directory) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_xwbs(root, &path, output);
        } else if path.extension().and_then(|value| value.to_str()).is_some_and(|value| value.eq_ignore_ascii_case("xwb")) {
            if let Ok(relative) = path.strip_prefix(root) {
                output.push(relative.to_owned());
            }
        }
    }
}

fn snr_db(source: &[i16], decoded: &[i16]) -> f64 {
    let (mut signal, mut noise) = (0.0, 0.0);
    for (&source, &decoded) in source.iter().zip(decoded) {
        let source = f64::from(source);
        let error = source - f64::from(decoded);
        signal += source * source;
        noise += error * error;
    }
    if noise == 0.0 { 300.0 } else { 10.0 * (signal / noise).log10() }
}

/// Fundamental frequency via mean positive-going zero-crossing spacing.
/// Used by the RESAMPLE legs only: their expectation is a pitch RATIO
/// (`f_out = f_src × S/O`), which needs a fundamental-true measure —
/// `estimate_frequency`'s bounded-lag autocorrelation folds to subharmonics
/// (fine for the stretch legs, whose pitch must NOT move, but it saturates
/// at its lag ceiling when the true fundamental halves).
fn zero_crossing_frequency(
    samples: &[i16],
    channels: usize,
    channel: usize,
    sample_rate: u32,
) -> f64 {
    let frames = samples.len() / channels;
    let mut crossings: Vec<usize> = Vec::new();
    for frame in 1..frames {
        let previous = samples[(frame - 1) * channels + channel];
        let current = samples[frame * channels + channel];
        if previous < 0 && current >= 0 {
            crossings.push(frame);
        }
    }
    if crossings.len() < 8 {
        return 0.0;
    }
    let spans = (crossings.len() - 1) as f64;
    let mean_period = (crossings[crossings.len() - 1] - crossings[0]) as f64 / spans;
    f64::from(sample_rate) / mean_period
}

fn estimate_frequency(samples: &[i16], channels: usize, channel: usize, sample_rate: u32) -> f64 {
    let frames = samples.len() / channels;
    let window_len = frames.min(8_192);
    if window_len < 64 {
        return 0.0;
    }
    let start = (frames - window_len) / 2;
    let mut signal: Vec<f64> = (0..window_len)
        .map(|frame| f64::from(samples[(start + frame) * channels + channel]))
        .collect();
    let mean = signal.iter().sum::<f64>() / signal.len() as f64;
    for sample in &mut signal {
        *sample -= mean;
    }
    let minimum_lag = (sample_rate as usize / 2_000).max(2);
    let maximum_lag = (sample_rate as usize / 40).min(window_len / 2);
    if minimum_lag >= maximum_lag {
        return 0.0;
    }
    let mut scores = Vec::with_capacity(maximum_lag - minimum_lag + 1);
    for lag in minimum_lag..=maximum_lag {
        let mut cross = 0.0;
        let mut left_power = 0.0;
        let mut right_power = 0.0;
        for frame in 0..window_len - lag {
            let left = signal[frame];
            let right = signal[frame + lag];
            cross += left * right;
            left_power += left * left;
            right_power += right * right;
        }
        let denominator = (left_power * right_power).sqrt();
        scores.push(if denominator > 0.0 { cross / denominator } else { -1.0 });
    }
    let best_index = scores
        .iter()
        .enumerate()
        .max_by(|left, right| left.1.total_cmp(right.1))
        .map(|(index, _)| index)
        .unwrap_or(0);
    let mut lag = (minimum_lag + best_index) as f64;
    if best_index > 0 && best_index + 1 < scores.len() {
        let left = scores[best_index - 1];
        let center = scores[best_index];
        let right = scores[best_index + 1];
        let curvature = left - 2.0 * center + right;
        if curvature.abs() > f64::EPSILON {
            lag += 0.5 * (left - right) / curvature;
        }
    }
    sample_rate as f64 / lag
}

fn stereo_lag(samples: &[i16], channels: usize) -> i32 {
    let frames = samples.len() / channels;
    let mut best = (i128::MIN, 0i32);
    for lag in -8i32..=8 {
        let mut score = 0i128;
        for frame in 8..frames.saturating_sub(8) {
            let right_frame = frame as i32 + lag;
            if right_frame < 0 || right_frame >= frames as i32 { continue; }
            score += i128::from(samples[frame * channels])
                * i128::from(samples[right_frame as usize * channels + 1]);
        }
        if score > best.0 {
            best = (score, lag);
        }
    }
    best.1
}

fn codec_snr_for_pcm(pcm: &[i16], format: SharedWaveFormat) -> f64 {
    let channels = format.channels() as usize;
    let samples_per_block = format.samples_per_block() as usize;
    let frames = pcm.len() / channels;
    let aligned_frames = frames / samples_per_block * samples_per_block;
    if aligned_frames == 0 {
        return 0.0;
    }
    let source = &pcm[..aligned_frames * channels];
    let encoded = adpcm::encode_interleaved(source, format).expect("corpus SNR encode");
    let decoded = adpcm::decode_interleaved(&encoded, format, aligned_frames as u32)
        .expect("corpus SNR decode");
    snr_db(source, &decoded)
}

fn validate_identity_runtime() -> IdentityRuntimeReport {
    let factor: &'static AtomicU64 = Box::leak(Box::new(AtomicU64::new(IDENTITY_Q31)));
    let publication = RatePublication::new(factor);
    publication.publish_identity(7, 3);
    let snapshot = publication.read();
    publication.reset_identity();

    let slots = XactSlots::new();
    let slot = slots.claim(1, 1, 1, 1).expect("identity slot claim");
    slots.abandon(slot).expect("identity slot cleanup");
    let frame = enter_frame(1).expect("identity TLS frame");
    drop(frame);
    let queue = MaintenanceQueue::<4>::new();

    let mut original_calls = 0usize;
    let wave_result = call_create_identity(
        1,
        |_| {
            original_calls += 1;
            1
        },
        || {},
    );

    let movie = MoviePolicy::new();
    let mut movie_original_calls = 0usize;
    let mut player = [0u8; 32];
    let (original_result, _) = unsafe {
        movie.call(
            player.as_mut_ptr().cast(),
            std::ptr::null_mut(),
            |_, _| {
                movie_original_calls += 1;
                0x1234
            },
        )
    };
    movie.set(MovieSuppressor::NonNativeOs, true);
    let (suppressed_result, _) = unsafe {
        movie.call(player.as_mut_ptr().cast(), std::ptr::null_mut(), |_, _| {
            movie_original_calls += 1;
            0x5678
        })
    };

    let checks = vec![
        check(
            "identity_clock",
            [i32::MIN, -1, 0, 1, i32::MAX]
                .into_iter()
                .all(|value| scale_music_count_q31(value, IDENTITY_Q31) == value)
                && factor.load(Ordering::Acquire) == IDENTITY_Q31,
            "identity Q31 preserves signed boundaries",
        ),
        check(
            "identity_snapshot_reset",
            snapshot.requested_percent == 100
                && !snapshot.committed
                && publication.read().generation == 0,
            "seqlock snapshot is coherent and reset returns identity",
        ),
        check(
            "identity_tls_slots_queue",
            slots.phase(slot) == Some(services::song_rate::xact_runtime::XactSlotPhase::Free)
                && services::song_rate::xact_runtime::current_frame().is_none()
                && queue.pop().is_none(),
            "TLS, XACT slots, and maintenance queue are empty after identity calls",
        ),
        check(
            "identity_wave_exactly_once",
            wave_result == 1 && original_calls == 1,
            "identity wave wrapper calls original exactly once",
        ),
        check(
            "identity_no_dynamic_redirect",
            identity_conversion_path("data/sound/win/dance/synt.xwb", "stock.xwb").is_none(),
            "LayeredFS song-rate seam returns no dynamic replacement",
        ),
        check(
            "identity_hook_rollback",
            true,
            "focused host tests cover every transactional hook rollback position",
        ),
        check(
            "identity_movie_policy",
            original_result == 0x1234
                && suppressed_result == 0
                && movie_original_calls == 1
                && u32::from_le_bytes(player[8..12].try_into().unwrap()) == 3
                && !movie.is_suppressed(MovieSuppressor::SongRate),
            "movie contributors preserve original behavior and song-rate starts false",
        ),
    ];
    IdentityRuntimeReport {
        passed: checks.iter().all(|check| check.passed),
        checks,
    }
}

/// Drive a StretchState to completion over the on-demand block-cache view
/// with a fixed produce-chunk size (frames). Returns (samples, clipped,
/// cyclic).
fn drive_streaming(
    view: &adpcm::BlockCachePcm<'_>,
    source_frames: usize,
    output_frames: usize,
    channels: usize,
    sample_rate: u32,
    loop_context: Option<stretch::LoopContext>,
    chunk: usize,
) -> (Vec<i16>, usize, usize) {
    let mut state = StretchState::new(
        source_frames,
        output_frames,
        channels,
        sample_rate,
        loop_context,
    )
    .expect("streaming state");
    let mut samples = Vec::new();
    loop {
        let mut out = vec![0i16; chunk.max(1) * channels];
        let produced = state.produce(view, &mut out).expect("streaming produce");
        samples.extend_from_slice(&out[..produced.frames * channels]);
        if produced.done {
            break;
        }
    }
    (samples, state.clipped_samples(), state.cyclic_windows())
}

/// Streaming-core evidence (plan Step 2 demo): byte equality vs the untouched
/// whole-buffer reference, chunking independence, and checkpoint/restore
/// across the full rate matrix, exercised through the real planning path
/// (virtual_bank::plan_entry, which carries the production full-entry loop
/// shape) and the real on-demand decode view (adpcm::BlockCachePcm) over the
/// synthetic bank's main entry. The throughput figure is informational only —
/// the real gate is the plan Step 5 live cabinet benchmark.
fn validate_streaming(source: &[u8], reverse_order: &[u8]) -> StreamingReport {
    let bank = xwb::parse_song_bank(source).expect("streaming source parse");
    let main_index = bank
        .entries
        .iter()
        .position(|entry| entry.name() == bank.name())
        .expect("streaming main entry");
    let entry = &bank.entries[main_index];
    let source_frames = entry.duration as usize;
    let channels = entry.format.channels() as usize;
    let sample_rate = entry.format.sample_rate();
    let decoded = adpcm::decode_interleaved(entry.data, entry.format, entry.duration)
        .expect("decode streaming source entry");
    let hop = stretch::StretchParameters::for_sample_rate(sample_rate)
        .expect("stretch parameters")
        .synthesis_hop;

    let mut checks = Vec::new();
    let mut rates = Vec::new();
    for percent in [25u32, 50, 75, 100, 125, 175] {
        let plan = virtual_bank::plan_entry(main_index, entry, percent).expect("streaming plan");
        let output_frames = plan.streamed.duration as usize;
        let loop_context = plan.loop_context;
        let reference = stretch::stretch_interleaved(
            &decoded,
            channels,
            sample_rate,
            output_frames,
            loop_context,
        )
        .expect("streaming reference stretch");
        let view = adpcm::BlockCachePcm::new(entry.data, entry.format, entry.duration)
            .expect("streaming block-cache view");

        let (whole, clipped, cyclic) = drive_streaming(
            &view,
            source_frames,
            output_frames,
            channels,
            sample_rate,
            loop_context,
            output_frames,
        );
        let byte_equality = whole == reference.samples;
        let counters_match =
            clipped == reference.clipped_samples && cyclic == reference.cyclic_windows;
        let (chunked, _, _) = drive_streaming(
            &view,
            source_frames,
            output_frames,
            channels,
            sample_rate,
            loop_context,
            997,
        );
        let chunking_independent = chunked == reference.samples;

        // Checkpoint at the first boundary past the midpoint, restore, and
        // compare the regenerated suffix against the uninterrupted run.
        let mut state = StretchState::new(
            source_frames,
            output_frames,
            channels,
            sample_rate,
            loop_context,
        )
        .expect("checkpoint state");
        let mut captured = None;
        loop {
            let mut out = vec![0i16; hop * channels];
            let produced = state.produce(&view, &mut out).expect("checkpoint produce");
            if captured.is_none() {
                if let Some(checkpoint) = state.checkpoint() {
                    if checkpoint.resume_frame() >= output_frames / 2 {
                        captured = Some(checkpoint);
                    }
                }
            }
            if produced.done {
                break;
            }
        }
        let checkpoint_restore = match captured {
            None => false,
            Some(checkpoint) => {
                let resume = checkpoint.resume_frame();
                let mut restored = StretchState::restore(
                    &checkpoint,
                    source_frames,
                    output_frames,
                    channels,
                    sample_rate,
                    loop_context,
                    &view,
                )
                .expect("checkpoint restore");
                let mut suffix = Vec::new();
                loop {
                    let mut out = vec![0i16; 1_024 * channels];
                    let produced = restored.produce(&view, &mut out).expect("suffix produce");
                    suffix.extend_from_slice(&out[..produced.frames * channels]);
                    if produced.done {
                        break;
                    }
                }
                suffix.as_slice() == &reference.samples[resume * channels..]
            }
        };

        let passed =
            byte_equality && counters_match && chunking_independent && checkpoint_restore;
        checks.push(check(
            &format!("streaming_{percent}"),
            passed,
            &format!(
                "loop=full-entry, output_frames={output_frames}, bytes={byte_equality}, counters={counters_match}, chunking={chunking_independent}, checkpoint={checkpoint_restore}"
            ),
        ));
        rates.push(StreamingRateResult {
            percent,
            loop_shape: "full-entry".into(),
            source_frames: u64::from(entry.duration),
            output_frames: output_frames as u64,
            byte_equality,
            counters_match,
            chunking_independent,
            checkpoint_restore,
            passed,
        });
    }

    // Informational throughput: repeated whole 75 percent stretches through
    // the on-demand view; latency is clamped to one millisecond so the figure
    // stays finite for serialization. Recorded, never gated.
    let plan = virtual_bank::plan_entry(main_index, entry, 75).expect("throughput plan");
    let throughput_output = plan.streamed.duration as usize;
    let iterations = 24usize;
    let (total_frames, latency_ms, _, _) = measure_operation(|| {
        let mut total = 0usize;
        for _ in 0..iterations {
            let view = adpcm::BlockCachePcm::new(entry.data, entry.format, entry.duration)
                .expect("throughput view");
            let (samples, _, _) = drive_streaming(
                &view,
                source_frames,
                throughput_output,
                channels,
                sample_rate,
                plan.loop_context,
                throughput_output,
            );
            total += samples.len() / channels;
        }
        total
    });
    let synthetic_frames_per_second = total_frames as f64 * 1_000.0 / latency_ms.max(1) as f64;
    checks.push(check(
        "streaming_throughput",
        true,
        &format!(
            "informational: {synthetic_frames_per_second:.0} frames/sec synthetic (never gates)"
        ),
    ));

    // Plan Step 3 demo: full synthetic engine replays at the demo rates —
    // the virtual bank served packet-by-packet through resolve, reassembled,
    // reparsed, decoded, and matched against the whole-buffer oracle. One
    // leg runs the preview-first fixture for entry-order coverage. The
    // exhaustive replay proof lives in the harness cargo-test phase; this
    // is its compact, independently re-derived release-run form.
    let mut replays = Vec::new();
    for (percent, bytes, entry_order) in [
        (50u32, source, "main-preview"),
        (175u32, reverse_order, "preview-main"),
    ] {
        let result = replay_virtual_bank(bytes, percent, entry_order);
        checks.push(check(
            &format!("streaming_replay_{percent}"),
            result.passed,
            &format!(
                "order={}, packets={}, reassembled={}B, pattern={}, reparse={}, bytes_vs_oracle={}, decode={}",
                result.entry_order,
                result.packet_count,
                result.reassembled_len,
                result.read_pattern,
                result.reparsed,
                result.matches_reference,
                result.decode_equality
            ),
        ));
        replays.push(result);
    }

    StreamingReport {
        passed: rates.iter().all(|rate| rate.passed)
            && replays.iter().all(|replay| replay.passed)
            && checks.iter().all(|check| check.passed),
        synthetic_frames_per_second,
        rates,
        replays,
        checks,
    }
}

/// Compact release-run re-derivation of the synthetic engine replay (the
/// exhaustive pump lives in the cargo-test phase): plan the virtual bank,
/// build the MAIN entry's bytes through the StretchState + encode_block
/// feed over the on-demand BlockCachePcm view (the non-main entry is the
/// verbatim preview passthrough), then serve the RE-pinned read pattern
/// through resolve — one 0x1000 header read at offset 0, per entry
/// sequential block-align-rounded 64 KiB packets bounded to the stream,
/// one defensive read past the end — and compare the reassembly against
/// the passthrough-aware whole-buffer oracle.
fn replay_virtual_bank(source: &[u8], percent: u32, entry_order: &str) -> StreamingReplayResult {
    let bank = xwb::parse_song_bank(source).expect("replay source parse");
    let layout = virtual_bank::plan_virtual_bank(&bank, percent, virtual_bank::StretchTarget::Main).expect("replay plan");

    // The whole-buffer replay oracle mirrors the virtual bank's serving
    // composition: the MAIN entry through the reference stretch, the
    // non-main entry passed through VERBATIM (the preview passthrough —
    // the header advertises stock values and the bytes are the resident
    // source's own).
    let oracle = {
        let mut encoded: Vec<Vec<u8>> = Vec::new();
        for (index, entry) in bank.entries.iter().enumerate() {
            if index != layout.main_entry_index {
                encoded.push(entry.data.to_vec());
                continue;
            }
            let plan = &layout.entries[index];
            let decoded = adpcm::decode_interleaved(entry.data, entry.format, entry.duration)
                .expect("replay oracle decode");
            let stretched = stretch::stretch_interleaved(
                &decoded,
                entry.format.channels() as usize,
                entry.format.sample_rate(),
                plan.streamed.duration as usize,
                plan.loop_context,
            )
            .expect("replay oracle stretch");
            encoded.push(
                adpcm::encode_interleaved(&stretched.samples, entry.format)
                    .expect("replay oracle encode"),
            );
        }
        let streamed = [layout.entries[0].streamed, layout.entries[1].streamed];
        let mut output = Cursor::new(Vec::new());
        xwb::write_song_bank_streaming(&bank, &streamed, &mut output, |index, entry_output| {
            entry_output.write_all(&encoded[index])
        })
        .expect("replay oracle write");
        output.into_inner()
    };

    let mut feeds: Vec<Vec<u8>> = Vec::new();
    for (index, entry) in bank.entries.iter().enumerate() {
        if index != layout.main_entry_index {
            // Verbatim passthrough: the serving layer copies the stock
            // bytes; the replay feed is the same slice.
            feeds.push(entry.data.to_vec());
            continue;
        }
        let plan = &layout.entries[index];
        let view = adpcm::BlockCachePcm::new(entry.data, entry.format, entry.duration)
            .expect("replay source view");
        let (samples, _, _) = drive_streaming(
            &view,
            entry.duration as usize,
            plan.streamed.duration as usize,
            entry.format.channels() as usize,
            entry.format.sample_rate(),
            plan.loop_context,
            997,
        );
        let block_samples =
            entry.format.samples_per_block() as usize * entry.format.channels() as usize;
        let mut encoded = Vec::new();
        for block in samples.chunks(block_samples) {
            adpcm::encode_block(block, entry.format, &mut encoded).expect("replay encode block");
        }
        assert_eq!(
            encoded.len(),
            plan.streamed.data_len,
            "replay feed length diverges from the plan"
        );
        feeds.push(encoded);
    }

    let virtual_size = layout.virtual_size;
    let mut file = vec![0u8; virtual_size as usize];
    let serve = |file: &mut Vec<u8>, offset: u64, len: u32| -> u32 {
        let mut served = 0u32;
        while served < len {
            let position = offset + u64::from(served);
            let span = layout.resolve(position, len - served);
            if span.len == 0 {
                break;
            }
            let start = position as usize;
            let end = start + span.len as usize;
            match span.region {
                virtual_bank::Region::PreData { offset: block } => file[start..end]
                    .copy_from_slice(&layout.pre_data[block..block + span.len as usize]),
                virtual_bank::Region::EntryData {
                    entry,
                    offset: within,
                } => {
                    let within = within as usize;
                    file[start..end]
                        .copy_from_slice(&feeds[entry][within..within + span.len as usize]);
                }
                virtual_bank::Region::Gap => {}
                virtual_bank::Region::Eof => break,
            }
            served += span.len;
        }
        served
    };

    let mut read_pattern = serve(&mut file, 0, 0x1000) == 0x1000;
    let mut packet_count = 0usize;
    for (index, feed) in feeds.iter().enumerate() {
        let block_align = u64::from(bank.entries[index].format.block_align());
        let packet = 65_536 / block_align * block_align;
        let mut cursor = 0u64;
        while cursor < feed.len() as u64 {
            let request = packet.min(feed.len() as u64 - cursor) as u32;
            let offset = layout.entry_offsets[index] + cursor;
            let served = serve(&mut file, offset, request);
            read_pattern = read_pattern
                && served == request
                && offset + u64::from(served) <= virtual_size;
            packet_count += 1;
            cursor += u64::from(served.max(1));
        }
    }
    // The stock EOF clamp: a read at the end serves nothing.
    read_pattern = read_pattern && serve(&mut file, virtual_size, 0x1000) == 0;

    let matches_reference = file == oracle;
    let (reparsed, decode_equality) = match xwb::parse_song_bank(&file) {
        Err(_) => (false, false),
        Ok(generated) => {
            let oracle_bank = xwb::parse_song_bank(&oracle).expect("replay oracle reparse");
            let equal = (0..2).all(|index| {
                let ours = adpcm::decode_interleaved(
                    generated.entries[index].data,
                    generated.entries[index].format,
                    generated.entries[index].duration,
                )
                .expect("replay generated decode");
                let reference = adpcm::decode_interleaved(
                    oracle_bank.entries[index].data,
                    oracle_bank.entries[index].format,
                    oracle_bank.entries[index].duration,
                )
                .expect("replay oracle decode");
                ours == reference
            });
            (true, equal)
        }
    };
    let passed = read_pattern && matches_reference && reparsed && decode_equality;
    StreamingReplayResult {
        percent,
        entry_order: entry_order.into(),
        packet_count,
        reassembled_len: file.len() as u64,
        read_pattern,
        reparsed,
        matches_reference,
        decode_equality,
        passed,
    }
}

fn latency_limit_ms(platform: &str) -> u128 {
    if platform == "native-windows" {
        NATIVE_WINDOWS_LATENCY_LIMIT_MS
    } else {
        CROSSOVER_LATENCY_LIMIT_MS
    }
}

fn measure_operation<T>(operation: impl FnOnce() -> T) -> (T, u128, u64, u64) {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::Arc;

    let baseline = current_working_set_bytes();
    let peak = Arc::new(AtomicU64::new(baseline.unwrap_or(0)));
    let stop = Arc::new(AtomicBool::new(false));
    let failed = Arc::new(AtomicBool::new(baseline.is_none()));
    let ready = Arc::new(std::sync::Barrier::new(2));
    let monitor_peak = Arc::clone(&peak);
    let monitor_stop = Arc::clone(&stop);
    let monitor_failed = Arc::clone(&failed);
    let monitor_ready = Arc::clone(&ready);
    let monitor = std::thread::spawn(move || {
        match current_working_set_bytes() {
            Some(current) => {
                monitor_peak.fetch_max(current, Ordering::AcqRel);
            }
            None => monitor_failed.store(true, Ordering::Release),
        }
        monitor_ready.wait();
        while !monitor_stop.load(Ordering::Acquire) {
            match current_working_set_bytes() {
                Some(current) => {
                    monitor_peak.fetch_max(current, Ordering::AcqRel);
                }
                None => monitor_failed.store(true, Ordering::Release),
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        match current_working_set_bytes() {
            Some(current) => {
                monitor_peak.fetch_max(current, Ordering::AcqRel);
            }
            None => monitor_failed.store(true, Ordering::Release),
        }
    });
    ready.wait();
    let started = Instant::now();
    let result = operation();
    let latency = started.elapsed().as_millis();
    stop.store(true, Ordering::Release);
    if monitor.join().is_err() {
        failed.store(true, Ordering::Release);
    }
    let peak = peak.load(Ordering::Acquire);
    let additional = match (baseline, failed.load(Ordering::Acquire)) {
        (Some(baseline), false) if peak != 0 => peak.saturating_sub(baseline),
        _ => u64::MAX,
    };
    (result, latency, peak, additional)
}

EOF

note "running pure module tests"
(cd "$TMP" && cargo test --quiet)

note "checking temporary harness for x86_64-pc-windows-msvc"
cat >"$TMP/windows_check.rs" <<EOF_WINDOWS
#![allow(dead_code)]
#[path = "$XACT_SRC/mod.rs"]
mod xact;
#[path = "$TMP/src/memory.rs"]
mod memory;
fn main() {
    let _ = memory::current_working_set_bytes();
    let _ = xact::rate::RateRatio::IDENTITY;
}
EOF_WINDOWS
rustc +nightly --edition 2021 --target x86_64-pc-windows-msvc \
  --emit metadata -o "$TMP/windows_check.rmeta" "$TMP/windows_check.rs"

note "running release validation + demo generation"
(cd "$TMP" && cargo run --release --quiet -- "$OUTPUT_DIR" "$SIBLING_REV" "$CORPUS_DIR" "$REQUIRE_CORPUS" "$PLATFORM")

[[ -s "$OUTPUT_DIR/report.json" ]] || die "validator did not write target/song-rate-validation/report.json"
python3 - "$OUTPUT_DIR/report.json" <<'PY_REPORT_CHECK'
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as report_file:
    report = json.load(report_file)

for retired in ("cache", "on_demand"):
    if retired in report:
        raise SystemExit(f"report carries the retired {retired} section")
identity = report.get("identity_runtime")
if not isinstance(identity, dict) or not identity.get("passed"):
    raise SystemExit("report identity_runtime section is missing or failed")
if not all(check.get("passed") for check in identity.get("checks", [])):
    raise SystemExit("report identity_runtime checks did not pass")
streaming = report.get("streaming")
if not isinstance(streaming, dict) or not streaming.get("passed"):
    raise SystemExit("report streaming section is missing or failed")
if not all(check.get("passed") for check in streaming.get("checks", [])):
    raise SystemExit("report streaming checks did not pass")
resample = report.get("resample")
if not isinstance(resample, list) or not resample:
    raise SystemExit("report resample section is missing or empty")
if not all(cell.get("passed") for cell in resample):
    raise SystemExit("report resample cells did not pass")
PY_REPORT_CHECK
note "validation passed"
