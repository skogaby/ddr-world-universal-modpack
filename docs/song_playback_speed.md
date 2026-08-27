# Song Playback Speed Research

> **SUPERSEDED (2026-08-11).** This is the original pre-implementation
> feasibility investigation, kept as the historical RE record (the clock-anchor,
> option, score-containment, and movie sections informed the shipped design and
> remain accurate). The FEATURE as shipped is the **streaming** model — a
> pitch-preserved time-stretch served through detoured XACT file-IO callbacks
> with a scaled Q31 gameplay clock, no XSB pitch rewrite, no disk cache — whose
> durable RE note is `docs/xact_streaming_research.md` (see its §8 for the
> implementation-time findings, incl. the preview passthrough). Architecture
> pointers: the Song Playback Speed row in `AGENTS.md`; planning:
> `.agents/planning/2026-08-08-song-rate-streaming/`. The dated §16 below is a
> post-implementation addition, not part of the 2026-08-05 investigation.

**Date:** 2026-08-05  
**Status:** Static feasibility investigation complete; no implementation or live
rate-mod test has been performed.  
**Primary target:** `gamemdx_20260721.dll` and the shipped
`xactengine2_10.dll`. The proposed gameplay-clock anchor was also checked on
the 2026-03-24, 2026-04-21, and 2026-06-16 game builds.

## 1. Executive Summary

A song-rate mod is feasible without rewriting the chart or decoding the song's
multi-megabyte wave bank.

The lowest-risk first implementation is a **pitch-changing rate mod**:

1. Patch the selected song's small XACT sound bank (`.xsb`) so the main sound's
   authored pitch field requests the chosen playback rate.
2. Scale the game's central gameplay `music_count` by the exact effective XACT
   rate. Leave every chart timestamp unchanged.
3. Make rate-dependent consumers such as Assist Tick use the same time-domain
   conversion.
4. Suppress score submission for every non-100% play.

The old XACT runtime's authored pitch field is the intended resampling control.
Static analysis proves that the field is ingested and normalized, but its exact
effect on DDR's streaming song voice still needs live proof. The expected result
is that a slower song sounds lower and a faster song sounds higher.
Pitch-preserving rate change is also technically possible, but it is a separate
and substantially larger project: decode the streaming XWB, time-stretch stereo
PCM, re-encode MS-ADPCM, rewrite the XWB layout, cache the result, and redirect
the streaming file open.

### Feasibility verdict

| Scope | Verdict | Confidence | Main cost |
|---|---|---|---|
| Pitch-changing rate, selected before a song | Feasible | High, pending live proof | XSB patcher + one central clock patch |
| Keep notes, mines, freezes, and rendering aligned | Feasible | High | Scale the authoritative playhead, not chart data |
| Assist Tick at non-100% | Feasible, but requires changes | High | Convert content milliseconds to wall milliseconds |
| Pitch-preserving rate | Feasible in principle | Medium | Full XWB decode/stretch/re-encode/cache pipeline |
| Change rate live during a song | Not recommended for v1 | Low/medium | Cue internals, re-anchoring, and discontinuity handling |
| Network matching/BPL at non-100% | Unsupported | High | Remote cabinets retain stock rate |

## 2. Evidence Labels and Address Conventions

- **[OBS]**: observed in Ghidra disassembly, an on-disk game file, or current
  source code.
- **[INF]**: inference from observed facts; requires a live test before it is a
  production invariant.
- **[REC]**: implementation recommendation.

`gamemdx.dll` has image base `0x180000000`. Addresses are shown both as Ghidra
virtual addresses and as module-relative offsets where useful. The XACT engine
has image base `0x00400000`.

## 3. Existing Song Audio Path

DDR World plays each song through Microsoft XACT 2.10. The per-song assets are:

```text
data/sound/win/dance/<code>.xsb   small sound/cue definition, about 326 bytes
data/sound/win/dance/<code>.xwb   multi-megabyte streaming wave bank
```

This is the same XACT engine and final mixer already used by
`src/services/game_audio.rs` and the Assist Tick track. See
`docs/xact_audio_research.md` for the full engine and manager reconstruction.

### 3.1 Load, prepare, and start sequence

The relevant 2026-07-21 functions are:

