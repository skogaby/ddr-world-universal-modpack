//! Host tests for the streaming binding runtime: ring + pending-slot
//! protocol, the producer (`GeneratorCore` synchronously, `spawn` on the
//! real thread), and the pure serve dispatch — the exact surface task-04's
//! IO-callback detours call.
//!
//! Owns its copies of the Step-3 fixture builder and whole-buffer oracle:
//! the `core/xact` `#[cfg(test)]` module is not importable from here, and
//! the oracle must stay an independent byte authority.

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::binding::{Binding, BindingState, PollOutcome, ServeOutcome};
use super::generator::{spawn, GeneratorCore, StepOutcome};
use crate::core::xact::{adpcm, resample, stretch, virtual_bank, xwb, WaveFormat};

const BLOCK_ALIGN: usize = 140;
const SAMPLES_PER_BLOCK: u32 = 128;
const SEGMENT_FOUR_OFFSET: usize = 2048;
/// Main-entry frames: 256 source blocks, matching the Step-3 replay fixture
/// (multiple 64 KiB packets at the slow rates, bounded debug runtime).
const MAIN_FRAMES: usize = 32_768;
/// Preview-entry payload: 16 whole source blocks (ADPCM data is always
/// whole blocks)...
const PREVIEW_PAYLOAD_FRAMES: usize = 2_048;
/// ...with a stock-shaped declared duration INSIDE the final block
/// (2,000 < 16 × 128): real banks never land on block boundaries, and a
/// block-exact preview fixture masked the 2026-08-10 live HeaderSynth
/// refusal (the stream serializer applied the generated-content
/// whole-block rule to the passthrough preview).
const PREVIEW_FRAMES: usize = 2_000;

pub(super) fn format(sample_rate: u32, channels: u8) -> WaveFormat {
    WaveFormat::from_packed(2 | (u32::from(channels) << 2) | (sample_rate << 5) | (48 << 23))
}

fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn fixed_name(name: &str) -> [u8; 64] {
    let mut result = [0; 64];
    result[..name.len()].copy_from_slice(name.as_bytes());
    result
}

fn round_up(value: usize, alignment: usize) -> usize {
    value.div_ceil(alignment) * alignment
}

/// Deterministic multi-tone interleaved PCM (the Step-3 fixture audio).
pub(super) fn tone_pcm(frames: usize, channels: usize) -> Vec<i16> {
    let mut pcm = Vec::with_capacity(frames * channels);
    for frame in 0..frames {
        for channel in 0..channels {
            let time = frame as f64 / 8_000.0;
            let tone = (std::f64::consts::TAU * (200.0 + 55.0 * channel as f64) * time).sin();
            let beat = (std::f64::consts::TAU * 3.0 * time).sin();
            pcm.push((12_000.0 * tone * (0.6 + 0.4 * beat)) as i16);
        }
    }
    pcm
}

/// Synthesize a strict-profile `tst1`/`tst1_s` bank around explicit
/// per-entry formats, encoded payloads, durations, and loops.
pub(super) fn build_bank_bytes(
    preview_first: bool,
    formats: [WaveFormat; 2],
    payloads: [&[u8]; 2],
    durations: [u32; 2],
    loops: [(u32, u32); 2],
) -> Vec<u8> {
    let code = "tst1";
    let entry_names = if preview_first {
        ["tst1_s", "tst1"]
    } else {
        ["tst1", "tst1_s"]
    };
    let data_lengths = [payloads[0].len(), payloads[1].len()];
    let data_offsets = [0, round_up(data_lengths[0], 2048)];
    let segment_four_len = data_offsets[1] + data_lengths[1];
    let mut bytes = vec![0; SEGMENT_FOUR_OFFSET + segment_four_len];

    bytes[0..4].copy_from_slice(b"WBND");
    put_u32(&mut bytes, 4, 43);
    put_u32(&mut bytes, 8, 42);
    for (index, (offset, length)) in [
        (52, 96),
        (148, 48),
        (196, 0),
        (196, 128),
        (SEGMENT_FOUR_OFFSET, segment_four_len),
    ]
    .into_iter()
    .enumerate()
    {
        put_u32(&mut bytes, 12 + index * 8, offset as u32);
        put_u32(&mut bytes, 16 + index * 8, length as u32);
    }

    put_u32(&mut bytes, 52, 0x0009_0001);
    put_u32(&mut bytes, 56, 2);
    bytes[60..124].copy_from_slice(&fixed_name(code));
    put_u32(&mut bytes, 124, 24);
    put_u32(&mut bytes, 128, 64);
    put_u32(&mut bytes, 132, 2048);
    put_u32(&mut bytes, 136, 0);
    bytes[140..148].copy_from_slice(&0x1122_3344_5566_7788u64.to_le_bytes());

    for index in 0..2 {
        let meta = 148 + index * 24;
        put_u32(&mut bytes, meta, durations[index] << 4);
        put_u32(&mut bytes, meta + 4, formats[index].packed());
        put_u32(&mut bytes, meta + 8, data_offsets[index] as u32);
        put_u32(&mut bytes, meta + 12, data_lengths[index] as u32);
        put_u32(&mut bytes, meta + 16, loops[index].0);
        put_u32(&mut bytes, meta + 20, loops[index].1);

        let name_offset = 196 + index * 64;
        bytes[name_offset..name_offset + 64].copy_from_slice(&fixed_name(entry_names[index]));
        let data_start = SEGMENT_FOUR_OFFSET + data_offsets[index];
        bytes[data_start..data_start + data_lengths[index]].copy_from_slice(payloads[index]);
    }

    bytes
}

/// Synthetic full-entry-loop fixture (the production loop shape — 25%/50%
/// fail `NoCandidate` without it), 8 kHz stereo tone audio. The preview's
/// declared duration sits INSIDE its whole-block payload (stock shape).
pub(super) fn replay_fixture(preview_first: bool) -> Vec<u8> {
    let fmt = format(8_000, 2);
    let main = adpcm::encode_interleaved(&tone_pcm(MAIN_FRAMES, 2), fmt).expect("encode main");
    let preview = adpcm::encode_interleaved(&tone_pcm(PREVIEW_PAYLOAD_FRAMES, 2), fmt)
        .expect("encode preview");
    let main_loop = (0, MAIN_FRAMES as u32);
    let preview_loop = (0, PREVIEW_FRAMES as u32);
    let (payloads, durations, loops) = if preview_first {
        (
            [preview.as_slice(), main.as_slice()],
            [PREVIEW_FRAMES as u32, MAIN_FRAMES as u32],
            [preview_loop, main_loop],
        )
    } else {
        (
            [main.as_slice(), preview.as_slice()],
            [MAIN_FRAMES as u32, PREVIEW_FRAMES as u32],
            [main_loop, preview_loop],
        )
    };
    build_bank_bytes(preview_first, [fmt, fmt], payloads, durations, loops)
}

