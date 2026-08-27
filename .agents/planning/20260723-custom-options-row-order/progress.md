# Progress — Custom Options Row Order

**Updated:** 2026-07-23
**Status:** Implementation complete + **validated on cabinet**. Ready to commit.
**NEXT ACTION:** Commit the change set (awaiting explicit go-ahead), then deploy/merge per
normal workflow.

Resume protocol: read `implementation/plan.md` (step checklist), `design/detailed-design.md`
(code sketches + rules), then this file. Idea decisions are in `idea-honing.md`; codebase
facts in `research/existing-mechanism.md`.

## Done

- Orientation + codebase research (row order == registration order; single lever is the
  `handles` iteration in `builder_hook::builder_detour_body`).
- Requirements clarified — 12 decisions, all confirmed (`idea-honing.md`). Only deviation
  from proposed defaults: Q2 id matching is **case-insensitive**.
- Detailed design written (`design/detailed-design.md`).
- Implementation plan written (`implementation/plan.md`).
- **Step 1** — added `row_order: Option<Vec<String>>` (`#[serde(default)]`) to
  `CustomOptionsConfig` (`src/mods/config.rs`).
- **Step 2** — added `src/services/custom_options/ordering.rs` (config store +
  pure `compute_order` + warn-once `display_order_for`); `pub mod ordering;` in `mod.rs`.
- **Step 3** — `custom_options::init()` reads `config...row_order` →
  `ordering::set_configured_order(...)`.
- **Step 4** — `builder_hook::builder_detour_body` reorders its `handles` snapshot via
  `ordering::display_order_for(&ids)` before injection (identity when unconfigured).
- **Step 5** — docs updated: README (example + `row_order` subsection + id list), AGENTS.md
  (custom_options config bullet), summary docs (`data_models.md`, `interfaces.md`,
  `components.md`).
- **Step 6 (build gates)** — `cargo check` clean, `cargo fmt` clean (no churn),
  `./build.sh` clean → `target/x86_64-pc-windows-msvc/release/ddr_world_hook.dll`.

## In flight

- Nothing uncommitted-and-pending beyond the change set below; no commit made yet
  (awaiting on-cabinet validation / explicit go-ahead to commit).

Change set: `src/mods/config.rs`, `src/services/custom_options/{ordering.rs (new),
mod.rs, builder_hook.rs}`, `README.md`, `AGENTS.md`,
`.agents/summary/{components,data_models,interfaces}.md`, and the root
`mod-config.json` (seeded `custom_options.row_order` with all 23 registered ids in
built-in order as a copy/rearrange baseline).

### Current built-in registration/display order (23 rows)

Enable order from `lib.rs` `mods_to_register`, then per-mod `register_option` order:
`premium_free`, `autoplay`, `timing_stats`, `pacemaker_to_mserror`,
`pacemaker_threshold`, `step_data_export`, `customize_appeal_board`,
`customize_background`, `customize_background_gameplay`, `customize_character_p1`,
`customize_character_p2`, `customize_lane_single`, `customize_lane_double`,
`customize_lanecover_single`, `customize_lanecover_double`, `customize_movie_size`,
`is_disp_weight`, `weight`, `center_arrows_1p`, `overlay_scale`, `overlay_opacity`,
`arrow_scale`, `arrow_opacity`. (webui cosmetics follow `CATEGORIES` order, skipping
categories with no discovered assets; profile fields register after cosmetics.)

## Deploy & test log

- **2026-07-23 — cabinet deploy, PASS.** Deployed with a manually rearranged `row_order`;
  the MODS-tab rows rendered in the edited order as expected. Feature confirmed working by
  maintainer.

## Deviations & open questions

- Malformed JSON *type* for `row_order` falls back to existing whole-file config default
  (out of scope to change; Alt C in design). Revisit only if it annoys operators.
- `ShowWhen` children can be visually stranded from their parent by operator ordering;
  visibility is id-resolved so it still works. Documented, intentional (Q7).

## Key facts for a cold resume

- **Change surface (4 code edits + docs):**
  1. `src/mods/config.rs` — add `row_order: Option<Vec<String>>` to `CustomOptionsConfig`.
  2. `src/services/custom_options/ordering.rs` — NEW: `CONFIGURED_ORDER: OnceCell<Vec<String>>`,
     `UNKNOWN_WARNED: AtomicBool`, pure `compute_order(registered, configured) ->
     (perm, unknown)`, `display_order_for(&[&str]) -> Vec<usize>` (identity fast-path +
     warn-once). Add `pub mod ordering;` in `custom_options/mod.rs`.
  3. `custom_options::init()` (mod.rs) — read `config::get()...custom_options.row_order`,
     call `ordering::set_configured_order(...)`.
  4. `builder_hook::builder_detour_body` — after the `handles` snapshot, reorder via
     `ordering::display_order_for(&ids)` (indices map 1:1: `handles[i].0 == OptionHandle(i)`).
- **Invariant:** never reorder `registry::STATE.options` (OptionHandle = index, must stay
  stable). Reorder only the transient snapshot; `rows::ROWS` + `options_scroll` follow it.
- **Default preserved:** absent/empty `row_order` ⇒ identity ⇒ byte-for-byte current behavior.
- **Rules:** listed-first (case-insensitive, dup→first-wins) → unlisted appended in
  registration order → unknown id ⇒ one WARN + ignore.
- **Build gate:** `cargo check --target x86_64-pc-windows-msvc` → `cargo fmt` → `./build.sh`.
  Validation is on-cabinet (matrix in plan Step 6).
- **Option id universe:** `premium_free`, `autoplay`, `center_arrows_1p`, `timing_stats`,
  `pacemaker_to_mserror`, `pacemaker_threshold`, `step_data_export`, `overlay_scale`,
  `overlay_opacity`, `arrow_scale`, `arrow_opacity`, `is_disp_weight`, `weight`, and webui
  `customize_*` (appeal_board, background, background_gameplay, character_p1, character_p2,
  lane_single, lane_double, lanecover_single, lanecover_double, movie_size).
