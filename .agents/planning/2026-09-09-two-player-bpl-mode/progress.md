# Progress — 2-Player BPL Mode

Updated: 2026-09-10
Status: **Step 5 of 5 — cabinet-validated 2026-09-10 (deploy #2): the battle HUD renders
in local 2P versus; 3 placements, 0 mod WARNs.** Remaining matrix items are optional
follow-ups (EX scoring, overlap, quick-restart-in-place). Uncommitted — maintainer
commits manually.
NEXT ACTION: none required. Optional: matrix items 3 (EX scoring ON), 4b (in-place
`song_reset` restart), 6 (overlap with PUS widgets / training strip) when convenient.

Resume protocol: `implementation/plan.md` (checklist) → `design/detailed-design.md`
(mechanism, offsets, error table) → `research/re-findings.md` (Ghidra R1–R7).

## Done

- Step 1 — `signatures.rs`: 4 AOBs (`battle_frame_ctor`, `actor_add_child`,
  `dance_matching_slot_probe`, `gpa_score_select`) + `derive_two_player_bpl` (2 RTTI
  vtables, rank fn, cabinet idx global, scene-resource manager global, 4 published
  values) + accessors `dance_matching_slot_off()` / `gpa_score_offsets()`.
  `song_reset::{live_dps, gameplay_actors, FIRST_CHILD_OFFSET, NEXT_SIBLING_OFFSET,
  GPA_SIDE_OFFSET}` → `pub(crate)`. Four-build sweep ALL GREEN, every AOB exactly one
  hit per build, every derived value identical across builds (slot 0x7F0; GPA
  0x1D0/0x1D8/0x1D4). `shape_diff.py`: all four AOBs identical through every consumer
  read window (divergence only in unread tails). Bug caught by the sweep: money-score
  imm32 is at match+**19** (not +18) — fixed in code + docs.
- Step 2 — `src/mods/two_player_bpl_mode/logic.rs` (eligibility / player_name / smooth /
  gauge_fraction / clone_vtable_image) + `scripts/validate_two_player_bpl.sh`: 17 host
  tests green.
- Step 3 + 4 (merged) — `src/mods/two_player_bpl_mode/mod.rs`: full runtime path
  (scene-armed frame poll, child classification by vtable, 5 gates, 0x290 alloc, stock
  ctor, vtable clone install, BATTLE_INFO fill, `addChild` + parent verify), slot-4
  wrapper (package pre-check + neutralise, scope-guarded `GameWork+0` flip), slot-6
  replacement (isEx-selected score → smooth → gauge → stock rank fn). Dry-run kept as a
  DEV-MODE switch (`DDR_BPL_DRY_RUN=1`, requires `layeredfs.developer_mode`) instead of
  a separate deploy. Registered in `src/mods/mod.rs` + `src/lib.rs` (after s_marvelous).
- Step 5 (docs) — AGENTS.md Key Entry Points row; README highlight (after Quick
  Restart/Fail/Logout) + full-list row; `docs/in_shop_battle_local_versus_research.md`
  §8 implementation addendum (incl. the §3 correction).
- Gates: `cargo check` clean, `cargo fmt` run, `./build.sh` clean (release DLL built),
  `validate_signatures.sh` ALL GREEN with the new keys attributed to
  `mod:two_player_bpl_mode` as REQUIRE/required, `validate_two_player_bpl.sh` green.

## In flight

Nothing. Working tree holds the uncommitted feature (see `git status`); the two
pre-existing modified files `.agents/learnings/learnings.md` and
`.agents/planning/2026-08-29-s-marvelous-judgement/progress.md` are NOT part of this
feature (maintainer's own edits — leave them).

## Deploy & test log

| # | Build | Result |
|---|---|---|
| 1 | 2026-09-10 00:59 | All signatures/derivations resolved live (identical to the sweep); mod initialised + enabled. 2P versus: gates passed, then `WARN dance_matching package not resident -- battle frame skipped` — NO frame, no crash. Root cause: `dance_matching_package` did TWO loads (`*(*global + 0x7F0)`) where the stock probe does THREE (`*(*(*global) + 0x7F0)` — the global holds the 0x28-byte manager OBJECT whose field 0 is the slot array). Read heap garbage past the manager ⇒ null/garbage ⇒ fail-open refusal. Fixed in `mod.rs` + all docs; rebuilt. |
| 2 | 2026-09-10 01:13 | **WORKING** — maintainer confirmed the BPL HUD in gameplay. Log: 3× `TwoPlayerBpl: battle frame placed (dps=0xac28180, is_ex=0, mcode=38903/38901/38901, diff=0/3/3, names=[PLAYER1,PLAYER2])` across three 2P versus plays (money score, guest players), each at the first GAMEPLAY frame (≈1 s after scene 28); every play exited via quick-fail (fast `finish`) → new DPS next song → new frame (same heap address reused, latch cleared at GAMEPLAY entry as designed). **Zero `TwoPlayerBpl` WARNs**; the run's other WARNs (PremiumFree BUG-1 diag, score_guard suppression) belong to the quick-fail path, not this mod. Matrix items covered: 2 (frame visible, PLAYER1/2 fallback names, money score), 4a (fresh DPS ⇒ fresh frame), 5 (quick-fail teardown ×3, clean). Not yet exercised: 1 (solo/doubles refusal line), 3 (EX scoring), 4b (in-place restart), 6 (overlap), 7 (toggle). |

### Cabinet matrix (design "Testing Strategy — Cabinet")

1. Solo / doubles / course: no frame; one `TwoPlayerBpl: no battle frame this song (...)`
   INFO; no WARN.
2. 2P versus, money score: frame at the READY panel; boards show names or
   `PLAYER1`/`PLAYER2`; gauges fill; 1st/2nd badges swap with the lead; margin readout
   = difference of the stock readouts. Log: `TwoPlayerBpl: battle frame placed (...)`.
3. 2P versus with operator EX scoring ON: gauge max = chart EX max; digits match the
   stock EX readouts.
4. Quick restart (press 1): fresh DPS ⇒ frame re-created (second `placed` line).
   In-place `song_reset` restart / training scrub: same frame, boards snap to 0.
5. Quick fail / natural song end / results: no crash at teardown, no WARN.
6. Visual overlap with power_user_statistics widgets and the training strip.
7. Mods-tab toggle OFF mid-song: frame stays; next song none. ON: next song has it.

What a failure looks like in the log (all fail-open):
- `gate input unavailable (...)` — stage_records / session-state decode down.
- `matching network not idle (local cabinet idx Some(N))` — should never fire; if it
  does, the static-init assumption (§8.1 of the RE doc) is wrong on that build.
- `dance_matching package not resident` — custom LayeredFS `dance_common`/loader
  masks; frame skipped, game unaffected.
- `Actor::addChild refused the battle frame` — precondition failed (check the parent /
  next fields); 0x290 bytes leaked once, game unaffected.
- `panic inside onInitialize wrapper / onUpdate replacement (contained)` — investigate
  the first occurrence with a debugger attached; frame stops updating.

## Deviations & open questions

- D6 amended (design): two files (`mod.rs` + `logic.rs`) instead of one, for the host
  harness.
- Plan Steps 3 and 4 were merged into one build with the dry-run as a dev-mode env
  switch (`DDR_BPL_DRY_RUN`), so the maintainer can still get the zero-write triage
  build by setting the env var, without a separate deploy.
- `song_reset::read_step` / `DPS_STEP_*` were NOT promoted after all — the readiness
  test "LayoutActor + two GamePlayActors present" (child walk) is the exact condition
  the normal DPS's case-1 guarantees, so no step read is needed.
- EX-score mode with DIFFERENT difficulties per side: the frame's `max_score` comes from
  `stage_record(0, stage)` (side 0's mcode/diff), exactly like stock's
  first-entered-side read. A side-1 chart with a larger EX max would show its gauge
  capped. Cosmetic; noted in AGENTS.md as a known v1 limit.
- The frame's `+0x1B0` participant count and BATTLE_INFO are written AFTER the stock
  ctor ran its 4-player branch (`GameWork+0 == 1`); the ctor's four
  `set player info : position=…` log lines will appear once per placement — expected.

## Key facts for a cold resume

- Everything the mod touches in game memory: its own 0x290 allocation, the DPS child
  list via the game's `addChild`, and a transient `GameWork+0` write restored by a
  `Drop` guard. No detours, no patches.
- Kill switch: Mods tab toggle, or `mods["two-player-bpl-mode"] = false` in
  mod-config.json.
- Offsets that are DERIVED (never hardcode): `dance_matching_slot_off`,
  `gpa_is_ex_off`, `gpa_ex_score_off`, `gpa_money_score_off`. Offsets that are
  layout constants attested by the ctor AOB: everything in `mod.rs`'s `FRAME_*` /
  `BI_*` tables.
