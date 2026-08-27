# Research — DDR World background-movie DirectShow player (`gamemdx`)

> RE record for the `non-native-os-support` movie-crash fix. All Ghidra addresses
> are file-relative to image base `0x180000000`, program `gamemdx_20260616.dll`
> (game version `MDX:J:F:A:2026061600`) in the `DDRWorld_Ghidra` project unless
> stated otherwise. Cross-version addresses verified on `gamemdx_20260324.dll`.
> Investigated 2026-07-21.

## The crash

Under CrossOver/Wine on macOS, the game hard-crashes (`EXCEPTION_ACCESS_VIOLATION`)
the moment a background movie (`.wmv` under `data/mdb_apx/movie/`) starts —
including autonomously ~15 s into the attract-mode demo loop. Crash callstack
(spice2x stackwalker, run of 2026-07-21 15:09, gamemdx base `0x6FFFFB3C0000`):

```
winmm                       ← faulting frame (Wine builtin, waveOut caps enumeration)
devenum                     ← audio-renderer device enumeration
quartz ×4                   ← DirectShow graph builder / intelligent-connect
gamemdx +0x23B0F0           ← RA after IGraphBuilder::RenderFile call   (in BuildGraph)
gamemdx +0x232DB3           ← RA in DShow::Open                          (after open vcall)
gamemdx +0x215F44           ← RA in Dx9Movie helper                      (after DShow::Open vcall)
gamemdx +0x215D9F           ← RA in Dx9Movie::Open                       (after helper call)
gamemdx +0x215C65           ← RA in Dx9Movie ctor                        (after Open call)
gamemdx +0x2165B9           ← RA in agcs::Movie ctor                     (after Dx9Movie ctor call)
gamemdx +0x07C68E           ← RA in movie actor                          (after agcs::Movie ctor call)
gamemdx +0x21D8E4/+0x21EF9B/+0x21D93B  ← actor-tree update pump
```

