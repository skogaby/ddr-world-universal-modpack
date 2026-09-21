# Design — Lit model shaders for the Background Dancers scene ("Dancer Lighting")

Status: Approved 2026-09-17 (maintainer scoped the feature and pre-approved the mechanism at the
end of the Background Dancers feature; solo-maintainer repo — implementation proceeds in-session).
RE record: `docs/background_dancers_research.md` §4. Predecessor decisions: the "Phase-2 idea" entry
in `.agents/planning/2026-09-16-enable-background-dancers/progress.md`.

## 1. Problem

Every model shader World ships is UNLIT (`texture × per-draw tint`). The dancers the Background
Dancers mod puts behind the lane read flat. The artists DID tag which surfaces are lit: 158 stock
materials name `mdl_ch_lambert` (every dancer body) / `mdl_bg_lambert` (every attached part) — but no
container of those names exists in `shader.arc`, so the engine's by-name lookup falls back to the unlit
`gs_model_*_default` programs (RE §4.1, §4.4).

## 2. Mechanism (approved — no new detours, no engine patches)

Synthesize exactly the two missing containers into the game's `shader.arc` through the existing
`shader_synthesis` seam (the boot read in `Application::onBoot`, one per session ⇒ **next-launch
semantics**). The engine's material→shader selection then routes every `lambert` material to them by
FNV-1 name hash, and nothing else changes: the `_constant*` glows, skydomes, the floor shadow and the
entire stage keep their stock programs (RE §4.4).

| container | name hash | program table | VS | PS |
|---|---|---|---|---|
| `data/shader/mdl_bg_lambert.gsp` | `fnv1_32("mdl_bg_lambert")` | **4 × `(0,0,0)`** (RE §4.2) | `mdl_bg_lambert.vs.d3dbc` (ours, static) | stock `gs_model_default` PS (sliced from the arc) |
| `data/shader/mdl_ch_lambert.gsp` | `fnv1_32("mdl_ch_lambert")` | **4 × `(0,0,0)`** | `mdl_ch_lambert.vs.d3dbc` (ours, skinned) | stock `gs_model_default` PS (same bytes) |

Design points the RE forced:

* **Four identical program entries.** The model pass binds `programs[stage]` with `stage ∈ {0, 2}` in
  production and no bounds check (RE §4.2); every stock model container has 4 entries. Ours too.
* **Stock PS, our VS.** The lit factor scales `tint.rgb` linearly, so the VS folds it into its COLOR0
  output and the game's own `gs_model_default` PS (`tex × color` + the stipple `texkill`) does the rest.
  Alpha, the dissolve and the `c2` reads are bit-exact by construction; no Konami bytecode in the repo;
  only two new committed blobs (VS). The PS blob is `extract_stock(arc, "gs_model_default").ps`.
