use super::{adpcm, digest, rate, resample, stretch, virtual_bank, xwb, WaveFormat};

const BLOCK_ALIGN: usize = 140;
const SAMPLES_PER_BLOCK: u32 = 128;
const SEGMENT_FOUR_OFFSET: usize = 2048;

fn format(sample_rate: u32, channels: u8) -> WaveFormat {
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

fn build_bank(preview_first: bool, tail_remainder: usize) -> Vec<u8> {
    build_bank_with_data_lengths(
        preview_first,
        [
            2 * BLOCK_ALIGN + tail_remainder,
            3 * BLOCK_ALIGN + tail_remainder,
        ],
    )
}

fn build_bank_with_data_lengths(preview_first: bool, data_lengths: [usize; 2]) -> Vec<u8> {
    let zero_payloads = [vec![0u8; data_lengths[0]], vec![0u8; data_lengths[1]]];
    let durations = [
        (data_lengths[0] / BLOCK_ALIGN) as u32 * SAMPLES_PER_BLOCK,
        (data_lengths[1] / BLOCK_ALIGN) as u32 * SAMPLES_PER_BLOCK,
    ];
    build_bank_bytes(
        preview_first,
        [format(48_000, 2), format(48_001, 2)],
        [&zero_payloads[0], &zero_payloads[1]],
        durations,
        [(0, durations[0]), (0, durations[1])],
    )
}

/// Synthesize a strict-profile `tst1`/`tst1_s` bank around explicit
/// per-entry formats, encoded payloads, durations, and loops.
fn build_bank_bytes(
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

fn assert_rejected(mut bytes: Vec<u8>, mutate: impl FnOnce(&mut Vec<u8>)) {
    mutate(&mut bytes);
    assert!(xwb::parse_song_bank(&bytes).is_err());
}

#[test]
fn parses_both_physical_entry_orders_as_borrowed_views() {
    for preview_first in [false, true] {
        let bytes = build_bank(preview_first, 0);
        let bank = xwb::parse_song_bank(&bytes).expect("valid synthetic bank");
        let expected = if preview_first {
            ["tst1_s", "tst1"]
        } else {
            ["tst1", "tst1_s"]
        };

        assert_eq!(bank.name(), "tst1");
        assert_eq!(bank.entries[0].name(), expected[0]);
        assert_eq!(bank.entries[1].name(), expected[1]);
        assert_eq!(bank.entries[0].format.sample_rate(), 48_000);
        assert_eq!(bank.entries[1].format.sample_rate(), 48_001);
        assert!(std::ptr::eq(
            bank.entries[0].data.as_ptr(),
            bytes[SEGMENT_FOUR_OFFSET..].as_ptr(),
        ));
        assert!(std::ptr::eq(bank.name().as_ptr(), bytes[60..].as_ptr()));
        assert!(std::ptr::eq(
            bank.entries[0].name().as_ptr(),
            bytes[196..].as_ptr(),
        ));
    }
}

#[test]
fn accepts_ordered_padding_and_compatible_custom_codes() {
    let mut bytes = build_bank(false, 0);
    bytes.copy_within(196..324, 208);
    put_u32(&mut bytes, 28, 200);
    put_u32(&mut bytes, 36, 208);

    let code = "custom_code_1234567890";
    bytes[60..124].copy_from_slice(&fixed_name(code));
    bytes[208..272].copy_from_slice(&fixed_name(code));
    bytes[272..336].copy_from_slice(&fixed_name(&format!("{code}_s")));

    let bank = xwb::parse_song_bank(&bytes).expect("ordered padding and custom code");
    assert_eq!(bank.name(), code);
    assert_eq!(bank.entries[0].name(), code);
    assert_eq!(bank.entries[1].name(), format!("{code}_s"));
}

#[test]
fn rejects_malformed_header_segments_and_bank_data() {
    let cases: &[(&str, fn(&mut Vec<u8>))] = &[
        ("magic", |b| b[0] = b'X'),
        ("version", |b| put_u32(b, 4, 42)),
        ("header version", |b| put_u32(b, 8, 41)),
        ("segment zero offset", |b| put_u32(b, 12, 53)),
        ("segment zero length", |b| put_u32(b, 16, 95)),
        ("segment one offset", |b| put_u32(b, 20, 149)),
        ("segment one length", |b| put_u32(b, 24, 24)),
        ("seek offset", |b| put_u32(b, 28, 197)),
        ("seek length", |b| put_u32(b, 32, 1)),
        ("name offset", |b| put_u32(b, 36, 197)),
        ("name length", |b| put_u32(b, 40, 64)),
        ("wave offset unaligned", |b| put_u32(b, 44, 2049)),
        ("wave length not eof", |b| {
            let length = u32::from_le_bytes(b[48..52].try_into().unwrap());
            put_u32(b, 48, length - 1);
        }),
        ("flags", |b| put_u32(b, 52, 0x0009_0000)),
        ("entry count", |b| put_u32(b, 56, 1)),
        ("bank name terminator", |b| b[123] = b'x'),
        ("metadata size", |b| put_u32(b, 124, 4)),
        ("entry name size", |b| put_u32(b, 128, 32)),
        ("alignment", |b| put_u32(b, 132, 4)),
        ("compact format", |b| put_u32(b, 136, 1)),
    ];

    for (name, mutate) in cases {
        let mut bytes = build_bank(false, 0);
        mutate(&mut bytes);
        assert!(
            xwb::parse_song_bank(&bytes).is_err(),
            "malformed {name} was accepted"
        );
    }
}

#[test]
fn rejects_malformed_entry_identity_format_and_ranges() {
    let cases: &[(&str, fn(&mut Vec<u8>))] = &[
        ("entry flags", |b| {
            let value = u32::from_le_bytes(b[148..152].try_into().unwrap());
            put_u32(b, 148, value | 1);
        }),
        ("zero duration", |b| put_u32(b, 148, 0)),
        ("codec", |b| {
            let value = u32::from_le_bytes(b[152..156].try_into().unwrap());
            put_u32(b, 152, (value & !3) | 1);
        }),
        ("channels", |b| {
            let value = u32::from_le_bytes(b[152..156].try_into().unwrap());
            put_u32(b, 152, (value & !(7 << 2)) | (1 << 2));
        }),
        ("sample rate", |b| {
            let value = u32::from_le_bytes(b[152..156].try_into().unwrap());
            put_u32(b, 152, value & !(0x3ffff << 5));
        }),
        ("raw block alignment", |b| {
            let value = u32::from_le_bytes(b[152..156].try_into().unwrap());
            put_u32(b, 152, (value & !(0xff << 23)) | (47 << 23));
        }),
        ("bits flag", |b| {
            let value = u32::from_le_bytes(b[152..156].try_into().unwrap());
            put_u32(b, 152, value | (1 << 31));
        }),
        ("loop past duration", |b| put_u32(b, 168, 257)),
        ("data out of bounds", |b| put_u32(b, 160, u32::MAX)),
        ("overlap", |b| put_u32(b, 180, 128)),
        ("second offset unaligned", |b| put_u32(b, 180, 2047)),
        ("entry name terminator", |b| b[196 + 63] = b'x'),
        ("duplicate entry identity", |b| {
            b[260..324].copy_from_slice(&fixed_name("tst1"));
        }),
        ("wrong entry identity", |b| {
            b[260..324].copy_from_slice(&fixed_name("other"));
        }),
    ];

    for (name, mutate) in cases {
        let mut bytes = build_bank(false, 0);
        mutate(&mut bytes);
        assert!(
            xwb::parse_song_bank(&bytes).is_err(),
            "malformed {name} was accepted"
        );
    }
}

#[test]
fn accepts_only_documented_stock_tail_remainders_and_duration_equations() {
    for remainder in [0, BLOCK_ALIGN - 1, BLOCK_ALIGN - 2] {
        let bytes = build_bank(false, remainder);
        let bank = xwb::parse_song_bank(&bytes).expect("documented tail must parse");
        let pcm = adpcm::decode_interleaved(
            bank.entries[0].data,
            bank.entries[0].format,
            bank.entries[0].duration,
        )
        .expect("documented tail must decode");
        assert_eq!(pcm.len(), bank.entries[0].duration as usize * 2);
    }

    assert_rejected(build_bank(false, 1), |_| {});
    assert_rejected(build_bank(false, BLOCK_ALIGN - 3), |_| {});
    assert_rejected(build_bank(false, 0), |b| {
        put_u32(b, 148, (SAMPLES_PER_BLOCK << 4) as u32);
    });

    let mut trimmed = build_bank(false, BLOCK_ALIGN - 1);
    let logical_frames = 2 * SAMPLES_PER_BLOCK - 17;
    put_u32(&mut trimmed, 148, logical_frames << 4);
    put_u32(&mut trimmed, 168, logical_frames);
    let bank = xwb::parse_song_bank(&trimmed).expect("non-block logical duration");
    let pcm = adpcm::decode_interleaved(
        bank.entries[0].data,
        bank.entries[0].format,
        bank.entries[0].duration,
    )
    .expect("trim logical duration");
    assert_eq!(pcm.len(), logical_frames as usize * 2);
}

#[test]
fn codec_is_deterministic_exact_and_preserves_stereo_order() {
    for channels in [1, 2, 3, 6] {
        let fmt = format(44_100, channels);
        let frames = fmt.samples_per_block() as usize * 3;
        let mut pcm = Vec::with_capacity(frames * channels as usize);
        for frame in 0..frames {
            for channel in 0..channels as usize {
                let phase = frame as f64 * (220.0 + channel as f64 * 330.0) * std::f64::consts::TAU
                    / 44_100.0
                    + channel as f64 * 0.3;
                pcm.push((phase.sin() * (12_000.0 - channel as f64 * 2_000.0)) as i16);
            }
        }

        let first = adpcm::encode_interleaved(&pcm, fmt).expect("encode");
        let second = adpcm::encode_interleaved(&pcm, fmt).expect("encode again");
        let mut streamed = Vec::new();
        adpcm::encode_interleaved_to(&pcm, fmt, &mut streamed).expect("streaming encode");
        assert_eq!(first, second);
        assert_eq!(first, streamed);
        assert_eq!(first.len(), 3 * fmt.block_align() as usize);

        let decoded = adpcm::decode_interleaved(&first, fmt, frames as u32).expect("decode");
        assert_eq!(decoded.len(), pcm.len());
        let (mut signal, mut noise) = (0.0, 0.0);
        for (&source, &actual) in pcm.iter().zip(&decoded) {
            signal += f64::from(source).powi(2);
            noise += (f64::from(source) - f64::from(actual)).powi(2);
        }
        let snr = 10.0 * (signal / noise).log10();
        assert!(snr >= 30.0, "{channels}-channel SNR was {snr:.2} dB");

        if channels >= 2 {
            assert_ne!(decoded[0], decoded[1]);
            assert_ne!(decoded[10], decoded[11]);
        }
    }
}

#[test]
fn codec_rejects_padding_and_malformed_blocks() {
    let mono = format(44_100, 1);
    let stereo = format(44_100, 2);
    assert!(adpcm::encode_interleaved(&[0; 127], mono).is_err());
    assert!(adpcm::encode_interleaved(&[0; 255], stereo).is_err());
    assert!(adpcm::decode_interleaved(&[0; 70], stereo, 128).is_err());

    let mut bad_predictor = vec![0; mono.block_align() as usize];
    bad_predictor[0] = 7;
    assert!(adpcm::decode_interleaved(&bad_predictor, mono, 128).is_err());
}

fn deterministic_pcm(fmt: WaveFormat, blocks: usize) -> Vec<i16> {
    let channels = fmt.channels() as usize;
    let frames = fmt.samples_per_block() as usize * blocks;
    let mut pcm = Vec::with_capacity(frames * channels);
    for frame in 0..frames {
        for channel in 0..channels {
            let phase = frame as f64 * (220.0 + channel as f64 * 330.0) * std::f64::consts::TAU
                / 44_100.0
                + channel as f64 * 0.3;
            pcm.push((phase.sin() * (12_000.0 - channel as f64 * 1_500.0)) as i16);
        }
    }
    pcm
}

fn expect_panic<T>(operation: impl FnOnce() -> T) -> bool {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        operation();
    }));
    std::panic::set_hook(previous);
    result.is_err()
}

#[test]
fn per_block_codec_wrappers_match_whole_buffer() {
    const BLOCKS: usize = 5;
    for channels in [1u8, 2, 6] {
        let fmt = format(44_100, channels);
        let channels = channels as usize;
        let block_align = fmt.block_align() as usize;
        let block_samples = fmt.samples_per_block() as usize * channels;
        let pcm = deterministic_pcm(fmt, BLOCKS);
        let whole = adpcm::encode_interleaved(&pcm, fmt).expect("whole-buffer encode");
        let frames = pcm.len() / channels;
        let whole_decoded =
            adpcm::decode_interleaved(&whole, fmt, frames as u32).expect("whole-buffer decode");

        let mut per_block_decoded = Vec::new();
        let mut per_block_encoded = Vec::new();
        for block in 0..BLOCKS {
            adpcm::decode_block(
                &whole[block * block_align..(block + 1) * block_align],
                fmt,
                &mut per_block_decoded,
            )
            .expect("per-block decode");
            adpcm::encode_block(
                &pcm[block * block_samples..(block + 1) * block_samples],
                fmt,
                &mut per_block_encoded,
            )
            .expect("per-block encode");
        }
        assert_eq!(
            per_block_decoded, whole_decoded,
            "{channels}-channel decode"
        );
        assert_eq!(per_block_encoded, whole, "{channels}-channel encode");
    }
}

#[test]
fn block_cache_view_matches_whole_buffer_decode() {
    use super::stretch::SourcePcm;

    const BLOCKS: usize = 80;
    let fmt = format(44_100, 2);
    let pcm = deterministic_pcm(fmt, BLOCKS);
    let data = adpcm::encode_interleaved(&pcm, fmt).expect("encode source entry");
    let logical = (BLOCKS * fmt.samples_per_block() as usize - 17) as u32;
    let expected = adpcm::decode_interleaved(&data, fmt, logical).expect("oracle decode");
    let view = adpcm::BlockCachePcm::new(&data, fmt, logical).expect("block-cache view");

    assert_eq!(view.frames(), logical as usize);
    assert_eq!(view.channels(), 2);
    for frame in 0..view.frames() {
        for channel in 0..2 {
            assert_eq!(view.sample(frame, channel), expected[frame * 2 + channel]);
        }
    }
    for frame in (0..view.frames()).rev() {
        for channel in 0..2 {
            assert_eq!(view.sample(frame, channel), expected[frame * 2 + channel]);
        }
    }
    // Deterministic pseudo-random order: stride 4871 is co-prime with the
    // frame count, so every frame is visited exactly once out of order.
    let frames = view.frames();
    for step in 0..frames {
        let frame = step * 4_871 % frames;
        for channel in 0..2 {
            assert_eq!(view.sample(frame, channel), expected[frame * 2 + channel]);
        }
    }
    // Same-slot alternation (direct-mapped collision) stays correct.
    let spb = fmt.samples_per_block() as usize;
    for _ in 0..8 {
        assert_eq!(view.sample(0, 0), expected[0]);
        assert_eq!(view.sample(64 * spb, 1), expected[64 * spb * 2 + 1]);
    }
    // Reads past the logical duration (and past the channel count) fail loudly.
    assert!(expect_panic(|| view.sample(logical as usize, 0)));
    assert!(expect_panic(|| view.sample(0, 2)));

    for channels in [1u8, 6] {
        let fmt = format(44_100, channels);
        let channels = channels as usize;
        let pcm = deterministic_pcm(fmt, 4);
        let data = adpcm::encode_interleaved(&pcm, fmt).expect("encode entry");
        let logical = (4 * fmt.samples_per_block() as usize) as u32;
        let expected = adpcm::decode_interleaved(&data, fmt, logical).expect("oracle decode");
        let view = adpcm::BlockCachePcm::new(&data, fmt, logical).expect("block-cache view");
        for frame in 0..view.frames() {
            for channel in 0..channels {
                assert_eq!(
                    view.sample(frame, channel),
                    expected[frame * channels + channel]
                );
            }
        }
    }
}

