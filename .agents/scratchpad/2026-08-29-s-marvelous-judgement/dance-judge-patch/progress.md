# Progress — step04 task-02 dance_judge asset synthesis and AFP patch

Updated: 2026-08-30
Status: done

## Checklist

- [x] 1. Baseline validate run (logs/00-baseline.log)
- [x] 2. core/geo.rs — GE2D label module (length-changing rebuilds) + harness mount
- [x] 3. core/ap2 recipe `clone_word_segment_with_new_shape` + tests
- [x] 4. folder_expansion wrapper over core::geo
- [x] 5. s_marvelous afp_patches.rs + assets.rs + mod.rs wiring
- [x] 6. validate script: ap2check `smarv-patch`/`geo-rewrite` modes + Leg D render proof
- [x] 7. Gates: validate green, cargo check msvc clean, cargo fmt clean, build.sh clean

## TDD record

1. Baseline — logs/00-baseline.log: exit 0; 73 lib + 63 bin tests, Legs A
   (76/76 byte-identical) / B / C green.
2. logs/01-geo-green.log: `src/core/geo.rs` landed with its 8-test suite
   (both endians, obfuscated+plain, equal/shorter in-place vs a verbatim
   copy of the LEGACY folder_expansion algorithm as the byte-identity
   oracle, longer append+repoint+filesize, aliased-string safety,
   multi-label mixed lengths, failure battery) — 81 lib tests green.
   NOTE: tests and implementation were authored together in one file (no
   separate RED commit point); the legacy-oracle tests pin the promoted
   behavior against the pre-existing shipped implementation, and an ad-hoc
   probe ran the rewriter on the REAL `dance_judge_shape32` (176→203
   bytes) with bemaniutils' filesize-validating `Shape.parse()` as the
   oracle — region resolved to `dance_judge0000_smarvelous`.
3. logs/02-recipe-green.log: the core/ap2 recipe + 4 tests
   (`edit_word_segment_recipe_*`: happy path with dynamic-id assertions +
   dictionary-singularity + fixed point; byte-identity vs the hand-driven
   task-01 primitive sequence; failure battery incl. byte-identical doc
   after every pre-mutation refusal; ambiguity fixture fails closed) —
   85 lib / 67 bin green, Legs A/B/C green.
4. logs/03-legd.log: Leg D end-to-end on the REAL template — chain
   resolved dynamically (shape 32, region `dance_judge0000_marvelous`),
   recipe allocated shape 54 / sprite 55 (§10's exact prediction, derived
   not hardcoded), geo rewrite 176→203 bytes, bemaniutils re-parse OK,
   38 non-blank frames rendered. Post-render color audit confirmed the
   violet placeholder palette in the frames (through the game's own
   timeline color transforms).
5. logs/04-cargo-check-msvc.log: `cargo check --target
   x86_64-pc-windows-msvc` clean, zero warnings.
6. logs/05-final-green.log (post-`cargo fmt`, which produced no changes —
   `cargo fmt -- --check` exit 0): 85 lib + 75 bin tests, Legs A/B/C/D
   green. Preview: `${TMPDIR}/s_marvelous_preview/in_smarvelous_patched.gif`.
7. logs/06-build.log: `./build.sh` release build clean.

## Deviations & open questions

- **Geo loading path is unverified on cabinet** (design Appendix B item):
  the stock loader loads geos per the afplist `<geo>` index list at IFS
  mount; whether the runtime ALSO opens `dance_judge_shape54` on demand
  when the patched template instantiates shape 54 is only provable live.
  The task-specified mechanism (`register_afp_geo_mapping` + mod geo file)
  is implemented; if the cabinet shows the shape-name lookup failing, the
  fallback is an `afplist.merged.xml` for the dance_judge IFS — NOT done
  here because a duplicate `<afp name="dance_judge">` entry risks
  double-registering the template (xml_merger appends children; it cannot
  edit the existing entry). For task-03's cabinet deploy checklist.
