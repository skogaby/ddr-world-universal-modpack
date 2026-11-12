//! XSB (XACT2 Sound Bank) writer — the DDR **song** profile.
//!
//! Port of the sibling `ddr-chart-tools`' `xsb::write` (its `docs/xsb_format.md`
//! describes every field): one wave bank, two simple cues — `<name>` playing
//! a SIMPLE sound on mix category 4 (gameplay) and `<name>_s` playing a
//! COMPLEX looping sound on category 3 (wheel preview) — complex sound
//! first, CRC-16 over `[0x12..]` stored at `0x08`. The engine validates the
//! CRC and silently plays nothing on a mismatch; cue names must equal the
//! basename byte for byte.
//!
//! The one deliberate difference from the tool: **wave indices**. Cues
//! reference wave-bank entries by INDEX. The tool writes its XWB preview
//! first (`<code>_s` = 0, `<code>` = 1) and its XSB accordingly; the stock
//! banks and `core::xact::song_bank` write main = 0, preview = 1 (the
//! stock `aaaa.xsb` decode in
//! `.agents/planning/20260725-assist-tick/research/xact-bank-format.md` §2.6
//! shows exactly these two bytes flipped relative to the tool). So
//! [`write_song_profile`] emits main → 0 / preview → 1, and
//! [`write_song_profile_indexed`] exposes the indices so the harness can
//! prove the port byte-identical to the tool with `(1, 0)`.
//!
//! The SE profile the assist-tick synthesizer needs lives in
//! `services/se_bank_synth/xsb.rs`; the CRC and hash are the same functions.
//!
//! Pure, deterministic; `scripts/validate_stepmania.sh` mounts it.

use std::fmt;

/// File magic: "SDBK" (little-endian).
const MAGIC: u32 = 0x4B42_4453;
/// XSB content/tool version for XACT2 v2.10 (DDR World).
const VERSION: u16 = 0x002B;
/// Windows platform byte.
const PLATFORM: u8 = 0x01;

const HEADER_SIZE: usize = 0x4A;
const NAME_FIELD_LEN: usize = 0x40;

/// `total_cues == max(16, cue_count)`; two cues → the floor.
const HASH_BUCKET_COUNT: u16 = 16;
const WAVEBANK_COUNT: u8 = 1;

const SIMPLE_SOUND_SIZE: usize = 19;
const COMPLEX_SOUND_SIZE: usize = 39;
const CUE_ENTRY_SIZE: usize = 5;
const NAME_INDEX_ENTRY_SIZE: usize = 6;

/// Cue flags: bit 2 set = playable sound cue, no variation/transition.
const CUE_FLAG_SOUND: u8 = 0x04;

const EMPTY_BUCKET: u16 = 0xFFFF;
const END_OF_CHAIN: u16 = 0xFFFF;
const NO_OFFSET: i32 = -1;

const CRC_OFFSET: usize = 0x08;
const CRC_DATA_START: usize = 0x12;

/// Longest basename accepted (the name fields are 64 bytes; DDR codes are
/// 4–6 characters; 16 is the tool's defensive cap).
pub const MAX_NAME_LEN: usize = 16;

/// Which XWB entry each cue plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WaveIndices {
    pub main: u16,
    pub preview: u16,
}

impl WaveIndices {
    /// Stock order (`song_bank.rs`): main = 0, preview = 1.
    pub const STOCK: Self = Self {
        main: 0,
        preview: 1,
    };
    /// The sibling tool's order: preview = 0, main = 1.
    pub const TOOL: Self = Self {
        main: 1,
        preview: 0,
    };
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XsbError {
    /// Not 1–16 lowercase ASCII letters, digits or `_`.
    BadName(String),
}

impl fmt::Display for XsbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadName(name) => write!(
                f,
                "sound bank name must be 1-{MAX_NAME_LEN} lowercase ASCII letters, digits or underscores, got {name:?}"
            ),
        }
    }
}

impl std::error::Error for XsbError {}