#[test]
fn block_codec_and_cache_reject_malformed_inputs() {
    let stereo = format(44_100, 2);
    let zero_channels = format(44_100, 0);
    let block_align = stereo.block_align() as usize;
    let block_samples = stereo.samples_per_block() as usize * 2;

    let mut sink_pcm = Vec::new();
    assert!(adpcm::decode_block(&vec![0; block_align - 1], stereo, &mut sink_pcm).is_err());
    assert!(adpcm::decode_block(&vec![0; block_align + 1], stereo, &mut sink_pcm).is_err());
    assert!(adpcm::decode_block(&vec![0; block_align], zero_channels, &mut sink_pcm).is_err());

    let mut sink_bytes = Vec::new();
    assert!(adpcm::encode_block(&vec![0; block_samples - 1], stereo, &mut sink_bytes).is_err());
    assert!(adpcm::encode_block(&vec![0; block_samples * 2], stereo, &mut sink_bytes).is_err());
    assert!(adpcm::encode_block(&vec![0; block_samples], zero_channels, &mut sink_bytes).is_err());

    let pcm = deterministic_pcm(stereo, 2);
    let data = adpcm::encode_interleaved(&pcm, stereo).expect("encode entry");
    let logical = (2 * stereo.samples_per_block() as usize) as u32;
    let mut bad_predictor = data.clone();
    bad_predictor[block_align] = 7;
    assert!(matches!(
        adpcm::BlockCachePcm::new(&bad_predictor, stereo, logical),
        Err(adpcm::AdpcmError::BadPredictor { index: 7 })
    ));
    let mut bad_tail = data.clone();
    bad_tail.push(0);
    assert!(adpcm::BlockCachePcm::new(&bad_tail, stereo, logical).is_err());
    assert!(adpcm::BlockCachePcm::new(&data, stereo, stereo.samples_per_block()).is_err());
    assert!(adpcm::BlockCachePcm::new(&data, zero_channels, logical).is_err());
}

#[test]
fn slice_pcm_view_is_a_faithful_trivial_source() {
    use super::stretch::SourcePcm;

    let pcm = stereo_sine(8_000, 64, 250.0, 375.0);
    let view = stretch::SlicePcm::new(&pcm, 2).expect("slice view");
    assert_eq!(view.frames(), 64);
    assert_eq!(view.channels(), 2);
    for frame in 0..64 {
        for channel in 0..2 {
            assert_eq!(view.sample(frame, channel), pcm[frame * 2 + channel]);
        }
    }

    assert!(matches!(
        stretch::SlicePcm::new(&pcm, 0),
        Err(stretch::StretchError::InvalidChannelCount)
    ));
    assert!(matches!(
        stretch::SlicePcm::new(&pcm[..3], 2),
        Err(stretch::StretchError::IncompleteSourceFrame)
    ));
    assert!(expect_panic(|| view.sample(64, 0)));
    assert!(expect_panic(|| view.sample(0, 2)));
}

/// Drive a `StretchState` to completion through a `SlicePcm`, honoring a
/// per-call chunk-size pattern (frames). Returns (samples, clipped, cyclic).
fn run_stretch_state(
    source: &[i16],
    channels: usize,
    sample_rate: u32,
    output_frames: usize,
    loop_context: Option<stretch::LoopContext>,
    mut chunk_frames: impl FnMut(usize) -> usize,
) -> Result<(Vec<i16>, usize, usize), stretch::StretchError> {
    let view = stretch::SlicePcm::new(source, channels).expect("source view");
    let mut state = stretch::StretchState::new(
        source.len() / channels,
        output_frames,
        channels,
        sample_rate,
        loop_context,
    )?;
    let mut samples = Vec::new();
    let mut call = 0usize;
    loop {
        let frames = chunk_frames(call).max(1);
        call += 1;
        let mut out = vec![0i16; frames * channels];
        let produced = state.produce(&view, &mut out)?;
        samples.extend_from_slice(&out[..produced.frames * channels]);
        if produced.done {
            break;
        }
        assert!(call < 1_000_000, "streaming run did not terminate");
    }
    Ok((samples, state.clipped_samples(), state.cyclic_windows()))
}

/// Deterministic multi-tone interleaved PCM for streaming-equality cells.
fn tone_pcm(frames: usize, channels: usize) -> Vec<i16> {
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

/// Half-up loop-boundary mapping (mirrors the shipped plan's rounding, as in
/// `loop_context_uses_cyclic_windows_and_preserves_seam`).
fn mapped_loop(
    source_start: usize,
    source_end: usize,
    source_frames: usize,
    output_frames: usize,
) -> stretch::LoopContext {
    let map = |frame: usize| (frame * output_frames + source_frames / 2) / source_frames;
    stretch::LoopContext {
        source_start,
        source_end,
        output_start: map(source_start),
        output_end: map(source_end),
    }
}

fn reference_stretch(
    source: &[i16],
    channels: usize,
    output_frames: usize,
    loop_context: Option<stretch::LoopContext>,
) -> Result<stretch::StretchResult, stretch::StretchError> {
    stretch::stretch_interleaved(source, channels, 8_000, output_frames, loop_context)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum LoopShape {
    None,
    Interior,
    Boundary,
}

fn loop_for_shape(
    shape: LoopShape,
    source_frames: usize,
    output_frames: usize,
) -> Option<stretch::LoopContext> {
    match shape {
        LoopShape::None => None,
        LoopShape::Interior => Some(mapped_loop(1_000, 7_000, source_frames, output_frames)),
        LoopShape::Boundary => Some(mapped_loop(0, source_frames, source_frames, output_frames)),
    }
}

/// Behavior parity with the whole-buffer reference: equal bytes and counters
/// where it succeeds, the identical `StretchError` where it fails. (The
/// reference deterministically reports `NoCandidate` for 25%/50% cells
/// without a full-entry loop — near the output end the nominal exceeds the
/// last window start by more than the search radius; production banks always
/// carry boundary-style whole-entry loops, which succeed at every rate.)
fn assert_streaming_matches_reference(
    source: &[i16],
    channels: usize,
    output_frames: usize,
    loop_context: Option<stretch::LoopContext>,
    label: &str,
) {
    let reference = reference_stretch(source, channels, output_frames, loop_context);
    let hop = stretch::StretchParameters::for_sample_rate(8_000)
        .unwrap()
        .synthesis_hop;
    for (pattern, chunk) in [("whole", output_frames), ("hop", hop)] {
        let streaming =
            run_stretch_state(source, channels, 8_000, output_frames, loop_context, |_| {
                chunk
            });
        match (&reference, streaming) {
            (Ok(reference), Ok((samples, clipped, cyclic))) => {
                assert_eq!(
                    samples, reference.samples,
                    "{label} ({pattern} chunks): streaming bytes diverge from the reference"
                );
                assert_eq!(
                    clipped, reference.clipped_samples,
                    "{label} ({pattern} chunks): clipped_samples diverge"
                );
                assert_eq!(
                    cyclic, reference.cyclic_windows,
                    "{label} ({pattern} chunks): cyclic_windows diverge"
                );
            }
            (Err(expected), Err(actual)) => {
                assert_eq!(
                    &actual, expected,
                    "{label} ({pattern} chunks): failure parity diverges"
                );
            }
            (reference, streaming) => panic!(
                "{label} ({pattern} chunks): reference {:?} vs streaming {:?}",
                reference.as_ref().map(|_| "Ok"),
                streaming.as_ref().map(|_| "Ok"),
            ),
        }
    }
}

#[test]
fn streaming_stretch_matches_reference_across_matrix() {
    const SOURCE_FRAMES: usize = 8_192;
    let stereo = tone_pcm(SOURCE_FRAMES, 2);
    for percent in [25u32, 50, 75, 100, 125, 175] {
        let output_frames = rate::target_for_percent(SOURCE_FRAMES as u64, 128, percent)
            .expect("rate target")
            .output_frames as usize;
        for shape in [LoopShape::None, LoopShape::Interior, LoopShape::Boundary] {
            let loop_context = loop_for_shape(shape, SOURCE_FRAMES, output_frames);
            assert_streaming_matches_reference(
                &stereo,
                2,
                output_frames,
                loop_context,
                &format!("stereo {percent}% {shape:?}"),
            );
        }
    }
    for channels in [1usize, 6] {
        let source = tone_pcm(SOURCE_FRAMES, channels);
        for percent in [50u32, 125] {
            let output_frames = rate::target_for_percent(SOURCE_FRAMES as u64, 128, percent)
                .expect("rate target")
                .output_frames as usize;
            for shape in [LoopShape::None, LoopShape::Interior] {
                let loop_context = loop_for_shape(shape, SOURCE_FRAMES, output_frames);
                assert_streaming_matches_reference(
                    &source,
                    channels,
                    output_frames,
                    loop_context,
                    &format!("{channels}-channel {percent}% {shape:?}"),
                );
            }
        }
    }
}

#[test]
fn streaming_stretch_boundary_shapes_and_error_parity() {
    let parameters = stretch::StretchParameters::for_sample_rate(8_000).unwrap();
    let window = parameters.window;
    let hop = parameters.synthesis_hop;
    let radius = parameters.search_radius;

    // Boundary output shapes: the loop-free minimum (first copy straight into
    // the terminal), one over, just under two windows, and a non-multiple.
    let source = tone_pcm(2_000, 2);
    for output_frames in [window + hop, window + hop + 1, 2 * window - 1, 2_777] {
        assert_streaming_matches_reference(
            &source,
            2,
            output_frames,
            None,
            &format!("boundary output {output_frames}"),
        );
    }
    // Impulse content at a non-multiple output (mirrors the reference
    // determinism cell) and a source at the exact minimum length.
    let mut impulse = vec![0i16; 2_000 * 2];
    impulse[1_000 * 2] = 20_000;
    impulse[1_000 * 2 + 1] = -20_000;
    assert_streaming_matches_reference(&impulse, 2, 2_731, None, "impulse 2731");
    let minimum_source = tone_pcm(window + radius, 2);
    assert_streaming_matches_reference(
        &minimum_source,
        2,
        window + hop,
        None,
        "minimum source and output",
    );

    // Validation parity: StretchState::new fails exactly like the reference.
    let assert_same_error = |source_frames: usize,
                             output_frames: usize,
                             channels: usize,
                             sample_rate: u32,
                             context: Option<stretch::LoopContext>,
                             label: &str| {
        let pcm = vec![0i16; source_frames * channels.max(1)];
        let reference =
            stretch::stretch_interleaved(&pcm, channels, sample_rate, output_frames, context)
                .expect_err(&format!("{label}: reference must fail"));
        let streaming = stretch::StretchState::new(
            source_frames,
            output_frames,
            channels,
            sample_rate,
            context,
        )
        .expect_err(&format!("{label}: streaming must fail"));
        assert_eq!(streaming, reference, "{label}: error parity diverges");
    };
    assert_same_error(2_000, 2_777, 0, 8_000, None, "zero channels");
    assert_same_error(
        window + radius - 1,
        2_777,
        2,
        8_000,
        None,
        "source too short",
    );
    assert_same_error(2_000, window + hop - 1, 2, 8_000, None, "output too short");
    assert_same_error(2_000, 2_777, 2, 0, None, "zero sample rate");
    for (field_label, context) in [
        (
            "loop source range",
            stretch::LoopContext {
                source_start: 700,
                source_end: 700,
                output_start: 100,
                output_end: 900,
            },
        ),
        (
            "loop output range",
            stretch::LoopContext {
                source_start: 100,
                source_end: 900,
                output_start: 2_800,
                output_end: 2_900,
            },
        ),
        (
            "loop source too short",
            stretch::LoopContext {
                source_start: 100,
                source_end: 100 + window - 1,
                output_start: 100,
                output_end: 900,
            },
        ),
        (
            "loop output too short",
            stretch::LoopContext {
                source_start: 100,
                source_end: 900,
                output_start: 100,
                output_end: 100 + window - 1,
            },
        ),
    ] {
        assert_same_error(2_000, 2_777, 2, 8_000, Some(context), field_label);
    }
    // Mid-run failure parity: the reference's NoCandidate cell fails the same
    // way through the streaming machine (during produce, not construction).
    let boundary_short = vec![0i16; 337 * 2];
    assert_streaming_matches_reference(&boundary_short, 2, 512, None, "NoCandidate parity");
}

#[test]
fn streaming_stretch_chunking_is_independent() {
    const SOURCE_FRAMES: usize = 8_192;
    let parameters = stretch::StretchParameters::for_sample_rate(8_000).unwrap();
    let hop = parameters.synthesis_hop;
    let window = parameters.window;

    let stereo = tone_pcm(SOURCE_FRAMES, 2);
    let mono = tone_pcm(SOURCE_FRAMES, 1);
    let cells: [(&[i16], usize, u32, LoopShape, &str); 2] = [
        (&stereo, 2, 75, LoopShape::Interior, "75% stereo interior"),
        (&mono, 1, 175, LoopShape::None, "175% mono none"),
    ];
    for (source, channels, percent, shape, label) in cells {
        let output_frames = rate::target_for_percent(SOURCE_FRAMES as u64, 128, percent)
            .expect("rate target")
            .output_frames as usize;
        let loop_context = loop_for_shape(shape, SOURCE_FRAMES, output_frames);
        let reference = reference_stretch(source, channels, output_frames, loop_context)
            .expect("chunking reference cell");
        for chunk in [1, hop - 1, hop, hop + 1, window + 17, 997, output_frames] {
            let (samples, clipped, cyclic) =
                run_stretch_state(source, channels, 8_000, output_frames, loop_context, |_| {
                    chunk
                })
                .expect("chunked run");
            assert_eq!(
                samples, reference.samples,
                "{label}: chunk {chunk} diverges"
            );
            assert_eq!(clipped, reference.clipped_samples, "{label}: chunk {chunk}");
            assert_eq!(cyclic, reference.cyclic_windows, "{label}: chunk {chunk}");
        }
        // Deterministic pseudo-random chunk sizes in 1..=window+hop.
        let mut seed = 0x2545_F491u64;
        let (samples, _, _) =
            run_stretch_state(source, channels, 8_000, output_frames, loop_context, |_| {
                seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
                (seed >> 33) as usize % (window + hop) + 1
            })
            .expect("randomized run");
        assert_eq!(samples, reference.samples, "{label}: random chunks diverge");
    }

    // Explicit small-buffer handling: below one frame is rejected (never a
    // silent stall), and only the whole-frame prefix of a misaligned buffer
    // is used; `done` fires exactly once with the full frame total.
    let output_frames = rate::target_for_percent(SOURCE_FRAMES as u64, 128, 75)
        .expect("rate target")
        .output_frames as usize;
    let reference = reference_stretch(&stereo, 2, output_frames, None).expect("reference");
    let view = stretch::SlicePcm::new(&stereo, 2).expect("view");
    let mut state =
        stretch::StretchState::new(SOURCE_FRAMES, output_frames, 2, 8_000, None).expect("state");
    let mut empty: [i16; 1] = [0];
    assert!(matches!(
        state.produce(&view, &mut empty[..1]),
        Err(stretch::StretchError::OutputTooShort {
            actual: 0,
            required: 1
        })
    ));
    let mut collected = Vec::new();
    let mut done_calls = 0usize;
    loop {
        let mut out = vec![7i16; 64 * 2 + 1]; // misaligned: one trailing sample
        let produced = state.produce(&view, &mut out).expect("misaligned produce");
        assert!(produced.frames <= 64);
        assert_eq!(out[64 * 2], 7, "trailing non-frame capacity must be unused");
        collected.extend_from_slice(&out[..produced.frames * 2]);
        if produced.done {
            done_calls += 1;
            break;
        }
    }
    assert_eq!(done_calls, 1);
    assert_eq!(collected, reference.samples);
}

#[test]
fn streaming_stretch_checkpoint_restore_reproduces_suffix() {
    const SOURCE_FRAMES: usize = 8_192;
    let parameters = stretch::StretchParameters::for_sample_rate(8_000).unwrap();
    let hop = parameters.synthesis_hop;
    let source = tone_pcm(SOURCE_FRAMES, 2);
    let output_frames = rate::target_for_percent(SOURCE_FRAMES as u64, 128, 75)
        .expect("rate target")
        .output_frames as usize;
    let loop_context = loop_for_shape(LoopShape::Interior, SOURCE_FRAMES, output_frames);
    let stretched_loop_start = loop_context.expect("interior loop").output_start;
    let reference = reference_stretch(&source, 2, output_frames, loop_context).expect("reference");
    let view = stretch::SlicePcm::new(&source, 2).expect("view");

    // Zero checkpoint: taken before any produce, reproduces the whole run.
    let mut state =
        stretch::StretchState::new(SOURCE_FRAMES, output_frames, 2, 8_000, loop_context)
            .expect("state");
    let zero = state.checkpoint().expect("zero checkpoint");
    assert_eq!(zero.resume_frame(), 0);

    // Drive the run with hop-sized chunks, capturing the first checkpoint
    // whose PREVIOUS event already sat inside the stretched loop region (so
    // the snapshot carries a cyclic previous window: resume − hop ≥ start).
    let mut loop_checkpoint = None;
    let mut full = Vec::new();
    loop {
        let mut out = vec![0i16; hop * 2];
        let produced = state.produce(&view, &mut out).expect("produce");
        full.extend_from_slice(&out[..produced.frames * 2]);
        if loop_checkpoint.is_none() {
            if let Some(checkpoint) = state.checkpoint() {
                if checkpoint.resume_frame() >= stretched_loop_start + 2 * hop {
                    loop_checkpoint = Some(checkpoint);
                }
            }
        }
        if produced.done {
            break;
        }
    }
    assert_eq!(full, reference.samples);
    assert!(
        state.checkpoint().is_none(),
        "no checkpoint once the terminal region has begun"
    );

    // Restore the zero checkpoint: identical whole run and counters.
    let mut restored = stretch::StretchState::restore(
        &zero,
        SOURCE_FRAMES,
        output_frames,
        2,
        8_000,
        loop_context,
        &view,
    )
    .expect("zero restore");
    let mut replay = Vec::new();
    loop {
        let mut out = vec![0i16; 1_024 * 2];
        let produced = restored.produce(&view, &mut out).expect("replay produce");
        replay.extend_from_slice(&out[..produced.frames * 2]);
        if produced.done {
            break;
        }
    }
    assert_eq!(replay, reference.samples);
    assert_eq!(restored.clipped_samples(), reference.clipped_samples);
    assert_eq!(restored.cyclic_windows(), reference.cyclic_windows);

    // Restore the loop-start checkpoint: byte-identical suffix + counters.
    let checkpoint = loop_checkpoint.expect("loop-start checkpoint captured");
    let resume = checkpoint.resume_frame();
    assert!(resume >= stretched_loop_start);
    let mut restored = stretch::StretchState::restore(
        &checkpoint,
        SOURCE_FRAMES,
        output_frames,
        2,
        8_000,
        loop_context,
        &view,
    )
    .expect("loop restore");
    let mut suffix = Vec::new();
    loop {
        let mut out = vec![0i16; 777 * 2];
        let produced = restored.produce(&view, &mut out).expect("suffix produce");
        suffix.extend_from_slice(&out[..produced.frames * 2]);
        if produced.done {
            break;
        }
    }
    assert_eq!(
        suffix,
        &reference.samples[resume * 2..],
        "restored suffix diverges from the uninterrupted run"
    );
    assert_eq!(restored.clipped_samples(), reference.clipped_samples);
    assert_eq!(restored.cyclic_windows(), reference.cyclic_windows);

    // Checkpoint fields are private (forgery is not constructible from
    // tests); exercise restore's validation surface instead. The checkpoint's
    // resume frame sits inside [hop, terminal): restoring the SAME checkpoint
    // against a shorter output whose terminal lies below it must be rejected.
    let short_output = resume + parameters.window - hop;
    if short_output >= parameters.window + hop {
        let result = stretch::StretchState::restore(
            &checkpoint,
            SOURCE_FRAMES,
            short_output,
            2,
            8_000,
            None,
            &view,
        );
        assert!(
            matches!(result, Err(stretch::StretchError::InvalidCheckpoint { .. })),
            "resume past the terminal must be rejected"
        );
    }
    // A cyclic previous window without a loop context is rejected.
    let result = stretch::StretchState::restore(
        &checkpoint,
        SOURCE_FRAMES,
        output_frames,
        2,
        8_000,
        None,
        &view,
    );
    assert!(matches!(
        result,
        Err(stretch::StretchError::InvalidCheckpoint { .. })
    ));
}

/// SourcePcm wrapper recording the accessed frame range per produce call.
struct InstrumentedPcm<'a> {
    inner: stretch::SlicePcm<'a>,
    low: std::cell::Cell<usize>,
    high: std::cell::Cell<usize>,
}

impl<'a> InstrumentedPcm<'a> {
    fn new(samples: &'a [i16], channels: usize) -> Self {
        Self {
            inner: stretch::SlicePcm::new(samples, channels).expect("instrumented view"),
            low: std::cell::Cell::new(usize::MAX),
            high: std::cell::Cell::new(0),
        }
    }

    fn reset(&self) {
        self.low.set(usize::MAX);
        self.high.set(0);
    }

    fn range(&self) -> Option<(usize, usize)> {
        (self.low.get() != usize::MAX).then(|| (self.low.get(), self.high.get()))
    }
}

impl stretch::SourcePcm for InstrumentedPcm<'_> {
    fn frames(&self) -> usize {
        stretch::SourcePcm::frames(&self.inner)
    }

    fn channels(&self) -> usize {
        stretch::SourcePcm::channels(&self.inner)
    }

    fn sample(&self, frame: usize, channel: usize) -> i16 {
        self.low.set(self.low.get().min(frame));
        self.high.set(self.high.get().max(frame));
        stretch::SourcePcm::sample(&self.inner, frame, channel)
    }
}

