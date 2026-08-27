# Runtime Integration Research

## Summary

The remaining runtime questions have implementation-grade answers:

- the central gameplay clock has an eight-byte, byte-identical overwrite window
  on all four investigated builds;
- `wavebank_create(file_id)` is a synchronous post-XACT acknowledgement suitable
  for committing the generated XWB's exact effective rate;
- scene 26 plus existing `GameWork`/`PlayerWork` state is sufficient to admit
  ordinary solo/doubles while naturally excluding matching, special, course,
  and local-versus flows;
- the selected song code does not need to be resolved at scene 26: the first
  qualifying dance XWB path supplies it after the rate/mode generation is armed.

Live proof is still required for transformed-bank audio quality, natural song
termination, CrossOver/native redirection, and end-to-end drift. Those are
implementation acceptance gates rather than unresolved architecture choices.

## Central Gameplay Clock Patch

`docs/song_playback_speed.md` identified the authoritative raw count calculation.
Ghidra re-verification found the following bytes at the transform site on every
investigated build:

```text
44 8D 34 18             LEA R14D,[RAX+RBX]
4C 8D 67 58             LEA R12,[RDI+0x58]
41 0F B7 54 24 2A       MOVZX EDX,word [R12+0x2A]
```

| Build | Transform site |
|---|---:|
| 2026-03-24 | `+0x5D37A` |
| 2026-04-21 | `+0x5D3EA` |
| 2026-06-16 | `+0x5CD2A` |
| 2026-07-21 | `+0x5CD6A` |

The first two complete instructions form a safe eight-byte redirect window. A
five-byte near jump plus three NOPs can replace them; the stub replays both,
scales signed `R14D` with the generation's fixed-point effective rate, and jumps
back to the `MOVZX`.

No conditional branch consumes flags between the patch and the following
`TEST ECX,ECX`, so the stub does not need to preserve incoming flags. It must
preserve all live registers except `R14D`, use only scratch registers or explicit
saves, and perform sign-aware deterministic rounding for negative lead-in time.

The patch should be installed once and never removed. Its multiplier starts at
exact identity; disabling the feature or selecting 100% changes state, not code.
Installation must verify the full eight bytes, populate/flush the stub before
redirecting, read back/flush the game-code write, and publish readiness only
after every check succeeds.

### Cross-version clock signature

Task 1 re-verified one structural match on all four supported binaries:

```text
48 63 89 84 00 00 00
48 8D 35 ?? ?? ?? ??
33 D2
48 8B 0C CE
E8 ?? ?? ?? ??
48 8B 10 48 8B C8
FF 92 48 02 00 00
44 8D 34 18 4C 8D 67 58
41 0F B7 54 24 2A
```

The wildcards are only the module-table RIP displacement and helper-call rel32.
The actor field `+0x84`, context-table load, XACT-style vcall `+0x248`, both
overwritten instructions, and following `MOVZX +0x2A` remain literal. The patch
window is always at match `+0x25`:

| Build | Signature match | Patch site |
|---|---:|---:|
| 2026-03-24 | `+0x5D355` | `+0x5D37A` |
| 2026-04-21 | `+0x5D3C5` | `+0x5D3EA` |
| 2026-06-16 | `+0x5CD05` | `+0x5CD2A` |
| 2026-07-21 | `+0x5CD45` | `+0x5CD6A` |

## Streaming XWB Acknowledgement

### Game path

The 2026-07-21 path is:

```text
song_bank_load (+0x61680)
  -> FileManager loads dance/<code>.xsb and dance/<code>.xwb
  -> audio::XwbFileCallback (+0x1AC650)
  -> sound_file_register (+0x1AA520)
  -> wavebank_create(file_id) (+0x1AB050)
  -> avs_fs_convert_path(record.virtual_path)
  -> CreateFileA(native_path, overlapped | no-buffering)
  -> IXACT2Engine::CreateStreamingWaveBank
  -> manager wave-bank/vector insertion
  -> IXACT2Engine::DoWork
  -> return true
```

`wavebank_create` returns false on native-open or XACT failure and true only
after the wave bank is accepted and installed. This is a stronger commit signal
than AVS lstat, path conversion, or `CreateFileA` alone.

### Cross-version signature

