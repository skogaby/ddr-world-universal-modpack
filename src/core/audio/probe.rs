//! Header-level probe: codec, rate, channels and the exact frame count,
//! without decoding a sample.
//!
//! - **OGG/Vorbis** — walks every page header (27 bytes + lacing table) and
//!   takes the last valid granule position of the first Vorbis logical
//!   stream. `libvorbisfile` (StepMania) reports
//!   `last_granule − (first_audio_page_granule − frames_in_that_page)`;
//!   the subtracted term is zero for libvorbis, lancer, ffmpeg and every
//!   other encoder in circulation, so it is not computed here (it would
//!   need the setup header's mode table). If a cut stream ever starts
//!   late, the decoder yields fewer frames than probed and the tail is
//!   padded with silence — a layout never breaks.
//! - **MP3** — skips ID3v2 tags (and an ID3v1 trailer), parses the first
//!   frame header, then: `Xing`/`Info` frame count × samples-per-frame, else
//!   `VBRI`, else a header walk over the whole file. StepMania's `libmad`
//!   reader skips a VBR `Xing` frame but DECODES a CBR `Info` frame (and any
//!   `VBRI` frame) as silence; `symphonia` skips all three, so those two
//!   cases report `leading_silence_frames = samples_per_frame` and the
//!   decoder prepends them.
//! - **WAV** — `fmt ` + `data` chunks; frames = data length ÷ block align,
//!   clamped to the file. PCM (8/16/24/32-bit, plain or
//!   WAVE_FORMAT_EXTENSIBLE) and IEEE float only.

use super::{AudioError, AudioInfo, Codec, MAX_FRAMES};

const MAX_CHANNELS: u16 = 8;

/// Probe `bytes` (the whole file). Never panics on hostile input.
pub fn probe(bytes: &[u8]) -> Result<AudioInfo, AudioError> {
    if bytes.len() < 12 {
        return Err(AudioError::Truncated {
            what: "file header",
        });
    }
    let info = if bytes.starts_with(b"OggS") {
        probe_ogg(bytes)?
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WAVE") {
        probe_wav(bytes)?
    } else if bytes.starts_with(b"ID3") || looks_like_mp3(bytes) {
        probe_mp3(bytes)?
    } else {
        return Err(AudioError::UnknownFormat);
    };
    if info.frames_exact == 0 {
        return Err(AudioError::Empty);
    }
    if info.frames_exact > MAX_FRAMES {
        return Err(AudioError::TooLong {
            frames: info.frames_exact,
        });
    }
    if info.rate == 0 {
        return Err(AudioError::Malformed {
            what: "sample rate 0",
        });
    }
    if info.channels == 0 || info.channels > MAX_CHANNELS {
        return Err(AudioError::Unsupported {
            what: "channel count",
        });
    }
    Ok(info)
}

// ---------------------------------------------------------------------
// Byte helpers (all bounds-checked)
// ---------------------------------------------------------------------

fn u8_at(b: &[u8], i: usize) -> Option<u8> {
    b.get(i).copied()
}

fn u16_le(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes([u8_at(b, i)?, u8_at(b, i + 1)?]))
}

fn u32_le(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes([
        u8_at(b, i)?,
        u8_at(b, i + 1)?,
        u8_at(b, i + 2)?,
        u8_at(b, i + 3)?,
    ]))
}

fn u32_be(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_be_bytes([
        u8_at(b, i)?,
        u8_at(b, i + 1)?,
        u8_at(b, i + 2)?,
        u8_at(b, i + 3)?,
    ]))
}

fn u64_le(b: &[u8], i: usize) -> Option<u64> {
    let lo = u32_le(b, i)?;
    let hi = u32_le(b, i + 4)?;
    Some(u64::from(lo) | (u64::from(hi) << 32))
}

/// Position of `needle` in `hay[from..]`, as an absolute index.
fn find_from(hay: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    let tail = hay.get(from..)?;
    tail.windows(needle.len())
        .position(|w| w == needle)
        .map(|p| from + p)
}

// ---------------------------------------------------------------------
// OGG / Vorbis
// ---------------------------------------------------------------------

struct OggPage {
    header_type: u8,
    granule: u64,
    serial: u32,
    body_start: usize,
    body_len: usize,
    next: usize,
}

