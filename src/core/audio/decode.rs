//! Streaming decode + normalisation into 128-frame stereo blocks.
//!
//! Chain per source packet: `symphonia` decode → interleaved `i16` →
//! channels to stereo (mono duplicated; three or more channels folded to
//! their mean on both sides) → rate: 44.1 / 48 kHz pass through, anything
//! else goes through `core::xact::resample::ResampleState` (the rate
//! engine's deterministic linear resampler) over a sliding window of
//! source frames, so odd rates never need the whole song in memory.
//!
//! [`Decoder`] (the main entry) emits exactly `total_frames` frames:
//! `lead_in_frames` of silence, the normalised stream, then silence padding
//! (or truncation). A decode error mid-stream turns the rest into silence
//! and is reported through [`Decoder::failed`] — the bank layout the
//! planner committed to is honoured either way. The last block is zero
//! padded when `total_frames` is not a block multiple.
//!
//! Seeking (`PreviewDecoder`) is done in the normalised frame domain: the
//! `symphonia` timestamp of raw frame 0 is learned from the first packet,
//! the reader is seeked accurately (MP3 bit-reservoir reference frames and
//! Vorbis warm-up are the demuxers' business), the decoder is reset and
//! frames before the target are dropped.

use std::io::Cursor;

use symphonia::core::audio::GenericAudioBufferRef;
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::codecs::CodecParameters;
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::packet::Packet;
use symphonia::core::units::Timestamp;

use super::{
    normalised_frames, AudioError, AudioInfo, Codec, StereoBlock, BLOCK_FRAMES, NATIVE_RATES,
};
use crate::core::xact::resample::ResampleState;
use crate::core::xact::stretch::SourcePcm;

/// Source frames decoded and discarded before a seek target (two MPEG-1
/// frames), see `Stream::seek_to`.
const SEEK_WARMUP_FRAMES: u64 = 2 * 1152;

/// What the main decode must produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetFormat {
    /// 44100 or 48000; must equal [`AudioInfo::target_rate`].
    pub rate: u32,
    /// Silence prepended (see `lead_in::choose_lead_in`).
    pub lead_in_frames: u64,
    /// Frames emitted in total, lead-in included (the planner's entry
    /// duration — a block multiple).
    pub total_frames: u64,
}

impl TargetFormat {
    /// The natural target for `info`: its normalised rate, `lead_in`
    /// frames of silence, and the stream rounded up to whole blocks.
    #[must_use]
    pub fn for_source(info: &AudioInfo, lead_in_frames: u64) -> Self {
        Self {
            rate: info.target_rate(),
            lead_in_frames,
            total_frames: super::round_up_to_blocks(
                lead_in_frames.saturating_add(info.target_frames()),
            ),
        }
    }
}

/// Block iterator over one audio file. See the module docs.
pub struct Decoder<'a> {
    stream: Option<Stream<'a>>,
    lead_in_left: u64,
    remaining: u64,
    emitted: u64,
    /// Normalised stereo samples decoded but not yet emitted.
    pending: Vec<i16>,
    pending_pos: usize,
    failed: Option<AudioError>,
    fade: Option<Fade>,
    /// Frame index (within the content, lead-in excluded) for the fade.
    content_pos: u64,
}

/// Linear fade-in / fade-out over `len` content frames.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Fade {
    pub len: u64,
    pub fade_in: u64,
    pub fade_out: u64,
}

impl Fade {
    /// Gain in Q15 for content frame `i`.
    fn gain_q15(&self, i: u64) -> i32 {
        let mut g: i64 = 32768;
        if self.fade_in > 0 && i < self.fade_in {
            g = g * i as i64 / self.fade_in as i64;
        }
        if self.fade_out > 0 && i.saturating_add(self.fade_out) > self.len {
            let left = self.len.saturating_sub(i) as i64;
            g = g * left / self.fade_out as i64;
        }
        g.clamp(0, 32768) as i32
    }
}

impl<'a> Decoder<'a> {
    /// Open `bytes` (probed as `info`) for the main song entry.
    pub fn open(
        bytes: &'a [u8],
        info: &AudioInfo,
        target: TargetFormat,
    ) -> Result<Self, AudioError> {
        if target.rate != info.target_rate() {
            return Err(AudioError::BadTarget {
                what: "rate differs from the normalised source rate",
            });
        }
        if target.lead_in_frames > target.total_frames {
            return Err(AudioError::BadTarget {
                what: "lead-in longer than total",
            });
        }
        let stream = Stream::open(bytes, info)?;
        Ok(Self {
            stream: Some(stream),
            lead_in_left: target.lead_in_frames,
            remaining: target.total_frames,
            emitted: 0,
            pending: Vec::new(),
            pending_pos: 0,
            failed: None,
            fade: None,
            content_pos: 0,
        })
    }