#[test]
fn streaming_stretch_source_access_is_bounded() {
    const SOURCE_FRAMES: usize = 8_192;
    const Q32_ONE: u128 = 1u128 << 32;
    let parameters = stretch::StretchParameters::for_sample_rate(8_000).unwrap();
    let (window, hop) = (parameters.window, parameters.synthesis_hop);
    let radius = parameters.search_radius;
    let source = tone_pcm(SOURCE_FRAMES, 2);

    for percent in [75u32, 175] {
        let output_frames = rate::target_for_percent(SOURCE_FRAMES as u64, 128, percent)
            .expect("rate target")
            .output_frames as usize;
        let terminal = output_frames - window;
        let phase_step = rate::round_half_up_u128(
            (hop as u128) * (SOURCE_FRAMES as u128) * Q32_ONE,
            output_frames as u128,
        )
        .expect("phase step");
        let mut main_positions = Vec::new();
        let mut position = hop;
        while position < terminal {
            main_positions.push(position);
            position += hop;
        }
        let total_events = 1 + main_positions.len() + 1;

        let view = InstrumentedPcm::new(&source, 2);
        let mut state = stretch::StretchState::new(SOURCE_FRAMES, output_frames, 2, 8_000, None)
            .expect("state");
        let mut event_index = 0usize;
        let mut done = false;
        while !done {
            view.reset();
            // One-frame capacity: at most one generation event per call.
            let mut out = [0i16; 2];
            let produced = state.produce(&view, &mut out).expect("produce");
            done = produced.done;
            let Some((low, high)) = view.range() else {
                continue; // drain-only call, no event ran
            };
            let (allowed_low, allowed_high) = if event_index == 0 {
                (0, window) // first-window copy
            } else if event_index < 1 + main_positions.len() {
                // Main event: candidate search + selected window fit
                // [nominal − radius, nominal + radius + window); the joint-SAD
                // reference window (anchored at previous + hop) extends the
                // lower bound by up to one source-hop (≤ hop·S/O + rounding).
                let phase = phase_step * event_index as u128;
                let nominal = ((phase + Q32_ONE / 2) / Q32_ONE) as usize;
                (
                    nominal.saturating_sub(radius + hop + 2),
                    nominal + radius + window,
                )
            } else {
                (SOURCE_FRAMES - window, SOURCE_FRAMES) // terminal anchor
            };
            assert!(
                low >= allowed_low && high < allowed_high,
                "{percent}% event {event_index}: accessed [{low}, {high}] outside [{allowed_low}, {allowed_high})"
            );
            event_index += 1;
        }
        assert_eq!(event_index, total_events, "{percent}%: event count");
    }
}

#[test]
fn serializer_preserves_identity_order_and_format() {
    for preview_first in [false, true] {
        let source = build_bank(preview_first, BLOCK_ALIGN - 1);
        let bank = xwb::parse_song_bank(&source).expect("source parse");
        let first_data = vec![0; 4 * BLOCK_ALIGN];
        let second_data = vec![0; 5 * BLOCK_ALIGN];
        let replacements = [
            xwb::EntryReplacement {
                data: &first_data,
                duration: 4 * SAMPLES_PER_BLOCK,
                loop_start: 32,
                loop_length: 400,
            },
            xwb::EntryReplacement {
                data: &second_data,
                duration: 5 * SAMPLES_PER_BLOCK,
                loop_start: 0,
                loop_length: 5 * SAMPLES_PER_BLOCK,
            },
        ];

        let output = xwb::serialize_song_bank(&bank, &replacements).expect("serialize");
        let mut streamed = Vec::new();
        xwb::write_song_bank(&bank, &replacements, &mut streamed).expect("stream serialize");
        assert_eq!(output, streamed);
        let reparsed = xwb::parse_song_bank(&output).expect("reparse");
        assert_eq!(reparsed.name(), bank.name());
        assert_eq!(reparsed.build_time, bank.build_time);
        for index in 0..2 {
            assert_eq!(reparsed.entries[index].name(), bank.entries[index].name());
            assert_eq!(reparsed.entries[index].format, bank.entries[index].format);
            assert_eq!(
                reparsed.entries[index].duration,
                replacements[index].duration
            );
            assert_eq!(
                reparsed.entries[index].loop_start,
                replacements[index].loop_start
            );
            assert_eq!(
                reparsed.entries[index].loop_length,
                replacements[index].loop_length
            );
            assert_eq!(reparsed.entries[index].data, replacements[index].data);
            if index > 0 {
                assert_eq!(reparsed.entries[index].data_offset % 2048, 0);
            }
        }
        assert_eq!(
            output,
            xwb::serialize_song_bank(&bank, &replacements).expect("deterministic serialize")
        );
    }
}

#[test]
fn serializer_rejects_partial_generated_payloads() {
    let source = build_bank(false, 0);
    let bank = xwb::parse_song_bank(&source).expect("source parse");
    let partial = vec![0; BLOCK_ALIGN - 1];
    let complete = vec![0; BLOCK_ALIGN];
    let replacements = [
        xwb::EntryReplacement {
            data: &partial,
            duration: SAMPLES_PER_BLOCK,
            loop_start: 0,
            loop_length: SAMPLES_PER_BLOCK,
        },
        xwb::EntryReplacement {
            data: &complete,
            duration: SAMPLES_PER_BLOCK,
            loop_start: 0,
            loop_length: SAMPLES_PER_BLOCK,
        },
    ];
    assert!(xwb::serialize_song_bank(&bank, &replacements).is_err());
}

/// Deterministic per-entry payload bytes for virtual-bank layout tests.
/// Content is irrelevant to layout equality — only lengths and positions
/// matter — but distinct per-entry patterns catch region mix-ups.
fn layout_payload(len: usize, seed: u8) -> Vec<u8> {
    (0..len)
        .map(|index| (index as u8).wrapping_mul(31).wrapping_add(seed))
        .collect()
}

/// The whole-bank oracle: `write_song_bank_streaming` with the given
/// streamed entry values and payload bytes.
fn streamed_bank_bytes(
    bank: &xwb::SongBank<'_>,
    entries: &[xwb::StreamedEntry; 2],
    payloads: &[Vec<u8>; 2],
) -> Vec<u8> {
    use std::io::Write;

    let mut output = Vec::new();
    xwb::write_song_bank_streaming(bank, entries, &mut output, |index, out| {
        out.write_all(&payloads[index])
    })
    .expect("streamed oracle bank");
    output
}

/// Reassemble `[0, virtual_size)` region by region through `resolve`,
/// requesting `chunk(call)` bytes per read (spanning reads iterate — one
/// resolve call serves at most the containing region's remainder).
fn serve_virtual_bank(
    layout: &virtual_bank::VirtualBankLayout,
    payloads: &[Vec<u8>; 2],
    mut chunk: impl FnMut(usize) -> u32,
) -> Vec<u8> {
    let mut assembled = Vec::new();
    let mut offset = 0u64;
    let mut call = 0usize;
    while offset < layout.virtual_size {
        let request = chunk(call).max(1);
        call += 1;
        let span = layout.resolve(offset, request);
        assert!(
            span.len > 0 && u64::from(span.len) <= u64::from(request),
            "span at {offset} served {} bytes for a {request}-byte request",
            span.len
        );
        let length = span.len as usize;
        match span.region {
            virtual_bank::Region::PreData { offset: block } => {
                assembled.extend_from_slice(&layout.pre_data[block..block + length]);
            }
            virtual_bank::Region::EntryData {
                entry,
                offset: within,
            } => {
                let start = usize::try_from(within).expect("entry offset fits usize");
                assembled.extend_from_slice(&payloads[entry][start..start + length]);
            }
            virtual_bank::Region::Gap => {
                assembled.resize(assembled.len() + length, 0);
            }
            virtual_bank::Region::Eof => panic!("EOF resolved inside [0, virtual_size)"),
        }
        offset += u64::from(span.len);
        assert!(call < 1_000_000, "virtual-bank serve did not terminate");
    }
    assembled
}

#[test]
fn virtual_bank_pre_data_matches_streaming_serializer() {
    for preview_first in [false, true] {
        for percent in [75u32, 125] {
            let source = build_bank(preview_first, 0);
            let bank = xwb::parse_song_bank(&source).expect("source parse");
            let layout =
                virtual_bank::plan_virtual_bank(&bank, percent, virtual_bank::StretchTarget::Main)
                    .expect("plan");
            let streamed = [layout.entries[0].streamed, layout.entries[1].streamed];
            let payloads = [
                layout_payload(streamed[0].data_len, 3),
                layout_payload(streamed[1].data_len, 5),
            ];
            let oracle = streamed_bank_bytes(&bank, &streamed, &payloads);

            // The synthesized block is exactly the serializer's pre-data
            // prefix, and the declared virtual size is the serialized length.
            assert_eq!(layout.pre_data.len(), 2_048);
            assert_eq!(layout.pre_data[..], oracle[..2_048]);
            assert_eq!(layout.virtual_size, oracle.len() as u64);
            assert_eq!(
                layout.virtual_size,
                xwb::serialized_song_bank_len(&bank, &streamed).expect("streamed length") as u64
            );
            assert_eq!(layout.entry_offsets[0], 2_048);
            assert_eq!(
                layout.entry_offsets[1],
                2_048 + round_up(streamed[0].data_len, 2_048) as u64
            );
            assert_eq!(layout.main_entry_index, usize::from(preview_first));

            // Completing the block with matching payloads yields a bank the
            // strict parser accepts, carrying the plan's stretched metadata.
            let mut completed = layout.pre_data.clone();
            completed.extend_from_slice(&payloads[0]);
            completed.resize(layout.entry_offsets[1] as usize, 0);
            completed.extend_from_slice(&payloads[1]);
            assert_eq!(completed, oracle);
            let reparsed = xwb::parse_song_bank(&completed).expect("virtual bank reparses");
            assert_eq!(reparsed.name(), bank.name());
            assert_eq!(
                reparsed.entries[layout.main_entry_index].name(),
                bank.name()
            );
            for index in 0..2 {
                assert_eq!(reparsed.entries[index].name(), bank.entries[index].name());
                assert_eq!(reparsed.entries[index].duration, streamed[index].duration);
                assert_eq!(
                    reparsed.entries[index].loop_start,
                    streamed[index].loop_start
                );
                assert_eq!(
                    reparsed.entries[index].loop_length,
                    streamed[index].loop_length
                );
                assert_eq!(reparsed.entries[index].data.len(), streamed[index].data_len);
            }
        }
    }
}

