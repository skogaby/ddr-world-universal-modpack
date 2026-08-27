# Native Windows Media Runtime in a CrossOver Bottle — Stock VC-1 Background Movies

**Status: WORKING** — verified live 2026-08-19: stock VC-1 `.wmv` background movies
render in the attract demo under CrossOver 26.3 (bottle `bemani`, wine-11.0 base,
macOS/Apple Silicon via Rosetta 2) with **zero file conversion**.

This note documents the complete recipe ("Path B") and the reverse-engineering
trail that produced it. The alternative ("Path A") — batch-transcoding all 387
movies to H.264 with `scripts/convert_movies.sh` — remains available and is
unaffected; the two paths compose (converted files simply decode through
winegstreamer instead).

Related:
- `.agents/planning/20260721-non-native-os-support/` — the original movie-player
  RE (object model, `BuildGraph` internals) and this note's addendum.
- `src/services/movie_policy.rs` — the shared `DShowPlayer::BuildGraph` detour
  (suppress/fallback modes, path absolutization).
- `src/services/mfplat_vih_fix.rs` — the in-process Wine mfplat fix (the final
  root cause; see §5).

---

## 1. Problem statement

DDR World plays background movies (`data/mdb_apx/movie/*.wmv`, 387 files, VC-1
Advanced profile in ASF) through a DirectShow filter graph built by gamemdx's
`DShowPlayer::BuildGraph`. Under CrossOver/Wine two independent failures occur:

1. **Crash** — spice2x's audio hooks IAT-patch `CoCreateInstance` process-wide
   and wrap `MMDeviceEnumerator`/`IAudioClient`; Wine's builtin `winmm` consumes
   those wrappers internally while quartz's devenum enumerates audio renderers
   during `RenderFile`, and faults. **Fix: launch spice2x with
   `-audiohookdisable`** (game audio is WASAPI and unaffected). Codec- and
   container-independent; required for ALL movie playback under Wine.
