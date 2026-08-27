//! The streaming producer (design req 16): one thread per bound generation
//! decodes the private source copy on demand, runs the resumable DSP state
//! — WSOLA when the binding preserves pitch, the plain resampler when it
//! does not — and encodes the TARGET (ring-served) entry's output blocks
//! into the binding's ring in virtual-offset order; the verbatim entry
//! never enters the ring. It maintains the loop-start checkpoint, honors
//! regeneration targets (reads behind the ring window), completes pending
//! slots as their ranges appear, and contains hard failures behind
//! `catch_unwind` → SilenceFill (req 28). The detours never synthesize.
//!
//! The synchronous [`GeneratorCore`] is the whole machine; [`spawn`] wraps
//! it on the `song-rate-generator` thread. Host tests drive the core
//! directly (deterministic pump) and the thread wrapper separately.
//!
//! Pure by the repo's host-compile discipline: no logging macros —
//! diagnostics flow through the binding's metrics and task-03's drain.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;
use std::time::Duration;

use super::binding::{Binding, ServeMode};
use crate::core::xact::resample::ResampleState;
use crate::core::xact::stretch::{SeededStretchState, SourcePcm, StretchCheckpoint, StretchState};
use crate::core::xact::virtual_bank::{EntryPlan, Region};
use crate::core::xact::{adpcm, xwb, WaveFormat};

/// How long the producer thread sleeps when paced out or waiting at
/// end-of-stream. The engine's packets carry ~1.3 s of audio each and the
/// look-ahead is 250 ms, so sub-millisecond wake latency is generous.
const IDLE_SLEEP: Duration = Duration::from_micros(500);

/// Frames requested per `StretchState::produce` call (the Step-3 feed's
/// granularity); the encoder drains whole blocks from the accumulated PCM.
const PRODUCE_FRAMES: usize = 1_024;

/// Verdict of one [`GeneratorCore::step`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepOutcome {
    /// Progress was made (production, regeneration, or slot completion).
    Working,
    /// Nothing to do right now: paced out, or idling at end-of-stream
    /// awaiting regeneration targets. Never terminal — a loop restart or a
    /// Quick-Restart re-create can demand regeneration at any time.
    Idle,
    /// The generation token requested a stop (retire or supersession).
    Stopped,
}

/// Why the producer could not be constructed (→ the thread wrapper flips
/// SilenceFill; the preflight validated everything, so this is a
/// should-not-happen guard).
#[derive(Debug)]
pub enum GeneratorError {
    Source(String),
    /// The binding is an identity passthrough — it serves the resident
    /// source directly and has NO producer by design (training design
    /// §4.5). Constructing one anyway is a caller bug.
    IdentityPassthrough,
}

impl std::fmt::Display for GeneratorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Source(detail) => write!(f, "generator source setup failed: {detail}"),
            Self::IdentityPassthrough => {
                write!(f, "identity passthrough bindings have no producer")
            }
        }
    }
}

impl std::error::Error for GeneratorError {}

/// The per-entry DSP behind [`Feed`]: pitch-preserving stretch (canonical
/// or seek-seeded), or plain resample, behind one produce/checkpoint
/// surface. All emit exactly their run's planned frames, so everything
/// downstream of the feed is byte-agnostic to the mode (design:
/// preserve-pitch option; training design §4.5 amendment).
enum DspState {
    Wsola(StretchState),
    /// A shift>0 mapping epoch's fresh seeded run (training design §4.5
    /// amendment): produces the output tail `[seek, output_frames)` only.
    SeededWsola(SeededStretchState),
    Resample(ResampleState),
}

impl DspState {
    fn produce(
        &mut self,
        view: &impl SourcePcm,
        out: &mut [i16],
    ) -> crate::core::xact::stretch::Produced {
        match self {
            Self::Wsola(state) => state.produce(view, out).expect("feed produce"),
            Self::SeededWsola(state) => state.produce(view, out).expect("feed produce"),
            Self::Resample(state) => state.produce(view, out).expect("feed produce"),
        }
    }