| Function | Ghidra address | Relative | Role |
|---|---:|---:|---|
| `FUN_180061680` | `0x180061680` | `+0x61680` | Builds the per-song paths and submits the XSB/XWB loads through FileManager |
| `FUN_180061E20` | `0x180061E20` | `+0x61E20` | DancePlaySequence state machine; waits for the banks and prepares the main cue |
| `FUN_180063220` | `0x180063220` | `+0x63220` | MatchingDancePlaySequence message handler; starts/stops the prepared song handle |
| `se_prepare` | `0x1801AA5C0` | `+0x1AA5C0` | Prepares a cue from audio-manager bank slot 5 |
| `se_start_prepared` | `0x1801AA680` | `+0x1AA680` | Starts the prepared cue handle |

**[OBS]** In `FUN_180061E20`, the song cue is prepared and the returned handle
is stored at `DancePlaySequence + 0x150`:

```asm
18006268F  OR      R14D, -1
180062693  CMP     dword ptr [R12 + 0x150], R14D
...
1800626C8  MOV     ECX, 5              ; per-song audio-manager bank
1800626CD  CALL    0x1801AA5C0         ; se_prepare(5, cue_name)
1800626D2  MOV     dword ptr [R12 + 0x150], EAX
```

**[OBS]** Message `0x1044` starts that prepared handle:

```asm
180063288  CMP     dword ptr [RCX + 0x150], -1
18006328F  JZ      ...
...
1800632B3  MOV     ECX, dword ptr [RBX + 0x150]
1800632B9  CALL    0x1801AA680         ; se_start_prepared(handle)
```

Message `0x100E` stops the same handle through `se_stop`. This lifecycle is
important for any live-rate alternative: the cue exists before playback and is
reachable through the audio manager's handle table. The recommended v1 does
not need to depend on those internal cue pointers because it modifies the XSB
before preparation.

### 3.2 Streaming XWB routing

**[OBS]** XSB data is loaded through FileManager and AVS before
`CreateSoundBank`. The streaming XWB is ultimately opened by XACT with
`CreateFileA`, after AVS path conversion. The current LayeredFS implementation
already handles both entry points:

- `avs_fs_open` redirects ordinary AVS reads.
- `avs_fs_convert_path` redirects a virtual path before native file access.

See `src/services/avs_layeredfs/file_hooks.rs::fs_open_body` and
`fs_convert_path_body`.

The pitch-changing design modifies only the XSB, so it does not depend on
streaming-XWB redirection. Pitch-preserving time stretch does depend on it and
must prove the full XWB redirect live before implementation proceeds.

## 4. XACT Pitch Is the Candidate Song-Rate Control

### 4.1 XSB sound-entry field

DDR's XSB sound-entry prefix contains:

| Entry offset | Type | Field |
|---:|---|---|
| `+0x00` | `u8` | flags |
| `+0x01` | `u16` | category |
| `+0x03` | `u8` | volume |
| `+0x04` | `i16` | pitch in cents |
| `+0x06` | `u8` | priority |
| `+0x07` | `u16` | total sound-entry length |

The durable format record is in the sibling project's
`ddr-chart-tools/docs/xsb_format.md`. The current modpack's
`src/services/se_bank_synth/xsb.rs` writes the same sound prefix for the Assist
Tick bank and already contains the XACT CRC implementation needed after a
patch.

**[OBS]** The installed `iwtd.xsb` confirms the stock main sound is a simple,
category-4 sound with pitch zero:

```text
000000c0: ...
000000ca: 04 04 00 b4 00 00 00 13 00 00 00 00 07 00 01 f8 ...
          ^^ cat=4 ^^^^^ pitch=0
```

This exact byte offset is an example, not an implementation contract. Stock
files can reverse the main/preview sound ordering, and custom files may use
different section offsets. A patcher must parse the XSB header and cue/sound
tables rather than writing `file + 0xCE`.

### 4.2 Engine interpretation

**[OBS, decompiler + data corroboration]** XACT function `FUN_004227B0`
initializes a sound from its XSB entry. The reconstructed body reads the signed
16-bit value at `sound_entry + 4`, accepts values strictly inside
`-0x960..0x960` (approximately +/-2400 cents), divides the accepted value by the
float constant `1200.0`, and stores the normalized pitch on the runtime sound
object:

```c
pitch_cents = *(i16 *)(sound_entry + 4);
if (-0x960 < pitch_cents && pitch_cents < 0x960) {
    normalized_pitch = (float)pitch_cents / 1200.0f;
} else {
    normalized_pitch = clamped_endpoint;
}
sound->normalized_pitch = normalized_pitch;
```

