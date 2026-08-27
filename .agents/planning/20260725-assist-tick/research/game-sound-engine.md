# DDR World Sound Engine — RE Notes (assist-tick feasibility)

**Target build:** `gamemdx_20260721.dll` (Ghidra project `gamemdx_20260721.dll`, image base
`0x180000000`). All addresses below are **file-relative** (`+0xNNNNNN`), i.e. add
`0x180000000` for the VA used in the Ghidra listing. Because the image base is
`0x180000000`, the Ghidra addresses quoted inline (`0x1801AA6E0`) and the file-relative
form (`+0x1AA6E0`) are the same number with the base stripped.

**Evidence discipline:** every line is tagged **[OBS]** (observed directly in the
disassembly/decompilation or in an on-disk game file) or **[INF]** (inferred — must be
re-verified before it is depended on). Per this project's rule, do not treat any **[INF]**
line as fact when implementing.

---

## Overview

DDR World's audio is **Microsoft XACT 2** (`xactengine2_10.dll`), driven entirely from
`gamemdx.dll` through a thin in-house singleton wrapper ("audio manager", global at
`+0x6F2D60`). There is **no** custom mixer, no XAudio2/DirectSound/WASAPI code in
`gamemdx.dll`, and no separate Konami sound middleware DLL.

Everything the game plays — menu BGM, voice, all SEs, **and the song audio itself** —
goes through this one XACT engine instance and therefore through one final mix. Sound
assets are XACT **sound banks** (`.xsb`, cue definitions) + **wave banks** (`.xwb`, audio
data), delivered either as ARC entries loaded through AVS (in-memory banks) or as loose
files opened with `CreateFileA` (streaming banks).