    /// Checkpoints exist for the canonical stretch only; the resampler's
    /// position is directly computable, and seeded runs reconstruct fresh
    /// (their loop-restart anchor belongs to the canonical `{0,0}` stream).
    fn checkpoint(&self) -> Option<StretchCheckpoint> {
        match self {
            Self::Wsola(state) => state.checkpoint(),
            Self::SeededWsola(_) | Self::Resample(_) => None,
        }
    }
}

/// One entry's live feed: the Step-3 `EncodedFeed` pipeline productionized
/// — `BlockCachePcm` (on-demand source decode) → [`DspState`] produce
/// (resumable stretch or resample) → whole-block accumulation →
/// `encode_block`.
struct Feed<'a> {
    view: adpcm::BlockCachePcm<'a>,
    state: DspState,
    format: WaveFormat,
    data_len: u64,
    /// Encoded bytes emitted so far (position within the entry's stream).
    emitted: u64,
    /// Whole PCM samples accumulated toward the next encoded block.
    pending_pcm: Vec<i16>,
    done: bool,
    /// Capture the latest checkpoint at or below this output frame (the
    /// stretched loop start — the loop-restart regeneration anchor).
    capture_target: Option<usize>,
}

impl<'a> Feed<'a> {
    fn new(
        entry: &xwb::SongEntry<'a>,
        plan: &EntryPlan,
        preserve_pitch: bool,
    ) -> Result<Self, GeneratorError> {
        let view = adpcm::BlockCachePcm::new(entry.data, entry.format, entry.duration)
            .map_err(|error| GeneratorError::Source(error.to_string()))?;
        let state = if preserve_pitch {
            DspState::Wsola(
                StretchState::new(
                    entry.duration as usize,
                    plan.streamed.duration as usize,
                    entry.format.channels() as usize,
                    entry.format.sample_rate(),
                    plan.loop_context,
                )
                .map_err(|error| GeneratorError::Source(error.to_string()))?,
            )
        } else {
            DspState::Resample(
                ResampleState::new(
                    entry.duration as usize,
                    plan.streamed.duration as usize,
                    entry.format.channels() as usize,
                    plan.loop_context,
                )
                .map_err(|error| GeneratorError::Source(error.to_string()))?,
            )
        };
        Ok(Self {
            view,
            state,
            format: entry.format,
            data_len: plan.streamed.data_len as u64,
            emitted: 0,
            pending_pcm: Vec::new(),
            done: false,
            // The stretch's loop-restart regeneration anchor; the resampler
            // seeks directly and needs no checkpoints.
            capture_target: if preserve_pitch {
                plan.loop_context.map(|context| context.output_start)
            } else {
                None
            },
        })
    }

