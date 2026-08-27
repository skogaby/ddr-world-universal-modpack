# Implementation Plan — Timing Offsets

Each step yields a working, demoable increment, builds on the prior step, and ends wired in.
No unit-test harness (CLAUDE.md): "test requirements" = `cargo check --target
x86_64-pc-windows-msvc` after every change, plus the **diagnostic-build-then-deploy**
discipline (ship one-shot logs, observe in spice2x log / DebugView, then proceed). Full
`./build.sh` before any deploy; the maintainer deploys and reports back.

**Sequencing rationale.** The mod's *core value* (offsets actually apply) is delivered by the
setter hook + config seed (Part II) and is fully demoable via `mod-config.json` **before** any
overlay UI exists. So Steps 1–5 ship the working feature config-only; Steps 6–9 add the
overlay infrastructure (Part I) and the in-game scalar rows; Step 10 is the cabinet matrix.
This front-loads the load-bearing, RE-derived risk (the apply path) and treats the overlay
upgrade — the larger but lower-risk codebase work — as a clean second phase. Part I is built
as **extensible-but-not-speculative** infra (a `RowKind` enum with `Boolean`+`Scalar` now,
evolvable to `Enum` later).

## Checklist

- [x] **Step 1:** Setter resolution — DONE. R1 prologue AOB found NON-unique (matches the setter AND a byte-identical twin map-setter); corrected to a **derived** resolution: added landmark sig `timing_set_call_landmark` (publisher's SOUND/INPUT/RENDER/BOMB set-pairs) + `derive_timing_config_setter()` decoding the CALL at landmark+0xA → `timing_config_set_int`. Verified via Ghidra `search_byte_patterns`: landmark matches only inside the publisher on both builds; derived setter = `0x1801acbf0` (20260324) / `0x1801ae460` (20260526). `cargo check` clean (with `RUSTUP_AUTO_INSTALL=0`). NOTE: `scripts/aob_check.py` needs raw DLL files (not available locally) — Ghidra search used instead.
- [x] **Step 2:** Scaffold — DONE. `src/mods/timing_offsets.rs` (`TimingOffsetsMod`, id `timing-offsets`); resolves `timing_config_set_int` in `init` (warns if missing), logs in `enable`/`disable`. Declared in `mods/mod.rs`, registered in `lib.rs`. Defined `FIELD_KEYS`/`FIELD_DEFAULTS`/`FIELD_COUNT` constants for later steps. `cargo check` clean.
- [x] **Step 3:** Setter detour + key-match diagnostic — DONE + VALIDATED on cabinet (Build 2). `GenericDetour<SetIntFn>`, FNV-1a key match, panic-guarded, self-disable if unresolved. Hook confirmed firing at boot.
- [x] **Step 4:** Substitute + capture stock + config load/seed — DONE + VALIDATED. Typed `TimingOffsetsConfig` in `ConfigFile`; `Mutex<TimingState>{configured,stock,master_on}`; hook captures stock on first write + substitutes when master ON. Cabinet confirmed: `incoming=87 -> forward=999` etc.; stock-capture correctly grabbed preset-1 live values (RENDER=36/BOMB=1).
- [x] **Step 5:** Live push + revert + persist — DONE + VALIDATED. `set_offset`/`get_offset` (pub), `push_to_map`/`push_all_configured`/`push_all_stock`, `persist_all`. **Plus a crash fix** (Build 1→2): added `MAP_READY` atomic latched by the first hook hit; all live setter pushes gate on it (can't call the setter before the game builds the config-map global — see learnings corollary). Build 2 ran clean on cabinet, no crash. **Config-only feature complete + validated.**
- [x] **Step 6:** Generalize `mod_menu` rows — DONE (`cargo check` + `./build.sh` clean). Added `RowKind {Boolean, Scalar}` + `MenuRow {key,label,hint,kind,indent,visible_when,on_change}`; state now holds `rows`/`contributed_rows`/`visible_rows`; `rebuild_rows`/`rebuild_visible` (registry mods → `Boolean` rows); nav over `visible_rows`; `activate_selected` dispatches by kind (Scalar arm no-op until Step 7); `toggle_registry_mod` preserves the exact toggle+persist behavior; `refresh_slots` renders by kind with indent. Pure refactor — booleans behave identically. NOT yet cabinet-tested (deferred to the combined Step 7/9 overlay test).
- [x] **Step 7:** Cabinet menu-button nav + Start-held coarse adjust — DONE (`cargo check` clean). `activate_selected` Scalar arm: Left/Right ±step (clamped), Start-held = coarse via new `coarse_held()` (`get_button_state(P1)|P2 & START`), drives `on_change` then mirrors via `set_row_value_and_refresh`. Cabinet menu buttons already primary nav (handle_exclusive_input matches MENU_* + 2/4/6/8 aliases); updated instructions text. NOT yet visibly tested (needs scalar rows from Step 9).
- [x] **Step 8:** Menu-button suppression — DONE (`cargo check` clean). Added 5 `GenericDetour<TriggerHoldFn>` statics + `menu_button_detour_body` (zeros trigger/hold for game-side callers when `IS_INPUT_SUPPRESSED && !IN_MODPACK_POLL`) via a `menu_button_detour!` macro; `install_menu_button_detours([5 getters])` called in `init` (best-effort per-button). Wrapped the modpack's own menu-button poll loop in `IN_MODPACK_POLL=true` so its reads bypass suppression. Mirrors the get_10key detour. Runtime checkpoint (does it actually block game input) deferred to overlay deploy.
- [x] **Step 9:** Row-registration API + parent/child + timing rows — DONE (`cargo check` + `./build.sh` clean). Added `mod_menu::{register_scalar_row, set_scalar_value, remove_rows_for}` + `ScalarRowSpec`; contributed rows merged after registry rows, `visible_when=(parent,1)` gating. Timing mod registers 4 scalar rows (`register_overlay_rows`) in `enable` under parent `timing-offsets` (labels/hints/ranges per design+R3), removes them in `disable`. Per-field `on_change` shims → `set_offset(idx,v)`. Also: `toggle_registry_mod` now calls `rebuild_rows` (not just `rebuild_visible`) so a mod's newly-registered rows appear immediately on toggle. AWAITING OVERLAY DEPLOY (validates Steps 6/7/8/9 together).
- [x] **Step 10:** Validation + finalize — DONE. Feature validated live on cabinet (offsets apply, parent/child rows, button suppression, hold-to-repeat all confirmed by maintainer). Finalize: reworded the one-shot boot log to `<KEY> stock=.. applied=..` (operational, not debug); confirmed no `println!`/`FUN_`/abs-addr in the new src (timing_offsets.rs, mod_menu.rs, input_manager.rs additions — described by role). Docs updated: README (mod table, config example + `timing_offsets` section, src tree, Mod Menu entry); summary `components.md` (timing_offsets mod + rewritten mod_menu + input_manager suppression), `architecture.md` (mod table), `interfaces.md` (config schema + Mod Menu Overlay API). Learnings: added the MAP_READY/init-thread corollary. `cargo check` clean.

---

## Step 1: Add and verify the setter AOB signature

**Objective:** Define `timing_config_set_int` in `src/core/signatures.rs` resolving to the
config-map int setter (`FUN_1801acbf0` / `FUN_1801ae460`), exactly one site on both builds.

**Guidance:** Use the R1 pattern
`48 89 7C 24 10 4C 8B C9 48 83 C9 FF 33 C0 49 8B F9 44 8B D2 41 B8 C5 9D 1C 81`. The `44 8B D2`
distinguishes the setter from the identical-prologue getter (`4C 8B D2`); the `41 B8 C5 9D 1C
81` is the FNV-1a seed. Follow existing `SignatureDefinition` style; describe by role, no
`FUN_`/addresses in the description beyond what other entries do.

**Test requirements:** `cargo check`. Validate uniqueness with `scripts/aob_check.py` (or
equivalent) against both DLLs — must match **count == 1** on 20260324 and 20260526. Confirm it
does NOT also match the getter.

**Integration:** Signature available via `ctx.signatures.get_address("timing_config_set_int")`.

**Demo:** Init resolve-count log shows `timing_config_set_int` resolved on both builds.

---

## Step 2: Scaffold the `timing-offsets` mod (trait, registration, inert)

**Objective:** Create `src/mods/timing_offsets.rs` with `TimingOffsetsMod` implementing `Mod`
(id `timing-offsets`, name "Timing Offsets", description per design, `required_signatures()`
returns `&[]`). `init`/`enable`/`disable` just log. Add `pub mod timing_offsets;` to
`src/mods/mod.rs` and a `reg.register(Box::new(...))` entry to `src/lib.rs`.

**Guidance:** Model lifecycle on `center_arrows_single.rs` (best-effort resolve in `init`,
self-disable in `enable`). No hook yet.

**Test requirements:** `cargo check`; deploy; confirm the mod registers (init log) and appears
toggleable in the mod menu without affecting anything.

**Integration:** Mod is part of the registry + config (`mods.timing-offsets`).

**Demo:** "Timing Offsets" appears in the mod menu; logs enable/disable; game unchanged.

---

## Step 3: Setter detour + key-match diagnostic (no substitution)

**Objective:** Resolve `timing_config_set_int` in `init`; in `enable`, if unresolved →
self-disable (load-bearing failure). Install a `GenericDetour` on the setter. The callback
matches the key arg against the four offset names (FNV-1a hash compare, or `strcmp`), logs
`{key, value}` once per key, and **calls the original unchanged** (no substitution yet).
Precompute `KEY_HASHES` at init.

**Guidance:** `catch_unwind`-guard the callback; null-check the key pointer; no allocation;
forward to original in all paths. Use `Mutex<TimingState>` for state (per design), but at this
step state is just the captured-key log.

**Test requirements:** `cargo check`; `./build.sh`; deploy diagnostic. Confirm at boot the four
keys flow through with the stock values (SOUND 87, INPUT 28, RENDER 17, BOMB 0), proving the
hook is installed before the boot publisher runs. Verify on the cabinet (20260526). **Load-
bearing checkpoint** — do not proceed until the hook is confirmed firing at boot.

**Integration:** The single apply-lever detour is live (observing only).

**Demo:** Logs show the boot publisher writing all four offsets through our hook with stock
values, on the cabinet build.

---

## Step 4: Substitute values + capture stock + config load/seed

**Objective:** Add the typed `TimingOffsetsConfig` section to `ConfigFile`
(`src/mods/config.rs`) with the four integer keys (serde defaults 87/28/17/0). In `init`/
`enable`, load it into `TimingState.configured` (clamped to `[-1000,1000]`). In the setter
hook: capture `stock[idx]` on the first observed write per key; when `master_on`, substitute
`clamp(configured[idx])` before forwarding.

**Guidance:** `master_on` is set true when the mod is enabled (Step 2 lifecycle). Stock capture
only records the *first* write per key (the boot publish), so later pushes don't clobber it.
Index order `[SOUND, INPUT, RENDER, BOMB]` fixed.

**Test requirements:** `cargo check`; `./build.sh`; deploy. Set a deliberately large
`sound_offset` (e.g. 400) in `mod-config.json`, boot, enter a song → audio audibly late
(GamePlayActor latches our value, per R2). Set back to default → normal. This proves the
config-seeded feature end-to-end **with no overlay UI yet**.

**Integration:** The feature works headlessly via `mod-config.json` — the load-bearing core is
complete.

**Demo:** Editing `timing_offsets.sound_offset` in the config file changes audio sync in the
next song; the other three fields likewise take effect.

---

## Step 5: Live push on change, master-OFF revert, immediate persist

**Objective:** Add the apply/persist plumbing the overlay will later drive: a public
`set_offset(idx, value)` on the mod that clamps, stores `configured[idx]`, calls the **original
setter** to push the value live, and persists the whole `timing_offsets` object via
`config::save_json_key`. Wire `disable()` to revert: push each `stock[idx]` (or default
87/28/17/0 if uncaptured) and remove the detour. Wire `enable()` to push all configured values
live after installing the hook.

**Guidance:** The live push calls the detour's `.call()` (original), not the hooked entry — no
recursion. Master ON (enable) pushes configured; master OFF (disable) pushes stock. All latch
next song (R2) — log/document this. Add `config::save_json_key` usage (already exists).

**Test requirements:** `cargo check`; `./build.sh`; deploy. Toggle the mod off in the mod menu
→ next song reverts to stock timing. Toggle on → configured values. Confirm the `timing_offsets`
JSON block is written on change and survives reboot. (Overlay scalar UI still absent — drive via
mod-menu master toggle + config edits.)

**Integration:** Config-only feature is **complete**: enable/disable reverts correctly, values
persist, changes apply next song. Everything below is the overlay UX layer.

**Demo:** Enabling/disabling "Timing Offsets" in the mod menu applies/reverts the configured
offsets on the next song; values persist across reboots.

---

## Step 6: Generalize `mod_menu` rows to `RowKind {Boolean, Scalar}`

**Objective:** Refactor `mod_menu` from a flat mod-list to a list of typed `MenuRow`s
(`RowKind::Boolean | Scalar`, plus `key`, `label`, `hint`, `indent`, `visible_when`,
`on_change` per design). The existing registry mods become `Boolean` rows whose `on_change`
toggles the mod (existing `toggle_callback` + `save_mod_states`). Rendering: `[ON]/[OFF]` for
Boolean (unchanged), signed integer for Scalar.

**Guidance:** Keep all behavior identical for the existing mod toggles — this is a pure
refactor that introduces the row abstraction without changing UX yet. No scalar rows exist in
the list yet (Step 9 adds them). Preserve the render-thread discipline (mutate widgets only in
`run_on_render_thread`, don't hold the state lock across a schedule).

**Test requirements:** `cargo check`; `./build.sh`; deploy. The mod menu looks and behaves
**exactly as before** (navigate, toggle mods, persist). Pure-refactor regression check.

**Integration:** The overlay now has a typed row model ready for scalar rows.

**Demo:** Mod menu is unchanged to the user, but internally every row is a `MenuRow`.

---

## Step 7: Cabinet menu-button nav primary + Start-held coarse adjust

**Objective:** In `handle_exclusive_input`, make cabinet `MENU_UP/DOWN/LEFT/RIGHT` the primary
nav (keep `2/4/6/8` as alias). For a selected `Scalar` row, Left/Right adjusts by `step_coarse`
when `input_manager::get_button_state(side) & START` (read both sides, OR the START bit), else
`step_fine`, clamped; fires `on_change`. For `Boolean`, Left/Right toggles as today. (No scalar
rows in the list yet — exercise via a temporary test scalar row or defer the live test to
Step 9.)

**Guidance:** Add a tiny helper `coarse_held() -> bool` reading `get_button_state` for both
players. Numpad `4/6` stays fine-only. Open/close remains triple-0.

**Test requirements:** `cargo check`; `./build.sh`. If exercising now, temporarily add a throwaway
scalar row to confirm fine/coarse adjust; otherwise verify compile + logic and defer the visible
test to Step 9. Existing boolean toggles must still work.

**Integration:** The overlay can adjust scalar values with fine/coarse steps via cabinet
buttons.

**Demo:** A scalar row's value changes by 1 on Left/Right and by 20 with Start held (shown via
the temporary row or at Step 9).

---

## Step 8: Game-side suppression of the five menu-button exports

**Objective:** In `input_manager`, install `GenericDetour`s on `arkMDXGetStart/Up/Down/Left/
Right` (signature `fn(i32,*mut u32,*mut u32)`), mirroring the existing `get_10key_detour`: each
calls the original, then if `IS_INPUT_SUPPRESSED && !IN_MODPACK_POLL`, zeroes `*trigger` and
`*hold` for the game-side caller. Keep handles alongside `GET_10KEY_DETOUR`.

**Guidance:** `catch_unwind`-guard each detour; null-check out-params before zeroing. No API
change — `mod_menu::open/close` already toggles `set_input_suppressed`. On any install failure,
log and leave that button un-suppressed (degraded, not fatal).

**Test requirements:** `cargo check`; `./build.sh`; deploy. With the overlay open, mash
Start/Up/Down/Left/Right → **no** game-side movement/credit/effect underneath (compare to the
numpad's existing suppression). A one-shot log confirms suppression active. **Runtime checkpoint**
(the one thing static analysis couldn't guarantee, per R5) — if buttons still bleed through, ask
the maintainer to load arkmdxbio2 into Ghidra to investigate the input path.

**Integration:** Cabinet-button overlay nav no longer leaks into the game.

**Demo:** Overlay open + cabinet buttons mashed → overlay navigates, game underneath ignores
them entirely.

---

## Step 9: `mod_menu` row-registration API + parent/child visibility; timing mod registers rows

**Objective:** Add the `mod_menu` registration API (`register_scalar_row(spec)`,
`set_scalar_value(key, value)`, `remove_rows_for(mod_id)`) and parent/child `visible_when`
filtering (drop child rows whose parent row value doesn't match when building the visible list).
In the timing mod's `enable()`, **best-effort** register the four scalar child rows
(`sound_offset`/`input_offset`/`render_offset`/`bomb_frame_offset`, indented under
`timing-offsets`, `visible_when=("timing-offsets",1)`, ranges/steps from design, hints from R3),
wiring each `on_change` to `set_offset` (Step 5). `remove_rows_for` in `disable()`.

**Guidance:** Visibility filter runs each refresh before the scroll/cursor math (so collapsing
children Just Works). If registration fails / `mod_menu` unavailable → log, continue (config-only
mode; the mod still applies values). `set_scalar_value` lets the mod reflect config/boot values
into the row display.

**Test requirements:** `cargo check`; `./build.sh`; deploy. Open overlay: with "Timing Offsets"
OFF, no child rows; turn it ON → four indented scalar rows appear with current values + hints;
Up/Down navigates into them; Left/Right adjusts (fine), Start-held adjusts (coarse); values
persist + apply next song; turning master OFF hides them and reverts. Confirm degradation: if
rows can't register, config-only still works.

**Integration:** Full in-overlay UX wired to the working apply path — the complete feature.

**Demo:** In the overlay, enabling Timing Offsets reveals four adjustable scalar rows; adjusting
one (with hint shown) changes that offset on the next song and persists.

---

## Step 10: Cabinet validation matrix + finalize

**Objective:** Run the full matrix on both builds and finalize.

**Matrix:**
- **Boot seed:** config values applied next song (each of the four; SOUND most audible).
- **Live change:** overlay adjust → applies next song, not mid-song (latch, documented).
- **Master OFF/disable:** reverts to stock next song.
- **Overlay UX:** child rows gated by master; fine/coarse adjust; persist across reboot.
- **Suppression:** cabinet buttons don't bleed through while overlay open.
- **Degradation:** setter AOB missing → mod self-disables cleanly; rows fail → config-only works.
- **Cross-version:** smoke-test 20260324 **and** 20260526.

**Finalize:** Scrub any `FUN_`/absolute-address references from shipped `src/` comments
(describe by role + signature name, per CLAUDE.md rule 9 / learnings). Update `README.md`
(new mod + `timing_offsets` config section + the "applies next song" note) and the mod table.
Confirm no `println!`, panic-safe callbacks, clamping, allocator-clean.

**Test requirements:** the matrix is the test; capture results in `summary.md`. `cargo check` +
full `./build.sh` clean, no warnings.

**Demo:** A documented pass across all matrix rows on both builds; the feature is configurable
via both `mod-config.json` and the in-game overlay, with binary-backed hint text.
