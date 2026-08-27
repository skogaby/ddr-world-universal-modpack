# Implementation Plan — Center Arrows for Single Player

Each step yields a working, demoable increment, builds on the prior step, and ends wired in.
This project has **no unit-test harness** (CLAUDE.md): "test requirements" = `cargo check
--target x86_64-pc-windows-msvc` after every change, plus the **diagnostic-build-then-deploy**
discipline (ship one-shot/per-pass logs, observe in spice2x log / DebugView, then proceed).
Full `./build.sh` before any deploy.

## Checklist

- [x] **Step 1:** Add signatures (`hud_layout_builder`, `hud_layout_setter`) + verify AOB on both builds — DONE: both patterns match exactly 1 site on both 20260324 (builder `0x18006c230`, setter `0x18006f5d0`) and 20260526 (builder `0x18006b4f0`, setter `0x18006e220`); `cargo check` clean
- [x] **Step 2:** Scaffold the mod (trait impl, registration, no-op) — appears in registry/mod menu — DONE: `src/mods/center_arrows_single.rs` (`CenterArrowsSingleMod`, id `center-arrows-single`), declared in `mods/mod.rs`, registered in `lib.rs`; `cargo check` clean
- [x] **Step 3:** Builder entry detour + detection — DONE. Detection rewritten after live testing: the `+0x84` play-state is LayoutActor ctor params (single/double STYLE), identical in 1P/2P. Real presence signal = `*(*(*player_slot) + 4)` where the player-object array is resolved via new `player_array_anchor` sig (RIP-decoded). Verified live: 1P-P1 → `single_player=true active_side=0`; 2P → `single_player=false`. Required a TRIPLE-deref fix (decompiler trap).
- [x] **Step 4:** Setter detour + side mapping — DONE. `parent→side` via `(parent-(root+0xE0))/0x48` with range+alignment guard; name matched as C-string. Verified live (`side=Some(0)`).
- [x] **Step 5:** Apply centering — DONE, **but mechanism corrected after first visual test.** Flat `CENTER_X=495` was wrong (collapsed group alignment — the lane/judge group and arrow group landed at different X, neither centered). Replaced with a **signed per-side uniform shift** `LANE_SHIFT=±360`: live 2P CE inspection showed every lane-relative element has identical P1↔P2 spacing of 719 (rigid translation), so shifting the active side by `719/2≈360` (P1 +, P2 −) lands it on the centered midpoint while preserving relative alignment. Handles P1- and P2-side single play symmetrically.
- [x] **Step 6:** Per-player `custom_options` row + texture — DONE (`bool_toggle("center_arrows_1p")`, per-player no cross-sync; `seop_item_center_arrows_1p.png` generated 176x16 RGBA).
- [x] **Step 7:** Gate registration on hook success + safety — DONE (option registered only after both detours install; rollback on partial install; panic-guarded callbacks; no-alloc center path; `disable` removes hooks + clears state). `cargo check` clean, no warnings.
- [~] **Step 8:** Cabinet validation — IN PROGRESS. Detection ✓; elements + lane + lane-cover centering ✓ on both P1- and P2-side single play (live-tested). The ±360 per-side shift is correct. One gap found and fixed: the end-of-song rocketship/"Fullcombo" effect (`FullcomboActor`) was at the side-offset position — fixed by adding `"fullcombo"` to `TARGET_KEYS` (it reads that coord from the same map via `FUN_18006e300`→setPositionXY, so it centers through the existing shift). Pending re-test of the fullcombo fix.
- [x] **Step 9 (was: lane-cover follow-up) — RESOLVED, no separate hook needed.** Both the lane cover and the FullcomboActor position via coords in the LayoutActor map (the same map our setter writes), NOT a truly independent AFP path as first feared. Lane cover centered automatically; FullcomboActor centered once `"fullcombo"` was added to `TARGET_KEYS`. No `ShutterActor`/AFP hook required. (research/r5 superseded.)
- [x] **Step 10 (2026-07-19, bug fix): DOUBLES gate — DONE, cabinet-validated.** Long-standing
  bug found during playfield-styling testing (2026-07-17, `capture/capture_20260717_013031.jpg`,
  logged as a follow-up in `.agents/planning/20260716-arrow-receptor-styling/progress.md`):
  playing DOUBLES as a single player with the option ON shifted the 8-panel lane right by +360 —
  the game ALREADY centers the doubles lane (`double_lane_usr`), so the presence-based
  `single_player` gate alone wrongly applied the shift on top. Fix: capture the per-side
  play STYLE (`builder_root + 0x84 + side*4`: `0=single/1=double/2=absent` — the exact
  field the builder's own lane-name selector branches on, per r2's correction note +
  `docs/hex_edit_porting.md` Hack 2) in the builder-entry hook's `PassState`, and gate the
  setter-hook shift on `style == 0` (side-offset single layout only; unknown styles
  conservatively skipped). Diagnostic transition log now includes `styles=[s0,s1]`.
  **Cabinet validation (2026-07-19, multiple songs alternating play styles):** doubles stayed
  centered (shift correctly skipped); singles still centered via the shift. Log evidence:
  attract demo `single_player=false styles=[0,0]` (no centering); doubles-as-1-player
  `single_player=true active_side=0 styles=[1,1]` (style gate suppresses); singles
  `single_player=true active_side=0 styles=[0,0]` (shift applies). **Empirical note:** in
  doubles BOTH sides read style `1` (`styles=[1,1]`), not `[1,2]` — the `2`/absent value was
  not observed; the gate reads the element's own side's style so this is inconsequential,
  but don't rely on `2` marking the inactive side.