    /// The regeneration a behind-window read performs. Stretch mode:
    /// restore the checkpoint (or start fresh), then produce-and-DISCARD
    /// the frames between the hop-aligned resume and the block-aligned
    /// target — the Step-3 `restore_at_block` mechanics. Resample mode: the
    /// state is purely positional, so this is a direct O(1) seek. Identical
    /// bytes are reproduced either way because both DSPs are deterministic.
    /// Returns the feed and the DISCARDED frame count (production work the
    /// caller reports into the binding's metrics).
    fn positioned_at(
        entry: &xwb::SongEntry<'a>,
        plan: &EntryPlan,
        preserve_pitch: bool,
        checkpoint: Option<&StretchCheckpoint>,
        target_frame: usize,
        target_byte: u64,
    ) -> (Self, u64) {
        let view = adpcm::BlockCachePcm::new(entry.data, entry.format, entry.duration)
            .expect("regeneration view: the source already parsed once");
        let channels = entry.format.channels() as usize;
        if !preserve_pitch {
            let mut state = ResampleState::new(
                entry.duration as usize,
                plan.streamed.duration as usize,
                channels,
                plan.loop_context,
            )
            .expect("regeneration resample state: constructed once already");
            state.positioned_at(target_frame);
            return (
                Self {
                    view,
                    state: DspState::Resample(state),
                    format: entry.format,
                    data_len: plan.streamed.data_len as u64,
                    emitted: target_byte,
                    pending_pcm: Vec::new(),
                    done: false,
                    capture_target: None,
                },
                0,
            );
        }
        let (mut state, resume_frame) = match checkpoint {
            Some(checkpoint) => {
                let state = StretchState::restore(
                    checkpoint,
                    entry.duration as usize,
                    plan.streamed.duration as usize,
                    channels,
                    entry.format.sample_rate(),
                    plan.loop_context,
                    &view,
                )
                .expect("regeneration restore: the checkpoint came from this run");
                (state, checkpoint.resume_frame())
            }
            None => {
                let state = StretchState::new(
                    entry.duration as usize,
                    plan.streamed.duration as usize,
                    channels,
                    entry.format.sample_rate(),
                    plan.loop_context,
                )
                .expect("regeneration state: constructed once already");
                (state, 0)
            }
        };
        let mut discard = target_frame
            .checked_sub(resume_frame)
            .expect("checkpoint resume must not pass the block-aligned target");
        let discarded = discard as u64;
        while discard > 0 {
            let mut scratch = vec![0i16; discard.min(4_096) * channels];
            let produced = state
                .produce(&view, &mut scratch)
                .expect("regeneration discard produce");
            assert!(produced.frames > 0, "regeneration discard stalled");
            discard -= produced.frames;
        }
        (
            Self {
                view,
                state: DspState::Wsola(state),
                format: entry.format,
                data_len: plan.streamed.data_len as u64,
                emitted: target_byte,
                pending_pcm: Vec::new(),
                done: false,
                capture_target: plan.loop_context.map(|context| context.output_start),
            },
            discarded,
        )
    }

    /// A shift>0 epoch's WSOLA feed (training design §4.5 amendment): a
    /// FRESH stretch seeded at the shift-mapped output frame — O(1), never
    /// the canonical alignment chain. `target_frame ≥ seek_frame` positions
    /// within the epoch (behind-window regeneration); the gap is
    /// produced-and-discarded — deterministic, because the seeded run is
    /// the epoch's byte authority. Seeded feeds capture NO checkpoints (the
    /// loop-restart anchor belongs to the canonical `{0,0}` stream).
    /// Returns the feed and the discarded frame count.
    fn new_seeded(
        entry: &xwb::SongEntry<'a>,
        plan: &EntryPlan,
        seek_frame: usize,
        target_frame: usize,
        target_byte: u64,
    ) -> (Self, u64) {
        let view = adpcm::BlockCachePcm::new(entry.data, entry.format, entry.duration)
            .expect("seeded view: the source already parsed once");
        let channels = entry.format.channels() as usize;
        let mut state = SeededStretchState::new(
            entry.duration as usize,
            plan.streamed.duration as usize,
            channels,
            entry.format.sample_rate(),
            seek_frame,
            &view,
        )
        .expect("seeded state: the plan validated once already");
        let mut discard = target_frame
            .checked_sub(seek_frame)
            .expect("regen target must not precede the epoch's seek point");
        let discarded = discard as u64;
        while discard > 0 {
            let mut scratch = vec![0i16; discard.min(4_096) * channels];
            let produced = state
                .produce(&view, &mut scratch)
                .expect("seeded discard produce");
            assert!(produced.frames > 0, "seeded discard stalled");
            discard -= produced.frames;
        }
        (
            Self {
                view,
                state: DspState::SeededWsola(state),
                format: entry.format,
                data_len: plan.streamed.data_len as u64,
                emitted: target_byte,
                pending_pcm: Vec::new(),
                done: false,
                capture_target: None,
            },
            discarded,
        )
    }