/// `true` for a name both the XSB and the game's 6-char basename buffer accept.
#[must_use]
pub fn is_valid_name(name: &str) -> bool {
    let b = name.as_bytes();
    !b.is_empty()
        && b.len() <= MAX_NAME_LEN
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_')
}

/// Song-profile XSB for `name`, cues `<name>` → wave 0 and `<name>_s` →
/// wave 1 (the order `song_bank::SongBankPlan` writes).
pub fn write_song_profile(name: &str) -> Result<Vec<u8>, XsbError> {
    write_song_profile_indexed(name, WaveIndices::STOCK)
}

/// Song-profile XSB with explicit wave indices.
pub fn write_song_profile_indexed(name: &str, waves: WaveIndices) -> Result<Vec<u8>, XsbError> {
    if !is_valid_name(name) {
        return Err(XsbError::BadName(name.to_string()));
    }
    Ok(build(name.as_bytes(), waves))
}

/// SIMPLE sound entry for the main track: flags=0x04 (simple + RPC),
/// category 4, volume 180, pitch 0, priority 0, entry_length 19,
/// `u16 wave_index` at +9, wavebank 0, then the 7-byte RPC reference
/// (len 7, count 1, code 0xF8).
const SIMPLE_SOUND_BYTES: [u8; SIMPLE_SOUND_SIZE] = [
    0x04, 0x04, 0x00, 0xB4, 0x00, 0x00, 0x00, 0x13, 0x00, 0x00, 0x00, 0x00, 0x07, 0x00, 0x01, 0xF8,
    0x00, 0x00, 0x00,
];
const SIMPLE_WAVE_INDEX_AT: usize = 9;

/// COMPLEX sound entry for the preview: flags=0x05 (complex + RPC),
/// category 3, volume 180, entry_length 39, one track; the track carries
/// the RPC preamble, volume 0xB4, the "mystery" byte 0xE0 (majority value
/// across stock banks), the loop/clip event, and its `u16 wave_index` at
/// track body offset 14 (sound offset 24).
const COMPLEX_SOUND_BYTES: [u8; COMPLEX_SOUND_SIZE] = [
    0x05, 0x03, 0x00, 0xB4, 0x00, 0x00, 0x00, 0x27, 0x00, 0x01, // prefix
    0x07, 0x00, 0x01, 0xF8, 0x00, 0x00, 0x00, // track RPC preamble
    0xB4, 0xE0, 0x00, 0x00, 0x00, 0x01, 0x01, 0x00, 0x00, 0x20, 0x00, 0x00, 0xFF, 0x0C, 0x00, 0x00,
    0x00, 0xFF, 0x00, 0x00, 0x00, 0x00,
];
const COMPLEX_WAVE_INDEX_AT: usize = 31;

struct Layout {
    soundbank_name: usize,
    wavebank_name: usize,
    sound: usize,
    simple_cue: usize,
    hash_table: usize,
    name_index: usize,
    cue_names: usize,
    total_size: usize,
    cue_name_table_len: usize,
}

impl Layout {
    fn compute(name_len: usize) -> Self {
        // Cue 0 `<name>_s`, cue 1 `<name>`, each NUL-terminated.
        let cue_name_table_len = (name_len + 2 + 1) + (name_len + 1);
        let soundbank_name = HEADER_SIZE;
        let wavebank_name = soundbank_name + NAME_FIELD_LEN;
        let sound = wavebank_name + (WAVEBANK_COUNT as usize) * NAME_FIELD_LEN;
        let simple_cue = sound + COMPLEX_SOUND_SIZE + SIMPLE_SOUND_SIZE;
        let hash_table = simple_cue + 2 * CUE_ENTRY_SIZE;
        let name_index = hash_table + (HASH_BUCKET_COUNT as usize) * 2;
        let cue_names = name_index + 2 * NAME_INDEX_ENTRY_SIZE;
        let total_size = cue_names + cue_name_table_len;
        Self {
            soundbank_name,
            wavebank_name,
            sound,
            simple_cue,
            hash_table,
            name_index,
            cue_names,
            total_size,
            cue_name_table_len,
        }
    }
}

