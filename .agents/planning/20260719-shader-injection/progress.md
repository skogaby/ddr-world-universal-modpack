# Progress — Shader Fixes (Index-Aware Bilinear Arrow/Judge Shaders)

Updated: 2026-07-19
Status: Steps 1–6 done (cabinet-accepted, docs updated) — awaiting commit go-ahead
NEXT ACTION: Commit when the maintainer asks. Open decision: fate of the
untracked `shader_arc_unpacked/` (reference material — keep local or remove;
do NOT commit without asking). Optional future refinement: `bilinear-sharp`
variant for crisper upscales (Q6 deferred) if the residual softness matters.

Resume protocol: read `implementation/plan.md` (step checklist),
`design/detailed-design.md` (authoritative mechanisms), and the research
records (`research/feasibility.md`, `research/judge-and-toolchain.md`,
repo-level `docs/shader_replacement_research.md`). Cabinet deploys/tests are
done by the MAINTAINER; the shader build is local
(`./scripts/build_shaders.sh`, Docker required only when shaders change).

## Done

- Feasibility proven on cabinet (2026-07-19): one-byte arrow-PS PoC via the
  LayeredFS arc overlay — modified bytecode visibly rendered in gameplay.
  Full RE record: `docs/shader_replacement_research.md` (container format,
  load path, boot order, stock shader decode, key addresses).
- The PoC run doubled as the playfield-styling v11 regression check (no
  LANE-DIAG lines, all gates green).
- PDD requirements clarification complete (7 questions, `idea-honing.md`):
  arrow+judge scope; hardcoded TEXEL=(1/768,1/384); Docker vkd3d toolchain
  with committed artifacts; math-gate verification; always-on
  `data_mods/shader_fixes/`; plain bilinear; 5 acceptance criteria.
- Pre-design research (`research/judge-and-toolchain.md`): judge PS decoded
  (fixed palette row V=0.15625, full vertex-color multiply; same sheet as
  the arrow renderer); all three bound sheets' dims confirmed from install
  data (768×192, 768×384, 192×384) → TEXEL constants safe for all
  (common-multiple collapse); toolchain validated end-to-end
  (fedora:42 `vkd3d-compiler` 1.14 compiles HLSL→ps_3_0; its disassembler
  independently reproduced the stock-PS hand decode; draft 4-tap PS
  compiles clean).
- Detailed design written and maintainer-approved
  (`design/detailed-design.md`), incl. the vkd3d-unoptimized-codegen note +
  Windows fxc golden-path fallback (§8.1) and two known-risks with
  contingencies (§6.2 UV_BIAS, §6.3 cell-edge bleed).
- Implementation plan written (`implementation/plan.md`, 6 steps).

## In flight

- Awaiting maintainer commit go-ahead. Step 6 docs done: README "Shader
  Fixes" section, AGENTS.md key-entry row, `docs/shader_replacement_research.md`
  §7 marked implemented + §8 toolchain rewritten + §5 texture-label
  correction, `docs/playfield_styling_research.md` §7 accepted-characteristic
  superseded.

## Deploy & test log

- 2026-07-19 (Step 5, cabinet — ACCEPTED): 2nd boot after updating
  `data_mods/`. Log clean: `shader_fixes` found, `arc cache up to date` →
  `using ./data_mods/_cache/arc/shader.arc`, no shader/D3D errors. 100 %
  regression looked stock (no change). Versus P1 50 % / P2 125 %
  (`capture/capture_20260719_175636.jpg`): significantly reduced aliasing on
  arrows/receptors and the judge text ("Marvelous!!!", "17 COMBO!!!"), no
  banding. Maintainer: "not perfect but significantly improved" — expected
  residual = inherent softness of 4-tap bilinear upscaling low-res palette
  art (esp. P2 125 % diagonals). `bilinear-sharp` (Q6) remains an available
  drop-in refinement if crisper upscales are wanted later.
  - Cosmetic: log reports `shader_fixes (6 files)` vs our 2 `.gsp` — almost
    certainly `.DS_Store` from the macOS→cabinet copy; harmless (can't match
    arc entry names).

- 2026-07-19 (CE, risk §6.2 resolution): froze the game in attract-mode
  autoplay (all-stock scale) and read the gs command buffers. c32
  (SamplerParameters) = identity (1,1,0,0); arrow quads integer-positioned,
  edge-aligned uv rects → per-pixel samples hit texel CENTERS at 1:1 →
  `UV_BIAS = 0` (NOT +0.5 — an earlier mis-derivation, corrected before any
  deploy). Recorded in `docs/shader_replacement_research.md` + design §6.2.
- 2026-07-19 (Steps 1–4 local build): `gsp_pack.py` selftest + stock-file
  inspect pass; toolchain image built (fedora:42 vkd3d 1.14);
  `build_shaders.sh` produces both `.gsp` with correct stock name hashes
  (arrow 0x9E93AC7B, judge 0x3489F7B5); disassembly review clean (VS reads
  only c32/c22; PS 8 texld, no flow control, no gradient ops); rebuild is
  bit-identical (deterministic). NOT yet deployed.

## Deviations & open questions

- Risk §6.2 (1:1 uv alignment) is unconfirmed until the Step 5 cabinet pass
  (or an optional CE read of VS c32 beforehand). Contingency is a one-line
  `UV_BIAS` change.
- `shader_arc_unpacked/` in the repo root is UNTRACKED maintainer-provided
  reference material (unpacked stock shader.arc). Step 1's stock-inspection
  test should prefer extracting from the local install
  (`~/Desktop/Miscellaneous/DDR WORLD/MDX-003_20260324/contents/data/arc/
  shader.arc` via `scripts/unpack_arc.py`) so nothing depends on the
  untracked folder. Ask the maintainer before committing/removing it.

## Key facts for a cold resume

- Feature dir: `.agents/planning/20260719-shader-injection/`.
- Deliverables: `shaders/src/gs_screencommand_{arrow,judge}.hlsl`,
  `scripts/gsp_pack.py`, `scripts/build_shaders.sh`, committed
  `data_mods/shader_fixes/arc/shader_arc/data/shader/*.gsp`. NO Rust/DLL
  changes.
- GSPW container + engine load path: `docs/shader_replacement_research.md`
  (name hash = FNV-1 of bare shader name; engine ignores CTAB — register
  binds are the contract: VS c32/c22, samplers s0 atlas / s1 palette).
- Stock output contracts: arrow PS rgb = palette.rgb (vertex color rgb NOT
  multiplied), a = palette.a·atlas.a·vColor.a, palette row = vColor.x;
  judge PS multiplies full vertex color, palette row fixed 0.15625.
- Toolchain: docker image from fedora:42 + vkd3d-compiler 1.14;
  `vkd3d-compiler -x hlsl -b d3dbc -e ps_main --profile ps_3_0 …`;
  disasm oracle: `-x d3dbc -b d3d-asm`. fxc golden path if perf ever
  matters (design §8.1).
- Cabinet GPUs are low-end (~2015 iGPU class) — the reason the fxc fallback
  is recorded.
