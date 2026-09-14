# Multiplayer Bot — Progress

Updated: 2026-09-14
Status: FEATURE COMPLETE — all 6 plan steps implemented and the maintainer confirmed the full bot
session on cabinet ("everything looks great in-game", 2026-09-14). User docs updated (README hero
section + Full Feature List row + BPL/Scores cross-references; `screenshots/versus_bot.png`).
**Skill model RETUNED 2026-09-14** after the maintainer's playtest + distribution review (see
Deviations) — `skill.rs` is now a lean + two-regime jitter model; design §4.6 rewritten.
Uncommitted — maintainer commits manually.
NEXT ACTION (maintainer): playtest the retuned bot (L10 should PFC nearly every song and MFC only
~1 in 10; L1 should fail ~1 in 10; the Marvelous count should exceed S-Marvelous until L9/L10;
the FAST/SLOW readout should show BOTH sides with a per-song tilt and short runs), then `git commit` (no attribution
trailers). Optional follow-ups, none
blocking: bemani-buddy migration for `opt_mod_bot_opponent` / `opt_mod_bot_opponent_level` (other
repo); the individual §7.3 items not yet ticked below can be spot-checked at leisure (every
`MultiplayerBot` WARN names its fail-open cause, so a field log is self-diagnosing).

Resume protocol: read this file, then `implementation/plan.md` (checklist = step status),
then `design/detailed-design.md` §4 for the component you are touching. RE facts:
`research/bot-controller-re.md`, `research/versus-impersonation-re.md`.

## Done

- 2026-09-13 — PDD Steps 1–8 complete: register accepted (D1–D24), research (U1–U11) in
  `research/`, design `Status: Approved 2026-09-13`, plan `Status: Approved 2026-09-13`,
  `summary.md` written.
- 2026-09-13 — **Step 1** (1 task: `extract-foot-panel-swap-service`): NEW
  `src/services/foot_panel_swap/{mod.rs,layout.rs}` (per-side `Controller { Off, Perfect, Bot }`,
  Bot > Perfect > Off; pre `Late` / post `Early` judge pair registered once; 0x58 stock object;
  `set_perfect` / `arm_bot` / `disarm_bot` / `controller`; the `Bot` branch WARNs once per arm
  and behaves as Off until Step 3 wires the panel object); `lib.rs` step 6b0 init;
  `autoplay.rs` slimmed to a client (no statics/callbacks/signatures; watermark asks the
  service). `layout.rs` host tests 7/7; `cargo check` / `cargo fmt` / `./build.sh` clean.
  Task record: `.agents/scratchpad/2026-09-13-multiplayer-bot/extract-foot-panel-swap-service/`.

- 2026-09-13 — **Step 2** (tasks `pure-cores` + the revised harness task): pure
  `src/mods/multiplayer_bot/{eligibility,skill,planner}.rs` (stub `mod.rs`); **no-Boo
  correction** (World's judge accepts grades 0..3 only — Good ±124 is the outermost graded
  window; a 125..160 ms event is matched, rejected, Missed at +160) applied to skill/planner and
  the design (A.5/§4.6/§4.7, approval re-dated); gauge RE
  (`docs/gauge_and_judge_scoring_research.md`); `tools/bot_sim/` offline simulator + HTML
  report (`scripts/bot_sim.sh`), `scripts/validate_multiplayer_bot.sh` = `cargo test` in the
  tool (63 tests); corpus run 66,210 songs / 6.6 s / 0 parse errors. Records:
  `.agents/scratchpad/2026-09-13-multiplayer-bot/pure-cores/`.