2. **Stall** — with the crash out of the way, `RenderFile` fails with
   `VFW_E_CANNOT_RENDER` (0x80040218) because CrossOver's GStreamer stack has no
   VC-1 decoder (VideoToolbox doesn't do VC-1). The game's `BuildGraph` error
   path never writes player state 3 ("opened") and the song waits forever on
   the movie-ready gate. The `non-native-operating-system-support` mod's
   fallback mode (`movie_mode: "fallback"`) converts the stall into a graceful
   "no movie this song".

Path B eliminates failure 2 outright: install Microsoft's own x64 Windows Media
runtime into the bottle so DirectShow decodes VC-1 through the real
`wmvdecod.dll` instead of winegstreamer.

## 2. The working configuration (operator recipe)

### 2.1 spice2x launch flags

```
-K ddr_world_hook.dll -audiohookdisable -icmphook
```

- `-audiohookdisable` — mandatory for movie playback (failure 1 above).
- `-icmphook` — network keepalive fix (unrelated to movies; see AGENTS.md).

### 2.2 mod-config.json

```json
"non_native_os_support": { "movie_mode": "fallback" }
```

Fallback mode runs the real graph build (suppress mode never would), and its
enable path installs the `mfplat_vih_fix` detour (§5). Any file that still
fails to render degrades to no-movie with a per-song INFO naming the hr.

### 2.3 Native DLLs → `drive_c/windows/system32/`

Copied from a real Windows 10 x64 installation (build 19041). **The WMP11
redistributable does NOT contain these as 64-bit binaries** — winetricks
`wmp11` is a dead end (its "x64" installer ships 32-bit media DLLs; XP x64 ran
WMP 32-bit). The files must come from a real Windows 10/11 x64 system32:

| File | Version | Role |
|------|---------|------|
| `qasf.dll` | 12.0.19041.1 | WM ASF Reader DirectShow filter |
| `WMVCORE.DLL` | 12.0.19041.6456 | Windows Media Format SDK core (reader object) |
| `WMASF.DLL` | 12.0.19041.1 | ASF container parser (wmvcore dep) |
| `wmvdecod.dll` | 10.0.19041.6456 | WMV/VC-1 video decoder DMO |
| `WMADMOD.DLL` | 10.0.19041.6093 | WMA audio decoder DMO (completeness; game movies are video-only) |
| `mfperfhelper.dll` | 10.0.19041.1 | static import of wmvcore/wmvdecod |
| `wmidx.dll` | 19041 | delay-load import of wmvcore (indexer) |

Keep the **builtin** `mfplat.dll` present (a July winetricks run had deleted
it; it was restored from a clean CrossOver install). wmvdecod delay-loads
MFPlat and its `DllRegisterServer` needs `MFTRegister` from it.

### 2.4 DllOverrides (HKCU `Software\Wine\DllOverrides`)

```
qasf, wmvcore, wmasf, wmvdecod, wmadmod, wmidx = native,builtin
mfplat, *mfplat                                = builtin
```

mfplat must stay builtin: the native one drags in the whole MF platform
(rtworkq/kernel scheduling) that Wine can't back. The builtin's one relevant
bug is fixed in-process (§5).

### 2.5 COM registration

```
regsvr32 /s qasf.dll wmvdecod.dll wmadmod.dll
```

(run inside the bottle; requires builtin mfplat present — see above). This
populates `HKCR\CLSID\{...}`, `HKCR\DirectShow\MediaObjects\<clsid>` (+
`Categories\<cat>\<clsid>`) and `HKCR\MediaFoundation\Transforms\...` for the
two DMOs, and the WM ASF Reader filter class.

### 2.6 Media Type source-filter mapping

Wine's quartz has no ASF byte-pattern entry, so `RenderFile` would pick the
async file source + winegstreamer. Add the mapping (already present on real
Windows):

```
[HKLM\Software\Classes\Media Type\{E436EB83-524F-11CE-9F53-0020AF0BA770}\{3026B275-8E66-CF11-A6D9-00AA0062CE6C}]
"Source Filter"="{187463A0-5BB7-11D3-ACBE-0080C75E246E}"
"0"="0,16,,3026B2758E66CF11A6D900AA0062CE6C"
```

(`{E436EB83…}` = MEDIATYPE_Stream, `{3026B275…}` = MEDIASUBTYPE_Asf, the byte
pattern = the ASF header GUID at offset 0, `{187463A0…}` = CLSID_WMAsfReader.)

**Do NOT add `Media Type\Extensions\.wmv`.** Wine's `get_media_type`
(filesource.c) returns FALSE the moment an Extensions key exists for the
file's extension, which *disables* the byte-pattern probe and forces the async
source → winegstreamer → no VC-1. This poisoned hours of earlier testing.

### 2.7 Hook-DLL pieces (ship in this repo, no bottle action needed)

- **Path absolutization** (`movie_policy::absolutize_request_path`, fallback
  mode only): the game passes `.\data\mdb_apx\movie\<code>.wmv`; Wine quartz's
  byte-pattern probe needs to open the file and fails on relative paths
  (silently falling back to the async source). The hook rewrites the narrow
  request path to absolute using `GetCurrentDirectoryA`. No-op on real
  Windows.
- **`mfplat_vih_fix`** (§5) — the final root-cause fix, Wine-gated, installed
  by the mod's enable path in fallback mode.
- **`ntdll_state_shim`** (§2.9) — Wine-gated, installed by the mod's enable
  path in fallback mode; only matters on bottles carrying native quartz.

### 2.9 ABANDONED EXPERIMENT: native quartz/devenum (SetRate-capable FGM)

**Status: dead end as of CrossOver 2026-08 — do not apply.** Kept as the
record of what was tried and where it fails.

Wine's builtin quartz implements `IMediaSeeking::SetRate` as a silent no-op
(returns S_OK, stores nothing, scales nothing — live-confirmed 2026-08-21,
movie-sync probe: readback 1.000, no visual speedup, recording comparison).
Replacing quartz+devenum with the native Windows 10 x64 binaries was tried
to get a real filter-graph manager:

1. Native `quartz.dll` + `devenum.dll` (Win10 19041 x64) into system32,
   overrides `native,builtin`, `regsvr32 /s devenum.dll` (quartz's CLSIDs
   are already registered by path; the override picks the binary).
2. **First blocker (SOLVED):** native quartz imports and calls
   `ntdll!RtlGetPersistedStateLocation` in `DllMain`. Wine declares it
   `@ stub` — no export exists (verified against CrossOver ntdll's export
   table); the loader snaps a synthesized abort thunk into quartz's IAT
   and the load fails. Fixed by `services/ntdll_state_shim.rs`:
   `LdrRegisterDllNotification` LOADED callback (post-snap, pre-DllMain)
   patches quartz's IAT slot to a local "no persisted state"
   implementation (`STATUS_OBJECT_NAME_NOT_FOUND`). Confirmed working
   live (`quartz.dll IAT patched ... pre-DllMain`). The shim stays in the
   codebase: Wine-gated, quartz-only, fail-open, inert on stock bottles.
3. **Second blocker (FATAL):** with native quartz loaded, the first movie
   open HARD-LOCKED the game on a black screen — BuildGraph never
   returned (no capture, no failure hr; the log's last activity is three
   fresh WineD3D fake windows spawning at the freeze). Signature: native
   quartz's intelligent connect instantiating a default Video
   Renderer/VMR — d3d/ddraw through wined3d, deadlocking against the
   game's live device on the update thread. That interaction is inside
   quartz×wined3d, beyond what an in-process hook can safely fix.

Revert (applied 2026-08-21): builtin files restored from
`drive_c/windows/system32.builtin-backup/`, both override values deleted.

**The supported path for rate-synced movies under Wine is seek-based
drift correction in the movie-sync engine** (running-state seeks are
proven on builtin quartz); real `SetRate` remains Windows-cabinet-only
pending its probe there.

### 2.8 Residue that can be cleaned (harmless, never read)

- `HKLM\Software\Microsoft\Windows Media\WMSDK\VideoDecode` /
  `HKCR\Windows Media\WMSDK\VideoDecode` FOURCC value names — added on the
  hypothesis wmvcore gates codecs on them; `+reg` tracing proved wmvcore never
  reads VideoDecode. Safe to delete or ignore.
- Pre-change registry backup:
  `winmm-repro-harness/bottle-backup-pre-native-wm/` (sibling project).

## 3. How the pieces chain at runtime

```
DShowPlayer::BuildGraph (gamemdx)
 └─ movie_policy detour: absolutize path → call original
     └─ quartz RenderFile
         ├─ byte-pattern probe (needs §2.6 key + absolute path)
         │    → WM ASF Reader (native qasf)          [§2.3–2.5]
         ├─ qasf wraps wmvcore's reader object; WMASF parses the ASF
         ├─ output-format negotiation: wmvcore lazily enumerates codec DMOs
         │    via msdmo DMOEnum(cat=VIDEO_DECODER, flags=INCLUDE_KEYED,
         │    intype=WVC1) → finds + loads native wmvdecod  [§2.5 registration]
         ├─ wmvdecod SetInputType: converts VIDEOINFOHEADER → IMFMediaType
         │    via builtin mfplat MFInitMediaTypeFromVideoInfoHeader
         │    → **mfplat_vih_fix injects the WVC1 FOURCC subtype**  [§5]
         ├─ decoder accepts; reader pin now offers 13 decoded formats
         │    (NV12/YV12/IYUV/I420/YUY2/UYVY/YVYU/NV11 + RGB32/24/565/555/8)
         └─ game MemRenderer (CheckMediaType: RGB32|RGB565|RGB555 only)
              connects — movie renders through the game's own pipeline
```

## 4. The investigation trail (what was actually wrong, in order)

Each layer was diagnosed with the standalone harness
(`~/Desktop/Projects/winmm-repro-harness/` — modes: *(default)* RenderFile with
NullRenderer, `dump-types`, `decode-link`, `dmo-direct`, `sync-reader`,
`sync-reader-fix`) plus `WINEDEBUG` channels through the raw `wineloader`
(`WINEMSYNC=1` required; the `bin/wine` Perl wrapper eats `WINEDEBUG` — use
`CX_DEBUGMSG=` with it).

1. **Crash at movie start** → spice2x audio hooks × Wine winmm (devenum audio
   enumeration). `-audiohookdisable`. (Prior session.)
2. **Async source picked instead of WM ASF Reader** → two causes: missing
   Media Type byte-pattern key (§2.6) and the game's relative path failing the
   probe's file-open (§2.7). Diagnosed via `movie_policy[diag]` in-process
   probe + harness A/B (absolute rendered hr=0, relative
   VFW_E_CANNOT_RENDER). (Prior session.)
3. **Reader selected but output pin offers only compressed WVC1** → the frontier
   this session inherited. Eliminated in order:
   - *Keyed-DMO hypothesis*: dead — no `Keyed` subkeys exist under
     `DirectShow\MediaObjects\<clsid>` in the bottle.
   - *DMOEnum "0 found"*: stale evidence — a fresh `+msdmo` trace showed
     enumeration WORKING (`found match "WMVideo Decoder DMO"`); the old trace
     predated the successful `regsvr32`. Those DMOEnum calls were also
     devenum's, not wmvcore's.
   - *`mfcore.dll` probe*: red herring — `GetModuleHandle` (loaded-check), not
     a LoadLibrary; wmvcore tolerates its absence.
   - *wmvcore never attempting codec discovery at open*: correct observation,
     wrong conclusion — discovery is **lazy**, it happens inside
     `IWMSyncReader::GetOutputFormatCount` (equivalently qasf's pin
     media-type enumeration during connect), not at `Open`.
4. **The actual root cause** (found with the harness's new `sync-reader` mode —
   native wmvcore driven directly): `GetOutputFormatCount` triggered
   `DMOEnum(VIDEO_DECODER, INCLUDE_KEYED, intype=WVC1)` → wmvdecod found,
   loaded, instantiated — then rejected the input type with
   `DMO_E_TYPE_NOT_ACCEPTED` (0x80040205). `+relay` tracing inside
   `SetInputType` showed wmvdecod converting the game's `VIDEOINFOHEADER`
   (cbFormat=0x71 = 88-byte VIH + 25 bytes VC-1 codec private data) through
   **builtin mfplat's `MFInitMediaTypeFromVideoInfoHeader(…, subtype=NULL)`**,
   which set `MF_MT_SUBTYPE = MFVideoFormat_RGB24`. See §5.

## 5. Root cause: Wine mfplat's FOURCC-blind subtype derivation

`MFInitMediaTypeFromVideoInfoHeader` / `…Header2` (wine-11.0
`dlls/mfplat/mediatype.c`), when the caller passes `subtype == NULL`, derives
the subtype **from `biBitCount` alone**:

```c
if (!subtype)
{
    switch (vih->bmiHeader.biBitCount)
    {
    ...
    case 24: subtype = &MFVideoFormat_RGB24; break;
    ...
    }
}
```

`biCompression` is never consulted. A WVC1 header (`biCompression='WVC1'`,
`biBitCount=24`) therefore comes back labeled **RGB24**. Windows' mfplat maps
a FOURCC `biCompression` to the FOURCC subtype GUID
(`{31435657-0000-0010-8000-00AA00389B71}` for 'WVC1'). Native wmvdecod trusts
the conversion, sees "RGB24", and refuses its own input.

**Fix** (`src/services/mfplat_vih_fix.rs`): a `GenericDetour` on the mfplat
export. When `subtype == NULL`, the buffer is at least `sizeof(VIDEOINFOHEADER)`,
and `biCompression` is a real FOURCC (not `BI_RGB`=0 / `BI_BITFIELDS`=3), call
the original through the trampoline with the FOURCC-derived subtype made
explicit — exactly the Windows semantics. Every other call passes through
byte-identically. Properties:

- **Wine-gated**: installs only if `ntdll!wine_get_version` resolves; real
  Windows is never touched.
- **Installed once** from `non_native_os_support::enable()` in fallback mode
  (the only mode that runs the graph build); stays installed — its semantics
  are strictly Windows-correct, so there's nothing to undo on disable.
- **Fail-open**: resolution/installation failure logs one WARN; movies keep
  degrading to no-movie as before.
- One-shot INFO on first injection:
  `mfplat_vih_fix: injected FOURCC subtype "WVC1" …`.

Validation:

- Harness `sync-reader-fix` (same fix as a raw byte patch):
  `GetOutputFormatCount` went `0x80040205 / 0` → `S_OK / 13 formats`,
  including RGB32 (`{E436EB7E…}`), RGB565 (`{E436EB7B…}`), RGB555
  (`{E436EB7C…}`) — precisely the set the game renderer accepts
  (`me::movie::impl::MemRenderer::CheckMediaType`, Ghidra `gamemdx_20260721`
  `0x18024ba10`).
- In-game 2026-08-19: detour installed at boot; `WVC1` injection fired at the
  attract demo's first movie open; **zero** `movie_policy: graph build failed`
  lines; movies visually confirmed rendering in demo/attract mode.

Upstream: this is a genuine Wine bug (FOURCC `biCompression` ignored on the
NULL-subtype path); a future Wine/CrossOver may fix it, at which point the
detour's injection branch simply never fires (explicit-subtype calls pass
through) — no compatibility hazard either way.

