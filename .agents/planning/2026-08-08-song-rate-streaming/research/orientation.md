# Orientation — Song Rate Streaming Redesign

Date: 2026-08-08. Blind-spot pass over the repository + the retired feature's
records before requirements clarification. Sources cited inline.

## What exists today (retired model, checkpoint `ee0368f`)

The full inventory is in the retired feature's records
(`.agents/planning/2026-08-05-song-playback-speed/progress.md`). Summary of the
module-level survey (all files under `src/services/song_rate/` unless noted):

| Module | Verdict for redesign | Notes |
|---|---|---|
| `lifecycle.rs` (623 ln) | KEEP | Pure phase machine (`Identity, Armed, Preparing, RedirectReady, XactInFlight, Committed, Completed, EarlyFailed, LateFailed`), scene-26 eligibility, rate-domain helpers (`snap_rate_percent`, 25..=175 step 5), `bind_song`. Zero cache dependency. Phase names referencing the redirect (`RedirectReady`) will need renaming/re-scoping. |
| `clock_patch.rs` (473 ln) | KEEP | Q31 stub + `RatePublication` seqlock; identity-first reset, committed-last publish. Proven on cabinet. |
| `transaction.rs` (357 ln) | KEEP (rework payloads) | Exactly-once create protocol, `TransactionParts`, `DDR_SONG_RATE_FAULT` selector. Cache coupling is only lease-id/cache-digest pass-through. |
| `xact_runtime.rs` (863 ln) | KEEP (trim) | 4-slot table, `RedirectToken`, TLS `FrameStack`, `MaintenanceQueue`, `BankTimeline`. Remove `lease_id` field + `ReleaseLease` maintenance kind; token digest fields shrink. |
| `wavebank_hook.rs` (269 ln) | KEEP (rework) | The two detours (`wavebank_create`/unregister) + readiness conjunction. Body's TransactionParts wiring survives; what "expose" means changes. |
| `conversion.rs` (560 ln) | SPLIT | Cache-independent: `dance_bank_song_code`, `song_code_digest`, phase choreography, bound-song check. Cache-coupled (REMOVE): `resolve_generated_bank` (coordinator.request), generated-path plumbing, lease transfer, quarantine, the whole two-stage open/expose redirect model. |
| `runtime.rs` (867 ln) | KEEP (trim) | The ONE scene callback, desired-percent atomics, `integration_ready()`, commit logging, 250 ms drain. Remove: `coordinator()`, `CACHE_ROOT`/`GAME_VISIBLE_ROOT`/`CACHE_LIMIT_BYTES`, lease/tombstone drain, `OPEN_REDIRECT` path cache. Drain still needed for: commit-visibility log poll, timeline drain, slot release (`finish_release`), plus whatever deferred cleanup streaming state needs. |
| `cache.rs` (963 ln) | REMOVE | Entire on-disk store. |
| `worker.rs` (1873 ln) | REMOVE (salvage bits) | Coordinator/deadline/admission/eviction all go. `GenerationRequest::from_source`'s validation + rate-target math is salvageable logic. |
| `model.rs` (923 ln) | MOSTLY REMOVE | Manifest/LRU/tombstone/cache-limit forms go; `FullDigest` (MD5 identity) survives if the new design wants identity digests. |
| `src/core/xact/rate.rs` | KEEP as-is | `RateRatio` (GCD-reduced), `target_for_percent` (output always whole ADPCM blocks; 28-bit ceiling check), `q31()`, `content_to_wall_ms`. |
| `src/core/xact/adpcm.rs` | KEEP | Blocks fully self-contained (stock stereo: 140 B / 128 frames). Per-block encode of an arbitrary exact-multiple window already possible; a public `encode_block` would be cleaner. |
| `src/core/xact/xwb.rs` | KEEP | `write_song_bank_streaming` already factors header emission from data: `StreamedEntry {data_len, duration, loop_start, loop_length}` fully determines the header + metadata + names + 2048-pad. Header synthesis for the virtual bank is essentially already written. |
| `src/core/xact/stretch.rs` | REFORMULATE | See below. |
| `src/core/xact/transform.rs` | REMOVE (reference) | The whole-song pipeline; its per-entry plan (`target_for_percent` + loop mapping via `map_boundary`) is the spec for what the incremental generator must reproduce. The 128 MiB ceiling lives here and dies with it. |
| `src/core/xact/digest.rs` | KEEP | Incremental MD5; streaming-friendly. |
| `src/mods/song_playback_speed.rs` (173 ln) | KEEP unchanged | Only touches `snap_rate_percent`, `set_desired_percent`, `integration_ready()` — all surviving surfaces. |
| `src/services/score_guard.rs` | KEEP unchanged | Pending rate-save ledger + sanitised logout, cabinet-proven. |