This reconstruction was checked against the constants at
`xactengine2_10.dll` `0x00401C78` (`1200.0f`) and the on-disk XSB layout. The
function's Ghidra body is currently under-defined after its first instruction,
so a future implementation should repair the function boundary and preserve a
full raw-disassembly excerpt before treating internal runtime offsets as an
API. The **file-format pitch field itself** is the stable interface and does not
require any runtime-object offset.

**[INF, strong]** XACT's documented/authored pitch behavior should resample the
streaming voice, making the source advance at `2^(cents/1200)`. Phase 0 exists
specifically to confirm this on DDR's streaming XWB path; until that live test
passes, this remains the feature's primary unproven assumption.

For desired rate `r`:

```text
pitch_cents   = round(1200 * log2(r))
effective_r   = 2^(pitch_cents / 1200)
```

The gameplay clock must use `effective_r`, not the unquantized UI percentage,
so integer-cent quantization cannot accumulate audio/chart drift over a long
song.

The engine's observed pitch range is much wider than a useful training range.
A conservative v1 range is 50%-125%.

### 4.3 Why patch the XSB rather than the live cue

The live prepared cue is reachable, and the XACT cue vtable has confirmed
variable methods:

| Cue vtable offset | XACT function | Meaning |
|---:|---:|---|
| `+0x48` | `0x0040B750` | `GetVariableIndex(name)` |
| `+0x50` | `0x0040C7B0` | `SetVariable(index, float)` |
| `+0x58` | `0x0040C8A0` | `GetVariable(index, out)` |

However, a cue variable affects pitch only when the authored XGS/XSB RPC graph
maps that variable to pitch. DDR's main song sound references RPC code `0xF8`,
but the semantic target of that curve is not established. Relying on a guessed
variable name or RPC meaning would be fragile.

Directly modifying the prepared cue's internal sound object is also possible in
principle, but would depend on undocumented XACT object offsets. The XSB pitch
field is authored, validated, and version-independent as long as the shipped
XACT2 bank profile remains the same. It is the preferred control surface.

## 5. The Authoritative Gameplay Clock

### 5.1 Current calculation

The GamePlayActor update `FUN_18005CCE0` computes one raw `music_count` and then
uses it throughout the frame for chart conversion, rendering, sibling
broadcasts, and judgment.

**[OBS]** 2026-07-21 disassembly:

```asm
18005CD2B  MOV     RAX, qword ptr [rip + application_global]
18005CD32  MOV     RBX, qword ptr [RAX + 0x1268]
18005CD39  SUB     EBX, dword ptr [RCX + 0x16C] ; SOUND_OFFSET
18005CD3F  SUB     EBX, dword ptr [RCX + 0x160] ; song/start anchor
18005CD45  MOVSXD  RCX, dword ptr [RCX + 0x84]  ; player side
...
18005CD59  CALL    option/context accessor
18005CD5E  MOV     RDX, qword ptr [RAX]
18005CD61  MOV     RCX, RAX
18005CD64  CALL    qword ptr [RDX + 0x248]
18005CD6A  LEA     R14D, [RAX + RBX]             ; authoritative raw mc
```

The resulting `R14D` is then passed to the existing count update, renderer
state, and `GamePlayActor::judgeNotes`:

```asm
18005CE4B  MOV     EDX, R14D
18005CE51  CALL    0x18005EB00                  ; count/display update
...
18005CE67  MOV     R9D, R14D                    ; renderer/foot-panel update
...
18005CE7C  MOV     EDX, R14D
18005CE82  CALL    0x18005EC70                  ; judgeNotes(actor, mc)
```

This is why scaling the playhead is preferable to rewriting every chart
timestamp. Notes, Results entries, mines, freeze endpoints, tempo events, and
measure data stay in their existing **content-millisecond** domain. The central
playhead simply advances through that domain at rate `r`.

### 5.2 Proposed clock transform

**[REC]** Replace the authoritative value with:

```text
scaled_mc = round(raw_mc * effective_r)
```

Scale the complete signed value around chart time zero, including the negative
lead-in. The existing formula is approximately `wall_elapsed - sound_offset`.
For a source voice running at `r`, the source position heard after the output
latency is correspondingly `r * (wall_elapsed - latency)`, so scaling around
zero preserves the existing calibration model.

Use a precomputed fixed-point multiplier or equivalent deterministic rounding.
The result must remain monotonic for positive wall-clock motion. Do not add a
fractional accumulator that can be reset differently for P1 and P2; both actors
must derive the same content time from the same raw count.

### 5.3 Candidate AOB anchor

The following structural pattern matched exactly once on each available build:

