# RE: DShowPlayer seek/rate surface (background-movie sync)

Ghidra findings, 2026-08-20, programs `gamemdx_20260616.dll` (primary; addresses
file-relative to `0x180000000`) and `gamemdx_20260721.dll` (cross-check).
Extends `.agents/planning/20260721-non-native-os-support/research/movie-player-re.md`
— and **corrects two of its vtable labels** (§3).

## 1. COM interface slots pinned (GUID-verified)

BuildGraph step-4 QIs, in order (20260616 IID data addresses):

| Player offset | Interface | IID (read from `DAT_1802dc*`) |
|---|---|---|
| +0x50 | `IMediaControl` | `56A868B1-0AD4-11CE-B03A-0020AF0BA770` |
| **+0x58** | **`IMediaSeeking`** | `36B73880-C2C8-11CF-8B46-00805F6CEF60` |
| +0x60 | `IMediaEventEx` | `56A868C0-0AD4-11CE-B03A-0020AF0BA770` |
| +0x68 | `IBasicAudio` | `56A868B3-0AD4-11CE-B03A-0020AF0BA770` |

Epilogue details (both builds):

- `IMediaSeeking::GetCapabilities` (vtbl +0x18) is checked for bit 0
  (`AM_SEEKING_CanSeekAbsolute`); on failure the interface is **Released and
  +0x58 nulled** — a live player can legitimately have a null IMediaSeeking.
- `+0x40` (audio state) = 3 iff +0x58 survived; request flag bit 2 (`+0x17`)
  = "require seekable": BuildGraph hard-fails `0xC0260001` if set and +0x58
  is null.
- When seekable, the epilogue calls the player's **own native seek** (vtbl
  +0x58) with position 0 — **absolute seeking is exercised on every stock
  movie open**.
- The graph opens **Paused** (`IMediaControl::Pause`, command `+0x0C = 1`);
  the actor's play command flips it to Run inside get-frame.

## 2. The game has native seek plumbing we can call directly

`FUN_18023aad0` (player vtbl **+0x58**), verified decompile:

```c
plVar1 = *(longlong **)(param_1 + 0x58);          // IMediaSeeking
if (plVar1 != NULL) {
    pos = param_2;                                 // 100ns LONGLONG
    zero = 0;
    (*vtbl[0x70])(plVar1, &pos, 1 /*AbsolutePositioning*/, &zero, 0 /*NoPositioning*/);
}
```

`IMediaSeeking` vtbl +0x70 = `SetPositions` (slot 14). Default time format is
`TIME_FORMAT_MEDIA_TIME` (100 ns) and the game never changes it ⇒ pass
`t_ms * 10_000`. Null-guarded — safe to call unconditionally on a live
player. **The sync engine should call this method rather than raw COM**: it
inherits the game's own semantics and null guard.

`SetRate` = `IMediaSeeking` vtbl **+0x88** (slot 17). The game NEVER calls it
(no rate plumbing exists) — we'd be the first caller; live probe required.
*Live result (Windows cabinet, 2026-08-23):* `SetRate(1.75)` on the freshly
built, still-**paused** WMV graph returns **E_INVALIDARG (0x80070057)**
(GetRate hr=0, readback 1.0). Running-state acceptance still untested there
— the two-stage application (paused attempt → running retry) discriminates
it per song; on Wine both states return the silent-no-op S_OK.
`GetDuration` = vtbl +0x50 (slot 10) for clamping.

## 3. Corrections to the 2026-07 RE doc's vtable table

The old doc's "+0x48 seek / +0x58 volume" labels are **swapped**:

- **vtbl +0x48** (`FUN_18023ad60`) writes the `+0x10` float = **deferred
  VOLUME**. get-frame consumes it: converts via a dB mapping and calls
  `IBasicAudio::put_Volume` (+0x68 interface, vtbl +0x38), then resets
  `+0x10 = -1.0`.
