# Task: Binding Runtime Core — Ring, Pending Slots, Generator, and the Serve Dispatch

## Description

Build the streaming engine's pure runtime heart in crate code, host-tested
end to end: the `Binding` runtime state (bounded 16 MiB ring, four pending
read slots, epoch guard, Active/SilenceFill/Retired), the producer thread
(`src/services/song_rate/generator.rs`: on-demand source decode →
`StretchState` → per-block ADPCM encode into the ring, loop-start
checkpointing, regeneration targets, pending-slot completion,
`catch_unwind` → SilenceFill, throughput/deferral metrics), and the pure
per-region serve dispatch the IO-callback detours will call (task-04's
windows glue is thin over this). Design reqs 16–18, 20–21, 27–29. No
detours, no transaction changes, no readiness change — the tree stays
identity-only.

## Background

Steps 2–3 proved every DSP and layout piece this task composes:
`StretchState` (byte-equal to the whole-buffer reference, resumable via
`StretchCheckpoint`/`restore`), `adpcm::BlockCachePcm` (on-demand source
decode) and `encode_block` (byte-equal per block), and
`virtual_bank::plan_virtual_bank`/`resolve` (the virtual layout + region
mapping with the stock EOF clamp). The Step-3 test harness
(`src/core/xact/tests.rs`, the `EncodedFeed` + replay pump) demonstrated
the exact production pipeline shape — this task builds the CRATE version:
producer-thread-driven, ring-buffered, deferral-capable.

Key structural readings (design Data Models + component specs):

- **The ring is indexed by absolute virtual data offset** (`base`,
  `produced`, `consumed` are virtual offsets): it linearly covers the data
  region — entry 0's stream, the zero-filled alignment gap, entry 1's
  stream — so one producer cursor serves both entries. The window
  `[produced − capacity, produced)` is always serveable; reads below it set
  a regeneration target. Capacity 16 MiB, an internal constant (req 38: no
  operator knobs).
- **Pending slots are SPSC handoffs**: the (future) read detour arms a
  slot (Release), the producer fills the caller's buffer + adds to the
  completion accumulator + marks Complete, the (future) poll detour
  consumes. Four slots (the engine keeps one outstanding read per stream).
  The completion accumulator abstracts `OVERLAPPED.Internal` — the pure
  layer takes a raw accumulator pointer (host tests pass a local cell;
  task-04 passes the real OVERLAPPED field), preserving the stock
  "accumulate on serve, report-and-zero on poll" protocol exactly.
- **Checkpoint/regeneration**: the producer captures the qualifying
  `StretchCheckpoint` at the stretched loop start of a looped entry
  (production banks carry full-entry loops — the zero checkpoint;
  hop-aligned resumes bridge to block-aligned targets by produce-and-
  discard, exactly the Step-3 `restore_at_block` mechanics). Reads behind
  the window regenerate deterministically from the nearest checkpoint (or
  zero) behind a deferral; identical bytes are reproduced (req 20).
- **SilenceFill** (req 28): a producer panic or allocation failure flips
  the binding state; all further data reads complete instantly with valid
  pre-encoded silent ADPCM blocks (encode of zeros); the stream must still
  parse and decode. The flip is the containment boundary
  (`catch_unwind` in the producer).
- **Epoch guard** (req 26): readers increment before validating
  `state == Active`, decrement after copy-out; reclamation requires
  `Retired ∧ readers == 0` (the drain checks — task-03 wires it).
- **Serve dispatch** (the readFile detour body minus windows types):
  resolve the virtual offset through `VirtualBankLayout::resolve` —
  PreData/Gap/Eof → copy/zero/clamp + accumulate, EntryData within the
  produced window → copy + accumulate, not-yet-produced or behind-window →
  arm a pending slot (behind-window sets the regeneration target first)
  and report Pending, SilenceFill → silent blocks immediately. Must be
  allocation-free, log-free, and panic-free (it runs in detour context in
  task-04; threading table).
- Module placement per the design sketch: `Binding` state in
  `src/services/song_rate/binding.rs` (alongside the Step-1 pure helpers),
  producer in a new `src/services/song_rate/generator.rs`; exact field
  shapes free, behavior binding. Host tests as sibling `*_tests.rs`
  modules (the validator harness mounts `src/services/song_rate/mod.rs`).

## Reference Documentation

**Required:**
- Design: `.agents/planning/2026-08-08-song-rate-streaming/design/detailed-design.md`
  (reqs 16–18, 20–21, 26–29; §`services/song_rate/generator.rs`;
  §`services/song_rate/io_callback_hook.rs` — the serve semantics this
  task's dispatch implements; Data Models: PendingSlot + Ring; Threading
  and Synchronization table; Error Handling rows for lag / producer death /
  behind-window)

**Additional References (if relevant to this task):**
- `docs/xact_streaming_research.md` — §3 read-pattern facts (packet sizes,
  one outstanding read, loop restart as the only backward jump), §7
  gotchas (the `Internal` accumulator protocol)
- `.agents/planning/2026-08-08-song-rate-streaming/implementation/step03-task-02-synthetic-engine-replay-harness/progress.md`
  — the proven feed pipeline + restore-at-block discard mechanics this
  task productionizes
- `.agents/planning/2026-08-08-song-rate-streaming/progress.md` —
  Deviations: the NoCandidate envelope (full-entry loops required at
  25%/50%; the generator must treat `NoCandidate` as a failure leg, never
  an impossibility)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements

1. `Binding` runtime state per the design sketch: `{file_id, generation,
   rate, layout: VirtualBankLayout, source: Box<[u8]>, ring, pending
   slots, state: AtomicU8 (Active | SilenceFill | Retired), readers:
   AtomicU32}`. Construction takes an already-planned layout + the private
   source copy (task-03's preflight produces both).
2. `Ring`: fixed 16 MiB buffer, atomics-cursored (`base`/`produced`/
   `consumed` as absolute virtual data offsets), single producer /
   multi-reader; `produced` Release-published, Acquire-read.
3. `PendingSlot`: `{state: Free|Armed|Complete, buffer ptr, completion
   accumulator ptr, offset, len}` all atomics, SPSC per slot; arming
   records the request, completion memcpys + accumulates + marks Complete;
   cancellation (retire/silence-fill) completes with the EOF-clamp
   semantics (0-byte completions permitted).
4. `generator.rs`: one `std::thread` per bound generation (name
   `song-rate-generator`), generation-token checked at every hop:
   `BlockCachePcm` over the binding's source copy → `StretchState::produce`
   → whole-block accumulation → `adpcm::encode_block` into the ring, in
   virtual-offset order across both entries (gap zeros emitted between
   them); maintains the loop-start checkpoint; honors regeneration targets
   (restore + produce-and-discard to the block-aligned target, or restart
   from zero); completes pending slots whose ranges become available;
   `catch_unwind` → SilenceFill flip; superseded generations stop at the
   next checkpoint-granularity opportunity.
5. A fault hook for task-03's `mid-song-failure` selector: an injectable
   kill-after-N-packets (or equivalent) knob that makes the producer die
   mid-stream through the same `catch_unwind` → SilenceFill path.
6. Throughput/deferral metrics recorded per generation (frames produced,
   wall time, max deferral latency, deferral count) — exposed for the
   drain to log at generation end (task-03 wires the logging; feeds plan
   Step 5's benchmark).
7. The pure serve dispatch: given the binding and a read `(offset, len,
   dest, accumulator)` → Served(n) / Pending / (post-retire) refused —
   implementing the region semantics above with the stock EOF clamp
   against `virtual_size`; allocation-free, log-free, panic-free; usable
   verbatim by task-04's detour.
8. Host tests (sibling `*_tests.rs`, validator harness): the full protocol
   matrix below; keep the added cargo-test phase cost proportionate (the
   Step-3 suites run in ~8 s; stay well under the ~30 s total budget).

## Dependencies

- Steps 2–3 (complete): `StretchState`/checkpoints, `BlockCachePcm`,
  `encode_block`, `plan_virtual_bank`/`resolve`.
- None within Step 4 (task-01 is independent; this task consumes no
  addresses). Blocks tasks 03 and 04.

## Implementation Approach

1. Land the data structures first (Ring, PendingSlot, Binding + epoch
   guard) with focused protocol tests (arm/complete/consume, cancellation,
   quiescence).
2. Port the Step-3 feed shape into `generator.rs` (producer loop +
   checkpoint + regeneration + silence-fill); prove byte equality of a
   fully drained ring against the Step-3 whole-buffer oracle composition
   (rebuild the minimal oracle in the test module — parse → plan → decode →
   reference stretch → encode → stream-write).
3. Add the serve dispatch over the ring + layout; drive the RE-pinned read
   pattern through it (header read spanning regions, sequential packets,
   loop-restart jump, EOF clamp) with a synchronous test pump that treats
   Pending as "run the producer, then re-poll".
4. Add the failure legs (silence-fill mid-stream, behind-window
   regeneration, cancellation) and the metrics.
5. Record progress in
   `.agents/planning/2026-08-08-song-rate-streaming/implementation/` (repo
   convention: NEVER `.agents/scratchpad/`); run the full gate set.

## Acceptance Criteria

1. **Generated bytes equal the oracle through the ring**
   - Given a synthetic full-entry-loop bank (both physical entry orders)
     and rates {50, 175} at minimum
   - When the producer fills the ring and the serve dispatch replays the
     engine read pattern (deferrals resolved by producer progress)
   - Then the reassembled virtual file is byte-identical to the
     whole-buffer oracle, reparses, and decodes

2. **Deferral protocol is exactly-once with stock accounting**
   - Given a read ahead of the produced watermark
   - When it is armed, completed by the producer, and consumed
   - Then the accumulator carries exactly the served byte count once,
     poll-before-complete reports incomplete, and a second consume finds
     the slot Free

3. **Behind-window reads regenerate deterministically**
   - Given a loop-restart read below the ring window
   - When the regeneration target is honored (checkpoint restore +
     discard-to-block-boundary, or restart from zero)
   - Then the re-served bytes are identical to the first serving

4. **Silence-fill keeps the stream valid**
   - Given a producer killed mid-stream via the fault hook
   - When further data reads are served
   - Then they complete instantly with valid silent ADPCM blocks, the
     binding state reads SilenceFill, and the reassembled stream still
     parses and decodes

5. **Reclamation waits for quiescence**
   - Given a retired binding with a reader inside the epoch guard
   - When reclamation eligibility is checked
   - Then it refuses until `readers == 0` and the state is Retired, and
     pending slots cancelled at retire complete with clamp semantics

6. **Tree is green and identity-only**
   - Given the completed task
   - When running the five standing gates
   - Then all pass (Windows check 0 warnings), and readiness/row
     registration behavior is unchanged from the Step-3 tree

## Metadata

- **Complexity**: High
- **Labels**: song-rate, streaming, generator, ring, concurrency,
  host-validation
- **Required Skills**: Rust atomics/lock-free protocols, WSOLA streaming
  core, XACT read protocol, repository host-validator harness
- **Generated By**: code-task-generator 2026-08-10
- **Source Plan**: `.agents/planning/2026-08-08-song-rate-streaming/implementation/plan.md`
- **Plan Step**: Step 4: Wire the callback detours, binding, and generator into the transaction
