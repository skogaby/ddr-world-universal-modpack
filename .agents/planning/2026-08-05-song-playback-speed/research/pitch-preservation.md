# Pitch Preservation Research

## Verdict

Pitch-preserving song speed is feasible and can be included in this feature.
The sibling `ddr-chart-tools` repository already supplies the difficult binary
format and codec layers:

```text
XWB parse -> stereo MS-ADPCM decode -> time stretch -> stereo MS-ADPCM encode
          -> XWB rebuild -> persistent cache -> streaming-file redirect
```

The missing component is the time-stretch algorithm. There is no ready Rust
implementation in either repository, but ITGmania contains long-shipped
SOLA/WSOLA-like prior art that changes tempo while preserving pitch. The safest
implementation is a small pure-Rust stretcher designed for this offline cache
pipeline, using one correlation offset for both stereo channels.

The user is the sole maintainer and code owner of both this repository and
`ddr-chart-tools`; code reuse between them is explicitly permitted and is not a
licensing blocker.

## Reusable `ddr-chart-tools` Code

The sibling repository's relevant modules are:

- `src/xwb/container.rs`: `XwbBank`, `XwbEntry`, `WaveFormat`, `parse`, and
  `write`. It parses XWB v43 streaming banks, preserves entry names/order and
  bank metadata, and rebuilds aligned metadata/wave-data segments.
- `src/xwb/adpcm/decode.rs::decode`: decodes arbitrary-channel MS-ADPCM into
  interleaved PCM, including stereo song banks.
- `src/xwb/adpcm/encode.rs::encode`: deterministic arbitrary-channel
  MS-ADPCM encoder with per-block predictor selection.
- `src/model/audio.rs::AudioBuffer` and `src/util/io.rs::LeReader`: small
  supporting data/parser types.

The complete sibling crate should not become a dependency: it would pull CLI
and Vorbis code that the hook does not need. Port and adapt the focused
container/codec modules into the song-rate service, then keep behavior aligned
through shared fixture tests or a validation script.

The current modpack's `src/services/se_bank_synth/adpcm.rs` is intentionally
mono-only for Assist Tick and cannot replace the sibling stereo codec.

## XSB Impact

Pitch preservation does not require changing XSB pitch. If the generated XWB
preserves the source bank name, entry count/order, names, and wave indices, the
existing XSB remains valid and XACT plays the stretched samples at their native
sample rate and pitch.

An XSB parser remains useful for robustly mapping the main and preview cues to
XWB entries because stock/custom files can reverse entry ordering. A strict v1
can instead transform every entry in a validated two-entry song bank, preserving
their order. That stretches both the main song and its selection preview and
avoids relying on index assumptions.

## Time-Stretch Algorithm

ITGmania's `RageSoundReader_SpeedChange` is a dependency-free, time-domain
correlation and crossfade algorithm resembling SOLA/WSOLA:

1. advance a desired source position by the requested rate;
2. search a short region for the segment most similar to the prior continuation;
3. select the best waveform-aligned source position;
4. crossfade between the old continuation and selected segment;
5. retain fractional position error for accurate long-term rate.

The local implementation uses roughly 30 ms windows and a 7.5 ms search range.
It processes channels independently, which can destabilize stereo imaging. The
Rust implementation should instead sum the match error across both channels and
apply the same selected offset to both. Joint-channel scoring remains meaningful
for anti-phase material where a mono/mid signal can collapse toward zero.

No objective quality suite exists in the local StepMania-derived code. The
initial 75%-125% range therefore requires listening tests on percussion-heavy,
sustained, quiet, and dense songs. More extreme rates can produce repetition,
transient smearing, or robotic artifacts and are not part of the first release.

## Exact Effective Rate

For source logical frame count `N` and requested rate `r`, the ideal output is
`N / r` frames. DDR's MS-ADPCM profile emits fixed 128-frame blocks, so the
target should be block-aligned:

```text
output_blocks = round(N / (128 * r))
output_frames = 128 * output_blocks
effective_rate = N / output_frames
```

The stretcher must emit exactly `output_frames`. The gameplay clock, Assist Tick,
Real Speed display, diagnostics, and saved metadata must use
`effective_rate`, not the UI percentage. This is the pitch-preserving equivalent
of using pitch-quantized cents in the earlier XSB design.

Preserve each source entry's sample rate; do not assume 44.1 kHz. Preserve bank
and entry identity, streaming flags, alignment, channel/codec profile, and entry
order. Recompute durations, loop extents, compressed lengths, and segment
offsets from the generated data.

## Runtime Cost

Pitch preservation is materially more expensive than XSB pitch patching:

- a typical 6-7 MB XWB expands to roughly 24 MB of stereo PCM;
- a 75% output is roughly 1.33 times the original duration and size;
- a straightforward implementation can peak above 100 MB while holding source,
  decoded PCM, stretched PCM, encoded entries, and final output;
- the existing quality-focused encoder evaluates seven predictors per block and
  may take seconds for a full song;
- every generated rate is approximately another full song bank on disk.

Process entries sequentially and release intermediate buffers aggressively.
Generation cannot be left until the native streaming open without risking a long
AVS-worker stall. Start it once song/rate selection is final, hold the normal
stage-loading flow until the cache entry is complete, and reuse the persistent
cache on later plays.

## Cache and Redirect

Generated banks should be immutable and keyed by:

- source XWB content digest;
- requested rate and exact output-frame targets;
- stretch algorithm/parameter version;
- codec implementation version;
- cache format version.

Publish with a temporary file plus atomic rename under
`data_mods/_cache/song_playback_speed/`. The file must never be committed or
distributed because it contains transformed game audio.

The current LayeredFS hooks can redirect AVS paths and
`avs_fs_convert_path`, which is the route XACT uses before native
`CreateFileA`. Native Windows and CrossOver must both prove that the generated
XWB path is opened. The exact acknowledgement point for committing the scaled
gameplay clock remains a focused research item.

## New Design Consequences

1. The production feature no longer needs the XSB pitch-changing path.
2. The hidden diagnostic should first use one pre-generated 75% XWB, then prove
   the central clock and score/movie policies before runtime generation is added.
3. Initial values should cover both directions: 75%, 100%, and 125%.
4. First-time playback of an uncached song/rate may extend the loading screen.
5. A configurable persistent cache limit and safe eviction policy are needed.
6. Real Speed can derive its displayed effective value from the same exact
   sample-count ratio.
7. Assist Tick must use that ratio and its fixed bank capacity must be checked
   against the slowed chart duration.

## Remaining Research

- Trace or experimentally establish which XWB duration/loop fields terminate
  the streaming voice.
- Confirm that transforming every entry preserves main/preview behavior for both
  observed entry orderings.
- Benchmark decode/stretch/encode time and peak memory on the cabinet class.
- Define objective tests for output length, rate error, discontinuities, clipping,
  stereo coherence, and deterministic output.
- Perform listening tests before expanding beyond 75%-125%.
- Prove the generated streaming XWB redirect under native Windows and CrossOver.
