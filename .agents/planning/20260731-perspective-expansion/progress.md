# Progress: Perspective Expansion (Distant, Incoming, Space)

Updated: 2026-08-05
Status: DONE — all 4 steps complete; three live passes; feature verified on
cabinet (2026-08-05, third pass: everything including the freeze-hold glow
correct on all perspective options). Shipped defaults accepted as-is
(distant_focal=3000, distant_zoom=0.9 — no tuning changes requested).
NEXT ACTION: none — maintainer commits the working tree (repo convention).

Resume protocol: read `implementation/plan.md` (Approved 2026-07-31; Steps 1–3
ticked) and `design/detailed-design.md` (Approved 2026-07-31, **Revised
2026-08-04** — the revision note at the top is the authoritative delta).
Nothing committed — maintainer commits himself (repo convention).

## Done

- Step 1 — generalized constant pipeline (behavior-preserving):
  `src/mods/player_perspective/mod.rs` (`PerspParams{mode,k,z0,skew}`,
  `PerspConstants`, `compute_constants`, shared `lane_center`),
  `pass_rewrite.rs` (notes pre + spot detour emit via `emit_persp_constants`;
  c49 = {d_min, z0, 0, 0} — c49.y now always latched z0, NEVER 0),
  `guideline_hook.rs` (`PerspLine` → carries `PerspConstants`; generalized map
  `sp = z0·k/(k+clamp(d))` converging about `(cx, anchor)`; record-center ≡
  lane-center equivalence documented in-code). Desk-check: HALLWAY constants
  bit-identical to shipped (c49.y=1.0 is ignored by the old VS — verified
  reserved).
- Step 2 — presets end-to-end: enum values DISTANT=2/INCOMING=3/SPACE=4 (SL
  order), clamps widened to [0,4]; latch resolves per-preset tunables into 4
  cabinet atomics; full `compute_constants` table (neg-tilt: anchor=receptor row,
  dir=y_dir; pos-tilt: anchor=(pos_y+entrance)/2 with entrance=360+360·y_dir,
  dir=−y_dir, z0; skew: cx lerp toward 640); cull contribution now
  `is_negative_tilt` only; config keys `distant_focal`=3000 / `distant_zoom`=0.9 /
  `skew_strength`=1.0 with latch-time clamps (100..100k / 0.1..1 / 0..1) +
  containment-formula comment; module doc/description/logs updated
  (`mode_name`, per-knob enable log). Desk-check vs design: entrance s≈1.004,
  receptor s≈0.816 @ ≈57 px displacement; INCOMING@skew=0 ≡ HALLWAY; doubles
  lerp no-op. Assets: `scripts/gen_option_labels.py` +3 RIBBONS, +3 WIDE
  previews; regenerated (6 net-new PNGs kept, churn restored via git checkout).
- Step 3 — z0 VS extension: `s *= PerspParams1.y` in both
  `shaders/src/gs_screencommand_arrow.hlsl` and `gs_screencommand_default.hlsl`
  (constants doc updated to the generalized names; default keeps `o3 = c23`).
  Rebuilt via `scripts/build_shaders.sh` (pinned fxc 9.29.952.3111, CrossOver):
  both persp VS blobs exactly +1 instruction (20→21, 21→22, vs_3_0, +16 B);
  both PS blobs byte-identical (untouched HLSL, deterministic compile) — only
  the 2 persp blobs are modified in git.
