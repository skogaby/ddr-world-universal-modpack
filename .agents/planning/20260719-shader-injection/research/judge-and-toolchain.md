# Research — Judge Shader + Texture Dims + Toolchain Validation (2026-07-19)

Pre-design research round. Three results, all local (no cabinet deploys).

## 1. `gs_screencommand_judge.gsp` decoded

- **VS blob is byte-identical to the arrow VS** (same 0x190 bytes — position
  passthrough, `mad o1.xy, v1, c32, c32.zwzw`, color × BaseColor).
- **PS is the same palette family with one twist:** palette row V is a
  compile-time constant, not vertex color:
  ```
  def  c0 = (1.0, 0.0, 0.15625, 0.0)      ; V = 0.15625 = palette row 2 center
  texld r0, v0, s0                        ; atlas
  mad   r0.xy, r0.x, c0.xy, c0.zw         ; U = atlas.RED, V = 0.15625
  texld r1, r0, s1                        ; palette
  mul   r1, r1, v1                        ; × vertex color (ALL channels)
  mul   oC0.w, r0.w, r1.w                 ; a = atlas.a × palette.a × vColor.a
  mov   oC0.xyz, r1
  ```
- The index-aware 4-tap treatment applies identically (per-tap palette lookup
  at fixed V, then blend colors).
- `JudgeEffectRenderer` ctor `FUN_180028210` binds the shader; its draw helper
  `FUN_180028490` emits 96×96-px cells normalized by the sheet at `this+0x20`.
- **The judge renderer's sheet is the SAME FileManager entry as the arrow
  base sheet** — statically confirmed in the setup function `FUN_18005cca0`
  (the `arrow%02d` entry pointer is handed to every renderer object,
  including the judge object at `RDI+0x150`). So the judge sheet = 768×192.

## 2. Texture dimensions — all three arrow-shader sheets confirmed

Setup fn `FUN_18005cca0` builds the names; art extracted from the local
install (`data/arc/2d/`):

| Sheet | Names | File | Dims | Renderer slot |
|---|---|---|---|---|
| Arrow/receptor/freeze art (per arrow skin) | `arrow%02d` | `2d_arrow00.arc` → `arrow00.dds` | **768×192** | `+0x20` (arrow, spot, judge renderers) |
| Shock-arrow electric crackle | `shock_effect%02d_%c` (l/m/s) | `2d_shock_effect00.arc` | **768×384** | ArrowRenderer `+0xD0` |
| Lane notice overlay | `lane_notice00` | `2d_lane_notice00.arc` | **192×384** | ArrowRenderer `+0xE0` |

**Correction to earlier labels** (research doc §5/§7 of
`docs/shader_replacement_research.md` + `playfield_styling_research.md` §7):
the live CE session's "+0xD0 arrow atlas 768×384" is actually the
shock-effect sheet, and "+0xE0 electric" is `lane_notice00` (192×384). The
live-measured dims were correct; the semantic labels were guesses.

**Q2 Option A fully verified:** with `du=1/768, dv=1/384`:
- shock sheet 768×384 → exact.
- arrow sheet 768×192 → texel-aligned uv = k/192 ⇒ uv·384 = 2k integral ⇒
  frac=0 at 1:1 → stock-identical; scaled = half-radius blend (crisper).
- lane notice 192×384 → uv·768 = 4k integral ⇒ same collapse ⇒
  stock-identical at 1:1; scaled = quarter-radius blend horizontally.
768 is a common multiple of {768,192} and 384 of {192,384} — the collapse
argument holds for every sheet either shader binds. No caveats remain.

## 3. Toolchain validated — Docker + vkd3d-compiler 1.14 (Fedora 42)

- Debian trixie ships vkd3d **1.2** (no HLSL frontend) — unusable.
- **Fedora 42 ships `vkd3d-compiler` 1.14** — full `hlsl` source type with
  `d3dbc` (SM1–3) target, plus `d3dbc → d3d-asm` disassembly. Runs natively
  on linux/arm64 in Docker (no emulation on Apple Silicon).
- Verified end-to-end:
  1. Reconstructed stock arrow PS in HLSL → compiles to valid `ps_3_0`
     d3dbc.
  2. Disassembling the **stock** PS blob with vkd3d reproduced the hand
     decode exactly (independent confirmation of the bytecode analysis).
  3. A draft 4-tap index-aware bilinear PS compiles clean to ps_3_0
     (~2 KB blob, no flow control — `texld`s unconditionally executed, no
     gradient ops).
- **Caveat:** vkd3d's d3dbc codegen is unoptimized (~5–10× the instruction
  count fxc produces, mostly redundant `mov`s). Irrelevant for GPU load at
  this workload (a handful of quads per frame at most against hardware that
  runs the full game), but worth remembering if a future shader is hot.
- Command shape:
  `vkd3d-compiler -x hlsl -b d3dbc -e main --profile ps_3_0 in.hlsl -o out.d3dbc`

## Design inputs settled by this round

1. Judge PS variant: same 4-tap core; V comes from `c0` constant instead of
   `v1.x` (and color multiply covers RGB too, matching stock).
2. `TEXEL = (1/768, 1/384)` is safe for every bound sheet (collapse argument
   above) — Q2 Option A stands with no residual risk.
3. Docker image: `fedora:42` + `vkd3d-compiler` package. No custom image
   build strictly needed (a `docker run` with a dnf install works, though
   caching a built image locally will make `build_shaders.sh` fast).
