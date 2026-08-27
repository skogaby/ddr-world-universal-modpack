# Research — Shader Replacement Feasibility (pre-completed)

The feasibility research for this project was completed 2026-07-19, before the
PDD project was set up. The authoritative record is the repo-level RE note:

**`docs/shader_replacement_research.md`** — covers:

1. Verdict: shader replacement is feasible and **cabinet-proven** (one-byte
   arrow-PS PoC visibly recolored arrows in gameplay).
2. `.gsp` GSPW container format, fully validated against all 35 shipped files
   (FNV-1 name hash, 3-table layout, raw D3D9 SM3 bytecode blobs).
3. Engine load path (Ghidra, build 20260616): FileManager → GSPW parser
   `FUN_18025ee30` → `FUN_180256150` memcpy → CreateVertexShader/
   CreatePixelShader. No bytecode validation.
4. Delivery: existing LayeredFS arc-overlay path
   (`data_mods/<mod>/arc/shader_arc/data/shader/<name>.gsp`), boot-order safe.
5. Stock arrow VS/PS decoded; the palette pipeline confirmed at bytecode level.
6. Texel-size acquisition options for the index-aware filter (§7):
   - **Option A**: `def` constants 1/768, 1/384 (width 768 shared by both known
     lane sheets; 384 a multiple of 192 → stock-identity preserved).
   - **Option B**: confirm `SamplerParameters` c32.zw is a half-texel offset and
     forward exact texel size from the VS through a spare interpolator.
7. Toolchain options for HLSL → ps_3_0 (fxc on Windows / wine / hand-assembly /
   committed bytecode).

Related earlier record: `docs/playfield_styling_research.md` §7 (why sampler
state cannot fix palette-indexed aliasing — the motivation for this project).
