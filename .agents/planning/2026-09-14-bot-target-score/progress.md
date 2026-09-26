# Progress — Multiplayer Bot "Target Score" tier

Updated: 2026-09-14
Status: Complete (uncommitted — maintainer commits manually); cabinet-validated 2026-09-14
NEXT ACTION: none — maintainer commits.
Resume protocol: read `implementation/plan.md` (checklist) → `design/detailed-design.md` → `research/orientation.md`; the register is `idea-honing.md`.

## Done

- Step 1 — `PersistMode::Local` (JSON cache both ways, never on the wire; load gate split by `LoadSource::{Network, JsonPrime}` through `PersistMode::accepts_load`) + `ScalarFormat::Labeled { prefix, terminal_value, terminal_label }`; matrix/format tests (56 green via `scripts/validate_custom_options.sh`, which now also mounts `persist_matrix_tests.rs` + `scalar_format_tests.rs`).
- Step 2 — `gpa_ghost_actor_probe` AOB + `derive_ghost_actor_probe` (publishes `gpa_ghost_actor_off`; `isReady`-prologue identity gate) + RTTI `ghost_actor_vtable`. Sweep: `[+]` on all four builds, `0x1F0` on 20250805/20260224, `0x1F8` on 20260721/20260825; `shape_diff.py` confirms the only divergence is the disp32 itself.
- Step 3 — `ghost.rs` (bands, S-Marv floor, `decide_tap`, `histogram`, `expected_tap_grade`), `skill::Form::next_side` + `Rng::below`, planner `SongState::with_ghost` / freeze N.G. `drop_hold` / shock N.G. press / `repro_miss`, `eligibility::{BotMode, TARGET_VALUE, clamp_value}`, `session::format_bot_name(BotMode)` (`TARGET`). `bot_sim` mounts `ghost.rs`; 94 tests green.
- Step 4 — `ghost_source.rs` (probed + vtable-gated + state==2 read of the human's GhostActor vector), filler ghost bind on the Results rebuild + length check + LV10 fallback (one WARN + 3 s toast) + `SongSummary` provenance, rows 1..=11 `Labeled` + both `Local`, impersonation `Active.mode` / plate / seed / flip+restore INFO (`mode=target ghost_id=… ghost_len=… target=[…] repro_miss=…` or `FALLBACK LV10 (<reason>)`), `s_marvelous::state::armed_window`.
- Step 5 — AGENTS.md (bot row, GamePlayActor fork note now `+0x1F0`, config section), `docs/multiplayer_bot_research.md` §11, README, `scripts/option_strings.py` + the three regenerated `seop_image_bot_opponent_level.png`. Gates: `cargo check` / `cargo fmt` / `./build.sh` clean; `validate_signatures.sh` ALL GREEN; `validate_multiplayer_bot.sh` 94/94; `validate_custom_options.sh` 56/56; privacy grep adds no hits.

## In flight

Nothing. Working tree holds the whole feature uncommitted (24 files + 2 new modules + this planning dir).

## Deploy & test log

- 2026-09-14 — first cabinet deploy: maintainer reports everything working in-game (Target Score selectable as text, replay engages, no issues observed). Feature declared done. Log anchors for any future look: boot `gpa_ghost_actor_off (derived) = 0x1F8|0x1F0`, `MultiplayerBot: ghost source ready (GamePlayActor+0x… -> GhostActor …)`; per song `target ghost bound on side N id=… notes=… smarv_floor=…` then the restore line's `repro_miss`.

## Deviations & open questions

- D5 fallback toast rides `toast::flash_with_hold` (fail-open when the widget renderer is down).
- Length mismatch is checked on every Results rebuild (not only the first bind) — same outcome, stricter.
- Ghost `Refusal::NotReady` should be unreachable (A1); if it ever logs, the state value is in the WARN.

## Key facts for a cold resume

- GhostActor field: `GamePlayActor + gpa_ghost_actor_off` (derived; `+0x1F8` 2026-03+, `+0x1F0` old). State pairs `+0x58+idx*8`, idx u16 `+0x82`, ready = 2, id i64 `+0x90`, `vector<u8>` `+0x98..+0xA0`.
- Ghost byte alphabet 0 M / 1 P / 2 Gr / 3 Gd / 4 Boo / 5 Miss / 6 O.K. / 7 N.G., one per Results index.
- S-Marv exclusion: Marvelous band `[W+1, 17]` when `s_marvelous::is_enabled()` and `state::armed_window(side) = W > 0`.
- Option value 11 = Target; both rows `PersistMode::Local`; the level row is `ScalarFormat::Labeled`.

---

## Addendum 2026-09-25 — Target Score presentation polish

Updated: 2026-09-25
Status: implemented, host-validated — cabinet deploy pending (uncommitted — maintainer commits manually)
NEXT ACTION: deploy (`./scripts/deploy.sh`; no `data_mods/` change needed) and run the checklist below.
Resume protocol: `docs/multiplayer_bot_research.md` §12 (RE + mechanism); module docs of
`target_name.rs`, `plate_label.rs`, `s_marvelous/state.rs` (`set_excluded`).

### Done
- S-Marvelous per-side exclusion (`state::set_excluded`, held by the impersonation for a Target
  session): side never armed; sticky window + song latch cleared ⇒ results tab / graph / emblems /
  upload stock for it; data-feed re-hides FAST/SLOW on its Marvelous (`flash::on_excluded_marvelous`);
  its results pane shows `scre_tab_num_minus` in the shared 7-row sheet's S-MARV slot; PUS readout
  omits S-Marv for it. Maintainer choice: "-" (the label word is baked into one sheet shared by
  both panes — full per-pane removal was offered and not chosen).
- Target name: `ghost_id_lookup` + `rival_set_dancer_name` AOBs, `derive_target_name_sites`
  (stage 1 `pw_chart_*_off`, stage 2 `pw_target_select_off`, `rival_sets_global`,
  `rival_set_score_entry`) — `[+]` on all five builds, sweep ALL GREEN.
- Plate = the target's name (own best ⇒ own name, else rival / ranking holder; must match the
  lookup's ghost id), `TARGET` when unnamed. Maintainer choice: original name on the plate + a
  smaller secondary "TARGET BOT" in the plate's own font (glyph sets have no parentheses).
- `plate_label.rs`: two process-lifetime SpriteLayers (gameplay `cote_edge_*` on the bot
  ScoreActor's `dance_name`/`name_usr`; results `cote_shadow_*` on
  `player_Np_info_usr/profile_usr/player_name_usr`), 0.55 × plate height, 2 px above,
  `input_manager` frame driver.
- Pre-existing bug fixed (maintainer-approved): impersonation's PlayerWork chart mirror used the
  20260324+ offsets on every build; now `player_work_chart_offsets()` (0x60/0x64/0x6C on
  20250805 / 20260224).
- Gates: `cargo check` / `cargo fmt` / `./build.sh` clean; `validate_multiplayer_bot.sh` 98/98;
  `validate_s_marvelous.sh` green; `validate_power_user_statistics.sh` green; sweep ALL GREEN.

### Deploy & test checklist (none run yet)
1. Boot log: `pw_chart_*_off (derived)`, `pw_target_select_off (derived)`, `MultiplayerBot:
   target-name lookup ready …`, `MultiplayerBot: PlayerWork chart fields …`; no
   `TARGET BOT label` WARN.
2. Target Score vs a rival / ranking target: flip INFO `impersonated as "<NAME>" … -- rival n /
   world ranking …, ghost id …, TARGET BOT label armed`; plate shows the name in gameplay, the
   versus/BPL frame and results; `TARGET BOT` sits just above the plate in gameplay and results
   (`MultiplayerBot: TARGET BOT label bound (Gameplay|Results, side N)`); gone on TOTAL RESULTS /
   next song. Placement constants: `plate_label::LABEL_RATIO` / `LABEL_GAP` (+ `Surface`
   alignment) — tune from a screenshot.
3. Own-best target ⇒ the human's own name on both plates; TARGET OFF ⇒ `TARGET`, no label, INFO
   `plate stays TARGET -- no target ghost …`.
4. S-Marvelous ON: bot side never flashes S-Marv / violet; its Marvelous shows NO FAST/SLOW;
   results pane: S-MARV "-", MARVELOUS inclusive, FAST/SLOW without Marvelous, stock graph;
   human pane unchanged. `SMarvelous: results row shows "-" …` once.
5. Level bots unchanged (`BOT LV<n>`, S-Marv classified).
6. Old build (20250805 or 20260224): Level bot plays the human's difficulty (it may not have
   before).

### Deviations & open questions
- A Target song that falls back to LV10 AFTER a named flip (ghost download failure / length
  mismatch) keeps the name + TARGET BOT label; S-Marv stays excluded for it too.
