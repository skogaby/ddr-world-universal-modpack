# Progress — non-native-os-support

Updated: 2026-08-19
Status: Done — implemented, live-validated, docs written. Uncommitted (maintainer commits).
NEXT ACTION: none — feature complete, including the 2026-08-19 native-VC-1
addendum (`research/native-vc1-decode-path-b.md`). (If resuming for follow-up:
read `summary.md`, then `docs/native_wm_runtime_bottle_setup.md` for the
movie-decode state of the art, then `design/detailed-design.md` and
`research/movie-player-re.md`.)

Resume protocol: `summary.md` → `design/detailed-design.md` (decisions D1–D5) →
`research/movie-player-re.md` (all addresses/state-machine facts) →
`research/native-vc1-decode-path-b.md` (Path B supersession).

## Done

- 2026-08-19 (Path B session): native VC-1 decode working end-to-end — bottle
  WM runtime + `mfplat_vih_fix` detour (Wine mfplat FOURCC-subtype bug) +
  fallback-mode path absolutization. Attract-demo movies visually confirmed.
  Authoritative record: `docs/native_wm_runtime_bottle_setup.md`.

- RE: full movie-player object chain decompiled in Ghidra (`gamemdx_20260616.dll`,
  functions renamed + plate comment on the stub target); `DShowPlayer::BuildGraph`
  (`0x18023AE40`) identified as the sole DirectShow entry (only `CLSID_FilterGraph`
  xref); per-slot null-safety audit of the player vtable; cross-version verify on
  `gamemdx_20260324.dll` (`0x180256EB0`, byte-identical AOB, unique on both).
- Signature: `movie_build_graph` added to `src/core/signatures.rs` (60 bytes,
  no wildcards, function-entry anchored).
- Mod merge: `git mv src/mods/raw_socket_network_fix.rs → non_native_os_support.rs`;
  struct `NonNativeOsSupportMod`, id `non-native-operating-system-support`; two
  independent sub-fixes (network promotion unchanged + movie-graph stub);
  `mods/mod.rs` + `lib.rs` registration updated; README mod table + config example
  and AGENTS.md entry-points row updated.
- Stub v1 (error return) — REJECTED after live test: soft-locked the attract demo
  (STATUS stuck at "opening"). See design D3.
- Stub v2 (fake success: player state +0x8 = 3, opened +0x14 stays 0, return 0) —
  live-validated.
- Build gates: `cargo check` clean → `cargo fmt` → `./build.sh` clean.

## Deploy & test log (bottle `bemani`, game `MDX:J:F:A:2026061600`, base 0x6FFFFB3C0000)

| Time (2026-07-21) | Build | Result |
|---|---|---|
| 15:09 | pre-fix baseline | CRASH in attract demo: winmm←devenum←quartz←gamemdx +0x23B0F0 (RenderFile). Log archived at bottle `log_crash_20260721_1509.txt` |
| 15:52–15:56 | stub v1 (error return) | Both sub-fixes resolved+installed (stub @ base+0x23AE40 ✓); network promotion fired; but demo run invalidated by an accidental double-launch (wineserver wedged pre-D3D; full `wineserver -k` reset afterward) |
| 15:57–16:04 | stub v1, clean run | No crash; suppression fired at former crash point; **SOFT-LOCK**: demo assets loaded 15:59:20 then scene 16 never advanced (5+ min). Log archived at bottle `log_softlock_run.txt` |
| 16:15–16:20 | stub v2 (state=3) | **PASS**: promotion 16:15:59 (`musicdata_load` flowed); attract demo 16:16:31 → suppression 16:16:33 → demo completed 16:17:22 (~50 s, normal) → attract loop cycled 3× (16:18:34, 16:19:36); `EXCEPTION_ACCESS` count 0; one-shot log exactly once. Game killed manually after validation |

## Deviations & open questions

- Cheap experiments (`-ddrsd`, `-audiodummy`) skipped as moot (design D5).
- Handoff's "missing movie file is a tolerated state" assumption was disproved —
  the tolerated no-movie path is at the actor level; documented in research doc.
- Residual (cosmetic, accepted): within one `Dx9Movie` the double-buffer swap
  never occurs for stubbed players; each song constructs a fresh object tree so
  nothing observable. Movie-status machine reaches 6/7 and idles.
- Not re-tested on real Windows hardware (no cabinet access this session). The
  stub only runs when the mod is enabled; on real hardware the mod should be
  disabled if music videos are wanted (documented in README).

## Key facts for a cold resume

- Stub target: gamemdx `DShowPlayer::BuildGraph`, AOB `movie_build_graph`,
  entry-anchored; detour writes `this+0x8 = 3u32`, leaves `this+0x14 = 0`,
  returns 0, never calls original.
- Never list `movie_build_graph` in `required_signatures()` — lenient contract
  (design D1).
- Old mod id `raw-socket-network-fix` is dead; new id
  `non-native-operating-system-support` (clean rename, no shim).
- Ghidra: project `DDRWorld_Ghidra`, program `gamemdx_20260616.dll` has the
  renamed functions + plate comment; cross-version program `gamemdx_20260324.dll`
  (opens as `gamemdx.dll`).
