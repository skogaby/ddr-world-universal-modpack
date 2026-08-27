# Idea Honing — Custom Options Row Order

Requirements clarification. Per the user's direction, each question carries a **proposed
default answer** derived from the spirit of the request; the user reviewed the whole list
and refuted any they disagreed with. **Status: all confirmed.** The only change from the
initial defaults was Q2 (id matching is case-insensitive).

---

### Q1. Config shape and location
Where does `row_order` live and what is its type?

**Answer (CONFIRMED):** A new field `row_order: Option<Vec<String>>` on
`CustomOptionsConfig` (the `custom_options` section of `mod-config.json`), with
`#[serde(default)]`. Absent **or** empty array → no reordering (identity =
current registration order). Operator-authored only; the DLL never writes it (existing
read-modify-write save paths already preserve unknown/sibling keys).

---

### Q2. What strings go in the array?
What identifies a row?

**Answer (CONFIRMED):** Each string is a registered option's `id` — the same
`RegisterSpec.id` used everywhere (e.g. `premium_free`, `autoplay`, `center_arrows_1p`,
`arrow_scale`, `overlay_opacity`, `customize_background`, `customize_lanecover_single`,
`is_disp_weight`, `weight`, `timing_stats`, `pacemaker_threshold`, ...). Matched
**case-insensitively** (ASCII case-fold — ids are kbin-valid snake_case, so
`eq_ignore_ascii_case` / lowercasing both sides is sufficient) to cover operator typos in
casing. No whitespace trimming (only case is normalized).

---

### Q3. Scope — which rows are reordered?
Does this affect only the modpack's custom rows, or native game options too?

**Answer (CONFIRMED):** Only the modpack's **custom** option rows on the MODS tab
(page 6). The framework only injects (and appends after) its own rows, so native game
option rows on other tabs are untouched. The DLL overlay **mod menu** (triple-0) is a
separate system and is **not** affected.

---

### Q4. Placement of options NOT listed in `row_order`
(Explicit in the request.)

**Answer (CONFIRMED):** Appended to the **end** of the list, after
all listed options, preserving their current **registration order** among themselves.

---

### Q5. Handling of an id that matches no registered option
(Explicit in the request.)

**Answer (CONFIRMED):** Log a **warning** and ignore
that entry; never fatal. Detail: the warning is emitted **once** (latched), not on every
menu open, and the message is worded softly (e.g. "no registered option `X` — ignoring;
it may be a typo or belong to a disabled mod") because a disabled mod / absent-asset id
is indistinguishable from a typo at runtime.

---

### Q6. Duplicate ids within `row_order`
e.g. `["autoplay", "premium_free", "autoplay"]`.

**Answer (CONFIRMED):** First occurrence wins (row placed at its first position);
later duplicates are ignored. No warning (or at most a `log_debug!`), since it's harmless.

---

### Q7. Parent/child (`ShowWhen`) adjacency
`weight` is a child of `is_disp_weight` (shown only when the parent is ON);
`pacemaker_threshold` similarly depends on `pacemaker_to_mserror`. Should the framework
force a child to stay glued after its parent regardless of `row_order`?

**Answer (CONFIRMED):** **No magic adjacency** — honor the array literally. If the
operator lists a child before/away from its parent, it displays there; visibility still
works correctly (it's resolved by id, independent of order). Documented as a known
characteristic. (Rationale: matches the request's "order is exactly as listed" spirit and
keeps the rule dead simple. A child whose parent is unlisted still falls to the end per
Q4, independently of the parent.)

---

### Q8. Per-player vs cabinet-wide
Is `row_order` one list for the whole cabinet, or per P1/P2?

**Answer (CONFIRMED):** A **single cabinet-wide** list (one array), applied
identically to both player sides. It's an operator layout preference, not per-player
state — so it sits directly under `custom_options` (a sibling of `p1`/`p2`), not inside
them.

---

### Q9. When does a change take effect?
Live-editable, or relaunch?

**Answer (CONFIRMED):** Read **once at boot** (config is loaded into a `OnceCell`),
cached in the service. Applied on **every menu open** within the session using the
boot-time value. Editing `mod-config.json` requires a **relaunch** to take effect —
consistent with every other config knob (`preview_window`, `persist_*`, etc.). Not
exposed as a live-editable mod-menu row (it's a list of strings, no UI affordance).

---

### Q10. Implementation locus
Where does the logic live?

**Answer (CONFIRMED):** In the `custom_options` service:
1. `custom_options::init()` reads `config::get()...row_order` and stores it in a static
   (empty/absent → identity).
2. A small pure ordering helper computes the display permutation of option indices from
   `(configured_order, registered ids)`, emitting the warn-once for unknown ids.
3. `builder_hook::builder_detour_body` applies that permutation to its `handles`
   snapshot before injecting rows — the one and only behavioral change. `OptionHandle`
   indices and the `options` Vec are untouched, so handles stay valid and ROWS/scroll
   order follow automatically.

---

### Q11. Documentation deliverables

**Answer (CONFIRMED):** Update `README.md` (custom_options config section — add
`row_order` with the current id list and a note that webui ids depend on enabled mods /
discovered assets), `AGENTS.md` (custom-options entry + config section), and the summary
docs (`data_models.md` `CustomOptionsConfig`, `interfaces.md` config schema). Add the
feature's `progress.md` per the repo's PDD convention.

---

### Q12. Testing / validation

**Answer (CONFIRMED):** No unit tests (repo convention — validation is live deploy
+ log observation). The pure ordering helper is written to be trivially reviewable. The
build gate is `cargo check` → `cargo fmt` → `./build.sh`; functional validation is a
cabinet deploy: verify a configured order renders in that order on the MODS tab (both
sides), an unlisted option lands at the end, and a bogus id logs one warning without
breaking the menu.
