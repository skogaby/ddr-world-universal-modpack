# Implementation Plan — Custom Options Row Order

Single-pass implementation (no interim demoable increments, per direction). Build after
the code lands, then validate on-cabinet. Test-driven is adapted to this repo's reality:
no unit-test harness — the pure `compute_order` helper is kept trivially reviewable, and
correctness is proven by the build gates plus the on-cabinet validation matrix in Step 6.

Reference docs (assumed in context during implementation):
- `design/detailed-design.md` — full design, code sketches, rules.
- `research/existing-mechanism.md` — where/why row order comes from today.
- `idea-honing.md` — the 12 confirmed requirement decisions.

## Checklist

- [ ] **Step 1 — Config field.** Add `row_order: Option<Vec<String>>` to `CustomOptionsConfig`.
- [ ] **Step 2 — Ordering module.** Add `src/services/custom_options/ordering.rs` (config store + pure `compute_order` + warn-once `display_order_for`); register `pub mod ordering;`.
- [ ] **Step 3 — Init wiring.** Read `custom_options.row_order` in `custom_options::init()` → `ordering::set_configured_order(...)`.
- [ ] **Step 4 — builder_hook wiring.** Reorder the `handles` snapshot via `ordering::display_order_for(&ids)` before injection.
- [ ] **Step 5 — Docs.** Update README, AGENTS.md, and summary docs (`data_models.md`, `interfaces.md`).
- [ ] **Step 6 — Build gates + on-cabinet validation.** `cargo check` → `cargo fmt` → `./build.sh`; then deploy and run the validation matrix.

Maintain `progress.md` throughout (repo convention): update after each step, before any
pause/handoff.

---

## Step 1: Add the `row_order` config field

**Objective.** Make `custom_options.row_order` a first-class, optional config value.

**Implementation.**
- In `src/mods/config.rs`, add to `struct CustomOptionsConfig`:
  ```rust
  /// Operator-defined display order for the modpack's custom option rows on the
  /// MODS tab. Each entry is an option id (case-insensitive). Listed ids render
  /// first in this order; any registered option not listed falls to the end
  /// (keeping registration order); any entry matching no registered option is
  /// logged once and ignored. Absent or empty => current registration order.
  #[serde(default)]
  pub row_order: Option<Vec<String>>,
  ```
- No changes to any writer — `save_mod_states`, `save_custom_options_values`,
  `save_json_key`, and the migration all read-modify-write named keys only, so a
  hand-authored `row_order` is preserved.

**Integration.** Consumed in Step 3.

**Validation for this step.** `cargo check` passes (field parses; absent ⇒ `None`).

---

## Step 2: Add the `ordering` module

**Objective.** Own the configured order and the permutation logic in one reviewable place.

**Implementation.**
- Create `src/services/custom_options/ordering.rs` per the design's code sketch:
  - `static CONFIGURED_ORDER: OnceCell<Vec<String>>` (ids ASCII-lowercased at store time).
  - `static UNKNOWN_WARNED: AtomicBool` (warn-once latch).
  - `pub(crate) fn set_configured_order(order: Vec<String>)` — lowercases + stores; empty
    stored as-is (treated as identity downstream).
  - `fn compute_order(registered: &[&str], configured: &[String]) -> (Vec<usize>, Vec<String>)`
    — **pure**: listed-first (dedup, first-wins) → unlisted appended in registration order;
    returns `(permutation, unknown_ids)`. Match via `id.eq_ignore_ascii_case(want)`.
  - `pub(crate) fn display_order_for(ids: &[&str]) -> Vec<usize>` — identity fast-path when
    unconfigured/empty; else `compute_order`, then warn **once** (sorted+deduped unknowns)
    via `log_warn!`, return the permutation.
- In `src/services/custom_options/mod.rs`, add `pub mod ordering;` with the other
  submodule declarations.

**Integration.** Called from Steps 3 (`set_configured_order`) and 4 (`display_order_for`).

**Validation for this step.** `cargo check` passes; re-read `compute_order` against the four
rules (Q4/Q5/Q6 + case-insensitive Q2) to confirm each branch.

---

## Step 3: Read config at service init

**Objective.** Plumb the operator's `row_order` into the ordering module once at boot.

