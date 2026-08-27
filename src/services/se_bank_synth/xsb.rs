//! XSB (XACT2 Sound Bank) writer — **sound-effect profile only**.
//!
//! Port of the sibling `ddr-chart-tools`' `xsb::write_se`, reduced to the one
//! bank this crate synthesizes: one wave bank, one simple cue, one *bare*
//! sound entry on mix category 6 with no runtime-parameter curve — the shape
//! 129 of the 138 sounds in the game's own `se_normal.xsb` gameplay-SE bank
//! use. The port is byte-faithful: `build_se("asti")` here is bit-identical
//! to the sibling's `write_se("asti")` output, which is what lets the offline
//! validation byte-compare the two.
//!
//! Layout (fixed ahead of the cue-name string, so every offset is a constant
//! for a given name length):
//!
//! ```text
//! [Header]           0x00..0x4A  magic, versions, CRC, counts, section offsets
//! [Soundbank name]   0x4A..0x8A  64-byte null-padded ASCII
//! [Wavebank name]    0x8A..0xCA  64-byte null-padded ASCII (must byte-match
//!                                the XWB's internal name — the engine pairs
//!                                banks by name, case-sensitively)
//! [Sound entry]      0xCA..0xD6  12-byte bare SE sound (category 6)
//! [Simple cue]       0xD6..0xDB  5 bytes, points at the sound entry
//! [Hash table]       0xDB..0xFB  16 × u16 buckets
//! [Name index]       0xFB..0x101 (u32 name_off, u16 next_in_chain)
//! [Cue name]         0x101..     "{name}\0" — must run exactly to EOF
//! ```
//!
//! The engine validates a CRC-16 over bytes `[0x12..]` stored at `0x08`; a
//! mismatch silently rejects the bank (audio goes dark, no error). CRC and
//! cue-name hash were reverse-engineered from `xactengine2_10.dll` — see the
//! shipped feature's RE record,
//! `.agents/planning/20260725-assist-tick/research/xact-bank-format.md`.
//!
//! Deterministic and pure CPU — safe on any thread.

/// File magic: "SDBK" (little-endian).
const MAGIC: u32 = 0x4B42_4453;
/// XSB content/tool version for XACT2 v2.10 (DDR World).
const VERSION: u16 = 0x002B;
/// Windows platform byte.
const PLATFORM: u8 = 0x01;

/// Fixed header size — section offsets are absolute file offsets.
const HEADER_SIZE: usize = 0x4A;
/// Fixed-width soundbank and wave-bank name fields.
const NAME_FIELD_LEN: usize = 0x40;

/// The format requires `total_cues == max(16, cue_count)`; with one cue this
/// is always the floor of 16.
const HASH_BUCKET_COUNT: u16 = 16;
const WAVEBANK_COUNT: u8 = 1;

/// The bare SE sound entry: 9-byte common prefix + `u16 wave_index` +
/// `u8 wavebank_index`, no trailing runtime-parameter-curve block.
const SE_SOUND_SIZE: usize = 12;
const CUE_ENTRY_SIZE: usize = 5;
const NAME_INDEX_ENTRY_SIZE: usize = 6;

/// Cue flags: bits 0 and 1 clear (no variation/transition table), bit 2 set
/// (playable sound cue). The engine's validator requires exactly this.
const CUE_FLAG_SOUND: u8 = 0x04;

/// Hash-table sentinels.
const EMPTY_BUCKET: u16 = 0xFFFF;
const END_OF_CHAIN: u16 = 0xFFFF;
const NO_OFFSET: i32 = -1;

/// Byte offset of the CRC field in the header.
const CRC_OFFSET: usize = 0x08;
/// First byte covered by the CRC (everything after CRC + timestamp).
const CRC_DATA_START: usize = 0x12;

/// Bare SIMPLE sound entry for a sound effect.
///
/// Layout: flags=0x00 (simple, **no** RPC block), category=6 (the gameplay-SE
/// mix bus), volume=254, pitch=0, priority=0, entry_length=12, wave_index=0,
/// wavebank_index=0. Values follow the game's own `se_normal.xsb`; volume 254
/// is what the system bank's `SYS_COIN` and `X_sys_OK1` use.
#[rustfmt::skip]
const SE_SIMPLE_SOUND_BYTES: [u8; SE_SOUND_SIZE] = [
    0x00,        // flags: simple, no runtime-parameter curve
    0x06, 0x00,  // category = 6 (gameplay SE mix bus)
    0xFE,        // volume = 254
    0x00, 0x00,  // pitch = 0
    0x00,        // priority = 0
    0x0C, 0x00,  // entry_length = 12
    0x00, 0x00,  // wave_index = 0
    0x00,        // wavebank_index = 0
];