- 2026-09-13 — **Step 3** (task `bot-controller`): `foot_panel_swap` now builds the cloned
  7-slot vtable + two `BotFootPanel` objects at init (slot 5 `getPressAge = CURRENT_MC[side] −
  event_mc[panel & 7]`, slot 6 zeroes the event) and the `Bot` arm of `swap_in` is live;
  `multiplayer_bot/filler.rs` (Results → `NoteView`s → `plan_frame`, permanent planner-vs-judge
  self-check), `self_test.rs` (`developer_mode` + `DDR_BOT_SELF_TEST=<level>` arms the bot on
  the human's entered side(s), tally INFO at exit), `mod.rs` `Mod` impl registered in `lib.rs`
  (`"multiplayer-bot": true`). 67 host tests; check/fmt/build clean. Record:
  `.agents/scratchpad/2026-09-13-multiplayer-bot/bot-controller/`.

- 2026-09-13 — **Step 4** (tasks `option-rows-textures-menu` + `impersonation`): `mod.rs` rows
  `bot_opponent` (bool, "Bot Opponent (1P Only)") + child `bot_opponent_level` (1..=10, default
  5, `ShowWhen::Equals`, load-clamped) → `option_on(side)` / `level(side)`; `disable` hides,
  `Duplicate` reseeds + re-shows; labels + three WIDE previews in `scripts/option_strings.py`
  (en/ja/ko) → 15 new PNGs (additions only); `option_menu_settings` entries after `autoplay`.
  NEW pure `session.rs` (`classify(prev,next,active) → Edge`, `format_bot_name`, PLAY_WINDOW;
  mounted, 6 tests) + `impersonation.rs` (§4.5 flip with 7 named probes + full `Written` undo on
  arm failure, restore of the 3 snapshot items, reseed on GAMEPLAY/song reset, 20 s watchdog,
  `active_bot_side()` for Step 5); scene callback runs impersonation FIRST; `init` also gates on
  `player_option_offset()`. 73 host tests; check/fmt/build clean. Records:
  `.agents/scratchpad/2026-09-13-multiplayer-bot/{option-rows-textures-menu,impersonation}/`.

- 2026-09-13 — **Step 5** (task `extra-stage-guard`): `extra_stage_grant` AOB appended to
  `SIGNATURES` (soft consumer) — sweep `[+]` on all four builds (`+0x1C6970` / `+0x1CA7E0` /
  `+0x1DD0B0` / `+0x1DDCD0`, raw scan = exactly 1 match each), `RESULT: ALL GREEN`;
  `extra_stage_guard.rs` = ONE `GenericDetour<unsafe extern "C" fn(i32)>` clearing `PW[bot]+0x4`
  around the original under a drop guard when `impersonation::active_bot_side()` is Some; fail-open
  (miss ⇒ WARN + stock rule, mod still enables); `mod.rs` wired. 73 host tests; check/fmt/build clean.
  Record: `.agents/scratchpad/2026-09-13-multiplayer-bot/extra-stage-guard/`.

- 2026-09-13 — **Step 6** (task `interaction-pass-and-docs`): interaction audit found a real
  defect class — with the human on P2, every "P1 governs when both entered" policy read the BOT
  side's stale option cache (`versus_mirror` never engages inside the window). Fix:
  `multiplayer_bot::is_bot_side(side)` + one-line exclusions in premium_free (`human_entered`),
  announcer_mute, training_mode (pre-shift + loop-latch governing side), calibration census,
  assist_tick latch. D22 taken: `game_audio::{versus_pan, set_versus_pan}` (+0x20C4 off the derived
  `audio_manager_global`, disp32 byte-identical on all four builds) written 1 at the flip, restored
  at exit / undo. `docs/multiplayer_bot_research.md` (10 sections) + AGENTS.md "Multiplayer Bot"
  row; `judge_hook.rs` doc names `foot_panel_swap` as the swap owner. 73 host tests; check / fmt /
  build / signature sweep ALL GREEN. Record:
  `.agents/scratchpad/2026-09-13-multiplayer-bot/interaction-pass-and-docs/`.

## In flight

- Nothing — implementation complete; cabinet validation is the maintainer's.

## Deploy & test log

- 2026-09-14 — **Full-session cabinet check PASSED** (maintainer): Steps 4–6 build deployed with the
  15 label/preview PNGs; bot session engages, plays and restores as designed. Individual §7.3
  line items below stay listed as the spot-check checklist.

- (pending) Step 1 build — design §7.3 item 1: autoplay ON ⇒ all Marvelous + watermark; boot
  log `FootPanelSwap started` + `FootPanelSwap: registered judge swap (… bot objects ready)`.
- (pending) Step 3 self-test — `developer_mode: true` + env `DDR_BOT_SELF_TEST=10`, then `=1`:
  `SELF-TEST bot armed on side N`, hands-off play, song-end `SELF-TEST tally … mismatch=0`.
  Repeat on an old build (20250805 / 20260224) for §7.3 item 10.
- (pending) Step 4 full session — §7.3 items 2–5, 7, 8. Deploy the DLL + the 15 PNGs. Log grep:
  `MultiplayerBot: registered BOT OPPONENT (1P ONLY) option` / `… BOT LEVEL option …` at boot;
  per song `MultiplayerBot: side N impersonated as "BOT LVn" for Pm's song mcode=… diff=… (sigma=…
  p_miss=… seed=…)` at the 25→26 edge, `… re-rolled on GAMEPLAY entry` at 28, `… restored (BOT LVn
  …) planned … | judged … | mismatch=0 frames=…` on the exit from scene 30; refusals with the
  option ON: `MultiplayerBot: not engaging this song -- <Refusal>`; NO WARN from `MultiplayerBot`
  (every WARN there is a named fail-open refusal or the 20 s watchdog). On screen: two READY
  panels, P2 lane at P1's speed/skin, `BOT LV10` name plate, two results panes, TOTAL RESULTS 1P
  only; no side-1 `save_sender` on the wire; P1's per-stage save proceeds.
- (pending) Step 6 governance + pan — human on P2 with P1's cached premium_free / LOOP SONG /
  assist_tick / announcer_mute ON: the bot session must follow P2's rows (no unexpected free stage,
  no loop, no clap track, announcer per P2); SEs pan left/right during the bot song, centre after.
  With `two-player-bpl-mode` ON (§7.3 item 9): the battle frame shows `BOT LV<n>` on the bot's board.