    /// Open a window `[start_frame, start_frame + len_frames)` of the
    /// normalised stream, padded to `total_frames` (≥ `len_frames`), with
    /// optional fades. Used by `PreviewDecoder`.
    pub(crate) fn open_window(
        bytes: &'a [u8],
        info: &AudioInfo,
        start_frame: u64,
        len_frames: u64,
        total_frames: u64,
        fade: Option<Fade>,
    ) -> Result<Self, AudioError> {
        if total_frames < len_frames {
            return Err(AudioError::BadTarget {
                what: "window total shorter than its length",
            });
        }
        let mut stream = Stream::open(bytes, info)?;
        stream.seek_to(start_frame)?;
        stream.limit = Some(len_frames);
        Ok(Self {
            stream: Some(stream),
            lead_in_left: 0,
            remaining: total_frames,
            emitted: 0,
            pending: Vec::new(),
            pending_pos: 0,
            failed: None,
            fade,
            content_pos: 0,
        })
    }

    /// The error that silenced the remainder, if any.
    #[must_use]
    pub fn failed(&self) -> Option<&AudioError> {
        self.failed.as_ref()
    }

    /// Frames emitted so far (lead-in and padding included).
    #[must_use]
    pub fn frames_emitted(&self) -> u64 {
        self.emitted
    }

    /// Frames still to emit.
    #[must_use]
    pub fn frames_remaining(&self) -> u64 {
        self.remaining
    }

    /// Refill `pending`; returns false once the stream is exhausted or
    /// failed (both leave `self.stream == None`).
    fn refill(&mut self) -> bool {
        let Some(stream) = self.stream.as_mut() else {
            return false;
        };
        self.pending.clear();
        self.pending_pos = 0;
        loop {
            match stream.next_frames(&mut self.pending) {
                Ok(true) if !self.pending.is_empty() => return true,
                Ok(true) => continue,
                Ok(false) => {
                    self.stream = None;
                    return false;
                }
                Err(e) => {
                    self.failed = Some(e);
                    self.stream = None;
                    return false;
                }
            }
        }
    }
}

impl Iterator for Decoder<'_> {
    type Item = StereoBlock;

    fn next(&mut self) -> Option<StereoBlock> {
        if self.remaining == 0 {
            return None;
        }
        let mut block: StereoBlock = [0; BLOCK_FRAMES * 2];
        let mut filled = 0usize;
        while filled < BLOCK_FRAMES && self.remaining > 0 {
            let room = (BLOCK_FRAMES - filled) as u64;
            if self.lead_in_left > 0 {
                let take = room.min(self.lead_in_left).min(self.remaining);
                filled += take as usize;
                self.lead_in_left -= take;
                self.remaining -= take;
                self.emitted += take;
                continue;
            }
            if self.pending_pos >= self.pending.len() && !self.refill() {
                // Exhausted: the rest of this block (and the stream) is
                // silence, already zeroed.
                let take = room.min(self.remaining);
                filled += take as usize;
                self.remaining -= take;
                self.emitted += take;
                continue;
            }
            let avail_frames = ((self.pending.len() - self.pending_pos) / 2) as u64;
            let take = room.min(avail_frames).min(self.remaining) as usize;
            let src = &self.pending[self.pending_pos..self.pending_pos + take * 2];
            let dst = &mut block[filled * 2..(filled + take) * 2];
            match self.fade {
                None => dst.copy_from_slice(src),
                Some(fade) => {
                    for (i, pair) in src.chunks_exact(2).enumerate() {
                        let g = fade.gain_q15(self.content_pos + i as u64);
                        dst[2 * i] = ((i32::from(pair[0]) * g) >> 15) as i16;
                        dst[2 * i + 1] = ((i32::from(pair[1]) * g) >> 15) as i16;
                    }
                }
            }
            self.content_pos += take as u64;
            self.pending_pos += take * 2;
            filled += take;
            self.remaining -= take as u64;
            self.emitted += take as u64;
        }
        Some(block)
    }
}

