//! SE bank synthesis — pure-CPU construction of the assist-tick mod's
//! pre-mixed tick track, in the game engine's own container formats.
//!
//! Three ports from the sibling `ddr-chart-tools` (offline-proven by the
//! shipped assist-tick feature) plus the clap mixer:
//!
//! - [`adpcm`] — MS-ADPCM mono encoder,
//! - [`xwb`] — fixed-header one-entry in-memory wave-bank writer, exposing
//!   the rewritable sample segment's offset/length,
//! - [`xsb`] — SE-profile sound-bank writer (one cue, mix category 6),
//! - [`containers`] — the public API over them: [`build_tick_containers`]
//!   (one-time container build) and [`synthesize_track`] (per-song mix of
//!   claps into a fixed-capacity mono buffer at sample-exact positions,
//!   encoded to exactly the wave bank's sample segment length).
//!
//! **No game ABI anywhere in this module** — everything here is deterministic
//! byte-work, callable from any thread. That split is load-bearing: per-song
//! synthesis runs on a background thread (design NFR-1) while the engine
//! calls that consume its output (`register`/`rewrite`/`play`/`stop` in
//! [`services::game_audio`](crate::services::game_audio)) stay game-thread-only.
//! The format submodules also have zero *crate* dependencies, so the whole
//! layer compiles stand-alone on a host machine for offline validation
//! against the sibling `ddr-se-bank` tool (`scripts/validate_se_bank_synth.sh`).
//!
//! The one-cue bank is registered ONCE for the process lifetime with the
//! entry declared at [`TICK_CAPACITY_MS`]; per song only the sample segment's
//! bytes change (the immortal-bank rule — the header the engine validated is
//! immutable). Charts whose ticks run past the capacity are truncated with a
//! count (FR-8); the segment tail past the song is encoded silence.
//!
//! Design: `.agents/planning/20260729-assist-tick-premixed-track/design/detailed-design.md`
//! §"Components 2". RE record: `research/rc-rd-re-lifecycle-synthesis.md`
//! (capacity/lifecycle) and the shipped feature's `xact-bank-format.md`
//! (container validator rules).

pub mod adpcm;
pub mod containers;
pub mod xsb;
pub mod xwb;

// Some re-exports have no crate-internal consumer yet — they are the API the
// tick-bank registration (game_audio) and the reworked assist_tick mod build
// on in the plan's later steps. cdylib: re-exports alone don't count as use.
#[allow(unused_imports)]
pub use containers::{
    build_tick_containers, scale_pcm, shift_bytes_for_ms, synthesize_track, SynthResult,
    TickContainers, BANK_NAME, TICK_CAPACITY_MS, TICK_RATE_HZ,
};

use crate::services::avs_layeredfs::mod_paths;
use crate::{log_info, log_warn};

/// Mod-relative path of the raw clap asset (mono i16 LE, 44100 Hz), resolved
/// through the `data_mods` mod-path resolver — canonically
/// `data_mods/assist_tick/clap_44k_mono.pcm`. Regenerate from the committed
/// Ogg source with:
/// `ffmpeg -i data_mods/assist_tick/source/clap.ogg -ac 1 -ar 44100 -f s16le
/// data_mods/assist_tick/clap_44k_mono.pcm`
/// (raw PCM so the DLL needs no Vorbis decoder — R-E).
pub const CLAP_PCM_REL: &str = "clap_44k_mono.pcm";

/// Shortest clap the loader accepts: 10 ms. Anything shorter is a truncated
/// or wrong file, not a usable timing reference (design error table:
/// "clap PCM asset missing/short ⇒ mod disabled").
const MIN_CLAP_SAMPLES: usize = (TICK_RATE_HZ / 100) as usize;

