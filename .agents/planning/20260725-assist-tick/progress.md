# Progress — Assist Tick

**Updated:** 2026-07-27
**Status:** **FEATURE COMPLETE — all 6 steps done and ticked.** Nothing committed (the maintainer
owns commits); both working trees green.

**NEXT ACTION:** none for the feature. Remaining for the maintainer: commit the work (this repo:
the feature; sibling `ddr-chart-tools`: Step 1 + the two pre-existing repairs), and the optional
open calls in "Deviations & open questions" (ADPCM quantizer rounding, item E; shellcheck pass,
item G; sibling structure.md doc touch-up, item H).

**Resume protocol:** read `design/detailed-design.md` (approved; dated 2026-07-26 amendments in
§4.1/§4.2.3/§5.3/§6/Appendix C) → this file → `implementation/plan.md`'s checklist.
`idea-honing.md` holds the 18 accepted decisions; `research/` the reverse-engineering record.
Per-task working records live under `implementation/tasks/<task>/`.

---

## Done

### Step 1 — the clap bank pair (2026-07-26)

Three tasks, all complete, all gates green, **nothing committed** (the maintainer owns commits).

| Task | Repo | Outcome |
|---|---|---|
| 01 — SE-profile XSB writer | `ddr-chart-tools` | `xsb::write_se` alongside `xsb::write`: one cue, one bare 12-byte sound entry, mix category 6, no RPC curve, wave index 0. Song profile proved byte-identical by a golden fixture captured off the pre-change build |
| 02 — bank-pair generator | `ddr-chart-tools` | `job::se_bank::generate` + `xwb::dump::describe` + a second binary `ddr-se-bank` (`generate` / `dump`). Deterministic; the dump is a documented `key=value` interface |
| 03 — build script + assets | this repo | `scripts/build_assist_tick_bank.sh` and the committed `data_mods/assist_tick/` tree |

```
data_mods/assist_tick/source/clap.ogg   10,704 B  Ogg Vorbis mono 44100 Hz, 9,423 samples
data_mods/assist_tick/banks/tick.xwb     5,416 B  sha256:46e6602892dc681c…
data_mods/assist_tick/banks/tick.xsb       262 B  sha256:d2fe533ea65e03e6…
```

Bank/cue name **`asti`**; wave bank has **1 entry**, 74 ADPCM blocks, duration 9,472 samples. The
engine's own validator rules were replayed offline against both files with the game's own banks as
controls (both PASS), and ten single-byte corruptions each tripped the expected rule. Generation is
byte-reproducible.

Full detail: `implementation/tasks/{se-profile-sound-bank-writer,se-bank-pair-generator,asset-build-script-and-banks}/progress.md`.

### Step 2 — `services::game_audio` (2026-07-26)

Two tasks, both complete, gates green, **nothing committed**.

| Task | Outcome |
|---|---|
| 01 — audio signature patterns | 3 patterns (`se_play`, `se_play_inner_body`, `bank_slot_of_file_loop`) + `derive_game_audio_addresses` in `src/core/signatures.rs`. Verified in Ghidra on **all four** builds *and* live |
| 02 — `services::game_audio` | `src/services/game_audio.rs` (`init` / `is_available` / `register_bank` / `play_cue`) + `services/mod.rs` + `lib.rs` step 6b1, plus the temporary Step-2 demo trigger |

**Resolved on the running build (20260721), all matching the research note's per-build table:**

```
se_play                    +0x1AA6E0      se_play_inner_body   +0x1AB7AF   (matches=1)
se_play_inner              +0x1AB7A0      (prologue verified)
audio_manager_global       +0x6F2D60      (RIP-decoded, moves every build)
audio_named_bank_count_site +0x1AA46C     (named bank count = 4)
```

**What the final boot proves** (`implementation/tasks/game-audio-service/logs/boot-final.log`):

