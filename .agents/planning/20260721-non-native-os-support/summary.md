# Summary — `non-native-os-support`

> **Status: DONE** — implemented and live-validated on the Macbook + CrossOver
> install (2026-07-21): the game boots ONLINE **and** survives background-movie
> songs (the attract demo, which auto-plays a movie song, now loops indefinitely
> where it previously crashed within seconds). Build gates green. Uncommitted
> (maintainer commits themselves). See `progress.md` for the live record.
>
> **2026-08-19 addendum:** the "movies must stay suppressed" conclusion is
> superseded — stock VC-1 movies now render natively under CrossOver via
> fallback mode + the native WM runtime + `services/mfplat_vih_fix.rs`. See
> `research/native-vc1-decode-path-b.md` and
> `docs/native_wm_runtime_bottle_setup.md`.

## What this is

The (unreleased, same-day) `raw-socket-network-fix` mod broadened into a single
**"Non-Native OS Support"** mod — the bundle of Wine/CrossOver-only workarounds —
plus a **new second sub-fix** that stops DDR World crashing when a song with a
background movie / music video plays under CrossOver/Wine on macOS:

- **(a) Network-status promotion** (existing): `arkmdxbio2!arkGetNetworkStatus`
  detour, CHECKING(4)→ONLINE(5) only. Unchanged.
- **(b) Movie-graph stub** (new): detour on gamemdx `DShowPlayer::BuildGraph` —
  the binary's only DirectShow entry point (`CoCreateInstance(CLSID_FilterGraph)`
  → `IGraphBuilder::RenderFile`, whose Wine implementation faults in builtin
  `winmm` during audio-renderer enumeration). The stub never calls the original:
  it fakes the success epilogue's one observable side effect (player state dword
  `+0x8 = 3`, "opened") and returns 0. The game's movie-status pollers proceed
  (an error-returning stub soft-locks the attract demo — learned live), the
  per-frame paths stay on their guarded early-returns (the `opened` byte stays
  0), and the song plays with a static background.

The sub-fixes resolve/install/self-disable **independently**; `is_active()` is
their union; a missing movie signature can never take down the network fix
(`required_signatures()` deliberately stays `&[]`).

Mod id: **`non-native-operating-system-support`** (clean rename from
`raw-socket-network-fix`, which never shipped; no back-compat shim).

## Artifacts

```
.agents/planning/20260721-non-native-os-support/
├── research/movie-player-re.md   # Full RE record: crash chain, object model, vtable audit,
│                                 #   status machine, the soft-lock lesson, AOB derivation
├── design/detailed-design.md     # Decision record D1–D5 (merge shape, stub level, stub
│                                 #   semantics, config migration, skipped experiments)
├── progress.md                   # Deploy & test log (crash baseline → v1 soft-lock → v2 pass)
└── summary.md                    # This file
```

Sibling record for sub-fix (a): `.agents/planning/20260721-raw-socket-network-fix/`.

## Code touched

- `src/mods/non_native_os_support.rs` (git-mv of `raw_socket_network_fix.rs`) —
  the merged mod, `NonNativeOsSupportMod`
- `src/core/signatures.rs` — new `movie_build_graph` AOB (60 bytes, no wildcards,
  function-entry anchored; unique on builds 20260616 `0x18023AE40` and 20260324
  `0x180256EB0`)
- `src/mods/mod.rs`, `src/lib.rs` — registration rename
- `README.md` (mod table + config example), `AGENTS.md` (entry-points row)
- Ghidra project `DDRWorld_Ghidra` / `gamemdx_20260616.dll`: movie-chain functions
  renamed (`DShowPlayer_BuildGraph`, `DShow_Open`, `Dx9Movie_*`, `agcs_Movie_ctor`,
  `DShowPlayer_OpenFile`) + plate comment on the stub target

## Validation (2026-07-21, bottle `bemani`, MDX:J:F:A:2026061600)

Boot ONLINE (promotion logged, `musicdata_load` flowed) → attract demo entered →
movie graph suppressed at the exact former crash RA (`+0x23B0F0`) → demo song
played through (~50 s) → attract loop cycled 3× → zero exceptions. Both the
crash **and** the soft-lock from the first stub iteration are fixed.

## Areas that may need refinement

- **Movies are gone under the mod, even on real Windows.** The stub can't tell
  Wine from Windows; on a real cabinet the mod should be disabled (or a future
  OS-detection gate added) if music videos are wanted. Documented in README.
- **Status machine idles at 6/7** ("ready"/"playing") rather than ever reporting
  a movie end. No observed consumer cares (songs end by chart, attract movies
  loop), but a future mode that waits for movie completion would need the stub
  to also fake an end state.
- **arkGetNetworkStatus ABI** — same note as the sibling record: re-confirm on a
  different arkmdxbio2 build if one ships.