---

## Step 1: Add and verify the two AOB signatures

**Objective:** Define `hud_layout_builder` (`FUN_18006c230`) and `hud_layout_setter`
(`FUN_18006f5d0`) in `src/core/signatures.rs`, each resolving to exactly one site.

**Guidance:** Author each pattern from the function prologue (see R3); wildcard RIP-relative
displacements and the stack-cookie load. Follow existing `SignatureDefinition` style.

**Test requirements:** `cargo check`. Ship a tiny diagnostic (or use the existing signature
resolve-count log at init) confirming both names resolve to a non-null address and match
**count == 1** on **both** 20260324 and 20260526. Use `scripts/aob_check.py` against both DLLs
if available to validate uniqueness before deploying.

**Integration:** Signatures live in the central store; consumed by later steps via
`ctx.signatures.get_address(...)`.

**Demo:** Init log shows `hud_layout_builder` and `hud_layout_setter` resolved (addresses
printed) on both game versions.

---

## Step 2: Scaffold the mod (trait, registration, inert)

**Objective:** Create `src/mods/center_arrows_single.rs` with a `CenterArrowsSingleMod`
implementing `Mod` (id `center-arrows-single`, name, description, `required_signatures()`
returning the two names). `init`/`enable`/`disable` just log for now. Add `pub mod
center_arrows_single;` to `src/mods/mod.rs` and a `Box::new(...)` entry to `mods_to_register`
in `src/lib.rs`.

**Guidance:** Model on `premium_free.rs` structure. No hooks installed yet.

**Test requirements:** `cargo check`; deploy; confirm the mod registers (init log) and is
toggleable in the mod menu without affecting anything.

**Integration:** Mod is now part of the registry and config (`mods.center-arrows-single`).

**Demo:** Mod appears in the in-game mod menu and logs enable/disable; game otherwise
unchanged.

---

## Step 3: Builder entry detour + detection diagnostic

**Objective:** Install a detour on `hud_layout_builder`. In the callback, read `builder_root`
(RCX), the two play-states (`+0x84`, `+0x88`), compute `{single_player, active_side}`, store
in the module static, and call the original. Emit a **one-shot-per-pass** INFO log of
`{s0, s1, single_player, active_side}`.