#[test]
fn virtual_bank_resolve_reconstructs_serializer_layout() {
    for preview_first in [false, true] {
        let source = build_bank(preview_first, 0);
        let bank = xwb::parse_song_bank(&source).expect("source parse");
        for percent in [75u32, 100, 125] {
            let layout =
                virtual_bank::plan_virtual_bank(&bank, percent, virtual_bank::StretchTarget::Main)
                    .expect("plan");
            let streamed = [layout.entries[0].streamed, layout.entries[1].streamed];
            let payloads = [
                layout_payload(streamed[0].data_len, 11),
                layout_payload(streamed[1].data_len, 13),
            ];
            let oracle = streamed_bank_bytes(&bank, &streamed, &payloads);
            let label = format!("preview_first={preview_first} {percent}%");

            // The engine's real shape: one 0x1000 header read, 64 KiB packets.
            let engine = serve_virtual_bank(&layout, &payloads, |call| {
                if call == 0 {
                    0x1000
                } else {
                    0x1_0000
                }
            });
            assert_eq!(engine, oracle, "{label}: engine-shaped serve diverges");

            // Arbitrary chunkings, including region-spanning requests and a
            // single whole-file request (clamped region by region).
            for chunk in [1u32, 2_047, 2_048, 4_097, u32::MAX] {
                let assembled = serve_virtual_bank(&layout, &payloads, |_| chunk);
                assert_eq!(assembled, oracle, "{label}: chunk {chunk} diverges");
            }
        }
    }
}

#[test]
fn virtual_bank_resolve_clamps_at_eof() {
    let source = build_bank(false, 0);
    let bank = xwb::parse_song_bank(&source).expect("source parse");
    let layout = virtual_bank::plan_virtual_bank(&bank, 75, virtual_bank::StretchTarget::Main)
        .expect("plan");
    let size = layout.virtual_size;

    // The stock EOF contract: reads clamp to min(len, size − offset); at or
    // past the end nothing is served.
    for offset in [size, size + 1, size + 0x1_0000] {
        let span = layout.resolve(offset, 0x1_0000);
        assert_eq!(span.region, virtual_bank::Region::Eof);
        assert_eq!(span.len, 0);
    }
    let straddling = layout.resolve(size - 1, 0x1_0000);
    assert_eq!(straddling.len, 1);
    assert!(matches!(
        straddling.region,
        virtual_bank::Region::EntryData { entry: 1, .. }
    ));

    // A zero-length request resolves to the containing region, zero bytes.
    let empty = layout.resolve(0, 0);
    assert_eq!(empty.region, virtual_bank::Region::PreData { offset: 0 });
    assert_eq!(empty.len, 0);

    // The header read spans regions: the pre-data span clamps at its
    // boundary and the continuation lands on entry-0 data.
    let header = layout.resolve(0, 0x1000);
    assert_eq!(header.region, virtual_bank::Region::PreData { offset: 0 });
    assert_eq!(header.len, 2_048);
    let continuation = layout.resolve(2_048, 0x1000 - 2_048);
    assert_eq!(
        continuation.region,
        virtual_bank::Region::EntryData {
            entry: 0,
            offset: 0
        }
    );

    // The inter-entry alignment gap is its own zero-fill region.
    let gap_start = 2_048 + layout.entries[0].streamed.data_len as u64;
    assert!(
        gap_start < layout.entry_offsets[1],
        "fixture must have a gap"
    );
    let gap = layout.resolve(gap_start, u32::MAX);
    assert_eq!(gap.region, virtual_bank::Region::Gap);
    assert_eq!(u64::from(gap.len), layout.entry_offsets[1] - gap_start);
    let entry_one = layout.resolve(layout.entry_offsets[1], u32::MAX);
    assert_eq!(
        entry_one.region,
        virtual_bank::Region::EntryData {
            entry: 1,
            offset: 0
        }
    );
}

#[test]
fn virtual_bank_plan_refusals_carry_entry_identity() {
    // The MAIN entry near the 28-bit ceiling: quadrupling its duration at
    // 25% overflows the XWB duration field; the refusal names the entry.
    // (preview_first=true puts the main entry at index 1.)
    const CEILING_BLOCKS: usize = (1 << 26) / SAMPLES_PER_BLOCK as usize + 1;
    let bytes = build_bank_with_data_lengths(true, [2 * BLOCK_ALIGN, CEILING_BLOCKS * BLOCK_ALIGN]);
    let bank = xwb::parse_song_bank(&bytes).expect("ceiling bank parses");
    assert!(matches!(
        virtual_bank::plan_virtual_bank(&bank, 25, virtual_bank::StretchTarget::Main),
        Err(virtual_bank::PlanError::EntryRate {
            index: 1,
            source: rate::RateError::DurationOutOfRange { .. },
        })
    ));
    // ...while the same bank plans fine at identity.
    assert!(virtual_bank::plan_virtual_bank(&bank, 100, virtual_bank::StretchTarget::Main).is_ok());

    // The same ceiling on the NON-MAIN entry no longer refuses: the
    // preview passthrough (step05-fix v2) never rate-plans it — its stock
    // values always fit.
    let bytes =
        build_bank_with_data_lengths(false, [2 * BLOCK_ALIGN, CEILING_BLOCKS * BLOCK_ALIGN]);
    let bank = xwb::parse_song_bank(&bytes).expect("side-ceiling bank parses");
    let layout = virtual_bank::plan_virtual_bank(&bank, 25, virtual_bank::StretchTarget::Main)
        .expect("side entry passes through");
    assert_eq!(
        layout.entries[1].streamed.duration,
        bank.entries[1].duration
    );

    // A one-frame terminal loop on the MAIN entry (entry 0, main-first)
    // collapses to an empty region at 175% (256 → 128 frames): the loop
    // refusal keeps its stub identity.
    let mut degenerate = build_bank(false, 0);
    put_u32(&mut degenerate, 164, 255);
    put_u32(&mut degenerate, 168, 1);
    let bank = xwb::parse_song_bank(&degenerate).expect("degenerate-loop bank parses");
    assert_eq!(
        virtual_bank::plan_virtual_bank(&bank, 175, virtual_bank::StretchTarget::Main).unwrap_err(),
        virtual_bank::PlanError::InvalidMappedLoop { index: 0 }
    );
}

/// A stock-shaped bank whose entries' declared durations sit INSIDE their
/// final blocks (real banks never land on block boundaries — the honest
/// fixture rule from the 2026-08-10 live HeaderSynth refusal).
fn partial_tail_bank(preview_first: bool) -> Vec<u8> {
    let main_len = 3 * BLOCK_ALIGN;
    let preview_len = 2 * BLOCK_ALIGN;
    let main_duration = 3 * SAMPLES_PER_BLOCK - 50;
    let preview_duration = 2 * SAMPLES_PER_BLOCK - 48;
    let payloads = [vec![0u8; main_len], vec![0u8; preview_len]];
    let (payloads, durations) = if preview_first {
        (
            [payloads[1].as_slice(), payloads[0].as_slice()],
            [preview_duration, main_duration],
        )
    } else {
        (
            [payloads[0].as_slice(), payloads[1].as_slice()],
            [main_duration, preview_duration],
        )
    };
    build_bank_bytes(
        preview_first,
        [format(48_000, 2), format(48_001, 2)],
        payloads,
        durations,
        [(0, durations[0]), (0, durations[1])],
    )
}

#[test]
fn identity_plan_advertises_stock_values_for_both_entries() {
    // The training-mode identity arm (design §4.5): BOTH entries pass
    // through verbatim — header values byte-equal to stock, identity rate,
    // no loop contexts — and the virtual file reassembles to the source
    // byte-for-byte.
    for preview_first in [false, true] {
        let source = partial_tail_bank(preview_first);
        let bank = xwb::parse_song_bank(&source).expect("source parse");
        let layout = virtual_bank::plan_identity_bank(&bank).expect("identity plan");
        assert_eq!(layout.main_entry_index, usize::from(preview_first));
        // Both entries verbatim ⇒ the target distinction is inert, but the
        // field must be populated coherently (== main) for consumers.
        assert_eq!(layout.target_entry_index, layout.main_entry_index);
        for entry in 0..2 {
            assert_eq!(
                layout.entries[entry].streamed.duration, bank.entries[entry].duration,
                "entry {entry} duration (preview_first {preview_first})"
            );
            assert_eq!(
                layout.entries[entry].streamed.data_len,
                bank.entries[entry].data.len()
            );
            assert_eq!(
                layout.entries[entry].streamed.loop_start,
                bank.entries[entry].loop_start
            );
            assert_eq!(
                layout.entries[entry].streamed.loop_length,
                bank.entries[entry].loop_length
            );
            assert_eq!(layout.entries[entry].rate, rate::RateRatio::IDENTITY);
            assert!(layout.entries[entry].loop_context.is_none());
        }
        // The identity layout reproduces the stock file exactly: pre-data,
        // entry offsets, gap fill, and total size.
        assert_eq!(layout.virtual_size, source.len() as u64);
        assert_eq!(layout.pre_data[..], source[..layout.pre_data.len()]);
        let payloads = [bank.entries[0].data.to_vec(), bank.entries[1].data.to_vec()];
        let assembled = serve_virtual_bank(&layout, &payloads, |_| 4_097);
        assert_eq!(assembled, source, "preview_first {preview_first}");
    }
}

#[test]
fn identity_plan_is_stock_shaped_where_plan_entry_100_is_not() {
    // The CRITICAL pin (research §5.3): `plan_entry(…, 100)` block-quantizes
    // the output, so an entry whose real duration sits inside its final
    // block advertises a DIFFERENT duration than stock — the identity arm
    // must plan through the passthrough path instead.
    let source = partial_tail_bank(false);
    let bank = xwb::parse_song_bank(&source).expect("source parse");
    let quantized = virtual_bank::plan_virtual_bank(&bank, 100, virtual_bank::StretchTarget::Main)
        .expect("rate plan at 100");
    let main = quantized.main_entry_index;
    assert_ne!(
        quantized.entries[main].streamed.duration, bank.entries[main].duration,
        "plan_entry(…, 100) must demonstrate the block-quantization hazard"
    );
    let identity = virtual_bank::plan_identity_bank(&bank).expect("identity plan");
    assert_eq!(
        identity.entries[main].streamed.duration,
        bank.entries[main].duration
    );
}

/// Field-level equality for [`xwb::StreamedEntry`] (no `PartialEq` derive on
/// the production type — test-side comparison keeps it that way).
#[track_caller]
fn assert_streamed_eq(actual: &xwb::StreamedEntry, expected: &xwb::StreamedEntry, label: &str) {
    assert_eq!(actual.data_len, expected.data_len, "{label}: data_len");
    assert_eq!(actual.duration, expected.duration, "{label}: duration");
    assert_eq!(
        actual.loop_start, expected.loop_start,
        "{label}: loop_start"
    );
    assert_eq!(
        actual.loop_length, expected.loop_length,
        "{label}: loop_length"
    );
}

/// The stock (verbatim passthrough) streamed values of one source entry.
fn stock_streamed(entry: &xwb::SongEntry<'_>) -> xwb::StreamedEntry {
    xwb::StreamedEntry {
        data_len: entry.data.len(),
        duration: entry.duration,
        loop_start: entry.loop_start,
        loop_length: entry.loop_length,
    }
}

#[test]
fn stretch_target_main_reproduces_the_shipped_plan() {
    // The step-01 regression pin (song-preview-rate design §Components 1):
    // the Main-target plan is byte-identical to the shipped gameplay plan —
    // stretched main entry (independent `plan_entry_values` recomputation),
    // verbatim side entry, serializer-oracle pre-data/offsets/size. Honest
    // fixture (durations inside the final block), both entry orders.
    for preview_first in [false, true] {
        for percent in [75u32, 125] {
            let source = partial_tail_bank(preview_first);
            let bank = xwb::parse_song_bank(&source).expect("source parse");
            let label = format!("preview_first={preview_first} {percent}%");
            let layout =
                virtual_bank::plan_virtual_bank(&bank, percent, virtual_bank::StretchTarget::Main)
                    .expect("plan");
            let main = usize::from(preview_first);
            let side = 1 - main;
            assert_eq!(layout.main_entry_index, main, "{label}: main index");
            assert_eq!(layout.target_entry_index, main, "{label}: target index");

            let expected_main = virtual_bank::plan_entry_values(
                main,
                bank.entries[main].format,
                bank.entries[main].duration,
                bank.entries[main].loop_start,
                bank.entries[main].loop_length,
                percent,
            )
            .expect("independent main plan");
            assert_streamed_eq(
                &layout.entries[main].streamed,
                &expected_main.streamed,
                &format!("{label}: main"),
            );
            assert_eq!(layout.entries[main].rate, expected_main.rate, "{label}");
            assert_eq!(
                layout.entries[main].loop_context, expected_main.loop_context,
                "{label}: main loop context"
            );

            assert_streamed_eq(
                &layout.entries[side].streamed,
                &stock_streamed(&bank.entries[side]),
                &format!("{label}: side"),
            );
            assert_eq!(
                layout.entries[side].rate,
                rate::RateRatio::IDENTITY,
                "{label}: side rate"
            );
            assert!(
                layout.entries[side].loop_context.is_none(),
                "{label}: side loop context"
            );

            // Layout surface against the serializer oracle.
            let streamed = [layout.entries[0].streamed, layout.entries[1].streamed];
            let payloads = [
                layout_payload(streamed[0].data_len, 17),
                layout_payload(streamed[1].data_len, 19),
            ];
            let oracle = streamed_bank_bytes(&bank, &streamed, &payloads);
            assert_eq!(layout.pre_data[..], oracle[..2_048], "{label}: pre-data");
            assert_eq!(layout.virtual_size, oracle.len() as u64, "{label}: size");
            assert_eq!(layout.entry_offsets[0], 2_048, "{label}: entry 0 offset");
            assert_eq!(
                layout.entry_offsets[1],
                2_048 + round_up(streamed[0].data_len, 2_048) as u64,
                "{label}: entry 1 offset"
            );
        }
    }
}

