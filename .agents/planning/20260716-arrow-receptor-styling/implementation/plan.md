# Implementation Plan — Playfield Styling

Design: `../design/detailed-design.md` (authoritative for all mechanisms,
offsets, and error handling — not repeated here).
Research: `../research/arrow-render-re.md`, `../research/existing-code.md`.

Per repo convention, maintain `../progress.md` throughout implementation
(update after each step and before any pause/handoff).

Validation baseline for every step: `cargo check --target
x86_64-pc-windows-msvc` clean, then a cabinet deploy (`./scripts/deploy.sh`)
observing `[DDR-Hook]` logs — this repo has no unit tests.

## Checklist

- [ ] Step 1: Resolution groundwork — derivations + verification-only mod skeleton
- [ ] Step 2: Option rows, per-side values, per-song latch (plumbing only)
- [ ] Step 3: Fill hook — registry, side binding, scale + opacity transform
- [ ] Step 4: Cull-window patch (collector site) wired to the latch
- [ ] Step 5: Guideline styling — capture + emitter detours + guideline cull site
- [ ] Step 6: Mine integration (`note_types_expansion::mine_render`)
- [ ] Step 7: Hardening, gate ordering, docs, full acceptance pass

---

## Step 1: Resolution groundwork — derivations + verification-only mod skeleton

**Objective:** Everything the mod needs is *resolved and verified* at boot,
with zero behavior change.

**Guidance:**
- `src/core/signatures.rs`: add derivations (design §4.3/§4.4) —
  collector (first CALL in `render_notes`), collector cull site (scan for
  `MOVSS XMM15, [RIP+disp]` → 720.0f), guideline draw (`get_offset_y` xref
  whose callee set excludes collector/fill), guideline cull site (XMM9
  form), guideline bulk emitter (callee writing tag 0x01 / stride 0x14;
  verify single caller), and the three renderer vtables via RTTI walk
  (`screen::ArrowRenderer`, `screen::SpotRenderer`,
  `screen::JudgeEffectRenderer`).
- New `src/mods/playfield_styling/mod.rs` (+ module registration in
  `mods/mod.rs`, `lib.rs`): id `playfield-styling`, name "Playfield
  Styling". `init` resolves/verifies all of the above + `player_array_anchor`
  and logs one INFO line per item (address + verified bytes where
  applicable); any miss → WARN + `init` returns false.

**Validation:** cargo check; deploy; boot log shows every derivation
resolved on the cabinet build; toggling the mod on/off does nothing else.

**Integrates:** registered in `lib.rs` like every mod; no hooks yet.

**Demo:** boot log lists all resolved addresses (fill, collector, cull
sites, guideline draw/emitter, 3 vtables) with verification results.

## Step 2: Option rows, per-side values, per-song latch (plumbing only)

**Objective:** The two options exist, persist, and latch per song — still no
render effect.

**Guidance:**
- Add `("arrow_scale", "ARROW SCALE")`, `("arrow_opacity", "ARROW OPACITY")`
  to `scripts/gen_option_labels.py`; run it; deploy the PNGs to the
  cabinet's `data_mods/custom_options/.../tex/`.
- `mod.rs`: register the two `Scalar` rows (design §4.1 spec) in `enable()`;
  `Duplicate` = success; enable-time reseed. Per-side `SCALE_PCT` /
  `OPACITY_PCT` atomics; `on_change` mirrors + logs.
- `scene_manager` callback: on GAMEPLAY entry, snapshot into `LATCHED`,
  compute the would-be cull bound (`720/min(s,1)`), log both sides + bound;
  on exit, log a stub stats line.
- Dev-note in code: row registration moves behind the full gate in Step 7.

**Validation:** cargo check; deploy; rows render on the Mods tab with
correct labels/ranges/steps; values survive card-out→card-in (network+JSON);
latch log fires at song start with correct values.

**Integrates:** consumes Step 1's init gate (rows only if init passed).

**Demo:** adjust ARROW SCALE to 50 on cabinet, start a song → log shows
`latch p1 s=0.50 op=1.00 cull=1440`.

## Step 3: Fill hook — registry, side binding, scale + opacity transform

**Objective:** Arrows, freezes, shocks, receptors, and hit flashes visibly
scale and fade per side. (Pop-in at s<1 expected — fixed in Step 4.)

**Guidance:**
- New `fill_hook.rs` per design §4.2: `install_enabled` detour on
  `render_sprite_final` (9-arg fn type); 16-slot registry with
  `REGISTRY_LEN` early-out; vtable classification; lane width from mode
  (+0xB0 / +0x98; JudgeEffect inherits per side, deferred bind); side via
  presence read / posX<640 / doubles→0; transform + color-copy compose;
  identity fast-path.
- Registry cleared by the Step 2 scene callback (entry + exit); bind
  one-shot INFO logs (`side/class/half_width/posX`).
- `catch_unwind` on the callback body; forward untouched on any anomaly.