The following structural prologue pattern matched exactly once on all four
builds. Only the stack-cookie and audio-manager RIP displacements vary:

```text
48 8B C4 55 41 54 41 55 41 56 41 57
48 8D A8 28 FF FF FF
48 81 EC B0 01 00 00
48 C7 45 90 FE FF FF FF
48 89 58 10 48 89 70 18 48 89 78 20
48 8B 05 ?? ?? ?? ??
48 33 C4
48 89 85 A0 00 00 00
48 63 F1
4C 8B 35 ?? ?? ?? ??
49 8B 56 68 49 8B 46 70
```

| Build | `wavebank_create` |
|---|---:|
| 2026-03-24 | `+0x1A84F0` |
| 2026-04-21 | `+0x1A9190` |
| 2026-06-16 | `+0x1A9FF0` |
| 2026-07-21 | `+0x1AB050` |

The hook ABI is `bool wavebank_create(i32 file_id)` under Microsoft x64.

### Wave-bank unregister target

The XWB file callback vtable's unload slot (`vftable + 0x28` on 2026-07-21)
receives `(this, file_id)` and forwards `file_id` to a manager-level unregister
routine. That routine distinguishes XSB from XWB, calls the XACT bank object's
release/destroy vslot, closes the native handle, removes the file-id record and
the wave-bank bookkeeping record, then returns. Its effective ABI is
`void wavebank_unregister(i32 file_id)` under Microsoft x64.

The following structural entry pattern matched exactly once on all four builds:

```text
48 89 5C 24 08 48 89 74 24 10 57 48 83 EC 20
48 8B 05 ?? ?? ?? ??
48 8B 35 ?? ?? ?? ??
48 63 F9 48 8D 14 BF 41 B8 03 00 00 00 48 C1 E2 05
48 03 50 28 0F B6 82 8F 00 00 00 48 8D 4C 10 11
48 8D 15 ?? ?? ?? ?? E8 ?? ?? ?? ?? 85 C0 75 ??
```

Only the two globals, `"xsb"` RIP reference, string-compare call, and branch
distance vary. The 0xA0 file-record stride (`LEA`/`SHL 5`), extension offset
`+0x8F`, and literal three-byte extension comparison remain structural.

| Build | `wavebank_create` | unregister | Delta |
|---|---:|---:|---:|
| 2026-03-24 | `+0x1A84F0` | `+0x1A8870` | `+0x380` |
| 2026-04-21 | `+0x1A9190` | `+0x1A9510` | `+0x380` |
| 2026-06-16 | `+0x1A9FF0` | `+0x1AA370` | `+0x380` |
| 2026-07-21 | `+0x1AB050` | `+0x1AB3D0` | `+0x380` |

The constant delta is corroborating evidence only; runtime resolution uses the
independent structural signatures and never derives unregister by addition.

### Commit protocol

1. Scene 26 arms generation `G` with the resolved requested rate but leaves the
   clock at identity.
2. The `wavebank_create` detour creates a thread-local call nonce/depth frame.
   LayeredFS sees the qualifying `data/sound/win/dance/<code>.xwb` for `G`,
   resolves the effective source, builds or reuses the cache entry, and records
   `{call nonce/depth, G, normalized-path digest, cache/output digests,
   effective_rate}` in that frame.
3. `avs_fs_convert_path` returns that generated native path only after the cache
   entry is complete and validated.
4. Generated-path exposure makes `G` temporarily uncancellable
   (`XactInFlight`), then the detour calls the original exactly once.
5. If the original returns true, post-call logic consumes only the token matching
   that nonce/depth/path/cache identity, then publishes safety policy and the
   exact rate for `G` before returning to the file callback.
6. A success without a matching token is a normal stock/static-LayeredFS bank
   and leaves the clock at identity.

The token prevents lstat, previews, unrelated/nested XWBs, worker-thread reuse,
or stale generations from activating the clock.

## Late XACT Failure Limitation

`wavebank_create` inserts native file-handle bookkeeping before calling
`CreateStreamingWaveBank`. On failure, the stock function returns false without
demonstrably removing that bookkeeping or closing the handle. Calling it again
immediately with the stock path could duplicate stale manager state.

Therefore:

- every failure before exposing the generated path still falls back cleanly to
  stock 100%; this covers unsupported banks, generation errors, cache errors,
  missing hooks, and failed mode classification;
- a generated path that reaches XACT but is rejected must abort that stage's
  loading cleanly, retain identity clock state, and keep score state untrusted;
- transparent same-attempt retry with stock audio is deferred until a safe
  cleanup/retry path is proven.

The hidden pre-generated-bank diagnostic and strict host validation make this a
residual failure path, not normal behavior. The user accepted this narrowed D21
policy before design readiness.

## Mode and Player Classification

### Normal e-amusement play

Ordinary e-amusement connectivity does not affect eligibility. Cards, profile
load/save, and the standard server connection remain active. Rate selection is
about gameplay sequence type, not online/offline status.

### Sequence gate

The mod arms only on transition into normal scene 26
(`SONG_TO_STAGE_INTERSTITIAL`). Existing scene research shows:

- normal chain: song select 25 -> interstitial 26 -> gameplay 28;
- matching/battle chain: separate scene range 47-57;
- event/special song-select variants: separate scenes, including 49 and the
  event/special tail around 56.

Those alternate flows never arm a non-100% normal generation and remain at
identity without requiring a fragile global “network connected” test.

### Course and local versus

At scene 26:

- `stage_records::game_work()` plus the decoded course-field offset identifies
  course mode. A nonzero course field forces 100%.
- `stage_records::player_work(side) + 0x4` is the existing per-side entered flag.
  Exactly one entered side selects that side's persisted `SONG SPEED` value.
- zero or two entered sides force 100%; two sides is local versus.
- doubles still has one entered player preference and is supported without
  needing to identify whether its later gameplay actor uses side 0 or side 1.

If `stage_records` or any required pointer is unavailable, classification fails
closed to 100%.

## Song Identity and Generation Timing

The implementation does not need a new selected-song global or model layout.
Scene 26 arms the rate/mode generation. The subsequent qualified dance XWB
virtual path carries the actual song code and is already the exact file XACT will
open.

On an uncached rate, the XWB handler performs generation on a background worker
while the streaming path-conversion call waits. This can pause or extend the
stage-loading screen, but does not execute CPU-heavy stretch under a game/service
lock. It validates that the generation is still armed before path exposure; once
exposed, the generation cannot be superseded until XACT resolves.

Quick Restart does not revisit scene 26 and normally retains the current slot-5
bank. The implementation must also support idempotent re-exposure/recommit of the
same cache generation if a supported build reloads it. A genuinely new song
reaches scene 26 and replaces the generation after no XACT call is in flight.

## Dependent Integration Notes

- Assist Tick must use the committed exact effective rate. To preserve its
  existing 300 seconds of chart-content coverage at the minimum 75% rate, its
  wall-time bank capacity should increase to 400 seconds.
- Real Speed should keep the selected target unchanged and derive its hidden
  multiplier from `Core BPM * effective_rate`. At gameplay entry the song-rate
  callback can reproduce `SetScrollSpeed`'s documented derived-field formula
  with raw Option reads/writes, avoiding a competing patch or a game-function
  call while scene-manager callback iteration is locked. The target/derived
  fields are established, but project notes disagree on which cached-BPM slot is
  Core; implementation must re-derive and live-validate that source before the
  feature can report ready.
- Movie suppression begins tentatively at a non-100% scene-26 arm, before
  `BuildGraph` ordering matters, and uses shared ownership of the sole
  `movie_build_graph` detour.
- Rate score taint is a per-side pending-stage-save count, deduplicated by
  generation and consumed only when the corresponding save is suppressed. Scene
  changes and the generic Quick-Fail reset do not clear it.

## Remaining Live Validation

- Native Windows and CrossOver both open the generated path and return success
  from `wavebank_create`.
- Main and preview entries work for both observed entry orderings.
- First note, final note, natural song end, and long-song drift align at 75% and
  125%.
- XWB duration/loop metadata terminates each voice correctly.
- Uncached generation time and peak memory are acceptable.
- The WSOLA-like output passes listening tests across representative song types.
- Quick Restart reuses the applied bank/rate and a new song replaces it.
- Generated-bank rejection produces a bounded loading failure, identity clock,
  and no trusted score.