```text
48 8B 05 ?? ?? ?? ??
48 8B 98 68 12 00 00
2B 99 6C 01 00 00
2B 99 60 01 00 00
48 63 89 84 00 00 00
48 8D 35 ?? ?? ?? ??
33 D2
48 8B 0C CE
E8 ?? ?? ?? ??
48 8B 10
48 8B C8
FF 92 48 02 00 00
44 8D 34 18
```

The match begins at the application-clock load. The candidate transform site is
`match + 0x3F`, whose expected bytes are:

```text
44 8D 34 18    LEA R14D,[RAX+RBX]
```

| Build | Pattern match | Transform site |
|---|---:|---:|
| 2026-03-24 | `+0x5D33B` | `+0x5D37A` |
| 2026-04-21 | `+0x5D3AB` | `+0x5D3EA` |
| 2026-06-16 | `+0x5CCEB` | `+0x5CD2A` |
| 2026-07-21 | `+0x5CD2B` | `+0x5CD6A` |

Wildcard rationale:

- The RIP-relative application global, module-base anchor, and `CALL rel32`
  displacements move between builds and are wildcarded.
- Actor field offsets `0x84`, `0x160`, and `0x16C`, the option vtable slot
  `0x248`, and the final `LEA R14D,[RAX+RBX]` are semantic structure and remain
  literal.

**This is a signature recommendation, not yet production code.** Runtime
installation must require exactly one match, re-check the target bytes, and
fail closed to 100% if the patch/stub cannot be installed. A live diagnostic
build should log raw and scaled counts at song start and near song end before
the feature is exposed in the UI.

## 6. Proposed XSB Rewrite

### 6.1 Safe patch algorithm

For `data/sound/win/dance/*.xsb` while a non-100% rate is latched:

1. Load the stock XSB through the original AVS functions.
2. Validate `SDBK`, XACT2 content/tool version 43, header bounds, section
   offsets, and all sound-entry lengths.
3. Resolve the main cue by its exact song-code name, not `_s`.
4. Follow its simple-cue entry to the referenced sound entry.
5. Require a simple sound with the expected main-track category/profile.
6. Write the selected signed pitch cents at sound-entry `+0x04`.
7. Recompute the XACT CRC over bytes `[0x12..EOF]` and patch header `+0x08`.
8. Save a deterministic cache entry keyed by source fingerprint, pitch cents,
   and patch-format version.
9. Redirect the AVS open to the cache file.

At 100%, return `None` from the dynamic handler so LayeredFS serves the literal
stock file. This gives the feature a true zero-data-footprint identity path.

### 6.2 Existing code to reuse

- `src/services/avs_layeredfs/file_hooks.rs`: existing AVS open/lstat/convert
  detours. Extend their replacement decision; do not install another detour.
- `src/services/avs_layeredfs/cache_hasher.rs`: deterministic cache
  invalidation.
- `src/services/avs_layeredfs/xml_merger.rs::load_bytes_from_avs_path`: pattern
  for reading game data through the unhooked AVS functions.
- `src/services/se_bank_synth/xsb.rs`: CRC table and CRC algorithm already
  validated against this XACT engine.

The song patcher should live in its own small module rather than expanding the
Assist Tick writer into a general parser. The writer intentionally emits one
fixed SE profile; a stock-XSB parser has different invariants and failure modes.

## 7. Why Chart Rewriting Is Not Recommended

An alternative is to leave `music_count` at wall-clock speed and divide every
chart timestamp by `r`. This appears simple but is not localized.

To remain coherent, it would need to transform:

- every `GameNote.music_count`;
- Results-vector timestamps;
- freeze heads/tails and judged cursors;
- tempo-map normalized milliseconds and stop lengths;
- measure/guideline timing;
- mine sidecars and injected note records;
- chart-end and sequence timing data;
- any sibling actor that consumes the `0x1045` music-count broadcast.

Missing one consumer creates a chart that is visually aligned in one subsystem
and wrong in another. Scaling the authoritative playhead leaves the chart's
internal content-time relationships intact and changes one source of time.

## 8. Integration Impact

### 8.1 Systems that should compose automatically

Provided the transform occurs before `FUN_18005EB00` and `judgeNotes`, these
systems remain in the same chart-content time domain:

- native note judgment;
- native arrow/receptor rendering;
- Autoplay (`src/mods/autoplay.rs`);
- note-type judgment and mine interval crossing
  (`src/mods/note_types_expansion/mines.rs`);
