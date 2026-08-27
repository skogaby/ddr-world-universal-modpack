# Idea Honing — Assist Tick

**Date:** 2026-07-25
**Status:** register fully accepted 2026-07-25 (D3 and D8 by maintainer override; D10 confirmed no-taint; remainder approved wholesale)

Decision register. Ordered by blast radius — D1–D5 change the architecture, data flow, or
user-visible behavior; D6–D15 are narrower or reversible. `Status` is one of `Proposed`,
`Accepted`, `Overridden`, `Assumed`, `Open`.

★ = a decision the rough idea did not consider.

| ID | Decision | Why it matters | Recommendation | Status |
|----|----------|----------------|----------------|--------|
| D1 | Audio path | Determines whether the tick shares the music's clock | Route through the game's own XACT engine; **no** self-hosted output path | Accepted |
| D2 ★ | Trigger source | Judgment-driven ticks would track the player's errors, not the beat | Chart-time driven; judge hook used as a per-frame clock only | Accepted |
| D3 | 1P/2P semantics | Decides which chart the single tick stream follows | Single centre-panned stream; solo side wins regardless of cabinet side, else P1 priority | **Accepted** (agent proposal overridden) |
| D4 | Which notes tick | Wrong set = ticks on things you don't step on | Taps + jumps (one per timestamp) + freeze heads; not tails, shocks, or mines | Accepted |
| D5 | Custom sample delivery | Decides whether we touch game data files | Our own XWB+XSB, generated offline, embedded, parked in unused bank slot 4 | Accepted |
| D6 | Option row shape | Row shape is baked into persisted per-player values | 2-value OFF/ON (`bool_toggle`), `PersistMode::Full` | Accepted |
| D7 | Volume control | Cheap now, awkward to retrofit into a persisted enum | None in v1 — bake the level, ride the cabinet SE volume | Accepted |
| D8 | Latency offset knob | Deferred — revisit in a targeted session if cabinet testing shows a need | **No knob and no config section.** Internal half-frame lead only, as one named constant | **Accepted** (agent proposal overridden) |
| D9 | Jitter tolerance | Sets whether v1 needs risky sub-frame work | Accept ~±8 ms (half-frame lead centering); measure, then escalate if needed | Accepted |
| D10 | Score-submission taint | StepMania disqualifies assist clap from ranking; this repo suppresses assisted scores | **No taint** — assisted plays upload normally; maintainer may revisit | Accepted |
| D11 | Note-timestamp source | SSQ-derived timing has a suspected TPS bug | Walk the actor's Results vector on the first judge tick per song | Accepted |
| D12 | Code placement | Project has a strict layering rule | New `services::game_audio` + thin `mods::assist_tick` | Accepted |
| D13 | Naming | Ids are persisted and operator-visible | mod `assist-tick`, option `assist_tick`, cue/bank `asti` | Accepted |
| D14 | Degradation | An in-process hook must never take the game down | Self-disable with a WARN on any missing piece; never a second audio path | Accepted |
| D15 | Sample licensing/provenance | StepMania asset redistributed in this repo | Embed it; note provenance + attribution in README | Assumed |
| D16 | XSB mix category (raised by research) | Song profile would put the tick on the **music** bus with an RPC curve attached | Emit the **SE profile (category 6, bare sound, no RPC)** — needs a small additive change in the sibling `ddr-chart-tools` XSB writer | Proposed |
| D17 | `se_play` vs `se_play_inner` | The mute filter applies to bank 4; the lock does not apply to the inner call | Call `se_play` (keeps the AVS lock); the filter provably does not veto in gameplay. Fall back to `se_play_inner` only if a cabinet test shows vetoing | Assumed |
| D18 | Free-slot selection | Hard-coding 4 breaks silently if a future build adds a fifth named bank | **Compute** the free slot at install time and verify it is free; never write `file_id[slot]` | Assumed |

---

## D1 — Audio path: the game's own XACT mixer

**Question.** Do we play the clap through the game's audio engine, or open our own audio
output from inside the process?

**Recommendation.** The game's engine. Create our own XACT in-memory wave bank + sound bank
and play cues through the existing `se_play` façade.