## 6. Diagnostics kept in the shipping DLL

- `movie_policy[diag]` (fallback mode, one-shot): logs the movie file's first
  16 bytes (expect `3026B2758E66CF11A6D900AA0062CE6C`) and the §2.6 registry
  key as seen from inside the (spice2x-hooked) game process. Two INFO lines
  per boot; kept — it verifies the two bottle-side prerequisites of the probe
  chain and costs nothing on real Windows.
- `mfplat_vih_fix` one-shot injection INFO — confirms decode negotiation
  reached the decoder.
- Per-failure `movie_policy: graph build failed (hr=…)` — names files that
  still can't render (feeds incremental Path-A conversion if ever needed).

## 7. Known limitations / notes

- The WM runtime DLLs cannot be redistributed with the modpack (Microsoft
  binaries); operators must source them from a real Windows 10/11 x64 install.
  Without them, fallback mode keeps working with no-movie degradation, and
  Path A (`scripts/convert_movies.sh`) remains the conversion route.
- WMA **audio** in movies is untested (game movies are video-only streams);
  wmadmod is registered and should cover it if ever needed.
- Movie playback under **suppress** mode is unchanged (never builds a graph);
  the bottle runtime only matters in fallback mode.
- The `sync-reader`/`sync-reader-fix` harness modes live in the sibling
  `winmm-repro-harness` project, not this repo.