fn parse_ogg_page(bytes: &[u8], at: usize) -> Option<OggPage> {
    if bytes.get(at..at + 4)? != b"OggS" || u8_at(bytes, at + 4)? != 0 {
        return None;
    }
    let header_type = u8_at(bytes, at + 5)?;
    let granule = u64_le(bytes, at + 6)?;
    let serial = u32_le(bytes, at + 14)?;
    let nsegs = usize::from(u8_at(bytes, at + 26)?);
    let lacing = bytes.get(at + 27..at + 27 + nsegs)?;
    let body_len: usize = lacing.iter().map(|&l| usize::from(l)).sum();
    let body_start = at + 27 + nsegs;
    let next = body_start.checked_add(body_len)?;
    if next > bytes.len() {
        return None;
    }
    Some(OggPage {
        header_type,
        granule,
        serial,
        body_start,
        body_len,
        next,
    })
}

fn probe_ogg(bytes: &[u8]) -> Result<AudioInfo, AudioError> {
    let mut pos = 0usize;
    let mut vorbis: Option<(u32, u32, u16)> = None; // serial, rate, channels
    let mut last_granule: Option<u64> = None;
    // Every iteration advances `pos` by ≥ 1 byte, so the walk is bounded by
    // the file length.
    while pos < bytes.len() {
        let Some(page) = parse_ogg_page(bytes, pos) else {
            // Lost sync (corrupt page or trailing junk): resync forward.
            match find_from(bytes, pos + 1, b"OggS") {
                Some(next) => {
                    pos = next;
                    continue;
                }
                None => break,
            }
        };
        if vorbis.is_none() && page.header_type & 0x02 != 0 {
            // Beginning-of-stream page: identification header?
            let body = bytes
                .get(page.body_start..page.body_start + page.body_len)
                .unwrap_or(&[]);
            if body.len() >= 30 && body[0] == 0x01 && &body[1..7] == b"vorbis" {
                let version = u32_le(body, 7).unwrap_or(1);
                let channels = u16::from(body[11]);
                let rate = u32_le(body, 12).unwrap_or(0);
                if version != 0 {
                    return Err(AudioError::Unsupported {
                        what: "vorbis version",
                    });
                }
                vorbis = Some((page.serial, rate, channels));
            }
        }
        if let Some((serial, _, _)) = vorbis {
            if page.serial == serial && page.granule != u64::MAX {
                last_granule = Some(page.granule);
            }
        }
        pos = page.next;
    }
    let Some((_, rate, channels)) = vorbis else {
        return Err(AudioError::Unsupported {
            what: "ogg without a vorbis stream",
        });
    };
    let frames = last_granule.unwrap_or(0);
    Ok(AudioInfo {
        codec: Codec::Vorbis,
        rate,
        channels,
        frames_exact: frames,
        leading_silence_frames: 0,
        audio_start: 0,
        audio_end: bytes.len(),
    })
}

// ---------------------------------------------------------------------
// WAV
// ---------------------------------------------------------------------

fn probe_wav(bytes: &[u8]) -> Result<AudioInfo, AudioError> {
    let mut pos = 12usize;
    let mut fmt: Option<(u16, u16, u32, u16, u16)> = None; // tag, ch, rate, align, bits
    let mut data: Option<(usize, usize)> = None;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32_le(bytes, pos + 4).unwrap_or(0) as usize;
        let body_start = pos + 8;
        match id {
            b"fmt " => {
                let tag = u16_le(bytes, body_start).ok_or(AudioError::Truncated { what: "fmt" })?;
                let channels =
                    u16_le(bytes, body_start + 2).ok_or(AudioError::Truncated { what: "fmt" })?;
                let rate =
                    u32_le(bytes, body_start + 4).ok_or(AudioError::Truncated { what: "fmt" })?;
                let align =
                    u16_le(bytes, body_start + 12).ok_or(AudioError::Truncated { what: "fmt" })?;
                let bits =
                    u16_le(bytes, body_start + 14).ok_or(AudioError::Truncated { what: "fmt" })?;
                let effective_tag = if tag == 0xFFFE {
                    // WAVE_FORMAT_EXTENSIBLE: the sub-format GUID's first
                    // two bytes carry the real tag.
                    u16_le(bytes, body_start + 24).ok_or(AudioError::Truncated {
                        what: "fmt extensible",
                    })?
                } else {
                    tag
                };
                fmt = Some((effective_tag, channels, rate, align, bits));
            }
            b"data" => {
                // Streams written without a known length carry 0xFFFFFFFF
                // (or 0); clamp to what is actually present.
                let available = bytes.len().saturating_sub(body_start);
                let len = if size == 0 || size > available {
                    available
                } else {
                    size
                };
                data = Some((body_start, len));
                break;
            }
            _ => {}
        }
        // Chunks are word-aligned.
        let step = 8usize.saturating_add(size).saturating_add(size & 1);
        pos = pos.saturating_add(step);
        if step == 0 {
            break;
        }
    }
    let Some((tag, channels, rate, align, bits)) = fmt else {
        return Err(AudioError::Malformed {
            what: "wav without fmt chunk",
        });
    };
    let Some((_, data_len)) = data else {
        return Err(AudioError::Malformed {
            what: "wav without data chunk",
        });
    };
    match (tag, bits) {
        (1, 8 | 16 | 24 | 32) | (3, 32 | 64) => {}
        _ => {
            return Err(AudioError::Unsupported {
                what: "wav sample format",
            })
        }
    }
    if align == 0 || channels == 0 || u32::from(align) != u32::from(channels) * u32::from(bits) / 8
    {
        return Err(AudioError::Malformed {
            what: "wav block align",
        });
    }
    Ok(AudioInfo {
        codec: Codec::Wav,
        rate,
        channels,
        frames_exact: (data_len / usize::from(align)) as u64,
        leading_silence_frames: 0,
        audio_start: 0,
        audio_end: bytes.len(),
    })
}

