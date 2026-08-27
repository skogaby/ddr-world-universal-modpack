# Implementation Plan — Shader Fixes (Index-Aware Bilinear Arrow/Judge Shaders)

Design: `../design/detailed-design.md` (authoritative for all mechanisms).
Progress tracking: `../progress.md` (live resume point, per AGENTS.md).

## Checklist

- [x] Step 1: GSPW packer/inspector (`scripts/gsp_pack.py`)
- [x] Step 2: Docker toolchain + `scripts/build_shaders.sh` skeleton
- [x] Step 3: Arrow shader (HLSL + build + local validation)
- [x] Step 4: Judge shader (HLSL + build + local validation)
- [x] Step 5: Cabinet deploy + acceptance pass
- [ ] Step 6: Reproducibility check, docs, cleanup

---

## Step 1: GSPW packer/inspector (`scripts/gsp_pack.py`)

**Objective:** A compiler-agnostic Python tool that wraps VS+PS d3dbc blobs
into a GSPW `.gsp` and can inspect/validate any `.gsp`.

**Guidance:** Implement per design §4.3/§5.1. Two modes:
- `--pack`: `--name <shader> --vs <blob> --ps <blob> -o <out.gsp>` — emits the
  stock header shape (counts (1,1,1), zeroed program entry, 16-byte-aligned
  blobs), FNV-1 name hash. Self-verifies before writing (re-parse, blob
  version tokens `0xFFFE03xx`/`0xFFFF03xx`, hash equality with FNV-1(name)).
- `--inspect <file.gsp>`: dump header fields, table entries, blob kinds —
  the validation rules are the ones proven against all 35 stock files.

**Tests (write alongside):** a `--selftest` mode that packs synthetic blobs,
re-inspects, and asserts every rule; plus inspecting the stock
`gs_screencommand_arrow.gsp`/`_judge.gsp` from the local install
(`data/arc/shader.arc` via `scripts/unpack_arc.py`) and asserting the parsed
fields match the research doc's layout.

**Integration:** standalone script; consumed by Step 2's build script.

**Demo:** `python3 scripts/gsp_pack.py --inspect <stock arrow .gsp>` prints
the validated layout; `--selftest` passes.

---

## Step 2: Docker toolchain + `scripts/build_shaders.sh` skeleton

**Objective:** One-command, reproducible HLSL → `.gsp` pipeline.

**Guidance:** Per design §4.4. Local image `ddr-shader-build` built from
`fedora:42` + `vkd3d-compiler` + `python3` (Dockerfile under `shaders/`).
Script asserts the pinned vkd3d version (1.14) and fails loudly on drift.
Manifest table in the script maps shader name → HLSL file. Compile both
entry points (`vs_main`/`ps_main`) per file, pack via `gsp_pack.py` into
`data_mods/shader_fixes/arc/shader_arc/data/shader/`, print sha256 per
output. Header comment documents the Windows fxc golden-path fallback
(design §8.1).

**Tests:** `--check` mode that builds/validates the image, asserts the
compiler version, compiles a trivial throwaway PS to ps_3_0 and disassembles
it (round-trip smoke). Run it.

**Integration:** consumes Step 1's packer; produces the committed artifacts
of Steps 3–4.

**Demo:** `./scripts/build_shaders.sh --check` passes on a machine with
Docker (no game data needed).

---

## Step 3: Arrow shader (HLSL + build + local validation)

**Objective:** `shaders/src/gs_screencommand_arrow.hlsl` (VS + index-aware
bilinear PS per design §4.1) built into a validated committed `.gsp`.

**Guidance:** Copy the design's shader source. Contractual details:
`register(c32)`/`register(c22)` on the VS constants, `s0`/`s1` samplers,
`POSITION/TEXCOORD0/COLOR0` semantics, `TEXEL=(1/768, 1/384)`,
`UV_BIAS=(0,0)`, per-tap alpha, vertex-alpha-once output contract.

**Tests:**
1. Build → `gsp_pack` self-verify passes; name hash equals the stock file's.
2. Disassemble both blobs (`vkd3d-compiler -x d3dbc -b d3d-asm`) and review:
   VS must reference c32/c22 and only them; PS must contain exactly 8
   `texld`s (4 atlas + 4 palette), no flow control, no gradient ops.
3. Re-inspect the final `.gsp` with `--inspect`.

**Integration:** first real manifest entry in `build_shaders.sh`; output
lands in the Step 5 deploy folder.

**Demo:** committed
`data_mods/shader_fixes/arc/shader_arc/data/shader/gs_screencommand_arrow.gsp`
passing all local validation.

---

## Step 4: Judge shader (HLSL + build + local validation)

**Objective:** Same for `gs_screencommand_judge.hlsl`.

**Guidance:** Design §4.2 — palette row V is the literal `0.15625`; blended
color multiplies the FULL vertex color (rgba), matching the stock contract.

**Tests:** as Step 3 (self-verify, hash equality, disassembly review — 8
`texld`s, the 0.15625 literal present in a `def`).

**Integration:** second manifest entry.

**Demo:** both `.gsp` files built, validated, committed-ready.

---

## Step 5: Cabinet deploy + acceptance pass

**Objective:** Prove the design's §7 checklist on hardware.

**Guidance:** Maintainer copies `data_mods/` wholesale (shader_fixes rides
along; add to `layeredfs.allowlist` if one is configured). One deploy
expected. Test order:
1. **100 % identity** (risk §6.2 gate): arrows, receptors, freeze, shock
   arrows, judge text, hit flash — visually identical to stock. If uniformly
   SOFT instead → uvs are edge-aligned → flip `UV_BIAS` to ±0.5, rebuild,
   redeploy (one-line contingency).
2. **Scaled** (playfield-styling 50 % and 150 %; overlay-element scale for
   judge family): smooth edges, NO palette banding, freeze-body seams clean
   (risk §6.3 watch), shock crackle coherent.
3. **Perf sanity:** dense chart, no stutter (design §8.1 note — if the
   ~2015-iGPU cabinet ever shows PS cost, the fxc golden path is the
   fallback).

**Tests:** the checklist itself + boot log confirming
`arc: regenerating cache for arc/shader.arc` / `using ./data_mods/_cache/…`
on first boot with the new files.

**Integration:** validates Steps 1–4 end-to-end on the real pipeline.

**Demo:** side-by-side cabinet captures: stock vs scaled-with-fix.

---

## Step 6: Reproducibility check, docs, cleanup

**Objective:** Acceptance items 4–5; leave the repo coherent.

**Guidance:**
1. Clean rebuild (`build_shaders.sh`) → sha256 must match the committed
   `.gsp` files (acceptance #4).
2. Docs:
   - README: LayeredFS/mod-table note for `shader_fixes` (what it fixes,
     always-on, how to disable).
   - AGENTS.md: Key Entry Points row (shader fixes → `shaders/src/`,
     build script, research doc).
   - `docs/shader_replacement_research.md`: add the final shader design
     (§7 → implemented), correct the §5 texture labels (+0xD0 = shock sheet,
     +0xE0 = lane_notice), add the judge decode facts.
   - `docs/playfield_styling_research.md` §7: update the "accepted
     characteristic" note to point at the shipped fix (and the same label
     correction).
3. `progress.md` final update; readiness gates (`cargo check` + `cargo fmt`
   + `./build.sh` are unaffected — no Rust changes — but run `cargo fmt`
   bare if any Rust was touched); commit only when the maintainer asks.

**Tests:** the sha256 comparison IS the test; docs reviewed by maintainer.

**Demo:** fresh-clone + Docker → `build_shaders.sh` → identical hashes;
docs tell the whole story.