Host test counts at `ee0368f`: 156 in the song-rate validator (23 xact core, 25
conversion, 22 lifecycle, 12 transaction, 9 xact_runtime, 6 clock_patch, 5
wavebank_hook, rest cache/worker/model). The cache/worker/model tests (~60) go
with their modules.

## The streamability finding (from `src/core/xact/stretch.rs`)

The WSOLA core is **already a left-to-right sliding-window process**:

- Output-driven main loop; per-step state is tiny: Q32.32 phase accumulator,
  `output_start`, previous selected `SourceWindow`, counters.
- Candidate search needs only source `[nominal − radius, nominal + radius +
  window)` ≈ 2160 frames @48 kHz; output writes touch only `[output_start,
  output_start + window)`.
- No global precomputation (local SAD correlation only).

What blocks incrementality today, all reformulable:

1. Whole-buffer API (`&[i16]` in, `Vec<i16>` out, full allocation up front).
2. The identity shortcut memcpys the whole buffer.
3. **Terminal end-anchor region**: output in `[output_frames − window,
   output_frames)` is written by a special non-hop terminal placement so the
   final sample equals the source's final sample. Needs to become a special
   final-region synthesis (only needs the last window+hop of source).
4. Diagnostics vecs (selected/nominal source starts) assume whole runs.

`output_frames` totals are needed up front but are pure numbers from
`RateTarget` — no audio required.

Loop context: nominal remapping + cyclic source windows are per-step local; a
loop-region restart during streaming needs either deterministic re-run from 0
or a checkpoint of the tiny per-step state at the loop-start output position
(cheap, deterministic).

## The audio-path finding (cabinet-proven 2026-08-07, retired feature's log)

- Slot-5 dance banks: audible bytes come from the FileManager RAM copy, served
  to XACT by gamemdx's registered file-read callback `FUN_1801aa250` (memcpy of
  each requested (offset,length), faked OVERLAPPED result). The CreateFileA
  streaming handle is a lookup key only — never read.
- Consequence for the redesign: with streaming synthesis serving the READ
  CALLBACK, the FileManager RAM copy no longer needs to be redirected at all —
  it can hold the STOCK bank and become the generator's SOURCE (in RAM,
  pre-loaded by the game itself at song confirm, ~3 s before wavebank_create).
  The old two-stage open-redirect invariant dissolves; `fs_open`/`fs_lstat`
  interception for song-rate goes away entirely.
- The preview player (`FUN_18010eab0`) uses the same slot-5 machinery at song
  select with no rate armed — pass-through must be the default.

## Unknowns (research targets)

U1. **Read-callback contract** (Ghidra, gamemdx + xactengine2_10): exact ABI of
    `FUN_1801aa250` (XACT2 `XACT_FILEIO_CALLBACKS` readFile shape?), the paired
    GetOverlappedResult callback, sync-vs-async completion semantics, and what
    the engine does on short/failed reads. Determines the underrun design.
U2. **Read pattern** (static RE of xactengine2_10 streaming path): packet
    sizes, header-read sequence at create/Prepare, whether reads are strictly
    sequential in the data region, loop-seek behavior. Static-only during PDD
    (no deployment); anything unresolvable statically becomes a first-step
    live-instrumentation gate in the implementation plan.
U3. **File-size checks**: does the engine (or gamemdx) ever consult the real
    CreateFileA handle's size (GetFileSize) or the file-table's stored size for
    a slot-5 streaming bank? The virtual stretched bank's size differs from the
    stock file's (bigger at slow rates). If checked, the size source must be
    ours.
U4. **Binding site**: where the (handle/file_id → virtual bank) binding is
    established and torn down (wavebank_create/unregister survive as the
    binding transaction?); interaction with the existing 4-slot/TLS machinery.
U5. **Threading/budget**: which thread invokes the read callback (engine
    notify/DoWork/AVS worker?), what stack/time budget it has, and where the
    producer thread hands off (ring vs progressive buffer).
U6. **Incremental WSOLA correctness**: reformulation must be provably
    equal-output to the whole-buffer core (host tests: byte-identical output
    for the same input/rate) — this keeps all existing DSP evidence valid.
U7. **Cabinet DSP margin**: 11× realtime measured under Wine on the MacBook;
    cabinet CPUs slower. Not measurable during PDD — the plan needs a
    benchmark/telemetry step.

## Proposed sequence

1. Research first (U1–U5 via Ghidra; U6 via code analysis) — the decision
   register's biggest recommendations (underrun policy, threading, commit
   authority) hinge on U1/U3/U5.
2. Then the batched decision register (Step 3), then readiness, design, plan.
3. Research output doubles as the durable `docs/` song-rate RE note (the old
   plan's deferred Step 8), per the handoff instruction.