// ---------------------------------------------------------------------
// MP3
// ---------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
struct Mp3Header {
    mpeg1: bool,
    rate: u32,
    channels: u16,
    /// Samples per frame (1152 for MPEG-1 Layer III, 576 for MPEG-2/2.5).
    spf: u32,
    /// Whole frame length in bytes, header included.
    frame_len: usize,
    has_crc: bool,
}

const BITRATES_V1_L3: [u32; 15] = [
    0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
];
const BITRATES_V2_L3: [u32; 15] = [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160];
const RATES_V1: [u32; 3] = [44_100, 48_000, 32_000];

fn parse_mp3_header(bytes: &[u8], at: usize) -> Option<Mp3Header> {
    let h = u32_be(bytes, at)?;
    if h & 0xFFE0_0000 != 0xFFE0_0000 {
        return None;
    }
    let version_bits = (h >> 19) & 0x3; // 0 = 2.5, 1 = reserved, 2 = 2, 3 = 1
    let layer_bits = (h >> 17) & 0x3; // 1 = Layer III
    let has_crc = (h >> 16) & 0x1 == 0;
    let bitrate_idx = ((h >> 12) & 0xF) as usize;
    let rate_idx = ((h >> 10) & 0x3) as usize;
    let padding = (h >> 9) & 0x1;
    let mode = (h >> 6) & 0x3;
    if version_bits == 1
        || layer_bits != 1
        || bitrate_idx == 0
        || bitrate_idx == 15
        || rate_idx == 3
    {
        return None;
    }
    let mpeg1 = version_bits == 3;
    let base_rate = RATES_V1[rate_idx];
    let rate = match version_bits {
        3 => base_rate,
        2 => base_rate / 2,
        _ => base_rate / 4,
    };
    let bitrate = if mpeg1 {
        BITRATES_V1_L3[bitrate_idx]
    } else {
        BITRATES_V2_L3[bitrate_idx]
    } * 1000;
    let spf = if mpeg1 { 1152 } else { 576 };
    let frame_len = (spf / 8 * bitrate / rate + padding) as usize;
    if frame_len <= 4 {
        return None;
    }
    Some(Mp3Header {
        mpeg1,
        rate,
        channels: if mode == 3 { 1 } else { 2 },
        spf,
        frame_len,
        has_crc,
    })
}

fn same_stream(a: &Mp3Header, b: &Mp3Header) -> bool {
    a.mpeg1 == b.mpeg1 && a.rate == b.rate && a.channels == b.channels
}

/// Byte length of one or more leading ID3v2 tags.
fn id3v2_len(bytes: &[u8], at: usize) -> Option<usize> {
    if bytes.get(at..at + 3)? != b"ID3" {
        return None;
    }
    let flags = u8_at(bytes, at + 5)?;
    let size_bytes = bytes.get(at + 6..at + 10)?;
    if size_bytes.iter().any(|&b| b & 0x80 != 0) {
        return None;
    }
    let size = size_bytes
        .iter()
        .fold(0usize, |acc, &b| (acc << 7) | usize::from(b & 0x7F));
    let footer = if flags & 0x10 != 0 { 10 } else { 0 };
    Some(10 + size + footer)
}