/// Decode the whole file to normalised stereo at `info.target_rate()`,
/// without lead-in or padding. Memory = 4 bytes per output frame; for
/// host tooling and the auto-sync analysis, not the bank writer.
pub fn decode_whole(bytes: &[u8], info: &AudioInfo) -> Result<Vec<i16>, AudioError> {
    let mut stream = Stream::open(bytes, info)?;
    let mut out = Vec::new();
    let expected = usize::try_from(info.target_frames().saturating_mul(2)).unwrap_or(0);
    out.try_reserve_exact(expected)
        .map_err(|_| AudioError::Decode("allocation failed".into()))?;
    while stream.next_frames(&mut out)? {}
    Ok(out)
}

// ---------------------------------------------------------------------
// Source stream: symphonia + channel/rate normalisation
// ---------------------------------------------------------------------

enum Rate {
    PassThrough,
    Resample {
        state: ResampleState,
        window: WindowPcm,
        /// Output frames produced so far (for `positioned_at` bookkeeping).
        produced: usize,
        output_frames: usize,
    },
}

struct Stream<'a> {
    reader: Box<dyn FormatReader + 'a>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    source_rate: u32,
    source_frames: u64,
    /// Silence still to synthesise in front of the first decoded frame.
    silence_left: u64,
    leading_silence: u64,
    /// Decoder trims delay/padding (Vorbis); then a packet's decoded
    /// frames start at `pts + trim_start`, else at `pts`.
    gapless: bool,
    /// `symphonia` timestamp of raw decoded frame 0 (the first packet's
    /// effective pts, see `effective_pts`).
    first_pts: Option<i64>,
    /// Drop decoded frames until this raw position (after a seek).
    skip_until_raw: u64,
    /// Source frames (silence included) handed to the normaliser so far.
    source_pos: u64,
    /// Normalised output frames handed out since open / the last seek.
    out_pos: u64,
    /// Stop after this many normalised frames (window decode).
    limit: Option<u64>,
    rate: Rate,
    ended: bool,
    scratch: Vec<i16>,
    stereo: Vec<i16>,
}

