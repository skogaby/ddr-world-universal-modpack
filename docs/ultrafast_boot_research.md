# Ultrafast Boot — Boot-Time SSQ Analysis Pipeline Research

> RE record for the "ultrafast boot" refactor of the `fast-bootup` mod: cache the
> boot-time chart-analysis outputs in a bin file so subsequent boots skip both the
> SSQ file reads and the re-parse, and remove the loader's per-frame pacing so
> cache misses load as fast as the disk allows.
>
> All Ghidra addresses are file-relative to image base `0x180000000`, program
> `gamemdx_20260721.dll` (game `MDX:J:F:A:2026072100`) unless suffixed `[0616]`
> (= `gamemdx_20260616.dll`). Investigated 2026-08-24. Companion docs:
> `.agents/planning/20260721-fast-bootup-ssq-race-fix/research/investigation.md`
> (async-load race, status enum) and
> `.agents/planning/20260724-fast-bootup-eol-overrun/research/investigation.md`
> (actor field map, dispatcher gate). This doc supersedes neither — it adds the
> *what is computed and where it lands* layer both stopped short of.

## 1. Overview

At boot, DDR World analyzes **every SSQ chart file in the game** to compute
metadata that `musicdb.xml` does not carry: per-chart min/core/max BPM, note
counts (→ EX score), shock/variable-BPM flags, groove-radar values, and a
handful of global maxima used for normalization. The whole pipeline is:

1. `Application::onBoot` creates the **step-data manager** (async file loader,
   singleton `DAT_1806f2f48`) and the in-memory **music DB**
   (`DAT_1806f2d78`, 0x258-byte entries parsed from `musicdb.xml`).
2. The boot screen graph creates `sequence::common::CheckStepDataActor`
   (the "NOW LOADING n%" screen). Its `onInit` registers one SSQ file per song
   with the manager and builds a work list of **5 items per song** (one per
   difficulty slot).
3. The manager's pump loads whole SSQ files asynchronously into heap buffers.
4. The actor's `onUpdate` (the function `fast-bootup` already hooks) processes
   **one work item per frame** stock: parse + summarize + analyze both play
   sides, write results into the music DB, release the file.
5. When the work list completes, the actor copies its accumulated global maxima
   into a global config block and sets its done flag.

Everything the pass produces is a **pure function of the SSQ file bytes** (plus
the side/difficulty selector), landing in a small, enumerable set of memory
locations. That is what makes an output cache sound: capture the analyzer
outputs per (file, side, difficulty) on first boot, replay the writes on
subsequent boots, and never read the chart files at all.

### Measured cost (cabinet log, 2026-08-24, gamemdx 20260721, i7-6700K, fast-bootup ON)

