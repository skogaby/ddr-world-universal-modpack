# Multiplayer Bot — Implementation Plan

Status: Approved 2026-09-13 (Step 2 revised 2026-09-13: `tools/bot_sim` offline simulator + gauge RE replaces the synthetic `--report` harness — approved in conversation)

Decomposition of `design/detailed-design.md` (Approved 2026-09-13). Section references
(`§4.1`, `§7.1`, `A.4` …) point into that document; this plan does not restate what it
specifies. Every step leaves the DLL building (`cargo check --target
x86_64-pc-windows-msvc` → `cargo fmt` → `./build.sh`) and ends with a cabinet-observable
demo. Cabinet runs are the maintainer's (`./scripts/deploy.sh`); each step names the log
lines / on-screen behaviour that constitute its pass.

- [x] Step 1: Extract the foot-panel swap into `services/foot_panel_swap` (autoplay becomes a client)
- [x] Step 2: Pure cores — `eligibility`, `skill`, `planner` + `tools/bot_sim` offline simulator / report (`scripts/validate_multiplayer_bot.sh`, `scripts/bot_sim.sh`)
- [x] Step 3: Bot controller — cloned-vtable `BotFootPanel`, `filler`, mod skeleton, dev self-test on the human's side
- [x] Step 4: Impersonation flip/restore, option rows, textures, menu placement — full bot session
- [x] Step 5: Extra-stage guard — `extra_stage_grant` signature + detour, signature sweep green
- [x] Step 6: Interaction pass, optional SE pan, RE note, AGENTS.md row

## Sequencing rationale