- mine rendering, once the ArrowRenderer's music-count copy is confirmed to
  receive the transformed value;
- freezes, shocks, and tempo changes;
- the `0x1045` sibling broadcast issued by the per-frame count update.

These are static expectations. The live matrix must include BPM changes,
stops, freezes, shocks, and injected mines.

### 8.2 Assist Tick is not automatically compatible

Assist Tick's immortal `astk` bank is a separate XACT voice. Its waveform plays
at normal 44.1 kHz and is rewritten between songs, independent of the selected
song's XSB. Scaling the game's `music_count` while leaving the tick waveform
unchanged causes accumulating drift.

Current timing in `src/mods/assist_tick.rs` is based on:

```text
content_ms = t_i + JUDGMENT_TIMING - SOUND_OFFSET - m0
commit_skip = mc_now - m0
```

For a normal-speed tick voice and a scaled content clock, the rate-aware form is
approximately:

```text
tick_position_wall_ms = (t_i + JUDGMENT_TIMING - m0_scaled) / r - SOUND_OFFSET
commit_skip_wall_ms    = (mc_now_scaled - m0_scaled) / r
```

**[INF]** `SOUND_OFFSET` remains outside the division because it represents
output-chain latency in wall milliseconds, while chart timestamps and the
scaled `m0` are content milliseconds. This derivation must be confirmed with
the same amplified ear-test technique used for Assist Tick's JUDGMENT TIMING
sign, and at both the beginning and end of a long song.

An alternative is to play the tick bank at the same XACT pitch. That is less
attractive because the bank is registered once per process and its authored
pitch is fixed at creation; supporting arbitrary song-to-song rates would need
multiple immortal banks or fragile live-cue manipulation.

### 8.3 Timing windows and displayed errors

With central playhead scaling, native judgment thresholds stay expressed in
content milliseconds. At 75%, a 22.5 ms content window lasts 30 ms in wall
time. This is a coherent rate-mod model and is useful for training, but it is a
product decision rather than an unavoidable technical result.

If stock **wall-time** judgment strictness is desired, judgment thresholds must
also be multiplied by `r`. That is not recommended for v1; changing both the
song and the timing-window policy makes early live results harder to interpret.

Power User Statistics currently reports the game's content-domain error. At a
non-100% rate:

```text
wall_error_ms = content_error_ms / r
```

The UI/CSV should either retain and label content milliseconds or deliberately
convert to wall milliseconds. Silent mixing of the two domains is unacceptable.

The Real Speed display should eventually multiply chart BPM by `r`, so its
label describes the effective playback rather than only the authored chart.

### 8.4 Background movies

Movie-backed backgrounds use a separate DirectShow graph. Changing the XACT
song rate does not change the movie graph's clock, so native Windows movie
playback will drift from the song.

The safest v1 behavior is to suppress background movies at non-100%, leaving
the static background. The existing CrossOver workaround already proves this
is playable by faking `DShowPlayer::BuildGraph` success.

`src/mods/non_native_os_support.rs` already owns the sole detour on
`movie_build_graph`. A rate mod must not install a second detour. If conditional
movie suppression becomes part of v1, promote that target to a shared service
with an explicit policy:

```text
Wine workaround active OR non-100% song rate -> fake opened, no graph
otherwise                                -> call original BuildGraph
```

A later native-only enhancement could investigate DirectShow
`IMediaSeeking::SetRate`, but it is not required to prove the core feature.

AFP/BM2D animated backgrounds are a separate path. Some sibling actors receive
the scaled `0x1045` broadcast, but complete animation-rate coverage has not been
proven. Treat visual animation synchronization as a live-test item, not a
guaranteed consequence of the central clock patch.

### 8.5 Score integrity

Every non-100% play must be treated as assisted/modified and must not upload a
score, including rates above 100%.

**[REC]** Extend `src/services/score_guard.rs` with a per-song shared-rate taint:

- latch it from the **effective applied rate**, not the editable UI value;
- suppress both participating sides whenever `effective_rate != 1.0`;
- include it in `is_stage_suppressed`;
- preserve it across Quick Restart of the same song;
- let the existing suppression path mark session taint and sanitize the logout
  save;
- refuse to apply a non-100% rate when `score_guard::is_available()` is false.

The final point should match Autoplay's fail-closed stance. A partial state in
which the audio changed but score suppression did not is not acceptable. If the
XSB patch or clock patch fails, the only safe user-visible result is stock 100%.
If the system cannot prove whether a partial patch was consumed, conservatively
taint the stage.