#[test]
fn stretch_target_side_inverts_the_plan() {
    // The song-preview-rate inverse plan (design §Components 1): the side
    // (`_s`) entry stretches, the main entry passes through verbatim.
    // Honest fixture, both entry orders; the completed virtual bytes
    // reparse carrying the stretched side metadata.
    for preview_first in [false, true] {
        let source = partial_tail_bank(preview_first);
        let bank = xwb::parse_song_bank(&source).expect("source parse");
        let label = format!("preview_first={preview_first}");
        let layout = virtual_bank::plan_virtual_bank(&bank, 75, virtual_bank::StretchTarget::Side)
            .expect("side plan");
        let main = usize::from(preview_first);
        let side = 1 - main;
        assert_eq!(layout.main_entry_index, main, "{label}: main index");
        assert_eq!(layout.target_entry_index, side, "{label}: target index");

        let expected_side = virtual_bank::plan_entry_values(
            side,
            bank.entries[side].format,
            bank.entries[side].duration,
            bank.entries[side].loop_start,
            bank.entries[side].loop_length,
            75,
        )
        .expect("independent side plan");
        assert_streamed_eq(
            &layout.entries[side].streamed,
            &expected_side.streamed,
            &format!("{label}: side"),
        );
        assert_eq!(layout.entries[side].rate, expected_side.rate, "{label}");
        // Stretched at a reduced rate, the honest tail must NOT equal stock.
        assert_ne!(
            layout.entries[side].streamed.duration, bank.entries[side].duration,
            "{label}: side entry must actually stretch"
        );

        assert_streamed_eq(
            &layout.entries[main].streamed,
            &stock_streamed(&bank.entries[main]),
            &format!("{label}: main"),
        );
        assert_eq!(
            layout.entries[main].rate,
            rate::RateRatio::IDENTITY,
            "{label}: main rate"
        );
        assert!(
            layout.entries[main].loop_context.is_none(),
            "{label}: main loop context"
        );

        // Serializer-aligned offsets, and the completed virtual bytes
        // round-trip through the strict parser with the stretched values.
        let streamed = [layout.entries[0].streamed, layout.entries[1].streamed];
        let payloads = [
            layout_payload(streamed[0].data_len, 23),
            layout_payload(streamed[1].data_len, 29),
        ];
        let oracle = streamed_bank_bytes(&bank, &streamed, &payloads);
        assert_eq!(layout.pre_data[..], oracle[..2_048], "{label}: pre-data");
        assert_eq!(layout.virtual_size, oracle.len() as u64, "{label}: size");
        assert_eq!(layout.entry_offsets[0], 2_048, "{label}: entry 0 offset");
        assert_eq!(
            layout.entry_offsets[1],
            2_048 + round_up(streamed[0].data_len, 2_048) as u64,
            "{label}: entry 1 offset"
        );
        let assembled = serve_virtual_bank(&layout, &payloads, |_| 4_097);
        assert_eq!(assembled, oracle, "{label}: served bytes");
        let reparsed = xwb::parse_song_bank(&assembled).expect("virtual bank reparses");
        assert_eq!(
            reparsed.entries[side].duration, expected_side.streamed.duration,
            "{label}: reparsed side duration"
        );
        assert_eq!(
            reparsed.entries[main].duration, bank.entries[main].duration,
            "{label}: reparsed main duration"
        );
    }
}

#[test]
fn stretch_target_side_maps_the_side_loop() {
    // An interior loop on the SIDE entry maps through `map_loop` (the known
    // 75% vector: 128+768 over 1024 frames → 176/1056) while the MAIN
    // entry's distinct stock loop passes through untouched.
    let side_frames = 8 * SAMPLES_PER_BLOCK; // 1024
    let main_frames = 4 * SAMPLES_PER_BLOCK;
    let payloads = [
        vec![0u8; 4 * BLOCK_ALIGN], // entry 0 = main (main-first order)
        vec![0u8; 8 * BLOCK_ALIGN], // entry 1 = side
    ];
    let source = build_bank_bytes(
        false,
        [format(48_000, 2), format(48_001, 2)],
        [&payloads[0], &payloads[1]],
        [main_frames, side_frames],
        [(64, 256), (128, 768)],
    );
    let bank = xwb::parse_song_bank(&source).expect("source parse");
    let layout = virtual_bank::plan_virtual_bank(&bank, 75, virtual_bank::StretchTarget::Side)
        .expect("side plan");
    assert_eq!(layout.target_entry_index, 1);
    assert_eq!(layout.entries[1].streamed.loop_start, 176);
    assert_eq!(layout.entries[1].streamed.loop_length, 1_056);
    assert_eq!(
        layout.entries[1].loop_context,
        Some(stretch::LoopContext {
            source_start: 128,
            source_end: 896,
            output_start: 176,
            output_end: 1_232,
        })
    );
    // The main entry's stock loop is untouched and context-free.
    assert_eq!(layout.entries[0].streamed.loop_start, 64);
    assert_eq!(layout.entries[0].streamed.loop_length, 256);
    assert!(layout.entries[0].loop_context.is_none());
}

#[test]
fn stretch_target_side_refusals_name_the_side_entry() {
    // The 28-bit XWB duration ceiling on the SIDE entry refuses with the
    // side entry's physical identity under `StretchTarget::Side`
    // (main-first order: side = index 1)...
    const CEILING_BLOCKS: usize = (1 << 26) / SAMPLES_PER_BLOCK as usize + 1;
    let bytes =
        build_bank_with_data_lengths(false, [2 * BLOCK_ALIGN, CEILING_BLOCKS * BLOCK_ALIGN]);
    let bank = xwb::parse_song_bank(&bytes).expect("ceiling bank parses");
    assert!(matches!(
        virtual_bank::plan_virtual_bank(&bank, 25, virtual_bank::StretchTarget::Side),
        Err(virtual_bank::PlanError::EntryRate {
            index: 1,
            source: rate::RateError::DurationOutOfRange { .. },
        })
    ));

    // ...while a MAIN entry at the ceiling passes through verbatim under
    // `Side` (the mirror of the shipped side-passthrough behavior).
    let bytes = build_bank_with_data_lengths(true, [2 * BLOCK_ALIGN, CEILING_BLOCKS * BLOCK_ALIGN]);
    let bank = xwb::parse_song_bank(&bytes).expect("main-ceiling bank parses");
    let layout = virtual_bank::plan_virtual_bank(&bank, 25, virtual_bank::StretchTarget::Side)
        .expect("main entry passes through");
    assert_eq!(layout.target_entry_index, 0);
    assert_eq!(
        layout.entries[1].streamed.duration,
        bank.entries[1].duration
    );
}

// ---------------------------------------------------------------------------
// Synthetic engine replay (plan Step 3 task-02): the RE-pinned XACT read
// pattern replayed against a virtual bank served by resolve + a pull-driven
// encoded feed built from the proven streaming pieces. Test-side only — the
// production producer is plan Step 4's.
// ---------------------------------------------------------------------------

/// Main-entry frames for the replay fixture: 256 blocks (35,840 encoded
/// bytes) so the slow demo rates need multiple 64 KiB packets (2 at 50%,
/// 3 at 25%) while debug runtime stays bounded.
const REPLAY_MAIN_FRAMES: usize = 32_768;
/// Preview-entry payload: 16 whole source blocks (ADPCM data is always
/// whole blocks)...
const REPLAY_PREVIEW_PAYLOAD_FRAMES: usize = 2_048;
/// ...with a stock-shaped declared duration INSIDE the final block
/// (2,000 < 16 × 128) — real banks' durations do not land on block
/// boundaries, and a block-exact preview fixture masked exactly that
/// (the 2026-08-10 live HeaderSynth refusal: the stream serializer applied
/// the generated-content whole-block rule to the passthrough preview).
const REPLAY_PREVIEW_FRAMES: usize = 2_000;

/// Synthetic full-entry-loop replay fixture (the production loop shape —
/// 25%/50% fail `NoCandidate` without it), 8 kHz stereo tone audio.
fn replay_fixture(preview_first: bool) -> Vec<u8> {
    replay_fixture_with_main_loop(preview_first, (0, REPLAY_MAIN_FRAMES as u32))
}

fn replay_fixture_with_main_loop(preview_first: bool, main_loop: (u32, u32)) -> Vec<u8> {
    let fmt = format(8_000, 2);
    let main =
        adpcm::encode_interleaved(&tone_pcm(REPLAY_MAIN_FRAMES, 2), fmt).expect("encode main");
    let preview = adpcm::encode_interleaved(&tone_pcm(REPLAY_PREVIEW_PAYLOAD_FRAMES, 2), fmt)
        .expect("encode preview");
    let preview_loop = (0, REPLAY_PREVIEW_FRAMES as u32);
    let (payloads, durations, loops) = if preview_first {
        (
            [preview.as_slice(), main.as_slice()],
            [REPLAY_PREVIEW_FRAMES as u32, REPLAY_MAIN_FRAMES as u32],
            [preview_loop, main_loop],
        )
    } else {
        (
            [main.as_slice(), preview.as_slice()],
            [REPLAY_MAIN_FRAMES as u32, REPLAY_PREVIEW_FRAMES as u32],
            [main_loop, preview_loop],
        )
    };
    build_bank_bytes(preview_first, [fmt, fmt], payloads, durations, loops)
}

/// The whole-buffer oracle: the validator's replay composition rebuilt as
/// a test helper — parse → plan → stretch the MAIN entry (decode →
/// reference stretch → whole-buffer encode) with the non-main entry passed
/// through VERBATIM (the preview passthrough) → stream write. The byte
/// authority every replay leg compares against.
fn transform_bank_oracle(source: &[u8], percent: u32) -> Vec<u8> {
    use std::io::Write;

    let bank = xwb::parse_song_bank(source).expect("oracle parse");
    let layout = virtual_bank::plan_virtual_bank(&bank, percent, virtual_bank::StretchTarget::Main)
        .expect("oracle plan");
    let streamed = [layout.entries[0].streamed, layout.entries[1].streamed];
    let mut encoded = Vec::new();
    for (index, entry) in bank.entries.iter().enumerate() {
        if index != layout.main_entry_index {
            encoded.push(entry.data.to_vec());
            continue;
        }
        let decoded = adpcm::decode_interleaved(entry.data, entry.format, entry.duration)
            .expect("oracle decode");
        let stretched = stretch::stretch_interleaved(
            &decoded,
            entry.format.channels() as usize,
            entry.format.sample_rate(),
            layout.entries[index].streamed.duration as usize,
            layout.entries[index].loop_context,
        )
        .expect("oracle stretch");
        encoded.push(
            adpcm::encode_interleaved(&stretched.samples, entry.format).expect("oracle encode"),
        );
    }
    let mut output = Vec::new();
    xwb::write_song_bank_streaming(&bank, &streamed, &mut output, |index, out| {
        out.write_all(&encoded[index])
    })
    .expect("oracle stream write");
    output
}

/// Pull-driven encoded feed for one planned entry — the production pipeline
/// composed from Step 2's proven pieces: `BlockCachePcm` (on-demand source
/// decode) → `StretchState::produce` (resumable stretch) → whole-block
/// accumulation → `encode_block`. Serves in-order encoded byte ranges;
/// bytes produced during a pass stay readable (the production ring window
/// covers the engine's look-ahead re-reads — the header read's entry-0
/// overlap), but a loop restart never reads them: it regenerates through
/// [`EncodedFeed::restore_at_block`].
struct EncodedFeed<'a> {
    view: adpcm::BlockCachePcm<'a>,
    state: stretch::StretchState,
    format: WaveFormat,
    data_len: usize,
    /// Encoded offset of `produced[0]` within the entry's stream.
    base_offset: usize,
    produced: Vec<u8>,
    /// Whole PCM samples accumulated toward the next encoded block.
    pending: Vec<i16>,
    done: bool,
    /// Capture the latest checkpoint whose resume frame is at or below this
    /// output frame (the loop-restart target).
    capture_target: Option<usize>,
    captured: Option<stretch::StretchCheckpoint>,
}

impl<'a> EncodedFeed<'a> {
    /// A verbatim passthrough feed (the non-main entry, step05-fix v2):
    /// the plan advertises the stock values and the serving layer copies
    /// the stock bytes directly — pre-filled here so `read_at` slices the
    /// source without ever touching the stretch machinery.
    fn verbatim(entry: &xwb::SongEntry<'a>, plan: &virtual_bank::EntryPlan) -> Self {
        assert_eq!(
            plan.streamed.data_len,
            entry.data.len(),
            "verbatim feed requires a passthrough plan"
        );
        let mut feed = Self::new(entry, plan, None);
        feed.produced = entry.data.to_vec();
        feed.done = true;
        feed
    }

    fn new(
        entry: &xwb::SongEntry<'a>,
        plan: &virtual_bank::EntryPlan,
        capture_target: Option<usize>,
    ) -> Self {
        let view = adpcm::BlockCachePcm::new(entry.data, entry.format, entry.duration)
            .expect("feed source view");
        let state = stretch::StretchState::new(
            entry.duration as usize,
            plan.streamed.duration as usize,
            entry.format.channels() as usize,
            entry.format.sample_rate(),
            plan.loop_context,
        )
        .expect("feed stretch state");
        let mut feed = Self {
            view,
            state,
            format: entry.format,
            data_len: plan.streamed.data_len,
            base_offset: 0,
            produced: Vec::new(),
            pending: Vec::new(),
            done: false,
            capture_target,
            captured: None,
        };
        feed.try_capture();
        feed
    }

    /// The regeneration a loop restart performs: `StretchState::restore`
    /// from the captured checkpoint, then produce-and-DISCARD the frames
    /// between the hop-aligned resume and the block-aligned target — never
    /// a replay of retained first-pass bytes.
    fn restore_at_block(
        entry: &xwb::SongEntry<'a>,
        plan: &virtual_bank::EntryPlan,
        checkpoint: &stretch::StretchCheckpoint,
        target_block: usize,
    ) -> Self {
        let view = adpcm::BlockCachePcm::new(entry.data, entry.format, entry.duration)
            .expect("restored source view");
        let mut state = stretch::StretchState::restore(
            checkpoint,
            entry.duration as usize,
            plan.streamed.duration as usize,
            entry.format.channels() as usize,
            entry.format.sample_rate(),
            plan.loop_context,
            &view,
        )
        .expect("checkpoint restore");
        let channels = entry.format.channels() as usize;
        let target_frame = target_block * entry.format.samples_per_block() as usize;
        let mut discard = target_frame
            .checked_sub(checkpoint.resume_frame())
            .expect("checkpoint resume must not pass the block-aligned target");
        while discard > 0 {
            let mut scratch = vec![0i16; discard.min(4_096) * channels];
            let produced = state.produce(&view, &mut scratch).expect("discard produce");
            assert!(produced.frames > 0, "discard produce stalled");
            discard -= produced.frames;
        }
        Self {
            view,
            state,
            format: entry.format,
            data_len: plan.streamed.data_len,
            base_offset: target_block * entry.format.block_align() as usize,
            produced: Vec::new(),
            pending: Vec::new(),
            done: false,
            capture_target: None,
            captured: None,
        }
    }

    fn try_capture(&mut self) {
        let Some(target) = self.capture_target else {
            return;
        };
        if let Some(checkpoint) = self.state.checkpoint() {
            if checkpoint.resume_frame() <= target
                && self
                    .captured
                    .as_ref()
                    .is_none_or(|held| checkpoint.resume_frame() > held.resume_frame())
            {
                self.captured = Some(checkpoint);
            }
        }
    }

    /// Generate until `end` encoded bytes exist (absolute stream offset).
    fn generate_to(&mut self, end: usize) {
        let channels = self.format.channels() as usize;
        let block_samples = self.format.samples_per_block() as usize * channels;
        while self.base_offset + self.produced.len() < end && !self.done {
            let mut out = vec![0i16; 1_024 * channels];
            let produced = self
                .state
                .produce(&self.view, &mut out)
                .expect("feed produce");
            self.pending
                .extend_from_slice(&out[..produced.frames * channels]);
            self.try_capture();
            while self.pending.len() >= block_samples {
                adpcm::encode_block(
                    &self.pending[..block_samples],
                    self.format,
                    &mut self.produced,
                )
                .expect("feed encode block");
                self.pending.drain(..block_samples);
            }
            if produced.done {
                self.done = true;
                assert!(
                    self.pending.is_empty(),
                    "stretch output is not whole blocks"
                );
                assert_eq!(
                    self.base_offset + self.produced.len(),
                    self.data_len,
                    "feed length diverges from the plan's data_len"
                );
            }
        }
    }

