# Plan — task-02 cache-format

Status: Approved 2026-08-24 (verified upstream approval chain; auto mode)

## Test scenarios (written first, against a `todo!()` skeleton)

1. **Round-trip:** build a `CacheFile` with: entry A (file identity, 10
   payloads incl. f64 bit-pattern results), entry B (absent identity, 1
   payload), entry C (file identity, 0 payloads). serialize → parse (matching
   stamp/size) → equals original.
2. **Truncation sweep:** serialize the round-trip fixture; for every prefix
   length `0..len-1`, parse must return `Empty{..}` — never panic, never
   `Loaded`.
3. **Invalidators:** bad magic, version≠1, stamp mismatch, size mismatch each
   ⇒ `Empty` with a distinct reason string.
4. **Absurd counts:** hand-craft headers/entries with entry_count > cap,
   payload_count > 10, string len > cap ⇒ `Empty`, no huge allocation
   (bounded by remaining-bytes checks).
5. **Trailing garbage:** valid file + extra bytes ⇒ `Empty` (strict length)
   — strictness keeps corruption detection simple.

## Implementation approach

`src/mods/fast_bootup/cache.rs`, dependency-free, csv.rs house style:
- Types per task spec (`CacheFile/FileEntry/Identity/SlotPayload`,
  `CacheLoad::{Loaded, Empty{reason}}`).
- `struct Reader<'a>{buf, pos}` with checked `u8/u16/u32/u64/i32/bytes/string`
  reads returning `Option`; `parse` is a straight-line decode; every count
  checked against caps AND remaining bytes.
- `serialize` mirrors field order; used by the Step 5 writer.
- Constants: `MAGIC=*b"DDRSSQC1"`, `FORMAT_VERSION=1`,
  `MAX_ENTRIES=65536`, `MAX_PAYLOADS=10`, `MAX_STR=1024`.
- Register `pub mod cache;` in `fast_bootup/mod.rs`.