### 8.6 Versus, doubles, and network play

There is one song audio stream, so P1 and P2 cannot use independent rates.

A player-facing option needs an explicit policy:

- solo/doubles: use the participating player's value;
- local versus: require both values to agree, or force 100%;
- network matching/BPL: force 100%.

P1-wins conflict resolution is technically easy but surprising and can silently
change P2's training setting. Requiring agreement is safer.

MatchingDancePlaySequence synchronizes start clocks with another cabinet, but
the remote cabinet will continue at stock audio and chart rate. Non-100% must
therefore be disabled in all networked modes unless the protocol is explicitly
extended on every participant, which is outside this feature's scope.

## 9. Option and Latch Design

### 9.1 UI options

Two viable UI shapes exist:

| Shape | Benefit | Cost |
|---|---|---|
| Cabinet-global enum in the mod overlay | Simplest shared-rate semantics and earliest availability | Does not follow the player's card |
| Per-player custom option | Natural training UX and profile persistence | Requires active-side and versus conflict resolution before bank load |

For a proof-of-concept, use a cabinet-global `75% / 100%` value. For a polished
solo-training feature, use a persisted custom option with a strict shared-rate
resolution policy.

Useful v1 presets:

```text
50%, 60%, 70%, 75%, 80%, 90%, 100%, 110%, 120%, 125%
```

The first live build should expose only `75% / 100%` to minimize the diagnosis
surface.

### 9.2 Latch point

`scene_manager` invokes callbacks before calling the constructor for the next
scene. `scene::SONG_TO_STAGE_INTERSTITIAL` (scene 26) is therefore the best
existing point to resolve and latch a song's shared rate after song/options
selection but before stage construction.

Network custom-option loads are applied at SONG_SELECT entry, before this latch.
The XSB rewrite handler should still snapshot the same generation/rate on its
first qualifying file request so XSB generation cannot observe a later UI
change.

Required lifecycle handling:

- Quick Restart keeps the same song/rate generation.
- A genuinely new song gets a new generation and cache key.
- Attract/demo loads always use 100% unless explicitly supported later.
- Course modes may load additional banks without returning through normal song
  selection and need a dedicated live test.

## 10. Pitch-Preserving Alternative

Pitch-preserving speed change cannot be obtained from XACT's static pitch field.
It requires changing the audio samples while playing the resulting bank at
normal pitch.

### 10.1 Required pipeline

1. Load and parse the selected streaming XWB.
2. Decode both stereo MS-ADPCM entries (main and preview) to PCM.
3. Apply a pitch-preserving time-stretch algorithm such as WSOLA or a phase
   vocoder.
4. Re-encode stereo MS-ADPCM.
5. Rewrite entry lengths, offsets, durations, alignment, and wave-data segment
   metadata.
6. Save a source-fingerprint + rate keyed cache file.
7. Redirect `avs_fs_convert_path` so XACT's subsequent `CreateFileA` opens the
   cache.
8. Scale the gameplay clock exactly as in the pitch-changing design.

The sibling `ddr-chart-tools` repository already has an XWB parser/writer and
MS-ADPCM decoder/encoder. It does not currently provide the time-stretch stage.
Porting those modules plus a high-quality stretch implementation is much more
code, CPU, cache space, and licensing surface than an XSB pitch patch.

### 10.2 Operational costs

- A speed variant is approximately another full song XWB on disk.
- Generating it synchronously at file open may lengthen the stage loading
  screen by seconds.
- Background pre-generation on song confirmation is preferable but requires a
  cancellation/generation protocol as users change selections.
- Cache invalidation must include the source XWB, rate, codec implementation,
  and stretch algorithm version.
- The native streaming-XWB redirect must be proven under Windows and
  CrossOver before relying on it.

**[REC]** Do not combine pitch preservation with the first rate-mod prototype.
Prove the XSB pitch and central clock model first; that work remains useful if a
time-stretch backend is added later.

## 11. Rejected or Deferred Approaches

### Rewrite every chart timestamp

Rejected for v1 because too many independently stored timestamps and timing
maps must be transformed consistently. The central playhead is a smaller and
safer control point.

### Set an XACT cue variable

Deferred because the only known song RPC (`0xF8`) has no established variable
name or pitch mapping. The methods exist, but authored behavior is unknown.

### Write an internal runtime-sound pitch field

Deferred because it depends on undocumented XACT object layout. It may be useful
for a disposable live spike, but not as the shipped mechanism.