**Rationale.** The game's audio is Microsoft XACT 2 (`xactengine2_10.dll`) and *everything*
— menu BGM, voice, all SEs, and the song audio itself — goes through one engine instance and
one final mix. A tick played there inherits the music's exact output latency; a self-hosted
client adds an independent, device-dependent offset we cannot measure in-process, which is
fatal for a timing aid. It also inherits panning, the cabinet's SE volume category, and the
engine's per-frame cue reaper for free, and needs no new crate dependencies.

**Alternatives rejected.**
- *Self-hosted XAudio2.* Fully investigated and it would work (DirectSound backend ⇒
  shared-mode ⇒ no device contention) with ~1-sample placement via an always-fed voice. Rejected
  because of (a) no shared clock with the music, (b) an endpoint-divergence failure mode
  that is silent and undetectable in-process. Kept as documented prior art only — we do not
  build two audio paths.
- *LayeredFS-replace an existing SE's wave data.* Works (the two in-memory banks do come
  through AVS) but sacrifices a stock SE and repacks a 17.7 MB arc.

---

## D2 ★ — Trigger source: chart time, not judgment

**Question.** The rough idea assumes the arrow-judgment hooks are the trigger point. Are they?

**Recommendation.** No — trigger on the note's own chart timestamp, independent of player
input. The judge hook is still the vehicle, used as a per-frame clock.

**Rationale.** StepMania's assist tick claps on the beat whether or not you hit the note, and
regardless of how early or late you hit it. The game's judgment fires on player input or on
late-window expiry, so a judgment-driven clap would follow the player's mistakes — the
opposite of a timing reference. Mechanically: `judge_hook` gives us
`fn(actor, music_count: i32)` once per frame per side, `music_count` is milliseconds, and each
note record carries its own `music_count`. We pre-build a sorted timestamp list and advance a
cursor.

**Consequence.** Ticks continue through missed notes — which is correct and is the whole point.

---

## D3 — 1P/2P semantics: one centre-panned stream, solo side wins, else P1

**Status: Accepted 2026-07-25.** The agent proposed per-side panned streams; the maintainer
overrode it. Recorded in full because the rejected reasoning contains a trap worth not
repeating.

**Accepted rule.**
- Pan is always `0.0` (**centre**). Never side-panned. This also removes any dependency on
  the cabinet's pan-by-side switch (`mgr+0x20C4`), deleting an unknown from the design.
- Exactly **one** tick stream, following exactly one side's chart, chosen once per song:

  | Session | Tick side |
  |---|---|
  | Solo, either cabinet side | that player's side (P2-side solo ticks P2's chart) |
  | 2P, both sides enabled | **P1** |
  | 2P, only one side enabled | that side |
  | Doubles | the single actor |

- The choice is made by enumerating the **active** actors, not by scanning per-side option
  values — otherwise an inactive side's stale persisted `on` value could hijack the tick.
  `src/mods/quick_restart_or_fail.rs:266-295` already walks the DPS child list and returns the
  live `GamePlayActor`s, each carrying its play side at `+0x84`; reuse that shape. Re-run on
  quick restart.
- Judge callbacks arriving from any other actor do nothing.

**Why the per-side proposal was wrong.** It assumed stereo panning bought player isolation.
An arcade cabinet is one shared stereo mix in one room — panning P1's ticks left does not stop
P2 hearing them, it only makes them quieter and off-centre. So with differing charts,
"per-side" gives *both* players two interleaved tick streams rather than each player their
own reference. The "it's less code" argument was also marginal (the side-selection logic it
avoids is ~30 lines) and is moot next to the acoustic problem.

**Bonus of the accepted rule:** the "both players on the same difficulty → nothing should be
noticed" requirement becomes literally true (one clap) rather than approximately true (two
coincident claps).

**Assumed sub-decision (D3a).** Active-side enumeration uses the DPS actor-list walk. If that
walk proves unreliable at gameplay start (research R6), fall back to: lock onto the first
actor observed whose side has the option enabled, preferring side 0 if both are seen in the
same frame.

---

## D4 — Which notes get a tick