    /// Produce and encode the next run of whole blocks, appending to
    /// `encoded` WITHOUT exceeding `max_bytes` (excess whole blocks stay in
    /// `pending_pcm` for the next call — a chunk larger than the ring
    /// window would evict bytes the engine has not consumed yet). Returns
    /// the produced frame count and updates the caller's best checkpoint.
    fn produce_blocks(
        &mut self,
        checkpoint: &mut Option<StretchCheckpoint>,
        encoded: &mut Vec<u8>,
        max_bytes: usize,
    ) -> usize {
        let channels = self.format.channels() as usize;
        let block_samples = self.format.samples_per_block() as usize * channels;
        let block_align = self.format.block_align() as usize;
        // Always allow at least one block per call: forward progress.
        let max_bytes = max_bytes.max(block_align);
        let mut frames_produced = 0usize;
        loop {
            while self.pending_pcm.len() >= block_samples
                && encoded.len() + block_align <= max_bytes
            {
                adpcm::encode_block(&self.pending_pcm[..block_samples], self.format, encoded)
                    .expect("feed encode block");
                self.pending_pcm.drain(..block_samples);
            }
            if self.done || encoded.len() + block_align > max_bytes {
                break;
            }
            let mut out = vec![0i16; PRODUCE_FRAMES * channels];
            let produced = self.state.produce(&self.view, &mut out);
            frames_produced += produced.frames;
            self.pending_pcm
                .extend_from_slice(&out[..produced.frames * channels]);
            self.try_capture(checkpoint);
            if produced.done {
                self.done = true;
                assert!(
                    self.pending_pcm.len() % block_samples == 0,
                    "dsp output is not whole blocks"
                );
            }
        }
        frames_produced
    }

    /// Whether the stretch is exhausted AND every produced block has been
    /// emitted into the ring.
    fn finished(&self) -> bool {
        self.done && self.pending_pcm.is_empty()
    }

    /// Keep the latest reconstructible checkpoint at or below the stretched
    /// loop start (the Step-3 capture rule): the deepest legal resume for a
    /// loop-restart regeneration.
    fn try_capture(&self, best: &mut Option<StretchCheckpoint>) {
        let Some(target) = self.capture_target else {
            return;
        };
        if let Some(checkpoint) = self.state.checkpoint() {
            if checkpoint.resume_frame() <= target
                && best
                    .as_ref()
                    .is_none_or(|held| checkpoint.resume_frame() > held.resume_frame())
            {
                *best = Some(checkpoint);
            }
        }
    }
}

/// The synchronous producer machine: `step()` performs one bounded unit of
/// work. The thread wrapper loops it; host tests drive it directly.
pub struct GeneratorCore<'a> {
    binding: &'a Binding,
    /// The binding's latched DSP mode, read once at construction.
    preserve_pitch: bool,
    bank: xwb::SongBank<'a>,
    feeds: [Option<Feed<'a>>; 2],
    /// Best loop-start checkpoint per entry; survives feed recreation (the
    /// regeneration anchor must outlive the pass that captured it).
    checkpoints: [Option<StretchCheckpoint>; 2],
    /// Next absolute virtual offset to produce (mirrors `ring.produced`).
    cursor: u64,
    /// The applied content mapping's byte-domain snapshot (training design
    /// §4.5): virtual bytes `[0, lead)` are silent, `[lead, …)` map to
    /// served-stream bytes from `shift`, silent past the content end.
    /// `{0, 0}` degenerates to the shipped linear production exactly.
    lead_bytes: u64,
    shift_bytes: u64,
    blocks_encoded: u64,
    /// Fault hook (task-03's `mid-song-failure`): panic after this many
    /// encoded blocks (0 = disabled).
    fault_kill_after_blocks: u64,
    /// Reusable encode destination (drained into the ring every chunk).
    scratch: Vec<u8>,
}