impl<'a> Stream<'a> {
    fn open(bytes: &'a [u8], info: &AudioInfo) -> Result<Self, AudioError> {
        let audio = info.audio_bytes(bytes);
        if audio.is_empty() {
            return Err(AudioError::Truncated { what: "audio data" });
        }
        let mss = MediaSourceStream::new(
            Box::new(Cursor::new(audio)),
            MediaSourceStreamOptions::default(),
        );
        let opts = FormatOptions::default();
        let reader: Box<dyn FormatReader + 'a> = match info.codec {
            Codec::Vorbis => Box::new(
                symphonia::default::formats::OggReader::try_new(mss, opts).map_err(open_err)?,
            ),
            Codec::Mp3 => Box::new(
                symphonia::default::formats::MpaReader::try_new(mss, opts).map_err(open_err)?,
            ),
            Codec::Wav => Box::new(
                symphonia::default::formats::WavReader::try_new(mss, opts).map_err(open_err)?,
            ),
        };
        let track = reader
            .default_track(TrackType::Audio)
            .ok_or_else(|| AudioError::Open("no audio track".into()))?;
        let track_id = track.id;
        let params = match &track.codec_params {
            Some(CodecParameters::Audio(p)) => p.clone(),
            _ => return Err(AudioError::Open("no audio codec parameters".into())),
        };
        // StepMania parity: libmad never trims (gapless off); libvorbisfile
        // drops the first packet and trims to the granule (gapless on).
        let gapless = info.codec != Codec::Mp3;
        let decoder = symphonia::default::get_codecs()
            .make_audio_decoder(&params, &AudioDecoderOptions::default().gapless(gapless))
            .map_err(open_err)?;
        let source_channels = params
            .channels
            .as_ref()
            .map(|c| c.count())
            .unwrap_or(usize::from(info.channels));
        let source_rate = params.sample_rate.unwrap_or(info.rate);
        if source_rate == 0 || source_channels == 0 {
            return Err(AudioError::Open("track without rate or channels".into()));
        }
        let rate = if NATIVE_RATES.contains(&source_rate) {
            Rate::PassThrough
        } else {
            let source_frames =
                usize::try_from(info.frames_exact).map_err(|_| AudioError::TooLong {
                    frames: info.frames_exact,
                })?;
            let output_frames = usize::try_from(normalised_frames(info.frames_exact, source_rate))
                .map_err(|_| AudioError::TooLong {
                    frames: info.frames_exact,
                })?;
            let state = ResampleState::new(source_frames, output_frames, 2, None)
                .map_err(|e| AudioError::Open(format!("resampler: {e}")))?;
            Rate::Resample {
                state,
                window: WindowPcm::new(source_frames),
                produced: 0,
                output_frames,
            }
        };
        Ok(Self {
            reader,
            decoder,
            track_id,
            source_rate,
            source_frames: info.frames_exact,
            gapless,
            silence_left: info.leading_silence_frames,
            leading_silence: info.leading_silence_frames,
            first_pts: None,
            skip_until_raw: 0,
            source_pos: 0,
            out_pos: 0,
            limit: None,
            rate,
            ended: false,
            scratch: Vec::new(),
            stereo: Vec::new(),
        })
    }

    /// Timestamp of the first frame the decoder will OUTPUT for `packet`.
    fn effective_pts(&self, packet: &Packet) -> i64 {
        let trim = if self.gapless {
            i64::try_from(packet.trim_start.get()).unwrap_or(0)
        } else {
            0
        };
        packet.pts.get().saturating_add(trim)
    }

    /// Normalised output frames the whole stream has.
    fn output_frames(&self) -> u64 {
        normalised_frames(self.source_frames, self.source_rate)
    }

    /// Learn the timestamp of raw frame 0 by peeking the first packet
    /// (only needed before a seek; the main path learns it as it goes).
    fn learn_first_pts(&mut self) -> Result<i64, AudioError> {
        if let Some(p) = self.first_pts {
            return Ok(p);
        }
        match self.reader.next_packet() {
            Ok(Some(packet)) => {
                let pts = self.effective_pts(&packet);
                self.first_pts = Some(pts);
                Ok(pts)
            }
            Ok(None) => Err(AudioError::Seek("stream has no packets".into())),
            Err(e) => Err(AudioError::Seek(e.to_string())),
        }
    }

    /// Position the stream so the next normalised frame is `out_frame`.
    fn seek_to(&mut self, out_frame: u64) -> Result<(), AudioError> {
        let total_out = self.output_frames();
        if out_frame >= total_out {
            return Err(AudioError::Seek("window starts past the end".into()));
        }
        // Source frame (silence included) the output frame maps to.
        let src_frame = if NATIVE_RATES.contains(&self.source_rate) {
            out_frame
        } else {
            // Resampler positions are `out · source/output`; start the
            // window two frames early so the interpolation base is inside it.
            (((u128::from(out_frame) * u128::from(self.source_frames))
                / u128::from(total_out.max(1))) as u64)
                .saturating_sub(2)
        };
        let src_frame = src_frame.min(self.source_frames.saturating_sub(1));
        let first_pts = self.learn_first_pts()?;

        if src_frame < self.leading_silence {
            // Inside the synthesised silence: rewind to the start.
            self.reader
                .seek(
                    SeekMode::Accurate,
                    SeekTo::Timestamp {
                        ts: Timestamp::new(first_pts),
                        track_id: self.track_id,
                    },
                )
                .map_err(|e| AudioError::Seek(e.to_string()))?;
            self.decoder.reset();
            self.silence_left = self.leading_silence - src_frame;
            self.skip_until_raw = 0;
        } else {
            let raw_target = src_frame - self.leading_silence;
            // Decoders carry state across frames beyond what the demuxers
            // rewind for (MP3's synthesis filterbank history, Vorbis'
            // overlap): start two MP3 frames early and discard, so the
            // window's first samples equal a continuous decode's.
            let seek_raw = raw_target.saturating_sub(SEEK_WARMUP_FRAMES);
            let ts = first_pts
                .checked_add(
                    i64::try_from(seek_raw).map_err(|_| AudioError::Seek("overflow".into()))?,
                )
                .ok_or_else(|| AudioError::Seek("timestamp overflow".into()))?;
            self.reader
                .seek(
                    SeekMode::Accurate,
                    SeekTo::Timestamp {
                        ts: Timestamp::new(ts),
                        track_id: self.track_id,
                    },
                )
                .map_err(|e| AudioError::Seek(e.to_string()))?;
            self.decoder.reset();
            self.silence_left = 0;
            // Packets now arrive from the demuxer's actual position (≤ the
            // request); decoded frames before the target are dropped by pts.
            self.skip_until_raw = raw_target;
        }
        self.source_pos = src_frame;
        // `out_pos` counts frames handed out since this seek (the window
        // limit is relative to its start).
        self.out_pos = 0;
        if let Rate::Resample {
            state,
            window,
            produced,
            ..
        } = &mut self.rate
        {
            let out_usize = usize::try_from(out_frame).unwrap_or(usize::MAX);
            state.positioned_at(out_usize);
            *produced = out_usize;
            window.reset(usize::try_from(src_frame).unwrap_or(0));
        }
        self.ended = false;
        self.stereo.clear();
        Ok(())
    }

    /// Append the next run of normalised stereo frames to `out`. `Ok(false)`
    /// once the stream (or the window limit) is exhausted.
    fn next_frames(&mut self, out: &mut Vec<i16>) -> Result<bool, AudioError> {
        if self.ended {
            return Ok(false);
        }
        if let Some(limit) = self.limit {
            if self.out_pos >= limit {
                self.ended = true;
                return Ok(false);
            }
        }
        let before = out.len();
        match &self.rate {
            Rate::PassThrough => {
                if !self.pull_source_frames()? {
                    self.ended = true;
                    return Ok(false);
                }
                out.extend_from_slice(&self.stereo);
                self.stereo.clear();
            }
            Rate::Resample { .. } => {
                if !self.produce_resampled(out)? {
                    self.ended = true;
                    return Ok(false);
                }
            }
        }
        let mut added = ((out.len() - before) / 2) as u64;
        if let Some(limit) = self.limit {
            let room = limit.saturating_sub(self.out_pos);
            if added > room {
                out.truncate(before + (room as usize) * 2);
                added = room;
            }
        }
        self.out_pos += added;
        Ok(true)
    }

    /// Decode one source step (silence run or packet) into `self.stereo`
    /// as normalised-channel stereo at the SOURCE rate. `Ok(false)` at end.
    fn pull_source_frames(&mut self) -> Result<bool, AudioError> {
        self.stereo.clear();
        if self.source_pos >= self.source_frames {
            return Ok(false);
        }
        if self.silence_left > 0 {
            let n = self
                .silence_left
                .min(1152)
                .min(self.source_frames - self.source_pos);
            self.stereo.resize((n as usize) * 2, 0);
            self.silence_left -= n;
            self.source_pos += n;
            return Ok(true);
        }
        loop {
            let packet = match self.reader.next_packet() {
                Ok(Some(p)) => p,
                Ok(None) | Err(SymError::ResetRequired) => return Ok(false),
                Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    return Ok(false)
                }
                Err(e) => return Err(AudioError::Decode(e.to_string())),
            };
            if packet.track_id != self.track_id {
                continue;
            }
            let effective = self.effective_pts(&packet);
            let first_pts = *self.first_pts.get_or_insert(effective);
            // Raw position of this packet's first decoded frame, from the
            // demuxer's clock (exact after seeks).
            let pts_raw = effective.saturating_sub(first_pts).max(0) as u64;
            let buf = match self.decoder.decode(&packet) {
                Ok(b) => b,
                Err(SymError::DecodeError(_)) | Err(SymError::IoError(_)) => {
                    // libmad/libvorbis skip a bad frame; so do we.
                    continue;
                }
                Err(e) => return Err(AudioError::Decode(e.to_string())),
            };
            let frames = buf.frames();
            if frames == 0 {
                continue;
            }
            let start_raw = pts_raw;
            convert_to_stereo(&buf, &mut self.scratch, &mut self.stereo);
            // Post-seek: drop frames before the target.
            if self.skip_until_raw > start_raw {
                let drop = (self.skip_until_raw - start_raw).min(frames as u64) as usize;
                self.stereo.drain(..drop * 2);
                if self.stereo.is_empty() {
                    continue;
                }
            }
            // Truncate at the probed length.
            let have = (self.stereo.len() / 2) as u64;
            let room = self.source_frames - self.source_pos;
            if have > room {
                self.stereo.truncate((room as usize) * 2);
            }
            self.source_pos += (self.stereo.len() / 2) as u64;
            return Ok(true);
        }
    }

    /// Resample path: produce up to one block of output frames.
    fn produce_resampled(&mut self, out: &mut Vec<i16>) -> Result<bool, AudioError> {
        let Rate::Resample {
            output_frames,
            produced,
            ..
        } = &self.rate
        else {
            return Ok(false);
        };
        let output_frames = *output_frames;
        if *produced >= output_frames {
            return Ok(false);
        }
        let want = BLOCK_FRAMES.min(output_frames - *produced);
        // Source frames needed: positions up to `(produced + want) ·
        // source/output` plus the interpolation neighbour and rounding slack.
        let needed_end = ((u128::from((*produced + want) as u64) * u128::from(self.source_frames))
            / u128::from(output_frames as u64)) as usize
            + 4;
        self.fill_window(needed_end)?;
        let Rate::Resample {
            state,
            window,
            produced,
            ..
        } = &mut self.rate
        else {
            return Ok(false);
        };
        let mut tmp = [0i16; BLOCK_FRAMES * 2];
        let result = state
            .produce(window, &mut tmp[..want * 2])
            .map_err(|e| AudioError::Decode(format!("resampler: {e}")))?;
        out.extend_from_slice(&tmp[..result.frames * 2]);
        *produced += result.frames;
        // Frames before the next read's base can go.
        let next_base = ((u128::from(*produced as u64) * u128::from(self.source_frames))
            / u128::from(output_frames as u64)) as usize;
        window.drop_before(next_base.saturating_sub(2));
        Ok(result.frames > 0)
    }

    /// Decode source frames into the window until it covers `needed_end`
    /// (exclusive) or the source ends (then the window pads silence).
    fn fill_window(&mut self, needed_end: usize) -> Result<(), AudioError> {
        loop {
            let covered = match &self.rate {
                Rate::Resample { window, .. } => window.end(),
                Rate::PassThrough => return Ok(()),
            };
            if covered >= needed_end.min(usize::try_from(self.source_frames).unwrap_or(usize::MAX))
            {
                return Ok(());
            }
            if !self.pull_source_frames()? {
                if let Rate::Resample { window, .. } = &mut self.rate {
                    window.pad_to_end();
                }
                return Ok(());
            }
            if let Rate::Resample { window, .. } = &mut self.rate {
                window.append(&self.stereo);
            }
            self.stereo.clear();
        }
    }
}