**Guidance:** Panic-guard the callback (`catch_unwind`). Read-only of game memory; always call
original. Use the project's `static mut` + `addr_of!` idiom for the pass state.

**Test requirements:** `cargo check`; deploy diagnostic build. **Verify R2 semantics live** in
three sessions: 1P P1-side, 1P P2-side, 2P/versus. Expected: P1-single → `s1==2,
single_player=true, active=0`; P2-single → `s0==2, active=1`; 2P → both `!=2`,
`single_player=false`. This is the load-bearing detection check — do not proceed to mutation
until these match.

**Integration:** Pass state is now populated every layout build; consumed by Step 4/5.

**Demo:** Logs correctly classify single vs. two-player and identify the active side across
all three session types.

---

## Step 4: Setter detour + target-key diagnostic (no mutation)

**Objective:** Install a detour on `hud_layout_setter`. In the callback, compute
`side = (parent − (builder_root + 0xE0)) / 0x48` (range/alignment-checked), read `name` as a
C-string, and — when `single_player && side==active_side` — log each `name` seen (once per
distinct key per pass). **Do not mutate `coord` yet.** Call the original.

**Guidance:** Panic-guard. Validate `side ∈ {0,1}` and exact stride alignment before trusting
it; bail (just call original) otherwise. No allocation on this path.

**Test requirements:** `cargo check`; deploy. Confirm the 8 target keys (`arrow_raw`, `arrow`,
`freeze_judge`, `judge`, `combo`, `fast_slow`, `filter`, `score_compare`) appear for the active
side, and that `side` maps correctly (active side only). Catch any key-name drift between
builds here.

**Integration:** Both hooks now cooperate via the shared pass state in a single nested call
stack; centering decision point is fully wired except the write.

**Demo:** Logs show, for an active single-player side, exactly the expected lane-relative keys
flowing through the setter with the correct computed side index.

---

## Step 5: Apply X-centering (Strategy A), option hardcoded ON

**Objective:** In the setter hook, when gated (`single_player && side==active_side &&
name_in_target_set(name)`), overwrite `coord[0] = CENTER_X` (495) before calling the original.
Option enablement is **temporarily hardcoded true** (no option row yet) so we can validate the
visual independent of the options plumbing.

**Guidance:** Define `CENTER_X: i32 = 495` and the target-key set as named constants. Mutate in
place; no allocation. Keep the per-pass diagnostic behind a debug flag.

**Test requirements:** `cargo check`; `./build.sh`; deploy. Enter 1P gameplay (P1 side): arrow
receptors + the lane-relative readouts should be centered (static RE confirms the renderers
read these stored coords and push them into the AFP layers, so this is expected, not
hypothetical). Repeat P2-side single: the active (P2) side centers, P1 side untouched. 2P:
nothing moves (gate). **Cosmetic check only:** confirm the static lane backdrop frame looks
right — if (unexpectedly) off-center, that's the sole trigger for the Step 9 contingency.

**Integration:** Core centering is now functional end-to-end (minus the user-facing toggle).

**Demo:** Single-player playfield visibly centers in-game; two-player is unaffected.

---

## Step 6: Per-player option row + texture

**Objective:** Generate the label texture and register the per-player toggle. Add
`("center_arrows_1p", "CENTER ARROWS (1P ONLY)")` to `scripts/gen_option_labels.py` `LABELS`
and run it (emits `seop_item_center_arrows_1p.png`). Register
`RegisterSpec::bool_toggle("center_arrows_1p").default_value(0).on_change(on_change)`;
`on_change(side, value)` writes `OPTION_ENABLED[side]` (per-player, **no** cross-sync). Replace
the hardcoded-ON from Step 5 with `OPTION_ENABLED[side]` (and/or a defensive
`custom_options::get_value(side, "center_arrows_1p")`).

**Guidance:** Mirror `premium_free` registration. Boolean reuses stock `seop_op_on/off`. Verify
`custom_options::is_available()` before registering.