Bottom line for the assist tick: the game already plays a per-note SE from inside its
judge loop (`se_game_shockarrow`, see [Existing SE Call Sites](#existing-se-call-sites)),
and the "play a cue by name" entry point is a clean 3-argument MS-x64 function that is
trivially callable from Rust. Getting a *custom* sample in is also tractable — see
[Custom-Sample Feasibility](#custom-sample-feasibility).

---

## Audio Stack

### What is present

| Fact | Evidence |
|---|---|
| Engine is XACT 2, COM-instantiated | **[OBS]** `+0x1AA000` calls `CoCreateInstance(CLSID@+0x3801B0, NULL, CLSCTX_INPROC_SERVER, IID@+0x3801C0, &engine)`. Reads `HKLM\Software\Microsoft\XACT` value `DebugEngine`; if non-zero uses the debug CLSID at `+0x3801A0` first, then falls back to the release CLSID. |
| The engine DLL is `xactengine2_10.dll` | **[OBS]** string at `+0x2DE0A0`, referenced only from a data slot at `+0x465FD8` (COM registration/telemetry list, not a `LoadLibrary` call site). **[OBS]** the DLL is shipped with the game at `contents/com/xactengine2_10.dll` (404,120 bytes, PE machine `0x8664` = x64). |
| XACT's own output backend is DirectSound | **[OBS]** `xactengine2_10.dll` import table is only `msvcrt / KERNEL32 / RPCRT4 / ole32 / ADVAPI32 / USER32 / WINMM`; `WINMM` imports are **only** `timeBeginPeriod`/`timeEndPeriod`. Its string table contains exactly two audio-backend strings: `DirectSound` and `DirectSoundEnumerateW`. **[INF]** ⇒ dsound is resolved dynamically (LoadLibrary/GetProcAddress or COM), no XAudio2 path. |
| Global settings file | **[OBS]** `data/sound/win/ddr.xgs` (string `+0x380108`), load requested at `+0x2872` inside `Application::onBoot`. Passed to `Initialize()` as `pGlobalSettingsBuffer`. |
| Engine is initialized with lookAheadTime = 250 ms | **[OBS]** `+0x1AAB60` builds the params struct with first dword `0xFA` = 250. **[INF]** 250 is `XACT_ENGINE_LOOKAHEAD_DEFAULT`. |

### What is *absent* (negative results, all **[OBS]**)

`gamemdx.dll`'s import table (`ghidra_list_imports`) contains **no**: `XAudio2*`,
`dsound`/`DirectSoundCreate*`, `waveOut*`, `mmdevapi`/`IMMDeviceEnumerator` helpers, ASIO,
or any Konami sound DLL. The only `winmm` imports are timer APIs
(`timeSetEvent/timeKillEvent/timeGetTime/timeGetSystemTime/timeBeginPeriod/timeEndPeriod`).
`ole32!CoCreateInstance` is imported and used for (a) the XACT engine and (b) the
DirectShow movie graph. There is no `IAudioClient`/`WASAPI`/`ASIO` string anywhere.

**Conclusion [OBS+INF]:** audio does *not* live in a sibling DLL. It lives in
`gamemdx.dll` (the wrapper + all policy) plus the redistributable `xactengine2_10.dll`
(mixer + device). No investigation redirect needed.

### XACT_RUNTIME_PARAMETERS reconstruction (from `+0x1AAB60`)

**[OBS]** the struct built on the stack, **[INF]** the field naming (matches the public
`XACT_RUNTIME_PARAMETERS` layout exactly, which is why the mapping is high-confidence):

| Offset | Value at `+0x1AAB60` | Field **[INF]** |
|---|---|---|
| `+0x00` | `0xFA` (250) | `lookAheadTime` |
| `+0x08` | `ddr.xgs` file data ptr | `pGlobalSettingsBuffer` |
| `+0x10` | `ddr.xgs` size | `globalSettingsBufferSize` |
| `+0x18` | 0 | `globalSettingsFlags` / `AllocAttributes` |
| `+0x20` | `+0x1AA250` | `fileIoCallbacks.CreateFileCallback` |
| `+0x28` | `+0x1AA350` | `fileIoCallbacks.GetOverlappedResultCallback` |
| `+0x30` | `+0x1AA0F0` | `fnNotificationCallback` |
| `+0x38` | 0 | `pRendererID` (default device) |

**[OBS]** After `Initialize`, five `RegisterNotification` calls are made with types
`0x01, 0x04, 0x0C, 0x10, 0x11`. **[INF]** = `CUEPREPARED`, `CUEDESTROYED`, `WAVEPREPARED`,
`WAVEDESTROYED`, `WAVEBANKPREPARED` — and the notification handler at `+0x1AA0F0`
dispatches on exactly those five values, which corroborates the mapping.

**[OBS]** `GetFinalMixFormat`-shaped call at engine vtable `+0x28` writes a
`WAVEFORMATEX`-shaped buffer and the wrapper stores `nChannels` into `mgr+0x2118`
(default preset to `2` earlier in the ctor). This is the value later used to decide how
many matrix coefficients to compute (stereo / 4ch / 5.1 branches in `+0x1ABF90`).

### Engine vtable map

**[INF] with strong corroboration.** Indices marked ✔ are **[OBS]** — the game actually
calls them and the argument shape matches the documented XACT signature.

| Offset | Method | Status |
|---|---|---|
| `+0x18` | `GetRendererCount` | [INF] |
| `+0x20` | `GetRendererDetails` | [INF] |
| `+0x28` | `GetFinalMixFormat(WAVEFORMATEXTENSIBLE*)` | ✔ called at `+0x1AAB60` |
| `+0x30` | `Initialize(const XACT_RUNTIME_PARAMETERS*)` | ✔ called at `+0x1AAB60` |
| `+0x38` | `ShutDown` | [INF] |
| `+0x40` | `DoWork()` | ✔ called every frame from `+0x3020`, and after every Play |
| `+0x48` | `CreateSoundBank(const void* pv, DWORD cb, DWORD flags, DWORD allocAttr, IXACT2SoundBank**)` | ✔ called at `+0x1AAFA0` |
| `+0x50` | `CreateInMemoryWaveBank(const void* pv, DWORD cb, DWORD flags, DWORD allocAttr, IXACT2WaveBank**)` | ✔ called at `+0x1AB050` (4 args + this observed) |
| `+0x58` | `CreateStreamingWaveBank(const XACT_STREAMING_PARAMETERS*, IXACT2WaveBank**)` | ✔ called at `+0x1AB050` |
| `+0x60` | `PrepareWave` | **[INF] — never called by the game. Existence unverified.** |
| `+0x68` | `PrepareInMemoryWave` | **[INF] — never called by the game. Existence unverified.** |
| `+0x78` | `RegisterNotification(const XACT_NOTIFICATION_DESCRIPTION*)` | ✔ called ×5 at `+0x1AAB60` |

> ⚠️ **[INF]** The XACT**2** `IXACT2Cue` vtable is provably *not* identical to XACT3's — see
> the Cue table below (Pause sits 4 slots later than in `xact3.h`). Therefore engine
> indices `≥0x60` that the game never touches **cannot** be assumed to match `xact3.h`.
> This is the single biggest unknown in this document.

### Sound-bank / wave-bank / cue / wave vtables

All ✔ rows are **[OBS]** (call site + argument shape + a semantic cross-check).

`IXACT2SoundBank`:

| Offset | Method | Status |
|---|---|---|
| `+0x00` | `XACTINDEX GetCueIndex(PCSTR)` | ✔ `+0x1AB7C5`: `CALL [rax]`, result compared `CMP AX, 0xFFFF` ⇒ 16-bit index, `0xFFFF` = not found |
| `+0x18` | `Prepare(XACTINDEX, DWORD, XACTTIME, IXACT2Cue**)` | ✔ `+0x1AB6xx` |
| `+0x20` | `Play(XACTINDEX, DWORD dwFlags, XACTTIME timeOffset, IXACT2Cue** ppCue)` | ✔ `+0x1AB805`: `RCX=this, EDX=idx, R8D=0, R9D=0, [rsp+0x20]=&cue` |
| `+0x30` | `Destroy()` | ✔ `+0x1AB3D0` (`.xsb` unload branch) |

`IXACT2WaveBank`:

| Offset | Method | Status |
|---|---|---|
| `+0x00` | `Destroy()` | ✔ `+0x1AB3D0` (`.xwb` unload branch, followed by `CloseHandle`) |
| `+0x10` | `XACTINDEX GetWaveIndex(PCSTR)` | **[INF]** (from `xact3.h` order) |
| `+0x28` | `Play(XACTINDEX, DWORD, DWORD playOffset, XACTLOOPCOUNT, IXACT2Wave**)` | **[INF]** — never called by the game |

`IXACT2Cue`:

| Offset | Method | Status |
|---|---|---|
| `+0x00` | `Play()` | ✔ `+0x1ABB30` (deferred-start branch) |
| `+0x08` | `Stop(DWORD)` | ✔ `+0x1AA7C0`, `+0x1AA850` |
| `+0x10` | `GetState(DWORD*)` | ✔ `+0x1AB8C0` tests `&0x08` (PLAYING), `+0x1ABB30` tests `&0x20` (STOPPED) |
| `+0x18` | `Destroy()` | ✔ `+0x1ABB30` reaper |
| `+0x20..0x38` | **unknown, 4 slots** — XACT2-only methods absent from XACT3 | **[INF]** never called |
| `+0x40` | `SetMatrixCoefficients(UINT32 nSrcChannels, UINT32 nDstChannels, float* pCoeffs)` | ✔ `+0x1ABF90` passes `(2, finalMixChannels, &coeffs)` |
| `+0x60` | `Pause(BOOL)` | ✔ `+0x1AB840` passes `1` |

`IXACT2Wave` (matches `xact3.h` exactly, unlike Cue):

| Offset | Method | Status |
|---|---|---|
| `+0x00` | `Destroy()` | ✔ `+0x1ABB30` |
| `+0x10` | `Stop(DWORD)` | ✔ `+0x1AA7C0` passes `1` |
| `+0x18` | `Pause(BOOL)` | ✔ `+0x1AB840` |
| `+0x20` | `GetState(DWORD*)` | ✔ `+0x1AB8C0`, `+0x1ABB30` |

---

## Key Addresses

Audio module = `+0x1AA000 .. +0x1AC000` (42 functions).

### Globals

| Global | Addr | Meaning |
|---|---|---|
| `audio_manager` | `+0x6F2D60` | **[OBS]** singleton pointer; object is `0x20F0` bytes, allocated from the AGCS app heap (`DAT_180466068`) in `Application::onBoot` at `+0x28A5`. `*(void**)mgr` = the `IXACT2Engine*`. |
| `file_manager` | `+0x6F2F48` | **[OBS]** the ARC/file manager singleton (0x158-byte object). **[INF]** same object the repo's `asset_loader.rs` calls `file_manager_singleton`. |
| `xgs_file_id` | `+0x46534C` | **[OBS]** file id of `ddr.xgs`. |
| `se_mute_filter` | `+0x6F2420` | **[OBS]** function pointer; called as `f(int* io_state)` with `*io_state = 5` before every SE play; if it returns `*io_state == 6` the play is **vetoed**. **[INF]** an event/attract/mute policy hook. |
| `audio_lock_count` | `+0x6F38F8` | **[OBS]** if `> 0`, the wrapper takes an AVS lock (`XCnbrep700000F` / `...0010`) around play/stop. |
| `bank_lock_count` | `+0x6F38FC` | **[OBS]** same, for the wave-bank vectors. |
| `pan_right` | `+0x359EB8` | **[OBS]** `1.0f` |
| `pan_left` | `+0x35A6F4` | **[OBS]** `-1.0f` |

### Functions

| Function | Addr | Role |
|---|---|---|
| `Application::onBoot` | `+0x20B0` | **[OBS]** requests `ddr.xgs` + `data/arc/soundbanks.arc`, constructs the audio manager. |
| `xact_engine_create` | `+0x1AA000` | **[OBS]** `CoCreateInstance` the XACT engine. |
| `audio_mgr_ctor` | `+0x1AAB60` | **[OBS]** `Initialize`, `RegisterNotification`×5, `GetFinalMixFormat`, registers `audio::XwbFileCallback` + `audio::XsbFileCallback`. |
| `xact_notification_cb` | `+0x1AA0F0` | **[OBS]** notification handler. |
| `xact_fileio_createfile_cb` | `+0x1AA250` | **[OBS]** XACT file-IO callback. |
| `xact_fileio_overlapped_cb` | `+0x1AA350` | **[OBS]** XACT file-IO callback. |
| `bank_slot_of_file` | `+0x1AA3C0` | **[OBS]** basename → bank slot: `bgm_menu`→0, `se_system`→1, `se_normal`→2, `voice`→3, anything else→**5**. |
| `sound_file_register` | `+0x1AA520` | **[OBS]** public entry: dispatch a loaded file by extension → `.xsb` → `+0x1AAFA0`, else → `+0x1AB050`. |
| `soundbank_create` | `+0x1AAFA0` | **[OBS]** `engine->CreateSoundBank(data, size, 0, 0, &mgr->bank[slot])` then `DoWork`. Skips if the slot is already occupied. |
| `wavebank_create` | `+0x1AB050` | **[OBS]** slot ∈ {0,3,5} → `CreateFileA` + `CreateStreamingWaveBank`; slot ∈ {1,2} → `CreateInMemoryWaveBank`. |
| `sound_file_unregister` | `+0x1AB3D0` | **[OBS]** `.xsb` → `SoundBank::Destroy`; `.xwb` → `WaveBank::Destroy` + `CloseHandle`. |
| **`se_play`** | **`+0x1AA6E0`** | **[OBS]** the public "play a cue" API. See signature below. |
| `se_play_inner` | `+0x1AB7A0` | **[OBS]** `GetCueIndex` + `SoundBank::Play` + slot registration. |
| `se_prepare` | `+0x1AA5C0` → `+0x1AB680` | **[OBS]** same but `SoundBank::Prepare` (no playback). |
| `se_start_prepared` | `+0x1AA680` → `+0x1AB720` | **[OBS]** starts a prepared handle. |
| `se_stop` | `+0x1AA7C0` | **[OBS]** stop by handle (22 xrefs). |
| `se_stop_all` | `+0x1AA850` | **[OBS]** walks all 256 slots. |
| `se_is_playing` | `+0x1AA8F0` → `+0x1AB8C0` | **[OBS]** `GetState & 0x08`. |
| `se_pause` | `+0x1AB840` | **[OBS]** `Cue::Pause(1)` / `Wave::Pause(1)`. |
| `handle_slot_alloc` | `+0x1AB5B0` | **[OBS]** round-robin allocate one of 256 handle slots (257 xrefs — inlined everywhere). |
| `apply_pan_matrix` | `+0x1ABF90` | **[OBS]** compute + `SetMatrixCoefficients`. |
| `audio_frame_update` | `+0x1ABB30` | **[OBS]** per-frame reaper: destroy STOPPED cues/waves, start deferred cues. |
| `frame_main` | `+0x3020` | **[OBS]** calls `+0x1ABB30` then `engine->DoWork()` exactly once per frame. |
| `se_set_volume` | `+0x1AA950` → `+0x1AB930` | **[OBS]** `(int category, float vol)`, then re-applies matrices to all live handles. |
| `se_set_stereo_enable` | `+0x1AAA60` | **[OBS]** writes `mgr+0x20C4` (the "pan by side" master switch). |
| `se_play_helper` | `+0x1A82F0` | **[OBS]** the widely-used front door (39 callers). See below. |
| `se_play_panned` | `+0x1AA780` | **[OBS]** `(bank, name, int side)` → pan constant → `se_play`. |
| `boot_load_se_banks` | `+0x2BEA0` | **[OBS]** loads `se_system.arc`, `se_normal.arc`, `voice.xwb`, `bgm_menu.xwb`. |
| `song_bank_load` | `+0x61680` | **[OBS]** loads `data/sound/win/dance/<code>.xsb` + `.xwb` at gameplay entry (bank slot 5). |
| `xsb_file_callback` | `+0x1AC5A0` | **[OBS]** `→ sound_file_register`. |
| `xwb_file_callback` | `+0x1AC650` | **[OBS]** `→ sound_file_register`. |
| `GamePlayActor::judgeNotes` | `+0x5EC70` | **[OBS]** the judge loop; plays `se_game_shockarrow` inline. |

### Audio manager object layout (`mgr` = `*(void**)0x1806F2D60`)

**[OBS]** unless noted.

| Offset | Contents |
|---|---|
| `+0x00` | `IXACT2Engine*` |
| `+0x08 + slot*0x10` | `int file_id` (`-1` = empty) for sound-bank slot `slot` |
| `+0x10 + slot*0x10` | `IXACT2SoundBank*` for slot `slot` (slots 0..5) |
| `+0x68 / +0x70 / +0x78` | `vector<{int file_id; int slot; HANDLE file; IXACT2WaveBank*}>` (0x20 stride) |
| `+0x8C, +0x90` | `float` per-category volumes **[INF]** |
| `+0x94` | `float` master/attenuation **[INF]** |
| `+0x98, +0x99, +0x9A` | mute flags **[INF]** |
| `+0xA0 + i*0x20` | **handle slot table, 256 entries**: `[0]=IXACT2Cue*`, `[+8]=IXACT2Wave*`, `[+0x10]=prepared flag`, `[+0x11]=deferred-play flag`, `[+0x14]=float pan`, `[+0x18]=int bank_id` |
| `+0x20A0..0x20C0` | pending-destroy vector |
| `+0x20C4` | `bool` stereo/pan-by-side enable |
| `+0x20E8` | `u32` round-robin cursor for slot allocation (masked `& 0xFF`) |
| `+0x20C8/0x20D0` | sorted `vector<{HANDLE, int file_id}>` for streaming banks |

---

## Sound Asset Loading

### The pipeline (all **[OBS]**)

1. Some code calls the file manager's load-by-path (`+0x1FEEB0(path, "sound")` at boot,
   `+0x1FEF30(file_manager, path)` for the per-song banks) → returns an `int file_id`
   into the file manager's table. The `"sound"` string is a **group tag** written into
   the file record at `+0x90/+0x91`; `song_bank_load` force-writes it (`len=5, "sound"`)
   for the per-song banks.
2. When the file's bytes are resident, the file manager dispatches by extension to a
   registered `FileCallback` — `audio::XsbFileCallback` (`+0x1AC5A0`) or
   `audio::XwbFileCallback` (`+0x1AC650`). RTTI strings at `+0x4BEBC0`
   (`.?AVXsbFileCallback@audio@@`) and `+0x4BEBF0` (`.?AVXwbFileCallback@audio@@`).
   These are registered exactly like `PngFileCallback` / `agcs::Bm2dFileCallback` /
   `agcs::ShaderFileCallback` in `Application::onBoot`.
3. Both callbacks funnel into `sound_file_register(file_id)` (`+0x1AA520`), which
   `strncmp`s the extension against `"xsb"`:
   - `.xsb` → `engine->CreateSoundBank(record.data, record.size, 0, 0, &mgr->bank[slot])`
   - `.xwb` → in-memory *or* streaming wave bank, chosen by slot (see below)

### Which banks exist

**[OBS]** verified against the on-disk install
(`.../CrossOver/Bottles/bemani/drive_c/ddr_world/contents/`):

| Slot | Basename | Sound bank source | Wave bank source | Wave-bank kind |
|---|---|---|---|---|
| 0 | `bgm_menu` | `data/arc/soundbanks.arc` → `bgm_menu.xsb` | `data/sound/win/bgm_menu.xwb` (loose) | **streaming** |
| 1 | `se_system` | `soundbanks.arc` → `se_system.xsb` | `data/arc/se_system.arc` → `se_system.xwb` | **in-memory** |
| 2 | `se_normal` | `soundbanks.arc` → `se_normal.xsb` | `data/arc/se_normal.arc` → `se_normal.xwb` | **in-memory** |
| 3 | `voice` | `soundbanks.arc` → `voice.xsb` | `data/sound/win/voice.xwb` (loose) | **streaming** |
| 5 | *anything else* | `data/sound/win/dance/<code>.xsb` | `data/sound/win/dance/<code>.xwb` | **streaming** |

**[OBS]** `soundbanks.arc` (15,424 bytes) contains exactly four entries, confirmed by
hexdump of its path table: `data/sound/win/bgm_menu.xsb`, `.../se_normal.xsb`,
`.../se_system.xsb`, `.../voice.xsb`. First payload begins with `SDBK` at `+0xCA`.

**[OBS]** `se_normal.arc` (17,740,288 bytes) contains exactly **one** entry:
`data/sound/win/se_normal.xwb`, **stored uncompressed** (packed size ==
unpacked size == `0x010EB1B4`), payload at file offset `0x40` starting with `WBND`.
Internal wave-bank friendly name `se_normal` at `+0x7C`.

**[OBS]** There are `_n`-suffixed sibling variants on disk (`se_normal_n.arc`,
`se_system_n.arc`, `soundbanks_n.arc`, `voice_n.xwb`, `bgm_menu_n.xwb`) — presumably a
newer/regional asset set selected at runtime. **[INF]** which one the game picks is not
established; the boot code requests the un-suffixed names literally, so either LayeredFS
or an AVS mount remaps them, or `_n` is dead weight. **Verify before shipping a mod that
patches only one of the pair.**

**[OBS]** `data/sound/win/dance/` holds one `<code>.xsb` (326 bytes!) + one `<code>.xwb`
(multi-MB) per song — so **the song audio is itself an XACT streaming wave bank driven by
a 326-byte hand-sized sound bank**. Example: `aaaa.xsb` = 326 bytes, `aaaa.xwb` = 6.4 MB.

**[OBS]** `ddr.xgs` is *not* present as a loose file under `data/sound/win/`. **[INF]** it
comes from an ARC or the encrypted `arkdata`; not investigated.

### Important loader constraints (all **[OBS]**)

- `soundbank_create` (`+0x1AAFA0`) is **guarded**: it only calls `CreateSoundBank` if
  `mgr->bank[slot] == NULL && mgr->file_id[slot] == -1`. **A second `.xsb` mapping to the
  same slot is silently ignored.** Since any unrecognised basename maps to slot **5**, and
  slot 5 is occupied by the current song's bank during gameplay, **we cannot register our
  own sound bank through the game's manager while a song is loaded.**
- Streaming wave banks are opened with
  `CreateFileA(native_path, GENERIC_READ, FILE_SHARE_READ, NULL, OPEN_EXISTING,
  FILE_FLAG_OVERLAPPED|FILE_FLAG_NO_BUFFERING, NULL)` where `native_path` is produced by
  `XCnbrep7000046` (libavs-win64 ordinal 71, an AVS path→native-path conversion).
  **[INF]** ⇒ streaming wave-bank *data* bypasses `avs_fs_open`, so this project's
  LayeredFS `avs_fs_open` hooks likely do **not** redirect it. In-memory wave banks
  (`se_system`, `se_normal`) and **all** sound banks *do* come through the file
  manager/AVS path and **are** LayeredFS-reachable.
- `CreateStreamingWaveBank` is called with a params struct of
  `{HANDLE file; u32 flags=0; u32 offset=0; u32 packetSize=0x20}` **[OBS]**.

---

## Play/Stop API + inferred signatures

### `se_play` — the one function you want

```c
// +0x1AA6E0   MS x64, extern "C"
// OBSERVED ABI from the disassembly:
//   ECX  = bank_id   (i32)   0=bgm_menu 1=se_system 2=se_normal 3=voice 5=song
//   RDX  = cue_name  (const char*, ASCII, NUL-terminated)
//   XMM2 = pan       (f32)   -1.0 = left, 0.0 = center, +1.0 = right
//   EAX  = handle    (u32)   0xFFFFFFFF on failure
uint32_t se_play(int32_t bank_id, const char* cue_name, float pan);
```

**[OBS] proof of the float ABI** (this is the detail that would silently break a Rust
binding): `+0x1AA6F9 MOVAPS XMM6, XMM2` / `+0x1AA74B MOVAPS XMM2, XMM6` — the third
argument travels in **XMM2**, not `R8D`. Rust `extern "system" fn(i32, *const c_char, f32)
-> u32` maps exactly.

Behaviour **[OBS]**:
1. If `bank_id != 1 && bank_id != 5`, calls `(*se_mute_filter)(&state)` with `state = 5`;
   if it comes back `6`, returns `0xFFFFFFFF` without playing.
2. Takes the AVS lock if `audio_lock_count > 0`.
3. `se_play_inner` (`+0x1AB7A0`): `sb = mgr->bank[bank_id]`; if `sb == NULL` → return `-1`;
   `idx = sb->GetCueIndex(cue_name)`; if `idx == 0xFFFF` → return `-1`;
   `hr = sb->Play(idx, 0, 0, &cue)`; if `hr < 0` → return `-1`;
   else register `cue` into a free handle slot with the pan and bank id, return the slot.

**Safety assessment for calling this from a hook [OBS unless noted]:**
- ✅ No asserts, no exceptions, no allocation. Unknown cue names are handled by the
  `0xFFFF` sentinel → clean `-1` return. Missing bank → clean `-1`.
- ✅ Nothing is lazily created — the manager and all four boot banks exist from
  `Application::onBoot` onward.
- ⚠️ **`mgr` itself is dereferenced unconditionally** (`MOV RSI, [0x1806F2D60]` then
  `MOV RBX, [RSI + RAX*8]`). If the global is still `0`, this is a null deref. **We must
  null-check `*(void**)0x1806F2D60` ourselves before every call.**
- ⚠️ `(*se_mute_filter)` is an indirect call through a global — also unchecked. It is
  populated by boot; **[INF]** safe after boot completes. Sidestep entirely by using
  `bank_id == 5`… which we can't (slot 5 collision), or by calling `se_play_inner`
  (`+0x1AB7A0`) directly, which skips the filter.
- ⚠️ Handle-slot exhaustion: 256 slots, round-robin; if all are live, `+0x1AB5B0`
  **destroys the cue it was given** and returns `-1`. No leak, no crash.
- **[INF]** Thread affinity: the wrapper takes an AVS lock, so it is probably
  thread-safe, but the game only ever calls it from the game thread. Stay on the game
  thread.

### The rest of the façade (all signatures **[INF]** from arg shapes, bodies **[OBS]**)

```c
// +0x1A82F0  the front door 39 call sites use.
//   bank_id : same as se_play
//   cue_name: ASCII
//   side    : 0 = P1 (pan -1.0), 1 = P2 (pan +1.0), >=2 = center (pan 0.0)
//             -- pan only applied when mgr+0x20C4 (stereo enable) is set
//   mode    : 0 = fire & forget, no bookkeeping
//             1 = stop the previous handle for [side][bank], play, remember handle + ms
//             2 = same as 1 but ONLY if >= 20 ms since the last play for [side][bank]
void se_play_helper(int bank_id, const char* cue_name, int side, int mode);

uint32_t se_prepare (int bank_id, const char* cue_name, float pan);  // +0x1AA5C0
void     se_start   (uint32_t handle);                               // +0x1AA680
void     se_stop    (uint32_t handle);                               // +0x1AA7C0
void     se_stop_all(void);                                          // +0x1AA850
bool     se_is_playing(uint32_t handle);                             // +0x1AA8F0
void     se_pause   (uint32_t handle);                               // +0x1AB840
void     se_set_volume(int category, float volume);                   // +0x1AA950
```

`mode == 2`'s 20 ms retrigger guard (`+0x1A82F0`, constant `0x14` compared against
`timeGetSystemTime(&mmt, TIME_MS)`) is **[OBS]** and is worth noting: the game already
has the exact "don't machine-gun the same SE" logic an assist tick needs on dense charts
— though for an assist tick we probably want *no* guard (a 16th-note stream at 200 BPM is
75 ms apart, so 20 ms wouldn't bite anyway).

---

## Custom-Sample Feasibility

### Q3a — can the loader take arbitrary content?

**Yes, two independent ways.**

**Route A — LayeredFS into `se_normal.arc` (data-side).** **[OBS]** `se_normal.arc` is a
plain uncompressed single-entry ARC whose payload is `se_normal.xwb`, and it is loaded
through the file manager/AVS and handed to `CreateInMemoryWaveBank`. This project already
repacks ARCs at open time (`src/services/avs_layeredfs/arc_handler.rs`, and
`shader_synthesis.rs` proves runtime synthesis of arc entries works). So: parse the stock
`se_normal.xwb`, **replace one existing wave entry's data with our clap**, rewrite the
XWB, serve it via the arc overlay, then play the corresponding stock cue name with
`se_play(2, "se_...", pan)`. The stock `se_normal.xsb` is left untouched — cue → wave
index mapping is preserved, so nothing else needs to be authored.
Cost: we sacrifice one existing SE (choose a menu-only one), and we repack a 17.7 MB arc
once into the LayeredFS cache.

**Route B — talk to the XACT engine directly (code-side, recommended).** **[OBS]** both
`CreateSoundBank` (engine `+0x48`) and `CreateInMemoryWaveBank` (engine `+0x50`) are
called by the game itself, so their vtable indices **and full argument shapes are
observed facts, not guesses**. We can therefore, at gameplay init:

```c
IXACT2Engine* eng = *(IXACT2Engine**)0x1806F2D60_deref;   // *(void**)mgr
eng->CreateInMemoryWaveBank(our_xwb_bytes, our_xwb_len, 0, 0, &our_wb);
eng->CreateSoundBank      (our_xsb_bytes, our_xsb_len, 0, 0, &our_sb);
XACTINDEX idx = our_sb->GetCueIndex("tick");        // SoundBank +0x00, OBSERVED
// per note:
our_sb->Play(idx, 0, 0, &cue);                      // SoundBank +0x20, OBSERVED
// reap when stopped (or reuse the game's reaper semantics):
cue->GetState(&st);  if (st & 0x20) cue->Destroy(); // Cue +0x10 / +0x18, OBSERVED
```

This keeps our banks **entirely out of the game's slot table**, so the slot-5 collision
problem disappears, no game data file is touched, and every vtable index used is one the
game itself exercises. XACT matches a sound bank to its wave bank **by name** at runtime,
so our XSB's wave-bank-name field just has to match our XWB's bank name. `DoWork()` is
already called every frame by `frame_main` (`+0x3020`), so we don't have to pump anything.

> **[INF]** Passing `ppCue = NULL` to `Play` for true fire-and-forget is documented XACT
> behaviour but is **not** observed here (the game always passes an out-pointer and reaps
> the cue itself). Prefer the observed pattern: take the cue and reap it, mirroring
> `+0x1ABB30`.

### Q3b — what formats does it accept?

**[OBS]** `WAVEBANKMINIWAVEFORMAT` codec field is 2 bits: `0=PCM, 1=XMA, 2=ADPCM, 3=WMA`.
**[OBS]** DDR's own song wave banks use **codec 2 (MS-ADPCM), 2 ch, 44100 Hz,
block_align_raw = 48** (i.e. 140-byte blocks, 128 samples/block).

**This is already solved in Rust.** the sibling `ddr-chart-tools` repository (a sibling
project of this repo) contains, **[OBS]**:

- `src/xwb/container.rs` — full **XWB v43 parser *and* writer** for
  "XACT2 header_version 42 (DDR's format)": 5-segment layout, 96-byte bank-data block,
  24-byte entry metadata, packed `WAVEBANKMINIWAVEFORMAT`, entry name table, configurable
  bank flags + alignment (streaming banks pad each entry to 2048).
- `src/xwb/adpcm/{encode,decode}.rs` — MS-ADPCM encoder and decoder.
- `src/ogg/decode.rs` — Ogg Vorbis → PCM (`AudioBuffer`). **Our clap is already an Ogg**,
  so the whole chain Ogg → PCM → MS-ADPCM → XWB exists today.
- `src/xsb/mod.rs` + `docs/xsb_format.md` — a **from-scratch XSB writer for DDR World**,
  including the **CRC-16 over bytes `[0x12..]` stored at `0x08`** (the engine silently
  rejects the bank and goes dark if it's wrong) and the **cue-name hash function**, both
  reverse-engineered from `xactengine2_10.dll` itself. Emits the DDR profile: 1 wave bank,
  2 simple cues (`{code}` + `{code}_s`), 2 sound entries, 16 hash buckets, 326 bytes.

So the "author a valid XSB/XWB pair" problem — normally the hard part of this plan — is
already implemented, documented, and validated against this exact engine version.
Caveats **[INF]**: the XSB writer emits exactly the 2-cue song profile (we'd use the main
cue and either give the XWB two entries or point both cues at entry 0), and the XWB writer
defaults to streaming-style 2048-byte entry alignment — for `CreateInMemoryWaveBank` we
want the buffer bank flags/alignment, which is a parameter, not a rewrite.

### Verdict

**YES — we can play a custom short sample through the game's own XACT mixer.**
Strongest evidence, in order:
1. **[OBS]** `CreateInMemoryWaveBank` + `CreateSoundBank` + `SoundBank::GetCueIndex` +
   `SoundBank::Play` + `Cue::GetState/Destroy` are all called by `gamemdx` itself, so
   every vtable index and argument list we need is a directly observed fact.
2. **[OBS]** A working, tested Rust XWB+XSB writer for this exact engine version already
   exists in a sibling project, including the CRC-16 the engine validates.
3. **[OBS]** The game already plays a per-note SE from inside its judge loop, so the
   whole "trigger a short cue per arrow on the game thread" pattern is proven in-engine.

Also worth stating: because the **song audio itself** is an XACT streaming wave bank in
the same engine and the same final mix **[OBS]**, a tick played this way inherits exactly
the music's output latency. Any self-hosted output path (our own XAudio2/WASAPI client)
would add an independent, unknown, device-dependent offset — fatal for a timing-critical
feature. This is the strongest argument for native integration beyond mere style.

---

## Existing SE Call Sites

### The one that matters: per-note SE inside the judge loop **[OBS]**

`GamePlayActor::judgeNotes` @ **`+0x5EC70`** (name is certain — the function logs
`"sequence::dance::GamePlayActor::judgeNotes"` with format string
`"shock ng : pressedDir=%d, musicCount=%d, note.musicCount=%d, diff=%d"`) contains an
**inlined copy** of `se_play_inner`, at the shock-arrow hit branch. String
`"se_game_shockarrow"` is at `+0x360D28`, loaded at **`+0x5F290`**; the play block is
approximately `+0x5F1D0 .. +0x5F2C0`:

```c
int state = 5;
(*se_mute_filter)(&state);
if (state != 6) {
    lock_if(audio_lock_count > 0);
    IXACT2SoundBank* sb = *(void**)(mgr + 0x30);          // 0x30 = (2+1)*0x10 => bank 2 = se_normal
    XACTINDEX idx = sb->GetCueIndex("se_game_shockarrow"); // vtable +0x00
    if (idx != 0xFFFF) {
        IXACT2Cue* cue = NULL;
        if (sb->Play(idx, 0, 0, &cue) >= 0)               // vtable +0x20
            handle_slot_alloc(mgr, cue, /*bank*/2, /*pan*/ pan_for_side(actor->side));
    }
    unlock_if(...);
}
```

`pan_for_side` is the same `mgr+0x20C4` / side-0 / side-1 ladder as `se_play_helper`,
reading the player side from `actor + 0x84` **[OBS]**.

**This is the cheapest possible integration point.** An assist tick is literally this
block with a different cue name, called once per arrow instead of once per shock hit.

### Other gameplay-scene SE call sites **[OBS]**

| Cue | String | Call site |
|---|---|---|
| `se_game_shockarrow` | `+0x360D28` | `+0x5F290` (in `judgeNotes`) |
| `se_game_miss` | `+0x361D90` | `+0x70CF3` in `FUN_1800709A0` |
| `se_game_fullcombo` | `+0x361730` | (xref not chased) |
| `se_game_clear` / `se_game_failed` | `+0x35DE30` / `+0x35DE00` | (result transitions) |
| `se_measure_start` | `+0x35DDD8` | referenced from a name **table** at `+0x35E1F0` |
| `se_common_count_down` | `+0x35F040` | |

**[OBS]** ~101 `se_*` cue names exist in `.rdata` (menu, e-pass, result, dan, event,
select). There is also `vo_*` (voice, bank 3). Names are **plain ASCII strings resolved by
`GetCueIndex` at call time** — no client-side hashing, no id table. That means adding a
cue name costs nothing on the game side.

**[OBS]** Non-obvious but useful: there is a **name table** of cue-name pointers at
`+0x35E1F0` (that's how `se_measure_start`'s only xref is a data slot) — i.e. some SEs are
selected by index into a table rather than by a literal at the call site.

---

## Timing/Latency Notes

**[OBS]**
- `Initialize` is given `lookAheadTime = 250` (ms). **[INF]** in XACT this governs
  *streaming* wave-bank read-ahead, not the latency of an in-memory cue.
- `engine->DoWork()` is called **exactly once per frame** from `frame_main` (`+0x3020`),
  immediately after the cue reaper. It is *also* called right after each Play/Stop in the
  wrapper paths (`+0x1AAFA0`, `+0x1AB050`, `+0x1AB720`, `+0x1ABF90`). So the notification
  and streaming service interval is one frame (16.7 ms @ 60 fps; the `fps-unlock` mod can
  make it shorter).
- `SoundBank::Play(idx, dwFlags, timeOffset, ppCue)` — the game **always** passes
  `dwFlags = 0` and `timeOffset = 0`. Nothing in `gamemdx` ever passes a non-zero
  `timeOffset`, and nothing uses `Prepare` + a scheduled start. **⇒ Playback is
  fire-and-forget; there is no sample-accurate absolute scheduling in use.**
  **[INF]** whether XACT2's `timeOffset` even implements scheduled start is unverified
  (in some XACT versions it is documented as reserved/unused). Do not build a design on it
  without testing.
- **[INF]** Consequence for the mod: an assist tick will be quantized to the frame on
  which we detect the note, exactly like the game's own `se_game_shockarrow`. At 60 fps
  that is up to ~16.7 ms of jitter, which is audible as looseness on fast streams. If that
  proves unacceptable, the mitigations are (a) hook a higher-frequency tick than the
  render frame, (b) fire the tick slightly early with a fixed lead and accept constant
  offset, or (c) investigate `timeOffset` / `Prepare`+`Play` scheduling — in that order of
  increasing risk.
- **[OBS]** The `SOUND_OFFSET` timing-config key (default `87`, see
  `docs/hex_edit_porting.md` §"Hack 4" and `src/core/signatures.rs`
  `timing_config_set_int`) is a **chart-timing** compensation, not a mixer parameter: it
  is published into the timing config map alongside `INPUT_OFFSET` / `RENDER_OFFSET` /
  `BOMB_FRAME_OFFSET`. **[OBS] negative result:** I did **not** find its consumer inside
  the audio module (`+0x1AA000..+0x1AC000` contains no reference to the timing config).
  `+0x1ACAE0` (called from boot right after audio init) allocates a separate 0x28-byte
  object at `+0x6F2D68` whose per-frame update `+0x1ACD40` runs from `frame_main` —
  **[INF]** a plausible audio/chart sync object, but unconfirmed. **An assist tick must
  respect `SOUND_OFFSET` the same way the arrows do**, i.e. derive its trigger from the
  same music-time source the judge uses (`musicCount` in `judgeNotes`), not from a
  wall-clock timer.
- **[OBS]** No mixer buffer size is visible in `gamemdx`; it is entirely inside
  `xactengine2_10.dll` / DirectSound.

---

## Open Questions / What I Could Not Determine

1. **The XACT2 engine vtable beyond `+0x58`.** `IXACT2Cue` provably deviates from
   `xact3.h` (`SetMatrixCoefficients` at `+0x40` not `+0x20`, `Pause` at `+0x60` not
   `+0x40`) **[OBS]**, so `PrepareWave` / `PrepareInMemoryWave` **cannot** be assumed at
   `+0x60`/`+0x68`. They may not exist in v2.10 at all. *This is the biggest unknown, and
   it is the reason Route B above is specified using only game-exercised methods.*
   **How to close it:** `contents/com/xactengine2_10.dll` is on disk (x64, 404 KB) —
   import it into Ghidra and read the vtable initializers directly. That single step also
   confirms `IXACT2WaveBank::Play`'s index.
2. **`IXACT2WaveBank::Play` index** (`+0x28` per `xact3.h`) — never called by the game.
   Only matters if we want to play a wave without a sound bank. Same fix as (1).
3. **`Play`'s `timeOffset` semantics in v2.10** — is scheduled start supported? Same fix.
4. **The `_n` asset variants** (`se_normal_n.arc` etc.) — which set the running game
   actually consumes. Matters for Route A only. Cheap to settle with a LayeredFS verbose
   log of the boot arc opens.
5. **`se_mute_filter` (`+0x6F2420`) policy** — what makes it return `6` (veto). It is
   bypassed for bank ids 1 and 5. If it vetoes during gameplay for bank 2 we'd silently
   lose ticks; `judgeNotes` calling it per shock-hit suggests it does not, but that is
   **[INF]**.
6. **Where `ddr.xgs` actually comes from** (not a loose file on disk).
7. **Whether `+0x6F2F48` is the same object the repo's `asset_loader.rs` resolves as
   `file_manager_singleton`.** Very likely **[INF]**; trivially checkable at runtime by
   logging both pointers.
8. **Handle-slot pressure.** 256 slots shared with the whole game. A tick per arrow on a
   dense chart plus the game's own SEs — **[INF]** the reaper frees STOPPED cues every
   frame and a 0.21 s clap occupies a slot for ~13 frames, so worst case is maybe 20–30
   concurrent slots. Should be fine, but measure. Route B (our own sound bank, our own
   cue bookkeeping) avoids the game's slot table entirely and sidesteps this.

---

## Cross-Version Caution

- Everything here is read off **`gamemdx_20260721.dll`**. This repo has 20260324,
  20260421, and 20260616 also loaded in Ghidra; the audio module was **not** diffed
  across them. Function addresses **will** move (the 20260526 `FileManager::Load` in
  `docs/customizer_asset_loading.md` is `+0x1FE720`, while this build's load-by-path is
  `+0x1FEF30` — a ~0x800 shift in the same neighbourhood).
- **Anchor by content, not address**, per project convention:
  - The audio-manager singleton and the whole façade are reachable from the **unique**
    string `"data/sound/win/ddr.xgs"` (boot) and from `bank_slot_of_file`'s literal table
    `"bgm_menu" / "se_system" / "se_normal" / "voice"` — four adjacent string pointers,
    an extremely stable fingerprint.
  - `se_play` / `se_play_inner` are best anchored via a known cue-name string
    (`"se_game_shockarrow"` for the inlined judge-loop copy) or by AOB over
    `+0x1AB7A0`'s distinctive prologue (`MOV RSI,[rip+mgr]` / `MOVSXD RDI,ECX` /
    `LEA RAX,[RDI+1]` / `ADD RAX,RAX` / `MOV RBX,[RSI+RAX*8]` / `CALL [RAX]` /
    `CMP AX, 0xFFFF`).
  - The XACT vtable indices are properties of `xactengine2_10.dll`, **not** of `gamemdx` —
    they are stable across game builds as long as the shipped engine DLL version does not
    change. Fingerprint the engine DLL, not the game DLL, if we hard-code indices.
- `mgr` member offsets (`+0x10 + slot*0x10`, `+0xA0 + i*0x20`, `+0x20C4`) are struct
  layout and may shift if Konami adds a member. Prefer deriving the sound-bank slot
  pointer from `se_play_inner`'s own arithmetic (or just call `se_play_inner`) over
  hard-coding `mgr+0x30`.
- **[OBS]** `se_normal.arc` differs in size between the shipped `se_normal.arc`
  (17,740,288 B, dated Mar 24) and `se_normal_n.arc` (19,484,928 B, dated May 26) — asset
  content is version-dependent too, so Route A's "replace wave entry N" must locate its
  victim entry **by wave name / index read from the parsed bank at runtime**, never by a
  baked-in byte offset.
