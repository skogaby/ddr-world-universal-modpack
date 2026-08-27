# Idea Honing — FPS Unlock

Requirements clarification for the FPS unlock mod (Hack 5 in `docs/hex_edit_porting.md`).
One question at a time; answers recorded as we go.

---

## Q1. Static value vs. per-scene auto-switch — what is the core behavior?

The prior research identified a fundamental tension:

- Raising the global FPS target makes **gameplay** scroll smooth (good — engine is
  delta-time based), BUT
- Some **menu/selection/attract** animations are frame-counted (advance a fixed step
  per tick, not multiplied by the global delta), so they run visibly **too fast** at
  high FPS.

Three possible shapes for the mod:

1. **Static global value only** — simplest. One configurable FPS target applied
   cabinet-wide for all scenes (mirrors `patches.js`). Accepts the menu-speedup
   side-effect; user picks the value knowing the tradeoff.
2. **Per-scene auto-switch only** — high target in gameplay, force 60 in
   menus/selection/attract. Kills the speedup but requires the display target to be
   re-writable live (RE-dependent — must verify it's re-read, not consumed once).
3. **Both, config-selectable** — ship the static path (always works) AND the
   per-scene auto-switch as an opt-in/recommended mode.

Which behavior do you want as the target for this implementation?

**Answer:** **Option 3 — both, config-selectable, but phased.**

- **Milestone 1:** ship the **static global value** path first (no scene awareness).
  Maintainer deploys and tests at the unlocked FPS.
- **Decision gate:** if menu/selection animations behave acceptably at the unlocked
  FPS (no unintended side-effects), **skip** the per-scene gating entirely — don't
  build complexity that isn't needed.
- **Milestone 2 (conditional):** only if the menu speedup is actually objectionable
  on the cabinet, add per-scene auto-switch (high in gameplay, 60 elsewhere).
- The **RE pass should also inform necessity** — e.g. whether the frame-counted
  animation paths are as prevalent as the prior doc claims, and whether live
  per-scene rewrite is even feasible (target re-read each frame vs. consumed once).

Net: design accommodates both, but Milestone 1 is static-only and we may stop there.

---

## Q2. Value selection model — fixed presets vs. free scalar

`patches.js` exposed FPS as a union of fixed options (60/120/144/165/240/360). The mod
is cabinet-wide (single display target), so it'll resemble timing-offsets: configured
via `mod-config.json` + adjusted in the mod overlay. How should the operator pick the
value — fixed enum presets, free scalar, or hybrid?

**Answer:** **Enum with preset values, but the preset list itself lives in
`mod-config.json`.**

- The overlay row is an **enum picker** cycling a list of FPS presets
  (default: 60 / 120 / 144 / 165 / 240 / 360).
- The **enum entries are defined in `mod-config.json`**, not hardcoded — so a user
  with an oddball refresh rate (e.g. 100, 280) can **add their value to the list** in
  config and have it appear as a selectable preset. (Defaults to the six standard
  values if the user doesn't specify.)
- **Scope implication (confirmed by code read):** `mod_menu`'s `RowKind` currently
  supports only `Boolean` and `Scalar` — there is **no `Enum` row kind yet**. So this
  milestone must either (a) add a real `Enum` RowKind to the overlay infra (mirrors
  how timing-offsets added `Scalar`/menu-button support — a "Part I"-style chunk), or
  (b) emulate an enum via a `Scalar` index row that maps an index → the configured
  preset list. → see Q3.

---

## Q3. Real `Enum` row kind vs. emulate with a `Scalar` index row?

Since the overlay only has `Boolean`/`Scalar` today, present the config-defined preset
list as either a new real `Enum` RowKind, or a `Scalar` index emulation?

**Answer:** **Option 1 — add proper `Enum` row support to the overlay.**

- Add a real `RowKind::Enum { index, labels }` to `mod_menu` (Left/Right cycles
  labeled entries, e.g. shows "144 FPS"); wire it through the render / adjust /
  visibility / hold-to-repeat paths the same way `Scalar` is. This is reusable
  overlay infra — the timing-offsets work explicitly anticipated an `Enum` kind.
- Aligns with the intent from Q2 to expose the enum entries in `mod-config.json`: the
  configured preset list becomes the `Enum` row's `labels`/values.
- This makes the feature a "two-part" effort like timing-offsets: **Part I** = the
  reusable `Enum` overlay infra; **Part II** = the FPS mod that consumes it.

---

## Q4. Master on/off semantics and the OFF / disabled value

When the mod is OFF, what FPS value applies? (Timing-offsets precedent: capture the
game's genuine stock value live on first write, revert to exactly that when OFF.) The
FPS target is cabinet-selected at app-init (60 normally, 75 if `MachineType == 1`), so
"stock" isn't a fixed constant. Also: is "changes take effect on restart" acceptable
if RE confirms the value is consumed once at app-init (not re-read per frame)?

**Answer:** **Capture genuine stock and revert to it on OFF (Option 1), with a 60 Hz
fallback (Option 2) if capture proves infeasible.**

- **No real cabinet runs at 75 Hz** (per maintainer) — so the `MachineType == 1` → 75
  branch is effectively dead in practice, but capturing the genuine computed value is
  still preferred and "seems pretty doable."
- **Preferred:** capture the stock display target at boot (whatever app-init computed)
  and restore it when toggling OFF. Mirrors the timing-offsets stock-capture pattern.
- **Acceptable fallback:** if clean capture turns out not to be feasible, just apply
  **60** when toggling ON→OFF.
- **"Changes take effect on restart" is acceptable** if RE confirms the display target
  is consumed only once at app-init and is not re-read per frame (analogous to
  timing-offsets' documented "takes effect next song"). RE should determine this; if
  the value *is* re-read live, runtime toggle can apply immediately instead.

---

## Q5. `mod-config.json` schema for the FPS mod

Confirm the typed config section shape (mirrors the timing-offsets precedent of a
dedicated typed section). Proposed:

```jsonc
"fps_unlock": {
  "presets": [60, 120, 144, 165, 240, 360],  // enum entries; user-editable, oddballs OK
  "selected": 120                             // active value, stored as raw FPS
}
```

**Answer:** all proposed defaults confirmed.

1. **Active selection stored as the raw FPS value** (`"selected": 120`), NOT an index —
   avoids 0-/1-based indexing confusion and survives reordering/editing `presets`.
2. **If `selected` isn't in `presets`, auto-add it** so the picker always includes the
   active value.
3. **If `presets` is missing/empty, default to** `[60, 120, 144, 165, 240, 360]`.
4. **Sort presets ascending for display** regardless of config order (cleaner picker).
5. **Field naming:** section key `fps_unlock`; fields `presets` / `selected`. Mod id is
   `fps-unlock` (kebab-case, matching the `mods` map convention).

> Implied normalization at load: dedupe presets, drop non-positive/invalid entries,
> sort ascending, ensure `selected` ∈ presets (auto-add), fall back to defaults if the
> list ends up empty. (Exact bounds/sanity rules to be pinned in design.)

---

## Q6. Apply mechanism — AOB byte-patch the imm32 vs. hook app-init/consumer?

Two levers from prior research: (1) AOB-scan the imm32 site and overwrite the `0x3C`
immediate in place; (2) detour the app-init function (`FUN_1800020f0`) or the consumer
(`FUN_1801f0030`) and rewrite the display-target field after the game computes it.

**Answer:** **Defer the choice to the RE pass**, with a race-condition tiebreaker.

- **Convention clarification (maintainer):** the project rule is specifically against
  hardcoding **file offsets** into patches — because hooks/patches must run
  version-agnostically and functions move between builds. A **plain byte patch is
  totally acceptable** as long as the target address was determined via AOB (or RTTI
  walk, RIP-relative derivation, etc.), not a baked-in offset. (Precedent:
  `timer_freeze`, `premium_free`, `song_limit_expansion` are all AOB-resolved byte
  patches.)
- So **both (1) and (2) are convention-compliant.** The decision is technical, not
  stylistic.
- **Tiebreaker:** if approach (1) has a real **race condition** — i.e. the DLL must win
  a race to overwrite the imm32 before app-init executes that instruction — then **(2)
  the hook is preferable** (it captures the genuine computed value naturally and has no
  race). RE must assess: does the DLL reliably patch before app-init runs that line?
  When does `FUN_1800020f0` execute relative to our init thread?
- **Decision owner:** the RE findings. Capturing genuine stock (Q4) and apply timing
  (Q4 "restart" nuance) both feed into this.

---

## Q7. Graceful degradation tiers

Same two-tier degradation model as timing-offsets?

**Answer:** **Yes — two-tier, mirroring timing-offsets.**

1. **Apply lever (AOB patch site / hook from Q6) — load-bearing.** If the FPS
   imm32 / hook point can't be resolved, the mod self-disables cleanly (declare in
   `required_signatures()` so `ModRegistry` skips it, or fail in `init()`). Without it
   the mod can do nothing.
2. **`Enum` overlay row — optional.** If overlay-row registration fails or the overlay
   infra isn't available, the mod still applies the configured `selected` FPS from
   `mod-config.json`. Config-file control is the fallback; the overlay is convenience.

---

## Q8. Overlay row label, hint text, entry format, OFF display

**Answer:**

1. **Row label:** `FPS TARGET`.
2. **Hint line:** **do NOT add side-effect warning text yet.** ⚠️ **Important new
   datapoint:** the hex edit being ported is **itself a DDR World hex edit** (not an
   older-version one). The maintainer's friend tested **that World FPS-unlock hex edit**
   and did **NOT** observe sped-up menu animations. FPS-unlock hacks *also* existed for
   **older DDR versions**, and **those older versions DID exhibit the menu-animation
   speedup** — the prior research (`docs/hex_edit_porting.md` Hack 5) appears to have
   **pre-emptively assumed** that older-version side-effect carries into World, but the
   live World test suggests **World does not exhibit it** (likely engine fixes / more
   dt-correct animation paths). To be confirmed during implementation/testing. → keep
   the hint neutral (e.g. just "Display refresh target."); add caveats only if a real
   side-effect is observed on the cabinet.
3. **Enum entry format:** `"60fps"` — lowercase, no space (e.g. `120fps`, `144fps`).
4. **When master toggle OFF:** **hide the enum row** entirely (via
   `visible_when`/`parent_row_key`, same as timing-offsets' scalar rows).

> **RE / scoping impact (high):** this strongly undercuts the prior doc's central
> rationale for per-scene switching (Milestone 2). The menu speedup is **confirmed real
> on older DDR versions** but **reported absent on World** (live test of the World hex
> edit). If World does NOT exhibit it, the **static global FPS value is likely the
> entire feature** and Milestone 2 can be dropped, not just deferred. The RE pass should
> specifically scrutinize the "frame-counted animations advance per-tick" claim against
> the *World* binary — i.e. check whether menu/AFP timeline advances in World actually
> multiply by the global delta `DAT_1806ea714` (⇒ no speedup) or step a fixed amount per
> tick (⇒ speedup, as in older versions). The prior doc may have carried the
> older-version behavior over to World by assumption rather than World-specific proof.

---

## Q9. Persistence of the selected FPS + `presets` write-back behavior

On overlay change, persist `fps_unlock.selected` to `mod-config.json` (cabinet-wide,
NOT via `custom_options` per-player machinery — mirrors timing-offsets writing directly
to its config section). Question: should the mod also write back a normalized `presets`
array, or leave the user's array untouched?

**Answer:**

- **Persist `selected` immediately on each overlay change** — read-modify-write to
  `mod-config.json` preserving other keys, exactly like timing-offsets. This is
  cabinet-wide config, so it does NOT use the per-player `custom_options`
  network/JSON persistence paths.
- **`presets` write-back: Option 1 — leave the user's `presets` array untouched.**
  Normalize only in-memory (sort ascending / dedupe / auto-add `selected`) for display;
  never rewrite the operator's hand-authored list. The user's config `presets` is the
  authoritative source; only `selected` is ever written back by the mod.

---

## Requirements clarification — COMPLETE (2026-06-28)

Maintainer confirmed requirements are covered thoroughly enough to proceed to the RE
research phase. Open items below are explicitly **RE-determined**, not requirements gaps.

### RE questions carried into research
1. **Re-verify the app-init imm32 site + AOB** on both builds (20260324, 20260526) —
   `FUN_1800020f0`, pattern `C7 44 24 ?? 3C 00 00 00 75 08 C7 44 24 ?? 4B 00 00 00`.
   Confirm uniqueness on both builds.
2. **Is the display target re-read each frame or consumed once at app-init?** Decides
   live-toggle-applies-immediately vs. restart-to-apply (Q4), and feeds the apply
   mechanism (Q6). Trace `+0x14` field → `FUN_1801f0030` → present/refresh path.
3. **Patch-in-place race vs. hook (Q6):** does the DLL reliably patch the imm32 before
   app-init executes that line? When does `FUN_1800020f0` run relative to our init
   thread? If a race exists → prefer the hook.
4. **Does World actually exhibit the menu-animation speedup?** (Q8 — high impact.)
   Confirmed on older DDR versions, reported absent on World. Scrutinize the
   "frame-counted advance per-tick" claim against the World binary: do menu/AFP
   timeline advances multiply by `DAT_1806ea714` (⇒ no speedup) or step fixed per tick
   (⇒ speedup)? Decides whether Milestone 2 exists at all.
5. **Stock-value capture feasibility (Q4)** + handling of the `MachineType==1`→75
   branch (believed dead in practice — no real 75 Hz cabinets).
6. **Overlay `Enum` infra scoping (codebase, not RE):** survey current `mod_menu`
   `Scalar` row implementation (render / adjust / visibility / hold-to-repeat / row
   API) to scope adding a parallel `Enum` RowKind.

---

## RE research — COMPLETE (2026-06-28). Findings + checkpoint decisions

Full notes in `research/r1`–`r4`. Headline: the feature **simplifies** vs. the prior doc.

### Answers to the carried RE questions
1. **AOB site:** CONFIRMED unique single match on **all three** loaded builds (20260324,
   20260526, **and 20250805** — a build the prior doc never checked). Byte-identical.
   Patch = `0x3C` imm32 at match+4, inside `Application::onBoot()` (`FUN_1800020f0`).
2. **Liveness:** the target is copied to global `DAT_1806ea488` and read by **exactly one
   function at boot** (`Renderer:initGs`), feeding **D3D device creation**. **Consumed
   once, never re-read per frame.** → static value is correct; **runtime change applies on
   restart** (confirmed unavoidable); **live per-scene rewrite is infeasible** (would need
   a device reset).
3. **Patch-vs-hook race:** apply lever = **AOB byte-patch via the existing `early_apply`
   phase** (precedent: `song_limit_expansion`). Byte-patch and a detour share the **same
   microsecond-wide boot deadline** (detour is NOT a wider window). Stock-capture is trivial
   (read the imm32 first). Fallback ladder ends in a strongly-non-preferred on-disk patch.
   **No ban/integrity risk** (unofficial networks, no checks — maintainer-confirmed); the
   only objection to on-disk is philosophical (runtime-only) + maintenance.
4. **Menu speedup on World:** engine is overwhelmingly delta-time based; sampled animation
   path scales by `DAT_1806ea714`. Combined with the live World hex-edit test (no speedup),
   the prior doc's speedup premise appears to be **older-version behavior carried into World
   by assumption.**
5. **Stock capture / 75Hz branch:** stock = the imm32 byte itself; the `MachineType==1→75`
   branch is dead in practice (no 75Hz cabinets).
6. **Enum overlay infra:** small/low-risk — exhaustive `match`es make adding `RowKind::Enum`
   compiler-self-checking; ~6 arms + a `register_enum_row` API, mirroring the `Scalar` work.

### Iteration-checkpoint decision (2026-06-28)
- **Milestone 2 (per-scene auto-switch): DROPPED ENTIRELY.** It is both infeasible via this
  lever (value consumed once) and unnecessary (no speedup on World). The design covers only
  the **static value + reusable `Enum` overlay row**. Per-scene switching is noted as
  explicitly out-of-scope/infeasible; if a real speedup ever appears on the cabinet, it
  would be a **separate** effort requiring a device-reset lever.
- **No contingency appendix** for per-scene (not "keep as documented contingency").
- **Ready to proceed to design.**