- AGENTS.md `player_perspective` row updated for the full preset family
  (Step 4's non-tuning doc work, front-loaded).
- Build gates green at every step: `cargo check` → `cargo fmt` → `./build.sh`
  (logs in `logs/`).

## In flight

(nothing — Round-2 revision built and gated; awaiting the second cabinet pass)

## Deploy & test log

- 2026-08-04 (maintainer, first live pass — full build of Steps 1–3):
  INCOMING/SPACE judged not worth keeping (unpleasant to play; skew spill) →
  REMOVED. DISTANT kept with two defects: (1) receptors sat ~57 px too low
  (the un-shifted mid-field-anchor map pulls the row toward the anchor);
  (2) the receptor hit flash stayed at the STOCK position/size (it's an AFP
  clip — the VS never touches it). Screenshot evidence: P1 DISTANT vs P2
  stock, flash floating above the mapped receptors.
- 2026-08-05 (maintainer, second live pass — Round-2 build): everything
  works as expected EXCEPT one new find: the arrow-shaped glow shown for
  the duration of a freeze hold renders unscaled/uncorrected at the stock
  receptor position (screenshot: DISTANT, glow floating above the mapped
  receptors).
- 2026-08-05 (maintainer, third live pass — Round-3 build): ALL PASS —
  everything verified, including the freeze-hold glow on all perspective
  options. Feature complete; defaults accepted without retuning.

### Round-3 revision (implemented 2026-08-05, awaiting cabinet pass)

Root cause (Ghidra, verified on 20260324/20260616/20260721): the freeze-hold
glow (and the tap hit-burst) is drawn by `screen::JudgeEffectRenderer` —
arrow-sheet cells at the receptor row, through its OWN per-frame draw
(20260721 `0x180028070`; record vector @ +0xA0/+0xA8, type field selecting
150/200 ms lifetimes) which emits its OWN tag-0x13 SetShader (judge shader @
this+0x98, constructor-proven, program hardcoded 0) into the global command
list — outside every rewritten pass, so it rendered flat. Fix, three parts:

- `shader_synthesis.rs`: judge container overlaid when AA **or** persp;
  prog 0 = stock VS + (AA ? AA judge PS : stock judge PS); prog 1 (persp) =
  **arrow persp VS blob** + same PS (stock judge VS is byte-identical to the
  stock arrow VS; same v0/v1 PS contract — no new HLSL/blob). Fingerprint
  version bumped v1→v2 (recipe changed for identical inputs — forces cache
  regeneration).
- `signatures.rs`: new best-effort AOB `judge_effect_render`
  (`48 89 5C 24 10 57 48 83 EC 40 48 8B 99 A8 00 00 00 4C 8B 89 A0 00 00 00
  48 8B F9` — prologue + structural vector-field loads; unique single hit on
  20260324 `0x1800279b0` / 20260616 `0x180028490` / 20260721 `0x180028070`).
- `pass_rewrite.rs`: judge-effect detour (spot recipe: constants + snapshot
  before, window rewrite after, same ≥2-programs gate). Side binding is
  presence-first per the maintainer's correction — versus (both present)
  uses the posX split (lanes guaranteed left/right) with NO cross-side
  fallback; single/doubles takes whichever side PUBLISHED constants
  (exactly one lane pass runs; robust to center-arrows-1P and to doubles'
  side-0 binding). Constants come from the published per-side block — no
  independent re-derivation (the judge object has no verified mode field).

### Round-3 verification list (maintainer)

Deploy DLL only is NOT enough if the synthesis cache predates v2 — deploy
DLL + confirm the boot log regenerates the containers (fingerprint bump
forces it; no blob changes this round).

1. DISTANT freeze hold: the glow lands ON the mapped receptors at their
   scale; taps' hit-burst likewise.
2. HALLWAY freeze hold: glow still correct (s≈1 at the row → ≈stock; boot
   log shows "judge-effect pass live" once).
3. OVERHEAD-only session: zero footprint (no constants, no rewrites — the
   detour early-outs on `any_side_latched`).
4. Versus P1 DISTANT / P2 OVERHEAD: P2's glow stays flat/stock (no
   cross-side fallback); center-arrows-1P single: glow tracks the centered
   lane.
5. AA OFF + perspective ON (config permutation): judge container now
   synthesizes with the STOCK judge PS — glow renders stock-looking but
   perspective-correct.
6. Then the outstanding Round-2 tuning item: `distant_focal`/`distant_zoom`
   defaults if desired.

### Round-2 revision (implemented 2026-08-04, awaiting cabinet pass)

