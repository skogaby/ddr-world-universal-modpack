# Orientation — Assist Tick

**Date:** 2026-07-25
**Purpose:** blind-spot pass before requirements. What the idea has to live in, what I found
that changes it, and what is still unknown.

Companion research (written the same day, more detail than this file):
- `research/game-sound-engine.md` — RE of the game's audio subsystem (Ghidra, `gamemdx_20260721.dll`)
- `research/audio-output-feasibility.md` — could we host our own audio output instead
- `research/existing-mechanisms.md` — the codebase APIs a new mod would use

---

## 1. What the idea asks for

Play StepMania's clap sample at each arrow's intended hit time, toggled by a per-player row
on the in-game MODS options tab. Rough idea assumed (a) the existing judgment hooks are the
trigger point and (b) audio playback is net-new work.

## 2. Findings that change the idea

### 2.1 The trigger must be chart time, not judgment

StepMania's assist tick is chart-driven: it claps at the note's timestamp whether or not the
player hits it, and regardless of *when* they hit it. The game's judgment fires on player
input (early/late) or on late-window expiry — so triggering off judgment would make the clap
track the player's errors instead of the beat, which inverts the feature's purpose.

The judge hook is still the right vehicle, but as a **clock**, not an event source:
`judge_hook`'s callback signature is `fn(actor: *mut u8, music_count: i32)`
(`src/services/judge_hook.rs:61`), it fires once per frame per side during gameplay, and
`music_count` is **milliseconds** (corroborated three ways in `research/existing-mechanisms.md` §C3).
Each note record carries its own `music_count` at `+0x08` (`src/mods/note_types_expansion/game_note.rs:38`).
So: build a sorted list of note timestamps once per song, then advance a cursor per frame.

### 2.2 Audio is not net-new work — the game hands us a usable API

DDR World's audio is **Microsoft XACT 2** (`xactengine2_10.dll`, COM-instantiated at
`+0x1AA000`; DirectSound backend). *Everything* — menu BGM, voice, all SEs, and the song
audio itself — goes through one engine instance and one final mix. `gamemdx.dll` has zero
XAudio2/DirectSound/WASAPI/ASIO imports of its own.

Three consequences:

1. **There is a clean, callable play-by-name function.**
   `se_play(i32 bank_id, const char* cue, f32 pan) -> u32 handle` at `+0x1AA6E0`, MS x64,
   pan in **XMM2** (observed, not inferred). No asserts, no allocation, clean `-1` on unknown
   cue or missing bank. ~101 `se_*` cue names are resolved by plain ASCII `GetCueIndex` at
   call time — no id table, no hashing on the game side.

2. **The exact pattern we need already exists in the judge loop.**
   `GamePlayActor::judgeNotes` (`+0x5EC70`) contains an inlined SE play of
   `se_game_shockarrow` on the shock-hit branch, panned by the actor's own side
   (`actor+0x84`). An assist tick is that block with a different cue name, fired per arrow.
   This also proves the SE mute filter does not veto during gameplay.

3. **Playing through the game's mixer inherits the music's own output latency.**
   Any self-hosted output path (our own XAudio2 client) would add an independent,
   device-dependent, unmeasurable offset — fatal for a timing aid. This is a stronger
   argument for native integration than style is.

The self-hosted path was investigated in full anyway (`research/audio-output-feasibility.md`):
it would *work* (DirectSound is shared-mode, so no device contention — spice2x even
`regsvr32`s the XACT engine at boot), needs no new crate dependencies, and could reach
~1-sample placement with an always-fed source voice. But it carries an
endpoint-divergence risk that is undetectable in-process, and it does not inherit the
music clock. Keep it as documented prior art, not as a second code path.

### 2.3 Getting our *own* sample in is tractable, and mostly already built

Custom-sample routes, in order of preference:

- **Our own banks, parked in an unused game bank slot.** `CreateInMemoryWaveBank`
  (engine vtable `+0x50`) and `CreateSoundBank` (`+0x48`) are both called by the game
  itself, so their indices and argument shapes are *observed*. The game's bank-slot mapper
  (`+0x1AA3C0`) assigns `bgm_menu→0, se_system→1, se_normal→2, voice→3, everything-else→5`
  — **slot 4 is never produced**. If we create our banks ourselves and write the
  `IXACT2SoundBank*` into `mgr->bank[4]`, then `se_play(4, "…", pan)` gives us the whole
  native façade for free: panning, the 256-slot handle table, the per-frame cue reaper, and
  the cabinet's SE volume category. Self-checking too: assert `bank[2] != NULL` (se_normal
  is loaded) and `bank[4] == NULL` before writing.