impl<'a> GeneratorCore<'a> {
    pub fn new(binding: &'a Binding) -> Result<Self, GeneratorError> {
        if binding.serve_mode() == ServeMode::IdentityPassthrough {
            return Err(GeneratorError::IdentityPassthrough);
        }
        let bank = xwb::parse_song_bank(binding.source())
            .map_err(|error| GeneratorError::Source(error.to_string()))?;
        let mut core = Self {
            binding,
            preserve_pitch: binding.preserve_pitch(),
            bank,
            feeds: [None, None],
            checkpoints: [None, None],
            cursor: binding.target_data_start(),
            lead_bytes: 0,
            shift_bytes: 0,
            blocks_encoded: 0,
            fault_kill_after_blocks: binding.fault_kill_after_blocks(),
            scratch: Vec::new(),
        };
        // A mapping published before the producer started (a bind-time
        // pre-shift) is simply the first mapping production runs under.
        if let Some(epoch) = binding.mapping_pending() {
            core.snapshot_mapping();
            binding.mark_mapping_applied(epoch);
        }
        Ok(core)
    }

    /// Refresh the byte-domain mapping snapshot from the binding's packed
    /// block-unit word.
    fn snapshot_mapping(&mut self) {
        let entry = self.binding.layout().target_entry_index;
        let align = u64::from(self.bank.entries[entry].format.block_align());
        let (shift_blocks, lead_blocks) = self.binding.content_mapping();
        self.shift_bytes = shift_blocks * align;
        self.lead_bytes = lead_blocks * align;
    }

    /// One bounded unit of work: honor the stop token, apply a pending
    /// content-mapping change (restart production at output 0 under it —
    /// the `ring_rewind` bumps the seqlock, exactly the behind-window
    /// machinery), pick up a regeneration target, complete ready pending
    /// slots, then produce the next main-entry chunk unless paced out. The
    /// side (preview) entry is a verbatim passthrough served from the
    /// binding's resident source — the producer never touches it
    /// (step05-fix). Internal failures panic — the thread wrapper's
    /// `catch_unwind` is the containment boundary (req 28).
    pub fn step(&mut self) -> StepOutcome {
        if self.binding.stop_requested() {
            return StepOutcome::Stopped;
        }
        if let Some(epoch) = self.binding.mapping_pending() {
            self.snapshot_mapping();
            let entry = self.binding.layout().target_entry_index;
            self.feeds[entry] = None;
            // Cross-epoch checkpoints describe the PREVIOUS epoch's bytes —
            // invalidate on every mapping change (training design §4.5
            // amendment): the current epoch's runs are the byte authority
            // and reposition without them.
            self.checkpoints = [None, None];
            let start = self.binding.target_data_start();
            self.binding.ring_rewind(start);
            self.cursor = start;
            self.binding.mark_mapping_applied(epoch);
            return StepOutcome::Working;
        }
        if let Some(target) = self.binding.take_regen_target() {
            if target < self.cursor {
                self.rewind_to(target);
                self.binding.producer_complete_ready_slots();
                return StepOutcome::Working;
            }
        }
        self.binding.producer_complete_ready_slots();
        if self.cursor >= self.binding.target_data_end() {
            // End of stream: stay available for regeneration (a loop
            // restart or Quick-Restart re-create can rewind at any time).
            return StepOutcome::Idle;
        }
        if self.binding.ring_produced() >= self.binding.pace_limit()
            && !self.binding.armed_slot_pending()
        {
            return StepOutcome::Idle;
        }
        self.produce_chunk();
        self.binding.producer_complete_ready_slots();
        StepOutcome::Working
    }