Wine's builtin `winmm` faults inside quartz's audio-renderer enumeration during
`RenderFile`'s intelligent-connect. Native `quartz`/`devenum` are not installable
for a 64-bit title and `winmm` has no native replacement (investigated in the
handoff's prior session — do not redo), so the durable fix is to never build the
graph.

## Object model (all names assigned in the Ghidra project)

Full synchronous construction chain — the movie object's *constructor* builds the
DirectShow graph:

| Function | 20260616 addr | Role |
|---|---|---|
| `FUN_18007c560` | `0x18007C560` | Gameplay/demo **movie actor** — creates `agcs::Movie` when the song has a movie (`this+0xE8 != 0`); the no-movie branch fills its state slots with `4/0` and skips creation entirely (the game's natural "song has no movie" path) |
| `FUN_18003fcc0` | `0x18003FCC0` | Second movie-actor site (same ctor call, `param_3 = 4|5`) |
| `agcs_Movie_ctor` (`FUN_1802164c0`) | `0x1802164C0` | `agcs::Movie` ctor (vftable `0x180388638`); allocates the 0x48-byte **shared state struct** (`this+0x18`) and the `Dx9Movie` |
| `Dx9Movie_ctor` (`FUN_180215bb0`) | `0x180215BB0` | Copies the filename (Ordinal_71 = sprintf-like), calls Open |
| `Dx9Movie_Open` (`FUN_180215c90`) | `0x180215C90` | Creates the `MovieAllocator` + `DShow` wrapper (`FUN_180232ef0`, vftable `0x180389c08`), calls the open helper, then writes shared STATUS=1 / COMMAND=4→6 **without checking the open result** |
| `FUN_180215ee0` | `0x180215EE0` | Open helper: builds the request struct `{path, type(0=narrow path), flags}` and vcalls `DShow::Open` (slot +0x08). **No null check on `this[8]`** — the DShow object must exist (rules out stubbing the DShow factory) |
| `DShow_Open` (`FUN_180232da0`) | `0x180232DA0` | Double-buffered: vcalls `OpenFile` (slot +0x38) on the **back** player — **return value discarded** — then swaps front/back if the front player's state reads 0 |
| `DShowPlayer_OpenFile` (`FUN_18023ab10`) | `0x18023AB10` | vtbl slot +0x38 = teardown (`FUN_18023b270`, fully null-guarded) + tail-call to BuildGraph (explains the missing stack frame) |
| **`DShowPlayer_BuildGraph` (`FUN_18023ae40`)** | **`0x18023AE40`** | **The stub target.** See below |
| `Dx9Movie_Update` (`FUN_180215f50`) | `0x180215F50` | Per-frame status machine on the shared struct (see below) |
| `FUN_180216180` | `0x180216180` | `agcs::Movie` render — gated on `this+0x4C` frame-ready flag, which only sets when get-frame returns non-null |

`DShowPlayer` (vftable `0x18038a6c8`, object 0xC8 bytes, factory `FUN_18023bb10`):

| Offset | Field | Notes |
|---|---|---|
| +0x00 | vftable | |
| +0x08 | **state dword** | 0 = closed/never-opened; BuildGraph success epilogue writes **3** ("opened"); real playback flips 2/3 via `IMediaControl::GetState` |
| +0x0C | command dword | play/pause/stop bits written by the tiny vtbl methods |
| +0x10 | seek position float | |
| +0x14 | **opened byte** | 1 only after a real graph build; gates the get-frame COM path |
| +0x15 | frame-pending byte | |
| +0x16/+0x17 | request-flag bits 0/2 | written at BuildGraph entry |
| +0x48 | `IGraphBuilder*` | null until CoCreateInstance |
| +0x50..+0x68 | `IMediaControl`/`IMediaEvent`/`IMediaSeeking`/`IBasicAudio`-family pointers | all null-guarded by every consumer |
| +0x78/+0x80/+0x88 | allocator/renderer/stream objects | |

`DShowPlayer` vtable (all decompiled + verified null-safe for the never-opened state):

| Slot | 20260616 | Behavior on stubbed player |
|---|---|---|
| +0x10 getState (`FUN_1800f3610`) | returns `+0x8` dword | returns whatever the stub wrote |
| +0x18/+0x20/+0x28 play/pause/stop | write `+0xC` command bits only | safe |
| +0x30 | copies `+0x40` dword to out-param | safe |
| +0x38 OpenFile | teardown + BuildGraph | re-enters the stub |
| +0x40 get-frame (`FUN_18023ab40`) | `if (state==0) return 0; if (opened==0) { if (+0x15==0) return 0; … }` | **early-returns before any COM pointer** as long as `opened`==0 |
| +0x48 seek | writes `+0x10` | safe |
| +0x58 volume (`FUN_18023aad0`) | null-checks `+0x58` | safe |

## `DShowPlayer::BuildGraph` (`0x18023AE40`; `0x180256EB0` on 20260324)

The **only** function in gamemdx that touches DirectShow — the sole xref to
`CLSID_FilterGraph` (`e436ebb3-524f-11ce-9f53-0020af0ba770` @ `0x1802DC8C0`;
`IID_IGraphBuilder` @ `0x1802DCA50`). Flow:

1. `CoCreateInstance(CLSID_FilterGraph, NULL, CLSCTX_INPROC_SERVER, IID_IGraphBuilder, &this->graph /*+0x48*/)`
2. Creates the game's custom renderer filter, `IGraphBuilder::AddFilter` (vtbl +0x18)
3. Switches on request type (`request+0x10`): 0 = narrow path → widen (`FUN_180238f80`) →
   **`IGraphBuilder::RenderFile` (vtbl +0x68) — call @ `0x18023B0ED`, RA `+0x23B0F0` = the crash frame**;
   1 = wide path → RenderFile @ `0x18023B0B2`; 2 = custom `me::movie::impl::Stream` source; 3 = callback
4. QIs `IMediaControl`/`IMediaSeeking`/`IMediaEvent`/audio (`+0x50..+0x68`)
5. Success epilogue (`0x18023B20A..`): `+0x40 = 0|3` (audio state), `IMediaControl` run vcall,
   `+0x0C = 1`, **`+0x08 = 3` (state "opened")**, **`+0x14 = 1` (opened)**, `return 0`
6. Error returns: `0xC0260001` (COM/render), `0xC0260002` (alloc), `0xC0260005` (bad file)

The sole call site (`OpenFile`) is tail-called from `DShow::Open`, which **discards
the return value**.

## Shared-state status machine (`Dx9Movie_Update`, `0x180215F50`)

Shared 0x48-byte struct: `+0x14` = STATUS (consumer-visible), `+0x18` = COMMAND
(requested), `+0x08` = creation-flag byte (bit1 of ctor `param_3`; 0 for both actor
sites), `+0x0C` = ready dword. STATUS values: 1 opening, 6 ready, 7 playing,
4 playing-await, 5, 2/3 stopped.

Per frame: `iVar3 = DShow->getState()` (= front player `+0x8`), then
`switch (STATUS)`. **Case 1 ("opening") only advances when `iVar3 == 3`** →
STATUS = 6 (ready). The demo/gameplay sequences poll STATUS before starting the
song.

### The soft-lock (first stub attempt — live-tested, failed)

A stub that returns `0xC0260002` without touching the player leaves state = 0
forever → STATUS stuck at 1 → **the attract demo loads all gameplay assets and
then waits forever on the movie-ready gate** (observed: assets loaded 15:59:20,
zero further activity for 5+ min, scene 16 never exited; network keepalives still
flowing — main loop alive, sequence waiting). Note this also disproves the
"missing movie file is a tolerated state" assumption: on a real cabinet the
no-movie path is taken at the **actor** level (`FUN_18007c560`'s `+0xE8 == 0`
branch) before any player exists; the player-level failure state is *not*
naturally reachable with a valid musicdb.

### The working stub (live-verified 2026-07-21 16:15–16:20)

Detour BuildGraph, never call the original, and fake the success epilogue's one
observable side effect: **write player `+0x08 = 3`, leave `+0x14` (opened) = 0,
return 0.** Then:

- `getState()` reads 3 → STATUS advances 1 → 6 → (on play command) 7 — sequences proceed
- get-frame: `state != 0` but `opened == 0` and `+0x15 == 0` → returns 0 before any
  COM pointer → nothing renders, `agcs::Movie`'s frame-ready flag never sets
- play/pause/stop/seek/volume: plain dword writes or null-guarded
- teardown/re-open (`OpenFile`): teardown fully null-guarded, resets state to 0,
  stub writes 3 again
- Known cosmetic residual: within one `Dx9Movie` instance the double-buffer swap
  waits for the front player's state to return to 0, which a stubbed player only
  does on teardown — irrelevant in practice since each song's actor constructs a
  fresh object tree

Observed: attract demo entered 16:16:31, suppression 16:16:33, demo completed
16:17:22 (normal ~50 s), attract loop cycled 3×, zero exceptions.

## Signature (`movie_build_graph` in `src/core/signatures.rs`)

```
4C 8B DC 56 57 41 54 41 55 41 56 48 83 EC 40 48 C7 44 24 30 FE FF FF FF
49 89 5B 18 49 89 6B 20 48 8B EA 48 8B F9 8B 42 14 24 01 88 41 16 8B 42 14
C1 E8 02 24 01 88 41 17 48 8D 71 48
```

Function-entry anchored: prologue (`MOV R11,RSP`; pushes; `SUB RSP,0x40`;
gs-cookie qword; RBX/RBP home stores; `MOV RBP,RDX`/`MOV RDI,RCX`) + the
distinctive request-flag extraction (`[RDX+0x14]` bit0 → `[RCX+0x16]`, bit2 →
`[RCX+0x17]`) + `LEA RSI,[RCX+0x48]` (the IGraphBuilder slot). Every byte is
structural — opcodes and struct-offset immediates; no relocations, no
call/jcc displacements in range (the first `JNZ` follows the pattern end), so
no wildcards. Unique single match on both supported builds:
`0x18023AE40` (20260616) and `0x180256EB0` (20260324, byte-identical function,
same CoCreateInstance/RenderFile structure — structurally grounded, not a
coincidence).

## Gotchas

- **Do NOT stub the `DShow` factory (`FUN_180232ef0`) or fail object creation** —
  `FUN_180215ee0` vcalls `this[8]->Open` without a null check.
- **Do NOT return an error without the state write** — soft-locks the attract
  demo (see above). The state write is load-bearing.
- **Do NOT set the `opened` byte (+0x14)** — get-frame would then walk null COM
  pointers (`FUN_18023b620`, `+0x78` deref) and crash.
- **Do NOT hook `CMovieClip` (`~+0x257xxx`)** — that's the AFP sprite system,
  unrelated to the DirectShow WMV player (`~+0x215xxx–0x23Bxxx`).
- `DShowPlayer_OpenFile` → BuildGraph is a tail call — BuildGraph's caller frame
  is missing from crash stacks; don't let that mislead xref walks.