* **World-space light.** `World` is bound at c14..c17 for every model draw (RE §4.3 — the Phase-2
  note's "only WVP" premise was wrong). The VS transforms the (skinned) normal by `World`'s 3×3,
  renormalizes, and dots it with a compile-time world-space light direction. Uniform scale +
  translation item worlds make this exact; the mirrored forearm inverts correctly.

## 3. The shaders (`shaders/src/mdl_lambert.hlsl`, one file, two entries, `vs_3_0`)

Common contract (must match the stock `gs_model_default` PS inputs):
`o0 POSITION = pos·WVP`, `o1 TEXCOORD0.xy = uv·rcp(c24.xy) + c24.zw` (`m_vTexAnime`, identity on all
stock lambert materials but honours a custom model's UV offset), `o2 COLOR0 = float4(c23.rgb·lit,
c23.a)`. No COLOR0 input (stock lambert layouts carry none — RE §4.4).

```
lit = AMBIENT + DIFFUSE · saturate(dot(n_w, L) · (1 − WRAP) + WRAP)      // WRAP = 0 ⇒ plain Lambert
AMBIENT = 0.65, DIFFUSE = 0.35, WRAP = 0.0, L = normalize(0.3, 1.0, 0.6)   // world space, key above-front
```
`lit ∈ [0.65, 1.0]` — never brighter than stock (a "shading" pass over textures that already carry A3's
baked lighting, per the maintainer's look target); the constants are `#define`s at the top of the file
so a cabinet re-tune is a recompile. No rim term (needs a view vector — no camera constant is bound;
out of scope).

* `vs_bg_main` — inputs `POSITION, NORMAL, TEXCOORD0`. `n_w = normalize(mul(n, World3x3))`.
* `vs_ch_main` — inputs `POSITION, BLENDINDICES, BLENDWEIGHT, TEXCOORD0, NORMAL`. Palette skinning
  reproducing the stock addressing exactly (RE §4.3): `w0 = 1 − Σ v3.xyz`, rows at `u = 0.125/0.375/
  0.625`, `v = (idx + 0.5)/(c22.x + c22.z)`, `texldl` LOD 0 (`tex2Dlod`); position through the 3×4
  rows, the normal through the same rows' 3×3 (`dot(row.xyz, n)`), renormalized, then `World3x3`.
* Registers: `float4 World0..3 : c14..c17`, `WVP0..3 : c18..c21`, `ModelParameters : c22`, `Tint : c23`,
  `TexAnime : c24`, `sampler2D BoneMatrices : s3` — explicit registers (repo convention), the WVP/World
  products written as the stock `mad` chains (`v.x·c18 + v.y·c19 + v.z·c20 + c21`).

Build: two manifest lines in `scripts/build_shaders.sh`, fxc golden path. Smoke test: `gsp_pack.py pack
--name mdl_ch_lambert --vs … --ps <sliced stock PS> --program 0:0 ×4` + `inspect --expect-name`.

## 4. Synthesis changes (`src/services/avs_layeredfs/shader_synthesis.rs` + pure `shader_layout.rs`)

* `Plan.lit_models: bool` = `mod_enabled_in_config("shader-fixes") ∧ mod_enabled_in_config
  ("background-dancers")` (the latter honours `DEFAULT_OFF_MODS` — replaces the local
  `unwrap_or(true)` closure for every mod check, behaviour-preserving for the default-ON ids) `∧
  shader_fixes.lit_models`. Soft degrade like the theme blobs: a missing lit VS blob drops ONLY the lit
  containers with one WARN; AA/persp/themes unaffected. A plan with only `lit_models` still synthesizes.
* New blob constants `BLOB_LIT_BG_VS = "mdl_bg_lambert.vs.d3dbc"`, `BLOB_LIT_CH_VS = "mdl_ch_lambert.vs.d3dbc"`;
  container names `LIT_BG = "mdl_bg_lambert"`, `LIT_CH = "mdl_ch_lambert"`, PS donor `MODEL_PS_DONOR =
  "gs_model_default"`.
* `build_all`: when `plan.lit_models`, `extract_stock(arc, MODEL_PS_DONOR)` once, then per container
  `write_container(dir, name, shader_layout::fnv1_32(name), &[our_vs], &[stock.ps],
  &shader_layout::model_programs())`. The hash is COMPUTED (no stock header exists) — the ONE place
  the module computes a hash; every other container still copies the stock header hash.
* Fingerprint bumps to `"v5 aa= persp= themes= lit= arc=…"` (+ the two blob hashes ride the existing
  per-blob loop). `planned_names` appends the two names when planned.
* `shader_layout.rs` (pure, host-tested via `validate_overlay_draw.sh`): `PlannedContainers.lit_models`
  + `planned(aa, persp, themes, lit)`; `MODEL_PROGRAM_ENTRIES: u8 = 4` + `model_programs()`; `fnv1_32`
  with tests pinned to the four known stock hashes (`gs_screencommand_arrow` 0x9E93AC7B,
  `gs_model_default` 0x6CD7F817, `mdl_bg_constant` 0xBDFE3C7B, `gs_model_skinning_default` 0x55A0AC03);
  `LIT_MODEL_CONTAINERS: [&str; 2]`.
* `status()` unchanged; the shader-fixes enable line gains `lit_models=`.

## 5. Config + operator surface

* `ShaderFixesConfig.lit_models: bool` (`#[serde(default = "default_true")]`) — **PROVISIONAL default
  `true`** for the first cabinet build so the tester sees the feature; the maintainer decides the shipped
  default after the cabinet result (design rule from the handoff: ask once, do not assume). A `false`
  default is a one-line change + README/AGENTS wording.
* `src/mods/shader_fixes.rs`: second enum row `shader-fixes-lit` "Dancer Lighting" OFF/ON under the
  mod's header, hint "Lit shading for the 3D background dancers. Restart the game to apply." Both rows'
  callbacks write the section WHOLE via a `persist_section()` reading two live atomics (`save_json_key`
  REPLACES the section — the s_marvelous rule); the atomics are seeded from config at enable.
* Gate visible in the log: `shader_synthesis: synthesizing (aa=…, persp=…, themes=…, lit=…)` and the two
  `mdl_*_lambert → N bytes (4 programs, 1 VS, 1 PS)` lines; `ShaderFixes: enabled (anti_aliasing=…,
  lit_models=…; synthesis: …)`.

## 6. Fail-open rules

| failure | outcome |
|---|---|
| `shader-fixes` or `background-dancers` disabled, or `lit_models=false` | not planned — stock lambert fallback (unlit), no log beyond the plan INFO |
| a lit VS blob missing/blocklisted (DLL-only deploy!) | ONE WARN `lit blob '…' not found — dancer lighting off`; lit dropped from the plan, everything else synthesizes |
| `gs_model_default` missing from the stock arc / bad blob | `build_all` error ⇒ existing path: WARN + sidecar poisoned + stock shaders (as today for any container error) |
| container ships but the engine rejects a blob | D3D `CreateVertexShader` fails inside the game's own job (`FUN_1802541a0`) — the same exposure every synthesized container already has; the material falls back to the default program |
| the scene never draws a lambert surface | nothing — the two extra registry objects cost 2 of 256 pool slots |

No rendering detours, no engine patches, no per-pass SetShader rewriting; the `gs_model_*_default` /
`mdl_*_constant*` stock containers are NEVER overlaid by this feature.

## 7. Cabinet checklist (Step 3)

Deploy = the DLL **and** `data_mods/shader_fixes/blobs/` (the two new `.d3dbc` files — a DLL-only deploy
leaves the row inert with the WARN above). Then:

1. Boot log: `shader_synthesis: synthesizing (… lit=true)` (or `cache up to date`), the two
   `mdl_bg_lambert → … (4 programs, 1 VS, 1 PS)` / `mdl_ch_lambert → …` lines, `ShaderFixes: enabled
   (… lit_models=true; synthesis: synthesized containers served)`. No WARN.
2. Background Dancers song: the dancer bodies AND their parts (head/face/chest/hips/forearm) show smooth
   form shading that follows the limbs as they move (upper/front surfaces brightest, undersides ≈ 65 %);
   the floor shadow blob and the WHOLE stage look exactly as before (RE §4.4). Check a `rinon`-class
   dancer (parts on every attach bone incl. the mirrored forearm — no dark/inverted forearm) and a
   hair/translucent part for alpha regressions.
3. Toggle DANCER LIGHTING OFF in the mod menu → `mod-config.json` `shader_fixes.lit_models=false` →
   restart → dancers flat again, `lit=false` in the log.
4. Tune: if the shading reads too strong/weak, adjust `AMBIENT`/`DIFFUSE`/`WRAP` in the HLSL, rebuild
   blobs, redeploy the blobs only (the fingerprint includes the blob hashes ⇒ automatic re-synthesis).
5. Ask the maintainer: shipped default ON or OFF?