fn build(name: &[u8], waves: WaveIndices) -> Vec<u8> {
    let layout = Layout::compute(name.len());
    let mut buf = vec![0u8; layout.total_size];

    // Header. CRC at 0x08..0x0A last; the timestamp at 0x0A..0x12 stays 0.
    buf[0x00..0x04].copy_from_slice(&MAGIC.to_le_bytes());
    buf[0x04..0x06].copy_from_slice(&VERSION.to_le_bytes());
    buf[0x06..0x08].copy_from_slice(&VERSION.to_le_bytes());
    buf[0x12] = PLATFORM;
    buf[0x13..0x15].copy_from_slice(&2u16.to_le_bytes()); // simple cues
    buf[0x15..0x17].copy_from_slice(&0u16.to_le_bytes()); // complex cues
    buf[0x19..0x1B].copy_from_slice(&HASH_BUCKET_COUNT.to_le_bytes());
    buf[0x1B] = WAVEBANK_COUNT;
    buf[0x1C..0x1E].copy_from_slice(&2u16.to_le_bytes()); // sounds
    buf[0x1E..0x20].copy_from_slice(&(layout.cue_name_table_len as u16).to_le_bytes());
    put_i32(&mut buf, 0x22, layout.simple_cue as i32);
    put_i32(&mut buf, 0x26, NO_OFFSET);
    put_i32(&mut buf, 0x2A, layout.cue_names as i32);
    put_i32(&mut buf, 0x2E, NO_OFFSET);
    put_i32(&mut buf, 0x32, NO_OFFSET);
    put_i32(&mut buf, 0x36, NO_OFFSET);
    put_i32(&mut buf, 0x3A, layout.wavebank_name as i32);
    put_i32(&mut buf, 0x3E, layout.hash_table as i32);
    put_i32(&mut buf, 0x42, layout.name_index as i32);
    put_i32(&mut buf, 0x46, layout.sound as i32);

    buf[layout.soundbank_name..layout.soundbank_name + name.len()].copy_from_slice(name);
    buf[layout.wavebank_name..layout.wavebank_name + name.len()].copy_from_slice(name);

    // Sounds: complex (preview) first — the only layout proven to play.
    let complex_at = layout.sound;
    let simple_at = complex_at + COMPLEX_SOUND_SIZE;
    buf[complex_at..complex_at + COMPLEX_SOUND_SIZE].copy_from_slice(&COMPLEX_SOUND_BYTES);
    buf[complex_at + COMPLEX_WAVE_INDEX_AT..complex_at + COMPLEX_WAVE_INDEX_AT + 2]
        .copy_from_slice(&waves.preview.to_le_bytes());
    buf[simple_at..simple_at + SIMPLE_SOUND_SIZE].copy_from_slice(&SIMPLE_SOUND_BYTES);
    buf[simple_at + SIMPLE_WAVE_INDEX_AT..simple_at + SIMPLE_WAVE_INDEX_AT + 2]
        .copy_from_slice(&waves.main.to_le_bytes());

    // Cues: 0 → complex/preview, 1 → simple/main.
    for (i, sound_off) in [complex_at, simple_at].into_iter().enumerate() {
        let off = layout.simple_cue + i * CUE_ENTRY_SIZE;
        buf[off] = CUE_FLAG_SOUND;
        buf[off + 1..off + 5].copy_from_slice(&(sound_off as u32).to_le_bytes());
    }

    // Cue names in cue order: "<name>_s\0<name>\0".
    let mut full_names: [Vec<u8>; 2] = [Vec::new(), Vec::new()];
    full_names[0].extend_from_slice(name);
    full_names[0].extend_from_slice(b"_s");
    full_names[1].extend_from_slice(name);
    let mut name_offsets = [0usize; 2];
    let mut off = layout.cue_names;
    for (i, full) in full_names.iter().enumerate() {
        buf[off..off + full.len()].copy_from_slice(full);
        name_offsets[i] = off;
        off += full.len() + 1;
    }

    for b in 0..HASH_BUCKET_COUNT as usize {
        let o = layout.hash_table + b * 2;
        buf[o..o + 2].copy_from_slice(&EMPTY_BUCKET.to_le_bytes());
    }
    let mut next = [END_OF_CHAIN; 2];
    for (i, full) in full_names.iter().enumerate() {
        let bucket = cue_name_hash_bucket(full, HASH_BUCKET_COUNT);
        let bucket_off = layout.hash_table + usize::from(bucket) * 2;
        let head = u16::from_le_bytes([buf[bucket_off], buf[bucket_off + 1]]);
        if head == EMPTY_BUCKET {
            buf[bucket_off..bucket_off + 2].copy_from_slice(&(i as u16).to_le_bytes());
        } else {
            // Two cues only: the chain tail is the head.
            next[usize::from(head)] = i as u16;
        }
    }
    for i in 0..2 {
        let entry = layout.name_index + i * NAME_INDEX_ENTRY_SIZE;
        buf[entry..entry + 4].copy_from_slice(&(name_offsets[i] as u32).to_le_bytes());
        buf[entry + 4..entry + 6].copy_from_slice(&next[i].to_le_bytes());
    }

    let crc = xact_crc16(&buf[CRC_DATA_START..]);
    buf[CRC_OFFSET..CRC_OFFSET + 2].copy_from_slice(&crc.to_le_bytes());
    buf
}

