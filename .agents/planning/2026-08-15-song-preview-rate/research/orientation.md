# Orientation: real-time rate preview at song select

Date: 2026-08-15. Blind-spot pass over the repo, the RE notes, and the
song-rate engine before requirements clarification.

## The idea in repo terms

Make the song-select preview (the `<code>_s` entry of the slot-5 dance XWB)
play at the currently desired `song_speed` rate with the currently desired
`preserve_pitch` DSP mode, updating live (re-triggering) when either row's
value changes while the preview is playing.

## Territory findings

### 1. Preview mechanics (already reverse-engineered)

From `docs/training_mode_research.md` §8.1 and `docs/xact_streaming_research.md`
§4/§5/§8:

- The preview player (`FUN_18010eab0` on 20260721) reads the highlighted song
  shared_ptr at `DAT_1806f2d50+0x1B0`, builds `data/sound/win/dance/<code>` +
  cue name `<code>_s`, and calls `FUN_1801ccd10(mgr+0xC8, 5, path, cue)` =
  FileManager load + slot-5 play. Fires when the wheel settles on a song.
- The preview XWB **is the same file gameplay uses** (two entries: main
  `<code>` + preview `<code>_s`). Creates go through `wavebank_create`
  (`song_rate_wavebank_create` — already detoured);
  `song_rate::selected_song` already publishes `{code_digest, main_len_ms}`
  on every create (armed or not, preview or gameplay).
- `wavebank_create` duplicate guard: a live bank with the same `file_id`
  returns 0 (no create). `wavebank_unregister` destroys the engine bank,
  closes the handle, removes the handle→file_id entry — already detoured
  with a pre-original binding retire (`unregister_prelude`).
- Entry metadata (duration, data region) is **fixed at header-parse time**
  (the 0x1000 header read inside create). A live bank cannot change its
  declared entry length ⇒ a rate change of a *playing* preview requires
  stop-cue → unregister → re-create (fresh header) → re-play.
- There is **no retail seek path** — restart-from-0 is the only re-trigger
  semantic available.

### 2. Streaming engine reusability

- `binding::prepare_binding(file_id, generation, percent, preserve_pitch,
  source, fault)` → virtual-bank plan + 16 MiB ring + generator thread;
  `io_callback_hook` serves reads for the bound file_id; `preserve_pitch`
  already selects WSOLA vs plain resample (`DspState` seam). WSOLA is only
  ~2.4× realtime under CrossOver — full pre-synthesis is unaffordable, but
  streaming start is sub-second (the cue needs only the first packets).
- **Limitation A:** `core/xact/virtual_bank.rs::plan_virtual_bank` hardcodes
  main-entry-stretched + side-entry-verbatim (the preview-passthrough
  deviation, maintainer-approved 2026-08-11). This feature needs the
  *inverse* plan (stretch `_s`, main verbatim) and the ring/generator
  main-only target ranges generalized to "target entry".
- **Limitation B:** the io hot path consults ONLY the single active binding
  (`io_callback_hook.rs::bound_verdict` → `registry().with_active`). A
  preview binding needs its own registry slot consulted on the miss path
  (one extra Acquire per unbound read).
- The gameplay bind is entangled with the two-stage transaction (Q31 clock,
  score ledger, movie suppression, `XactSlots`, lifecycle phases). A preview
  bind must touch **none** of that — it is an audio-serving concern only.
  This makes the preview bind *simpler* than the gameplay bind, but it must
  be a separate qualification path in the create detour, not a reuse of
  `bind_for_create`.

### 3. Play/stop primitives (all resolved already)

From `docs/xact_audio_research.md` + `src/services/game_audio.rs`:

- `se_play(slot=5, cue_name, pan/*XMM2*/)` — public façade, resolved,
  live-proven; resolves cue names by strcmp at call time.
- `IXACT2SoundBank` vt+0x00 `GetCueIndex(name)`, vt+0x20 `Play`, vt+0x28
  `Stop(cueIndex, STOP_IMMEDIATE)` — engine-DLL-stable, exercised by the
  tick-bank machinery.
