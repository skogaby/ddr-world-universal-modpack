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