### Hook DirectSound frequency directly

Deferred because it requires identifying the song's underlying DirectSound
voice inside XACT/spice2x, adds a lower-level hook, and still changes pitch.
The authored XSB field already requests the same class of operation safely.

### Change rate during gameplay

Deferred because it creates a discontinuity between the song voice and the
content clock and requires a precisely shared re-anchor. A per-song latch is
both simpler and better aligned with the game's options lifecycle.

## 12. Recommended Implementation Phases

### Phase 0: diagnostic proof

- Hard-code one development-only rate: 75% (`pitch ~= -498 cents`).
- Patch one selected song's XSB cache.
- Install the central clock transform with raw/scaled diagnostic logging.
- Require score suppression before activating.
- Suppress movies.
- Keep the feature out of normal configuration/UI.

Live proof criteria:

- the song audibly plays slower/lower;
- first notes, last notes, and song end remain aligned;
- no increasing drift on a long chart;
- native misses/judgments occur at the audible cues;
- the scene exits naturally when the slowed cue and chart finish;
- 100% restores literal stock behavior after restart.

### Phase 1: safe training mod

- Add `75% / 100%` UI selection.
- Make Assist Tick rate-aware.
- Add shared-rate score taint for both sides.
- Support solo and doubles.
- Force 100% in versus, courses, demos, and network modes until each is proven.
- Keep background movies disabled at non-100%.

### Phase 2: broaden and polish

- Add the full preset list and persistence.
- Support agreed-rate local versus.
- Decide content-ms vs wall-ms presentation for statistics.
- Update Real Speed to include song rate.
- Test and, if worthwhile, synchronize native movie playback.
- Evaluate pitch-preserving cached XWB generation as a separate feature.

## 13. Validation Matrix

### 13.1 Offline tests

- XSB parser rejects malformed headers, counts, offsets, and entry lengths.
- Main-cue resolution works with both observed sound orderings.
- Only the main sound pitch changes; preview sound remains unchanged.
- CRC output matches stock and the existing Assist Tick implementation.
- 100% produces no replacement file and no byte changes.
- Cache key changes with source content, cents, and format version.
- Rate conversion round-trips `percentage -> cents -> effective_r`.
- Clock scaling covers negative lead-in, zero, long songs, overflow, and
  monotonic fractional rates.
- Both player actors produce the same scaled content time.
- Score-guard tests cover stage suppression, Quick Restart, subsequent 100%
  stages, and logout sanitization.
- Assist Tick host validation checks beginning/middle/end sample positions at
  75%, 100%, and 125%.

### 13.2 Live tests

- Solo P1, solo P2, and doubles.
- Long constant-BPM song for drift.
- BPM changes, stops, freezes, shocks, and mines.
- Quick Restart, Quick Fail, natural finish, and exit during lead-in.
- Consecutive songs changing `75% -> 100% -> 75%`.
- Assist Tick at the first playable row and near song end.
- Timing Offsets and JUDGMENT TIMING at non-default values.
- Combined Playfield Styling, Player Perspective, and Power User Statistics.
- Movie-backed song under native Windows and CrossOver.
- Backend capture proving no non-100% stage result reaches the server and the
  sanitized logout still persists profile/options.
- Failure injection: bad XSB, missing score hook, missing clock signature, and
  unwritable cache all result in stock 100% or a conservatively suppressed
  stage, never a partially trusted score.

Build readiness remains the repository standard:

```bash
cargo check --target x86_64-pc-windows-msvc
cargo fmt
./build.sh
```

## 14. Open Questions

1. Does the XACT-authored pitch field behave identically for every stock and
   custom streaming song bank under native Windows and CrossOver?
2. Does scene 26 always precede the first per-song XSB open in normal, Quick
   Restart, course, and network flows? Source ordering supports it, but logs
   must prove it.
3. Does the parent sequence finish exclusively from scaled child/chart state,
   or is there an unscaled duration timer on an edge mode?
4. Should judgment windows scale in wall time with the song, or retain stock
   wall-time strictness?
5. Should local versus require equal rates or force 100% unconditionally?
6. Which AFP/BM2D background animations consume the scaled `0x1045` count, and
   which remain frame-delta driven?
7. Is native DirectShow `SetRate` worth supporting, or is static-background
   training mode preferable?
8. Is pitch preservation important enough to justify the XWB cache pipeline?

## 15. Recommended Decision

Proceed with a narrow, score-safe, pitch-changing prototype:

```text
Rates:        75% and 100%
Modes:        solo + doubles
Audio:        patch main XSB pitch, leave XWB unchanged
Chart:        leave timestamps unchanged; scale authoritative music_count
Assist Tick:  rate-aware wall-time conversion
Movies:       suppress at 75%
Scores:       suppress both sides at every non-100% rate, fail closed
```

This prototype tests the two load-bearing assumptions with the least new code:
XACT's authored pitch controls the streaming song rate, and one central scaled
playhead keeps the game's chart pipeline aligned. If both pass a long-song live
test, the remaining work is integration and UX rather than unknown core
feasibility.

## 16. Rate-aware Real Speed (2026-08-11, post-implementation RE)

Ghidra work on `gamemdx_20260721.dll` (byte-spot-checked on 20260616) done while
implementing streaming-design req 33; the full session record is
`.agents/planning/2026-08-08-song-rate-streaming/implementation/step06-task-02-real-speed-effective-rate/context.md`.
This section is the durable copy.

### The Option scroll-speed cluster (`ddr::player::Option`)

Reached per side via the derived `player_option_table` chain
(`*(*(table + side*8)) + 0xE0` — the same object Assist Tick reads
`timing_music` from). Vtable base `0x180387978` on 20260721. Layout stable
since 20250805 (bulk-hack RE + this session):

| Offset | Type | Meaning |
|---|---|---|
| `+0x8` | int | Speed TYPE: **0 = Real Speed mode** (target-derived), **1 = fixed multiplier** |
| `+0xC` | int | Fixed multiplier ×100 (`SetHispeed`: clamp [25,800], snap 5) |
| `+0x10` | int | DERIVED multiplier ×100 — `SetScrollSpeed` (vtable+0xD0, `FUN_1801e0d90`) output: `clamp(trunc(target·100 / divisor), 25, 800)`; divisor = Max BPM (+0x90, cap-sentineled) stock, Core BPM (+0x88) with the Real Speed Fix's R26 cave. Clamp constants `DAT_18035a740/744` = 25/800 |
| `+0x14` | int | Real-speed TARGET |
| `+0x80/+0x88/+0x90` | double×3 | Min / Core / Max BPM — `SetBPMs(min,core,max)` (vtable+0xB8, `FUN_1801e0d20`) writes them **and immediately re-calls SetScrollSpeed**, so `+0x10` is re-derived at every chart load |
| vtable`+0x208` | fn | Speed-type getter (`FUN_1800f4460`, trivial `+0x8` read) |
| vtable`+0x218` | fn | ACTIVE-multiplier getter (`FUN_1801e19d0`): type 1 → `+0xC`, type 0 → `+0x10` |

### The consumer chain (why writing Option+0x10 alone is inert)

The **GamePlayActor latches the multiplier at construction** (`FUN_18005cce0`
state 0 via `FUN_18005be90`): `+0x29C = vcall(Option vtable+0x218)` (int ×100),
`+0x290 = +0x294 = multiplier / 100.0` (f32 pair: lerp current/target, start
time at `+0x298` — mid-song speed-change animation). **Per frame (state 4) the
actor re-writes the renderers from its own floats**:
`speed = (int)(lerp(+0x290, +0x294, t)·100)` → `SpotRenderer+0x28` and
`ArrowRenderer+0xA0` (the field `mine_render`/`playfield_styling` read). The
construction-tail store pair
(`F3 0F 11 83 94 02 00 00  F3 0F 11 83 90 02 00 00`) matches exactly twice on
both 20260721 (`18005cbf8`/`18005cc46`) and 20260616 (`18005cbb8`/`18005cc06`)
— the cluster is build-stable.

### The shipped recompute (`src/services/song_rate/real_speed.rs`)

At a committed non-identity rate, once per side per song at that side's FIRST
judge dispatch (post-commit, post-construction — the Assist Tick anchor's
timing guarantee): for Real-Speed-mode sides (type 0) only, derive
`clamp(trunc(target·100 / (core_bpm × source/output)), 25, 800)` (host-tested,
native-faithful f64 trunc) and write the ACTOR cluster
(`+0x29C`, `+0x290`, `+0x294`) plus `Option+0x10` for display consistency.
Owned by the `song_playback_speed` mod — deliberately independent of the Real
Speed Fix toggle (req 33); identity/uncommitted songs take no write at all, so
both toggle states keep stock behavior bit-identically. Every failure leg
(unresolved table, unreadable chain, out-of-domain target/core) skips the
write: stock behavior, one latched WARN.