fn open_err(e: SymError) -> AudioError {
    AudioError::Open(e.to_string())
}

/// `buf` → interleaved stereo i16 in `stereo` (appended).
fn convert_to_stereo(
    buf: &GenericAudioBufferRef<'_>,
    scratch: &mut Vec<i16>,
    stereo: &mut Vec<i16>,
) {
    buf.copy_to_vec_interleaved::<i16>(scratch);
    let channels = buf.num_planes().max(1);
    let frames = scratch.len() / channels;
    stereo.reserve(frames * 2);
    match channels {
        1 => {
            for &s in scratch.iter() {
                stereo.push(s);
                stereo.push(s);
            }
        }
        2 => stereo.extend_from_slice(&scratch[..frames * 2]),
        n => {
            for frame in scratch.chunks_exact(n) {
                let sum: i32 = frame.iter().map(|&s| i32::from(s)).sum();
                let avg = (sum / n as i32) as i16;
                stereo.push(avg);
                stereo.push(avg);
            }
        }
    }
}

// ---------------------------------------------------------------------
// Sliding source window for the resampler
// ---------------------------------------------------------------------

/// A `SourcePcm` over a moving range of decoded stereo frames. Reads
/// outside the buffered range return the nearest edge sample (never a
/// panic): the producer sizes the window so this does not happen.
struct WindowPcm {
    total: usize,
    start: usize,
    buf: Vec<i16>,
    padded: bool,
}