```
GameAudio: slot layout OK (se_normal slot 2 bank = 0x14facf00)
GameAudio: claiming free sound-bank slot 4 (of 6)
GameAudio: CreateInMemoryWaveBank('asti', 5416 bytes) hr=0x00000000
GameAudio: CreateSoundBank('asti', 262 bytes)         hr=0x00000000
GameAudio: bank 'asti' registered in slot 4 (file_id left at -1 deliberately)
GameAudio: cue 'asti' -> index 0 in bank 'asti' (slot 4)
GameAudio demo: song-start clap at music_count=-87 played=true     ×4 songs
→ registrations: 1   claps: 4   crash records: 0
```

- The free slot was **computed** and came out as 4, with 0/1/2/3/5 legitimately held by the game's
  own banks (proven a second way by the AC8 probe, where a second bank found nothing free).
- **One registration, four claps** — the bank survived four song loads and unloads. That is the
  design's central claim about leaving `file_id` at `-1`, observed rather than argued.
- `played=true` on bank **4**, which is *not* one of the two banks exempt from the game's
  sound-effect mute filter ⇒ **research risk R-2 (mute-filter veto) is closed**: the filter does not
  veto us.

All four negative paths assigned to the agent were exercised against the live game, each producing
exactly one warning and no crash: bank files renamed away; one CRC-covered byte of `tick.xsb`
flipped (`hr=0x8AC70007`); a second bank with no free slot left; and a nonexistent cue played on
every judge frame of two songs (exactly **one** warning across thousands of calls).

### Step 3 — `mods::assist_tick`, end-to-end ticking (2026-07-26)

Three tasks, all complete, gates green, **nothing committed**.

| Task | Outcome |
|---|---|
| 01 — hoist note-record helpers | Reading half of `note_types_expansion/game_note.rs` moved to **`src/types/game_note.rs`** (`GameNote` + panel/state/kind/result constants + `actor_results_range` + `for_each_result` + `GameNote::mine`); injection half stayed behind as `note_types_expansion/notes_vec.rs` (`GameNotesVec`, `NotesVecError`). Both halves proven **byte-identical** by diff; 5 importers updated; no re-export shim. Maintainer's mine-chart regression check: implicitly covered (mine machinery compiled + `NoteTypesExpansion` boots normally); explicit mine-chart listen still worth doing in Step 4's matrix |
| 02 — mod skeleton + tick list | `src/mods/assist_tick.rs` (id `assist-tick`), registered in `mods/mod.rs` + `lib.rs`. Scene wiring (arm on GAMEPLAY entry incl. quick-restart re-entry, clear on exit), judge pre @ Normal as the clock, list built once per song from the dispatched actor's Results vector: every `music_count >= 0`, sorted, exact dedup — deliberately over-permissive (predicate is Step 4's) |
| 03 — clock + playback | Rewind guard (`partition_point` re-seek), adaptive half-frame lead (`adaptive_lead()`, delta clamped 2..=34 ms, fallback 8 ms), cursor advance past all due + exactly one `play_cue(c"asti", 0.0)` per frame (FR-4), first-10-ticks measurement lines, **Step 2 `demo` scaffolding deleted**. Plus the maintainer-directed latency knob (below) |

**Observed on the live install** (song: Ace out, Challenge 10):

```
AssistTick: song build -- side=0 results=438 kept=437 first=[8888, 9110, 9333, 9777, ...]
AssistTick: clock -- observed frame delta 3 ms, computed lead 1 ms, operator offset 150 ms
tick deltas, offset 0:   [1, 4, -2, 3, 6, 0, 6, 1, -5, 1]        (fires on schedule, ±6 ms)
tick deltas, offset 150: [-141..-155], mean ≈ -148, spread ±5     (fires early by the offset)
→ registration once across songs; 2nd song rebuilt once; entry/exit lines once each; 0 crashes
```