    /// Produce the next run of MAIN-entry bytes at the cursor under the
    /// mapping snapshot. The cursor walks `[target_data_start, target_data_end)`
    /// only — the ring never holds the gap or the side entry (step05-fix),
    /// so the window can never slide past a range the engine still reads.
    /// Every chunk is bounded by a quarter of the ring so one append can
    /// never evict bytes the engine has not consumed (with the production
    /// 16 MiB ring the 16-block bound binds instead; the quarter matters
    /// for the shrunken test rings).
    fn produce_chunk(&mut self) {
        let layout = self.binding.layout();
        let entry = layout.target_entry_index;
        let window_bound = (self.binding.ring_capacity() / 4).max(1);
        let block_align = self.bank.entries[entry].format.block_align() as usize;
        let chunk_bytes = (16 * block_align)
            .min(window_bound / block_align * block_align)
            .max(block_align);
        let within = self.cursor - layout.entry_offsets[entry];
        let stream_len = layout.entries[entry].streamed.data_len as u64;
        if within < self.lead_bytes {
            // The silent approach lead (training design §4.5).
            let run = (self.lead_bytes - within).min(chunk_bytes as u64) as usize;
            self.emit_silence(entry, run);
            return;
        }
        let content_pos = within - self.lead_bytes + self.shift_bytes;
        if content_pos >= stream_len {
            // The silent tail past the shifted content's end.
            let run =
                (self.binding.target_data_end() - self.cursor).min(chunk_bytes as u64) as usize;
            self.emit_silence(entry, run);
            return;
        }
        self.ensure_feed_at(entry, content_pos);
        let feed = self.feeds[entry].as_mut().expect("feed just ensured");
        assert_eq!(
            feed.emitted, content_pos,
            "feed position diverged from the mapped cursor"
        );
        self.scratch.clear();
        // Never run past the virtual entry's end: the mapped content window
        // is `stream_len − content_pos` at most (the shift consumes tail
        // blocks the silent fill replaces).
        let chunk_bytes = (chunk_bytes as u64)
            .min(stream_len - content_pos)
            .min(self.binding.target_data_end() - self.cursor) as usize;
        let frames =
            feed.produce_blocks(&mut self.checkpoints[entry], &mut self.scratch, chunk_bytes);
        let produced_len = self.scratch.len() as u64;
        assert!(
            feed.emitted + produced_len <= feed.data_len,
            "feed overran the planned entry stream"
        );
        // SAFETY: single-producer ring write (this thread).
        unsafe { self.binding.ring_append(self.cursor, &self.scratch) };
        feed.emitted += produced_len;
        self.cursor += produced_len;
        self.binding.add_frames_produced(frames as u64);
        self.blocks_encoded += (self.scratch.len() / block_align) as u64;
        if feed.finished() {
            assert_eq!(
                feed.emitted, feed.data_len,
                "feed length diverges from the plan's data_len"
            );
        }
        self.note_encoded_blocks();
    }

    /// Append `bytes` of silent blocks at the cursor (whole blocks by
    /// construction: lead/shift/stream lengths are all block multiples).
    fn emit_silence(&mut self, entry: usize, bytes: usize) {
        let block = self.binding.silent_block(entry);
        self.scratch.clear();
        self.scratch.reserve(bytes);
        while self.scratch.len() < bytes {
            self.scratch.extend_from_slice(block);
        }
        debug_assert_eq!(self.scratch.len(), bytes, "silent run is whole blocks");
        // SAFETY: single-producer ring write (this thread).
        unsafe { self.binding.ring_append(self.cursor, &self.scratch) };
        self.cursor += bytes as u64;
        self.blocks_encoded += (bytes / block.len()) as u64;
        self.note_encoded_blocks();
    }

    fn note_encoded_blocks(&self) {
        if self.fault_kill_after_blocks != 0 && self.blocks_encoded >= self.fault_kill_after_blocks
        {
            panic!(
                "song-rate fault injection: producer killed after {} blocks",
                self.blocks_encoded
            );
        }
    }

