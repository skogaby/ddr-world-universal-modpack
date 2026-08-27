# Rough Idea — Timing Offsets

A new mod in the DDR World hook DLL that exposes the game's built-in **timing-offset
record** as configurable values, ported from the 32-bit `patches.js` "sound offset"
hex hack and the fuller reverse-engineering writeup in `docs/hex_edit_porting.md`,
Hack 4. Target: the 64-bit builds (20260324 primary, 20260526 = cabinet build).

## What the game does (per the RE doc — TO BE RE-VERIFIED THIS SESSION)

At sound/input subsystem init, the game builds a **timing-offset record** (5 fields,
0x14 bytes) and publishes each field into a runtime config map keyed by name. There
is a table of ten 0x14-byte records (one per cabinet/PC preset); the selected record
is copied into the per-run struct.

| Record off | Engine config key | Type | Record-0 default | Meaning |
|---|---|---|---|---|
| `+0x00` | `SOUND_OFFSET` | i32 | 87 | audio sync; larger = audio later |
| `+0x04` | `INPUT_OFFSET` | i32 | 28 | input/judge timing offset (the "SSQ"/judge offset) |
| `+0x08` | `RENDER_OFFSET` | i32 | 17 | render/display latency compensation |
| `+0x0C` | `BOMB_FRAME_OFFSET` | i32 | 0 | shock-arrow ("bomb") frame timing |
| `+0x10` | `HIGH_PRECISION_INPUT` | bool | 1 (on) | sub-frame input timestamping |

Documented anatomy (build 20260324):
- **Timing init / publisher**: `FUN_18002bbd0` (anchored by `"ConfigBank.csv"`,
  `"Timing Init: %d"`).
- **Record builder**: `FUN_180012f50` (record 0's first 16 bytes from a `.rdata`
  constant table at `0x180358960`; records 1–9 + all `+0x10` bool bytes inline).
- **Config setters**: `FUN_1801acbf0` (set int), `FUN_1801acb50` (set the
  `HIGH_PRECISION_INPUT` bool); getter `FUN_1801acd50`. Keys hashed (FNV-1a) into the
  map at `DAT_1806ebcf0`.
- **HIGH_PRECISION_INPUT** is read once into input-manager state `DAT_1806ebc70 +
  0x1261`; the per-button event recorder `FUN_1800229e0` snaps the event timestamp to
  the per-frame clock when it's OFF, keeps the sub-frame timestamp when ON. Shipped ON;
  the lever is forcing it OFF.

The original `patches.js` only exposed `SOUND_OFFSET` (default 87, range 0–1000); this
mod aims to expose the whole record.

## Recommended hook-DLL approach (from the RE doc — two clean levers)

1. **Patch the defaults** — AOB-scan the record builder, rewrite the `.rdata` rec0 ints
   and/or the inline imm32s. Static value, matches `patches.js` semantics.
2. **Write the live state / re-set via the config-map setter** — after the subsystem
   inits, re-set the published int offsets via the same setter the game uses
   (`FUN_1801acbf0(key, value)` analog) keyed by `"SOUND_OFFSET"` etc., and write the
   `HIGH_PRECISION_INPUT` live byte (`DAT_1806ebc70 + 0x1261`) which the per-tick reader
   picks up live. The more flexible path for a config-driven / runtime-toggle mod.

AOB anchor for the record builder (both builds, per doc):
`C7 45 ?? 57 00 00 00 C7 45 ?? 1C 00 00 00` (record-1 SOUND/INPUT inline pair), or
anchor via the `"Timing Init: %d"` / `"ConfigBank.csv"` strings → init fn → builder call.

## Re-verification mandate (this session)

The maintainer explicitly wants all of the above RE findings — function roles,
addresses/offsets, the record layout, the config-map setter signatures, and the
HIGH_PRECISION_INPUT live-state location — **re-verified fresh against the binaries
this session** (Ghidra MCP: `gamemdx_20260324`, `gamemdx_20260526` [cabinet build],
and the 32-bit `gamemdx_x86_20250610_02`). Per the project's own handoff lesson
("re-verify every load-bearing claim in an RE handoff") and the memory-note caveat
that the running build doesn't match the Ghidra DBs offset-for-offset, the binary is
the sole source of truth — the documented absolute addresses are provenance, not gospel.

## Reference

- `docs/hex_edit_porting.md` → "Hack 4 — Timing Offsets" (full 32-bit + 64-bit anatomy,
  record layout, setter/getter functions, patch-site table for both builds), and the
  cross-version summary table at the end.
- Memory note `timing-offsets-table` (project memory) — condensed version of the above.
- Completed PDD example to mirror: `.agents/planning/20260612-center-arrows-single/`.
- Existing mod patterns: `src/mods/timer_freeze.rs`, `src/mods/premium_free.rs`
  (binary patches + per-player option), `src/mods/center_arrows_single.rs` (recent
  AOB-resolved hook + custom option). Config schema: `src/mods/config.rs`. Signatures:
  `src/core/signatures.rs`.
