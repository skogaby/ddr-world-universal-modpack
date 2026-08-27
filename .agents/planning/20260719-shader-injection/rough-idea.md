# Rough Idea — Shader Injection / Replacement

## Motivation

1. **Fix the scaled-arrow aliasing properly.** The playfield-styling mod scales
   gameplay arrows/receptors; scaled art shows nearest-texel pixelation that
   CANNOT be fixed with sampler states (proven — the lane art is
   palette-indexed; see `docs/playfield_styling_research.md` §7). The real fix
   is replacing the `gs_screencommand_arrow` pixel shader with an
   **index-aware bilinear** variant: sample the 4 neighboring atlas texels,
   palette-lookup EACH, then blend the resulting COLORS.
2. **Unlock shader-level mods generally** — establish shader replacement as a
   reusable mod class for this codebase.

## Feasibility status (pre-answered before this PDD project)

Feasibility was proven on 2026-07-19 with a cabinet-confirmed proof of concept
(one-byte R↔G swizzle patch to the arrow PS, delivered via the existing
LayeredFS arc-overlay path — no new hooks, no DLL changes). The full RE record
lives in `docs/shader_replacement_research.md`:

- `.gsp` GSPW container format fully solved (35/35 files parse)
- Engine load path verified in Ghidra (bytecode memcpy'd unvalidated to
  CreateVertexShader/CreatePixelShader)
- Boot order verified (LayeredFS hooks install ~1 s before `shader.arc` opens)
- Stock arrow VS/PS decoded from bytecode; vertex UVs are normalized;
  texel-size acquisition options identified (§7 of the research doc)

## What this project designs

The production version: the index-aware bilinear arrow PS (and whatever
packaging/toolchain/config surface it should have), shipped as a proper mod.