**Test requirements:** `cargo check`; `./build.sh`; deploy. Row "CENTER ARROWS (1P ONLY)"
appears on the Mods tab, legible (condensed is fine). Toggling ON centers on next layout build;
OFF restores stock. Card out/in restores the per-player value (network + JSON persistence). P1
and P2 values are independent.

**Integration:** Full user-facing feature: per-player opt-in, persisted, gated to
single-player.

**Demo:** A player enables "CENTER ARROWS (1P ONLY)" in options; their single-player play is
centered and the choice persists across sessions; the other side's setting is independent.

---

## Step 7: Gate registration on hook success + safety hardening

**Objective:** Ensure the option row is registered **only if both detours installed**
(`HOOKS_OK`). On any signature/detour failure: log a warning and register nothing (no inert
row). Final pass over both callbacks for FFI-safety (panic guards, no `unwrap`/`expect`/
panicking-index, null/range guards), and confirm no allocation on the centering path.

**Guidance:** Install + verify detours in `init()`/early `enable()`, set `HOOKS_OK`, and gate
`register_option` on it. Confirm `disable()` removes detours (or relies on `ModRegistry`
auto-removal per the existing pattern) and clears `OPTION_ENABLED`.

**Test requirements:** `cargo check`; deploy. Normal path: row present + working. Simulate
failure (e.g. temporarily break one signature pattern in a local build): confirm the row is
**absent**, a warning is logged, and the game + other mods run normally with no crash. Toggle
the mod off in the mod menu → centering stops, row removed/inert per design.

**Integration:** Mod now fully honors graceful degradation and the "no inert row" rule.

**Demo:** With signatures intact, the feature works; with a signature forced to fail, the row
never appears and nothing else breaks.

---

## Step 8: Cabinet validation matrix + lane-frame cosmetic check

**Objective:** Run the full validation matrix and confirm the (RE-predicted) Strategy A result,
including the one residual cosmetic check (static lane frame).

**Guidance / matrix:**
- 1P P1-side: receptors + readouts centered; **static lane backdrop frame** alignment noted.
- 1P P2-side: active side centers; inactive side untouched.
- 2P/versus (option on for one/both): no centering.
- Option off: stock layout.
- Persistence: card out/in round-trip.
- Cross-version: smoke-test 20260324 **and** 20260526.

**Decision (expected: ship A).** Static RE already confirms the receptors + lane-relative
elements center via Strategy A. The only open question is the **static lane backdrop frame**:
if it's acceptably aligned → **done, ship A**. Only if that specific frame reads off-center →
Step 9 (targeted Strategy B lane-layer reposition); force-double remains the last resort.

**Test requirements:** the matrix above is the test. Capture results in the summary.

**Demo:** A documented pass across all matrix rows on both builds, with a recorded lane-skin
decision.

---

## Step 9 (conditional): Strategy B — reposition the single lane AFP layer

**Objective:** Only if Step 8 shows the single lane backdrop doesn't follow the element
centering. Capture the per-side lane AFP layer id (from the `%dp_lane_usr` bind in the builder)
and reposition it to the centered X via `bm2d_api::set_position` (or `mc_set_param` position
param), gated identically (`single_player && side==active_side && OPTION_ENABLED[side]`).

**Guidance:** Reuse `src/services/bm2d_api.rs` wrappers (`set_position`/`mc_set_param`); see
`series_filter_scroll.rs` for the AFP-position-injection precedent. Determine the centered AFP
X (may differ in units from the layout-space `CENTER_X`; derive by reading what the centered/
double lane resolves to, or by matching the receptor center). Keep the change minimal and
gated.

**Test requirements:** `cargo check`; `./build.sh`; deploy. Single lane art now co-located with
the centered receptors on both sides; 2P and option-off unaffected; re-run the Step 8 matrix.
If still wrong, fall back to force-double and retest.

**Integration:** Completes the preferred Q4 behavior (single lane skin, centered).

**Demo:** Single-player play shows the **single** lane skin centered with its receptors — the
preferred visual — persisted per-player and gated to single-player.
