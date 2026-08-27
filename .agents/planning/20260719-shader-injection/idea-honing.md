# Idea Honing — Shader Injection

Requirements clarification Q&A. One question at a time; answers recorded as
decided.

## Q1: Scope — what does this project ship?

Three tiers are on the table (not mutually exclusive long-term, but the project
needs a defined boundary):

- **(a) Arrow fix only** — one replacement `gs_screencommand_arrow.gsp` with the
  index-aware bilinear PS, shipped as a static `data_mods/` asset. No DLL code,
  no config, no framework. Smallest possible surface.
- **(b) Arrow + judge fix** — same, but also treat
  `gs_screencommand_judge.gsp` (judge/hit-flash renderer, same palette family)
  if its aliasing is visible at scale.
- **(c) A shader-mod framework** — DLL-side support (e.g. a mod that manages
  shader overlays, toggles them via the overlay menu / config, maybe validates
  GSPW containers at boot). Establishes "shader mods" as a first-class mod
  class.

**Answer:** **(b) Arrow + judge fix.** Ship replacement
`gs_screencommand_arrow.gsp` AND `gs_screencommand_judge.gsp` with index-aware
bilinear PS variants, as static `data_mods/` assets. No DLL framework this
project (can come later if more shader mods materialize).

## Q2: Texel-size strategy — Option A (hardcoded) or Option B (exact per-draw)?

From `docs/shader_replacement_research.md` §7:

- **Option A — hardcoded `def` constants** (`du=1/768`, `dv=1/384`): PS-only
  change, self-contained. Width 768 is shared by both known lane sheets, and
  384 is a multiple of 192, so stock-identity at aligned scales holds for both;
  the 768×192 sheet just gets a slightly tighter blend radius (softer, never
  banded). Risk: a texture with width ≠ 768 bound to these shaders would break
  stock-identity.
- **Option B — exact per-draw texel size**: confirm `SamplerParameters`
  (c32).zw is a half-texel offset (one CE session or diagnostic shader), then
  the VS forwards exact `(1/W,1/H)` through a spare interpolator. Correct for
  every texture, but touches the VS too and needs the confirmation step first.

**Answer:** **(A) Hardcoded `def` constants** (`du=1/768`, `dv=1/384`).
PS-only change. Upgrade path to (B) stays open — only the source of `du/dv`
changes, the filter core is identical. Verify the judge sheet's dimensions as
part of the work.

## Q3: Toolchain — how do the replacement shaders get built?

The PS must be compiled/assembled to ps_3_0 bytecode and wrapped in the GSPW
container. Options:

- **(a) Committed artifacts + generator script** — write the shader in HLSL (or
  asm), compile once, commit BOTH the source and the final `.gsp` to the repo
  (e.g. under `data_mods/` or an assets dir). A small Python script wraps
  bytecode → GSPW. Rebuilding requires a Windows box or wine+fxc, but that's
  rare (only when the shader logic changes).
- **(b) wine + fxc build step** — `scripts/` gains a shader-build script that
  runs fxc under wine on macOS; `.gsp` is a build product, not committed.
  Reproducible but adds a wine dependency to the dev environment.
- **(c) Hand-assembled bytecode in the generator** — no fxc at all; the Python
  script emits hand-authored SM3 token streams. Zero external deps but hardest
  to maintain/review.

**Answer:** **Hybrid (a)+(b), Docker-backed with vkd3d-compiler.**
- HLSL source lives in-repo (`shaders/src/*.hlsl`), one file per shader.
- `scripts/build_shaders.sh` compiles at-will via a Docker image running
  **vkd3d-compiler** (Wine project's native HLSL→d3dbc compiler, supports
  ps_3_0/vs_3_0, plain C, runs natively on linux/arm64 — no wine, no fxc.exe,
  no x86 emulation on Apple Silicon).
- A small Python packer wraps bytecode → GSPW container (format fully solved)
  and drops the `.gsp` into the mod folder under `data_mods/`.
- Final `.gsp` artifacts are ALSO committed, so cabinet deploys (wholesale
  `data_mods/` copy) never depend on Docker.
- Safe because the engine ignores CTAB and binds by register convention — any
  conforming SM3 compiler works; output is verifiable by local disassembly
  (and optionally cross-checked once against fxc on the maintainer's Windows
  machine).