fn put_i32(buf: &mut [u8], offset: usize, value: i32) {
    buf[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

/// XACT2 cue-name hash bucket (`xactengine2_10.dll`'s `GetCueIndex` helper):
/// per character `h = 3·h + (h >> 1) + c`, wrapping u16; bucket = `h % n`.
fn cue_name_hash_bucket(name: &[u8], bucket_count: u16) -> u16 {
    let mut h: u16 = 0;
    for &c in name {
        h = h
            .wrapping_mul(3)
            .wrapping_add(h >> 1)
            .wrapping_add(u16::from(c));
    }
    h % bucket_count
}

/// CRC-16 the engine validates; it stores `!crc` at 0x08.
fn xact_crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &b in data {
        crc = CRC_TABLE[usize::from((u16::from(b) ^ crc) & 0xFF)] ^ (crc >> 8);
    }
    !crc
}

#[rustfmt::skip]
const CRC_TABLE: [u16; 256] = [
    0x0000, 0x1189, 0x2312, 0x329b, 0x4624, 0x57ad, 0x6536, 0x74bf,
    0x8c48, 0x9dc1, 0xaf5a, 0xbed3, 0xca6c, 0xdbe5, 0xe97e, 0xf8f7,
    0x1081, 0x0108, 0x3393, 0x221a, 0x56a5, 0x472c, 0x75b7, 0x643e,
    0x9cc9, 0x8d40, 0xbfdb, 0xae52, 0xdaed, 0xcb64, 0xf9ff, 0xe876,
    0x2102, 0x308b, 0x0210, 0x1399, 0x6726, 0x76af, 0x4434, 0x55bd,
    0xad4a, 0xbcc3, 0x8e58, 0x9fd1, 0xeb6e, 0xfae7, 0xc87c, 0xd9f5,
    0x3183, 0x200a, 0x1291, 0x0318, 0x77a7, 0x662e, 0x54b5, 0x453c,
    0xbdcb, 0xac42, 0x9ed9, 0x8f50, 0xfbef, 0xea66, 0xd8fd, 0xc974,
    0x4204, 0x538d, 0x6116, 0x709f, 0x0420, 0x15a9, 0x2732, 0x36bb,
    0xce4c, 0xdfc5, 0xed5e, 0xfcd7, 0x8868, 0x99e1, 0xab7a, 0xbaf3,
    0x5285, 0x430c, 0x7197, 0x601e, 0x14a1, 0x0528, 0x37b3, 0x263a,
    0xdecd, 0xcf44, 0xfddf, 0xec56, 0x98e9, 0x8960, 0xbbfb, 0xaa72,
    0x6306, 0x728f, 0x4014, 0x519d, 0x2522, 0x34ab, 0x0630, 0x17b9,
    0xef4e, 0xfec7, 0xcc5c, 0xddd5, 0xa96a, 0xb8e3, 0x8a78, 0x9bf1,
    0x7387, 0x620e, 0x5095, 0x411c, 0x35a3, 0x242a, 0x16b1, 0x0738,
    0xffcf, 0xee46, 0xdcdd, 0xcd54, 0xb9eb, 0xa862, 0x9af9, 0x8b70,
    0x8408, 0x9581, 0xa71a, 0xb693, 0xc22c, 0xd3a5, 0xe13e, 0xf0b7,
    0x0840, 0x19c9, 0x2b52, 0x3adb, 0x4e64, 0x5fed, 0x6d76, 0x7cff,
    0x9489, 0x8500, 0xb79b, 0xa612, 0xd2ad, 0xc324, 0xf1bf, 0xe036,
    0x18c1, 0x0948, 0x3bd3, 0x2a5a, 0x5ee5, 0x4f6c, 0x7df7, 0x6c7e,
    0xa50a, 0xb483, 0x8618, 0x9791, 0xe32e, 0xf2a7, 0xc03c, 0xd1b5,
    0x2942, 0x38cb, 0x0a50, 0x1bd9, 0x6f66, 0x7eef, 0x4c74, 0x5dfd,
    0xb58b, 0xa402, 0x9699, 0x8710, 0xf3af, 0xe226, 0xd0bd, 0xc134,
    0x39c3, 0x284a, 0x1ad1, 0x0b58, 0x7fe7, 0x6e6e, 0x5cf5, 0x4d7c,
    0xc60c, 0xd785, 0xe51e, 0xf497, 0x8028, 0x91a1, 0xa33a, 0xb2b3,
    0x4a44, 0x5bcd, 0x6956, 0x78df, 0x0c60, 0x1de9, 0x2f72, 0x3efb,
    0xd68d, 0xc704, 0xf59f, 0xe416, 0x90a9, 0x8120, 0xb3bb, 0xa232,
    0x5ac5, 0x4b4c, 0x79d7, 0x685e, 0x1ce1, 0x0d68, 0x3ff3, 0x2e7a,
    0xe70e, 0xf687, 0xc41c, 0xd595, 0xa12a, 0xb0a3, 0x8238, 0x93b1,
    0x6b46, 0x7acf, 0x4854, 0x59dd, 0x2d62, 0x3ceb, 0x0e70, 0x1ff9,
    0xf78f, 0xe606, 0xd49d, 0xc514, 0xb1ab, 0xa022, 0x92b9, 0x8330,
    0x7bc7, 0x6a4e, 0x58d5, 0x495c, 0x3de3, 0x2c6a, 0x1ef1, 0x0f78,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stock_aaaa_shape() {
        // Field-by-field decode of the stock `aaaa.xsb` (research §2.6):
        // 326 bytes, offsets 0x104 / 0x13A / 0x8A / 0x10E / 0x12E / 0xCA,
        // CRC 0xCB7F — the stock file's CRC covers a non-zero timestamp,
        // which is NOT part of the CRC range, so the CRC must match too.
        let xsb = write_song_profile("aaaa").expect("xsb");
        assert_eq!(xsb.len(), 326);
        assert_eq!(&xsb[0..4], b"SDBK");
        assert_eq!(u16::from_le_bytes([xsb[8], xsb[9]]), 0xCB7F);
        assert_eq!(xsb[0x12], 1);
        assert_eq!(u16::from_le_bytes([xsb[0x13], xsb[0x14]]), 2);
        assert_eq!(u16::from_le_bytes([xsb[0x1E], xsb[0x1F]]), 12);
        let rd = |o: usize| i32::from_le_bytes([xsb[o], xsb[o + 1], xsb[o + 2], xsb[o + 3]]);
        assert_eq!(rd(0x22), 0x104);
        assert_eq!(rd(0x2A), 0x13A);
        assert_eq!(rd(0x3A), 0x8A);
        assert_eq!(rd(0x3E), 0x10E);
        assert_eq!(rd(0x42), 0x12E);
        assert_eq!(rd(0x46), 0xCA);
        assert_eq!(&xsb[0x4A..0x4E], b"aaaa");
        assert_eq!(&xsb[0x8A..0x8E], b"aaaa");
        // Complex sound's track wave index = 1, simple sound's = 0.
        assert_eq!(&xsb[0xE9..0xEB], &[0x01, 0x00]);
        assert_eq!(&xsb[0xFA..0xFC], &[0x00, 0x00]);
        assert_eq!(
            &xsb[0x104..0x10E],
            &[0x04, 0xCA, 0, 0, 0, 0x04, 0xF1, 0, 0, 0]
        );
        assert_eq!(&xsb[0x13A..], b"aaaa_s\0aaaa\0");
        // Hash: bucket 1 = cue 1, bucket 14 = cue 0, rest empty.
        for b in 0..16usize {
            let v = u16::from_le_bytes([xsb[0x10E + 2 * b], xsb[0x10F + 2 * b]]);
            let expect = match b {
                1 => 1,
                14 => 0,
                _ => 0xFFFF,
            };
            assert_eq!(v, expect, "bucket {b}");
        }
    }

    #[test]
    fn tool_order_flips_only_the_wave_indices_and_crc() {
        let ours = write_song_profile("zab12c").expect("xsb");
        let tool = write_song_profile_indexed("zab12c", WaveIndices::TOOL).expect("xsb");
        assert_eq!(ours.len(), tool.len());
        let diff: Vec<usize> = (0..ours.len()).filter(|&i| ours[i] != tool[i]).collect();
        let simple_wave = 0xCA + COMPLEX_SOUND_SIZE + SIMPLE_WAVE_INDEX_AT;
        let complex_wave = 0xCA + COMPLEX_WAVE_INDEX_AT;
        let mut expected = vec![8, 9, complex_wave, simple_wave];
        expected.sort_unstable();
        assert_eq!(diff, expected);
    }

    #[test]
    fn sizes_track_name_length() {
        assert_eq!(write_song_profile("zab1").unwrap().len(), 326);
        assert_eq!(write_song_profile("zab12").unwrap().len(), 328);
        assert_eq!(write_song_profile("zab12c").unwrap().len(), 330);
    }

    #[test]
    fn name_rule() {
        assert!(write_song_profile("").is_err());
        assert!(write_song_profile("ZAB1").is_err());
        assert!(write_song_profile("a-b").is_err());
        assert!(write_song_profile("a".repeat(17).as_str()).is_err());
        assert!(write_song_profile("a_1").is_ok());
        assert!(write_song_profile("a".repeat(16).as_str()).is_ok());
    }

    #[test]
    fn hash_chains_two_cues_in_one_bucket() {
        // Find a name whose two cue names collide, then check the chain.
        let mut found = None;
        for a in b'a'..=b'z' {
            for b in b'a'..=b'z' {
                let name = [a, b];
                let full0 = [a, b, b'_', b's'];
                if cue_name_hash_bucket(&full0, 16) == cue_name_hash_bucket(&name, 16) {
                    found = Some(String::from_utf8(name.to_vec()).unwrap());
                    break;
                }
            }
            if found.is_some() {
                break;
            }
        }
        let name = found.expect("a colliding two-letter name exists");
        let xsb = write_song_profile(&name).expect("xsb");
        let layout = Layout::compute(name.len());
        let bucket = cue_name_hash_bucket(name.as_bytes(), 16) as usize;
        let head = u16::from_le_bytes([
            xsb[layout.hash_table + 2 * bucket],
            xsb[layout.hash_table + 2 * bucket + 1],
        ]);
        assert_eq!(head, 0);
        let next0 = u16::from_le_bytes([xsb[layout.name_index + 4], xsb[layout.name_index + 5]]);
        assert_eq!(next0, 1);
        let next1 = u16::from_le_bytes([xsb[layout.name_index + 10], xsb[layout.name_index + 11]]);
        assert_eq!(next1, END_OF_CHAIN);
    }
}