- REMOVED INCOMING/SPACE: enum values 3/4 gone (old persisted values clamp to
  DISTANT=2; wire values unchanged), `skew_strength` config key gone,
  generator entries + 4 PNGs deleted. `PerspConstants.cx` retained as a free
  constant (re-adding skew = a table change).
- Receptor realignment `ty` (c49.z): `compute_constants` derives it so the
  mapped receptor row lands at stock height (map_point of pos_y; exactly 0.0
  for HALLWAY — bit-exact regression there). Both persp VS blobs recompiled
  (+1 instr each: 22/23; PS blobs untouched). Consequence: DISTANT maps
  content from beyond the stock 720 cull bound on screen → the draw-distance
  contribution now applies for ANY latched preset (was: negative-tilt only).
- Hit-flash tracking (ALL presets, per maintainer): `pass_rewrite` pre
  publishes each side's resolved `PerspConstants` (flag-last atomics;
  cleared at song boundaries); `lane_hook::apply_one`'s ReceptorFlash branch
  composes playfield scale first, then the shared `map_point` on the clip
  translation + `comp *= sp` on the root-MC component scale. Retry until the
  side's constants publish (first lane pass). `note_result_setup` capture is
  now consumer-refcounted (`FlashConsumer` — the guideline_hook pattern);
  perspective acquires it at enable, drives `lane_scene_transition` from its
  scene callback, and drains the pending queue from its lane pass — all so
  the correction works with playfield_styling config-disabled. ≈Identity for
  HALLWAY (flash sits at the s=1 anchor; small sp pull if its registration
  point is below the row — MORE correct than before, per the maintainer's
  "generic solution for all perspectives").

### Round-2 verification list (maintainer)

Deploy DLL + blobs + PNGs together (the 2 changed blobs regenerate synthesis
via fingerprint; mirror the 4 PNG deletions on the deploy target).

1. Row UI: exactly 3 values now (OVERHEAD/HALLWAY/DISTANT); a profile that
   had INCOMING/SPACE selected loads as DISTANT.
2. HALLWAY regression: pixel-identical lane (ty=0 path); hit flash now
   composed — same-or-better (may pull ≤ a few px/% toward the row).
3. DISTANT: receptor row at ≈stock height (ty≈−57 @ defaults); hit flash
   lands ON the mapped receptors at their scale (≈0.82×); no pop-in at the
   entrance edge (cull contribution now active for DISTANT — latch log shows
   cull=1600); freezes straight; missed notes shrink off past the row.
4. Mixed sides (P1 DISTANT / P2 OVERHEAD): P2's flash untouched.
5. With playfield_styling ALSO scaling (arrow_scale < 100%): flash composes
   both (position+size track the scaled AND mapped receptors).
6. Optional robustness: playfield-styling mod config-disabled → DISTANT flash
   correction still works (perspective-owned capture/drain path).
7. Tune `distant_focal` / `distant_zoom`; commit config.rs defaults if moved.

## Deviations & open questions

- Ran `gen_option_labels.py` via an ephemeral temp-dir venv (no local Pillow);
  maintainer confirmed keeping the outputs. No repo env changes.
- No commits made (repo convention: maintainer commits himself) — working tree
  holds Steps 1–3 + the PDD artifacts, gates green.

## Key facts for a cold resume

- c49.y (z0) is load-bearing for the NEW blobs: DLL emits it every pass (1.0
  for HALLWAY/INCOMING). Never deploy the new blobs with a pre-Step-1 DLL.
- Old-blob failure mode (stale synthesis cache): c49.y ignored → DISTANT/SPACE
  render un-zoomed (~10–40% oversized), visual-only; HALLWAY/INCOMING unaffected.
- Preset table source of truth: `compute_constants` in
  `src/mods/player_perspective/mod.rs`; design §Data Models mirrors it.
- Generator gotcha stands: regenerating labels rewrites ALL PNGs — keep net-new
  only, `git restore` the rest.