/// A `goru`-shaped (GOLD RUSH) 4-entry fixture: `goru_cs` / `goru` /
/// `goru_ac` / `goru_s`, the ONLY stock World song bank with more than two
/// waves. The three long entries share the main tone with a different
/// seed byte-shift so a region mix-up between variants is visible.
pub(super) fn goru_fixture() -> Vec<u8> {
    let fmt = format(8_000, 2);
    let names = ["goru_cs", "goru", "goru_ac", "goru_s"];
    let mut payloads = Vec::new();
    for (index, _) in names.iter().enumerate() {
        let frames = if index == 3 {
            PREVIEW_PAYLOAD_FRAMES
        } else {
            MAIN_FRAMES
        };
        let mut pcm = tone_pcm(frames, 2);
        for sample in &mut pcm {
            *sample = sample.wrapping_add(index as i16 * 97);
        }
        payloads.push(adpcm::encode_interleaved(&pcm, fmt).expect("encode entry"));
    }
    let durations = [
        MAIN_FRAMES as u32,
        MAIN_FRAMES as u32,
        MAIN_FRAMES as u32,
        PREVIEW_FRAMES as u32,
    ];

    let count = names.len();
    let meta_off = 148;
    let names_off = meta_off + count * 24;
    let wave_off = round_up(names_off + count * 64, 2048);
    let mut offsets = Vec::with_capacity(count);
    let mut cursor = 0usize;
    for (index, payload) in payloads.iter().enumerate() {
        let off = if index == 0 {
            0
        } else {
            round_up(cursor, 2048)
        };
        offsets.push(off);
        cursor = off + payload.len();
    }
    let mut bytes = vec![0; wave_off + cursor];
    bytes[0..4].copy_from_slice(b"WBND");
    put_u32(&mut bytes, 4, 43);
    put_u32(&mut bytes, 8, 42);
    for (index, (offset, length)) in [
        (52, 96),
        (meta_off, count * 24),
        (names_off, 0),
        (names_off, count * 64),
        (wave_off, cursor),
    ]
    .into_iter()
    .enumerate()
    {
        put_u32(&mut bytes, 12 + index * 8, offset as u32);
        put_u32(&mut bytes, 16 + index * 8, length as u32);
    }
    put_u32(&mut bytes, 52, 0x0009_0001);
    put_u32(&mut bytes, 56, count as u32);
    bytes[60..124].copy_from_slice(&fixed_name("goru"));
    put_u32(&mut bytes, 124, 24);
    put_u32(&mut bytes, 128, 64);
    put_u32(&mut bytes, 132, 2048);
    put_u32(&mut bytes, 136, 0);
    bytes[140..148].copy_from_slice(&0x1122_3344_5566_7788u64.to_le_bytes());
    for index in 0..count {
        let meta = meta_off + index * 24;
        put_u32(&mut bytes, meta, durations[index] << 4);
        put_u32(&mut bytes, meta + 4, fmt.packed());
        put_u32(&mut bytes, meta + 8, offsets[index] as u32);
        put_u32(&mut bytes, meta + 12, payloads[index].len() as u32);
        put_u32(&mut bytes, meta + 16, 0);
        put_u32(&mut bytes, meta + 20, durations[index]);
        let name_at = names_off + index * 64;
        bytes[name_at..name_at + 64].copy_from_slice(&fixed_name(names[index]));
        let data_at = wave_off + offsets[index];
        bytes[data_at..data_at + payloads[index].len()].copy_from_slice(&payloads[index]);
    }
    bytes
}

/// The whole-buffer oracle: parse → plan → stretch the MAIN entry
/// (decode → reference stretch → whole-buffer encode) with the non-main
/// entry passed through VERBATIM (the preview passthrough, step05-fix v2)
/// → stream write. The independent byte authority every serve-dispatch leg
/// compares against.
fn transform_bank_oracle(source: &[u8], percent: u32) -> Vec<u8> {
    transform_bank_oracle_mode(source, percent, true)
}

/// Whole-buffer oracle with an explicit DSP mode: `preserve_pitch = true`
/// stretches (WSOLA), `false` resamples — both against the same plan.
fn transform_bank_oracle_mode(source: &[u8], percent: u32, preserve_pitch: bool) -> Vec<u8> {
    transform_bank_oracle_target(
        source,
        percent,
        preserve_pitch,
        virtual_bank::StretchTarget::Main,
    )
}

/// Whole-buffer oracle with an explicit stretch target: the TARGET entry
/// is transformed (WSOLA or resample per `preserve_pitch`) and the other
/// entry passes through verbatim — the byte authority for BOTH the
/// gameplay (Main) and song-select preview (Side) plans.
pub(super) fn transform_bank_oracle_target(
    source: &[u8],
    percent: u32,
    preserve_pitch: bool,
    target: virtual_bank::StretchTarget,
) -> Vec<u8> {
    let bank = xwb::parse_song_bank(source).expect("oracle parse");
    let layout = virtual_bank::plan_virtual_bank(&bank, percent, target).expect("oracle plan");
    let streamed: Vec<_> = layout.entries.iter().map(|plan| plan.streamed).collect();
    let mut encoded = Vec::new();
    for (index, entry) in bank.entries.iter().enumerate() {
        if index != layout.target_entry_index {
            encoded.push(entry.data.to_vec());
            continue;
        }
        let decoded = adpcm::decode_interleaved(entry.data, entry.format, entry.duration)
            .expect("oracle decode");
        let transformed = if preserve_pitch {
            stretch::stretch_interleaved(
                &decoded,
                entry.format.channels() as usize,
                entry.format.sample_rate(),
                layout.entries[index].streamed.duration as usize,
                layout.entries[index].loop_context,
            )
            .expect("oracle stretch")
            .samples
        } else {
            resample::resample_interleaved(
                &decoded,
                entry.format.channels() as usize,
                layout.entries[index].streamed.duration as usize,
                layout.entries[index].loop_context,
            )
            .expect("oracle resample")
        };
        encoded.push(adpcm::encode_interleaved(&transformed, entry.format).expect("oracle encode"));
    }
    let mut output = Vec::new();
    xwb::write_song_bank_streaming(&bank, &streamed, &mut output, |index, out| {
        out.write_all(&encoded[index])
    })
    .expect("oracle stream write");
    output
}

/// Build a live binding around a fixture at `percent`; `preserve_pitch`
/// selects the DSP mode (true = WSOLA, false = resample); `ring_capacity`
/// overrides the production 16 MiB for window-pressure tests.
fn make_binding(
    source: Vec<u8>,
    percent: u32,
    preserve_pitch: bool,
    ring_capacity: Option<usize>,
) -> Binding {
    make_binding_target(
        source,
        percent,
        preserve_pitch,
        ring_capacity,
        virtual_bank::StretchTarget::Main,
    )
}