- **vtbl +0x58** (`FUN_18023aad0`) = **the native absolute seek** (§2).

## 4. Completion / loop semantics (D9 resolved)

Event pump `FUN_18023b620`, called from get-frame every frame
(`WaitForSingleObject` on the media-event handle at `+0x70`, drains
`IMediaEventEx::GetEvent` vtbl +0x40 / `FreeEventParams` +0x60):

- **EC_COMPLETE (code 1) + loop flag `+0x16` set** (request flag bit 0) →
  after the drain, native **seek to 0** (vtbl +0x58) — the game loops movies
  by absolute seek.
- EC_COMPLETE without loop, or EC_USERABORT (2) / EC_ERRORABORT (3) →
  player stop command (vtbl +0x28 → command 4 → `IMediaControl::Stop` in
  get-frame).

**D9 resolution:** read the captured player's `+0x16` at open and mirror
stock — loop set ⇒ map positions modulo duration; loop clear ⇒ clamp (a
seek at/past the end just lets the stock completion path stop the movie,
which is already stock behavior for movies shorter than the song).

## 5. Threading

Everything COM on this player is serialized on one thread: get-frame (per
frame, actor update) runs the command dispatch (`Run`/`Pause`/`Stop`), the
deferred volume, and the event pump (including the loop-wrap seek).
`song_reset::on_song_reset` notifications fire on the same game update
thread ⇒ our seeks are naturally serialized with the game's own COM usage.
BuildGraph (and our capture) also runs there.

## 6. Player state model (for capture bookkeeping)

- `+0x08` state: 0 closed · 2 running · 3 opened/not-running (get-frame
  refreshes it from `IMediaControl::GetState` each frame).
- `+0x0C` command: 1 pause · 2 run · 4 stop (dispatched in get-frame).
- `+0x14` opened byte: 1 only after a REAL graph build (stays 0 for faked
  epilogues) — a natural "sync engine may touch this player" gate.
- Double-buffered players: the `DShow` wrapper (`FUN_180232ef0`) allocates
  TWO DShowPlayers; `DShow_Open` (`0x180232da0`) swaps front/back on each
  open. Capture keys off the BuildGraph hook's `this` (per-call, so always
  the player being opened) plus teardown awareness: `DShowPlayer_OpenFile`
  (vtbl +0x38) tears down before rebuilding, and `FUN_18023b270` is the
  teardown (releases all COM pointers).

## 7. Cross-build verification

The existing `movie_build_graph` AOB matches once on `gamemdx_20260721.dll`
at `0x18024a780`; its decompile is structurally identical to 20260616 —
same request-flag extraction, same QI order and slots (+0x50..+0x68), same
GetCapabilities/release logic, same native seek-to-0, same epilogue writes.
Object layout and vtable slot +0x58 are stable across builds. **No new AOB
signatures are required** — the sync engine derives everything from the
already-scanned `movie_build_graph` hook (`this` capture) and fixed struct
offsets.

## 8. Stock movie corpus (ffprobe, CrossOver install)

386 `.wmv` files under `data/mdb_apx/movie/`:

- **All video-only — zero files carry an audio stream.** No audio renderer
  in any graph ⇒ no audio-rate constraint on `SetRate`; the only rate
  consumer is the video pipeline.
- VC-1, mix of 1280×720@60, 1280×720@30, 640×360@30; sampled durations
  107–141 s.

## 9. Remaining live-only unknowns (front-load in the plan)

1. `SetRate` acceptance + visual behavior on the real graph (Windows quartz
   + WM ASF Reader; Wine quartz). The game's custom renderer presents
   samples against the graph clock — rate-scaled delivery should follow,
   but only a cabinet probe proves it.
2. Seek granularity/latency on VC-1 ASF (keyframe snapping; index quality
   of stock files).
3. Decode headroom at 175 % (720p60 VC-1).
4. Wine quartz seek robustness mid-run (CrossOver fallback mode,
   best-effort per D8).