**Recommendation.** Tick on: normal taps, jumps (**one** tick per distinct timestamp per side,
not one per panel), and freeze **heads**. Do **not** tick on: freeze tails, shock arrows, or
mines (this repo's own injected note type).

**Rationale.** Matches StepMania (claps on tap rows including freeze heads; one clap per row).
Shocks and mines are notes you must *avoid*, so a "step here" cue would be actively
misleading. Dedup by timestamp is required — a jump is multiple panel entries at the same
`music_count`, and three simultaneous claps would just be louder.

**Depends on research R3** (the note-kind taxonomy at note `+0x00` — currently
`ARROW=0, THINOUT=1, FREEZE_TAIL=2`, with shock classification not yet pinned down).

---

## D5 — Custom sample delivery: our own banks in unused slot 4

**Recommendation.**
1. Transcode `clap.ogg` → 16-bit PCM mono 44.1 kHz **offline**.
2. Generate `tick.xwb` (BUFFER wave bank) + `tick.xsb` (sound bank, cue name `asti`)
   **once, offline**, using the sibling `ddr-chart-tools` XWB/XSB writers.
3. Commit both blobs (~20 KB) and `include_bytes!` them into the DLL.
4. At runtime, on the game thread: `CreateInMemoryWaveBank` + `CreateSoundBank`, then write
   the resulting sound-bank pointer into the game's **unused bank slot 4**, and play with
   `se_play(4, "asti", 0.0)` (centre pan — see D3).

**Rationale.** The game's bank-slot mapper produces only slots 0,1,2,3,5 — slot 4 is
unreachable by any game code path. Parking our bank there gives us the entire native façade
(pan, handle table, per-frame reaper, SE volume category) for the cost of one pointer write,
and it is self-verifying: assert `bank[2] != NULL` (se_normal loaded) and `bank[4] == NULL`
before writing. No game data file is touched, no arc is repacked, and every vtable index we
use is one the game itself exercises.

The normally-hard part — authoring an XSB the engine accepts (it validates a CRC-16 and goes
silently dark if it's wrong) — is already implemented and validated against this exact engine
version in `ddr-chart-tools`.

**Sub-decisions.**
- **PCM vs MS-ADPCM:** recommend raw PCM (codec 0). 19 KB is nothing and it skips the
  encoder. Pending research R2 (does an in-memory bank accept codec 0).
- **Operator-swappable sound:** recommend a cheap optional override — if
  `data_mods/assist_tick/tick.xwb` + `tick.xsb` exist on disk, load those instead of the
  embedded blobs. Pure `fs::read`, no format code in the DLL.
- **Deferred:** in-process WAV → bank synthesis (drop a `.wav`, we build the banks). Nice
  feature, but a malformed bank fails *silently*, so v1 should ship a blob validated offline.

**Fallback if slot 4 turns out not to be free (R1):** keep our own sound-bank pointer, call
`SoundBank::Play` directly, and run our own cue reaper on the judge hook. Costs pan and the
volume category.

---

## D6 — Option row: per-player OFF/ON on the MODS tab

**Recommendation.** `RegisterSpec::bool_toggle("assist_tick")` with `PersistMode::Full`,
registered through the `custom_options` service — same shape as `autoplay` and `premium_free`.
`bool_toggle` *is* the 2-value OFF/ON enum row and reuses the game's stock OFF/ON ribbon
sprites, so no value-label assets are needed.

**Cost.** One shipped label texture (`seop_item_assist_tick.png`, 176×16) plus a line in
`scripts/gen_option_labels.py`. Note a framework quirk: the label atlas flushes once at boot,
so the row's label appears only after the next launch following install.

`PersistMode::Full` = network save + network load + offline JSON cache, i.e. the setting
follows the player's card. That is right for a per-player gameplay preference.

---

## D7 — Volume control: none in v1

**Recommendation.** Bake a sensible level into the sample. Because we play through bank slot 4
via the game's façade, the clap automatically rides the cabinet's SE volume category.

**Rationale.** Adding volume *now* means either an enum row with custom value labels (real
asset cost) or a second config knob. And changing a 2-value row into a 4-value row *later*
migrates every player's persisted value. If volume proves necessary, the cheap path is to
author several cues at different levels in our own XSB (it's ~330 bytes) and select by option
value — a self-contained follow-up.

---

## D8 — Latency offset knob: none

**Status: Accepted 2026-07-25.** The agent proposed a config value plus an overlay scalar row;
the maintainer deferred the whole thing — if cabinet testing shows the clap needs nudging, it
gets its own smaller, targeted PDD session.

**Consequences.**
- **No `assist_tick` section in `mod-config.json` at all.** The mod is gated purely by
  `mods["assist-tick"]`, so there is no new config struct, no serde defaults, no README config
  documentation, and no example-config change.
- No `mod_menu::register_scalar_row` usage, hence no overlay rows to register or remove on
  disable.
- The internal **half-frame lead** that centres frame-quantization error (see D9) stays, but as
  a single named `const` in one place, so promoting it to a knob later is a one-line addition
  rather than a refactor.


---

## D9 — Jitter: accept frame quantization in v1

**Recommendation.** Ship with frame-quantized triggering (~±8 ms at 60 fps after lead
centering) and measure on cabinet before doing anything clever.

**Rationale.** The game calls `engine->DoWork()` exactly once per frame and always plays with
`timeOffset = 0` — there is no sample-accurate scheduling anywhere in the binary, and whether
XACT 2.10's `timeOffset` even implements scheduled start is unverified. Note the reference
point: the game's own `se_game_shockarrow` has exactly this jitter. Also, note spacing is
generous relative to a frame (16ths at 200 BPM are 75 ms apart), so ticks never collide.

**Escalation ladder if it sounds loose,** in increasing order of risk: (1) confirm improvement
at 120 fps via the existing `fps-unlock` mod; (2) test `Play`'s `timeOffset` semantics against
the real engine; (3) leading-silence-plus-seek to place the attack sub-frame.

---

## D10 — Score-submission taint: none

**Status: Accepted 2026-07-25** (maintainer: "let's not taint the scores, I can revisit that
later if I decide it was the wrong call").

Assisted plays upload normally. The mod registers **no** `score_guard` interaction of any kind —
no per-side taint, no session flag, and no readiness gate on `score_guard::is_available()`
(unlike `autoplay`, which fails closed).

**Rationale.** The tick alters nothing about judgment, timing windows, or scoring; it makes
audible what is already on screen. It is closer to `real_speed_fix` or the playfield styling
options than to Autoplay.

**Recorded counter-argument, for whoever revisits this.** StepMania classifies assist clap as
an *assist* and disqualifies it from ranking, and this repo has a deliberate posture of not
uploading assisted or incomplete scores. If the call is reversed, the change is small and
local: mirror `autoplay`'s per-side taint (`score_guard::mark_*`) while the option is on for a
side that is playing. Note that reversing it would also raise the fail-closed question —
whether the mod should refuse to enable when suppression can't be guaranteed.

---

## D11 — Note-timestamp source: the actor's Results vector

**Recommendation.** On the first judge tick of each song, walk the side's Results vector via
the existing helpers (`actor_results_range` reading `actor+0xB0/0xB8`, then `for_each_result`
at stride 0x40), collect each eligible note's `music_count` (`note+0x08`), dedup, sort into a
per-side `Vec<i32>`, and advance a cursor per frame thereafter.

**Rationale.** No new hooks, no new signatures, no SSQ parsing — and specifically *not* SSQ
parsing, because `timing.rs::beat_to_music_count` has a suspected TPS-normalization bug
(returns raw ticks at the file's TPS while the engine normalizes to ms). Reading the game's
own note records sidesteps that entirely. Building on the first judge tick rather than at
GAMEPLAY scene entry is required: scene callbacks fire *before* the next scene is built, so no
actor exists yet.

**Also handles:** quick restart — if `music_count` goes backwards, reset the cursor.

---

## D12 — Code placement

**Recommendation.** Two pieces:
- `src/services/game_audio.rs` — the XACT/SE binding: resolve `se_play` + the audio-manager
  global, create and register our banks, expose `play_cue(bank, cue, pan)` /
  `play_tick()` / `is_available()`.
- `src/mods/assist_tick.rs` — the mod: option row, per-side latch, judge-hook subscription,
  timestamp list + cursor.

**Rationale.** The project's layering rule puts game-system integrations in `services/` and
mod behavior in `mods/`. A sound service is reusable (custom judgment SEs, menu SEs, a
metronome mod), and it keeps the `unsafe` game-ABI surface in one auditable file.

---

## D13 — Naming

- mod registry id: `assist-tick` (kebab, matches `mods` map convention)
- option id: `assist_tick` (snake — becomes `<mod_assist_tick>` on the wire and
  `seop_item_assist_tick` as the label texture)
- our XACT bank/cue name: `asti` (soundbank name = wavebank name = cue name; the sibling XSB
  writer ties these together, and 4 chars matches DDR's own song-code profile)
- config section: none (see D8)
- display label: `ASSIST TICK`

---

## D14 — Graceful degradation

**Recommendation.** The mod self-disables with a single WARN — never a panic, never a crash,
never a fallback audio path — if any of these hold: the `se_play` signature doesn't resolve;
the audio-manager global is null when we first need it; wave-bank or sound-bank creation
fails; bank slot 4 is already occupied; `judge_hook` is unavailable.

Bank creation happens on the **game thread at first gameplay entry**, never on the DLL init
thread (the project has a documented crash class for calling game functions from init before
their globals exist).

---

## D15 — Sample provenance (Assumed)

The clap is StepMania's `assist_tick` asset, extracted by you. I'm assuming embedding it in
this repo is acceptable for a private-cabinet modpack, and that the right thing to do is note
its provenance and attribute StepMania in the README. Say so if you'd rather ship the mod
without the asset and have operators supply their own file (D5's override path already
supports that).

---

## Open research (to close after the register is accepted)

| # | Item | Blocking |
|---|------|----------|
| R1 | Is bank slot 4 free for the process lifetime — does anything iterate or tear down bank slots? | D5's shape |
| R2 | Does `CreateInMemoryWaveBank` accept codec 0 (raw PCM)? Correct BUFFER-bank flags/alignment? | asset format |
| R3 | Note-kind taxonomy: taps vs freeze heads vs shocks in the note record | D4 |
| R4 | Version-stable AOB anchors for `se_play` and the audio-manager global | mechanical |
| R5 | Perceived jitter on cabinet at 60 vs 120 fps | post-v1 measurement |
| R6 | Does the DPS actor-list walk reliably enumerate active sides at gameplay start (it currently exists for a different purpose — forcing game over)? | D3a |

---

## D16 — XSB mix category: emit the SE profile, not the song profile

**Raised by research (`research/xact-bank-format.md` §8.3), not present in the original register.**

**Finding.** The sibling `ddr-chart-tools` XSB writer emits DDR's **song** profile: sound
category 4/3 with an RPC (runtime parameter curve) `0xF8` attached. The game's gameplay SEs
instead use **category 6** with a bare 12-byte sound entry and no RPC. Both are structurally
valid — the engine accepts either — but the category determines which mix bus the cue rides.

**Recommendation.** Emit the SE profile. Consequences of not doing so: the tick would follow
**music** volume rather than SE volume and would be subject to any category-level ducking
curve, and it would carry an RPC that references `ddr.xgs` state we never set — unpredictable
rather than merely mis-bussed.

**Cost.** A ~12-byte output difference in the sibling repo's XSB writer — additive (a new
`write_se(code)` entry point or a profile parameter; the existing song path stays untouched).
This is a **cross-repo prerequisite** for the asset-generation step and must appear in the
implementation plan as such. The CRC-16 and cue-name hash are already confirmed verbatim
against the engine and reproduce all five stock banks, so only the sound-entry bytes change.

**Alternative considered and rejected.** Ship the song profile and live on the music bus. Cheap,
but it trades a known-good 12-byte change for an unpredictable RPC. Rejected.

---

## D17 — Which entry point plays the cue (Assumed)

Call **`se_play`** (`+0x1AA6E0`). It takes the AVS lock when `audio_lock_count > 0`, which
`se_play_inner` does not, and matches what the game itself does everywhere.

Bank id 4 is **not** exempt from the `se_mute_filter` veto (only ids 1 and 5 are), but the
filter provably does not veto during gameplay — `judgeNotes`' own inlined `se_game_shockarrow`
play goes through it on bank 2 and is audible. If a cabinet test ever shows ticks being
swallowed, the fallback is to call `se_play_inner` (`+0x1AB7A0`) directly, which skips the
filter; note that also skips the lock.

---

## D18 — Free-slot selection is computed, not hard-coded (Assumed)

Research confirmed slot 4 is unreachable on all four builds in the Ghidra project, but the
guard is nearly free, so: at install time, **scan** the 6-slot array for a slot whose
`IXACT2SoundBank*` is NULL **and** whose `file_id` is `-1`, verify the expected slot is the one
found, and decline to install if none is free. Never write `file_id[slot]` — leaving it at `-1`
is precisely what makes the game's only slot destroyer (a `find_if` on `file_id`) structurally
unable to match our slot.

Also verify `bank[2] != NULL` (i.e. `se_normal` is loaded) before trusting the layout — a cheap
sanity check that the slot-stride assumption holds on this build.

---

## D5 amendment — asset delivery by file, not by `include_bytes!`

**Amended 2026-07-25**, after confirming the in-repo precedent: `shader_fixes` ships committed
binary blobs under `data_mods/shader_fixes/blobs/*.d3dbc` and loads them **at runtime** through
`mod_paths::find_first_modfile("blobs/<name>")` (`src/services/avs_layeredfs/shader_synthesis.rs:121`),
degrading to stock behavior when they're absent.

Assist tick mirrors that exactly:

```
data_mods/assist_tick/banks/tick.xwb      # generated offline, committed (~5.5 KB)
data_mods/assist_tick/banks/tick.xsb      # generated offline, committed (~330 B)
data_mods/assist_tick/source/clap.ogg     # StepMania source sample, committed (10 KB)
scripts/build_assist_tick_bank.sh         # ffmpeg -> ddr-chart-tools; documents the one-time run
```

Rationale: one code path instead of embed-plus-override; the operator sound swap becomes the
primary path rather than a special case; a missing or malformed pair funnels into the D14
self-disable we need anyway.

**Accepted tradeoff (small):** the built DLL is installed on its own, so the game install needs a
one-time copy of `data_mods/assist_tick/` before ticks are audible — the same operational
requirement `shader_fixes` already has. Testing is local (CrossOver install root given by
`$DDR_WORLD_INSTALL`), so this is a `cp -r` into that directory, not a remote sync.
`include_bytes!` remains a one-line alternative if a self-contained DLL is ever preferred.

---

## Readiness Confirmed 2026-07-25

Maintainer confirmed D16 and the register as a whole ("everything else looks good to me").
All 18 decisions are Accepted or Assumed; none Open.

**Design rests on these assumptions:**
1. Embedding the StepMania clap in this repo is acceptable for a private-cabinet modpack (D15).
2. `se_mute_filter` does not veto during gameplay for bank ids other than 1 and 5 — evidenced by
   `judgeNotes`' own `se_game_shockarrow` play on bank 2 being audible, but not proven for bank 4
   until a cabinet test (D17).
3. The 6-slot sound-bank array stride holds on the running build — verified at runtime before use
   rather than assumed (D18).

**Research backing:** XACT audio subsystem RE; audio-output alternative feasibility; codebase
API survey; bank-slot lifetime + cross-build signature verification (4 builds); XWB/XSB format
with the engine's own validator transcribed and a real bank generated and checked; note-kind
taxonomy + actor enumeration with the shock test verified at six sites.

**Deliberately deferred:** latency knob (D8), volume control (D7), score taint (D10),
in-process WAV→bank synthesis (D5).

**Deferred to implementation-time diagnostic builds (not design blockers):** sibling-actor list
completeness on the first judge tick; whether anything produces `state[i] > 1`; doubles started
from the P2 card reader; the tick-coalescing window on TPS-150 charts.