/// Does a plausible frame header (confirmed by a second one) sit at `at`?
fn frame_header_confirmed(bytes: &[u8], end: usize, at: usize) -> Option<Mp3Header> {
    let h = parse_mp3_header(bytes, at)?;
    let next = at.checked_add(h.frame_len)?;
    if next.checked_add(4)? > end {
        // Last frame of the file: accept on its own.
        return Some(h);
    }
    let n = parse_mp3_header(bytes, next)?;
    same_stream(&h, &n).then_some(h)
}

/// Find the first confirmed frame header in `[from, end)`.
fn find_first_frame(bytes: &[u8], from: usize, end: usize) -> Option<(usize, Mp3Header)> {
    let mut at = from;
    while at + 4 <= end {
        if bytes[at] == 0xFF && bytes[at + 1] & 0xE0 == 0xE0 {
            if let Some(h) = frame_header_confirmed(bytes, end, at) {
                return Some((at, h));
            }
        }
        at += 1;
    }
    None
}

fn looks_like_mp3(bytes: &[u8]) -> bool {
    // Headerless MP3s start on a sync word; confirm with a second frame.
    find_first_frame(bytes, 0, bytes.len().min(4096)).is_some_and(|(at, _)| at == 0)
}

fn probe_mp3(bytes: &[u8]) -> Result<AudioInfo, AudioError> {
    let mut start = 0usize;
    // Several stacked ID3v2 tags occur in the wild; the loop advances by
    // ≥ 10 bytes per tag.
    while let Some(len) = id3v2_len(bytes, start) {
        start = start
            .checked_add(len)
            .ok_or(AudioError::Malformed { what: "id3v2 size" })?;
        if start >= bytes.len() {
            return Err(AudioError::Truncated {
                what: "mp3 after id3v2",
            });
        }
    }
    let mut end = bytes.len();
    if end >= start + 128 && bytes.get(end - 128..end - 125) == Some(b"TAG") {
        end -= 128;
    }
    let (first, header) = find_first_frame(bytes, start, end).ok_or(AudioError::Malformed {
        what: "no mp3 frame header",
    })?;
    let spf = u64::from(header.spf);

    // Xing / Info / VBRI live in the first frame.
    let side_info = match (header.mpeg1, header.channels) {
        (true, 1) => 17,
        (true, _) => 32,
        (false, 1) => 9,
        (false, _) => 17,
    };
    let tag_at = first + 4 + usize::from(header.has_crc) * 2 + side_info;
    let tag = bytes.get(tag_at..tag_at + 4);
    let (frames, leading_silence) = if matches!(tag, Some(b"Xing") | Some(b"Info")) {
        let flags = u32_be(bytes, tag_at + 4).ok_or(AudioError::Truncated { what: "xing" })?;
        let count = if flags & 0x1 != 0 {
            u64::from(u32_be(bytes, tag_at + 8).ok_or(AudioError::Truncated { what: "xing" })?)
        } else {
            walk_mp3_frames(bytes, first + header.frame_len, end, &header)
        };
        // StepMania decodes a CBR `Info` frame as silence, skips `Xing`.
        let silence = if tag == Some(b"Info") { spf } else { 0 };
        (count * spf + silence, silence)
    } else if bytes.get(first + 4 + 32..first + 4 + 36) == Some(b"VBRI") {
        let count = u64::from(
            u32_be(bytes, first + 4 + 32 + 14).ok_or(AudioError::Truncated { what: "vbri" })?,
        );
        (count * spf + spf, spf)
    } else {
        (walk_mp3_frames(bytes, first, end, &header) * spf, 0)
    };
    Ok(AudioInfo {
        codec: Codec::Mp3,
        rate: header.rate,
        channels: header.channels,
        frames_exact: frames,
        leading_silence_frames: leading_silence,
        audio_start: start,
        audio_end: end,
    })
}

/// Count frames from `from` to `end`, resyncing over junk. Every step
/// advances by ≥ 1 byte.
fn walk_mp3_frames(bytes: &[u8], from: usize, end: usize, first: &Mp3Header) -> u64 {
    let mut count = 0u64;
    let mut at = from;
    while at + 4 <= end {
        match parse_mp3_header(bytes, at) {
            Some(h) if same_stream(first, &h) => {
                count += 1;
                at += h.frame_len;
            }
            _ => match find_first_frame(bytes, at + 1, end) {
                Some((next, h)) if same_stream(first, &h) => at = next,
                _ => break,
            },
        }
    }
    count
}