Steps 3 and 4 each retire one of the two load-bearing RE facts (§1) on the cabinet: Step 3
proves the judge grades a DLL-owned `IFootPanel` exactly where `getPressAge` says (without
touching player entry at all — the bot drives the HUMAN's own actor under a dev flag), Step
4 proves the two-value flip builds a genuine 2P session. They are independent of one
another; Step 3 goes first only because it reuses Step 1's service and Step 2's cores
directly and its blast radius is one actor's judge. If the maintainer prefers to retire the
flip risk first, Step 4's `impersonation.rs` can be pulled ahead of Step 3 with a
"bot that never presses" demo — nothing else in the order changes.

Step 1 goes first because it is a pure refactor with a bit-for-bit regression oracle
(today's autoplay) and every later step builds on its arbitration API; landing it alone
keeps the extraction reviewable as a diff without new behaviour.

---

## Step 1: Extract the foot-panel swap into `services/foot_panel_swap` (autoplay becomes a client)

**Objective.** Move the `judgeNotes` pre/post swap, the stock `AutoFootPanel` object and the
three signatures out of `src/mods/autoplay.rs` into a shared service with the per-side
`Controller { Off, Perfect, Bot }` arbitration (§4.1, §4.2) — `Bot` is declared and arbitrated
now, wired in Step 3. Autoplay's observable behaviour is unchanged.

**Guidance.**
- New `src/services/foot_panel_swap/` (declared in `src/services/mod.rs`):
  - `layout.rs` — PURE (no `crate::` imports): `Controller`, `effective_controller(bot_armed,
    perfect) -> Controller`, the object size constant `PANEL_OBJECT_SIZE = 0x58` and the
    `GamePlayActor` field offsets the swap reads (`ACTOR_SIDE = 0x84`, `ACTOR_RESULTS_BEGIN =
    0xB0`, `ACTOR_CUR_BEAT = 0x168` — autoplay.rs today misnames `0x168` `NOTE_COUNT`; the RE
    (`update(this, &results, cur_beat, mc)`) says it is the current beat position, rename it).
  - `mod.rs` — `init(&SignatureStore) -> bool` (resolves `judge_notes`, `auto_foot_panel_vtable`,
    `auto_foot_panel_update`, `judge_hook::foot_panel_offset()`; allocates the stock-shaped object
    at 0x58 via `memory::alloc_zeroed`; registers `register_pre(Priority::Late, swap_in)` +
    `register_post(Priority::Early, swap_out)` ONCE), `is_available()`, `set_perfect(side, on)`,
    `arm_bot`/`disarm_bot` (store the fill fn; the `Bot` branch of `swap_in` is a TODO that falls
    through to `Off` until Step 3), `controller(side)`.
  - `swap_in`/`swap_out` are the moved autoplay callbacks with the per-side stash
    (`ORIGINAL_FOOT_PANEL[2]`) and the `side = (*(actor+0x84) == 1)` read. Both are invoked
    inside the dispatcher's `catch_unwind` but must stay panic-free (no indexing on `side`).
- `src/lib.rs`: call `foot_panel_swap::init(&signatures)` immediately after step 6b
  (`judge_hook::init`) — the service is a judge_hook subscriber and mods `init` at step 7.
- `src/mods/autoplay.rs`: delete `AUTO_PANEL`/`AUTO_UPDATE`/`FOOT_PANEL_OFFSET`/
  `ORIGINAL_FOOT_PANEL`/both callbacks/both judge registrations; `required_signatures() ->
  &[]`; `init` returns `foot_panel_swap::is_available()`; `autoplay_on_change` calls
  `foot_panel_swap::set_perfect(side, v != 0)` + the existing `score_guard::set_autoplay_taint`;
  keep the fail-closed `score_guard::is_available()` gate, the option row and the watermark;
  watermark predicate becomes `foot_panel_swap::controller(side) == Controller::Perfect &&
  stage_records::side_entered(side).unwrap_or(true)` (§4.2 — the service, not autoplay's own
  flag, so Step 3's `Bot` precedence hides the mark on the bot side for free). `disable` clears
  both sides via `set_perfect(side, false)`.
- The judge_hook priorities are load-bearing (pre `Late` / post `Early`; `per_song_judgement_offsets`
  at `Early`, PUS at `Normal` keep their order) — do not change them.

**Tests.**
- Host: `layout.rs` `#[cfg(test)]` — `effective_controller` truth table (Bot > Perfect > Off);
  `PANEL_OBJECT_SIZE >= 0x40` and `== 0x58`. Mounted by the harness Step 2 creates; for this
  step run it through a one-off temp-crate `#[path]` mount (the `scripts/validate_two_player_bpl.sh`
  shape) or defer the harness run to Step 2 — the assertions are `const`-checkable either way.
- Signature sweep is unaffected (no signature added or renamed; `report.py` will show the three
  autoplay signatures moving from `required_signatures` to `get_address` consumers in the
  service — that is expected).
- Cabinet (maintainer, §7.3 item 1): autoplay ON for the human — every note Marvelous, watermark
  bouncing, `Autoplay: side=N ON` + a new `foot_panel_swap: …` init INFO in the log; autoplay OFF
  — plain play, no swap INFO per frame (the callbacks must stay silent).

**Integration.** First step; autoplay is the only client. `lib.rs` gains one service init line.

**Demo.** Autoplay behaves exactly as before the change, from the options menu, on the cabinet;
the boot log shows `foot_panel_swap` initialised before the mods register.

---

## Step 2: Pure cores — `eligibility`, `skill`, `planner` + `tools/bot_sim` offline simulator

**Objective.** Implement and host-test the three dependency-free modules of §4.4, §4.6, §4.7
before any engine wiring exists, and stand up the offline simulator + HTML report (§7.1) that
every later step re-runs — a real-corpus tuning oracle for the skill curves and a permanent
planner-vs-judge consistency check.

**Guidance (as built).**
- `src/mods/multiplayer_bot/{eligibility.rs, skill.rs, planner.rs}` — no `crate::` imports;
  `planner.rs` reaches `skill` via `use super::skill`; declared from a stub
  `src/mods/multiplayer_bot/mod.rs` (no `Mod` impl yet). `eligibility::Inputs` carries
  `Option<T>` + `Refusal::Unavailable` (the BPL `GateInputs` shape). `skill.rs`: **World has
  no Boo** — `GOOD_WINDOW_MS = 124` is the outermost graded window, `decide` treats |d| > 124
  as Miss, `grade_for_offset` ∈ {0,1,2,3,5}. `planner.rs`: `NoteView.kind` (tails never
  decided/pressed), floored `E > +124 ⇒ Miss`, one live event per panel per frame with panel
  reservation by blocked notes, plans resolved once in note order.
- RE for the simulator: `docs/gauge_and_judge_scoring_research.md` (judge acceptance, freeze
  judge, `judge_submit` order/formulas, NORMAL gauge exact integers) — 20260825 addresses.
- `tools/bot_sim/` (native crate, no deps): `chart.rs`, `judge_model.rs`, `gauge.rs`,
  `scoring.rs`, `report.rs`, `main.rs` (CLI, threads, `~`-collapsed paths in the report);
  `#[path]` mounts live in `core/ssq/mod.rs` and `bot/mod.rs` (real directories so the `..`
  chains resolve). `src/core/ssq/timing.rs` gained a read-only `entries()` accessor for the
  inverse tempo map.
- `scripts/validate_multiplayer_bot.sh` = `cargo test` in the tool (+ `--report <ssq-dir>`);
  `scripts/bot_sim.sh` = the simulator wrapper. `.gitignore`: the tool's `target/` and the
  default report outputs.

**Tests.** 63 host tests (34 mounted DLL-module tests + 29 tool tests) — see §7.1 for the
pinned list. Corpus run (1,586 files, 6,621 single charts, 66,210 songs, 6.6 s): 0 parse
errors, planner/judge mismatches 66 of ~2.4 M judgements (0.003 % — dense streams where a
late-planned Good loses the one-judgement-per-frame race until +160; a game behaviour, not a
planner bug).

**Integration.** `layout.rs` from Step 1 joins the tool's mounts. Nothing engine-facing
changes.

**Demo.** `./scripts/bot_sim.sh "$DDR_WORLD_INSTALL/data/mdb_apx/ssq"` produces the HTML
report: level table, heatmaps, filterable scorecards. Tuned constants (maintainer-approved
2026-09-13 after viewing the report): `σ₁ 60 / σ₁₀ 5.4 / p₁ 0.13 / exp 1.4` ⇒ L10 71 % MFC+
(4 % S-MFC), fail L1 60 % (b 7 … E 95), L2 27 %, L3 5 %, L4+ ≈ 0 — the stock `75/5/0.05/1.5`
gave L1 55 % → L2 0 % (a cliff) and L10 86 % MFC+.

## Step 3: Bot controller — cloned-vtable `BotFootPanel`, `filler`, mod skeleton, dev self-test on the human's side

**Objective.** Make the `Bot` controller real (§4.1 internals, §5.2) and prove on the cabinet that
the judge grades a DLL-owned panel exactly at the planned event times on the current build —
WITHOUT touching player entry: a dev-mode flag arms the bot on the HUMAN's entered side, so the
human's own actor is "played" by the bot at level N.

**Guidance.**
- `services/foot_panel_swap/layout.rs`: `#[repr(C)] BotFootPanel` (§5.2 — `vtable +0x00`,
  `is_held[8] +0x08`, `was_just_pressed[8] +0x10`, `event_mc[8] +0x18`, padded to 0x58) with
  `const` offset assertions (`offset_of!`), `BotPanelFlags`, and the pure
  `bot_vtable_image(donor: &[usize; 7], col, get_press_age, consume_press) -> [usize; 8]`
  (the `two_player_bpl_mode::logic::clone_vtable_image` shape: COL at `[0]`, slots 0–4 verbatim,
  5/6 replaced).
- `services/foot_panel_swap/mod.rs`: at `init`, read the 7 stock slots + COL from the RTTI
  `auto_foot_panel_vtable`, build the image into a `memory::alloc_zeroed` region (RWX), install
  it into two `BotFootPanel` objects (one per side); the two `unsafe extern "C"` slots of §4.1
  (`bot_get_press_age` = `CURRENT_MC[side_of(this)] − event_mc[panel & 7]`, `bot_consume_press`
  zeroes `event_mc[panel & 7]`; `side_of` compares `this` against the two static objects, unknown
  ⇒ side 0). Implement the `Bot` branch of `swap_in`: stash, `CURRENT_MC[side] = mc`, call the
  armed `BotFillFn`, copy the flags into the side's object, write the object into the slot.
  `arm_bot` while `PERFECT[side]` ⇒ one INFO ("bot controller takes precedence over autoplay on
  side N").
- `src/mods/multiplayer_bot/filler.rs` (§4.8): `start_song(side, level, seed)` / `reset(side)` /
  `tally(side)` / `fill`. Read `cur_beat = *(actor + ACTOR_CUR_BEAT)`, the result range via
  `types::game_note::actor_results_range` + `for_each_result` into a retained `Vec<NoteView>`
  (`music_count`/`beat_count`/`state`/`length` from `GameNote`, `unjudged = ts < 0 && grade ==
  0xFF`), then `planner::plan_frame`, then map `PanelFlags → BotPanelFlags`. Per-side
  `Mutex<SongState>` via `try_lock` (contention ⇒ empty flags + rate-limited WARN);
  `memory::is_readable` on `actor+0x84..+0x170` once per song; result count ≤ 8192;
  `catch_unwind` around the body. **Self-check (kept permanently):** on the first frame a note
  is judged, compare the game's grade (`result+0x0C`) with the planner's expected
  `grade_for_offset(E − note.mc)`; count mismatches in the tally (a non-zero count means the
  controller assumption broke on this build — one WARN per song).
- `src/mods/multiplayer_bot/mod.rs`: `MultiplayerBotMod` skeleton (id/name/description per §4.3,
  `required_signatures() -> &[]`, `init` requires `foot_panel_swap::is_available()` +
  `stage_records::is_available()` + `scene_manager::is_available()` + `score_guard::is_available()`,
  `is_active` = init succeeded). No option rows yet. **Dev self-test**: when
  `config.layeredfs.developer_mode` AND `DDR_BOT_SELF_TEST=<level>` are set (the `DDR_BPL_DRY_RUN`
  gating shape), a scene callback arms the bot on every ENTERED side at GAMEPLAY entry
  (`arm_bot(side, filler::fill)` + `start_song` + `score_guard::set_autoplay_taint(side, true)`),
  disarms + logs the tally on leaving {28, 29, 30}. This path is dev-only and stays (it is the
  build-portability probe for §7.3 item 10).
- Register `MultiplayerBotMod` in `src/lib.rs` (`mods_to_register`, after `autoplay`) and add
  `"multiplayer-bot": true` to `mod-config.json` `mods`; declare `pub mod multiplayer_bot;` in
  `src/mods/mod.rs`.

**Tests.**
- Host (harness from Step 2): `layout.rs` — `BotFootPanel` offsets/size; `bot_vtable_image`
  layout (COL, verbatim 0–4, replaced 5/6); `BotPanelFlags` ⇄ planner `PanelFlags` field parity
  (a copy round-trip). `filler.rs` is engine-facing — cabinet-validated.
- Cabinet (maintainer): with `DDR_BOT_SELF_TEST=10`, play a song hands-off — the game grades
  ~99.9 % Marvelous, the arm INFO names σ/p/seed, the song-end tally INFO shows `mismatch=0`, and
  PUS's ms-error readout (if enabled) tracks tiny ± values. With `DDR_BOT_SELF_TEST=1`: visible
  Greats/Goods/Boos/Misses, gauge drains, a fail is possible, `mismatch=0` still. With
  `DDR_BOT_SELF_TEST` unset: nothing armed, no per-frame logging. Autoplay ON + self-test ⇒ the
  precedence INFO once and the bot (not Perfect) drives.

**Integration.** Fills the `Bot` branch Step 1 left open; consumes Step 2's cores. The mod
exists in the registry (shows in the Mods tab) but does nothing without the dev flag.

**Demo.** A level-N bot plays the human's chart on the human's own lane; the log's
`mismatch=0` line is the proof that `event = mc − getPressAge` lands on the planned `note.mc +
d` on this build.

---

## Step 4: Impersonation flip/restore, option rows, textures, menu placement — full bot session

**Objective.** The user-facing feature end to end: BOT OPPONENT (1P ONLY) + BOT LEVEL rows drive
a genuine 2P versus session against the bot from the next song's start through its stage
results (§4.3, §4.5, §4.11, §5.1, §5.3).

**Guidance.**
- `src/mods/multiplayer_bot/impersonation.rs`: the §4.5 state machine, driven by ONE
  `scene_manager::on_scene_change` callback registered at `enable` (the same callback hosts
  Step 3's dev self-test, gated so the two never both arm a side). `apply` follows §4.5 steps
  1–8 exactly — every pointer probed with `memory::is_readable` before the read/write, the
  `rec_b+0x00 == rec_h+0x00` refusal, snapshot-then-write, undo-on-failure. Read the gate inputs
  from `stage_records::{side_entered, game_work, course_field_offset, event_mode, stage_counter,
  stage_record, player_work, player_option_offset}` — `GameWork+0x0` / `+0x4` are fixed header
  words (§2.3; `two_player_bpl_mode/mod.rs` already reads `+0x0` this way); the course word is at
  `game_work + course_field_offset()`, never a literal `0x70`. `restore()` is idempotent and
  also called from `disable`. `active_bot_side() -> Option<usize>` for Step 5. The 20 s
  in-window watchdog of §6 (diagnostic WARN only) — reuse the quick_restart watchdog shape.
  Re-seed + `filler::reset` on `next == GAMEPLAY` while Active and on `song_reset::on_song_reset`.
- `mod.rs`: `OPTION_ON: [AtomicBool; 2]`, `LEVEL: [AtomicI32; 2]`; rows per §4.3/R1 —
  `RegisterSpec::bool_toggle("bot_opponent").display_name("Bot Opponent (1P Only)")…` then
  `RegisterSpec::scalar("bot_opponent_level", 1, 10, 1, ScalarFormat::Integer).default_value(5)
  .show_when(ShowWhen::Equals{..}).persist_transform(save_identity, load_clamp)` (parent first;
  parent on `custom_options::is_available()`, child on `row_injection_available()`;
  `Err(Duplicate)` ⇒ reseed from `get_value(side, id)` — note the argument order differs from
  `set_value(id, side, v)`); `on_change` callbacks are plain `fn`, panic-free, fire for both sides
  at registration. `disable`: `set_option_available` both rows false, `restore()`, remove the
  scene callback + song_reset subscription. Log the §4.5 INFO lines (apply, refusal reason when the
  entered side's option is ON, restore + tally).
- Textures: `scripts/option_strings.py` `LABELS["bot_opponent"]` = en `BOT OPPONENT (1P ONLY)` /
  ja / ko, `LABELS["bot_opponent_level"]` = en `BOT LEVEL` / ja / ko (all three languages are
  mandatory); optional `PREVIEWS` (bool off/on SPLIT panel, level single WIDE panel — follow the
  `center_arrows_1p` / `assist_tick_volume` entries). Regenerate with
  `python3 scripts/gen_option_labels.py`; commit only the generated PNGs under
  `data_mods/custom_options/select_music_option_lang_{eng,jpn,kor}_v3_ifs/tex/` (never hand-edit).
- `mod-config.json` `option_menu_settings`: insert `{"id":"bot_opponent"}` and
  `{"id":"bot_opponent_level"}` (both menus) right after the `autoplay` entry.
- Deploy note for the maintainer: the DLL AND the texture PNGs must ship together (a DLL-only
  deploy shows blank in-game labels); the overlay PLAYER SETTINGS tab needs no textures, so the
  first cabinet test can proceed from the 0-0-0 menu even if the atlas lags.

**Tests.**
- Host: `eligibility` is already covered (Step 2); add a pure `impersonation::snapshot_roundtrip`
  test if the snapshot/name formatting is factored into a dependency-free helper (`format_bot_name(level)
  -> [u8; 9]` — `BOT LV10\0` fits; assert every level 1..=10 is ≤ 8 chars + NUL).
- Cabinet (maintainer, §7.3 items 2–5, 7, 8): L10 on P1 — two READY panels, P2 lane at P1's
  speed/skin, name plate `BOT LV10`, mostly Marvelous, two results panes, TOTAL RESULTS 1P only,
  no `save_sender` for side 1 (or a suppressed one), P1's per-stage save proceeds; L1 — bot fails
  visibly; human on P2 pad — bot takes P1; quick restart re-rolls (different pattern, new seed in
  the INFO), quick fail returns to a 1P song select, option OFF ⇒ next song 1P; real 2P / doubles
  / course ⇒ no flip, one INFO naming the refusal. Log grep: `multiplayer-bot: side N impersonated
  …`, the restore INFO with the tally, no WARN.

**Integration.** Arms Step 3's controller through `foot_panel_swap::arm_bot`; the dev self-test
remains available but is mutually exclusive with an active impersonation per side. Autoplay
(Step 1 client) keeps working for the human; a cached `autoplay = ON` on the bot side is
out-ranked (Step 1's arbitration, Step 3's INFO).

**Demo.** Turn BOT OPPONENT on and pick a level in either menu; the next song is a full 2P
versus session against `BOT LV<n>`, ending on a two-pane stage results screen, and the session
continues as stock 1P afterwards.

---

## Step 5: Extra-stage guard — `extra_stage_grant` signature + detour, signature sweep green

**Objective.** R10 / D21: the game's extra-stage grant must not consider the bot (§4.9, §4.10,
A.4). Fail-open.

**Guidance.**
- `src/core/signatures.rs`: add `SignatureDefinition { name: "extra_stage_grant", pattern:
  <A.4 AOB>, description: … }`. It is a soft consumer (`get_address` at mod `init`, NOT in
  `required_signatures`) so `report.py` classifies a miss as soft.
- `src/mods/multiplayer_bot/extra_stage_guard.rs`: `GenericDetour<unsafe extern "C" fn(i32)>`
  installed once via `hooks::install_enabled` at the first `enable` (the `announcer_mute` shape:
  `static mut` detour slot, `addr_of!` read in the callback, passthrough flag on `disable`).
  Callback per §4.9: when `impersonation::active_bot_side()` is `Some(bot)` and the guard is
  enabled, read `stage_records::player_work(bot)`, probe, clear `PW+0x4` around `hook.call(arg)`,
  restore (scope-guard shape so an unwinding original cannot leave the byte cleared — the
  callback body itself must be panic-free). One INFO per guarded call (it fires once per song at
  most). Missing AOB ⇒ one WARN at `init` ("extra-stage grant will consider the bot") and the mod
  still enables.
- Do NOT read anything at `match+N`; the detour target is the match address itself.

**Tests.**
- `./scripts/validate_signatures.sh ~/Desktop/ddr_modules` ALL GREEN — `extra_stage_grant`
  must hit exactly once on 20250805 / 20260224 / 20260721 / 20260825 (Ghidra already attests
  three; the sweep attests 20260721). `shape_diff.py` not required (nothing read at `match+N`).
- Cabinet (maintainer, §7.3 item 6): on a 3-stage setting with a low-level bot that does not AAA,
  the human who AAAs still gets EXTRA STAGE; the log shows the guard INFO on the results
  window-out. Without the bot, extra stage behaves as stock.

**Integration.** Consumes Step 4's `active_bot_side()`; adds the feature's only new signature
and only new detour.

**Demo.** A level-1 bot's failure no longer blocks the human's extra stage; the signature sweep
report shows `extra_stage_grant` resolving on all four builds.

---

## Step 6: Interaction pass, optional SE pan, RE note, AGENTS.md row

**Objective.** Confirm the cross-mod interactions the design accepts as-is (R11, N6, D14, D15,
D23), take the optional cosmetic D22 if trivial, and leave durable documentation.

**Guidance.**
- Interaction checklist (code review + cabinet): autoplay ON on the bot side (cached value) ⇒ the
  precedence INFO, bot drives, watermark hidden; `two_player_bpl_mode` frame engages against the
  bot with the bot's name (§7.3 item 9); quick restart / quick fail / premium free
  (`stage_counter` frozen ⇒ the mirror uses the frozen index — fine) / training mode (D14: verify
  the bot just plays the looped section; refuse via eligibility if it misbehaves) / PUS (may show a
  widget for the bot — acceptable) / S-Marvelous (paints the bot's pane — acceptable). Fix
  anything that breaks the HUMAN's play; document anything merely cosmetic.
- D22 (optional): only if `game_audio` or `signatures` already exposes the resolved
  `audio_manager_global` pointer — write the versus-pan byte `+0x20C4` at apply, restore at
  restore, probed. Otherwise skip and record why in the RE note. Never a new signature.
- `docs/multiplayer_bot_research.md`: consolidate the durable RE facts from the design's Appendix A
  and the two research notes (actor-count seam, versus word readers, commit/record mirroring,
  extra-stage grant, AutoFootPanel vtable + judge algebra, Option layout) — addresses file-relative
  to `0x180000000`, no local paths.
- `AGENTS.md` Key Entry Points: one row for "Multiplayer Bot" in the established style (mod path,
  ids, service, gates, the two RE facts, the planner's two non-negotiable rules, fail-open
  behaviour, host script, RE doc). Also mention the service move in the Autoplay-adjacent rows if
  any reference `autoplay.rs`'s swap directly.
- Final readiness gates: `cargo check` → `cargo fmt` (whole crate) → `./build.sh` →
  `./scripts/validate_multiplayer_bot.sh` → `./scripts/validate_signatures.sh ~/Desktop/ddr_modules`
  → `git grep -nE "/(Users|home)/[^/ ]+/" -- . ':!target'` adds no new hits.

**Tests.** Host harness + signature sweep re-run; cabinet §7.3 items 9–10 (BPL frame; old build
20250805 or 20260224 repeating items 2–3 — the controller's build-independence claim).

**Integration.** No new mechanisms; closes the loop on every accepted interaction decision.

**Demo.** With `two-player-bpl-mode` ON, a bot session shows the BPL score boards / rank badges /
margin with `BOT LV<n>` on the bot's board; the same DLL on an old build produces the same
`mismatch=0` tally.

---

## Out of scope (tracked for `summary.md`)

- bemani-buddy migration for `opt_mod_bot_opponent` / `opt_mod_bot_opponent_level` (other
  repo); the JSON cache carries the values until then.
- Skill-curve tuning beyond D6's anchors — constants live in `skill.rs`; the `--report` table is
  the tuning aid.
- Phase-2 ideas the register did not accept: streak/fatigue modelling, bot presence on TOTAL
  RESULTS, independent bot chart.
