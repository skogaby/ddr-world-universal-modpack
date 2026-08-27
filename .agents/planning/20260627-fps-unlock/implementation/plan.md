# Implementation Plan — FPS Unlock

> Incremental, TDD-spirit plan (no unit-test harness in this repo → "test" = `cargo check`
> + targeted log/visual observation on deploy). Each step is `cargo check`-gated and ends
> wired-in — no orphaned code. **Sequencing per maintainer:** Part I (Enum overlay infra)
> first (low risk), then Part II (the mod), with **all cabinet testing deferred to one
> consolidated validation phase at the end** (Step 8). Full `./build.sh` only before that
> final deploy.
>
> Design: `../design/detailed-design.md`. Research: `../research/r1`–`r4`.
> Requirements: `../idea-honing.md`.

## Checklist

- [ ] **Step 1** — Part I: add `RowKind::Enum` variant + handle all match sites (inert)
- [ ] **Step 2** — Part I: `EnumRowSpec` + `register_enum_row` + adjust/render/repeat behavior
- [ ] **Step 3** — Part II: `fps_target_imm32` signature (AOB) + resolution logging
- [ ] **Step 4** — Part II: `FpsUnlockConfig` typed section + normalization + defaults
- [ ] **Step 5** — Part II: mod scaffold + registration (inert: no patch, no row yet)
- [ ] **Step 6** — Part II: `early_apply` byte-patch + stock capture + OFF/disable revert
- [ ] **Step 7** — Part II: register the `FPS TARGET` enum row + `on_change` persist + degradation/`is_active`
- [ ] **Step 8** — Consolidated cabinet validation + README/docs finalize

---

## Step 1 — `RowKind::Enum` variant + exhaustive match handling (Part I)

**Objective:** Introduce `RowKind::Enum { index: usize, values: Vec<i32>, labels: Vec<String> }`
in `src/mods/mod_menu.rs` and make every existing `match` on `RowKind` total again, with
correct (if minimal) `Enum` behavior. No public registration API yet — this step just makes
the type exist and compile cleanly everywhere.

**Guidance:**
- Add the variant to the `RowKind` enum (`:59`). Update the module doc comment that says
  "room for a future `Enum` variant" to reflect it now exists.
- Handle the compiler-flagged sites (from `research/r4`): `row_value` (return
  `values[index]`), `clone_row` (clone the vecs), `set_row_value_and_refresh` writer (set
  index from a value), `refresh_slots` render (show `labels[index]`, white), and the
  `activate_selected` adjust arm (cycle index ±1, clamped at ends; fire `on_change` with
  `values[index]`; mirror new index into the row).
