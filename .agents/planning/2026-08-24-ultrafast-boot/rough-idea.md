# Rough Idea — Ultrafast Boot

Refactor the existing `fast-bootup` mod (same mod id, same toggle — **not** a new
mod) so that the boot-time SSQ analysis pass is cached:

1. **First boot (cache build):** the game loads and analyzes every SSQ chart as
   stock (with fast-bootup's existing batching). We capture the analyzer
   outputs the game computes for each (file × difficulty × side) — data that is
   NOT available in `musicdb.xml` (BPM min/core/max, note counts → EX score,
   shock/variable-BPM flags, groove-radar values, corruption state) — and
   persist them to a bin file.
2. **Subsequent boots (cache replay):** for every chart whose backing file is
   unchanged, skip both the file read and the re-parse entirely — inject the
   pre-computed values into the game's in-memory music DB and actor
   accumulators, and release the loader entries through the game's own
   machinery. Changed/new charts fall back to the stock load+analyze path and
   refresh the cache.
3. **Eliminate the per-frame loading pacing:** for whatever does still need to
   load (first boot, cache misses), remove the loader's artificial pacing (the
   4-opens-per-pump × once-per-frame cap) so files load as fast as the disk
   allows, in effect.

Measured stakes (cabinet log 2026-08-24, ~1441 songs, fast-bootup ON): the SSQ
window is ~15.5 s of a ~28 s boot (≈55 %). Cache-hit boots should eliminate it
almost entirely; cache-miss boots should shrink it to true disk speed.

Full RE backing this idea: `docs/ultrafast_boot_research.md`.