**Implementation.**
- In `custom_options::init()` (`mod.rs`), after the existing sub-inits, add:
  ```rust
  let configured = crate::mods::config::get()
      .and_then(|c| c.custom_options.as_ref())
      .and_then(|c| c.row_order.clone())
      .unwrap_or_default();
  ordering::set_configured_order(configured);
  ```
- (No new layering concern: `custom_options_persistence` already reads
  `crate::mods::config`; config is loaded well before service init.)

**Integration.** `CONFIGURED_ORDER` is now populated before any MODS-tab open.

**Validation for this step.** `cargo check` passes; a boot log line is not required, but the
value is available to Step 4.

---

## Step 4: Apply the permutation in `builder_hook`

**Objective.** Inject rows in the configured display order — the sole behavioral change.

**Implementation.**
- In `builder_hook::builder_detour_body`, immediately after the existing `handles`
  snapshot is built (registration order; `handles[i].0 == OptionHandle(i)`), insert:
  ```rust
  // Reorder per the operator's configured row_order (identity if unconfigured).
  let handles: Vec<(OptionHandle, String, RowKindTag)> = {
      let ids: Vec<&str> = handles.iter().map(|(_, id, _)| id.as_str()).collect();
      let perm = super::ordering::display_order_for(&ids);
      perm.into_iter().map(|i| handles[i].clone()).collect()
  };
  ```
- Leave the `clear_side` + allocate/register loop unchanged; it now walks `handles` in
  display order, so scene-graph order and `rows::ROWS` order both follow.
- No edits to `rows.rs`, `filter_hook.rs`, `dtor_hook.rs`, or `options_scroll`.

**Integration.** End-to-end: config → static → injection order → visual + scroll order.

**Validation for this step.** `cargo check` passes. (Functional proof deferred to Step 6.)

---

## Step 5: Documentation

**Objective.** Make `row_order` discoverable and record the current id universe.

**Implementation.**
- **`README.md`** — in the `custom_options` config section: add `row_order` to the complete
  example, and a short subsection describing the rules (listed-first, unlisted-to-end,
  unknown ⇒ warn+ignore, case-insensitive, cabinet-wide, relaunch-to-apply). Include the
  current id list, noting webui ids depend on enabled mods / discovered assets.
- **`AGENTS.md`** — add a `row_order` note to the `custom_options` bullet under Config, and
  (optionally) a one-line pointer in the entry-points table.
- **Summary docs** — `.agents/summary/data_models.md` (`CustomOptionsConfig` struct +
  field note) and `.agents/summary/interfaces.md` (config schema block) get `row_order`.

**Integration.** Docs match shipped behavior.

**Validation for this step.** Prose review; example JSON is valid.

---

## Step 6: Build gates + on-cabinet validation

**Objective.** Prove the whole feature end-to-end.

**Implementation / procedure.**
1. Build gates (AGENTS.md order): `cargo check --target x86_64-pc-windows-msvc` →
   `cargo fmt` (whole crate) → `./build.sh` clean.
2. Deploy (`./scripts/deploy.sh`) and run the validation matrix, watching the MODS tab and
   the `[DDR-Hook]` log:
   - **No `row_order`** ⇒ rows in current (registration) order (zero-change default).
   - **Partial order** ⇒ listed ids lead in order; the rest follow in registration order.
   - **Full order** ⇒ exact order on both P1 and P2.
   - **Bogus id** ⇒ menu normal, exactly **one** WARN naming the bogus id.
   - **Duplicate id** ⇒ placed once, no misbehavior.
   - **Case variance** (e.g. `Premium_Free`) ⇒ matches `premium_free`.
   - **Parent/child** (`weight` before `is_disp_weight`) ⇒ `weight` renders where listed;
     toggling the parent still shows/hides it correctly.

**Integration.** Feature complete and validated.

**Validation for this step.** All matrix rows pass; build is clean.

---

## Notes / risks

- **Blast radius is tiny** and default-preserving: with no `row_order`, `display_order_for`
  returns identity and the injection loop is byte-for-byte as today.
- **Malformed JSON type** for `row_order` (e.g. a string, or array of non-strings) triggers
  the pre-existing whole-file config fallback (all `custom_options` revert to defaults for
  that boot, with the existing parse WARN). Out of scope to change; recorded as Alt C in the
  design.
- **`ShowWhen` children** can be stranded from their parent by the operator's ordering;
  visibility is unaffected (id-resolved). Documented as a known characteristic.
