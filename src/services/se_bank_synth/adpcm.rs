//! Assist Tick's fixed mono adapter over the shared XACT MS-ADPCM codec.
//!
//! The fixed XSB and one-entry in-memory XWB policy remain service-local. Only
//! the byte codec is shared. `scripts/validate_se_bank_synth.sh` proves this
//! adapter is byte-identical to both the previous local encoder and sibling
//! `ddr-chart-tools` revision
//! `fa3500bd65ca0350411ef5113070055340eb5a6d`.

use crate::core::xact::{adpcm, WaveFormat};

pub const BLOCK_ALIGN: usize = 70;
pub const SAMPLES_PER_BLOCK: usize = 128;

const MONO_FORMAT: WaveFormat = WaveFormat::from_packed(2 | (1 << 2) | (44_100 << 5) | (48 << 23));

/// Encode mono PCM while retaining Assist Tick's historical zero-padding API.
///
/// The shared codec itself rejects partial blocks. Assist Tick's production
/// mixer always supplies exact blocks, but preserving this adapter behavior
/// keeps its standalone container generator compatible with existing callers.
pub fn encode_mono(samples: &[i16]) -> Vec<u8> {
    if samples.is_empty() {
        return Vec::new();
    }

    let mut padded = Vec::new();
    let input = if samples.len() % SAMPLES_PER_BLOCK == 0 {
        samples
    } else {
        let target = samples.len().div_ceil(SAMPLES_PER_BLOCK) * SAMPLES_PER_BLOCK;
        padded.reserve_exact(target);
        padded.extend_from_slice(samples);
        padded.resize(target, 0);
        &padded
    };

    adpcm::encode_interleaved(input, MONO_FORMAT)
        .expect("fixed mono format and block-aligned Assist Tick input must encode")
}

/// One self-contained encoded block of silence.
pub fn silence_block() -> &'static [u8; BLOCK_ALIGN] {
    static BLOCK: std::sync::OnceLock<[u8; BLOCK_ALIGN]> = std::sync::OnceLock::new();
    BLOCK.get_or_init(|| {
        let encoded = adpcm::encode_interleaved(&[0; SAMPLES_PER_BLOCK], MONO_FORMAT)
            .expect("fixed mono silence block must encode");
        let mut block = [0; BLOCK_ALIGN];
        block.copy_from_slice(&encoded);
        block
    })
}