- The manager's slot-5 sound-bank pointer sits at `mgr + 0x10 + 5*0x10`
  (manager global already derived by `derive_game_audio_addresses`).
- Stopping by cue index stops all instances of that cue — acceptable (one
  preview instance at a time).

### 4. Change-callback contract

`custom_options/api.rs:246`: `on_change` runs on the game's thread but the
documented contract is atomics-only (no game APIs, no locking). It also
fires on **every scroll tick** and on framework events (card-in seeding,
persistence load). ⇒ the callback may only stash desired state; the actual
re-trigger must be debounced and executed from the song-rate runtime's
permanent scene callback / 250 ms drain (`song_rate/runtime.rs`), which is
the established pattern for this kind of deferred game-API work.

### 5. In-flight work (collision check)

- `2026-08-13-training-mode`: Step 8/9 complete, only docs (Step 9) remain;
  it landed the ring-rewind/scrub machinery on `Binding` and
  `selected_song`. No overlap beyond shared files; coordinate doc edits.
- `2026-08-12-preserve-pitch-toggle`: landed — gives us the DSP-mode seam
  for free.

### 6. Diagnostics already available

`BankTimeline` (in `wavebank_hook`) records create/unregister events with
file_id on diagnostic boots — exactly the tool to answer the
unregister-on-confirm question (R-A below) with one cabinet session.

## The critical safety question

**R-A: does the game unregister the preview bank when a song is confirmed
(select → loading), before the gameplay create?** If yes (expected — the
loading scene reloads song assets), the preview binding retires naturally
through the existing unregister detour and the gameplay create re-parses a
fresh header. If no, the gameplay path would hit the duplicate guard and
*reuse the live preview bank object* — whose header the engine parsed with
preview-stretched durations — which would corrupt gameplay audio length.
The design must either confirm the unregister (timeline observation +
static RE of the select→loading transition) or force unregister+recreate of
any preview-bound bank on scene-25 exit. **Blocking research item.**

## Other unknowns (research items)

- **R-B: re-trigger composition.** Preferred: compose from primitives we
  already hold (SoundBank Stop + unregister trampoline + create call +
  `se_play(5, cue)`) — zero new signatures. Risks: the game's preview play
  path may apply fade/volume/category state `se_play` doesn't; the game
  holds a cue handle for its preview (256-entry handle table) and may later
  Stop a stale/recycled handle. Fallback: derive `FUN_1801ccd10` (the
  game's own load+play — one new signature + ABI confirmation, and its
  same-file semantics need static reading anyway for R-A).
- **R-C: preview cue behavior** — loop? game-driven stop/fade after N
  seconds of wall time? Determines what a slow-rate (longer) preview does
  at its tail (likely: the game stops it on wheel move / timeout — fine).
- **R-D: where the game stores its preview cue handle** (for stale-handle
  risk assessment in R-B), or confirmation that `Stop(cueIndex)` is
  sufficient and safe.
- **R-E: `_s` entry format** — presumed stereo MS-ADPCM 47 kHz like the
  main entry (same file); confirm `xwb::parse_song_bank` exposes what the
  planner variant needs (it already parses both entries).
- **R-F: scene gating** — confirm the options modal (MODS tab) can only
  fire value changes at scene 25 (song select) and what scene id the
  drain should gate on; confirm wheel movement is impossible while the
  modal is open.

## Consequences for scope

- Once armed (desired ≠ 100), *every* wheel-settle preview can play at rate
  via create-time binding — no re-trigger needed for wheel moves; the
  re-trigger dance is only for value changes while a preview is playing.
- Identity (100%) keeps zero footprint: no preview binds, no re-triggers;
  changing TO 100 needs one final re-trigger to restore the stock preview.
- Versus: one cabinet-global preview vs two per-side desired rates — needs
  a policy decision.