- Generalize the repeat-gate (`selected_is_scalar` → e.g. `selected_repeats` / "scalar or
  enum") so a held direction cycles enum entries.
- Index resolution helper: a small fn to find the index of a value in `values` (for the
  writer + initial resolution), defaulting safely if absent.

**Test / validation:** `cargo check --target x86_64-pc-windows-msvc` clean. Since no row of
this kind is registered yet, behavior is unchanged at runtime (pure type addition).

**Integration:** Self-contained in `mod_menu.rs`. Existing Boolean/Scalar rows unaffected.

**Demo:** Code compiles with the new variant fully handled; a developer can construct a
`RowKind::Enum` row in code and the render/adjust/clone paths all handle it (verified by the
exhaustive-match compile). No user-visible change yet.

---

## Step 2 — `EnumRowSpec` + `register_enum_row` public API (Part I)

**Objective:** Expose the registration API mirroring `register_scalar_row`, so a mod can
contribute an enum row gated under a parent toggle.

**Guidance:**
- Add `EnumRowSpec { key, label, hint, parent_row_key, values, labels, initial_value,
  on_change }` (design "Components → Part I").
- `register_enum_row(spec)`: resolve `initial_value` → index (caller guarantees it's in
  `values`; clamp/fallback to 0 if not), build the `MenuRow` with `kind: Enum{..}`,
  `visible_when: parent_row_key.map(|p| (p, 1))`, push to `contributed_rows`. Mirror the
  `register_scalar_row` body (`:188`).
- Confirm `remove_rows_for` already removes by key (it does) — enum rows clean up the same
  way.
- Decide clamp-vs-wrap for cycling = **clamp** (matches Scalar). Document inline.

**Test / validation:** `cargo check` clean. Optionally (dev-only, reverted before commit)
register a throwaway enum row under an existing mod toggle to eyeball it — but not required;
Step 7 wires the real consumer.

**Integration:** Additive public fn in `mod_menu`. No caller yet (the FPS mod in Step 7 is
the first), so guard against "dead code" warnings consistently with the crate's existing
`#![allow(dead_code)]`.

**Demo:** The overlay infra can now host a labeled pick-list row via a single
`register_enum_row(..)` call; ready for any mod (FPS is first). Reusable deliverable
complete.

---

## Step 3 — `fps_target_imm32` signature (Part II)

**Objective:** Add the AOB signature for the FPS immediate and confirm it resolves.

**Guidance:**
- Add to `SIGNATURES` in `src/core/signatures.rs`:
  `name = "fps_target_imm32"`, module gamemdx, pattern
  `C7 44 24 ?? 3C 00 00 00 75 08 C7 44 24 ?? 4B 00 00 00`.
- The byte to patch (`0x3C` imm32) is at **match + 4**. If the signature framework supports
  a per-entry offset, set it to 4; otherwise the mod computes `addr + 4`. Pick whichever
  matches existing conventions (check how other patch-site signatures store their offset).
- Log the resolved address at scan time (the existing `resolve_all` summary already lists
  found/missing; a dedicated one-shot log in the mod's `early_apply` is added in Step 6).

**Test / validation:** `cargo check` clean. (Live resolution is verified at Step 8; r1
already confirmed uniqueness on all three builds statically.)

**Integration:** Signature is referenced by the mod's `required_signatures()` in Step 5.

**Demo:** `fps_target_imm32` appears in the signature table; on any future boot the scan
summary reports it found/among the located functions.

---

## Step 4 — `FpsUnlockConfig` typed config + normalization (Part II)

**Objective:** Add the `fps_unlock` config section, defaults, and the in-memory
normalization, with a clean separation between the **as-loaded** presets (for write-back)
and the **normalized** values/labels (for the row).

**Guidance:**
- In `config.rs`: add `FpsUnlockConfig { presets: Vec<i32> (default [60,120,144,165,240,360]),
  selected: i32 (default 60) }` and `pub fps_unlock: Option<FpsUnlockConfig>` on
  `ConfigFile` (mirror `timing_offsets`). Default `selected = 60` (decided — mod-on no-op).
- Normalization fn (lives in the mod, not config — config stays a dumb typed reader):
  drop entries outside a sane FPS bound (finalize, e.g. `[1, 1000]`), dedupe, sort asc; if
  empty → defaults; ensure `selected ∈ presets` (auto-add, re-sort). Produce
  `values: Vec<i32>` + `labels = values.map(|v| format!("{v}fps"))`.
- Keep the **original** presets (as read) separate, for Q9 write-back fidelity.

**Test / validation:** `cargo check` clean. Logic is pure/deterministic — reason through the
normalization cases in code review (empty list, dup, unsorted, selected-absent, all-invalid).

**Integration:** The mod (Step 5) reads `config::get().fps_unlock`, normalizes, and stores
both forms in its struct.

**Demo:** With a hand-edited `mod-config.json` (`presets:[144,60,60,100]`, `selected:240`),
the normalized in-memory result is `values=[60,100,144,240]` (240 auto-added, sorted, deduped)
— demonstrable via a one-shot log added here or in Step 5.

---

## Step 5 — `fps-unlock` mod scaffold + registration (Part II, inert)

**Objective:** Create `src/mods/fps_unlock.rs` implementing `Mod`, registered in `lib.rs`,
that loads+normalizes config but does **not** yet patch or register a row. Establishes the
mod's presence end-to-end.

**Guidance:**
- `FpsUnlockMod { patch_site: Option<PatchSite>, applied: bool, config (normalized + original
  presets), row_registered: bool }`. `unsafe impl Send`.
- `id()="fps-unlock"`, `name()="FPS Unlock"`, `description()`, `required_signatures()=
  &["fps_target_imm32"]`. `init` loads/normalizes config (Step 4) and stores it; `enable`/
  `disable` are stubs for now (log only).
- Add `pub mod fps_unlock;` to `src/mods/mod.rs`; add
  `Box::new(mods::fps_unlock::FpsUnlockMod::new())` to the `mods_to_register` vec in
  `lib.rs` (so `early_apply` will be reachable next step — placement among early-apply mods).
- Default config behavior: appears in the `mods` map / overlay as a Boolean toggle (registry
  mod) automatically.

**Test / validation:** `cargo check` clean. Mod shows up in the registry/mod-menu toggle list
(verified at Step 8, but structurally guaranteed by registration).

**Integration:** Wired into `lib.rs` registration + `early_apply` loop. Inert (no patch yet).

**Demo:** "FPS Unlock" appears as a toggleable mod in the overlay and in `mod-config.json`'s
`mods` map; toggling it persists (existing registry behavior). No FPS effect yet.

---

## Step 6 — `early_apply` byte-patch + stock capture + revert (Part II, the apply lever)

**Objective:** Make the mod actually change the FPS target: in `early_apply`, AOB-resolve the
site, capture stock, and patch to `selected` when enabled. This is the load-bearing,
race-critical core.

**Guidance:**
- Implement `early_apply(ctx)`: get `fps_target_imm32` addr from `ctx.signatures`; compute
  `imm_addr = addr + 4`; read 4 bytes (`stock`), validate `stock[0]==0x3C` (warn+abort if
  not); store `PatchSite{imm_addr, stock}`. If the mod is enabled in config AND
  `selected != stock_value`, `memory::protect` + write `selected as u32` LE at `imm_addr`;
  set `applied=true`. If disabled, capture only (no write). Return `false` only on
  resolve/validation failure.
- One-shot INFO log: `fps-unlock: site @ {imm_addr:p}, stock={stock_fps}, selected={selected},
  patched={bool}` — this is the line the Step-8 deploy looks for (boot-race confirmation).
- `init` no-ops the patch when `applied` (mirror `song_limit_expansion`'s early_applied flag).
- `disable`: revert `imm_addr` to `stock` (panic-free, `protect`+write). `is_active()` returns
  `patch_site.is_some()` (self-disable rendering, mirroring timing-offsets).
- Keep all memory writes panic-free; scope `unsafe` narrowly.

**Test / validation:** `cargo check` clean. (Functional confirmation — does the patch land
before `onBoot`, does the refresh rate change — is the Step-8 deploy; that's the one
empirical risk per r2. Fallback ladder in the design if it loses the race.)

**Integration:** Uses the Step-3 signature + Step-4 config. The mod now has full config-only
capability (the feature is functionally complete here, sans overlay row).

**Demo (deferred to Step 8 deploy):** With `fps_unlock.selected=144` + mod enabled, boot log
shows the patch line and the cabinet renders at 144 / smoother scroll. (Code-complete + check
clean is the per-step gate.)

---

## Step 7 — Register the `FPS TARGET` enum row + persistence + degradation (Part II)

**Objective:** Wire Part I to Part II: in `enable()`, register the enum row (optional tier);
on change, persist and update state. Complete the two-tier degradation.

**Guidance:**
- In `enable()`: build `values`/`labels` from normalized config; `register_enum_row(EnumRowSpec{
  key:"fps-target", label:"FPS Target", hint:"Display refresh target.",
  parent_row_key:Some("fps-unlock"), values, labels, initial_value:selected, on_change })`.
  Set `row_registered` on success; if registration is unavailable, log and continue
  (config-only fallback — Q7 tier 2).
- `on_change(value)`: validate `value ∈ presets`; update `config.selected`; persist via
  `save_json_key("fps_unlock", json!({ "presets": <ORIGINAL presets as-loaded>, "selected":
  value }))` (Q9 — preserve operator's array, only change selected). One-shot log: "applies on
  next launch". (Optionally also write the live imm32 — but it's already latched in the device,
  so it has no effect this session; persisting + next-launch is the contract.)
- `disable()`: `remove_rows_for(&["fps-target"])` + the Step-6 stock revert.
- Confirm `is_active()` (Step 6) drives the master toggle's `[ON]/[OFF]` correctly and the
  child row hides when the master is OFF (via `visible_when`).

**Test / validation:** `cargo check` clean. Walk through degradation branches in review
(AOB-miss → mod skipped; row-register fail → config-only).

**Integration:** Full feature assembled: master toggle → child enum row → pick value →
persist → applies next launch. No orphaned code.

**Demo (deferred to Step 8):** Triple-0 overlay → enable FPS Unlock → `FPS TARGET` row appears
→ cycle `60fps…360fps` → selection persists to `mod-config.json` → next launch renders at the
chosen rate. Row hidden when master OFF.

---

## Step 8 — Consolidated cabinet validation + docs finalize

**Objective:** Single end-to-end validation pass on the cabinet (all testing deferred here per
maintainer), then finalize user docs.

**Guidance — build & deploy:** `./build.sh` (and `./build_win7.sh` if applicable), then
`./scripts/deploy.sh`. Validate the matrix:

1. **Boot-race + apply (the key risk, r2):** mod on, `selected=144` → boot log shows
   `fps-unlock: site @ …, patched=true` **before** render init; cabinet runs at 144 (smoother
   scroll). If the patch is observed not to take effect → invoke the r2 fallback ladder
   (detour `FUN_1801eda10`, then last-resort on-disk) and note in summary.
2. **Menu-speedup check (settles r3 empirically):** at 144/240, confirm menu/selection
   animations run at **normal** wall-clock speed (expected per r3 + the live test). If they
   DO speed up → Milestone 2 reconsideration (separate effort; document, don't fix here).
3. **Overlay enum:** cycle presets, confirm `Nfps` labels, hold-to-repeat, hidden-when-OFF,
   persistence across restart.
4. **OFF/disable:** toggle off → next launch renders stock 60.
5. **Oddball preset:** add `100` to config `presets` → appears in picker; `selected` write-back
   leaves `presets` array as authored.
6. **Self-disable:** (only checkable if the AOB ever fails — confirmed resolving in r1, so
   expected ON).

**Guidance — docs:**
- Update `README.md` Included Mods table (add **FPS Unlock**) + a config section for
  `fps_unlock` (presets/selected, "applies on next launch", how to add oddball rates).
- Update `AGENTS.md` config notes + the mod list if it enumerates mods.
- Mark `docs/hex_edit_porting.md` Hack 5 as IMPLEMENTED, and **correct the two RE errors**
  found this session (target at struct +0x1C not +0x14; real consumer chain
  `FUN_1801eda10`→`Renderer:initGs`, not `FUN_1801f0030`). Note Milestone-2 dropped + why.
- Consider an `.agents/summary/` refresh entry (components.md mod table) — or leave to the
  summary regen; note it.

**Test / validation:** The deploy matrix above IS the validation. Capture observed results in
the feature `summary.md` (and a `progress.md` only if the work spans sessions — per the repo's
convention that progress.md is a multi-session handoff aid, not required for single-session
completion).

**Integration:** Feature complete and documented. Commit/push on maintainer request
(solo-maintainer repo).

**Demo:** End-to-end on the cabinet: enable FPS Unlock, pick `144fps` in the overlay, restart,
gameplay runs smooth at 144 with normal-speed menus; config persists; toggling off reverts to
60 next launch.

---

## Notes on sequencing & risk

- **Enum-first (Steps 1–2)** is deliberately low-risk pure-Rust overlay work, fully decoupled
  from the binary patch. If the FPS lever ever needed rework, the Enum infra still stands as a
  reusable deliverable.
- **The single real risk** is the Step-6 boot-race (does `early_apply` beat `onBoot`?),
  intentionally validated in the one consolidated Step-8 deploy. Fallback ladder is documented
  in the design (r2) so a failed race isn't a dead end.
- **No mid-stream deploys** (per maintainer) — every step is `cargo check`-gated and left
  wired-in so the first and only cabinet test exercises the whole feature.
