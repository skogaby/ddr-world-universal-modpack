# Summary — Background Movies on the stage screens

## Artifacts

| File | Contents |
|---|---|
| `rough-idea.md` | the maintainer request |
| `research/orientation.md` | RE facts added during planning (entry-10 canvas + walk gate, fit fields, both AOBs, UV convention), the modpack code map, the Griffin House source |
| `idea-honing.md` | decision register D1–D17, all Accepted; `Readiness Confirmed 2026-09-23` |
| `design/detailed-design.md` | approved design (R1–R16, components, data models, error handling, tests, RE appendix) |
| `implementation/plan.md` | approved 7-step plan with checklist |

The underlying reverse engineering is `docs/background_dancers_research.md` §8.

## Design in brief

Two new values on the Background Dancers "Background Movies" row. **STAGE SCREENS**: when the song's stage has
screens (its arc lists an `offscreen1.dds` member — the ten stock monitor/replicant stages, plus any custom
stage whose screen image is named `offscreen1`), one byte of the MovieActor's layer choice is patched `09 → 0A`
for the song so the movie draws into the 1280² `offscreen1` render target that the screen materials already
sample, VIDEO SIZE is written FULLSCREEN, and the fit rectangle is set to A3's (0,0,1280,1280); otherwise the
song plays as THUMBNAIL. **MOVIE ONLY (NO DANCERS)**: the whole 3D scene hides while a movie is really drawn.
Screen materials stay unlit and outline-free everywhere. The Griffin House TV becomes a screen as the
custom-content proof of concept.

## Next steps

1. Run the code-task-generator sop against `implementation/plan.md` (one step at a time), then code-assist on
   each task in order — or implement the steps directly, keeping `progress.md` current.
2. Cabinet deploys after Steps 2, 3, 4 and 6; the full design §7.3 list in Step 7.

## Assumptions to watch

- The whole routing chain is static evidence only — Step 2's first deploy is the real test (screens should be
  BLACK before it, per §8.4 of the RE doc).
- The screen texture hash is assumed to be FNV-1 of the folded name (same hasher as the model registry); the
  Step 3 texture-size log line confirms it.
- Custom Resolution resizes the render target to `render_w²`; expected transparent, unverified.