impl WindowPcm {
    fn new(total: usize) -> Self {
        Self {
            total,
            start: 0,
            buf: Vec::new(),
            padded: false,
        }
    }

    fn reset(&mut self, start: usize) {
        self.start = start.min(self.total);
        self.buf.clear();
        self.padded = false;
    }

    fn end(&self) -> usize {
        if self.padded {
            self.total
        } else {
            self.start + self.buf.len() / 2
        }
    }

    fn append(&mut self, stereo: &[i16]) {
        self.buf.extend_from_slice(stereo);
    }

    fn pad_to_end(&mut self) {
        self.padded = true;
    }

    fn drop_before(&mut self, frame: usize) {
        if frame <= self.start {
            return;
        }
        let drop_frames = (frame - self.start).min(self.buf.len() / 2);
        self.buf.drain(..drop_frames * 2);
        self.start += drop_frames;
    }
}

impl SourcePcm for WindowPcm {
    fn frames(&self) -> usize {
        self.total
    }

    fn channels(&self) -> usize {
        2
    }

    fn sample(&self, frame: usize, channel: usize) -> i16 {
        let len = self.buf.len() / 2;
        if frame < self.start {
            return self.buf.get(channel.min(1)).copied().unwrap_or(0);
        }
        let rel = frame - self.start;
        if rel >= len {
            if self.padded || len == 0 {
                return 0;
            }
            return self.buf[(len - 1) * 2 + channel.min(1)];
        }
        self.buf[rel * 2 + channel.min(1)]
    }
}

