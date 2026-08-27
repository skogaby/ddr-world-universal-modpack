# XACT Audio Research — DDR World's Sound Engine, and Playing Mod-Owned Sounds Through It

Consolidated reverse-engineering record from the assist-tick feature
(`.agents/planning/20260725-assist-tick/research/` holds the full per-topic notes; this file is
the durable summary, because the findings are reusable well beyond that feature). All gamemdx
addresses are file-relative to the module's `0x180000000` image base and refer to the
**2026-07-21** build unless noted; nothing in shipped code hard-codes them — resolution is by the
AOB patterns and derivations in `src/core/signatures.rs` (`se_play`, `se_play_inner_body`,
`bank_slot_of_file_loop` + `derive_game_audio_addresses`).

The shipped consumer of all of this is `src/services/game_audio.rs`, which owns the entire
game-ABI surface described here in one auditable file.

## 1. Architecture

- The engine is **COM-instantiated Microsoft XACT 2** (`xactengine2_10.dll`, shipped with the
  game; base `0x00400000`). Its output backend is DirectSound, resolved dynamically. The game
  binary itself imports **no audio API at all** — no XAudio2, no DirectSound, no WASAPI.
- *Everything* audible — menu BGM, voice, every sound effect, and the song audio itself — passes
  through one engine instance and one final mix. This is why a mod sound should play through it
  rather than through a self-hosted audio client: a cue played here inherits the music's exact
  output latency. (A self-hosted XAudio2 path was fully investigated and works, but adds an
  independent, device-dependent offset that cannot be measured in-process — rejected for
  timing-critical use.)
- The engine is created inside the game's `Application::onBoot`, which completes **after** a
  hook DLL's init thread finishes. Consequence: `GetModuleHandleA("xactengine2_10.dll")` at DLL
  init time returns null; any engine-presence check must happen at first use on the game thread.
- `IXACT2Engine::DoWork()` is called exactly once per frame by the game's main loop (call site
  `+0x3020`), together with a **per-frame cue reaper** (`+0x1ABB30`) that destroys finished cues.
  Mod code never needs to pump the engine or reap its own cues.
- Audio submission is **frame-quantized**: work is submitted once per frame with no scheduled
  start times, so a freshly triggered cue becomes audible on the next mix after its frame, plus
  the backend's mixing-buffer latency (large under Wine/CrossOver DirectSound — measured
  ~125–150 ms trigger-to-audible there; small on native hardware).

