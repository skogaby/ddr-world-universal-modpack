# Summary — Custom Resolution (arbitrary resolution rendering)

2026-09-05. PDD run from rough idea to approved plan, then implementation through plan
Step 3 (the first cabinet checkpoint). Live state: `progress.md`.

## Artifacts

| file | role |
|---|---|
| `rough-idea.md` | the maintainer's ask (16:9 1080p/1440p/4K + 4:3 SD; no asset rework) |
| `research/orientation.md` | codebase impact inventory, precedents, blind spots |
| `research/sd-cabinet-path.md` | Ghidra: machine-type fan-out, no SD boot gate, present-mode call sites, ark force-SD flag |
| `research/engine-findings.md` | H1 refuted (bm2d VS uses c50–c53), `sys_copy` shape, spice2x source, four-build AOB sweep |
| `prototypes/shader_dump/`, `prototypes/aob_sweep/` | raw evidence (disassemblies, CTABs, byte dumps, `sweep.py`) — never source |
| `idea-honing.md` | 20-decision register, accepted wholesale; Readiness Confirmed |
| `design/detailed-design.md` | self-contained design (Approved) |
| `implementation/plan.md` | 8 steps with checklist (Approved); Steps 1–2 done, 3 implemented |
| `progress.md` | resume point + cabinet checkpoint script |

Code: `src/mods/custom_resolution/{mod,plan,sites,patches,present,display_modes}.rs`,
`src/core/signatures.rs` (5 AOBs + `CustomResolutionAnchors`), `src/mods/config.rs`
(`ResolutionConfig`), `src/lib.rs`, `mod-config.json`, `scripts/validate_custom_resolution.sh`.
Working records: `.agents/scratchpad/2026-09-05-arbitrary-resolution/step0{1,2,3}-*/`.

## Design in one paragraph

The engine's logical 1280×720 canvas is independent of the physical render target, so
resolution support is boot-time byte patching of the immediates the game reads once
(back-buffer selector, surface ctor hoists + RT-struct dims, list-viewport table,
letterbox source rect) plus three detours: `graphics_init` (PRESENT rt dims → output,
depth policy), `letterbox_rect_fn` (present-mode policy, re-asserted per scene), and the
tag-0x0C scissor handler (canvas → RT px). `render` and `output` are independent; the
engine's own `StretchRect` scales when they differ, which is also how 4:3 SD output works
(render 720p, present cropped/letterboxed into 640×480 — the path SD cabinets shipped
with; only the SD data was discontinued). Root 7's `ScreenRoot` is re-canvased to 1280×720
so the modpack UI keeps its coordinates. Everything fails open to stock with one WARN;
the fullscreen fail-safe validates the mode before any patch lands.

## Next steps

1. **Maintainer:** cabinet checkpoint #1 per `progress.md` (SD 640×480, CrossOver window).
2. Resume at plan Step 4 (root-7 re-canvas + overlay rows + README), then 5 (letterbox
   policy → Tier-A 1080p/4K from a 720p render), 6 (native render + scissor), 7 (depth
   replacement + AFP projection redirect), 8 (docs/AGENTS.md/learnings).
3. Decide Phase 2 (shader scaler on the present pass) after judging Tier-A softness.

## Assumptions to keep an eye on

- H2 (depth larger than colour is legal on retail D3D9 — SD precedent) covers SD and
  native render; render < output waits for Step 7.
- Root-7 re-canvas also moves the game's loading-screen art — verify at checkpoint #2.
- D3DMetal fill rate at 4K is the operator's problem (FPS-unlock precedent), not gated.