#[cfg(test)]
mod tests {
    use super::super::probe::{self, tests::build_wav};
    use super::*;

    fn tone(frames: usize, channels: usize) -> Vec<i16> {
        let mut v = Vec::with_capacity(frames * channels);
        for i in 0..frames {
            for c in 0..channels {
                v.push(((i * 37 + c * 1000) % 20000) as i16 - 10000);
            }
        }
        v
    }

    #[test]
    fn wav_passthrough_emits_exactly_total_with_lead_in_and_padding() {
        let pcm = tone(1000, 2);
        let wav = build_wav(44_100, 2, &pcm);
        let info = probe::probe(&wav).expect("probe");
        assert_eq!(info.frames_exact, 1000);
        let target = TargetFormat {
            rate: 44_100,
            lead_in_frames: 256,
            total_frames: 1536,
        };
        let mut dec = Decoder::open(&wav, &info, target).expect("open");
        let blocks: Vec<StereoBlock> = dec.by_ref().collect();
        assert_eq!(blocks.len(), 12);
        assert!(dec.failed().is_none());
        assert_eq!(dec.frames_emitted(), 1536);
        let flat: Vec<i16> = blocks.iter().flat_map(|b| b.iter().copied()).collect();
        assert!(flat[..512].iter().all(|&s| s == 0));
        assert_eq!(&flat[512..512 + 2000], &pcm[..]);
        assert!(flat[512 + 2000..].iter().all(|&s| s == 0));
    }

    #[test]
    fn truncation_and_partial_last_block() {
        let pcm = tone(1000, 2);
        let wav = build_wav(48_000, 2, &pcm);
        let info = probe::probe(&wav).expect("probe");
        let target = TargetFormat {
            rate: 48_000,
            lead_in_frames: 0,
            total_frames: 300,
        };
        let blocks: Vec<StereoBlock> = Decoder::open(&wav, &info, target).expect("open").collect();
        assert_eq!(blocks.len(), 3);
        let flat: Vec<i16> = blocks.iter().flat_map(|b| b.iter().copied()).collect();
        assert_eq!(&flat[..600], &pcm[..600]);
        assert!(flat[600..].iter().all(|&s| s == 0));
    }

    #[test]
    fn mono_22050_is_duplicated_and_resampled_to_44100() {
        let frames = 2205;
        let pcm = tone(frames, 1);
        let wav = build_wav(22_050, 1, &pcm);
        let info = probe::probe(&wav).expect("probe");
        assert_eq!(info.target_rate(), 44_100);
        assert_eq!(info.target_frames(), 4410);
        let target = TargetFormat::for_source(&info, 1280);
        assert_eq!(
            target.total_frames,
            super::super::round_up_to_blocks(1280 + 4410)
        );
        let mut dec = Decoder::open(&wav, &info, target).expect("open");
        let blocks: Vec<StereoBlock> = dec.by_ref().collect();
        assert!(dec.failed().is_none(), "{:?}", dec.failed());
        let flat: Vec<i16> = blocks.iter().flat_map(|b| b.iter().copied()).collect();
        assert_eq!(flat.len() as u64, target.total_frames * 2);
        assert!(flat[..1280 * 2].iter().all(|&s| s == 0));
        // Even output frames land on source frames exactly (ratio 2:1);
        // both channels carry the mono signal.
        let content = &flat[1280 * 2..(1280 + 4410) * 2];
        for k in 0..frames {
            assert_eq!(content[(2 * k) * 2], pcm[k], "frame {k}");
            assert_eq!(content[(2 * k) * 2 + 1], pcm[k], "frame {k} R");
        }
        // Odd frames interpolate between neighbours.
        for k in 0..frames - 1 {
            let (a, b) = (i32::from(pcm[k]), i32::from(pcm[k + 1]));
            let got = i32::from(content[(2 * k + 1) * 2]);
            assert!(
                (got - (a + b) / 2).abs() <= 1,
                "frame {k}: {got} vs {a}/{b}"
            );
        }
        // Reference: whole-buffer resampler agrees sample for sample.
        let stereo_src: Vec<i16> = pcm.iter().flat_map(|&s| [s, s]).collect();
        let reference =
            crate::core::xact::resample::resample_interleaved(&stereo_src, 2, 4410, None)
                .expect("reference");
        assert_eq!(content, &reference[..]);
        assert!(flat[(1280 + 4410) * 2..].iter().all(|&s| s == 0));
    }

