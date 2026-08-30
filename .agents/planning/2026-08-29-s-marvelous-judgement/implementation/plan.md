# Implementation Plan — S-Marvelous Judgement

Status: Approved 2026-08-29

Design: `.agents/planning/2026-08-29-s-marvelous-judgement/design/detailed-design.md`
(Approved 2026-08-29). Each step leaves the system working and demonstrable;
cabinet deploys are this repo's real validation. Maintain `progress.md` in the
feature directory throughout (per AGENTS.md).

## Checklist

- [x] Step 1: Mod skeleton, classification tap, and log-only validation
- [x] Step 2: core/ap2 parser + serializer with round-trip identity
- [x] Step 3: core/ap2 editing primitives
- [x] Step 4: Gameplay flash end-to-end (first synthesized AFP on cabinet)
- [ ] Step 5: Combo digits
- [ ] Step 6: Full-combo splash
- [ ] Step 7: Results score tab row + exclusive MARVELOUS
- [ ] Step 8: Results judgement graph
- [ ] Step 9: Full-combo emblems (per-stage + total results)
- [ ] Step 10: Integration hardening and documentation

---

Step 1: Mod skeleton, classification tap, and log-only validation

- **Objective**: the `s-marvelous` mod exists, classifies live, and proves the
  ±12 subset property on the cabinet before any art or AFP work.