// ---------------------------------------------------------------------
// Tests (hand-built headers; real-encoder fixtures live in the harness)
// ---------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A 16-bit PCM WAV with `frames` frames of `channels` channels.
    pub fn build_wav(rate: u32, channels: u16, samples: &[i16]) -> Vec<u8> {
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

    fn ogg_page(header_type: u8, granule: u64, serial: u32, seq: u32, body: &[u8]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(b"OggS");
        v.push(0);
        v.push(header_type);
        v.extend_from_slice(&granule.to_le_bytes());
        v.extend_from_slice(&serial.to_le_bytes());
        v.extend_from_slice(&seq.to_le_bytes());
        v.extend_from_slice(&0u32.to_le_bytes());
        let mut lacing = Vec::new();
        let mut rem = body.len();
        while rem >= 255 {
            lacing.push(255);
            rem -= 255;
        }
        lacing.push(rem as u8);
        v.push(lacing.len() as u8);
        v.extend_from_slice(&lacing);
        v.extend_from_slice(body);
        v
    }

    fn vorbis_ident(rate: u32, channels: u8) -> Vec<u8> {
        let mut b = vec![0x01];
        b.extend_from_slice(b"vorbis");
        b.extend_from_slice(&0u32.to_le_bytes());
        b.push(channels);
        b.extend_from_slice(&rate.to_le_bytes());
        b.extend_from_slice(&[0u8; 12]);
        b.push(0xB8);
        b.push(0x01);
        b
    }

    #[test]
    fn wav_probe_counts_frames_from_data_chunk() {
        let samples: Vec<i16> = (0..1000).map(|i| (i % 7) as i16).collect();
        let wav = build_wav(44_100, 2, &samples);
        let info = probe(&wav).expect("probe");
        assert_eq!(info.codec, Codec::Wav);
        assert_eq!(info.rate, 44_100);
        assert_eq!(info.channels, 2);
        assert_eq!(info.frames_exact, 500);
        assert_eq!(info.leading_silence_frames, 0);
    }

    #[test]
    fn wav_probe_clamps_oversized_data_chunk() {
        let samples = vec![0i16; 64];
        let mut wav = build_wav(22_050, 1, &samples);
        let n = wav.len();
        wav[n - 128 - 4..n - 128].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        let info = probe(&wav).expect("probe");
        assert_eq!(info.frames_exact, 64);
        assert_eq!(info.rate, 22_050);
        assert_eq!(info.channels, 1);
    }

    #[test]
    fn wav_probe_rejects_non_pcm_and_truncation() {
        let mut wav = build_wav(44_100, 2, &[0; 8]);
        wav[20] = 2; // MS-ADPCM tag
        assert_eq!(
            probe(&wav),
            Err(AudioError::Unsupported {
                what: "wav sample format"
            })
        );
        assert!(matches!(
            probe(&wav[..10]),
            Err(AudioError::Truncated { .. })
        ));
        assert_eq!(probe(&[0u8; 64]), Err(AudioError::UnknownFormat));
    }

    #[test]
    fn ogg_probe_uses_last_granule_of_the_vorbis_serial() {
        let serial = 0xDEAD_BEEF;
        let mut f = ogg_page(0x02, 0, serial, 0, &vorbis_ident(48_000, 2));
        f.extend(ogg_page(0x00, 0, serial, 1, &[3, 0, 0]));
        f.extend(ogg_page(0x00, 1024, serial, 2, &[0; 300]));
        // A page of another stream with a larger granule must be ignored.
        f.extend(ogg_page(0x00, 99_999, 7, 0, &[0; 10]));
        f.extend(ogg_page(0x00, u64::MAX, serial, 3, &[0; 20]));
        f.extend(ogg_page(0x04, 123_456, serial, 4, &[0; 20]));
        let info = probe(&f).expect("probe");
        assert_eq!(info.codec, Codec::Vorbis);
        assert_eq!(info.rate, 48_000);
        assert_eq!(info.channels, 2);
        assert_eq!(info.frames_exact, 123_456);
    }

    #[test]
    fn ogg_probe_resyncs_over_junk_and_rejects_non_vorbis() {
        let serial = 5;
        let mut f = ogg_page(0x02, 0, serial, 0, &vorbis_ident(44_100, 1));
        f.extend_from_slice(b"junkjunkjunk");
        f.extend(ogg_page(0x04, 777, serial, 1, &[0; 5]));
        assert_eq!(probe(&f).expect("probe").frames_exact, 777);

        let mut opus = vec![0x4F, 0x70, 0x75, 0x73, 0x48, 0x65, 0x61, 0x64];
        opus.extend_from_slice(&[0u8; 30]);
        let f = ogg_page(0x02, 0, 9, 0, &opus);
        assert_eq!(
            probe(&f),
            Err(AudioError::Unsupported {
                what: "ogg without a vorbis stream"
            })
        );
        let f = ogg_page(0x02, 0, serial, 0, &vorbis_ident(44_100, 1));
        assert_eq!(probe(&f), Err(AudioError::Empty));
    }

    /// MPEG-1 Layer III, 44.1 kHz, 128 kbps, stereo, no CRC, no padding.
    const MP3_HDR: [u8; 4] = [0xFF, 0xFB, 0x90, 0x00];
    const MP3_FRAME_LEN: usize = 417;

    fn mp3_frame(body: &[u8]) -> Vec<u8> {
        let mut f = MP3_HDR.to_vec();
        f.extend_from_slice(body);
        f.resize(MP3_FRAME_LEN, 0);
        f
    }

    fn info_frame(tag: &[u8; 4], frames: u32) -> Vec<u8> {
        let mut body = vec![0u8; 32];
        body.extend_from_slice(tag);
        body.extend_from_slice(&0x0000_0001u32.to_be_bytes());
        body.extend_from_slice(&frames.to_be_bytes());
        mp3_frame(&body)
    }

    fn id3v2(payload_len: usize) -> Vec<u8> {
        let mut v = b"ID3\x04\x00\x00".to_vec();
        let n = payload_len as u32;
        v.extend_from_slice(&[
            ((n >> 21) & 0x7F) as u8,
            ((n >> 14) & 0x7F) as u8,
            ((n >> 7) & 0x7F) as u8,
            (n & 0x7F) as u8,
        ]);
        v.extend(std::iter::repeat_n(0xFFu8, payload_len));
        v
    }

    #[test]
    fn mp3_probe_header_fields_and_walk() {
        let mut f = Vec::new();
        for _ in 0..10 {
            f.extend(mp3_frame(&[]));
        }
        let info = probe(&f).expect("probe");
        assert_eq!(info.codec, Codec::Mp3);
        assert_eq!(info.rate, 44_100);
        assert_eq!(info.channels, 2);
        assert_eq!(info.frames_exact, 10 * 1152);
        assert_eq!(info.leading_silence_frames, 0);
        assert_eq!((info.audio_start, info.audio_end), (0, f.len()));
    }

    #[test]
    fn mp3_probe_skips_id3_and_trailer_and_reads_xing() {
        let mut f = id3v2(300);
        let start = f.len();
        f.extend(info_frame(b"Xing", 250));
        for _ in 0..3 {
            f.extend(mp3_frame(&[]));
        }
        let end = f.len();
        f.extend_from_slice(b"TAG");
        f.extend_from_slice(&[b'x'; 125]);
        let info = probe(&f).expect("probe");
        assert_eq!(info.frames_exact, 250 * 1152);
        assert_eq!(info.leading_silence_frames, 0);
        assert_eq!((info.audio_start, info.audio_end), (start, end));
    }

    #[test]
    fn mp3_probe_info_frame_adds_stepmania_silence() {
        let mut f = info_frame(b"Info", 40);
        f.extend(mp3_frame(&[]));
        let info = probe(&f).expect("probe");
        assert_eq!(info.frames_exact, 41 * 1152);
        assert_eq!(info.leading_silence_frames, 1152);
    }

    #[test]
    fn mp3_probe_walk_resyncs_over_junk() {
        let mut f = Vec::new();
        f.extend(mp3_frame(&[]));
        f.extend(mp3_frame(&[]));
        f.extend_from_slice(&[0x12; 77]);
        f.extend(mp3_frame(&[]));
        f.extend(mp3_frame(&[]));
        let info = probe(&f).expect("probe");
        assert_eq!(info.frames_exact, 4 * 1152);
    }

    #[test]
    fn mp3_probe_rejects_tag_only_file() {
        let f = id3v2(100);
        assert!(matches!(probe(&f), Err(AudioError::Truncated { .. })));
        let mut f = id3v2(100);
        f.extend_from_slice(&[0u8; 500]);
        assert!(matches!(probe(&f), Err(AudioError::Malformed { .. })));
    }
}