/// [`make_binding`] with an explicit stretch target (Side = the
/// song-select preview shape: the `_s` entry rides the ring, the main
/// entry serves verbatim).
fn make_binding_target(
    source: Vec<u8>,
    percent: u32,
    preserve_pitch: bool,
    ring_capacity: Option<usize>,
    target: virtual_bank::StretchTarget,
) -> Binding {
    let bank = xwb::parse_song_bank(&source).expect("fixture parses");
    let layout = virtual_bank::plan_virtual_bank(&bank, percent, target).expect("fixture plans");
    let rate = layout.entries[layout.target_entry_index].rate;
    drop(bank);
    let source = source.into_boxed_slice();
    match ring_capacity {
        Some(capacity) => {
            Binding::with_ring_capacity(5, 1, rate, layout, source, preserve_pitch, capacity)
        }
        None => Binding::new(5, 1, rate, layout, source, preserve_pitch),
    }
    .expect("binding constructs")
}

/// Drive the synchronous producer until the pending request completes;
/// Pending means "run the producer, then re-poll" (the engine's polled
/// contract with the deferral resolved by producer progress).
fn pump(binding: &Binding, core: &mut GeneratorCore<'_>, accumulator: &mut u64) -> u64 {
    for _ in 0..500_000 {
        match unsafe { binding.poll(accumulator as *mut u64) } {
            PollOutcome::Complete(bytes) => return bytes,
            PollOutcome::Incomplete => {
                core.step();
            }
            PollOutcome::NotPending => panic!("pump found nothing pending"),
        }
    }
    panic!("pump never completed the pending read");
}

/// One engine read against the serve dispatch, deferrals pumped through the
/// synchronous producer. Mirrors the stock accounting: synchronous serves
/// accumulate and the caller reports-and-zeroes; deferred serves complete
/// through poll (which zeroes).
fn serve_pumped(
    binding: &Binding,
    core: &mut GeneratorCore<'_>,
    file: &mut [u8],
    offset: u64,
    len: u32,
    accumulator: &mut u64,
) -> u32 {
    let dest = file[offset as usize..].as_mut_ptr();
    match unsafe { binding.serve(offset, len, dest, accumulator as *mut u64) } {
        ServeOutcome::Served(served) => {
            assert_eq!(
                *accumulator,
                u64::from(served),
                "synchronous serve accumulates exactly the served count"
            );
            *accumulator = 0;
            served
        }
        ServeOutcome::Pending => {
            let bytes = pump(binding, core, accumulator);
            assert_eq!(*accumulator, 0, "poll zeroes the accumulator");
            u32::try_from(bytes).expect("completion byte count fits u32")
        }
        ServeOutcome::Refused => panic!("serve refused a live read"),
    }
}

/// Replay the RE-pinned engine read pattern (0x1000 header read spanning
/// regions, per-entry sequential block-align-rounded 64 KiB packets, one
/// defensive EOF read) entirely through the serve dispatch.
fn replay_via_serve(binding: &Binding, core: &mut GeneratorCore<'_>) -> Vec<u8> {
    let virtual_size = binding.layout().virtual_size;
    let mut file = vec![0u8; usize::try_from(virtual_size).expect("virtual size fits")];
    let mut accumulator = 0u64;

    let header = serve_pumped(binding, core, &mut file, 0, 0x1000, &mut accumulator);
    assert_eq!(header, 0x1000, "the header read completes in full");

    for entry in 0..binding.layout().entries.len() {
        let data_len = binding.layout().entries[entry].streamed.data_len as u64;
        let block_align = u64::from(binding.entry_format(entry).block_align());
        let packet = 65_536 / block_align * block_align;
        let mut cursor = 0u64;
        while cursor < data_len {
            let request = packet.min(data_len - cursor) as u32;
            let offset = binding.layout().entry_offsets[entry] + cursor;
            let served = serve_pumped(binding, core, &mut file, offset, request, &mut accumulator);
            assert_eq!(served, request, "in-stream packet reads serve in full");
            cursor += u64::from(served);
        }
    }

    // The defensive read at EOF: the stock clamp serves zero bytes,
    // synchronously (never a deferral).
    let mut past = [0u8; 16];
    match unsafe {
        binding.serve(
            virtual_size,
            0x1000,
            past.as_mut_ptr(),
            &mut accumulator as *mut u64,
        )
    } {
        ServeOutcome::Served(0) => {}
        other => panic!("EOF read must serve zero bytes synchronously, got {other:?}"),
    }
    assert_eq!(accumulator, 0, "EOF serves accumulate nothing");

    file
}

// ── Pending-slot protocol ────────────────────────────────────────────

#[test]
fn deferred_read_completes_exactly_once_with_stock_accounting() {
    let binding = make_binding(replay_fixture(false), 50, true, None);
    let mut core = GeneratorCore::new(&binding).expect("core constructs");
    let entry_zero = binding.layout().entry_offsets[0];
    let block = binding.entry_format(0).block_align();

    // Ahead of the (empty) watermark: the read defers.
    let mut buffer = vec![0u8; block as usize];
    let mut accumulator = 0u64;
    let outcome = unsafe {
        binding.serve(
            entry_zero,
            block,
            buffer.as_mut_ptr(),
            &mut accumulator as *mut u64,
        )
    };
    assert_eq!(outcome, ServeOutcome::Pending);

    // Poll before completion reports incomplete; nothing accumulated.
    assert_eq!(
        unsafe { binding.poll(&mut accumulator as *mut u64) },
        PollOutcome::Incomplete
    );
    assert_eq!(accumulator, 0);

    // Producer progress completes it exactly once with stock accounting.
    let bytes = pump(&binding, &mut core, &mut accumulator);
    assert_eq!(bytes, u64::from(block));
    assert_eq!(accumulator, 0, "poll reported and zeroed the accumulator");

    // A second consume finds the slot Free again.
    assert_eq!(
        unsafe { binding.poll(&mut accumulator as *mut u64) },
        PollOutcome::NotPending
    );

    // The range is now produced: the same read serves synchronously and
    // accumulates exactly once.
    let mut again = vec![0u8; block as usize];
    let outcome = unsafe {
        binding.serve(
            entry_zero,
            block,
            again.as_mut_ptr(),
            &mut accumulator as *mut u64,
        )
    };
    assert_eq!(outcome, ServeOutcome::Served(block));
    assert_eq!(accumulator, u64::from(block));
    assert_eq!(again, buffer, "deferred and synchronous serves agree");

    let metrics = binding.metrics_snapshot();
    assert_eq!(metrics.deferral_count, 1);
    assert!(metrics.frames_produced > 0);
}

// ── Byte equality through the ring (AC-1) ────────────────────────────

#[test]
fn replay_through_the_serve_dispatch_matches_the_oracle() {
    for (percent, preview_first) in [(50, false), (50, true), (175, false), (175, true)] {
        let source = replay_fixture(preview_first);
        let oracle = transform_bank_oracle(&source, percent);
        let binding = make_binding(source, percent, true, None);
        let mut core = GeneratorCore::new(&binding).expect("core constructs");

        let file = replay_via_serve(&binding, &mut core);
        assert_eq!(
            file.len(),
            oracle.len(),
            "virtual size diverges at {percent}% preview_first={preview_first}"
        );
        assert!(
            file == oracle,
            "served bytes diverge from the oracle at {percent}% preview_first={preview_first}"
        );

        // The reassembled virtual file is a valid bank: reparse and decode.
        let bank = xwb::parse_song_bank(&file).expect("served file reparses");
        for entry in &bank.entries {
            adpcm::decode_interleaved(entry.data, entry.format, entry.duration)
                .expect("served entry decodes");
        }
    }
}