    #[test]
    fn six_channels_fold_to_their_mean() {
        let mut pcm = Vec::new();
        for _ in 0..300 {
            pcm.extend_from_slice(&[600, 0, 0, 0, 0, 0]);
        }
        let wav = build_wav(44_100, 6, &pcm);
        let info = probe::probe(&wav).expect("probe");
        let target = TargetFormat::for_source(&info, 0);
        let flat: Vec<i16> = Decoder::open(&wav, &info, target)
            .expect("open")
            .flat_map(|b| b.into_iter())
            .collect();
        assert!(flat[..600].iter().all(|&s| s == 100));
    }

    #[test]
    fn decode_whole_matches_source_for_passthrough() {
        let pcm = tone(777, 2);
        let wav = build_wav(44_100, 2, &pcm);
        let info = probe::probe(&wav).expect("probe");
        assert_eq!(decode_whole(&wav, &info).expect("decode"), pcm);
    }

    #[test]
    fn truncated_data_pads_with_silence_without_failing_layout() {
        let pcm = tone(1000, 2);
        let mut wav = build_wav(44_100, 2, &pcm);
        let info = probe::probe(&wav).expect("probe");
        // Chop the file: the probe already ran on the full header, the
        // decoder sees fewer frames than planned.
        wav.truncate(wav.len() - 1000);
        let target = TargetFormat::for_source(&info, 0);
        let mut dec = Decoder::open(&wav, &info, target).expect("open");
        let n = dec.by_ref().count();
        assert_eq!(n as u64, target.total_frames / 128);
        assert_eq!(dec.frames_emitted(), target.total_frames);
    }

    #[test]
    fn open_rejects_mismatched_target() {
        let wav = build_wav(22_050, 1, &tone(100, 1));
        let info = probe::probe(&wav).expect("probe");
        let bad = TargetFormat {
            rate: 48_000,
            lead_in_frames: 0,
            total_frames: 128,
        };
        assert!(matches!(
            Decoder::open(&wav, &info, bad),
            Err(AudioError::BadTarget { .. })
        ));
        let bad = TargetFormat {
            rate: 44_100,
            lead_in_frames: 256,
            total_frames: 128,
        };
        assert!(matches!(
            Decoder::open(&wav, &info, bad),
            Err(AudioError::BadTarget { .. })
        ));
    }

    #[test]
    fn window_seek_and_fade_on_wav() {
        let pcm: Vec<i16> = (0..4000)
            .flat_map(|i| [10_000i16, (i % 100) as i16])
            .collect();
        let wav = build_wav(44_100, 2, &pcm);
        let info = probe::probe(&wav).expect("probe");
        let fade = Fade {
            len: 1000,
            fade_in: 100,
            fade_out: 200,
        };
        let flat: Vec<i16> = Decoder::open_window(&wav, &info, 1500, 1000, 1024, Some(fade))
            .expect("open")
            .flat_map(|b| b.into_iter())
            .collect();
        assert_eq!(flat.len(), 2048);
        // Right channel carries the frame index mod 100 → starts at 1500 % 100 = 0.
        assert_eq!(flat[500 * 2 + 1], (2000 % 100) as i16);
        // Fade-in: frame 0 silent, frame 50 at half gain, frame 500 full.
        assert_eq!(flat[0], 0);
        assert!((i32::from(flat[50 * 2]) - 5000).abs() <= 1);
        assert_eq!(flat[500 * 2], 10_000);
        // Fade-out: frame 900 at half gain, frame 999 nearly silent.
        assert!((i32::from(flat[900 * 2]) - 5000).abs() <= 1);
        assert!(flat[999 * 2] < 100);
        // Padding beyond the window is silence.
        assert!(flat[2000..].iter().all(|&s| s == 0));
    }
}