> **Amendment 2026-07-29 (pre-mixed-tick-track RE spike — corrects and sharpens the last point
> above).** The *trigger* is frame-quantized, but the engine's own start grid is a second,
> independent quantizer: `SoundBank::Play` (vt`+0x20`) → `Sound_Play` computes each wave's
> scheduled start (`Wave_ComputeScheduledStartMs`: `qpc_now_ms + XSB_event_time`), inserts it
> into a time-sorted queue (`Sound_InsertWaveSortedOrStartNow`), and that queue is drained ONLY
> by the engine's **notify-thread pump** (`Sound_PumpUpdate_DrainScheduledWaves`), signalled by
> the DirectSound render thread every **~10 ms packet**. `DoWork` (game thread) never starts
> waves. Three consequences, all live-confirmed on the cabinet:
>
> 1. **No sample-accurate start exists** — the started voice
>    (`Wave_StartNow_NoSampleOffset`, `voice->Start(1,0,0,0)`) carries no sample offset, so
>    individually triggered cues quantize to the packet grid no matter how precisely they are
>    triggered. Per-tick triggering therefore has an irreducible ~±½ frame ±½ packet
>    inter-onset jitter; only content pre-mixed *inside one voice* is sample-exact — which is
>    why the assist-tick mod synthesizes one whole-song tick track per song
>    (`services/se_bank_synth`) and plays it as a single cue.
> 2. **`Play`'s `timeOffset` parameter is NOT a seek.** It only fast-forwards the cue's *event*
>    timeline (`sched = now + event_time − timeOffset`); a wave already due starts at
>    **sample 0**. Seeking into content is done by rewriting the wave's sample bytes shifted
>    by whole MS-ADPCM blocks (self-contained, 128 samples = 2.90 ms each) — see
>    `game_audio::rewrite_tick_wave`.
> 3. **The engine never copies an in-memory wave bank's data** — it reads the client-owned
>    buffer lazily for the bank's lifetime, so a bank registered once can have its sample
>    bytes rewritten in place between songs (header immutable; rewrite only after an immediate
>    `SoundBank::Stop`, vt`+0x28`, flags=1). Also note the engine pairs sound↔wave banks by
>    internal name **globally**: two banks sharing a name cross-pair (live-confirmed as "wrong
>    wave plays"), hence the tick bank's name `astk` ≠ the retired per-tick bank's `asti`. The
>    tick bank claims **no manager slot** (the slots' only readers are the `se_play` façade,
>    which the tick path bypasses to dispatch `Play`/`Stop` directly on the retained
>    `IXACT2SoundBank*`).
>
> Engine functions named in the shared Ghidra project (`xactengine2_10.dll`): `SoundBank_Play`
> `0x423990`, `SoundBank_Stop` `0x423b80`, `Sound_Play` `0x422ab0`,
> `Sound_PumpUpdate_DrainScheduledWaves` `0x422da0`, `Wave_ComputeScheduledStartMs` `0x4136c0`,
> `Sound_InsertWaveSortedOrStartNow` `0x421b10`, `Wave_StartNow_NoSampleOffset` `0x414180`,
> `Engine_NotifyThreadPump` `0x411850`. Full record:
> `.agents/planning/20260729-assist-tick-premixed-track/research/` + that feature's design doc.

## 2. The audio manager and its six sound-bank slots

An in-house "audio manager" singleton wraps the engine. Its global pointer **moves on every game
build** and is therefore derived by RIP-relative decode from inside the `se_play_inner_body`
pattern match, never scanned directly (`+0x6F2D60` on 20260721).

Layout (verified at runtime before any write):

| Offset | Field |
|---|---|
| `+0x00` | `IXACT2Engine*` |
| `+0x08 + n*0x10` | slot `n` `int file_id` (`-1` = empty) |
| `+0x10 + n*0x10` | slot `n` `IXACT2SoundBank*` (`NULL` = empty) |

Six slots (`n` in `0..=5`), proven three independent ways: the constructor's memset size, its
twelve `-1` stores, and the slot destroyer's loop terminator.

**Slot mapping** (`+0x1AA3C0`): a loaded bank file's basename selects its slot — `bgm_menu`→0,
`se_system`→1, `se_normal`→2, `voice`→3, anything else→5 (the per-song bank). **Slot 4 is never
produced by that mapping** and stays free for the process lifetime. The count of named banks is
an imm8 in the mapper (`audio_named_bank_count_site`, asserted `== 4` at registration time —
a fifth named bank in a future build would map to slot 4 and collide).

### The load-bearing trick: a bank the game can never destroy

The only code in the game that destroys a sound-bank slot (`+0x1AB3D0`) selects its victim with a
linear *"find the slot whose `file_id` equals this file id"* search over all six slots, and
destroys nothing on no-match. Therefore: register a mod-owned bank by writing **only the slot's
bank pointer and leaving `file_id` at `-1`** — a value that can never be a live file id — and the
bank survives every song load, song unload, and scene transition for the process lifetime.
Writing a plausible `file_id` (the tempting "fix" for the half-populated slot) would make the
destroyer target it and the sound would die mid-session with no error. Nothing in the game reads
the two fields together except its own bank loader's admission guard, so the half-populated state
is inert. Verified live: one registration, claps across four song loads in one session.

Registered banks are deliberately **never** destroyed by the mod either — destroying an XACT bank
a live cue may still reference is a known crash class, and an idle bank costs one pointer plus
two leaked buffers.

## 3. Playing a cue: `se_play`

The public "play a sound effect" façade (`+0x1AA6E0`):

```
u32 se_play(i32 bank_id /*ECX*/, const char* cue /*RDX*/, f32 pan /*XMM2*/)
```

- **ABI trap:** the pan argument is a float, so under Microsoft x64 it travels in **XMM2**, not a
  GPR. Rust declaration: `unsafe extern "system" fn(i32, *const c_char, f32) -> u32`. An integer
  third parameter compiles and silently passes garbage.
- `bank_id` is the manager slot index. Cue names are resolved by the sound bank at call time
  (byte-exact `strcmp` — case-sensitive); there is no client-side hash or id table, so new cue
  names cost nothing.
- Returns a handle into the manager's shared **256-entry cue handle table**, or `0xFFFFFFFF` on
  failure (unknown cue, or table exhausted — exhaustion *leaks* a cue rather than crashing).
- The inner play entry (`se_play_inner`, `+0x1AB7A0`) skips the game's sound-effect mute filter
  (at the cost of the AVS lock the public entry takes). Resolved but unused: the filter was
  proven live **not** to veto a mod bank in slot 4.
- The game itself plays a per-note SE from inside its judge loop (the shock-arrow hit sound), so
  "play a cue per note from a judge callback" is an engine-exercised pattern.

## 4. Creating banks at runtime

Only vtable indices the game itself exercises are safe — `IXACT2Cue`'s layout provably deviates
from the public XACT 3 headers, so guessing "reasonable" slots is not.

| Interface | Index | Method |
|---|---|---|
| `IXACT2Engine` | `+0x48` | `CreateSoundBank(this, pv, cb, dwFlags, dwAllocAttrs, ppOut)` |
| `IXACT2Engine` | `+0x50` | `CreateInMemoryWaveBank(same shape)` |
| `IXACT2SoundBank` | `+0x00` | `GetCueIndex(this, PCSTR) -> u16` (`0xFFFF` = not found) |
| `IXACT2SoundBank` | `+0x20` | `Play(this, cueIndex, dwFlags, timeOffsetMs, ppCue)` — `timeOffset` is NOT a seek (§1 amendment); `ppCue=NULL` = engine auto-release |
| `IXACT2SoundBank` | `+0x28` | `Stop(this, cueIndex, dwFlags)` — only flag bit 0 (`STOP_IMMEDIATE`) is legal; any other bit ⇒ `E_INVALIDARG` |

These are properties of `xactengine2_10.dll`, stable across game builds as long as the shipped
engine DLL doesn't change — hence the engine-module presence check before the first dispatch.

Rules established by disassembly and confirmed live:

1. **Create the wave bank first.** An in-memory wave bank is fully prepared synchronously inside
   the call (its "prepared" notification fires before it returns). `CreateSoundBank` does *not*
   resolve wave banks (linking is by internal name, late), but ordering removes any dependence on
   that lazy resolution.
2. **The engine retains both byte buffers.** Neither the wave data nor the XSB is copied;
   `Box::leak` both for the bank's (= process) lifetime, *before* handing pointers to the engine.
3. Banks are paired by **internal name**, byte-identical including case — never by filename.
   (The repo's files are `tick.{xwb,xsb}` with internal bank/cue name `asti`, deliberately
   different, exactly as the game's own `se_system.xwb` carries internal name `SE_SYSTEM`.)
4. **A malformed sound bank fails silently in the game's own loader** — it ignores the HRESULT
   and audio simply goes dark. Mod code must log the HRESULT itself, and bank files should be
   generated and validated offline, never synthesized ad hoc.

Useful HRESULT vocabulary (full table in the planning research's `xact-bank-format.md`):
`0x8AC70007` malformed bank (header/CRC/structure), `0x8AC70006` wrong bank type (in-memory bank
marked streaming), `0x8AC70002` engine not initialized, `0x80070057` null buffer/zero size.

## 5. Bank file format constraints (the engine's own validator)

For generating XWB/XSB pairs the engine will accept (the in-process synthesizer lives in
`src/services/se_bank_synth/` — ports of the sibling `ddr-chart-tools`' writers;
`scripts/validate_se_bank_synth.sh` replays these rules offline against the sibling's own
parser/decoder, plus byte-identity checks of the ports):

- **Wave bank:** buffer (non-streaming), `header_version` 42, entry-name element size 64
  (enforced even when unused), alignment 4; exact segment offsets are checked, and file length
  minus the wave-data segment offset must equal that segment's length exactly.
- **Codec:** MS-ADPCM (codec 2). Every wave entry in every DDR bank on disk is ADPCM; raw PCM is
  structurally accepted but entirely unexercised on this cabinet. Mono is proven in use.
- **Sound bank:** the DDR profile — one wave bank, simple cues, a 16-bucket cue-name hash table,
  and a **CRC-16 over the file that the engine validates and silently rejects on mismatch**.
  Gameplay SEs use mix **category 6** with a bare sound entry and **no** runtime-parameter curve;
  the song profile (category 4/3 + RPC curve) would put the sound on the music bus and reference
  global audio state.

## 6. Cross-version anchors

The three AOB patterns (`se_play`, `se_play_inner_body`, `bank_slot_of_file_loop`) matched
uniquely on all four builds available at research time (2026-03-24, 2026-04-21, 2026-06-16,
2026-07-21). The manager global is always derived, never scanned. Runtime guards before the one
pointer write: named-bank count still 4, and the `se_normal` slot's bank pointer non-null (proves
both the slot-array layout and that normal boot completed).
