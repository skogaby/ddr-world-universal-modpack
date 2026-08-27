# Streaming Mechanism Research — Design Implications

Date: 2026-08-08. Distills the RE findings in `docs/xact_streaming_research.md` (the
durable note produced by this research pass) into the mechanism choices the design must
make. Read that note first; this file assumes it.

## The core mechanism (follows directly from the RE facts)

Detour gamemdx's TWO XACT file-IO callbacks (readFile `FUN_1801aa250`,
getOverlappedResult `FUN_1801aa350` on 20260721; both resolved by ONE new AOB on the
audio-manager constructor's callback-setup region, RIP-decoded). For everything not
rate-bound, call the original (trampoline) — byte-exact stock behavior, including the
preview player and all other slots. For the ONE rate-bound handle:

- **Header region reads** (offset < 2048): serve from a synthesized stock-shaped header
  (`core::xact::xwb::write_song_bank_streaming`'s header emission already computes it
  from `{data_len, duration, loop_start, loop_length}` per entry — pure rate math,
  milliseconds). The engine's single 0x1000 header read is issued synchronously inside
  `wavebank_create` — i.e. inside our existing create detour — so binding strictly
  before the original call makes the first read race-free.
- **Data region reads**: serve from the incremental generator's output buffer;
  `min(len, virtual_size − offset)` EOF clamp exactly like stock.

### Binding identity — by file_id, established pre-original

The create detour already receives `file_id`. Bind `{file_id, generation}` BEFORE
calling the original (after verifying: armed generation, slot-5, dance-bank path parse,
song match, source RAM copy parseable, header synthesized, generator started). The read
detour resolves handle→file_id exactly as stock does (same sorted-vector walk under the
same AVS mutex gate — a cost class the stock callback already pays on every read) and
compares against the bound file_id. No handle capture, no race with the in-create header
read, and unbind happens in the existing unregister detour before the engine forgets the
handle.

This REPLACES the old two-stage open-redirect invariant: the FileManager RAM copy now
holds the STOCK bank and becomes the generator's SOURCE rather than the audible bytes.
`fs_open`/`fs_lstat` interception for song-rate disappears entirely.

### Commit authority

The wavebank_create transaction survives with "expose" → "bind": pre-original bind,
original runs (engine parses OUR header — a definitive audio-authority success signal
once create returns ≥ 0), post-original commit in the existing order (score taint →
movie → snapshot → Q31 LAST) or unbind + LateFailed on failure. The exactly-once/TLS/
slot machinery in `transaction.rs`/`xact_runtime.rs` carries over with lease fields
deleted.

## Back-pressure: deferral is native (the decisive finding)

`FUN_00426c80` tolerates `FALSE + ERROR_IO_PENDING` at issue time; completion is polled
via the getOverlappedResult callback with `bWait=0` from the engine pump (the ONLY call
site). So if the generator hasn't reached the requested range, the read detour can
return pending without blocking any thread, and our getOverlappedResult detour reports
completion when the bytes exist. One outstanding read per stream context; the engine's
~250 ms look-ahead plus 64 KiB packets (~1.3 s of stereo ADPCM audio each) make genuine
underruns unreachable at ≥1× synthesis speed (measured ≈11× under Wine).

Requirement discovered: the stock getOverlappedResult callback reports instant
completion for ANY vector-listed handle — a deferral without intercepting BOTH callbacks
would surface as a spurious 0-byte completion. Both detours are mandatory as a pair.

## Generator shape (options for the register)

Producer thread (generation-tokened, like the old worker thread but permanent-per-song
rather than deadline-bounded) running the incremental WSOLA + per-block ADPCM encode;
detours only memcpy from the produced region and update cursors (allocation-free,
log-free, lock-free — atomics + fixed buffers).

Output buffering options:
- **A. Progressive whole-bank buffer**: allocate virtual_size once at bind; producer
  fills left to right; any offset ever readable. Simplest; memory = stretched bank size
  (12 MB stock song → ~50 MB at 25%; grows with song length — weakens the "arbitrary
  length" guarantee on 4 GB cabinets, though the 28-bit duration ceiling caps the
  theoretical worst near ~300 MB/entry).
- **B. Bounded ring**: fixed budget (e.g. 8–16 MiB ≈ 2.5–5 min of audio ahead);
  strictly length-independent; backward jumps (loop restart, engine re-read) outside
  the window require deterministic regeneration (restart WSOLA from 0 or from a saved
  loop-start checkpoint — per-step state is ~5 words, checkpointing is free).
- Sequential-access reality: header once, then strictly forward packets, loop-start
  jump only for looped entries (the gameplay-played main entry typically has
  loop_length 0; the `_s` preview entry inside the gameplay bank is not played during
  gameplay).

## Incremental WSOLA reformulation (from `src/core/xact/stretch.rs` analysis)

The main loop is already a left-to-right sliding window: O(window + 2·radius ≈ 2160
frames) source lookback, O(window) output lookahead, per-step state = phase accumulator
+ previous window + cursor. Blockers are only API-shape: whole-buffer in/out, the
whole-buffer identity shortcut, and the terminal end-anchor region (last `window`
frames, computable once the source tail is available — it always is, the whole stock
source sits in RAM). Reformulation = a `StretchState` streaming core; the existing
whole-buffer function becomes a reference implementation and the equality property
(streaming output ≡ whole-buffer output, byte-identical) keeps all Step-1 DSP evidence
valid. Source PCM decode is per-block on demand (`adpcm.rs` blocks are self-contained).

## Source lifetime hazard

The generator reads the stock bank from the game's FileManager RAM buffer, which is
freed after unregister/file release. Two safe shapes: (a) copy the source bytes once at
bind (~6–15 MB, CRT-heap-free — our own allocation); (b) read the game buffer in place
with a strict stop-the-producer-before-original-unregister handshake. (a) buys total
lifetime decoupling for one memcpy; (b) saves memory but couples producer shutdown to
the unregister detour's timing budget.

## What survives / what dies (module level)

Survives: lifecycle (phases renamed where redirect-specific), clock_patch, score_guard,
runtime scene callback + desired atomics + drain (retargeted), transaction/xact_runtime
minus lease/cache fields, wavebank_hook, `dance_bank_song_code`, rate.rs, adpcm.rs,
xwb.rs, digest.rs, the option mod, the backend.
Dies: cache.rs, worker.rs, model.rs (manifest/LRU/tombstone/limits), conversion.rs's
redirect/store/lease/quarantine halves, transform.rs (becomes reference/spec),
`cache_limit_gib`, the `_cache/song_playback_speed` directory, the 30 s deadline, the
128 MiB admission.

## Failure surface (new classes the design must decide)

- Bind-time failures (source unparseable, 28-bit duration overflow, header synth
  refusal) → don't bind → stock 100 % (EarlyFailed) — same fail-open leg as before.
- Mid-song generator failure (panic, OOM) — NEW audible class: options are silence-fill
  (complete reads with pre-encoded silence blocks; song continues, clock/taint kept),
  indefinite deferral (audio stalls while the chart runs — worst option), or forced
  song abort (no clean mechanism exists). Silence-fill is the only shape that keeps the
  player in a judgeable song.
- Sustained producer-behind (cabinet CPU slower than realtime): deferral covers
  transients; a persistently-behind producer degenerates to stuttering audio. A
  realtime-margin benchmark belongs in the implementation plan's first live step.
