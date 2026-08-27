# Summary — Shader Fixes (Index-Aware Bilinear Arrow/Judge Shaders)

Date: 2026-07-19

## Artifacts created

```
.agents/planning/20260719-shader-injection/
├── rough-idea.md                    — motivation + pre-answered feasibility
├── idea-honing.md                   — 7-question requirements Q&A (all decided)
├── research/
│   ├── feasibility.md               — pointer to docs/shader_replacement_research.md
│   └── judge-and-toolchain.md       — judge PS decode, texture dims, vkd3d validation
├── design/
│   └── detailed-design.md           — approved design (shader source, packer, build,
│                                      risks + contingencies, fxc golden-path note)
├── implementation/
│   └── plan.md                      — 6-step plan with checklist
├── progress.md                      — live resume point (AGENTS.md convention)
└── summary.md                       — this file

docs/shader_replacement_research.md  — repo-level RE record (container format, load
                                       path, PoC, key addresses) — written pre-PDD
```

## Design at a glance

Replace `gs_screencommand_arrow` + `gs_screencommand_judge` pixel shaders
with index-aware bilinear variants (4 atlas taps → palette-lookup each →
blend the COLORS), fixing the scaled-playfield aliasing that sampler state
provably cannot (palette-indexed art). Delivery is pure `data_mods/` content
via the existing LayeredFS arc overlay — no DLL code, no hooks, no config.
Texel size hardcoded (1/768, 1/384), proven exactly stock-preserving at 1:1
for every sheet either shader binds. Built from in-repo HLSL via Docker +
vkd3d-compiler (native arm64), packed by a self-verifying Python GSPW
packer; final .gsp artifacts committed. Windows fxc "golden path" recorded
as the perf fallback (cabinet iGPUs are ~2015-class; vkd3d codegen is
unoptimized but far below noise at this workload).

## Implementation approach

1. `scripts/gsp_pack.py` (pack/inspect/selftest)
2. Docker toolchain + `scripts/build_shaders.sh`
3. Arrow shader → validated committed .gsp
4. Judge shader → same
5. Cabinet acceptance pass (single deploy expected; UV_BIAS contingency ready)
6. Reproducibility check + docs + cleanup

## Next steps

- Commit when ready (nothing committed yet).
- Optional future refinement: `bilinear-sharp` variant (Q6 deferred) for
  crisper upscales if the residual softness at high scale matters.

## Status

Steps 1–6 complete; cabinet-accepted 2026-07-19 (P1 50 % / P2 125 %:
markedly reduced aliasing, no banding; 100 % unchanged from stock). Docs
updated. Awaiting commit go-ahead.

## Areas that may need refinement

- Risk §6.2 (1:1 uv center vs edge alignment) resolves only at the cabinet
  spot-check or a CE read — the design carries a one-line contingency.
- Freeze-body tiling seams at scale (risk §6.3) — observe at 50 %.