- (pending) Step 5 extra stage — §7.3 item 6: 3-stage setting, low-level bot, human AAAs ⇒ EXTRA
  STAGE granted; log `MultiplayerBot: extra-stage grant evaluated without the bot (side N)` at the
  results window-out of stage 0; boot log `MultiplayerBot: extra-stage guard installed`.

## Deviations & open questions

- **Skill model RETUNE — 2026-09-14 (maintainer targets after playtest):** the 2026-09-13
  zero-mean Gaussian (σ 60→5.4, p_miss 0.13→0, exp 1.4) gave L10 71 % MFC, L1 60 % fail,
  ≥ 50 % S-Marv from L6 and EX% saturated (96.6/99.0/99.9) over L8–10. Targets: ~10 % MFC at L10,
  ~10 % fail at L1, exclusive Marvelous > S-Marvelous nearly everywhere (S-Marv common only
  from ~L7, dominant at L9/10), a smooth EX% ramp. Key insight: a zero-centred bell of ANY width
  puts ≥ 2.4× more Marvelous-tier hits in the 24 ms S-Marv band than the 10 ms Marvelous shell
  — the Marv > S-Marv target is unreachable by σ alone, so the model gained a per-song ±LEAN
  centring the tight core on the shell (17 → 14.5 @L8 → 11.7 ms), an AR(1) drift, a two-regime
  jitter (pocket σ 3.5→2.5 w.p. 35 %→100 %, loose σ 60→12), `p_miss 0.015·u^1.4`, and a per-song
  log-normal `form` factor (sd 0.35) on loose σ + p_miss (smooths the NORMAL gauge's sharp
  miss-rate knee: fail 12/4/1/0 % over L1–4 instead of a cliff). Result (3 seeds): L10 9.8 % MFC
  (98 % PFC), L1 11.6 % fail, Marv > S-Marv through L9, S-Marv 15 → 26 (L7) → 61 % (L10), EX
  62 → 79 → 90 → 92 → 97 → 99. All anchors in `skill::Params`/`DEFAULT`; `curve_from` is shared
  with the simulator's `--set key=value` overrides (the old `--sigma-*`/`--pmiss-*` flags are
  gone); `Form` lives in the planner's `SongState` (re-rolled per song / reset). Summary table
  grew per-grade shares, mean score, PFC%, per-difficulty MFC%, and the per-song SLOW share ± sd.
  **Same-day follow-up:** the lean's SIDE is a sticky Markov chain (per-song `p_late` 50/50 ±
  0.15, stickiness 0.7) instead of one sign per song — every song shows both FAST and SLOW
  (maintainer: an all-one-side attempt reads as unnatural); grade mix provably unchanged.
  78 host tests. Record: `.agents/scratchpad/2026-09-13-multiplayer-bot/skill-retune/`.
- (superseded) Skill-curve tuning 2026-09-13: `SIGMA_L1_MS 60 / SIGMA_L10_MS 5.4 / P_MISS_L1
  0.13 / P_MISS_EXP 1.4` — the constants the playtest rejected.