/// Read and validate the raw clap asset (mono i16 LE 44100 Hz) through the
/// mod-path resolver. File IO — init/background threads only, never a
/// per-frame path. `None` (with one WARN naming the reason) when the asset
/// is missing, unreadable, odd-length, or too short to be a clap — the
/// consuming mod treats that as an init-time prerequisite failure (FR-6).
pub fn load_clap_pcm() -> Option<Vec<i16>> {
    let path = match mod_paths::find_first_modfile(CLAP_PCM_REL) {
        Some(p) => p,
        None => {
            log_warn!(
                "SeBankSynth: clap asset '{}' not found under data_mods (expected data_mods/assist_tick/{})",
                CLAP_PCM_REL,
                CLAP_PCM_REL
            );
            return None;
        }
    };
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            log_warn!("SeBankSynth: reading '{}' failed: {}", path, e);
            return None;
        }
    };
    if bytes.len() % 2 != 0 || bytes.len() / 2 < MIN_CLAP_SAMPLES {
        log_warn!(
            "SeBankSynth: clap asset '{}' is {} bytes -- not a plausible mono i16 PCM clap (need an even byte count and at least {} samples)",
            path,
            bytes.len(),
            MIN_CLAP_SAMPLES
        );
        return None;
    }
    let samples: Vec<i16> = bytes
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect();
    log_info!(
        "SeBankSynth: loaded clap asset '{}' ({} samples, {} ms)",
        path,
        samples.len(),
        samples.len() as u64 * 1000 / TICK_RATE_HZ as u64
    );
    Some(samples)
}

// ── Debug dump (offline container validation) ────────────────────────

/// Where the debug dump writes its container pair.
const DUMP_DIR: &str = "./data_mods/_cache/assist_tick_synth";
/// The dump's known test pattern: a clap every 500 ms for 20 s. Chosen so
/// `ddr-se-bank dump` block offsets are trivially predictable (500 ms =
/// 22 050 samples = block 172 remainder 34).
const DUMP_PATTERN_STEP_MS: i32 = 500;
const DUMP_PATTERN_TICKS: i32 = 40;

/// Debug-gated container dump for offline validation (implementation plan
/// Step 1's test hook): when `layeredfs.developer_mode` is set, synthesize
/// the container pair with a known clap pattern spliced into the wave bank's
/// sample segment and write both to `data_mods/_cache/assist_tick_synth/`,
/// where the sibling `ddr-se-bank dump` and the engine-validator replay
/// checks can inspect them.
///
/// Spawned onto a background thread (a full-capacity encode does not belong
/// on the boot path); pure CPU + file IO, no game ABI. Call after the
/// LayeredFS mod-path scan so the clap asset resolves. No-op outside
/// developer mode.
pub fn debug_dump_if_dev() {
    let dev = crate::mods::config::get()
        .and_then(|c| c.layeredfs.as_ref())
        .map(|l| l.developer_mode)
        .unwrap_or(false);
    if !dev {
        return;
    }
    std::thread::spawn(|| {
        let result = std::panic::catch_unwind(dump_containers);
        if result.is_err() {
            log_warn!("SeBankSynth: debug dump panicked -- dump incomplete");
        }
    });
}

fn dump_containers() {
    let Some(clap) = load_clap_pcm() else {
        log_warn!("SeBankSynth: debug dump skipped -- clap asset unavailable");
        return;
    };

    let started = std::time::Instant::now();
    let mut containers = build_tick_containers();
    let pattern: Vec<i32> = (0..DUMP_PATTERN_TICKS)
        .map(|i| i * DUMP_PATTERN_STEP_MS)
        .collect();
    let synth = synthesize_track(&clap, &pattern);
    containers.xwb_bytes[containers.sample_seg_offset..].copy_from_slice(&synth.encoded);
    let elapsed_ms = started.elapsed().as_millis();

    if !mod_paths::mkdir_p(DUMP_DIR) {
        log_warn!("SeBankSynth: debug dump could not create '{}'", DUMP_DIR);
        return;
    }
    for (name, bytes) in [
        ("tick.xsb", &containers.xsb_bytes),
        ("tick.xwb", &containers.xwb_bytes),
    ] {
        let path = format!("{}/{}", DUMP_DIR, name);
        if let Err(e) = std::fs::write(&path, bytes) {
            log_warn!("SeBankSynth: debug dump write '{}' failed: {}", path, e);
            return;
        }
    }
    log_info!(
        "SeBankSynth: debug dump wrote {}/tick.{{xsb,xwb}} (xsb={} B, xwb={} B, seg_off={}, seg_len={}, pattern={}x{}ms, mixed={} clipped={} dropped={}, {} ms)",
        DUMP_DIR,
        containers.xsb_bytes.len(),
        containers.xwb_bytes.len(),
        containers.sample_seg_offset,
        containers.sample_seg_len,
        DUMP_PATTERN_TICKS,
        DUMP_PATTERN_STEP_MS,
        synth.mixed,
        synth.clipped,
        synth.dropped,
        elapsed_ms
    );
}