**The latency knob (design amendment, maintainer-directed).** The first listening pass heard the
claps 100–200 ms **late** while the log showed them firing within ±6 ms of schedule — the clock
was right; the residual is the audio chain's trigger-to-audible latency (XACT's once-per-frame
submit + DirectSound mixing buffer, large under CrossOver/Wine), which the half-frame lead cannot
see. Appendix C row 1 was promoted early (config half only): new `assist_tick` config section,
`offset_ms: i32` (default 0, positive = claps fire earlier), latched once per song, third term of
the horizon. With 150 on this install the maintainer confirmed the claps on-sync. The overlay row
for live tuning stays deferred. (A first theory blamed the config's `sound_offset: 981`; wrong —
`timing-offsets` is disabled, that value never applies.)

Chart-driven-through-misses was implicitly demonstrated: the scripted play is input-less (all
misses) and the claps kept time throughout.

### Step 4 — eligibility predicate, side selection, coalescing (2026-07-26) — implemented, matrix pending

Two tasks (breakdown approved 2026-07-26), gates green, **nothing committed**.

| Task | Outcome |
|---|---|
| 01 — predicate + coalescing | `should_tick` transcribed from the research reference (kind==0 whitelist → engine's own 4-per-side shock test → live-panel guard → `mc >= 0`), `length[]` deliberately unconsulted with the `FREEZE ARROW: OFF` reasoning in the doc block; `COALESCE_MS = 4` (provisional, Step 6 re-measures on TPS-150); per-reason rejection counts on the build line |
| 02 — side selection + diagnostics | Sibling walk from `*(actor+0x08)` (vtable compare vs `gameplay_actor_vtable`, bounded, containment-validated), FR-5 choice with every side enabled (the gate is Step 5's), latch moved to the **chosen actor pointer**, degraded fallback = Step 3's behaviour + one WARN; per-song diagnostic line closes design §7.2 items 1+3 |

**Observed live** (Ace out — turns out to be a shock chart):

```
song build -- dispatched=0x9c7b6b0 siblings=1 sides=[0] styles=[0] chosen_side=0
              results=438 kept=340 rej_kind=7 rej_shock=91 rej_panel=0 rej_neg=0 coalesced=0
```

Reconciliation exact (340+7+91 = 438), and corroborated by the game itself: kept(340) +
shocks(91) = **431 = the results screen's max-combo denominator** for this chart. Kept dropped
from Step 3's over-permissive 437. Clock untouched (deltas still ≈ −148 ±6 under the 150 ms
offset).

### Post-Step-4 addition — live overlay row for the latency offset (2026-07-26/27, maintainer-directed)

Appendix C row 1's second half: the offset moved from a per-song latch to a per-frame `AtomicI32`,
seeded from `assist_tick.offset_ms` at enable and adjustable **live mid-song** from a mod-menu
overlay child row ("Tick Latency Offset", nested under the mod's master toggle, fine 1 ms /
coarse 25 ms, bounds −250..500). Changes persist back via `save_json_key` (the `timing_offsets`
precedent; the config-struct doc comment updated accordingly — the DLL now writes this section).
Live-not-latched deliberately: a cabinet-wide operator calibration knob tuned by ear, not a
per-player option, so the per-song-latch convention does not apply. Row removed in `disable()`.
Verified: renders in the overlay (maintainer), seed line `latency offset 150 ms, live via
overlay` in the boot log, gates green, installed.

### Step 5 — option row, latching, lifecycle (2026-07-27) — implemented, manual pass pending

Two tasks (lean verification per the maintainer's scope decision), gates green, **nothing
committed**.

| Task | Outcome |
|---|---|
| 01 — label asset | `("assist_tick", "ASSIST TICK")` in `gen_option_labels.py`; `seop_item_assist_tick.png` (176×16 RGBA) committed + installed. Regeneration also churned 5 unrelated PNGs (Pillow rendering drift) — reverted; noted for Step 6 |
| 02 — option row + gate | `bool_toggle("assist_tick")` default OFF, `PersistMode::Full`, Duplicate=success+reseed; on_change = atomic store; FR-8 latch at GAMEPLAY entry; FR-5 completed with enabled-filtering (0 enabled → inert with NO list build); degraded-mode refinement (re-arm rebuild when only a disabled side is visible); disable resets atomics; FR-10 honored (no score_guard) |

**One live session showed the whole gate:** song 1 (default OFF) → `no participating side has
ASSIST TICK on -- song inert` (no list built); maintainer toggled the row ON in-game; song 2 →
full `song build` + ticks with their overlay-tuned 125 ms offset (which had persisted and
reseeded — the persistence path exercised incidentally). FR-7 + FR-8 demonstrated live.

### Step 6 — diagnostic pass, docs, final gates (2026-07-27)

Run directly at the maintainer's direction (matrix re-run waived, task-file ceremony skipped;
one working record at `implementation/tasks/step6-closing-pass/progress.md`). Per-tick logging
demoted to debug (a full song now logs zero per-tick lines); NFR audit clean; §7.2 answers
recorded (solo-verified; 2P/doubles observations ride the per-song diagnostic line whenever those
sessions happen); README (Included Mods row + the full `## Assist Tick` section folding in Step
1's asset pipeline + Complete Example/row_order/available-ids), AGENTS.md (Key Entry Points row +
config bullet), and the new durable `docs/xact_audio_research.md`; final gates green; final boot
clean (default-OFF inert, JSON cache carries the id, crash log empty of crashes).

**Step 5's checkbox was ticked on the maintainer's "I've done enough in-game testing" (2026-07-27)**
— the live session had already demonstrated FR-7/FR-8 end to end (OFF inert → toggled ON in-game →
next song ticked, tuned offset persisting across relaunch).

## In flight

Nothing being edited. The tree builds clean; the installed DLL's sha256 matches the build output.
The installed `mod-config.json` carries `"assist_tick": {"offset_ms": 150}` (operator-owned).

Step 3's three and Step 4's two task files (under `.agents/tasks/20260725-assist-tick/step0{3,4}/`)
are all implemented; each task's working dir under `implementation/tasks/` closes with
`Status: Complete`. **Step 4's plan checkbox is deliberately unticked pending the maintainer's
behavior matrix.**

Four decisions settled while decomposing Step 3 (all honored by the implementation):

1. **The shared note-record helpers are hoisted to `src/types/game_note.rs`, split** — maintainer's
   call, rather than assist_tick reaching into `note_types_expansion`. The **reading** half moves
   (`GameNote`, `panel`/`state`/`kind`/`result`, `actor_results_range`, `for_each_result`, and
   `GameNote::mine`, which must travel with the struct because it writes its private `_pad` fields);
   the **injection** half (`GameNotesVec`, `NotesVecError`) stays behind, since it is bound to the
   app-heap allocator and only that mod will ever call it. `types/` over `core/` because `core/` is
   documented as *game-agnostic* and a DDR note-record layout is not.
2. **The mod is split from its clock.** Task 02 builds the timestamp list and logs it but plays
   nothing; task 03 adds the cursor, the adaptive lead and the playback. Same reasoning the plan
   applies to Step 3 as a whole: a wrong note-record read and a wrong clock produce the same
   symptom, so they are verified separately — task 02 purely from `log.txt`, task 03 by ear.
3. **Step 2's scaffolding survives until task 03 removes it**, so no intermediate state has the audio
   path deleted before its replacement exists.
4. **FR-4 (one tick per frame) lands in task 03**, not Step 4, because the cursor-advance loop is
   where it naturally lives. Noted so Step 4's decomposition does not expect it as outstanding.

Uncommitted in this repo:

```
 M .agents/planning/20260725-assist-tick/design/detailed-design.md   (dated amendments §4.1/§4.2.3/§5.3/§6/App C)
 M .agents/planning/20260725-assist-tick/implementation/plan.md      (Steps 1–3 ticked)
 M README.md                                                        (## Assist Tick Sound, Step 1)
 M src/core/signatures.rs                                           (+3 patterns, +derivations)
 M src/lib.rs                                                       (game_audio::init at 6b1; AssistTickMod in the mod list)
 M src/services/mod.rs                                              (pub mod game_audio + doc)
 M src/mods/mod.rs                                                  (pub mod assist_tick + doc bullet)
 M src/mods/config.rs                                               (AssistTickConfig + ConfigFile field + fallbacks)
 M src/services/game_audio.rs → still ?? (new file; Step 3 removed its demo block)
 D src/mods/note_types_expansion/game_note.rs                       (split by Step 3 task 01)
 M src/mods/note_types_expansion/{mod,hooks,mine_render,mines,note_type,registry}.rs   (import updates)
?? src/types/game_note.rs                                           (hoisted reading half)
?? src/mods/note_types_expansion/notes_vec.rs                       (injection half)
?? src/mods/assist_tick.rs                                          (the mod)
?? data_mods/assist_tick/  ?? scripts/build_assist_tick_bank.sh
?? .agents/planning/20260725-assist-tick/{progress.md,implementation/tasks/}
?? .agents/tasks/20260725-assist-tick/{step02,step03}/
```

Also uncommitted in the sibling `ddr-chart-tools` (Step 1, plus two pre-existing repairs — see item D
below).

**Steps 1–3 are ticked in `implementation/plan.md`** — Step 3's tick followed the maintainer's
on-sync confirmation of 2026-07-26.

## Deploy & test log

| Date | What | Result |
|---|---|---|
| 2026-07-26 | Step 1 — nothing deployed (no DLL code changed) | — |
| 2026-07-26 | `data_mods/assist_tick/` copied into `$DDR_WORLD_INSTALL/data_mods/` (one-time), sha256-verified | ok |
| 2026-07-26 | Boot 1 — task 01's signatures | all 3 unique, all 4 derivations correct; **and** exposed the XACT-module-check defect below |
| 2026-07-26 | Boot 2 — happy path after the fix | slot 4 computed, both HRESULTs `0x0`, clap `played=true`, 2nd song clapped with no 2nd registration |
| 2026-07-26 | Boot 3 — banks renamed away | one WARN naming the expected path; no clap; otherwise identical boot |
| 2026-07-26 | Boot 4 — one byte of `tick.xsb` flipped | `CreateSoundBank hr=0x8AC70007`, one WARN with the HRESULT, nothing written; latch held over a 2nd song |
| 2026-07-26 | Boot 5 — throwaway probe (AC8 + AC9) | `no free sound-bank slot` ×1; nonexistent cue on every frame → `failure sentinel` WARN ×1. Probe reverted, source verified byte-identical |
| 2026-07-26 | Boot 6 — final build, installed | 1 registration, 4 claps over 4 attract songs, 0 crash records |
| 2026-07-26 | **Maintainer listening pass** | **Clap heard** — a single clap at the start of the next attract-demo song. Step 2's demo satisfied; R-2 (mute-filter veto) and the mix-bus question both closed by ear |
| 2026-07-26 | Step 3 boot 1 — tasks 01–03, offset 0 | Mod registered/enabled; note-types boots normally post-hoist; song build 437/438 kept, strictly increasing; tick deltas ±6 ms; 2nd song rebuilt once, no 2nd registration. NOTE: attract demo runs under ATTRACT_DEMO (16), not GAMEPLAY (28) — a real card-in song is needed, via `scripts/game_nav/` |
| 2026-07-26 | **Maintainer listening pass 1** | Claps **100–200 ms late** by ear despite on-schedule firing → diagnosed as output-chain trigger-to-audible latency → latency knob |
| 2026-07-26 | Step 3 boot 2 — `offset_ms: 150` | Clock line shows the offset; tick deltas mean ≈ −148, spread ±5; 0 crash records |
| 2026-07-26 | **Maintainer listening pass 2** | **On-sync confirmed** ("more or less on-sync … the right fix"). Step 3's demo satisfied |
| 2026-07-26 | Step 4 task 01 — predicate + coalescing | kept 437→340; 340+7+91=438 exact; kept+shocks = 431 = the chart's max-combo denominator |
| 2026-07-26 | Step 4 task 02 — side selection | `siblings=1 sides=[0] styles=[0] chosen_side=0`, no DEGRADED marker, kept unchanged, 0 WARNs, 0 crashes |
| 2026-07-26/27 | Overlay latency row (fine step 1 after maintainer feedback) | renders in overlay (maintainer-verified); boot seeds `latency offset 150 ms, live via overlay` |
| 2026-07-27 | **Maintainer behavior matrix** | **Everything looks good** — Step 4 confirmed and ticked |
| 2026-07-27 | Step 5 — label asset + option row installed | row registers; default-OFF song inert (no list); maintainer toggled ON in-game → next song built + ticked; offset 125 ms persisted/reseeded |
| 2026-07-27 | **Maintainer**: enough in-game testing — Steps 5+6 proceed without the full matrix | Step 5 ticked |
| 2026-07-27 | Step 6 final build + boot | zero per-tick log lines; default-OFF inert; `custom_options.p1.assist_tick` in the JSON cache; crash log clean. **Feature complete** |

## Deviations & open questions

### Settled during Step 3

1. **Per-tick measurement lines ship at info, not debug** (task 03 said debug): the logger boots at
   `Info` and nothing ever calls `set_log_level`, so debug lines cannot reach `log.txt` — and those
   numbers are the step's verification. Bounded at 10 per song; Step 6 demotes or deletes them.
2. **Operator latency offset promoted early** (maintainer-directed): `assist_tick.offset_ms`
   config section, applied as a third horizon term. Design amended in place (§4.2.3, §5.3,
   Appendix C row 1 — config half only; the overlay row for live tuning stays deferred). Rationale
   and measurements in the Step 3 section above and in
   `implementation/tasks/tick-clock-and-playback/progress.md`.

### Settled during Step 2

1. **The XACT engine-module check cannot live in `game_audio::init`** — design §4.1 and §6 said it
   did. The boot log proves the engine is COM-instantiated inside `Application::onBoot`, which
   completes *after* the DLL's init thread finishes, so the guard always failed and the service was
   permanently disabled. Escalated; **maintainer chose to move it to the first `register_bank`**
   (immediately before the vtable dispatch it protects — strictly tighter). The design carries dated
   in-place amendments in §4.1 and §6. **Consequence for Step 3:** `is_available()` now means
   "addresses resolved" and no longer pre-empts the wrong-engine case, which instead surfaces as one
   declined registration.
2. **`play_cue` logs the resolved cue index at info, not warn** (requirement 12), so that a genuinely
   missing cue still yields exactly one *warning* for the session — the play sentinel's.
3. **All permanent registration failures latch**, so a caller that retries every song still produces
   one warning. The null-manager case deliberately does not latch (it is the one transient failure).
4. **The Step-2 scaffolding claps once per song**, not once per session — a once-per-session latch
   would have made the "survives song loads" claim unobservable.

### Settled during Step 1

1. **The `wave_index 0` amendment is safe** — confirmed by stock precedent (`se_system.xsb`'s sound 9
   points at wave index 0). The documented two-entry fallback is not needed. *(Now doubly confirmed:
   the live engine resolved `cue 'asti' -> index 0`.)*
2. **Input is Ogg Vorbis only and the pipeline never re-encodes** — that is what makes the committed
   bytes reproducible on any machine. Maintainer decision: the transcode branch was dropped from the
   build script entirely; a non-conforming source is rejected with the conversion command printed.
3. **Bank/cue name `asti` ≠ file names `tick.*`, deliberately** — two independent namespaces, exactly
   as the game itself does (`se_system.xwb` carries the internal name `SE_SYSTEM`).

### Carried into later steps

| # | Item | Affects |
|---|---|---|
| A | ~~Delete `game_audio.rs`'s `mod demo` block + the `demo::install();` call~~ **done** (Step 3 task 03) | done |
| B | ~~Load the banks via `mod_paths::find_first_modfile`~~ **done** (assist_tick::init) | done |
| C | ~~`play_cue` passes `asti`, centre-panned `0.0`~~ **done** (Step 3 task 03) | done |
| D | **Two pre-existing issues repaired** in the sibling repo to unblock its gates: two `clippy::manual_checked_ops` errors (new lint in Rust 1.96) in `src/job/mod.rs`, and `cargo run` ambiguity from the second binary (fixed with `default-run`). Reported, not part of this feature | reported |
| E | **ADPCM encode quality:** the clap measures 17.4 dB SNR. The encoder is not broken (disabling its predictor search costs 0.8 dB) but its quantizer truncates where it should round — worth ~5 dB. Deliberately **not** changed: `adpcm::encode` is shared with the song-conversion path. Maintainer's call; if taken, the committed bank must be regenerated | optional |
| F | The README section from Step 1 covers the **asset pipeline**. Step 6's README work should fold it in with the mod's *Included Mods* entry rather than adding an overlapping section | Step 6 |
| G | `shellcheck` is not installed here, so `build_assist_tick_bank.sh` has had `bash -n` plus ten behavioural checks only | optional |
| H | `src/bin/` is not listed in the sibling repo's `.spec/steering/structure.md` layout | optional doc touch-up |
| I | Handle-table pressure (research R-3) is not yet stressed — Step 2 plays one clap per song. Step 4's one-tick-per-frame is what bounds it | Step 4 |
| J | `se_play_inner` is resolved and unused — the one-line mitigation in design §6. **Probably never needed now** that R-2 is closed, but keep it | Step 4+ |

## Key facts for a cold resume

- **What this feature is:** a clap on every arrow's chart timestamp, as a timing reference. One new
  mod (`assist-tick`) plus one new service (`game_audio`) in a Rust hook DLL injected into DDR World.
- **Audio goes through the game's own XACT 2 engine**, never a second audio path — that is what makes
  the clap share the music's clock and latency. A self-hosted XAudio2 path was investigated and
  rejected.
- **Ticks are chart-time driven, not judgment driven.** `judge_hook` is used only as a per-frame
  clock; its callback is `fn(actor: *mut u8, music_count: i32)` and `music_count` is
  **milliseconds** (it starts negative — the lead-in — as the logs show at `-87`).
- **The load-bearing runtime trick, now proven live:** register our sound bank into the free slot on
  the game's audio manager by writing *only* the slot's bank pointer, leaving its `file_id` at `-1`.
  The only code that destroys a slot is a linear "find the slot whose `file_id` matches" search,
  which matches nothing — so the bank survived four song loads in one session.
- **A malformed sound bank is rejected SILENTLY by the game's own loader** (it ignores the HRESULT);
  `game_audio` logs it. That is why the banks are generated and validated offline.
- **The `se_play` ABI trap:** the third argument is a float travelling in **XMM2**. Declared
  `extern "system" fn(i32, *const c_char, f32) -> u32`.
- **Never call game functions from the DLL init thread.** Bank creation happens on the game thread at
  the first judge dispatch. Related: the XACT engine module is not even loaded during our init.
- **Eligibility predicate (FR-2), for Step 4:** `kind == 0`, not a shock (all four panels of a side
  set to 1), at least one non-zero panel state, non-negative timestamp. **Do NOT consult `length[]`**
  — that breaks under the `FREEZE ARROW: OFF` modifier.
- **Verification split:** the maintainer runs all gameplay/listening verification; the agent's share
  is offline validation, the build gates, and reading `log.txt` out of the local install.
- **Nothing is committed by the agent** in either repository.
- Two repos: this one, and the sibling `ddr-chart-tools` (which does have a real test harness —
  `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt`; 337 tests green).