From `log.txt` (skogaby's cabinet, ~1441 songs / ~1466 SSQ files):

| Time | Event |
|---|---|
| 00:02:01 | spice2x launch |
| 00:02:03 | hook init complete, FastBootup enabled |
| 00:02:04 | first boot SSQ `avs_fs_open` |
| 00:02:19 | last boot SSQ open; NTX mine-injection tail; `ArkBootMode` ctor |
| 00:02:20 | `HARDWARE_CHECK` |
| 00:02:28 | `ArkGameMode` |
| 00:02:31 | `TITLE_SCREEN` |

The SSQ window is **~15.5 s of a ~28 s boot (≈55 %)**, at a steady ~95–105
opens/second. Even with fast-bootup's batching, the wall time is bound by the
*loading*, not the processing — see §6. Killing this window is the entire
payoff of the cache.

## 2. Key Functions

| Address (20260721) | Name | Role |
|---|---|---|
| `0x180032030` | `CheckStepDataActor::onInit` (vtable[4]) | Builds work list; registers SSQ files with the manager |
| `0x180032360` | `CheckStepDataActor::onUpdate` (vtable[6]) | Per-item: summarize + analyze ×2 sides + music-DB writes + release. The existing `check_step_data_update` hook target |
| `0x1801cbdc0` | SSQ summarize (`SsqReader` chunk index) | Walks chunk list, caches tempo/step chunk pointers, converts tempo table to ms |
| `0x1801c8680` | `IStepReader::Analyze` | Decodes one (side, difficulty) chart into note records; fills the 14-int result block + 5-int radar block. **Already detoured by NTX** |
| `0x1801c8ea0` / `0x1801c9440` / `0x1801c9710` | Analyze helpers | Radar/measure computation (internals not fully decoded; outputs land in the blocks we cache) |
| `0x1801b43f0` | SSQ path builder | `data/mdb_apx/ssq/<basename>.ssq`, with a hardcoded split-file table (`stvi`, `dopa2`, `sabm`, … → `<basename>_<1..5>.ssq`) |
| `0x1801fef30` | Manager: register file | FNV-1a name hash dedupe; refcount at `rec+0x24`; new records status ← 1, pushed to pending queue |
| `0x1801fdbf0` | Manager: pending-queue pump | Drives record status machine; **issues at most `mgr+0x70` (= 4) new file opens per call** |
| `0x1801fe150` | Manager: release-queue pump | Cancels in-flight entries (→ status 7) or unregisters (refcount--, buffer free at 0) |
| `0x1801fecf0` | Manager: unregister/free | refcount 0 → destroy reader, free buffer, clear name, slot → freelist |
| `0x1801ff1b0` | Manager: queue release | Pushes an entry index onto the release queue (called by `onUpdate` per item) |
| `0x1801fe380` | Manager: **blocking drain** | `while (pending ∨ release) { sleep(1 ms); pump other mgr (0x180202a40 on DAT_1806f2f60); pump; finalize; }` — the game's own "load at full speed" primitive |
| `0x1801ff7a0` / `0x1801ff880` | Alt-load (ARC provider) path | Fallback when the loose-file open fails; status 4 |
| `0x1801fe900` → `0x180201960` | Reader factory | `agcs::File::me_io::FileReader` on device `"local"` (AVS-mounted; see §6.3) |
| `0x1801fd590` / `0x1801fd350` | Manager ctor / param defaults | Defaults `{count=0x800, open_cap=4, prio=-1, flag=1, device="local"}`; onBoot overrides count=0x1000, prio byte=0x7E. Ctor creates the manager's I/O worker thread (64 KiB stack) at `mgr+0x48` |
| `0x1801b7e80` | Music-DB `lower_bound` by mcode | Over `DAT_1806f2d78` array (0x258 stride), keyed by `entry->vfunc[0]()` (getMcode) |
| `0x1801b4290` | Music-DB find-by-mcode (null-checked) | Convenience wrapper over the above |
| `0x180003020` | Application per-frame tick | Pumps `0x1801fdbf0` + `0x1801fe150` **once per frame**, then dispatches actor message 0x102 (onUpdate) via `FUN_18021dc70` |
| `0x180002af0` / `0x1800020b0` | Mode-switch / `Application::onBoot` | Additional blocking-drain call sites (drain is used liberally by the game itself) |

Other Analyze callers (unaffected by any boot cache): `FUN_18005aea0`
(callers `0x180057ec0` / `0x180061e20` — gameplay/stage chart load, the path
NTX relies on for real mine injection) and `FUN_1801df160`.

**Globals:**

| Address | Meaning |
|---|---|
| `DAT_1806f2f48` | Step-data manager singleton (existing signature `step_data_global_table`) |
| `DAT_1806f2f60` | Sibling resource manager (pumped alongside in the drain) |
| `DAT_1806f2d78` | Music DB `{begin, end}` pointer pair, 0x258-byte entries |
| `DAT_1806f14f8` | Global config object; boot pass copies actor accumulators to `+0x30..+0x54` |
| `DAT_1806f2858` | Error reporter fn-ptr (`"ME1529" / "FILE CORRUPTION ERROR"`) |
| `DAT_180393f40` | Variable-BPM threshold (double) for the `+0x124` flag |

## 3. Struct Layouts

### 3.1 Step-data manager (`DAT_1806f2f48`, ctor `0x1801fd590`)

| Offset | Meaning |
|---|---|
| `+0x00` | u32 record capacity (0x1000 at boot) |
| `+0x08` | records base — **0x40-stride** record array |
| `+0x28` | name records base — **0xA0-stride** |
| `+0x48` | manager I/O worker thread object |
| `+0x70` | u32 **max new file opens per pump call = 4** (ctor param[1], default from `0x1801fd350`; onBoot does *not* override it) |
| `+0x78/+0x80/+0x88` | pending queue vector {begin, end, cap} (entry indices) |
| `+0x98/+0xA0/+0xA8` | release queue vector |
| `+0xB8/+0xC0` | ARC alt-load provider vector |
| `+0xD8` | free-list head (next link at `rec+0x3C`) |
| `+0xE0` | rb-tree: FNV-1a(name) → entry index (register dedupe) |
| `+0x150/+0x154` | lock-enable flag / depth (AVS mutex via `XCnbrep700000f/10`) |

### 3.2 Step-data record (0x40 stride, base `[mgr+0x08]`)

| Offset | Meaning |
|---|---|
| `+0x00` | u32 FNV-1a name hash |
| `+0x08` | SSQ buffer ptr (32-byte-aligned alloc on heap `DAT_180466038`) |
| `+0x10` | read offset |
| `+0x14` | u32 length |
| `+0x18` | u32 alloc size (len rounded up to 32) |
| `+0x1C` | u32 last I/O result |
| `+0x20` | u32 **status**: 0 idle, 1 queued, 2 opening/size-query, 3 reading, 4 alt-load, 5 failed, 6 **complete** (buf may be null for empty/absent data), 7 cleanup, 8 finalized |
| `+0x24` | u32 refcount (register +1; 5 per song at boot — one per difficulty work item) |
| `+0x28` | FileReader ptr |
| `+0x30` | alt-load handle |
| `+0x38` | i32 priority (onInit writes −99 = `0xFFFFFF9D`) |
| `+0x3C` | free-list next |

Note (from the EOL-overrun research): the game stores `-1` as `entry_index`
for songs whose SSQ couldn't be registered and will read the bytes just before
the records array — any replacement logic must tolerate negative indices the
same way `fast_bootup::should_process_more` already does.

### 3.3 Name record (0xA0 stride, base `[mgr+0x28]`)

| Offset | Meaning |
|---|---|
| `+0x01` | device name (empty ⇒ `"local"`) |
| `+0x11` | path string (`data/mdb_apx/ssq/xxx.ssq`) |
| `+0x8E` | u8 offset of the *filename* within the path (used by onUpdate's `sota.ssq`/`thr8.ssq` compares) |
| `+0x8F` | u8 offset used by the alt-load provider lookup |
| `+0x90/+0x91` | extension length / extension chars (onInit force-fixes empty ext → `"ssq"`) |

### 3.4 Work item (12 bytes, vector at `actor+0x88..+0x90`)

`{ i32 entry_index, i32 difficulty (0..4), i32 mcode }` — 5 per song, all five
sharing one `entry_index` (register dedupes by name hash and refcounts to 5).
~1441 songs ⇒ ~7205 items, ~1466 distinct files.

### 3.5 `CheckStepDataActor` (extends the 20260724 field map)

| Offset | Meaning |
|---|---|
| `+0x20` | u32 lifecycle flags; done mask `0x24` (dispatcher gate) |
| `+0x58` | per-phase {u32 counter, u32 aux} pairs (cursor) |
| `+0x80/+0x82` | u16 phase count / current phase |
| `+0x88/+0x90` | work array begin/end |
| `+0xA8..+0xCC` | **10 × i32 global accumulators** (see §5.3) — copied to `*DAT_1806f14F8 + 0x30..0x54` at completion |
| `+0xD8` | ptr → u32 loading-percent display target (`counter*100/total_items`) |

### 3.6 `step::SsqReader` (stack-constructed in onUpdate)

`{ +0x00 vftable, +0x08 flag, +0x10 data ptr (= rec+0x08), +0x18 size
(= rec+0x14), +0x20 end-chunk ptr, +0x28 tempo chunk (type 1) ptr, +0x30 tempo
tick array, +0x38..+0x48 ms-converted tempo vector }` — `0x1801cbdc0` fills
+0x20..+0x48 by walking the chunk list (`ptr += *ptr` over
`{u32 len, u16 type, u16 mark}` headers; terminators `len==0`, `type==2`,
`mark==0xFFFF` — the exact walk `fast_bootup::ssq_chunk_list_walkable`
mirrors).

### 3.7 Analyze outputs (per side × difficulty — the cache payload core)

`Analyze(this, notes, measures, result, radar, mode, difficulty, option) -> u8`
(signature already documented and detoured in
`src/mods/note_types_expansion/hooks.rs`). Persistent outputs per call:

**`result` block — 14 × i32 (0x38 bytes), zeroed by the caller:**

| Index | Meaning |
|---|---|
| `[0]` | regular step count (jumps counted once) |
| `[1]` | freeze count (note records with any nonzero length field) |
| `[2]` | shock count (all-panel pattern rows) |
| `[3]` | unknown (never observed written in the boot path) |
| `[4]` | unknown count — feeds the `+0x12E` per-slot music-DB flag |
| `[5]/[6]` | min / max note time (field `+0x04` of the note records) |
| `[7]` | unknown |
| `[8..9]` | f64 **min BPM** (init DBL_MAX, filled by the BPM-duration walk, rounded) |
| `[10..11]` | f64 **core BPM** (the BPM whose active duration is longest — argmax over a BPM→duration rb-tree) |
| `[12..13]` | f64 **max BPM** (rounded) |

**`radar` block — 5 × i32** (groove-radar-shaped 5-tuple; filled by the
Analyze helpers). onUpdate consumes it only for the global accumulators
(§5.3).

**Return `u8`** — parse success (`isAnalyzable` in the game's own log line).

Nothing else escapes: the notes/measures vectors and the stack `player::Option`
are destroyed immediately after each call, and the SSQ buffer is freed after
the item's release. **Cache = {result[14], radar[5], ret} × 2 sides per work
item** (≈ 160 B/item ⇒ ~1.2 MiB for 7205 items), plus the per-file identity
(§7.2).

### 3.8 Music-DB entry writes (0x258 stride; slot index `idx = difficulty + side*5`)

Everything onUpdate writes into the entry found by mcode:

| Offset | Type | Value |
|---|---|---|
| `+0x94` | u16 | song-wide max BPM — max-accumulated from `(int)result[12..13]` across all 10 slots (skipped when 0) |
| `+0x96` | u16 | song-wide min BPM — min-accumulated from `(int)result[8..9]` (skipped when 0) |
| `+0x98 + idx*4` | i32[10] | per-slot max BPM |
| `+0xC0 + idx*4` | i32[10] | per-slot core BPM |
| `+0xE8 + idx*4` | i32[10] | per-slot min BPM |
| `+0x11A + idx` | u8[10] | has-shock flag (`result[2] > 0`) |
| `+0x124 + idx` | u8[10] | variable-BPM flag (`|maxBPM − minBPM| > DAT_180393F40`) |
| `+0x12E + idx` | u8[10] | flag from `result[4] > 0` (semantics unresolved) |
| `+0x1B0` | u8 | corruption flag — set when entry vfunc `+0x70`(side, difficulty) says the chart should exist but `ret == 0 ∨ (result[0]+result[2]) == 0`; accompanied by the `"INVALID SSQ"` log + `(*DAT_1806F2858)("ME1529", "MDX1529", "FILE CORRUPTION ERROR", mcode)` report |
| `+0x1B4 + idx*4` | i32[10] | **EX score** = `(result[0] + result[1] + result[2]) * 3` |

Charts that fail to load (status 5, null buffer) still take this path with the
caller's zeroed result block — zero BPMs (u16 accumulates skipped by the `!= 0`
guards), zero EX score, corruption flag if the DB expected the chart. A cache
replay of `{ret=0, zeros}` reproduces it bit-for-bit.

## 4. Boot Flow Detail

1. **`Application::onBoot` (`0x1800020B0`)** — creates the manager
   (`count=0x1000, open_cap=4/pump, device "local"`), mounts
   `/local/data → /data` (AVS, `XCnbrep700004B`), registers file-type
   callbacks, registers `data/arc/startup.arc` and **calls the blocking drain
   `0x1801FE380`** (precedent: the game blocks the boot thread on this loader
   with no rendering). Then creates the music DB from `musicdb.xml`, registers
   `data/arc/soundbanks.arc`, drains again.
2. **`CheckStepDataActor::onInit` (`0x180032030`)** — for each of
   `(dbEnd−dbBegin)/0x258` songs × 5 difficulties: build the path via
   `0x1801B43F0` (basename from entry vfunc `+0x08`; hardcoded split-file
   specials), register (`0x1801FEF30` → status 1 + pending queue; duplicate
   names just refcount++), write priority −99, push work item.
3. **Per frame (`0x180003020`)** — one pending-pump + one release-pump, then
   actor dispatch: message 0x102 → onUpdate iff `(flags & 0x24) == 0`.
4. **`onUpdate` per work item** (status gate `{0,5,6,8}`):
   - Build stack `SsqReader` over `{rec buf, rec len}`; if both nonzero, run
     the summarize walk (`0x1801CBDC0`).
   - For side 0 and side 1: zero the result block, construct a default
     `ddr::player::Option`, call **Analyze** (the NTX detour), then perform
     every write in §3.8 and the accumulator updates in §5.3.
   - `0x1801FF1B0(mgr, entry_index)` — queue the release (5th release frees
     the buffer).
   - `counter[phase]++`, zero later-phase counters, update the percent display
     `*(actor+0xD8) = counter*100 / ((end−begin)/12)`.
   - **Completion (inside the final item's processing):** copy
     `actor+0xA8..+0xCC` (10 dwords) → `*DAT_1806F14F8 + 0x30..+0x54`, set
     done flag (`+0x20 |= 4`, parents `|= 8`).

## 5. What Must Be Reproduced by a Cache Replay

### 5.1 Per (work item × side): the §3.8 writes

Replayable purely from the cached `{result, radar, ret}` + the work item's
`{difficulty, mcode}` + live music-DB lookups (by-mcode via `0x1801B7E80` /
`0x1801B4290`, both derivable from the resolved onUpdate body). Keying the
cache by **file + difficulty + side** (not mcode) makes it immune to
`musicdb.xml` edits/rearrangement — mcode mapping happens live at replay time.

### 5.2 The corruption branch

Replay the flag write and the `(*DAT_1806F2858)` report (operators rely on the
service-menu corruption error for genuinely broken charts). The `XCnbrep`
debug log line can be skipped or replayed — cosmetic.

### 5.3 Actor accumulators (`actor+0xA8..+0xB8`) and the global copy

> **Correction (implementation, 2026-08-24):** the accumulators are **per
> side, ten total** — not five. onUpdate's side loop advances its
> accumulator base by 5 ints each iteration (`local_228 += 5`), so side 0
> writes `+0xA8..+0xB8` and **side 1 writes `+0xBC..+0xCC`** (with the
> sota/thr8 special on each side's first two ints). The completion block
> copies all ten (`+0xA8..+0xCC`) to `*DAT_1806F14F8 + 0x30..+0x54`. The
> "+0xBC..+0xCC ride along zero" note below was wrong; the replay folds each
> side's radar block into its own 5-int window. This does not change the
> cache payload (still `radar[5]` per file×difficulty×**side**); it only
> affects which actor offsets the replay applier writes.

Per item × side, after Analyze:

```
if (file name at name_rec+0x11+*(name_rec+0x8E) == "sota.ssq")
    actor+0xA8 = max(actor+0xA8, radar[0])
if (== "thr8.ssq")
    actor+0xAC = max(actor+0xAC, radar[1])
actor+0xB0 = max(actor+0xB0, radar[2])
actor+0xB4 = max(actor+0xB4, radar[3])
actor+0xB8 = max(actor+0xB8, radar[4])
```

(The two hardcoded filenames look like normalization anchors for the first two
radar axes.) `+0xBC..+0xCC` are never written by this path — they ride along
zero-initialized in the completion copy to `*DAT_1806F14F8+0x30..+0x54`.
The cache must store the radar 5-tuple *and* the per-file basename so the
special cases replay exactly.

### 5.4 Completion mechanics

Two viable shapes (decide in design):

- **Full replication** — after replaying the last item, write the percent,
  perform the global copy, and set the done flags exactly as the original
  does. All fields are already documented (here + the 20260724 map).
- **Last-item-stock** — replay items `0..n−2` from cache (accumulating into the
  actor fields directly), leave the final item to load and process through the
  original; its completion branch then runs the global copy and done-flag walk
  stock. Costs one file read; saves replicating the completion block.

### 5.5 Release / manager end-state

Stock end-state after boot: every record refcount 0, buffers freed, name slots
cleared, slots on the freelist. Replaying `0x1801FF1B0` per processed work
item reaches the same end-state through the game's own machinery — including
for never-loaded entries, because the manager already has a first-class shape
for "complete with no data":

- **Status 6 with `buf==0 ∧ len==0`** is exactly what the stock pump produces
  for an empty file (`getSize()==0` → release reader, status 6, buffer stays
  null), and both pumps handle it: the pending pump drops status-6 entries
  without opening; the release path frees them without the error branch
  (`0x1801FECF0` only error-logs for statuses ∉ {6,8}).

So the cache-hit fast path is: flip still-queued records `1 → 6` (leaving
buf/len zero), replay the writes, queue the releases. Records the pump already
started (status 2/3 — up to `open_cap` per pump may be in flight before the
first onUpdate runs) should instead either (a) be released through the stock
cancel path (release of an in-flight entry → status 7 → pump → 8 → freed), or
(b) simply be processed stock and used to refresh the cache — self-healing and
zero extra machinery.

## 6. Threading & Pacing — Why It Takes 15.5 s Today

### 6.1 The pacing chain

- The pending pump issues **at most `mgr+0x70` = 4 new file opens per call**
  (requeues the rest), and the app tick pumps **once per frame**.
- The observed ~100 opens/s matches this cap × the boot loop's frame rate on
  the logging cabinet (remote-display 31 Hz vsync ⇒ ~124/s ceiling): the
  window is **open-cap × fps bound, not disk bound**. At a true 60 fps the
  ceiling is ~240/s (~6–7 s for 1466 files) — still pacing-bound on any SSD.
- The actual I/O runs on the manager's own worker thread (created in the ctor)
  through `agcs::File::me_io::FileReader`; state-machine transitions happen in
  the pump. Each file needs ≥2 pump rounds (open → size/read → complete), but
  transitions pipeline across the in-flight set, so throughput ≈ min(cap×fps,
  device speed).

### 6.2 Removing the pacing ("as fast as the disk allows")

Two complementary, low-risk levers, both inside stock semantics:

1. **Raise `mgr+0x70`** (one u32 write on the already-resolved
   `step_data_global_table` deref, applied while the boot actor is live and
   restored after). The cap is a plain ctor parameter (default 4 from
   `0x1801FD350`; onBoot never overrides it), not a tuned invariant — the
   manager is built for 0x1000 in-flight records. Memory note: every in-flight
   completed-but-unprocessed entry holds its whole file buffer; with
   fast-bootup's batch processing draining every frame the peak is
   open-rate × ~1 frame ≈ tens of MB at cap 64 — fine.
2. **Pump harder when starved.** The game's own blocking drain
   (`0x1801FE380`: `sleep(1 ms) → pump DAT_1806F2F60 → pump → finalize` until
   both queues empty) is called from `onBoot`, mode-switches, and the manager
   dtor — precedent that main-thread blocking drains are legal. For the
   loading screen, a **bounded** variant (drain for ≤N ms per frame from
   inside the onUpdate hook, mirroring the drain's exact call sequence, then
   yield so the percent bar renders and the frame loop keeps servicing
   watchdog/IO) gets disk-speed loading without a frozen screen.

With the cache active, cache-miss boots (first boot, new/changed songs) are
the only ones that load files at all — these levers make *those* boots fast;
cache-hit boots skip the window entirely.

### 6.3 The reads are AVS-backed (LayeredFS-visible)

The manager's device `"local"` maps to the AVS mount (`/local/data → /data`,
mounted in onBoot). Field evidence: our LayeredFS `avs_fs_open` hook observes
every boot SSQ open (the `judgement_offsets: song identity … (ssq open)` burst
in `log.txt` — 1466 lines, fed by
`per_song_judgement_offsets::override_hook::on_ssq_open` via the normalized
`mdb_apx/ssq/…` path). Consequences:

- LayeredFS chart replacement applies at boot ⇒ **cache identity must be the
  *resolved* file** (mod-folder override when present), not just the game
  path. The DLL already has this resolution machinery (`chart_length`'s
  LayeredFS-aware SSQ resolution).
- Our per-open hook overhead (path normalize + mod-folder probe) is on this
  hot path ~1466 times; skipping loads also skips that.
- The alt-load (ARC provider) path exists for archived files; on current
  installs SSQs are loose (proven by the open burst). Files that would resolve
  through an ARC can simply be treated as cache misses (processed stock).

## 7. Cache Design Space (for the planning phase)

### 7.1 Interception shape — a refactor of `fast-bootup` (maintainer decision: same mod, same id/toggle, no new mod)

The existing `check_step_data_update` detour remains the single hook point;
its loop gains a cache-hit fast path:

- **onInit stays stock** (registration + work list). No second detour needed
  for correctness; the ≤`open_cap` reads the pump issues before our first
  onUpdate call are handled per §5.5(b).
- **onUpdate detour**: per work item, cache hit ⇒ replay (§5), release, advance
  cursor, never call the original; cache miss ⇒ existing gated batch path
  (readiness walk + bounds gates all carry over), then **capture** the
  analyzer outputs for the cache. Capture options: (i) read the music-DB
  fields back after `hook.call()` — but the result/radar blocks are stack
  locals, so radar and `result[4]` aren't recoverable from the DB alone; or
  (ii) capture at the **Analyze boundary** — which is already detoured by NTX
  (one-detour-per-target!) — so capture must go through a shared dispatcher on
  the existing NTX detour (judge_hook model: NTX injection + boot capture as
  subscribers), or read the result/radar pointers (args R9/[RSP+0x28]) from a
  registered subscriber. This is the main structural decision for design.
- All-items-processed in a single frame on full cache hit: 7205 replays of
  pure arithmetic + DB writes — no per-frame cap needed on the hit path (keep
  a cap on the miss path as today).

### 7.2 Cache format & keying

- One bin file next to `mod-config.json` (versioned header, e.g.
  `data_mods/_cache/step_data/v1.bin` — matches the shader-synthesis cache
  precedent).
- **Header invalidators:** cache-format version, DLL version, gamemdx build
  (the analyzer itself could change), and the split-file specials table hash
  (it's hardcoded per-build in `0x1801B43F0`).
- **Per-file key:** registered game path + resolved backing file (mod-folder
  override or stock) + file size + mtime (via `avs_fs_lstat`, or host stat on
  the resolved OS path — the LayeredFS layer already produces both). Miss on
  any component ⇒ stock load + re-capture for that file only.
- **Per-file payload:** basename (for the sota/thr8 accumulator specials) +
  for each of the ≤5 difficulties present as work items × 2 sides:
  `{result[14 i32], radar[5 i32], ret u8}`.
- NTX does **not** need to be a cache invalidator (§8.1); LayeredFS chart
  swaps are covered by per-file identity.

### 7.3 What ultrafast boot does *not* touch

Gameplay/course chart loading (`FUN_18005AEA0` / `FUN_1801DF160` callers),
`chart_length`, song-select SSQ opens, NTX's real (gameplay) injection — all
downstream of different Analyze call sites and unaffected.

## 8. Cross-Mod Interactions

### 8.1 NoteTypesExpansion — safe, and boot gets cheaper

NTX's Analyze detour runs the **original first**, then injects mine notes into
the notes vector. At boot the counts/radar were already computed by the
original *before* injection, and onUpdate frees the notes vector immediately —
so boot-time injection affects **nothing persistent** (log evidence: hundreds
of boot-time `NoteType 'mines': injected …` lines, all discarded). Skipping
Analyze for cache hits therefore changes no observable state; it also skips
NTX's boot-time sidecar churn (whose staleness needed the 2026-08-23
chunk-less-analyze reset). NTX's gameplay behavior is untouched (different
call sites). Only constraint: **one detour per target** — any boot-capture at
the Analyze boundary must share NTX's existing detour via a dispatcher.

### 8.2 Per-Song Judgement Offsets

The boot `on_ssq_open` identity burst disappears on cache-hit boots. Harmless:
at boot `LOCKED_CODE` is meaningless (overwritten at the first real stage
load), and the mod's musicdb bootstrap crawl reads `startup.arc`/musicdb
directly, not the SSQ stream.

### 8.3 fast-bootup's existing safety gates

Both prior crash classes remain relevant on the **miss** path and their gates
carry over unchanged: the bounded chunk-walk readiness gate (async-load race)
and the done-flag + cursor-bounds gates (EOL overrun). The cache-hit path is
immune to both by construction (no buffer walks; cursor advanced by us with
the same bounds).

## 9. Open Questions / Measurements for Design

1. **`result[4]` semantics** (→ `+0x12E` flag) — not yet identified (filled
   inside the Analyze helpers). Cache stores it verbatim either way.
2. **Radar block consumer** — who reads `*DAT_1806F14F8+0x30..+0x54` (radar
   normalization at song select?). Doesn't affect the replay, but would
   confirm the sota/thr8 anchor theory.
3. **Real-cabinet pacing split** — confirm on 60 Hz hardware that raising
   `mgr+0x70` alone reaches disk speed (instrument open→complete latency per
   file from the LayeredFS hook) before adding the bounded drain.
4. **Percent-bar UX on full cache hit** — the bar jumps 0→100 in one frame;
   decide whether that's fine (it is on a 1-frame boot) or worth a fake ramp.
5. **Analyze-boundary capture ABI** — verify the `result`/`radar` stack args
   survive to detour-return time on both builds (they're caller-owned locals;
   they do — but confirm no build reuses the slots).
6. **Manager pump concurrency** — the 20260721 race proved buffer visibility
   lags status on weak-memory translation; all *static* pump call sites are
   main-thread, but the manager owns a worker thread whose exact duties
   (device I/O only vs. also pumping) weren't pinned down. The cache-hit
   design never touches in-flight records' buffers, and status flips are
   confined to queued (status-1) records, so this only matters if we add the
   bounded drain — which mirrors the stock drain call sequence exactly.

## 10. Cross-Version Notes

- Structural spot-checks on `gamemdx_20260616.dll`: manager ctor defaults
  byte-identical (`0x1801FCCC0` [0616] = `{0x800, 4, −1, 1, "local",
  "../../data/dx9/", "default"}` — same open cap 4), single
  `data/mdb_apx/ssq/` and `../../data/dx9/` string sites. The 20260721
  research already established the record/actor offsets are stable across
  20260616 → 20260721.
- Implementation must derive **everything** from the already-resolved anchors
  (no new hardcoded addresses): `check_step_data_vtable` gives onInit
  (vtable[4]) and onUpdate (vtable[6]); the release fn (`0x1801FF1B0`), music
  DB global (`DAT_1806F2D78`), by-mcode lookup (`0x1801B4290`), error reporter
  (`DAT_1806F2858`), global config (`DAT_1806F14F8`), threshold
  (`DAT_180393F40`), and the manager pumps (`0x1801FDBF0`/`0x1801FE150`, e.g.
  via the tiny drain function's body) are all RIP-relative/call-rel32 decodes
  from the onUpdate/onBoot bodies using the existing scanner primitives.
- Per steering policy, verify every derivation on both supported builds before
  shipping.

## 11. Implementation status (2026-08-24)

The ultrafast-boot refactor of `fast-bootup` is implemented and locally
validated on gamemdx 20260721 (CrossOver, ~1499 files / 7305 items):

- **Capture (first boot):** wrote `data_mods/_cache/step_data/v1.bin` =
  1,262,453 bytes (1499 entries), pass ~2.5 s, boots to TITLE, 0 exceptions.
- **Parity (temporary Step-6 diff, since removed):** across the full library,
  every freshly-captured payload matched the cache **0 field mismatches** —
  the cache stores exactly what a stock analysis produces.
- **Replay (cache-hit boot):** 7304/7305 items replayed, 1498 records flipped,
  boot pass **~42 ms with 1 SSQ open** (the final stock item); a temporary
  entry-dump A/B confirmed the replayed music-DB entry fields (max/core/min
  BPM, EX, shock, variable-BPM, song-wide BPM) are **byte-identical** to a
  stock boot; 0 `INVALID SSQ` / `ME1529` (D9 respected).
- **Mutation drills:** touch one SSQ ⇒ exactly that song re-analyzed (1498
  verified); corrupt the header ⇒ WARN + full rebuild; delete the bin ⇒ full
  rebuild; disable the mod ⇒ stock slow boot (~1466 SSQ opens). Cache size is
  stable across replay boots (the writer unions partial re-captures of an
  unchanged file rather than truncating).

Derived addresses (`signatures.rs::derive_ultrafast_boot`, all decoded from
the resolved onUpdate body, confirmed on 20260721): `music_db_global`
`+0x6F2D78`, `variable_bpm_threshold` `+0x393F40`, `find_music_by_mcode`
`+0x1B4290`, `step_data_release` `+0x1FF1B0`. The onUpdate anchors are present
on 20260616 (onUpdate = `FUN_180032c90`); re-verify the four disp32/call
decodes there before shipping to that build.
