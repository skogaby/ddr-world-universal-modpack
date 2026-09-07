# Context — Step 1: pure resolution model + config section

Task source: `.agents/planning/2026-09-05-arbitrary-resolution/implementation/plan.md`
Step 1 (Status: Approved 2026-09-05, pre-authorized). Design:
`.agents/planning/2026-09-05-arbitrary-resolution/design/detailed-design.md` §4.1, §4.3, §5
(Status: Approved 2026-09-05). No `code-task-generator` task file — the maintainer
pre-authorized running code-assist directly from the approved plan; recorded here so the
audit trail is explicit. `CODEASSIST.md` absent; `AGENTS.md` read (no commits; `cargo fmt`
whole-crate; host tests via temp-crate harness scripts because `retour` does not compile on
ARM hosts).

## Build / test commands (repo root)

- `cargo check --target x86_64-pc-windows-msvc` — type check (the only compile that sees the
  whole crate on this host).
- `./scripts/validate_custom_resolution.sh` — NEW in this step: harness that mounts
  `src/mods/custom_resolution/plan.rs` and runs its `#[cfg(test)]` suite (pattern:
  `scripts/validate_training_mode.sh`).
- `cargo fmt`.

## Requirements (from plan Step 1 / design §4.1)

R-1.1 `plan.rs` is dependency-free (no `crate::`, no `windows`/`retour`) so the harness compiles it.
R-1.2 `parse_dims("WxH")` case-insensitive `x`, rejects junk/zero/odd dims.
R-1.3 `resolve_render(spec, output)`: `"output"` → output; `"WxH"`; `"NN%"` → even-rounded scale of output.
R-1.4 `compute(input)` → `Outcome::{Inert, Rejected(String), Plan}`: stock ⇒ Inert; 16:9 tolerance
`|9w−16h| ≤ 16`, 4:3 `|3w−4h| ≤ 12`; other aspects Rejected; `h < 360` / `w > 8192` Rejected;
4:3 ⇒ render coerced to 1280×720 (`coerced_render`), `Sd(sd_present)` policy; 16:9 render ≠
output ⇒ `ForceLetterbox` + `CreateOutputSized` depth iff render < output in either dim; render
== output ⇒ `Stock` policy/depth; `force_aa_zero` = output ≠ stock unless msaa `"stock"`;
`recanvas_root7` = output ≠ stock; `redirect_afp_projection` = render ≠ output.
R-1.5 `present_mode(policy, requested)`: Stock/Sd(Crop) identity; Sd(Letterbox) 1→0; ForceLetterbox any→0.
R-1.6 `scissor_scale(x,y,w,h, rt, canvas, offset_px)` → u16 tuple, round-to-nearest, clamped to rt.
R-1.7 `ResolutionConfig` serde section in `config.rs` (`output`, `render`, `presets`, `sd_present`, `msaa`
with defaults) + `ConfigFile.resolution` + both fallback literals.
R-1.8 `src/mods/custom_resolution/mod.rs` declares `pub mod plan;` (no Mod impl yet); `mods/mod.rs` registers the module.

## Files touched

- `src/mods/custom_resolution/mod.rs` (new), `src/mods/custom_resolution/plan.rs` (new)
- `src/mods/mod.rs` (+1 line), `src/mods/config.rs` (section + field + 2 fallbacks)
- `scripts/validate_custom_resolution.sh` (new)

## Assumptions

- Step 3's gating constants (`SUPPORTS_LETTERBOX_POLICY`, `SUPPORTS_NATIVE_RENDER`) are
  introduced in THIS step as `pub const` = false with tests asserting the gated rejections,
  since `compute` is the pure layer they gate.
- Percent renders: `round(output·pct/100)` then round down to even.