    /// Ensure the TARGET entry's feed is positioned at `content_pos` (bytes
    /// into the served stream): reuse a feed already there, start fresh, or
    /// reposition. Canonical WSOLA (`shift == 0`) repositions via checkpoint
    /// restore plus produce-and-discard — the Step-3 `restore_at_block`
    /// mechanics. A shift>0 epoch in WSOLA mode constructs the SEEDED feed
    /// (training design §4.5 amendment) — O(1), never the canonical chain.
    /// Resample mode seeks positionally. Deterministic: identical bytes are
    /// reproduced either way. Positioning discards count into the binding's
    /// frames-produced metric (they are real DSP work).
    fn ensure_feed_at(&mut self, entry: usize, content_pos: u64) {
        if let Some(feed) = &self.feeds[entry] {
            if feed.emitted == content_pos {
                return;
            }
        }
        let layout = self.binding.layout();
        let format = self.bank.entries[entry].format;
        let align = u64::from(format.block_align());
        debug_assert_eq!(
            content_pos % align,
            0,
            "content positions are block-aligned"
        );
        let spb = format.samples_per_block() as usize;
        let (feed, discarded) = if self.preserve_pitch && self.shift_bytes > 0 {
            let seek_frame = usize::try_from(self.shift_bytes / align)
                .expect("shift block index fits usize")
                * spb;
            let target_frame =
                usize::try_from(content_pos / align).expect("block index fits usize") * spb;
            Feed::new_seeded(
                &self.bank.entries[entry],
                &layout.entries[entry],
                seek_frame,
                target_frame,
                content_pos,
            )
        } else if content_pos == 0 {
            (
                Feed::new(
                    &self.bank.entries[entry],
                    &layout.entries[entry],
                    self.preserve_pitch,
                )
                .expect("forward feed construction"),
                0,
            )
        } else {
            let block = content_pos / align;
            let target_frame = usize::try_from(block).expect("block index fits usize") * spb;
            let checkpoint = self.checkpoints[entry]
                .as_ref()
                .filter(|checkpoint| checkpoint.resume_frame() <= target_frame)
                .cloned();
            Feed::positioned_at(
                &self.bank.entries[entry],
                &layout.entries[entry],
                self.preserve_pitch,
                checkpoint.as_ref(),
                target_frame,
                content_pos,
            )
        };
        self.binding.add_frames_produced(discarded);
        self.feeds[entry] = Some(feed);
    }

    /// Rewind for a behind-window read (design req 20): block-align the
    /// target, bump the ring's seqlock, and continue linearly from there —
    /// `produce_chunk` repositions the feed under the mapping (silent
    /// regions need no feed at all). Deterministic: identical bytes are
    /// reproduced. Regen targets are main-entry offsets by construction
    /// (the side entry is resident and never behind-window).
    fn rewind_to(&mut self, target: u64) {
        let layout = self.binding.layout();
        let span = layout.resolve(target, 1);
        let Region::EntryData {
            entry,
            offset: within,
        } = span.region
        else {
            // Regen targets are entry-data by construction (check_spans);
            // ignore anything else.
            return;
        };
        if entry != layout.target_entry_index {
            return;
        }
        let align = u64::from(self.bank.entries[entry].format.block_align());
        let aligned = within / align * align;
        self.feeds[entry] = None;
        let new_cursor = layout.entry_offsets[entry] + aligned;
        self.binding.ring_rewind(new_cursor);
        self.cursor = new_cursor;
    }
}

/// Run the producer on its own thread (`song-rate-generator`, one per bound
/// generation). A panic or setup failure anywhere inside flips the binding
/// to SilenceFill — the containment boundary (req 28) — and the thread
/// records its wall time and exits. The thread stays alive at end-of-stream
/// (regeneration duty) until the generation token stops it.
pub fn spawn(binding: Arc<Binding>) -> std::io::Result<std::thread::JoinHandle<()>> {
    std::thread::Builder::new()
        .name("song-rate-generator".into())
        .spawn(move || run(&binding))
}

fn run(binding: &Binding) {
    let outcome = catch_unwind(AssertUnwindSafe(|| drive(binding)));
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(_)) | Err(_) => binding.enter_silence_fill(),
    }
    binding.record_wall();
}

fn drive(binding: &Binding) -> Result<(), GeneratorError> {
    let mut core = GeneratorCore::new(binding)?;
    loop {
        match core.step() {
            StepOutcome::Working => {}
            StepOutcome::Idle => std::thread::sleep(IDLE_SLEEP),
            StepOutcome::Stopped => return Ok(()),
        }
    }
}