- **Guidance**: design §4.3, §4.10, §5.1, §5.3. Create `src/mods/s_marvelous/`
  (`mod.rs`, `state.rs`), register in `src/lib.rs`
  (`required_signatures = ["judge_submit"]`). Add the armed/disarmed tap block
  to `data_feed.rs` beside the calibration tap (atomics only; call
  `data_feed::install` from the mod's init). Wire the scene callback
  (arm/latch at GAMEPLAY entry with `window_ms` from config, disarm at exit)
  and the `song_reset` subscription. Log one INFO per song end:
  `smarv=N marv_total=M side=S window=W`.
- **Tests**: host tests for the `state.rs` combo-bit machine and window edges
  (inclusive |ms| == window, O.K. neutrality, combo restarts); config clamp
  tests.
- **Integration**: standalone; every later step consumes `state.rs`.
- **Demo**: cabinet log shows per-song S-Marv counts; autoplay yields
  `smarv == marv_total`; toggling the mod off makes the block cost one
  relaxed load.

Step 2: core/ap2 parser + serializer with round-trip identity

- **Objective**: parse any AP2 template and re-serialize it byte-identically —
  the foundation every AFP patch stands on.
- **Guidance**: design §4.1. New pure module `src/core/ap2/` (`parse.rs`,
  `write.rs`, model types). Opaque carriage of unmodeled tags; recursive
  DefineSprite sections; frame-label name-reference arrays; string-table
  cipher round-trip reusing `core/afp.rs`. Include a synthetic-fixture builder
  (constructs minimal valid AP2 docs via the writer — no Konami bytes in the
  repo). Scaffold `scripts/validate_ap2.sh` (temp-crate harness, house
  pattern) with optional dev-only legs: round-trip real templates from a local
  game-data dir and cross-check structure against bemaniutils `parseafp`.
- **Tests**: round-trip identity on synthetic fixtures; malformed-input
  rejection without panics; alignment/limit validation (string table ≤ 64 KiB,
  packed-field widths).
- **Integration**: consumes `core/afp.rs`; consumed by Steps 3–4.
- **Demo**: `cargo test` green; on the dev machine the script round-trips
  `dance_judge` and `detail_result` byte-identically.

Step 3: core/ap2 editing primitives

- **Objective**: the mutations the feature needs, verified on host.
- **Guidance**: design §4.1 API: label read/write,
  `clone_labeled_segment` (+`TagRemap`), `add_shape`,
  `add_place_object_named`, `adjust_placements`, `max_character_id`,
  string-table growth with offset fixups.
- **Tests**: on synthetic docs — cloned segment has correct frame spans/tag
  indices/label entries; remapped character IDs; serialized output re-parses;
  placement adjustments hit only predicated objects. Dev-only: patch a real
  template, re-parse with bemaniutils, render the new label with
  `afputils render` to eyeball the segment.
- **Integration**: extends Step 2's model; consumed by Steps 4, 6, 7, 9.
- **Demo**: a synthetic doc gains an `in_smarvelous` segment and survives
  round-trip; dev render shows the cloned segment playing.

Step 4: Gameplay flash end-to-end (first synthesized AFP on cabinet)

- **Objective**: S-MARVELOUS word shows on cabinet for ≤ window steps — the
  step most likely to invalidate the synthesis architecture, done as early as
  possible.
- **Guidance**: design §4.2, §4.4. Promote geo label-rewrite to a shared
  helper; asset pipeline for `dance_judge`: donor-anchored atlas clone of the
  word-art region (maintainer's recolored PNG under `data_mods/s_marvelous/`),
  cloned geo with rewritten labels, geo MD5 mapping registration.
  `afp_patches.rs`: `register_patch("dance_judge", …)` running the Step 3
  editor (clone `in_marvelous` → `in_smarvelous`, remap to the new shape).
  Extend `overlay_element_styling`: `pub fn judge_clip(side)` + idempotent
  shared capture install requested from `s_marvelous` init. Flash re-drive
  from the tap (`afp_mc_op(mc, 0xF09, "in_smarvelous")`), gated on
  patch-success flag.
- **Tests**: host tests for the patch fn against a synthetic dance_judge-shaped
  fixture (label exists, shape remapped); everything else is cabinet
  validation.
- **Integration**: first consumer of Steps 1–3 together; establishes the
  patch-success gating pattern reused by Steps 6, 7, 9.
- **Demo**: cabinet gameplay shows S-MARVELOUS on tight steps and MARVELOUS on
  loose ones; calibration hide hides it; judgement styling scales it; versus
  binds per side; mod disabled ⇒ stock word.

Step 5: Combo digits

- **Objective**: all-S-Marvelous combos show the new digit set + tint.
- **Guidance**: design §4.5, §4.11. New signature `combo_digit_refresh`
  (tint-immediates anchor); post-original detour replicating the traversal-6
  walk for places {10,100,1000} with `daco_combo_smarvelous_%d` (FRESH-mode
  injection) + the new tint pair on root2/root3 (vfunc+0x98). First cabinet
  deploy verifies tint semantics with a debug pure color before finalizing
  constants.
- **Tests**: host test for the override predicate
  (`stock worst == 0 && combo_is_all_smarv`); cabinet for visuals and the
  self-healing degrade.
- **Integration**: consumes Step 1's combo bit; independent of Step 4's AFP
  path (textures only).
- **Demo**: cabinet combo counter swaps art/tint while all-S, reverts on the
  first loose Marvelous, unaffected by O.K. steps.

Step 6: Full-combo splash

- **Objective**: an all-S MFC plays the S-MARVELOUS splash.
- **Guidance**: design §4.6. Dev-machine dump of `dance_fullcombo` to name the
  splash art (Appendix B item); patch all four templates
  (`clone_labeled_segment("marbelous_in" → "s_marbelous_in")` + art remap);
  new signature `fullcombo_actor_on_message` (module-unique
  `81 FA 34 10 00 00`); post-original detour re-driving the label when
  `type == 0 && combo_is_all_smarv`. Verify live that a missing label is a
  benign no-op before enabling by default.
- **Tests**: host test for the patch fn on synthetic fixtures shaped like the
  four templates; cabinet for the splash.
- **Integration**: reuses Step 4's patch/asset patterns and Step 1's bit.
- **Demo**: autoplay full song ⇒ S-MFC splash, sound plays once; a single
  loose Marvelous ⇒ stock MFC splash.

Step 7: Results score tab row + exclusive MARVELOUS

- **Objective**: native S-MARVELOUS row on per-stage results; MARVELOUS shows
  stock − n.
- **Guidance**: design §4.7, §5.2. Record-stream recompute helper
  (`smarv_count_from_record`, fail-closed) — host-tested, shared with Steps
  8–9. `detail_result` patch: `smarvelous_num_usr` instance + label placement
  (`scre_tab_detail_smarv` FRESH injection) + `adjust_placements` row
  repositioning. New signature `playdata_tab_update` + derived row helpers;
  post-original detour: mod-owned SpriteLayer on the new instance, stock
  marvelous widget glyph rewrite via `spritelayer_set_names`.
- **Tests**: host tests for the recompute helper on synthetic records
  (alignment, length-mismatch rejection) and for the exclusive-count math;
  cabinet for layout/visuals across 1P/2P/versus.
- **Integration**: first results-scene consumer; recompute helper feeds Steps
  8–9.
- **Demo**: results screen shows the S-MARVELOUS row with correct exclusive
  counts, indistinguishable in style from stock rows; patch failure ⇒ stock
  tab (no row, stock MARVELOUS count).

Step 8: Results judgement graph

- **Objective**: S-Marvelous as its own series + legend on the graph tab.
- **Guidance**: design §4.8. New signature `graph_tab_rebuild` + call-site
  derivations (chart append, lambda vftables, legend text ctor); per-second
  bucketing from record streams; one-shot marvelous-series subtraction;
  per-frame post-original series append + legend line.
- **Tests**: host tests for bucketing (section rule, edge timestamps);
  cabinet for chart rendering, page switching, and versus.
- **Integration**: consumes Step 7's recompute helper and record plumbing.
- **Demo**: graph shows the S-Marv series in its own color with a
  "■S-MARVELOUS" legend entry; marvelous series correspondingly reduced.

Step 9: Full-combo emblems (per-stage + total results)

- **Objective**: S-MFC emblems on both results surfaces.
- **Guidance**: design §4.9. Identify the template hosting `fc_usr` (dev dump,
  Appendix B); patch `loop_smfc` segment; signatures `result_window_build` +
  `total_result_populate`; post-original one-shot re-drives
  (`afp_mc_op(0xF09, "loop_smfc")`; total results bitmap re-load with injected
  `scre_total_player_*` texture). S-MFC condition from the recompute helper +
  clear kind 10.
- **Tests**: host test for the S-MFC predicate; cabinet for both screens
  (MFC vs S-MFC vs PFC stages side by side in a course).
- **Integration**: completes the results surface set; reuses Steps 3/4/7
  machinery.
- **Demo**: an all-S stage shows the S-MFC emblem on stage results and its
  pane on total results; a stock MFC stage shows the stock emblem.

Step 10: Integration hardening and documentation

- **Objective**: the full feature behaves as one product; knowledge is
  durable.
- **Guidance**: run the design §7 regression sweep: mod disabled ⇒
  byte-identical stock (all templates, all detours passthrough); song_reset
  paths (quick restart instant + delayed, training scrub/loop); rate play;
  course mode; interaction checks (pacemaker_swap, overlay_element_styling,
  calibration). Fix what falls out. Update
  `docs/s_marvelous_judgement_research.md` with the display-side RE and final
  mechanisms; add the AGENTS.md entry-point row; README operator notes
  (assets, config key, reboot-once atlas note).
- **Tests**: no new functionality — this step's validation is the sweep
  itself plus `cargo check` / `cargo fmt` / `./build.sh` readiness gates.
- **Integration**: end-to-end.
- **Demo**: full regression checklist green on cabinet; a fresh reader can
  navigate the feature from AGENTS.md.