// ── Behind-window regeneration (AC-3) ────────────────────────────────

#[test]
fn behind_window_loop_restart_regenerates_identical_bytes() {
    // A ring far smaller than entry 0's stream forces the loop-restart read
    // below the window.
    let capacity = 2_048usize;
    let binding = make_binding(replay_fixture(false), 50, true, Some(capacity));
    let mut core = GeneratorCore::new(&binding).expect("core constructs");
    let entry_zero = binding.layout().entry_offsets[0];
    let block = binding.entry_format(0).block_align() as usize;
    let data_len = binding.layout().entries[0].streamed.data_len;
    assert!(data_len > capacity * 4, "fixture must dwarf the test ring");

    // Stream entry 0 sequentially in 4-block packets, recording the first
    // serving of the loop-start range.
    let packet = (4 * block) as u32;
    let mut first_serving = vec![0u8; packet as usize];
    let mut scratch = vec![0u8; packet as usize];
    let mut accumulator = 0u64;
    let mut cursor = 0u64;
    while cursor < data_len as u64 {
        let request = u64::from(packet).min(data_len as u64 - cursor) as u32;
        let dest = if cursor == 0 {
            first_serving.as_mut_ptr()
        } else {
            scratch.as_mut_ptr()
        };
        let served = match unsafe {
            binding.serve(
                entry_zero + cursor,
                request,
                dest,
                &mut accumulator as *mut u64,
            )
        } {
            ServeOutcome::Served(served) => {
                accumulator = 0;
                served
            }
            ServeOutcome::Pending => {
                let bytes = pump(&binding, &mut core, &mut accumulator);
                u32::try_from(bytes).expect("fits")
            }
            ServeOutcome::Refused => panic!("refused mid-stream"),
        };
        assert_eq!(served, request);
        cursor += u64::from(served);
    }

    // The loop restart: a read at the entry start, now far below the window.
    let mut second_serving = vec![0u8; packet as usize];
    let outcome = unsafe {
        binding.serve(
            entry_zero,
            packet,
            second_serving.as_mut_ptr(),
            &mut accumulator as *mut u64,
        )
    };
    assert_eq!(
        outcome,
        ServeOutcome::Pending,
        "a read below the ring window must defer for regeneration"
    );
    let bytes = pump(&binding, &mut core, &mut accumulator);
    assert_eq!(bytes, u64::from(packet));
    assert_eq!(
        second_serving, first_serving,
        "regeneration must reproduce identical bytes"
    );
    assert_eq!(binding.state(), BindingState::Active);
}

// ── Silence-fill (AC-4) ──────────────────────────────────────────────