- Model simplifications (report §4): head-Missed freeze ⇒ N.G. (game may tap-Miss the tail);
  accepted-candidate-past-+160 ⇒ Miss (game double-submits); ranks = community table.

- Plan deviates from the design text in two small ways (maintainer-approved with the plan):
  `foot_panel_swap` is a directory (`mod.rs` + pure `layout.rs`) so the `BotFootPanel`
  layout / vtable image get host tests; `eligibility::Inputs` carries `Option<T>` +
  `Refusal::Unavailable` (BPL `GateInputs` shape).
- Step 3's dev self-test (`DDR_BOT_SELF_TEST=<level>`, dev-mode gated) is PERMANENT — it is
  the old-build portability probe (§7.3 item 10).
- Step 4 (recorded in the `impersonation` scratchpad): the arm-FAILURE undo restores every
  written byte (incl. the mirrored chart fields + Option copy), broader than the design's
  3-item snapshot — the normal `restore()` is exactly the design's three items; the restore
  re-syncs the bot side's autoplay taint to `foot_panel_swap::controller(bot) == Perfect`
  instead of clearing it blindly.

## Key facts for a cold resume

- Host is macOS ARM: plain `cargo test` cannot compile `retour`; pure modules run through
  `scripts/validate_multiplayer_bot.sh` = `cargo test --manifest-path tools/bot_sim/Cargo.toml`
  (the simulator crate `#[path]`-mounts the DLL's pure files from `tools/bot_sim/src/{core/ssq,bot}/mod.rs`
  — mounts must live in REAL directories or the `..` chains don't resolve). Cross-module pure
  imports use `use super::x`. New pure file ⇒ add a mount line there.
- Judge facts (20260825, `docs/gauge_and_judge_scoring_research.md`): grades accepted 0..3
  (`grade < min(best,4)` — no Boo), ONE accepted note per actor per frame, rejected match keeps
  the press, Miss at `mc > note.mc+160`, walk cutoff −260, jump spread 66, shock window [−34,+84];
  `judge_submit` broadcasts FC → combo(0x1033) → grade code; NORMAL gauge init 5000/10000,
  Good = 0 change, formulas transcribed in `tools/bot_sim/src/gauge.rs`.
- Readiness gates per hand-back: `cargo check --target x86_64-pc-windows-msvc` → `cargo fmt`
  (whole crate) → `./build.sh` → harness → `./scripts/validate_signatures.sh
  ~/Desktop/ddr_modules` if `signatures.rs` changed (Step 5). Never `git commit`/push.
- Judge swap slots are load-bearing: pre `Priority::Late` / post `Priority::Early` (autoplay's
  today; `per_song_judgement_offsets` Early, PUS Normal must keep their order).
- `judge_hook::foot_panel_offset()` = `GamePlayActor` `IFootPanel*` slot (0x270 old / 0x278
  new). Actor side `+0x84`; results vector `+0xB0/+0xB8` (0x40 stride, `types::game_note`);
  current beat `+0x168` (autoplay.rs misnames it `NOTE_COUNT`).
- Stock `AutoFootPanel` object is 0x58 on 20260721+ (autoplay allocates 0x40 — latent
  under-allocation, fixed by the service).
- `lib.rs`: services init in numbered steps; `judge_hook::init` is step 6b (~line 463); mods
  `register` (→ `init`) at step 7 (~line 579) and `enable_with_config` at step 8. Insert
  `foot_panel_swap::init(&signatures)` right after 6b.
- Scenes (0-idx): 25 SONG_SELECT, 26 SONG_TO_STAGE_INTERSTITIAL, 27 STAGE_INDICATOR,
  28 GAMEPLAY, 29 STAGE_RESULT, 30 RESULTS_DETAIL, 31 stage-bump wait, 32 FINAL_RESULTS,
  34 EAM_EXIT. Play window = {26..30}. Scene callbacks fire BEFORE the original
  `createNextSequence` runs (flip at 25→26 lands before the 27/28 loaders read `PW+0x4`).
- `custom_options`: `get_value(side, id)` vs `set_value(id, side, v)` — argument order differs.
  Parent before child. `Err(Duplicate)` = success, reseed atomics. Bool rows on
  `is_available()`, scalar rows on `row_injection_available()`.
- Dev-mode gate shape: `crate::mods::config::get().and_then(|c| c.layeredfs.as_ref())
  .map(|l| l.developer_mode)` + `std::env::var_os("…")` (see `two_player_bpl_mode/mod.rs`).