- **LayeredFS a replacement wave into `se_system.arc`/`se_normal.arc`** (both are in-memory
  banks that *do* come through AVS, unlike the streaming banks). Works, but sacrifices an
  existing SE and repacks a 17.7 MB arc.

And the hard part — authoring a valid XWB+XSB pair for *this engine version* — is already
implemented, documented and validated in a sibling project:
the sibling `ddr-chart-tools` repository has `src/xwb/container.rs` (v43 parser **and** writer,
parameterized bank name/flags/alignment/format), `src/xwb/adpcm/` (MS-ADPCM codec),
`src/ogg/decode.rs` (Ogg → PCM — our clap is already Ogg), and `src/xsb/mod.rs` +
`docs/xsb_format.md` (from-scratch XSB writer including the CRC-16 the engine silently
validates and the cue-name hash, both RE'd from `xactengine2_10.dll`). `xsb::write(code)`
emits soundbank name = wavebank name = main cue name = `code`, with the main cue pointing at
wave **index 1**.

So the asset can be generated offline, once, and embedded with `include_bytes!` (~20 KB).

### 2.4 Per-side is the natural implementation, not the special case

The rough idea proposes "if both players enable it, follow P1's chart". But there is one
`GamePlayActor` **per side** (`src/mods/quick_restart_or_fail.rs:266-295` literally returns a
`Vec` of them; each carries its play side at `+0x84`), and the judge hook fires per side with
that side's actor. So per-side ticking — each enabled side hearing its own chart, panned to
its own side exactly like the game's own SEs — is *less* code than the P1-priority rule, and
removes the stated limitation. When both sides are on the same difficulty the two ticks
coincide (L+R at the same instant ≈ one centered clap), which is the "nothing should be
noticed" outcome the idea asks for.

## 3. Constraints inherited from the codebase

- Frame-quantized triggering: the game calls `engine->DoWork()` exactly once per frame
  (`frame_main` `+0x3020`), always with `dwFlags=0, timeOffset=0` — there is no
  sample-accurate scheduling in use anywhere. Expect ~±8 ms jitter at 60 fps with
  half-frame lead centering; less with the `fps-unlock` mod.
- `mgr` (`+0x6F2D60`) is dereferenced unchecked by `se_play` — we must null-check it.
- Scene callbacks fire *before* the next scene is built, so no actor exists at GAMEPLAY
  entry: latch options on the scene event, build the tick list on the first judge tick
  (what `note_types_expansion` already does).
- `on_change` for a custom option can fire on the init thread and on a spawned JSON-prime
  thread, not just the render thread → atomics only.
- The option-label texture atlas flushes **once** at boot (`src/lib.rs:359`), so a
  newly-installed row has no label until the next launch.
- Don't call game functions from the DLL init thread; gate on a hook-proven ready latch.
- Don't derive note times from the SSQ — `timing.rs::beat_to_music_count` has a suspected
  TPS normalization bug. Read timestamps off the game's own note records.

## 4. Unknowns worth closing before/while designing

| # | Unknown | How to close | Blocking? |
|---|---|---|---|
| R1 | Is bank slot 4 genuinely free for the process lifetime — does anything iterate/tear down bank slots? | Ghidra: xrefs to the bank-slot array in `mgr`; check scene-teardown paths | Yes — picks the integration shape |
| R2 | Does `CreateInMemoryWaveBank` accept codec 0 (raw PCM), or must we MS-ADPCM encode? Correct BUFFER-bank flags/alignment? | `xactengine2_10.dll` into Ghidra (it's on disk, 404 KB) + one cabinet test | Yes — asset format |
| R3 | Which note kinds are taps vs freeze heads vs shocks in the Notes/Results vector (`kind` at `+0x00`), and are shocks distinguishable? | Ghidra on the judge/render classifiers + a diagnostic build logging kinds | Yes — "what gets a tick" |
| R4 | Version-stable AOB anchors for `se_play` and the `mgr` global | Content-anchored fingerprints exist: the `bgm_menu/se_system/se_normal/voice` string quad, `se_game_shockarrow`, and `se_play_inner`'s distinctive prologue | No — mechanical |
| R5 | Real perceived jitter on cabinet | Deploy and listen; compare 60 vs 120 fps | No — measure after v1 |

## 5. Proposed sequence

Requirements register first (the audio-path question is already answered by research, so the
open decisions are mostly behavioral), then close R1–R3 with targeted RE, then design.
Implementation should front-load risk by proving the trigger machinery on cabinet with an
**existing** game cue before introducing our own banks.
