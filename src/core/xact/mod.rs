//! Pure XACT2 format and codec support.
//!
//! The focused XWB/MS-ADPCM implementation is adapted from sibling
//! `ddr-chart-tools` revision
//! `fa3500bd65ca0350411ef5113070055340eb5a6d`. Intentional differences are:
//! borrowed XWB payload views, strict DDR World song-bank validation, checked
//! layout arithmetic, logical-duration validation for stock partial tails, and
//! direct interleaved encoding that rejects rather than pads partial blocks.

pub mod adpcm;
pub mod digest;
pub mod rate;
pub mod resample;
pub mod stretch;
pub mod virtual_bank;
pub mod xwb;

/// Packed XACT2 `WAVEBANKMINIWAVEFORMAT` descriptor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WaveFormat(u32);

impl WaveFormat {
    pub const CODEC_ADPCM: u8 = 2;

    #[must_use]
    pub const fn from_packed(bits: u32) -> Self {
        Self(bits)
    }

    #[must_use]
    pub const fn packed(self) -> u32 {
        self.0
    }

    #[must_use]
    pub const fn codec(self) -> u8 {
        (self.0 & 0x3) as u8
    }

    #[must_use]
    pub const fn channels(self) -> u8 {
        ((self.0 >> 2) & 0x7) as u8
    }

    #[must_use]
    pub const fn sample_rate(self) -> u32 {
        (self.0 >> 5) & 0x3_ffff
    }

    #[must_use]
    pub const fn block_align_raw(self) -> u8 {
        ((self.0 >> 23) & 0xff) as u8
    }

    #[must_use]
    pub const fn bits_per_sample_flag(self) -> u8 {
        ((self.0 >> 31) & 1) as u8
    }

    #[must_use]
    pub const fn block_align(self) -> u32 {
        (self.block_align_raw() as u32 + 22) * self.channels() as u32
    }

    #[must_use]
    pub const fn samples_per_block(self) -> u32 {
        let channels = self.channels() as u32;
        let block_align = self.block_align();
        if channels == 0 || block_align < 7 * channels {
            return 0;
        }
        ((block_align - 7 * channels) * 8) / (4 * channels) + 2
    }
}

#[cfg(test)]
mod tests;