#[test]
fn mid_stream_producer_death_switches_to_valid_silence() {
    let source = replay_fixture(false);
    let binding = Arc::new(make_binding(source, 50, true, None));
    // Kill the producer after 8 encoded blocks — mid-way through the bytes
    // the header read needs, so the death lands mid-deferral.
    binding.set_fault_kill_after_blocks(8);
    let handle = spawn(Arc::clone(&binding)).expect("generator thread spawns");

    let virtual_size = binding.layout().virtual_size;
    let mut file = vec![0u8; usize::try_from(virtual_size).expect("fits")];
    let mut accumulator = 0u64;

    // Serve the whole engine pattern against the dying producer; every read
    // must still complete (silence after the flip).
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut read_all = |file: &mut [u8], offset: u64, len: u32| -> u32 {
        let dest = file[offset as usize..].as_mut_ptr();
        match unsafe { binding.serve(offset, len, dest, &mut accumulator as *mut u64) } {
            ServeOutcome::Served(served) => {
                accumulator = 0;
                served
            }
            ServeOutcome::Pending => loop {
                match unsafe { binding.poll(&mut accumulator as *mut u64) } {
                    PollOutcome::Complete(bytes) => {
                        break u32::try_from(bytes).expect("fits");
                    }
                    PollOutcome::Incomplete => {
                        assert!(
                            Instant::now() < deadline,
                            "silence-fill must complete reads promptly"
                        );
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    PollOutcome::NotPending => panic!("pending read vanished"),
                }
            },
            ServeOutcome::Refused => panic!("refused while alive"),
        }
    };

    assert_eq!(read_all(&mut file, 0, 0x1000), 0x1000);
    for entry in 0..binding.layout().entries.len() {
        let data_len = binding.layout().entries[entry].streamed.data_len as u64;
        let block_align = u64::from(binding.entry_format(entry).block_align());
        let packet = 65_536 / block_align * block_align;
        let mut cursor = 0u64;
        while cursor < data_len {
            let request = packet.min(data_len - cursor) as u32;
            let offset = binding.layout().entry_offsets[entry] + cursor;
            let served = read_all(&mut file, offset, request);
            assert_eq!(served, request, "reads complete under silence-fill");
            cursor += u64::from(served);
        }
    }

    assert_eq!(binding.state(), BindingState::SilenceFill);

    // The reassembled stream is still a valid bank: it parses and decodes.
    let bank = xwb::parse_song_bank(&file).expect("silence-filled file reparses");
    for entry in &bank.entries {
        adpcm::decode_interleaved(entry.data, entry.format, entry.duration)
            .expect("silence-filled entry decodes");
    }

    // The dead producer's thread has exited (silence serves need no thread).
    binding.retire();
    handle.join().expect("generator thread joins");
}

// ── Retire, cancellation, quiescence (AC-5) ──────────────────────────

#[test]
fn retire_cancels_pending_and_waits_for_reader_quiescence() {
    let binding = make_binding(replay_fixture(false), 50, true, None);
    let entry_zero = binding.layout().entry_offsets[0];
    let block = binding.entry_format(0).block_align();

    // Arm a pending read (nothing produced yet).
    let mut buffer = vec![0u8; block as usize];
    let mut accumulator = 0u64;
    let outcome = unsafe {
        binding.serve(
            entry_zero,
            block,
            buffer.as_mut_ptr(),
            &mut accumulator as *mut u64,
        )
    };
    assert_eq!(outcome, ServeOutcome::Pending);

    // A reader is inside the epoch guard while the binding retires.
    binding.reader_enter();
    binding.retire();
    assert!(
        !binding.reclaim_eligible(),
        "reclamation must wait for reader quiescence"
    );

    // The retire cancelled the armed slot with EOF-clamp semantics: a
    // 0-byte completion.
    assert_eq!(
        unsafe { binding.poll(&mut accumulator as *mut u64) },
        PollOutcome::Complete(0)
    );
    assert_eq!(accumulator, 0);

    // Post-retire reads refuse.
    let outcome = unsafe {
        binding.serve(
            entry_zero,
            block,
            buffer.as_mut_ptr(),
            &mut accumulator as *mut u64,
        )
    };
    assert_eq!(outcome, ServeOutcome::Refused);

    binding.reader_exit();
    assert!(binding.reclaim_eligible());
    assert_eq!(binding.state(), BindingState::Retired);
}

// ── Thread lifecycle and metrics ─────────────────────────────────────

#[test]
fn spawned_generator_covers_the_bank_and_records_metrics() {
    let source = replay_fixture(true);
    let oracle = transform_bank_oracle(&source, 175);
    let binding = Arc::new(make_binding(source, 175, true, None));
    let handle = spawn(Arc::clone(&binding)).expect("generator thread spawns");

    let virtual_size = binding.layout().virtual_size;
    let mut file = vec![0u8; usize::try_from(virtual_size).expect("fits")];
    let mut accumulator = 0u64;
    let deadline = Instant::now() + Duration::from_secs(30);

    let mut read_all = |file: &mut [u8], offset: u64, len: u32| -> u32 {
        let dest = file[offset as usize..].as_mut_ptr();
        match unsafe { binding.serve(offset, len, dest, &mut accumulator as *mut u64) } {
            ServeOutcome::Served(served) => {
                accumulator = 0;
                served
            }
            ServeOutcome::Pending => loop {
                match unsafe { binding.poll(&mut accumulator as *mut u64) } {
                    PollOutcome::Complete(bytes) => break u32::try_from(bytes).expect("fits"),
                    PollOutcome::Incomplete => {
                        assert!(Instant::now() < deadline, "producer stalled");
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    PollOutcome::NotPending => panic!("pending read vanished"),
                }
            },
            ServeOutcome::Refused => panic!("refused while active"),
        }
    };

    assert_eq!(read_all(&mut file, 0, 0x1000), 0x1000);
    for entry in 0..binding.layout().entries.len() {
        let data_len = binding.layout().entries[entry].streamed.data_len as u64;
        let block_align = u64::from(binding.entry_format(entry).block_align());
        let packet = 65_536 / block_align * block_align;
        let mut cursor = 0u64;
        while cursor < data_len {
            let request = packet.min(data_len - cursor) as u32;
            let offset = binding.layout().entry_offsets[entry] + cursor;
            let served = read_all(&mut file, offset, request);
            assert_eq!(served, request);
            cursor += u64::from(served);
        }
    }

    assert!(file == oracle, "thread-produced bytes match the oracle");

    // Retire stops the producer promptly (the generation token) and the
    // metrics carry the full production record — the MAIN entry only (the
    // side entry is a verbatim passthrough the producer never touches).
    binding.retire();
    handle.join().expect("generator thread joins");
    let metrics = binding.metrics_snapshot();
    let planned_frames = u64::from(
        binding.layout().entries[binding.layout().main_entry_index]
            .streamed
            .duration,
    );
    assert_eq!(metrics.frames_produced, planned_frames);
    assert!(metrics.wall_nanos > 0, "wall time recorded at thread exit");
    assert!(binding.reclaim_eligible());
}

// ── Synchronous core lifecycle ───────────────────────────────────────

#[test]
fn core_idles_at_end_of_stream_and_stops_on_request() {
    let binding = make_binding(replay_fixture(false), 175, true, None);
    let mut core = GeneratorCore::new(&binding).expect("core constructs");

    // Drive to completion: the core reports Idle at end-of-stream (it stays
    // available for regeneration) — never a terminal state by itself.
    let mut steps = 0u64;
    loop {
        match core.step() {
            StepOutcome::Working => {}
            StepOutcome::Idle => break,
            StepOutcome::Stopped => panic!("stopped without a request"),
        }
        steps += 1;
        assert!(steps < 100_000, "core failed to reach end of stream");
    }
    let produced = binding.layout().virtual_size;
    let mut accumulator = 0u64;
    let mut buffer = vec![0u8; 512];
    // Everything is produced: an in-stream read serves synchronously.
    let outcome = unsafe {
        binding.serve(
            binding.layout().entry_offsets[0],
            512,
            buffer.as_mut_ptr(),
            &mut accumulator as *mut u64,
        )
    };
    assert_eq!(outcome, ServeOutcome::Served(512));
    assert!(produced > 0);

    binding.request_stop();
    assert_eq!(core.step(), StepOutcome::Stopped);
}

/// REGRESSION PIN (cabinet finding 2026-08-10, step05-fix): the engine's
/// bank-prepare primes a stream context for EVERY wave — including the
/// never-played-during-gameplay preview entry — and the game's loading
/// screen waits for that read. Under the preview passthrough (v2) the side
/// entry's bytes are the resident STOCK bytes: the prepare read completes
/// SYNCHRONOUSLY, with zero producer involvement (the old linear-ring
/// model took the full stream production time — 23+ s at 25% live; the v1
/// side-buffer fix still stalled on stretching the preview at the DSP's
/// few-×-realtime speed at 47 kHz).
#[test]
fn side_entry_prepare_read_completes_without_main_production() {
    for preview_first in [false, true] {
        let source = replay_fixture(preview_first);
        let stock = xwb::parse_song_bank(&source).expect("stock parse");
        let binding = make_binding(source.clone(), 25, true, None);
        let side = 1 - binding.layout().main_entry_index;
        let side_offset = binding.layout().entry_offsets[side];
        let main_offset = binding.layout().entry_offsets[binding.layout().main_entry_index];

        // The prepare-shaped read: the side entry's first packet. NO
        // producer exists at all — the serve must complete synchronously
        // from the resident source.
        let read_len = 2_048u32;
        let mut buffer = vec![0u8; read_len as usize];
        let mut accumulator = 0u64;
        let outcome =
            unsafe { binding.serve(side_offset, read_len, buffer.as_mut_ptr(), &mut accumulator) };
        assert_eq!(
            outcome,
            ServeOutcome::Served(read_len),
            "the side entry must serve synchronously (order {preview_first})"
        );
        // Verbatim: the served bytes ARE the stock preview bytes.
        assert_eq!(
            &buffer[..],
            &stock.entries[side].data[..read_len as usize],
            "order {preview_first}"
        );
        // The whole point: zero main-entry production was needed.
        assert_eq!(
            binding.ring_produced(),
            main_offset,
            "serving the side entry must not require producing the main entry (order {preview_first})"
        );
    }
}

// ── Resample mode (preserve-pitch OFF) ───────────────────────────────

/// AC (design §Components 2): with `preserve_pitch = false` the producer
/// streams the RESAMPLE oracle's bytes through the identical plan/serve
/// composition — the mode swap is invisible to everything downstream.
#[test]
fn resample_mode_replay_matches_the_resample_oracle() {
    for (percent, preview_first) in [(50, false), (50, true), (175, false), (175, true)] {
        let source = replay_fixture(preview_first);
        let stretch_oracle = transform_bank_oracle_mode(&source, percent, true);
        let resample_oracle = transform_bank_oracle_mode(&source, percent, false);
        assert_ne!(
            stretch_oracle, resample_oracle,
            "modes must actually differ at {percent}%"
        );
        let binding = make_binding(source, percent, false, None);
        assert!(!binding.preserve_pitch(), "flag reaches the binding");
        let mut core = GeneratorCore::new(&binding).expect("core constructs");

        let file = replay_via_serve(&binding, &mut core);
        assert!(
            file == resample_oracle,
            "served bytes diverge from the resample oracle at {percent}% preview_first={preview_first}"
        );

        // Still a valid bank end to end.
        let bank = xwb::parse_song_bank(&file).expect("served file reparses");
        for entry in &bank.entries {
            adpcm::decode_interleaved(entry.data, entry.format, entry.duration)
                .expect("served entry decodes");
        }
    }
}

/// Behind-window regeneration in resample mode: the positional seek must
/// reproduce identical bytes (no checkpoints involved).
#[test]
fn resample_mode_behind_window_regen_is_identical() {
    let capacity = 2_048usize;
    let binding = make_binding(replay_fixture(false), 50, false, Some(capacity));
    let mut core = GeneratorCore::new(&binding).expect("core constructs");
    let entry_zero = binding.layout().entry_offsets[0];
    let block = binding.entry_format(0).block_align() as usize;
    let data_len = binding.layout().entries[0].streamed.data_len;
    assert!(data_len > capacity * 4, "fixture must dwarf the test ring");

    let packet = (4 * block) as u32;
    let mut first_serving = vec![0u8; packet as usize];
    let mut scratch = vec![0u8; packet as usize];
    let mut accumulator = 0u64;
    let mut cursor = 0u64;
    while cursor < data_len as u64 {
        let request = u64::from(packet).min(data_len as u64 - cursor) as u32;
        let dest = if cursor == 0 {
            first_serving.as_mut_ptr()
        } else {
            scratch.as_mut_ptr()
        };
        let served = match unsafe {
            binding.serve(
                entry_zero + cursor,
                request,
                dest,
                &mut accumulator as *mut u64,
            )
        } {
            ServeOutcome::Served(served) => {
                accumulator = 0;
                served
            }
            ServeOutcome::Pending => {
                let bytes = pump(&binding, &mut core, &mut accumulator);
                u32::try_from(bytes).expect("fits")
            }
            ServeOutcome::Refused => panic!("refused mid-stream"),
        };
        assert_eq!(served, request);
        cursor += u64::from(served);
    }

    let mut second_serving = vec![0u8; packet as usize];
    let outcome = unsafe {
        binding.serve(
            entry_zero,
            packet,
            second_serving.as_mut_ptr(),
            &mut accumulator as *mut u64,
        )
    };
    assert_eq!(
        outcome,
        ServeOutcome::Pending,
        "a read below the ring window must defer for regeneration"
    );
    let bytes = pump(&binding, &mut core, &mut accumulator);
    assert_eq!(bytes, u64::from(packet));
    assert_eq!(
        second_serving, first_serving,
        "resample regeneration must reproduce identical bytes"
    );
    assert_eq!(binding.state(), BindingState::Active);
}

// ── Side-target bindings (song-select preview rate, step 01) ─────────

/// AC2 + AC4 (preview design §Components 2 / §Testing 2): a Side-target
/// binding streams the `_s` entry through the ring in BOTH DSP modes while
/// the main entry serves verbatim — the full engine-shaped replay (header
/// read spanning regions, per-entry packets, defensive EOF read) is
/// byte-identical to the Side whole-buffer oracle.
#[test]
fn side_target_replay_matches_the_oracle() {
    for preview_first in [false, true] {
        for percent in [50u32, 175] {
            for preserve_pitch in [true, false] {
                let label =
                    format!("{percent}% preview_first={preview_first} wsola={preserve_pitch}");
                let source = replay_fixture(preview_first);
                let oracle = transform_bank_oracle_target(
                    &source,
                    percent,
                    preserve_pitch,
                    virtual_bank::StretchTarget::Side,
                );
                let binding = make_binding_target(
                    source,
                    percent,
                    preserve_pitch,
                    None,
                    virtual_bank::StretchTarget::Side,
                );
                let layout = binding.layout();
                assert_eq!(
                    layout.target_entry_index,
                    1 - layout.main_entry_index,
                    "{label}: side target"
                );
                let mut core = GeneratorCore::new(&binding).expect("core constructs");

                let file = replay_via_serve(&binding, &mut core);
                assert!(
                    file == oracle,
                    "{label}: served bytes diverge from the Side oracle"
                );

                // Ring production stayed inside the target (side) entry's
                // range — the verbatim main entry never entered the ring.
                let produced = binding.ring_produced();
                assert!(
                    produced >= binding.target_data_start()
                        && produced <= binding.target_data_end(),
                    "{label}: production left the target range"
                );

                // The reassembled virtual file is a valid bank.
                let bank = xwb::parse_song_bank(&file).expect("served file reparses");
                for entry in &bank.entries {
                    adpcm::decode_interleaved(entry.data, entry.format, entry.duration)
                        .expect("served entry decodes");
                }
            }
        }
    }
}

/// The 4-entry `goru` shape through the REAL serve dispatch: exactly one
/// entry rides the ring (the `goru` main for gameplay, `goru_s` for the
/// preview bind); `goru_cs` / `goru_ac` and the other role entry serve
/// verbatim from the resident source, and the reassembled file is the
/// whole-buffer oracle byte for byte.
#[test]
fn four_entry_bank_replay_matches_the_oracle_for_both_targets() {
    for (target, expected_target) in [
        (virtual_bank::StretchTarget::Main, 1usize),
        (virtual_bank::StretchTarget::Side, 3usize),
    ] {
        for percent in [50u32, 175] {
            let label = format!("goru {percent}% target={target:?}");
            let source = goru_fixture();
            let oracle = transform_bank_oracle_target(&source, percent, true, target);
            let binding = make_binding_target(source.clone(), percent, true, None, target);
            let layout = binding.layout();
            assert_eq!(layout.entries.len(), 4, "{label}");
            assert_eq!(layout.main_entry_index, 1, "{label}");
            assert_eq!(layout.preview_entry_index, 3, "{label}");
            assert_eq!(layout.target_entry_index, expected_target, "{label}");
            let mut core = GeneratorCore::new(&binding).expect("core constructs");

            let file = replay_via_serve(&binding, &mut core);
            assert!(
                file == oracle,
                "{label}: served bytes diverge from the oracle"
            );

            let produced = binding.ring_produced();
            assert!(
                produced >= binding.target_data_start() && produced <= binding.target_data_end(),
                "{label}: production left the target range"
            );

            // Every verbatim entry is the STOCK bytes, at the header's offsets.
            let served = xwb::parse_song_bank(&file).expect("served file reparses");
            let stock = xwb::parse_song_bank(&source).expect("stock parses");
            assert_eq!(served.entry_count(), 4);
            for index in 0..4 {
                if index != expected_target {
                    assert_eq!(
                        served.entries[index].data, stock.entries[index].data,
                        "{label}: entry {index}"
                    );
                    assert_eq!(served.entries[index].name(), stock.entries[index].name());
                }
                adpcm::decode_interleaved(
                    served.entries[index].data,
                    served.entries[index].format,
                    served.entries[index].duration,
                )
                .expect("served entry decodes");
            }
        }
    }
}

/// AC3: on a Side-target binding the MAIN entry is the verbatim one — its
/// prepare-shaped first packet serves synchronously from the resident
/// source with ZERO production (the inverse of
/// `side_entry_prepare_read_completes_without_main_production`).
#[test]
fn main_entry_prepare_read_completes_without_side_production() {
    for preview_first in [false, true] {
        let source = replay_fixture(preview_first);
        let stock = xwb::parse_song_bank(&source).expect("stock parse");
        let binding = make_binding_target(
            source.clone(),
            25,
            true,
            None,
            virtual_bank::StretchTarget::Side,
        );
        let main = binding.layout().main_entry_index;
        let main_offset = binding.layout().entry_offsets[main];

        let read_len = 2_048u32;
        let mut buffer = vec![0u8; read_len as usize];
        let mut accumulator = 0u64;
        let outcome =
            unsafe { binding.serve(main_offset, read_len, buffer.as_mut_ptr(), &mut accumulator) };
        assert_eq!(
            outcome,
            ServeOutcome::Served(read_len),
            "the verbatim main entry must serve synchronously (order {preview_first})"
        );
        assert_eq!(
            &buffer[..],
            &stock.entries[main].data[..read_len as usize],
            "order {preview_first}"
        );
        assert_eq!(
            binding.ring_produced(),
            binding.target_data_start(),
            "serving the main entry must not require producing the side entry (order {preview_first})"
        );
    }
}

/// AC5: retire-under-read semantics are unchanged on a Side-target
/// binding — a deferred target-entry read is cancelled with the EOF-clamp
/// 0-byte completion and later serves refuse.
#[test]
fn side_target_retire_cancels_pending() {
    let binding = make_binding_target(
        replay_fixture(false),
        50,
        true,
        None,
        virtual_bank::StretchTarget::Side,
    );
    let target_offset = binding.target_data_start();
    let block = binding
        .entry_format(binding.layout().target_entry_index)
        .block_align();

    let mut buffer = vec![0u8; block as usize];
    let mut accumulator = 0u64;
    let outcome = unsafe {
        binding.serve(
            target_offset,
            block,
            buffer.as_mut_ptr(),
            &mut accumulator as *mut u64,
        )
    };
    assert_eq!(outcome, ServeOutcome::Pending, "nothing produced yet");

    binding.retire();
    assert_eq!(
        unsafe { binding.poll(&mut accumulator as *mut u64) },
        PollOutcome::Complete(0),
        "retire cancels with the EOF-clamp semantics"
    );
    let outcome = unsafe {
        binding.serve(
            target_offset,
            block,
            buffer.as_mut_ptr(),
            &mut accumulator as *mut u64,
        )
    };
    assert_eq!(outcome, ServeOutcome::Refused, "post-retire reads refuse");
    assert!(binding.reclaim_eligible());
}

// ── Content mapping (training mode Step 1) ───────────────────────────

/// Build an IdentityPassthrough binding over a fixture — the training-mode
/// identity arm's runtime shape. No producer thread exists for these.
pub(super) fn make_identity_binding(source: Vec<u8>) -> Binding {
    let bank = xwb::parse_song_bank(&source).expect("fixture parses");
    let layout = virtual_bank::plan_identity_bank(&bank).expect("identity plan");
    drop(bank);
    Binding::new_identity_passthrough(5, 1, layout, source.into_boxed_slice())
        .expect("identity binding constructs")
}

/// The mapped main-entry reference (design §4.5): `lead` silent blocks,
/// then the stream's content from block `shift`, then silent tiling to the
/// entry's declared length.
pub(super) fn remap_main_entry(
    stream: &[u8],
    format: WaveFormat,
    shift_blocks: u64,
    lead_blocks: u64,
) -> Vec<u8> {
    let align = format.block_align() as usize;
    let zeros = vec![0i16; format.samples_per_block() as usize * format.channels() as usize];
    let mut silent = Vec::new();
    adpcm::encode_block(&zeros, format, &mut silent).expect("silent block encodes");
    let mut out = Vec::with_capacity(stream.len());
    for _ in 0..lead_blocks {
        out.extend_from_slice(&silent);
    }
    let shift = shift_blocks as usize * align;
    if shift < stream.len() {
        out.extend_from_slice(&stream[shift..]);
    }
    while out.len() < stream.len() {
        out.extend_from_slice(&silent);
    }
    out.truncate(stream.len());
    out
}

#[test]
fn generator_refuses_identity_passthrough_bindings() {
    let binding = make_identity_binding(replay_fixture(false));
    assert!(
        GeneratorCore::new(&binding).is_err(),
        "identity passthrough bindings have no producer by design"
    );
}

/// The SEEDED mapped main-entry reference (design §4.5 amendment): `lead`
/// silent blocks, then a FRESH stretch seeded at the shift-mapped position
/// — never a slice of the canonical stream — then silent tiling to the
/// entry's declared length. The byte authority for a shift>0 WSOLA epoch.
fn remap_main_entry_seeded(
    source_bank: &[u8],
    percent: u32,
    shift_blocks: u64,
    lead_blocks: u64,
) -> Vec<u8> {
    let bank = xwb::parse_song_bank(source_bank).expect("seeded reference parse");
    let layout = virtual_bank::plan_virtual_bank(&bank, percent, virtual_bank::StretchTarget::Main)
        .expect("seeded reference plan");
    let main = layout.main_entry_index;
    let entry = &bank.entries[main];
    let format = entry.format;
    let spb = format.samples_per_block() as usize;
    let stream_len = layout.entries[main].streamed.data_len;
    let decoded =
        adpcm::decode_interleaved(entry.data, format, entry.duration).expect("seeded decode");
    let seek_frame = shift_blocks as usize * spb;
    let tail = stretch::stretch_seeded_interleaved(
        &decoded,
        format.channels() as usize,
        format.sample_rate(),
        layout.entries[main].streamed.duration as usize,
        seek_frame,
    )
    .expect("seeded reference stretch");
    let encoded = adpcm::encode_interleaved(&tail, format).expect("seeded encode");
    let zeros = vec![0i16; spb * format.channels() as usize];
    let mut silent = Vec::new();
    adpcm::encode_block(&zeros, format, &mut silent).expect("silent block encodes");
    let mut out = Vec::with_capacity(stream_len);
    for _ in 0..lead_blocks {
        out.extend_from_slice(&silent);
    }
    out.extend_from_slice(&encoded);
    while out.len() < stream_len {
        out.extend_from_slice(&silent);
    }
    out.truncate(stream_len);
    out
}

#[test]
fn stretch_mapping_change_reserves_the_remapped_stream() {
    // Oracle re-anchored (Step-2 task-01): a shift>0 WSOLA epoch serves the
    // SEEDED stream, not a canonical slice.
    for percent in [50u32, 175] {
        let source = replay_fixture(false);
        let binding = make_binding(source.clone(), percent, true, None);
        let mut core = GeneratorCore::new(&binding).expect("core constructs");

        // First pass: the unmapped stream, byte-equal to the canonical oracle.
        let oracle = transform_bank_oracle(&source, percent);
        let original = replay_via_serve(&binding, &mut core);
        assert!(
            original == oracle,
            "unmapped stream diverges from the oracle at {percent}%"
        );

        // Publish a mapping: the generator must pick it up, bump the ring
        // seqlock (ring_rewind — the existing behind-window machinery), and
        // restart production at output 0 under the new mapping.
        const SHIFT_BLOCKS: u64 = 5;
        const LEAD_BLOCKS: u64 = 2;
        let rewinds_before = binding.ring_rewind_count();
        assert!(binding.set_content_mapping(SHIFT_BLOCKS, LEAD_BLOCKS));
        assert_eq!(binding.content_mapping(), (SHIFT_BLOCKS, LEAD_BLOCKS));

        let remapped = replay_via_serve(&binding, &mut core);
        assert!(
            binding.ring_rewind_count() > rewinds_before,
            "the mapping change must bump the ring seqlock at {percent}%"
        );

        let main = binding.layout().main_entry_index;
        let offset = binding.layout().entry_offsets[main] as usize;
        let len = binding.layout().entries[main].streamed.data_len;
        let mut expected = oracle.clone();
        let reference = remap_main_entry_seeded(&source, percent, SHIFT_BLOCKS, LEAD_BLOCKS);
        assert_eq!(reference.len(), len);
        expected[offset..offset + len].copy_from_slice(&reference);
        assert!(
            remapped == expected,
            "re-served bytes diverge from the seeded reference at {percent}%"
        );
    }
}

#[test]
fn deep_shift_seeds_without_producing_the_pre_shift_chain() {
    // AC-3 (O(1) seeding): a deep-shift epoch's production is bounded by the
    // CONTENT window — the canonical model would have produced (and
    // discarded) the whole pre-shift alignment chain first.
    let source = replay_fixture(false);
    let binding = make_binding(source, 50, true, None);
    let mut core = GeneratorCore::new(&binding).expect("core constructs");

    // Canonical pass to establish the baseline production count.
    let _ = replay_via_serve(&binding, &mut core);
    let baseline = binding.metrics_snapshot().frames_produced;

    let main = binding.layout().main_entry_index;
    let format = binding.entry_format(main);
    let spb = u64::from(format.samples_per_block());
    let align = u64::from(format.block_align());
    let stream_blocks = binding.layout().entries[main].streamed.data_len as u64 / align;
    // Shift three quarters of the stream: the pre-shift chain dwarfs the
    // remaining content window.
    let shift_blocks = stream_blocks * 3 / 4;
    let content_frames = (stream_blocks - shift_blocks) * spb;
    let pre_shift_frames = shift_blocks * spb;
    assert!(binding.set_content_mapping(shift_blocks, 0));

    let _ = replay_via_serve(&binding, &mut core);
    let delta = binding.metrics_snapshot().frames_produced - baseline;
    assert!(
        delta > 0,
        "the mapped replay must produce the content window"
    );
    assert!(
        delta <= content_frames + 4_096,
        "seeded production ({delta} frames) must stay bounded by the content \
         window ({content_frames} frames) — the canonical chain is {pre_shift_frames}"
    );
}

#[test]
fn epoch_change_invalidates_checkpoints_and_regen_reproduces_seeded_bytes() {
    // AC-4: a canonical run captures its loop-start checkpoint; a mapping
    // change opens a new epoch (checkpoints invalidated); a behind-window
    // regeneration WITHIN the new epoch must reproduce the CURRENT (seeded)
    // run's bytes — never the previous epoch's.
    let capacity = 2_048usize;
    let binding = make_binding(replay_fixture(false), 50, true, Some(capacity));
    let mut core = GeneratorCore::new(&binding).expect("core constructs");
    let entry_zero = binding.layout().entry_offsets[0];
    let block = binding.entry_format(0).block_align() as usize;
    let data_len = binding.layout().entries[0].streamed.data_len;
    assert!(data_len > capacity * 4, "fixture must dwarf the test ring");

    // Canonical pass over the whole entry: captures the loop-start
    // checkpoint (the state this test proves is NOT reused post-epoch).
    let packet = (4 * block) as u32;
    let mut scratch = vec![0u8; packet as usize];
    let mut accumulator = 0u64;
    let mut serve_at = |core: &mut GeneratorCore<'_>, offset: u64, dest: &mut [u8]| {
        let request = u32::try_from(dest.len().min((data_len as u64 - offset) as usize))
            .expect("request fits");
        let served = match unsafe {
            binding.serve(
                entry_zero + offset,
                request,
                dest.as_mut_ptr(),
                &mut accumulator as *mut u64,
            )
        } {
            ServeOutcome::Served(served) => {
                accumulator = 0;
                served
            }
            ServeOutcome::Pending => {
                let bytes = pump(&binding, core, &mut accumulator);
                u32::try_from(bytes).expect("fits")
            }
            ServeOutcome::Refused => panic!("refused mid-stream"),
        };
        assert_eq!(served, request);
    };
    let mut cursor = 0u64;
    while cursor < data_len as u64 {
        let len = (packet as u64).min(data_len as u64 - cursor) as usize;
        serve_at(&mut core, cursor, &mut scratch[..len]);
        cursor += len as u64;
    }

    // New epoch.
    const SHIFT_BLOCKS: u64 = 5;
    const LEAD_BLOCKS: u64 = 2;
    assert!(binding.set_content_mapping(SHIFT_BLOCKS, LEAD_BLOCKS));

    // Stream the mapped entry sequentially, recording a packet deep in the
    // content region (the 20-block offset is a whole number of 4-block
    // packets, so the recording pass lands on it exactly).
    let target = 20 * block;
    assert_eq!(target % packet as usize, 0, "target must be packet-aligned");
    assert!(target > LEAD_BLOCKS as usize * block, "target is content");
    let mut first_serving = vec![0u8; packet as usize];
    let mut cursor = 0u64;
    while cursor < data_len as u64 {
        let len = (packet as u64).min(data_len as u64 - cursor) as usize;
        if cursor == target as u64 {
            serve_at(&mut core, cursor, &mut first_serving[..len]);
        } else {
            serve_at(&mut core, cursor, &mut scratch[..len]);
        }
        cursor += len as u64;
    }

    // The behind-window re-read: regeneration within the SAME epoch.
    let mut second_serving = vec![0u8; packet as usize];
    serve_at(&mut core, target as u64, &mut second_serving[..]);
    assert_eq!(
        second_serving, first_serving,
        "within-epoch regeneration must reproduce the seeded run's bytes"
    );
    assert_eq!(binding.state(), BindingState::Active);
}