- **1px composite offset (shared machinery, pre-existing):** atlas_cloner's
  donor-slot placement composites the PNG at the IMGRECT origin while the
  game samples the UVRECT (inset 1px) — the 344×61 art (uvrect-sized) will
  render shifted up-left by 1px with a 1px crop at the far edges, same as
  every folder_expansion clone ever shipped. Cosmetically invisible;
  noted, not fixed (shared code, out of scope).
- **Skin gate is byte-equality, not IFS identity** (the seam carries only
  bytes): if a skin-suffixed IFS ships a byte-identical `dance_judge`
  template, the patch applies there too but the injected geo/region only
  resolve for the 0000 IFS — the cloned word degrades to invisible on that
  skin. v1 scope accepts this (task requirement 3); WARN fires only for
  byte-DIFFERENT variants.
- **`patch_applied()` stays latched across disable** (deliberate): a
  template already streamed patched remains patched in game memory;
  task-03 must gate on `patch_applied() && mod active`.
- **Recipe is non-atomic on failure** (documented on the fn): `None` may
  leave the doc partially edited — callers discard the doc (the patch fn
  parses per call; the common refusals are pre-checked before mutation and
  covered by byte-identity tests).
- **Staging runs once per boot** (`STAGED` kept across disable/enable):
  inputs (arc + PNG) cannot change mid-session; re-enable just re-arms
  `PATCH_READY`.

## Files created/changed

- `src/core/geo.rs` — NEW std-only GE2D label module (`labels`,
  `rewrite_labels` with length-changing rebuilds) + 8 host tests.
- `src/core/mod.rs` — register `pub mod geo`.
- `src/core/ap2/edit.rs` — `WordSegmentClone` +
  `Ap2Doc::clone_word_segment_with_new_shape` (the §10 recipe, dynamic id
  resolution, ambiguity fail-closed).
- `src/core/ap2/mod.rs` — re-export `WordSegmentClone`.
- `src/core/ap2/tests.rs` — 4 recipe tests.
- `src/mods/folder_expansion.rs` — `patch_ge2d_labels` now a thin wrapper
  over `core::geo::rewrite_labels` (equal/shorter byte-identical; longer
  keys now rebuild instead of silently truncating).
- `src/mods/s_marvelous/assets.rs` — NEW enable-time staging: arc/IFS
  extraction, descramble, word-chain resolution, recipe dry-run (id
  precompute), geo rewrite + write + `register_afp_geo_mapping`,
  donor-anchored cache-guarded atlas clone, mod-paths rescan-once.
- `src/mods/s_marvelous/afp_patches.rs` — NEW patch registration/fn +
  `patch_ready()`/`patch_applied()` + latched WARNs + v1 skin gate.
- `src/mods/s_marvelous/mod.rs` — module decls + `afp_patches::activate()`
  in enable / `deactivate()` in disable.
- `scripts/validate_s_marvelous.sh` — geo module mount; ap2check
  `smarv-patch` + `geo-rewrite` modes; Leg D (real-recipe render proof →
  `${TMPDIR}/s_marvelous_preview/in_smarvelous_patched.gif`).

## API surface for task-03

- `s_marvelous::afp_patches::patch_ready() -> bool` — assets staged +
  patch registered + mod enabled (cleared on disable).
- `s_marvelous::afp_patches::patch_applied() -> bool` — the patch fn
  produced output this session (latched; survives disable — combine with
  the mod's active state).
- The new segment label is `assets::NEW_LABEL` (`"in_smarvelous"`); the
  template key is `assets::TEMPLATE_NAME` (`"dance_judge"`). The label
  frame is discoverable at runtime the same way the stock handler does it,
  or numerically: the placements-only clone appends the segment at the END
  of the root timeline (Leg D: label_frame=600 on the stock 600-frame
  template — but always resolve by label, not by constant).

Status: Complete (uncommitted — maintainer commits manually)