- Scales to future shaders: new `.hlsl` in `src/`, add to the build list.
- Fallback if vkd3d's SM3 output proves problematic: wine+fxc container.

## Q4: Behavioral safety gate — how do we verify "stock-identical at 100 %"?

The replacement shaders serve ALL play (styled and stock, all players, attract
mode). The filter must degrade to exact stock output at 1:1 sampling. Options
for verifying that before calling the feature done:

- **(a) Visual A/B on cabinet** — play at 100 % scale with and without the
  overlay; maintainer eyeballs arrows/receptors/freeze/judge. Cheap but
  subjective.
- **(b) Screenshot pixel-diff** — capture the same frame (attract loop or a
  paused/known state) with and without the overlay and diff offline. Stronger,
  needs capture discipline.
- **(c) Math argument only** — rely on the frac=0 collapse proof (uv·texSize
  integral at aligned scales → 4 taps collapse to the stock tap) plus visual
  spot check.

**Answer:** **(c) Math argument + visual spot-check.** The frac=0 collapse
proof is the gate (at aligned scales the blend weights are exactly 0/1 → stock
single tap); one visual pass at 100 % on cabinet confirms. Screenshot
pixel-diff (b) held in reserve if anything looks off.

## Q5: Packaging & operator surface — where do the .gsp files live, and is
## there any toggle?

With no DLL framework (Q1), the shaders are pure `data_mods/` content. Options:

- **(a) Own mod folder, always-on** — e.g. `data_mods/shader_fixes/arc/
  shader_arc/data/shader/*.gsp`. Present = active. Disable by
  deleting/renaming the folder or via `layeredfs.blocklist`. No DLL/config
  involvement.
- **(b) Fold into an existing mod folder** (e.g. alongside playfield-styling's
  assets) so it ships/travels with the feature that motivated it.
- **(c) Own mod folder + a config-gated copy step** — some DLL involvement to
  enable/disable via mod-config.json (starts to look like the Q1(c) framework
  we deferred).

**Answer:** **(a) Own mod folder, always-on.**
`data_mods/shader_fixes/arc/shader_arc/data/shader/*.gsp`. Present = active.
Escape hatch = remove folder or `layeredfs.blocklist`. No DLL/config
involvement.

## Q6: Filter quality/behavior — plain bilinear, or anything fancier?

The core fix is 4-tap index-aware bilinear (palette-lookup each tap, blend the
COLORS). Sub-questions:

- Alpha handling: stock output alpha = palette.a × atlas.a × vColor.a. The
  4-tap version blends per-tap (palette.a × atlas.a) alongside RGB, then
  multiplies vColor.a once. This keeps edge alpha smooth (it's where much of
  the "jaggies" live).
- Sharpness: plain bilinear softens slightly when DOWN-scaling below ~50 %.
  Options: accept it (simplest, matches how the game's own LINEAR-filtered art
  behaves), or add a sharpening tweak (e.g. sample-position clamping /
  "bilinear-sharp" that narrows the blend window to the texel edges), which
  looks crisper at small scales but adds tuning risk.
- Upscaling >100 %: bilinear is unambiguous there (this is where the current
  pixelation is worst).

**Answer:** **(a) Plain bilinear.** Textbook 4-tap weights, alpha blended
per-tap. No sharpening tunable this project; bilinear-sharp remains a drop-in
refinement later (same 4-tap structure, remapped weights).

## Q7: Success criteria & acceptance — what must be true to call this done?

Draft (for confirmation/amendment):

1. At 100 % playfield/overlay scale: output visually indistinguishable from
   stock (Q4's math gate + one cabinet spot-check pass).
2. At scaled settings (e.g. 50 %, 150 %): arrow/receptor/freeze/hit-flash and
   judge-family edges render smoothly — no nearest-texel staircase, and
   critically NO palette banding (the failure mode that killed the sampler
   approach).
3. No frame-rate regression observable on cabinet (ps_3_0, 4 taps → trivially
   cheap, but confirm).
4. `scripts/build_shaders.sh` reproduces the committed `.gsp` bit-for-bit from
   the committed HLSL on a clean machine with Docker.
5. Docs updated: README mod-table row (or LayeredFS section note),
   `docs/shader_replacement_research.md` gains the final shader design,
   AGENTS.md key-entry row.

**Answer:** **Confirmed as drafted** (items 1–5 above are the acceptance
criteria).