**Validation:** cargo check; diagnostic deploy at identity first (zero
transforms fire, binds still logged); then 50%: receptors shrink centered
in place, arrows converge to lane center, freeze bodies/tails and shock
overlay coherent, hit flash scaled; opacity 50 fades everything; versus =
independent sides; reverse scroll correct.

**Integrates:** reads Step 2's `LATCHED`; gated by Step 1's resolution.

**Demo:** side-by-side versus with P1=50%/P2=100% — two coherent playfields;
screenshot-comparable to the reference images (pop-in at screen bottom
acknowledged as the Step 4 gap).

## Step 4: Cull-window patch (collector site) wired to the latch

**Objective:** No arrow pop-in at any shrink scale.

**Guidance:**
- New `cull_patch.rs` per design §4.3: mod-owned f32 slot (int3-cave near
  the collector, RIP-reachable; near-VirtualAlloc fallback), init 720.0;
  verified disp32 rewrite of the collector site at enable; latch writes
  `720/min(s,1)`; disable/identity writes 720.0. Never unpatch.
- Wire into Step 2's latch callback (replace the log-only bound).

**Validation:** cargo check; deploy; at 25% scale on a fast chart the
bottom edge shows no pop-in; at 100% the slot is 720.0 (log) and behavior
is byte-identical to stock; mixed versus (25/100) clean on both sides.

**Integrates:** consumes Step 1's verified cull site; driven by Step 2's
latch; visual effect completes Step 3.

**Demo:** 25% scale, high-BPM chart — arrows enter smoothly from the screen
bottom; toggle mod off mid-session → next song fully stock.

## Step 5: Guideline styling — capture + emitter detours + guideline cull site

**Objective:** The measure guideline scales/fades with the lane in both
scroll directions (load-bearing per A6).

**Guidance:**
- New `guideline_hook.rs` per design §4.4: capture detour on the guideline
  draw (side bind, `Ybase/s` pre-scale + restore, PASS_STATE); transform
  detour on the bulk emitter (0x14-record rewrite: x-about-center, y/w/h
  scale, alpha MSB compose); patch the guideline cull site to the Step 4
  float slot.
- Both detours via `install_enabled`, `catch_unwind`, forward-untouched
  default.

**Validation:** cargo check; deploy with guideline option enabled: 50% →
lines match the scaled lane width/positions; reverse scroll → no early
cut-off at the top; opacity composes; guideline OFF → no observable change;
other sprite draws unaffected (emitter forward-untouched path).

**Integrates:** shares the Step 4 float slot and Step 2 latch; completes
the all-or-nothing gate set.

**Demo:** guideline + reverse + 50% scale — lines track the shrunken lane
across the full scroll range.

## Step 6: Mine integration (`note_types_expansion::mine_render`)

**Objective:** Mines follow the scaled/faded playfield when both mods are
active.

**Guidance:**
- `fill_hook.rs`: expose `pub fn style_for_renderer(*const u8) ->
  Option<StyleSnapshot>` and `pub fn cull_bound() -> f32` (lock-free,
  render-thread).
- `mine_render.rs`: inside its existing pass, query the snapshot for the
  live renderer; transform mine quad `(x, y, w, h)` + alpha identically;
  replace hardcoded 720/margin checks with `cull_bound()`. `None` → stock
  path untouched.

**Validation:** cargo check; deploy with a mine chart: 50% → mines track
the scaled columns and fade with opacity; extended window shows no mine
pop-in at 25%; playfield-styling disabled → mine behavior byte-identical
to today.

**Integrates:** consumes Step 3's registry and Step 4's bound; zero new
hooks (respects the existing `render_notes` detour ownership).

**Demo:** mine chart at 50% scale with both mods on — mines, arrows, and
receptors all coherent.

## Step 7: Hardening, gate ordering, docs, full acceptance pass

**Objective:** Ship-ready: strict A6 gating, tidy logging, documentation,
and the full A8 acceptance checklist executed on cabinet.

**Guidance:**
- Move option-row registration behind the complete gate (fill detour +
  collector patch + guideline detours/patch all installed) — remove the
  Step 2 dev-note; verify a forced failure (temporarily bad AOB) yields
  self-disable with no rows and clean logs.
- Audit: `catch_unwind` coverage, hot-path log absence, mid-song disable
  (`MOD_ENABLED` gate + 720.0 slot write + latch clear), re-enable reseed.
- Cross-build sanity: run the derivation chain against the 20260324-lineage
  build in Ghidra (collector/cull-site/guideline resolution) before
  declaring version-agnostic.
- Docs: README mod-table row; AGENTS.md Key Entry Points row; new
  `docs/` RE note for the arrow render path (distilled from
  `research/arrow-render-re.md`); final `progress.md` + `summary.md`.
- Execute the full A8 checklist (design §7) and record results in
  `progress.md`'s deploy & test log.

**Validation:** the A8 checklist — all items pass on cabinet.

**Integrates:** finalizes everything; no new functionality.

**Demo:** the complete feature: two new rows on the Mods tab driving a
scaled, faded playfield across singles/versus/doubles/reverse, with
persistence and stock-perfect behavior when off.