    /// Serve `len` encoded bytes at `offset` (in order; a read below the
    /// feed's base is a harness bug — the engine never rewinds except to
    /// the loop start, which goes through `restore_at_block`).
    fn read_at(&mut self, offset: u64, len: usize) -> &[u8] {
        let offset = usize::try_from(offset).expect("offset fits usize");
        assert!(offset >= self.base_offset, "read below the feed base");
        let end = offset + len;
        assert!(end <= self.data_len, "read past the entry stream");
        self.generate_to(end);
        &self.produced[offset - self.base_offset..end - self.base_offset]
    }
}

/// One replayed read: `(virtual offset, requested, served)`.
type ReplayRead = (u64, u32, u32);

/// Serve one read request by iterating `resolve` region by region — the
/// serving surface Step 4's read detour uses — placing every returned byte
/// at its virtual offset in `file`.
fn serve_read(
    layout: &virtual_bank::VirtualBankLayout,
    feeds: &mut [EncodedFeed<'_>; 2],
    file: &mut [u8],
    reads: &mut Vec<ReplayRead>,
    offset: u64,
    len: u32,
) -> u32 {
    let mut served = 0u32;
    while served < len {
        let position = offset + u64::from(served);
        let span = layout.resolve(position, len - served);
        if span.len == 0 {
            assert_eq!(span.region, virtual_bank::Region::Eof, "only EOF is empty");
            break;
        }
        let position = position as usize;
        let target = &mut file[position..position + span.len as usize];
        match span.region {
            virtual_bank::Region::PreData { offset: block } => {
                target.copy_from_slice(&layout.pre_data[block..block + span.len as usize]);
            }
            virtual_bank::Region::EntryData {
                entry,
                offset: within,
            } => {
                target.copy_from_slice(feeds[entry].read_at(within, span.len as usize));
            }
            virtual_bank::Region::Gap => target.fill(0),
            virtual_bank::Region::Eof => unreachable!("zero-length EOF handled above"),
        }
        served += span.len;
    }
    reads.push((offset, len, served));
    served
}

/// Replay the RE-pinned engine read pattern against the virtual bank: one
/// 0x1000 header read at offset 0 (spanning the pre-data block into
/// entry-0 data), then per streamed entry sequential block-align-rounded
/// 64 KiB packets bounded to the stream, then one defensive read past the
/// end (the stock EOF clamp serves nothing).
fn replay_engine_reads(
    layout: &virtual_bank::VirtualBankLayout,
    feeds: &mut [EncodedFeed<'_>; 2],
) -> (Vec<u8>, Vec<ReplayRead>) {
    let virtual_size = usize::try_from(layout.virtual_size).expect("virtual size fits usize");
    let mut file = vec![0u8; virtual_size];
    let mut reads = Vec::new();

    let header = serve_read(layout, feeds, &mut file, &mut reads, 0, 0x1000);
    assert_eq!(header, 0x1000, "the header read must complete in full");

    for entry in 0..2 {
        let data_len = layout.entries[entry].streamed.data_len as u64;
        let block_align = u64::from(feeds[entry].format.block_align());
        let packet = 65_536 / block_align * block_align;
        let mut cursor = 0u64;
        while cursor < data_len {
            let request = packet.min(data_len - cursor) as u32;
            let served = serve_read(
                layout,
                feeds,
                &mut file,
                &mut reads,
                layout.entry_offsets[entry] + cursor,
                request,
            );
            assert_eq!(served, request, "in-stream packet reads serve in full");
            cursor += u64::from(served);
        }
    }

    let past = serve_read(
        layout,
        feeds,
        &mut file,
        &mut reads,
        layout.virtual_size,
        0x1000,
    );
    assert_eq!(past, 0, "reads at EOF serve nothing");

    (file, reads)
}

#[test]
fn streaming_feed_bytes_match_the_oracle_payloads() {
    let source = replay_fixture(false);
    let bank = xwb::parse_song_bank(&source).expect("parse fixture");
    for percent in [50u32, 175] {
        let layout =
            virtual_bank::plan_virtual_bank(&bank, percent, virtual_bank::StretchTarget::Main)
                .expect("plan");
        let oracle = transform_bank_oracle(&source, percent);
        let oracle_bank = xwb::parse_song_bank(&oracle).expect("oracle reparses");
        for entry in 0..2 {
            let mut feed = if entry == layout.main_entry_index {
                EncodedFeed::new(&bank.entries[entry], &layout.entries[entry], None)
            } else {
                EncodedFeed::verbatim(&bank.entries[entry], &layout.entries[entry])
            };
            let data_len = layout.entries[entry].streamed.data_len;
            // Serve the whole stream in uneven in-order slices.
            let mut bytes = Vec::new();
            let mut cursor = 0usize;
            let mut step = 0usize;
            while cursor < data_len {
                let len = [977usize, 2_048, 139, 65_520][step % 4].min(data_len - cursor);
                step += 1;
                bytes.extend_from_slice(feed.read_at(cursor as u64, len));
                cursor += len;
            }
            assert_eq!(bytes.len(), data_len, "{percent}% entry {entry}: length");
            assert_eq!(
                bytes, oracle_bank.entries[entry].data,
                "{percent}% entry {entry}: feed bytes diverge from the oracle payload"
            );
        }
    }
}

#[test]
fn engine_replay_reassembles_the_oracle_bank() {
    for preview_first in [false, true] {
        let source = replay_fixture(preview_first);
        let bank = xwb::parse_song_bank(&source).expect("parse fixture");
        for percent in [25u32, 50, 100, 175] {
            let label = format!("preview_first={preview_first} {percent}%");
            let layout =
                virtual_bank::plan_virtual_bank(&bank, percent, virtual_bank::StretchTarget::Main)
                    .expect("plan");
            let oracle = transform_bank_oracle(&source, percent);
            let feed_for = |entry: usize| {
                if entry == layout.main_entry_index {
                    EncodedFeed::new(&bank.entries[entry], &layout.entries[entry], None)
                } else {
                    EncodedFeed::verbatim(&bank.entries[entry], &layout.entries[entry])
                }
            };
            let mut feeds = [feed_for(0), feed_for(1)];
            let (file, reads) = replay_engine_reads(&layout, &mut feeds);
            assert_eq!(file.len() as u64, layout.virtual_size, "{label}: size");
            assert_eq!(file, oracle, "{label}: reassembly diverges from the oracle");

            // Read-pattern fidelity: the exact header read, sequential
            // block-align-rounded packets, nothing past the virtual size.
            assert_eq!(reads[0], (0, 0x1000, 0x1000), "{label}: header read");
            let packet = (65_536 / BLOCK_ALIGN * BLOCK_ALIGN) as u64;
            let mut index = 1;
            for entry in 0..2 {
                let data_len = layout.entries[entry].streamed.data_len as u64;
                let mut cursor = 0u64;
                while cursor < data_len {
                    let (offset, requested, served) = reads[index];
                    index += 1;
                    assert_eq!(
                        offset,
                        layout.entry_offsets[entry] + cursor,
                        "{label}: packets are sequential"
                    );
                    assert_eq!(
                        u64::from(requested),
                        packet.min(data_len - cursor),
                        "{label}: block-align-rounded packet"
                    );
                    assert_eq!(served, requested, "{label}: in-stream serve");
                    assert!(
                        offset + u64::from(served) <= layout.virtual_size,
                        "{label}: read past the virtual size"
                    );
                    cursor += u64::from(served);
                }
            }
            let (offset, _, served) = reads[index];
            assert_eq!(
                (offset, served),
                (layout.virtual_size, 0),
                "{label}: EOF read"
            );
            assert_eq!(reads.len(), index + 1, "{label}: read count");

            // The reassembled bank is playable: it reparses and both
            // entries decode identically to the oracle's.
            let reparsed = xwb::parse_song_bank(&file).expect("reassembled bank reparses");
            let oracle_bank = xwb::parse_song_bank(&oracle).expect("oracle reparses");
            for entry in 0..2 {
                assert_eq!(
                    reparsed.entries[entry].duration, layout.entries[entry].streamed.duration,
                    "{label}: entry {entry} duration"
                );
                let ours = adpcm::decode_interleaved(
                    reparsed.entries[entry].data,
                    reparsed.entries[entry].format,
                    reparsed.entries[entry].duration,
                )
                .expect("reassembled entry decodes");
                let reference = adpcm::decode_interleaved(
                    oracle_bank.entries[entry].data,
                    oracle_bank.entries[entry].format,
                    oracle_bank.entries[entry].duration,
                )
                .expect("oracle entry decodes");
                assert_eq!(
                    ours, reference,
                    "{label}: entry {entry} decoded PCM diverges"
                );
            }
        }
    }
}

#[test]
fn loop_restart_reproduces_identical_bytes() {
    let spb = SAMPLES_PER_BLOCK as usize;

    // (a) The production shape: a full-entry loop's stretched start is
    // output frame 0, so the qualifying checkpoint is the zero checkpoint
    // and the discard is empty. The re-served window must byte-match the
    // first serving without reading any retained first-pass byte.
    {
        let source = replay_fixture(false);
        let bank = xwb::parse_song_bank(&source).expect("parse fixture");
        let layout = virtual_bank::plan_virtual_bank(&bank, 50, virtual_bank::StretchTarget::Main)
            .expect("plan");
        let main = layout.main_entry_index;
        let plan = &layout.entries[main];
        let loop_start_frame = plan.streamed.loop_start as usize;
        assert_eq!(loop_start_frame, 0, "full-entry loop starts at zero");
        let target_block = loop_start_frame / spb;
        let mut feed = EncodedFeed::new(&bank.entries[main], plan, Some(target_block * spb));
        let data_len = plan.streamed.data_len;
        // First pass serves the whole entry — past the stretched loop end.
        let first_pass = feed.read_at(0, data_len).to_vec();
        let checkpoint = feed.captured.expect("qualifying checkpoint captured");
        assert_eq!(checkpoint.resume_frame(), 0, "zero checkpoint, no discard");
        let mut restarted =
            EncodedFeed::restore_at_block(&bank.entries[main], plan, &checkpoint, target_block);
        let target_byte = target_block * BLOCK_ALIGN;
        let window = (data_len - target_byte).min(2 * 65_520);
        let reserved = restarted.read_at(target_byte as u64, window).to_vec();
        assert_eq!(
            reserved,
            &first_pass[target_byte..target_byte + window],
            "50% full-entry restart bytes diverge from the first serving"
        );
    }

    // (b) The discard bridge is real on an interior loop (allowed at 75% —
    // only 25%/50% require the full-entry shape): the captured checkpoint
    // is hop-aligned below the block-aligned target, so restore must
    // produce and discard the bridging frames before re-serving.
    {
        let source = replay_fixture_with_main_loop(false, (4_000, 20_000));
        let bank = xwb::parse_song_bank(&source).expect("parse fixture");
        let layout = virtual_bank::plan_virtual_bank(&bank, 75, virtual_bank::StretchTarget::Main)
            .expect("plan");
        let main = layout.main_entry_index;
        let plan = &layout.entries[main];
        let loop_start_frame = plan.streamed.loop_start as usize;
        assert!(loop_start_frame > 0, "interior loop starts past zero");
        let target_block = loop_start_frame / spb;
        let target_frame = target_block * spb;
        let mut feed = EncodedFeed::new(&bank.entries[main], plan, Some(target_frame));
        let data_len = plan.streamed.data_len;
        let first_pass = feed.read_at(0, data_len).to_vec();
        let checkpoint = feed.captured.expect("qualifying checkpoint captured");
        assert!(
            checkpoint.resume_frame() < target_frame,
            "the discard bridge must be exercised (resume {} < target {target_frame})",
            checkpoint.resume_frame()
        );
        let mut restarted =
            EncodedFeed::restore_at_block(&bank.entries[main], plan, &checkpoint, target_block);
        let target_byte = target_block * BLOCK_ALIGN;
        let window = (data_len - target_byte).min(2 * 65_520);
        let reserved = restarted.read_at(target_byte as u64, window).to_vec();
        assert_eq!(
            reserved,
            &first_pass[target_byte..target_byte + window],
            "75% interior restart bytes diverge from the first serving"
        );
    }
}

#[test]
fn exact_rate_targets_and_reference_vectors() {
    let slow = rate::target_for_percent(38_400, 128, 75).expect("75 percent target");
    assert_eq!(slow.output_blocks, 400);
    assert_eq!(slow.output_frames, 51_200);
    assert_eq!(slow.rate, rate::RateRatio::new(3, 4).unwrap());
    assert_eq!(slow.rate.q31().unwrap(), 1_610_612_736);
    assert_eq!(slow.rate.content_to_wall_ms(750).unwrap(), 1_000);
    assert_eq!(slow.rate.content_to_wall_ms(-750).unwrap(), -1_000);

    let identity = rate::target_for_percent(38_400, 128, 100).expect("100 percent target");
    assert_eq!(identity.output_frames, 38_400);
    assert_eq!(identity.rate, rate::RateRatio::IDENTITY);
    assert_eq!(identity.rate.q31().unwrap(), 1i64 << 31);

    let fast = rate::target_for_percent(38_400, 128, 125).expect("125 percent target");
    assert_eq!(fast.output_blocks, 240);
    assert_eq!(fast.output_frames, 30_720);
    assert_eq!(fast.rate, rate::RateRatio::new(5, 4).unwrap());
    assert_eq!(fast.rate.q31().unwrap(), 2_684_354_560);
    assert_eq!(fast.rate.content_to_wall_ms(1_250).unwrap(), 1_000);

    let half_up = rate::target_for_percent(192, 128, 100).expect("1.5 rounds to 2");
    assert_eq!(half_up.output_blocks, 2);
    assert_eq!(half_up.output_frames, 256);
    assert_eq!(
        rate::target_for_percent(1, 128, 125).unwrap().output_blocks,
        1
    );
}

#[test]
fn scalar_rate_domain_boundaries_and_step() {
    // The maintainer-approved scalar domain: multiples of 5 in 25..=175.
    let slowest = rate::target_for_percent(38_400, 128, 25).expect("25 percent target");
    assert_eq!(slowest.output_frames, 153_600);
    assert_eq!(slowest.rate, rate::RateRatio::new(1, 4).unwrap());
    assert_eq!(slowest.rate.q31().unwrap(), 1i64 << 29);

    let fastest = rate::target_for_percent(38_400, 128, 175).expect("175 percent target");
    // 38_400 * 100 / (128 * 175) = 171.43 blocks → half-up 171 → 21_888 frames.
    assert_eq!(fastest.output_blocks, 171);
    assert_eq!(fastest.output_frames, 21_888);
    // The 175% factor exceeds i32::MAX — the clock slot is 64-bit by design.
    let fast_q31 = fastest.rate.q31().unwrap();
    assert!(fast_q31 > i64::from(i32::MAX));
    let expected = (38_400f64 / 21_888f64 * (1u64 << 31) as f64).round() as i64;
    assert_eq!(fast_q31, expected);

    // Interior multiples of 5 are accepted across the domain.
    for percent in [30u32, 55, 95, 105, 150, 170] {
        assert!(
            rate::target_for_percent(38_400, 128, percent).is_ok(),
            "{percent}% must be accepted"
        );
    }
    // Out-of-domain: below, above, non-multiple-of-5, zero.
    for percent in [0u32, 5, 20, 24, 26, 77, 101, 176, 180, 1_000] {
        assert!(
            matches!(
                rate::target_for_percent(38_400, 128, percent),
                Err(rate::RateError::UnsupportedPercent { .. })
            ),
            "{percent}% must be rejected"
        );
    }

    // Extreme-slow reachability: near the 28-bit XWB duration ceiling, 25%
    // quadruples output frames past the field — the documented early-failure
    // leg (design req 20), never a panic or wrap.
    assert!(matches!(
        rate::target_for_percent((1 << 26) + 128, 128, 25),
        Err(rate::RateError::DurationOutOfRange { .. })
    ));
    // ...while the same source is fine at identity.
    assert!(rate::target_for_percent((1 << 26) + 128, 128, 100).is_ok());
}

#[test]
fn rate_signed_rounding_saturation_and_errors() {
    let two = rate::RateRatio::new(2, 1).unwrap();
    assert_eq!(two.content_to_wall_ms(1).unwrap(), 1);
    assert_eq!(two.content_to_wall_ms(-1).unwrap(), -1);

    let half = rate::RateRatio::new(1, 2).unwrap();
    assert_eq!(half.scale_i32(1), 1);
    assert_eq!(half.scale_i32(-1), -1);
    assert_eq!(two.scale_i32(i32::MAX), i32::MAX);
    assert_eq!(two.scale_i32(i32::MIN), i32::MIN);
    assert_eq!(
        rate::RateRatio::new(301, 384).unwrap().scale_i32(i32::MAX),
        1_683_314_004
    );

    assert!(rate::RateRatio::new(0, 1).is_err());
    assert!(rate::RateRatio::new(1, 0).is_err());
    assert!(rate::target_for_percent(1_000, 0, 75).is_err());
    assert!(rate::target_for_percent(1_000, 128, 77).is_err());
    assert!(rate::target_for_percent((1 << 28) - 1, 128, 75).is_err());
}

fn stereo_sine(sample_rate: u32, frames: usize, left_hz: f64, right_hz: f64) -> Vec<i16> {
    let mut samples = Vec::with_capacity(frames * 2);
    for frame in 0..frames {
        let time = frame as f64 / sample_rate as f64;
        samples.push((16_000.0 * (std::f64::consts::TAU * left_hz * time).sin()) as i16);
        samples.push((16_000.0 * (std::f64::consts::TAU * right_hz * time).sin()) as i16);
    }
    samples
}

fn estimate_frequency(samples: &[i16], channels: usize, channel: usize, sample_rate: u32) -> f64 {
    let mut crossings = Vec::new();
    for frame in 1..samples.len() / channels {
        let previous = f64::from(samples[(frame - 1) * channels + channel]);
        let current = f64::from(samples[frame * channels + channel]);
        if previous < 0.0 && current >= 0.0 {
            let fraction = -previous / (current - previous);
            crossings.push(frame as f64 - 1.0 + fraction);
        }
    }
    let first = crossings[1];
    let last = crossings[crossings.len() - 2];
    (crossings.len() as f64 - 3.0) * sample_rate as f64 / (last - first)
}

#[test]
fn stretch_changes_duration_without_changing_pitch() {
    const SAMPLE_RATE: u32 = 8_000;
    const SOURCE_FRAMES: usize = 8_000;
    let source = stereo_sine(SAMPLE_RATE, SOURCE_FRAMES, 250.0, 375.0);
    let source_peak = source
        .iter()
        .map(|sample| sample.unsigned_abs())
        .max()
        .unwrap();

    for percent in [75, 100, 125] {
        let target = rate::target_for_percent(SOURCE_FRAMES as u64, 128, percent).unwrap();
        let first = stretch::stretch_interleaved(
            &source,
            2,
            SAMPLE_RATE,
            target.output_frames as usize,
            None,
        )
        .expect("stretch");
        let second = stretch::stretch_interleaved(
            &source,
            2,
            SAMPLE_RATE,
            target.output_frames as usize,
            None,
        )
        .expect("repeat stretch");

        assert_eq!(first.samples, second.samples);
        assert_eq!(first.selected_source_starts, second.selected_source_starts);
        assert_eq!(first.samples.len(), target.output_frames as usize * 2);
        assert_eq!(&first.samples[..2], &source[..2]);
        assert_eq!(
            &first.samples[first.samples.len() - 2..],
            &source[source.len() - 2..]
        );
        assert!(first
            .samples
            .iter()
            .all(|sample| sample.unsigned_abs() <= source_peak));

        for (channel, expected) in [(0, 250.0), (1, 375.0)] {
            let actual = estimate_frequency(&first.samples, 2, channel, SAMPLE_RATE);
            let error = ((actual - expected) / expected).abs();
            assert!(
                error <= 0.0025,
                "{percent}% channel {channel}: {actual:.3} Hz, error {:.3}%",
                error * 100.0
            );
        }
    }
}

#[test]
fn stretch_keeps_one_path_for_stereo_relationships() {
    const SAMPLE_RATE: u32 = 8_000;
    const FRAMES: usize = 8_000;
    let target = rate::target_for_percent(FRAMES as u64, 128, 75).unwrap();

    let mono: Vec<i16> = (0..FRAMES)
        .map(|frame| {
            (12_000.0 * (std::f64::consts::TAU * 250.0 * frame as f64 / SAMPLE_RATE as f64).sin())
                as i16
        })
        .collect();
    let identical: Vec<i16> = mono.iter().flat_map(|&sample| [sample, sample]).collect();
    let identical_out = stretch::stretch_interleaved(
        &identical,
        2,
        SAMPLE_RATE,
        target.output_frames as usize,
        None,
    )
    .unwrap();
    assert!(identical_out
        .samples
        .chunks_exact(2)
        .all(|frame| frame[0] == frame[1]));

    let anti_phase: Vec<i16> = mono.iter().flat_map(|&sample| [sample, -sample]).collect();
    let anti_out = stretch::stretch_interleaved(
        &anti_phase,
        2,
        SAMPLE_RATE,
        target.output_frames as usize,
        None,
    )
    .unwrap();
    assert!(anti_out
        .samples
        .chunks_exact(2)
        .all(|frame| i32::from(frame[0]) == -i32::from(frame[1])));

    let mut asymmetric = stereo_sine(SAMPLE_RATE, FRAMES, 250.0, 410.0);
    asymmetric[4_000 * 2] = 30_000;
    asymmetric[4_000 * 2 + 1] = -30_000;
    let asymmetric_out = stretch::stretch_interleaved(
        &asymmetric,
        2,
        SAMPLE_RATE,
        target.output_frames as usize,
        None,
    )
    .unwrap();
    let left_peak = asymmetric_out
        .samples
        .chunks_exact(2)
        .enumerate()
        .max_by_key(|(_, frame)| frame[0].unsigned_abs())
        .unwrap()
        .0;
    let right_peak = asymmetric_out
        .samples
        .chunks_exact(2)
        .enumerate()
        .max_by_key(|(_, frame)| frame[1].unsigned_abs())
        .unwrap()
        .0;
    assert_eq!(left_peak, right_peak);
}

#[test]
fn stretch_boundaries_ties_and_short_inputs_are_deterministic() {
    const SAMPLE_RATE: u32 = 8_000;
    let parameters = stretch::StretchParameters::for_sample_rate(SAMPLE_RATE).unwrap();
    let frames = 2_000;
    let silence = vec![0i16; frames * 2];
    let output_frames = 2_777;
    let result =
        stretch::stretch_interleaved(&silence, 2, SAMPLE_RATE, output_frames, None).unwrap();
    assert_eq!(result.samples.len(), output_frames * 2);
    assert_eq!(result.selected_source_starts[0], 0);
    assert_eq!(
        *result.selected_source_starts.last().unwrap(),
        frames - parameters.window
    );
    assert_eq!(&result.samples[..2], &[0, 0]);
    assert_eq!(&result.samples[result.samples.len() - 2..], &[0, 0]);
    for (selected, nominal) in result
        .selected_source_starts
        .iter()
        .zip(&result.nominal_source_starts)
        .skip(1)
        .take(result.selected_source_starts.len() - 2)
    {
        assert_eq!(*selected, (*nominal).min(frames - parameters.window));
    }

    let mut impulse = silence;
    impulse[1_000 * 2] = 20_000;
    impulse[1_000 * 2 + 1] = -20_000;
    let first = stretch::stretch_interleaved(&impulse, 2, SAMPLE_RATE, 2_731, None).unwrap();
    let second = stretch::stretch_interleaved(&impulse, 2, SAMPLE_RATE, 2_731, None).unwrap();
    assert_eq!(first.samples, second.samples);

    let too_short = vec![0; (parameters.window + parameters.search_radius - 1) * 2];
    assert!(stretch::stretch_interleaved(&too_short, 2, SAMPLE_RATE, 1_000, None).is_err());
    assert!(stretch::stretch_interleaved(&[0; 3], 2, SAMPLE_RATE, 1_000, None).is_err());
    assert!(stretch::stretch_interleaved(
        &vec![0; frames * 2],
        2,
        SAMPLE_RATE,
        parameters.window + parameters.synthesis_hop - 1,
        None,
    )
    .is_err());
    assert!(stretch::candidate_precedes(10, 99, 10, 101, 100));
    assert!(!stretch::candidate_precedes(10, 101, 10, 99, 100));

    let boundary_short = vec![0; 337 * 2];
    assert!(matches!(
        stretch::stretch_interleaved(&boundary_short, 2, SAMPLE_RATE, 512, None),
        Err(stretch::StretchError::NoCandidate)
    ));

    let valid_mono = vec![0; 400];
    assert!(matches!(
        stretch::stretch_interleaved(&valid_mono, 1, SAMPLE_RATE, usize::MAX, None),
        Err(stretch::StretchError::AllocationFailed)
    ));
    let valid_stereo = vec![0; 400 * 2];
    assert!(matches!(
        stretch::stretch_interleaved(&valid_stereo, 2, SAMPLE_RATE, usize::MAX, None),
        Err(stretch::StretchError::ArithmeticOverflow)
    ));
}

#[test]
fn equal_length_stretch_is_byte_identical() {
    const SAMPLE_RATE: u32 = 8_000;
    const FRAMES: usize = 8_000;
    let source = stereo_sine(SAMPLE_RATE, FRAMES, 250.0, 375.0);
    let result = stretch::stretch_interleaved(&source, 2, SAMPLE_RATE, FRAMES, None).unwrap();
    assert_eq!(result.samples, source);
    assert_eq!(result.clipped_samples, 0);
}

#[test]
fn loop_context_uses_cyclic_windows_and_preserves_seam() {
    const SAMPLE_RATE: u32 = 8_000;
    const FRAMES: usize = 8_000;
    let mut source = vec![0i16; FRAMES * 2];
    const SOURCE_LOOP_START: usize = 1_000;
    const SOURCE_LOOP_END: usize = 7_000;
    for frame in SOURCE_LOOP_START..SOURCE_LOOP_END {
        let phase = (frame - SOURCE_LOOP_START) % 40;
        let sample = (phase as i32 * 600 - 12_000) as i16;
        source[frame * 2] = sample;
        source[frame * 2 + 1] = -sample;
    }
    let target = rate::target_for_percent(FRAMES as u64, 128, 75).unwrap();
    let output_frames = target.output_frames as usize;
    let output_loop_start = (SOURCE_LOOP_START * output_frames + FRAMES / 2) / FRAMES;
    let output_loop_end = (SOURCE_LOOP_END * output_frames + FRAMES / 2) / FRAMES;
    let context = stretch::LoopContext {
        source_start: SOURCE_LOOP_START,
        source_end: SOURCE_LOOP_END,
        output_start: output_loop_start,
        output_end: output_loop_end,
    };
    let result =
        stretch::stretch_interleaved(&source, 2, SAMPLE_RATE, output_frames, Some(context))
            .expect("looped stretch");

    assert!(result.cyclic_windows > 0);
    let source_seam = source[SOURCE_LOOP_START * 2..SOURCE_LOOP_START * 2 + 2]
        .iter()
        .zip(&source[(SOURCE_LOOP_END - 1) * 2..SOURCE_LOOP_END * 2])
        .map(|(&first, &last)| (i32::from(first) - i32::from(last)).abs())
        .max()
        .unwrap();
    let output_seam = result.loop_seam_max_delta.expect("seam metric");
    assert!(output_seam <= source_seam + 2_048);
}

// ─── Seeded stretch (training-mode O(1) seeks, design §4.5 amendment) ────
//
// A shift>0 mapping epoch in pitch-preserved mode is served by a FRESH
// stretch seeded at the half-up-mapped source position — never a slice of
// the canonical stream. Frame count is exact by construction
// (`output_frames − seek_frame`); byte-level alignment across epochs is
// deliberately unpinned. Loop context: none (seeks play linearly).

const SEEDED_SOURCE_FRAMES: usize = 6_000;

/// Drive a `SeededStretchState` to completion through a `SlicePcm`,
/// honoring a per-call chunk-size pattern (frames).
fn run_seeded_stretch_state(
    source: &[i16],
    channels: usize,
    sample_rate: u32,
    output_frames: usize,
    seek_frame: usize,
    mut chunk_frames: impl FnMut(usize) -> usize,
) -> Result<Vec<i16>, stretch::StretchError> {
    let view = stretch::SlicePcm::new(source, channels).expect("source view");
    let mut state = stretch::SeededStretchState::new(
        source.len() / channels,
        output_frames,
        channels,
        sample_rate,
        seek_frame,
        &view,
    )?;
    let mut samples = Vec::new();
    let mut call = 0usize;
    loop {
        let frames = chunk_frames(call).max(1);
        call += 1;
        let mut out = vec![0i16; frames * channels];
        let produced = state.produce(&view, &mut out)?;
        samples.extend_from_slice(&out[..produced.frames * channels]);
        if produced.done {
            break;
        }
        assert!(call < 1_000_000, "seeded streaming run did not terminate");
    }
    Ok(samples)
}

#[test]
fn seeded_reference_emits_the_exact_tail_frame_count() {
    // T1: exact `output_frames − seek` frames across seek positions
    // including block boundaries and the final block (spb = 128; every
    // non-identity plan's output is a whole-block multiple).
    let source = tone_pcm(SEEDED_SOURCE_FRAMES, 2);
    for percent in [50u32, 175] {
        let output_frames = resample_output_frames(SEEDED_SOURCE_FRAMES, percent);
        assert_eq!(output_frames % 128, 0, "plans are block-quantized");
        let seeks = [
            0usize,
            128,
            1_280,
            output_frames / 2 / 128 * 128,
            output_frames - 128,
        ];
        for seek in seeks {
            let tail = stretch::stretch_seeded_interleaved(&source, 2, 8_000, output_frames, seek)
                .expect("seeded reference");
            assert_eq!(
                tail.len(),
                (output_frames - seek) * 2,
                "{percent}% seek {seek}"
            );
        }
        // Past-the-end seeks refuse (callers never ask: the mapped serve
        // tiles silence past the content end).
        assert!(stretch::stretch_seeded_interleaved(
            &source,
            2,
            8_000,
            output_frames,
            output_frames
        )
        .is_err());
    }
}

#[test]
fn seeded_streaming_matches_the_seeded_reference() {
    // T2: the streaming form is byte-identical to the whole-buffer seeded
    // reference under any chunking.
    let source = tone_pcm(SEEDED_SOURCE_FRAMES, 2);
    for percent in [50u32, 175] {
        let output_frames = resample_output_frames(SEEDED_SOURCE_FRAMES, percent);
        for seek in [128usize, output_frames / 2 / 128 * 128, output_frames - 128] {
            let reference =
                stretch::stretch_seeded_interleaved(&source, 2, 8_000, output_frames, seek)
                    .expect("seeded reference");
            for chunk in [1usize, 137, 4_096] {
                let streamed =
                    run_seeded_stretch_state(&source, 2, 8_000, output_frames, seek, |_| chunk)
                        .expect("seeded streaming run");
                assert!(
                    streamed == reference,
                    "{percent}% seek {seek} chunk {chunk} diverges from the reference"
                );
            }
        }
    }
}

#[test]
fn seeded_runs_are_deterministic_and_start_at_the_mapped_source() {
    // T3/T4: repeat runs are identical, and a mid-song seek (no back-off)
    // opens with a straight copy from the half-up-mapped source position —
    // the fresh run's first window, mirroring the reference stretch's
    // first-window copy at 0.
    let source = tone_pcm(SEEDED_SOURCE_FRAMES, 2);
    let output_frames = resample_output_frames(SEEDED_SOURCE_FRAMES, 50);
    let seek = 2_560; // 20 blocks — deep inside, far from both ends.
    let first = stretch::stretch_seeded_interleaved(&source, 2, 8_000, output_frames, seek)
        .expect("first run");
    let second = stretch::stretch_seeded_interleaved(&source, 2, 8_000, output_frames, seek)
        .expect("second run");
    assert_eq!(first, second);

    let mapped = (seek as u128 * SEEDED_SOURCE_FRAMES as u128 + output_frames as u128 / 2)
        / output_frames as u128;
    let mapped = mapped as usize;
    assert_eq!(
        &first[..8],
        &source[mapped * 2..mapped * 2 + 8],
        "the seeded run opens at the mapped source position"
    );
}

#[test]
fn md5_known_vectors_and_incremental_updates_match() {
    assert_eq!(
        digest::md5_bytes(b"").to_hex(),
        "d41d8cd98f00b204e9800998ecf8427e"
    );
    assert_eq!(
        digest::md5_bytes(b"abc").to_hex(),
        "900150983cd24fb0d6963f7d28e17f72"
    );
    let mut incremental = digest::Md5::new();
    incremental.update(b"a");
    incremental.update(b"b");
    incremental.update(b"c");
    assert_eq!(incremental.finalize(), digest::md5_bytes(b"abc"));
}

// ─── Resampler (preserve-pitch OFF path) ─────────────────────────────────
//
// The resample map's spec, asserted independently below: output frame `i`
// reads the Q32 source position `i × round_half_up(S·2^32, O)` in the global
// segments, and `source_start·2^32 + rel × round_half_up(Lsrc·2^32, Lout)`
// inside the loop segment; samples are linear-interpolated per channel with
// half-away rounding on the fractional term.

const RESAMPLE_SOURCE_FRAMES: usize = 8_192;
const RESAMPLE_PERCENTS: [u32; 5] = [25, 50, 75, 125, 175];

fn resample_output_frames(source_frames: usize, percent: u32) -> usize {
    rate::target_for_percent(source_frames as u64, 128, percent)
        .expect("rate target")
        .output_frames as usize
}

/// Pull a `ResampleState` to completion with per-call capacities from
/// `chunk_frames` (clamped to ≥ 1 frame, mirroring `run_stretch_state`).
fn run_resample_state(
    source: &[i16],
    channels: usize,
    output_frames: usize,
    loop_context: Option<stretch::LoopContext>,
    mut chunk_frames: impl FnMut(usize) -> usize,
) -> Result<Vec<i16>, resample::ResampleError> {
    let view = stretch::SlicePcm::new(source, channels).expect("source view");
    let mut state = resample::ResampleState::new(
        source.len() / channels,
        output_frames,
        channels,
        loop_context,
    )?;
    let mut samples = Vec::new();
    let mut call = 0usize;
    loop {
        let frames = chunk_frames(call).max(1);
        call += 1;
        let mut out = vec![0i16; frames * channels];
        let produced = state.produce(&view, &mut out)?;
        samples.extend_from_slice(&out[..produced.frames * channels]);
        if produced.done {
            break;
        }
        assert!(call < 1_000_000, "resample streaming run did not terminate");
    }
    Ok(samples)
}

/// Pure sine for period measurement (unlike the multi-tone `tone_pcm`).
fn sine_pcm(frames: usize, channels: usize, period_frames: f64) -> Vec<i16> {
    let mut pcm = Vec::with_capacity(frames * channels);
    for frame in 0..frames {
        let value = (std::f64::consts::TAU * frame as f64 / period_frames).sin();
        for _ in 0..channels {
            pcm.push((12_000.0 * value) as i16);
        }
    }
    pcm
}

/// Mean distance between positive-going zero crossings of channel 0.
fn mean_zero_crossing_period(samples: &[i16], channels: usize) -> f64 {
    let mut crossings = Vec::new();
    let frames = samples.len() / channels;
    for frame in 1..frames {
        let previous = samples[(frame - 1) * channels];
        let current = samples[frame * channels];
        if previous < 0 && current >= 0 {
            crossings.push(frame);
        }
    }
    assert!(
        crossings.len() >= 8,
        "too few crossings to measure a period"
    );
    let spans = crossings.len() - 1;
    (crossings[spans] - crossings[0]) as f64 / spans as f64
}

#[test]
fn resample_reference_tracks_ratio_and_length() {
    const PERIOD: f64 = 64.0;
    for channels in [1usize, 2] {
        let source = sine_pcm(RESAMPLE_SOURCE_FRAMES, channels, PERIOD);
        for percent in RESAMPLE_PERCENTS {
            let output_frames = resample_output_frames(RESAMPLE_SOURCE_FRAMES, percent);
            let output = resample::resample_interleaved(&source, channels, output_frames, None)
                .expect("reference resample");
            assert_eq!(
                output.len(),
                output_frames * channels,
                "{percent}% ch{channels}: exact output length"
            );
            // Pitch follows the EXACT plan ratio: period scales by O/S.
            let expected = PERIOD * output_frames as f64 / RESAMPLE_SOURCE_FRAMES as f64;
            let measured = mean_zero_crossing_period(&output, channels);
            let error = (measured - expected).abs() / expected;
            assert!(
                error < 0.02,
                "{percent}% ch{channels}: period {measured:.2} vs expected {expected:.2}"
            );
        }
    }
}

#[test]
fn resample_endpoints_and_source_access_are_bounded() {
    let source = tone_pcm(RESAMPLE_SOURCE_FRAMES, 2);
    for percent in [25u32, 175] {
        let output_frames = resample_output_frames(RESAMPLE_SOURCE_FRAMES, percent);
        let output = resample::resample_interleaved(&source, 2, output_frames, None)
            .expect("reference resample");
        // Frame 0 is source frame 0 exactly (phase 0, frac 0).
        assert_eq!(&output[..2], &source[..2], "{percent}%: first frame");

        // Streaming: every access stays inside [0, frames): the clamp at the
        // final interpolation pair never reaches past the last source frame.
        let view = InstrumentedPcm::new(&source, 2);
        let mut state =
            resample::ResampleState::new(RESAMPLE_SOURCE_FRAMES, output_frames, 2, None)
                .expect("state");
        let mut out = vec![0i16; 1_024 * 2];
        loop {
            let produced = state.produce(&view, &mut out).expect("produce");
            if produced.done {
                break;
            }
        }
        let (low, high) = view.range().expect("source accessed");
        assert_eq!(low, 0, "{percent}%: low bound");
        assert!(
            high < RESAMPLE_SOURCE_FRAMES,
            "{percent}%: high bound {high}"
        );
    }
}

#[test]
fn resample_streaming_matches_reference_across_matrix() {
    let source = tone_pcm(RESAMPLE_SOURCE_FRAMES, 2);
    for percent in RESAMPLE_PERCENTS {
        let output_frames = resample_output_frames(RESAMPLE_SOURCE_FRAMES, percent);
        for shape in [LoopShape::None, LoopShape::Interior, LoopShape::Boundary] {
            let loop_context = loop_for_shape(shape, RESAMPLE_SOURCE_FRAMES, output_frames);
            let reference = resample::resample_interleaved(&source, 2, output_frames, loop_context)
                .expect("reference resample");
            for chunk in [output_frames, 512] {
                let streaming =
                    run_resample_state(&source, 2, output_frames, loop_context, |_| chunk)
                        .expect("streaming resample");
                assert_eq!(
                    streaming, reference,
                    "{percent}% {shape:?} ({chunk}-frame chunks): bytes diverge"
                );
            }
        }
    }
}

#[test]
fn resample_chunking_is_independent() {
    let source = tone_pcm(RESAMPLE_SOURCE_FRAMES, 2);
    let output_frames = resample_output_frames(RESAMPLE_SOURCE_FRAMES, 75);
    let loop_context = Some(mapped_loop(
        1_000,
        7_000,
        RESAMPLE_SOURCE_FRAMES,
        output_frames,
    ));
    let whole = run_resample_state(&source, 2, output_frames, loop_context, |_| output_frames)
        .expect("whole-buffer pull");
    for chunk in [1usize, 7, 1_024] {
        let chunked = run_resample_state(&source, 2, output_frames, loop_context, |_| chunk)
            .expect("chunked pull");
        assert_eq!(chunked, whole, "{chunk}-frame chunks diverge");
    }
    // Varying capacities inside one run.
    let varied = run_resample_state(&source, 2, output_frames, loop_context, |call| {
        [3usize, 129, 1, 511][call % 4]
    })
    .expect("varied pull");
    assert_eq!(varied, whole, "varied chunks diverge");

    // Zero capacity is a typed error (never a silent stall), matching the
    // stretch's contract; the state is still usable afterwards.
    let view = stretch::SlicePcm::new(&source, 2).expect("view");
    let mut state =
        resample::ResampleState::new(RESAMPLE_SOURCE_FRAMES, output_frames, 2, loop_context)
            .expect("state");
    let mut empty: [i16; 0] = [];
    assert!(matches!(
        state.produce(&view, &mut empty),
        Err(resample::ResampleError::OutputTooShort { .. })
    ));
    let mut out = vec![0i16; output_frames * 2];
    let produced = state.produce(&view, &mut out).expect("full pull");
    assert!(produced.done);
    assert_eq!(out[..produced.frames * 2], whole[..]);

    // Produce after completion: zero frames, still done.
    let produced = state.produce(&view, &mut out).expect("post-done pull");
    assert_eq!(
        (produced.frames, produced.done),
        (0, true),
        "post-done produce"
    );
}

#[test]
fn resample_seek_reproduces_suffix() {
    let source = tone_pcm(RESAMPLE_SOURCE_FRAMES, 2);
    let output_frames = resample_output_frames(RESAMPLE_SOURCE_FRAMES, 75);
    let loop_context = Some(mapped_loop(
        1_000,
        7_000,
        RESAMPLE_SOURCE_FRAMES,
        output_frames,
    ));
    let whole = run_resample_state(&source, 2, output_frames, loop_context, |_| output_frames)
        .expect("whole-buffer pull");
    let view = stretch::SlicePcm::new(&source, 2).expect("view");
    for target in [0usize, 1, 1_280, output_frames / 2, output_frames - 1] {
        let mut state =
            resample::ResampleState::new(RESAMPLE_SOURCE_FRAMES, output_frames, 2, loop_context)
                .expect("state");
        state.positioned_at(target);
        assert_eq!(state.position(), target, "seek target {target}");
        let mut out = vec![0i16; (output_frames - target) * 2];
        let produced = state.produce(&view, &mut out).expect("suffix pull");
        assert!(produced.done, "seek {target}: done after full suffix");
        assert_eq!(
            out[..produced.frames * 2],
            whole[target * 2..],
            "seek {target}: suffix diverges"
        );
    }
    // Seeking to (or past) the end is immediately done.
    let mut state =
        resample::ResampleState::new(RESAMPLE_SOURCE_FRAMES, output_frames, 2, loop_context)
            .expect("state");
    state.positioned_at(output_frames + 5);
    assert_eq!(state.position(), output_frames, "seek clamps to end");
    let mut out = vec![0i16; 2];
    let produced = state.produce(&view, &mut out).expect("end pull");
    assert_eq!((produced.frames, produced.done), (0, true));
}

#[test]
fn resample_loop_segment_maps_positions_exactly() {
    // Ramp source (sample == frame index): linear interpolation of a ramp
    // reproduces the position itself, so the piecewise map is assertable
    // value-by-value against independent integer math.
    const Q32: u128 = 1u128 << 32;
    let source_frames = 4_096usize;
    let source: Vec<i16> = (0..source_frames as i16).collect();
    let output_frames = resample_output_frames(source_frames, 75);
    let context = mapped_loop(512, 3_584, source_frames, output_frames);

    let output = resample::resample_interleaved(&source, 1, output_frames, Some(context))
        .expect("looped resample");

    let step_global = rate::round_half_up_u128(source_frames as u128 * Q32, output_frames as u128)
        .expect("global step");
    let loop_source_len = (context.source_end - context.source_start) as u128;
    let loop_output_len = (context.output_end - context.output_start) as u128;
    let step_loop =
        rate::round_half_up_u128(loop_source_len * Q32, loop_output_len).expect("loop step");
    let expected_position = |frame: usize| -> u128 {
        if (context.output_start..context.output_end).contains(&frame) {
            context.source_start as u128 * Q32 + (frame - context.output_start) as u128 * step_loop
        } else {
            frame as u128 * step_global
        }
    };
    for frame in 0..output_frames {
        let position = expected_position(frame);
        let base = (position / Q32) as usize;
        let frac = position % Q32;
        let s0 = i128::from(source[base.min(source_frames - 1)]);
        let s1 = i128::from(source[(base + 1).min(source_frames - 1)]);
        let expected = s0
            + rate::divide_half_away_i128((s1 - s0) * frac as i128, Q32 as i128)
                .expect("interpolation");
        assert_eq!(
            i128::from(output[frame]),
            expected,
            "frame {frame}: map diverges"
        );
    }

    // Seam alignment: the loop segment enters at exactly source_start, and
    // the engine's loop restart (output_end → output_start) is
    // source-continuous with the segment's end approaching source_end.
    assert_eq!(
        output[context.output_start] as usize, context.source_start,
        "loop entry reads source_start exactly"
    );
    let segment_last = expected_position(context.output_end - 1);
    assert!(
        (segment_last / Q32) as usize <= context.source_end - 1,
        "loop segment stays below source_end"
    );
    assert!(
        context.source_end as u128 * Q32 - segment_last <= 2 * step_loop,
        "loop segment ends within one step of source_end"
    );
}

#[test]
fn resample_validation_and_identity() {
    let source = tone_pcm(256, 2);
    // Invalid channel count / frame counts.
    assert!(matches!(
        resample::resample_interleaved(&source, 0, 256, None),
        Err(resample::ResampleError::InvalidChannelCount)
    ));
    assert!(matches!(
        resample::ResampleState::new(0, 256, 2, None),
        Err(resample::ResampleError::InvalidFrameCounts)
    ));
    assert!(matches!(
        resample::ResampleState::new(256, 0, 2, None),
        Err(resample::ResampleError::InvalidFrameCounts)
    ));
    assert!(matches!(
        resample::resample_interleaved(&[0i16; 3], 2, 4, None),
        Err(resample::ResampleError::IncompleteSourceFrame)
    ));
    // Loop-context validation (empty and out-of-range), with parity between
    // the reference and the streaming constructor.
    for context in [
        stretch::LoopContext {
            source_start: 100,
            source_end: 100,
            output_start: 10,
            output_end: 20,
        },
        stretch::LoopContext {
            source_start: 0,
            source_end: 300,
            output_start: 0,
            output_end: 20,
        },
        stretch::LoopContext {
            source_start: 0,
            source_end: 200,
            output_start: 20,
            output_end: 10,
        },
        stretch::LoopContext {
            source_start: 0,
            source_end: 200,
            output_start: 0,
            output_end: 999,
        },
    ] {
        assert!(matches!(
            resample::resample_interleaved(&source, 2, 256, Some(context)),
            Err(resample::ResampleError::InvalidLoopContext { .. })
        ));
        assert!(matches!(
            resample::ResampleState::new(256, 256, 2, Some(context)),
            Err(resample::ResampleError::InvalidLoopContext { .. })
        ));
    }
    // Identity ratio: a straight copy in both forms.
    let identity =
        resample::resample_interleaved(&source, 2, 256, None).expect("identity reference");
    assert_eq!(identity, source, "identity reference copies the source");
    let streaming = run_resample_state(&source, 2, 256, None, |_| 33).expect("identity streaming");
    assert_eq!(streaming, source, "identity streaming copies the source");
}