/// Build a complete SE-profile XSB for `name` (1–16 ASCII alphanumerics —
/// enforced by the caller supplying the fixed bank name; asserted here).
///
/// `name` becomes both 64-byte name fields and the single cue's name. The
/// engine matches a sound bank to its wave bank by name and resolves cues
/// with a byte-exact `strcmp`, so the companion XWB's internal name must be
/// `name` **including case**, and playback must use exactly that cue name.
pub fn build_se(name: &str) -> Vec<u8> {
    let name_b = name.as_bytes();
    assert!(
        !name_b.is_empty()
            && name_b.len() <= 16
            && name_b.iter().all(|b| b.is_ascii_alphanumeric()),
        "XSB bank name must be 1-16 ASCII alphanumerics, got {name:?}"
    );

    // Section offsets (every one must equal the running byte cursor exactly,
    // and the cue-name string must run exactly to EOF — the engine's
    // validator checks both).
    let soundbank_name = HEADER_SIZE;
    let wavebank_name = soundbank_name + NAME_FIELD_LEN;
    let sound = wavebank_name + (WAVEBANK_COUNT as usize) * NAME_FIELD_LEN;
    let simple_cue = sound + SE_SOUND_SIZE;
    let hash_table = simple_cue + CUE_ENTRY_SIZE;
    let name_index = hash_table + (HASH_BUCKET_COUNT as usize) * 2;
    let cue_names = name_index + NAME_INDEX_ENTRY_SIZE;
    let cue_name_table_len = name_b.len() + 1;
    let total_size = cue_names + cue_name_table_len;

    let mut buf = vec![0u8; total_size];

    // -- Header. CRC at 0x08..0x0A is filled last; the 64-bit timestamp at
    //    0x0A..0x12 stays zero (the engine never validates it). --
    buf[0x00..0x04].copy_from_slice(&MAGIC.to_le_bytes());
    buf[0x04..0x06].copy_from_slice(&VERSION.to_le_bytes()); // content_version
    buf[0x06..0x08].copy_from_slice(&VERSION.to_le_bytes()); // tool_version
    buf[0x12] = PLATFORM;
    buf[0x13..0x15].copy_from_slice(&1u16.to_le_bytes()); // simple_cue_count
    buf[0x15..0x17].copy_from_slice(&0u16.to_le_bytes()); // complex_cue_count
                                                          // 0x17..0x19 unknown: must be 0 (already zeroed)
    buf[0x19..0x1B].copy_from_slice(&HASH_BUCKET_COUNT.to_le_bytes());
    buf[0x1B] = WAVEBANK_COUNT;
    buf[0x1C..0x1E].copy_from_slice(&1u16.to_le_bytes()); // sound_count
    buf[0x1E..0x20].copy_from_slice(&(cue_name_table_len as u16).to_le_bytes());
    // 0x20..0x22 unknown: must be 0 (already zeroed)
    put_i32(&mut buf, 0x22, simple_cue as i32);
    put_i32(&mut buf, 0x26, NO_OFFSET); // complex_cue_off
    put_i32(&mut buf, 0x2A, cue_names as i32);
    put_i32(&mut buf, 0x2E, NO_OFFSET); // unknown, must be -1
    put_i32(&mut buf, 0x32, NO_OFFSET); // variation_off
    put_i32(&mut buf, 0x36, NO_OFFSET); // transition_off
    put_i32(&mut buf, 0x3A, wavebank_name as i32);
    put_i32(&mut buf, 0x3E, hash_table as i32);
    put_i32(&mut buf, 0x42, name_index as i32);
    put_i32(&mut buf, 0x46, sound as i32);

    // -- Both 64-byte name fields (trailing bytes already zero). --
    buf[soundbank_name..soundbank_name + name_b.len()].copy_from_slice(name_b);
    buf[wavebank_name..wavebank_name + name_b.len()].copy_from_slice(name_b);

    // -- The one bare sound entry. --
    buf[sound..sound + SE_SOUND_SIZE].copy_from_slice(&SE_SIMPLE_SOUND_BYTES);

    // -- The one simple cue, pointing at the sound entry's file offset. --
    buf[simple_cue] = CUE_FLAG_SOUND;
    buf[simple_cue + 1..simple_cue + 5].copy_from_slice(&(sound as u32).to_le_bytes());

    // -- Hash table: every bucket empty except the cue name's, which holds
    //    cue index 0. --
    for b in 0..HASH_BUCKET_COUNT as usize {
        let off = hash_table + b * 2;
        buf[off..off + 2].copy_from_slice(&EMPTY_BUCKET.to_le_bytes());
    }
    let bucket = cue_name_hash_bucket(name_b, HASH_BUCKET_COUNT) as usize;
    let bucket_off = hash_table + bucket * 2;
    buf[bucket_off..bucket_off + 2].copy_from_slice(&0u16.to_le_bytes());

    // -- Name index: one entry, chain of length 1. --
    buf[name_index..name_index + 4].copy_from_slice(&(cue_names as u32).to_le_bytes());
    buf[name_index + 4..name_index + 6].copy_from_slice(&END_OF_CHAIN.to_le_bytes());

    // -- Cue name string (NUL terminator already zero). --
    buf[cue_names..cue_names + name_b.len()].copy_from_slice(name_b);

    // -- CRC-16 over [0x12..], stored at 0x08. --
    let crc = xact_crc16(&buf[CRC_DATA_START..]);
    buf[CRC_OFFSET..CRC_OFFSET + 2].copy_from_slice(&crc.to_le_bytes());

    buf
}

fn put_i32(buf: &mut [u8], offset: usize, value: i32) {
    buf[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

/// XACT2 cue-name hash bucket (matches `xactengine2_10.dll`'s `GetCueIndex`
/// helper). Per character: `h = 3*h + (h >> 1) + c`, all wrapping u16;
/// bucket = `h % bucket_count`.
fn cue_name_hash_bucket(name: &[u8], bucket_count: u16) -> u16 {
    let mut h: u16 = 0;
    for &c in name {
        h = h
            .wrapping_mul(3)
            .wrapping_add(h >> 1)
            .wrapping_add(c as u16);
    }
    h % bucket_count
}

/// CRC-16 used by the XACT2 engine to validate XSB contents. The engine
/// stores `!crc` at offset 0x08 and rejects the bank silently on mismatch.
fn xact_crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &b in data {
        crc = CRC_TABLE[((b as u16) ^ crc) as usize & 0xFF] ^ (crc >> 8);
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
