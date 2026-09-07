# Custom Resolution — Implementation Plan

Status: Approved 2026-09-05 (maintainer pre-authorized plan + implementation through
the first cabinet checkpoint; see `idea-honing.md` Readiness line).

Design: `design/detailed-design.md`. Register: `idea-honing.md`. Research: `research/`.

## Checklist

- [x] Step 1: Pure resolution model (`plan.rs`) + config section + host tests
- [x] Step 2: Signatures, derivations and the four-build sweep
- [x] Step 3: Mod skeleton, `early_apply` output path (back-buffer + AA + display-mode fail-safe) and the graphics-init detour with PRESENT fixup — **first cabinet checkpoint (Tier A: 1080p output, 720p render; SD 640×480 crop)**
- [x] Step 4: Root-7 re-canvas + overlay rows + README — cabinet checkpoint (widgets/menu/toasts at 1080p and SD) — DELIVERED as the `logical_screen` redirect (D5 v3), not a root re-canvas
- [x] Step 5: Letterbox present policy (SD letterbox option, forced letterbox for 16:9 render ≠ output) — cabinet checkpoint
- [x] Step 6: Native render path: surface/viewport/letterbox-src immediates + scissor detour — cabinet checkpoint (1080p/4K native, scissored menus, H3/H5)
- [x] Step 7: render < output depth replacement (R12) — cabinet checkpoint (perf mode). The AFP projection redirect (R13) was DELIVERED by Step 4's `logical_screen` (the 4 AFP loads read the render block); no `afp_projection.rs`
- [x] Step 8: Docs, AGENTS.md row, learnings, signature-sweep integration; Phase-2 scaler decision record (deferred — Tier B needs none)

---

Step 1: Pure resolution model + config section

**Objective.** Land the host-testable core: `src/mods/custom_resolution/plan.rs`
(`Dims`, `parse_dims`, `resolve_render`, `compute` → `Outcome`, `present_mode`,
`scissor_scale`) and the `ResolutionConfig` section in `src/mods/config.rs`
(`ConfigFile.resolution` + both fallback literals + `default_*` fns), plus the
module directory with `mod.rs` declaring the submodules (no `Mod` impl yet).

**Guidance.** Follow the design §4.1 signatures exactly; keep `plan.rs` free of
`windows`/`retour` imports so `cargo test` builds it on the ARM host. Aspect
tolerance `|w·9 − h·16| ≤ 16`; 4:3 = `|w·3 − h·4| ≤ 12`. 4:3 with a non-stock
`render` → coerce to 1280×720 and record `coerced_render: true` in the plan for
the INFO. Percent renders round to even.

**Tests.** Unit tests in `plan.rs`: parse (`"1920x1080"`, `"3840X2160"`, junk),
`resolve_render` (`"output"`, `"75%"` of 1080p = 1440×810 → 1440×810 even,
`"50%"` of 1440p, explicit dims), `compute` (stock ⇒ `Inert`; 1080p ⇒ render ==
output, `force_aa_zero`, `ForceLetterbox`? no — `Stock` policy when render ==
output; 720p render/1080p output ⇒ `ForceLetterbox` + `CreateOutputSized` depth;
SD ⇒ `Sd(Crop)` + `Stock` depth + `recanvas_root7`; 21:9 ⇒ `Rejected`; 8K ⇒
`Rejected`), `present_mode` table, `scissor_scale` identity / 1.5× / 3× / offset /
clamp.

**Integration.** Config parses on the cabinet with the section absent (defaults
= inert). Nothing else references the module yet.

**Demo.** `cargo test custom_resolution::plan` green; `cargo check --target
x86_64-pc-windows-msvc` green.

Step 2: Signatures, derivations and the four-build sweep

**Objective.** Add to `src/core/signatures.rs` the five linear AOBs
(`display_backbuffer_dims`, `render_surface_hoist`, `list_viewport_table`,
`letterbox_rect_fn`, `scissor_handler`) and a `derive_custom_resolution` in
`resolve_derived` publishing `graphics_init`, `aa_config_imm` (pseudo-address),
`screen_w_global`, `screen_h_global`, `render_surfaces_global`,
`surface_create`, `present_depth_release`, `present_depth_addref`, and the
`afp_projection_screen_loads` site list (published as `afp_proj_load_0..N`).
Also a pure helper module `src/mods/custom_resolution/sites.rs` that, given a
match address and a byte window, finds the immediate sites for groups 3–5 (the
paired `C7 85` scan, the `C7 41 14`/`C7 40 16` scan, the letterbox imms) and
returns `Vec<ImmSite { addr, width, stock }>`.

**Guidance.** Use `scanner::decode_call_rel32` / `decode_rip_relative` /
`scan_first_call_rel32`; add a `find_imm32_stores` primitive to `scanner.rs`
only if a second consumer appears (rule 5). Descriptions must state what each
consumer reads at `match+N`. The `sites.rs` helpers take `&[u8]` slices so they
are host-testable against byte fixtures copied from the sweep report.

**Tests.** `sites.rs` unit tests on fixture bytes (the exact instruction runs
recorded in `prototypes/aob_sweep/REPORT.md` and this session's Ghidra listings):
expect 6 viewport pairs, 3+1+2 RT dims, letterbox imm offsets +0x35 / `[RBX+0x298]`.
Then `./scripts/validate_signatures.sh ~/Desktop/ddr_modules` ALL GREEN and
`scripts/sig_harness/shape_diff.py` over the five new AOBs.

**Integration.** Signatures resolve at boot and print in the boot log; no
consumer yet.

**Demo.** Sweep output shows `[+]` for all five AOBs and every derivation on
20250805 / 20260224 / 20260721 / 20260825.

Step 3: Mod skeleton + output path + PRESENT fixup — FIRST CABINET CHECKPOINT

**Objective.** `CustomResolutionMod` registered in `src/lib.rs` (config default
OFF). `early_apply`: config → `plan::compute` → too-late check → `display_modes::
validate` → `patches::apply` groups 1 (back-buffer, both branches) + 2 (AA) →
`present::install` (graphics_init detour: diagnostics pre, PRESENT rt dims →
output post; depth left stock in this step). `is_active()` truthful. One-shot
diagnostics per design §6.

**Guidance.** Copy `fps_unlock.rs`'s shape for early_apply/patch/revert. Use
`memory::apply_checked_patch` with the stock bytes as `expected`. The detour on
`graphics_init` is installed in `early_apply` (before onBoot); if
`hooks::install` fails, skip the whole OUTPUT set (rollback group 1/2) and WARN —
never leave a bigger back-buffer with a 720p PRESENT viewport. The letterbox
detour is Step 5 and the native-render set is Step 6, so in this step `compute`
accepts ONLY 4:3 outputs (the stock mode-1 crop is the correct SD picture) and
returns `Rejected` for any 16:9 non-stock output — a 16:9 output with a 720p
render would be cropped by the game's per-scene mode-1 selection. Gate this with
two constants (`SUPPORTS_LETTERBOX_POLICY`, `SUPPORTS_NATIVE_RENDER`, both
`false` here) that Steps 5/6 flip.

**Tests.** Host: `compute` gating constants. Cabinet (maintainer, CrossOver
window): `output = "640x480"` → the window is 640×480, the game shows the 960-px
crop, TEST menu letterboxed, boot log shows group 1/2 applied, PRESENT rt
`1280×720 → 640×480`, no `screen_w/h ≠ output` WARN. Mod OFF ⇒ byte-identical
stock boot log (zero patches).

**Integration.** Registered mod; `mod-config.json` gains the `resolution` section
(inert defaults). Everything else untouched.

**Demo.** DDR World running in a 640×480 window on the CrossOver install with
stock-SD-cabinet presentation, toggled purely by `mod-config.json`.

Step 4: Root-7 re-canvas + overlay rows + README

**Objective.** `canvas_fix.rs` (on_frame, identity-gated `set_size(1280,720)`),
the two overlay enum rows (RESOLUTION from presets, RENDER SCALE) persisting the
whole section via `save_json_key`, README section (settings, next-launch, spice2x
flag conflicts, recovery).

**Guidance.** Rows follow `fps_unlock`'s `register_enum_row` + `remove_rows_for`
usage; label `640x480` as `640x480 (SD 4:3)`; RENDER SCALE ignored with an INFO
when output is 4:3. The canvas fix must read `+0x50/+0x54` through
`memory::is_readable` and refuse on mismatch (AGENTS.md rule: identity gate
before dereferencing an unpinned layout).

**Tests.** Host: none new beyond row-spec construction. Cabinet: at 640×480 the
mod menu, toasts and PUS widget appear at the same canvas positions as at 720p
(previously they would have rendered at 2× — the root-7 canvas was 640×480);
loading-screen art intact; row edits persist and show "applies next launch".

**Integration.** Uses `input_manager::on_frame` and `widget_renderer::
render_list_manager()`; no new derivations.

**Demo.** Switching RESOLUTION from the overlay menu, rebooting, and getting the
new size with a correctly placed mod menu.

Step 5: Letterbox present policy

**Objective.** `letterbox_rect_fn` detour with `plan::present_mode`; enable
`sd_present = "letterbox"` and lift the 16:9 `render ≠ output` refusal
(`SUPPORTS_LETTERBOX_POLICY = true`). With render still fixed at 1280×720 this
step delivers **Tier A**: 1920×1080 / 4K output from a 720p render through the
engine's LINEAR StretchRect.

**Guidance.** Install in `early_apply` (the present-chain ctor calls the fn
once). Policy table per design §4.1. If the detour cannot be installed, keep the
refusal for 16:9 render ≠ output and disable the letterbox SD option (WARN).

**Tests.** Host: `present_mode` table already covered; add a `compute` case for
1080p output/720p render ⇒ `ForceLetterbox`. Cabinet: (a) SD letterbox — full
HUD visible at 640×360, TEST menu unchanged; (b) 1920×1080 output, 1280×720
render — full-screen image (no crop at scene transitions), mod menu placed
correctly, StretchRect LINEAR softness as expected; the PRESENT depth is still
the 720p `render_depth` here — observe whether draws survive (H2, retail runtime)
and log it; if the panel goes black, Step 7's depth replacement is pulled
forward.

**Integration.** Second detour; `compute` gating updated.

**Demo.** DDR World presenting at native 1080p/4K panel resolution from the 720p
render, no hardware scaler involved.

Step 6: Native render path (Tier B)

**Objective.** Patch groups 3 (surfaces), 4 (list viewports), 5 (letterbox src)
via `sites.rs`; `scissor.rs` detour; lift `SUPPORTS_NATIVE_RENDER`. Atomic
RENDER set (all groups + scissor detour or nothing).

**Guidance.** Verify every site's stock bytes before the first write; roll back
the whole set on any failure. Scissor body per design §4.8 (temporary record
rewrite + restore, `catch_unwind`). Log the first three scaled scissor records
once (INFO) for the cabinet check.

**Tests.** Host: `sites.rs` fixtures (Step 2), `scissor_scale`. Cabinet at
1920×1080 then 3840×2160, render == output: geometry crisp (arrows, guidelines,
AFP shapes), options menu / song wheel / any scrolling list clips correctly
(scissor), results screen + photo/upload path (H3), attract loop, TEST menu; AA
config confirmed 0 in the graphics-init diagnostic on the real cabinet (H5);
CrossOver frame time at 4K; `shader_fixes` AA visibly filtering lane art.

**Integration.** Third detour; the RENDER set becomes available to `compute`.

**Demo.** The game rendering natively at 4K.

Step 7: Depth replacement + AFP projection redirect

**Objective.** `present::fixup` depth policy (`CreateOutputSized` via
`surface_create` + release/addref; nulled-depth fallback + WARN) for render <
output; `afp_projection.rs` disp32 redirect for render ≠ output.

**Guidance.** Content-verify each redirected load reads the derived
`screen_w/h_global` before writing; near-alloc the two `u32` slots
(`memory::alloc_near`). Both fail-open.

**Tests.** Host: `compute` depth/redirect flags. Cabinet: 1280×720 render on
1920×1080 output — no black panel, AFP UI pixel-aligned with screen-command HUD
(the D19 drift symptom absent); SD 640×480 crop with the redirect — bitmap edges
no longer half-pixel shifted.

**Integration.** Completes R12/R13.

**Demo.** Perf mode (720p render, 1080p/4K output) clean on CrossOver.

Step 8: Docs and closure

**Objective.** `docs/custom_resolution.md` (RE facts from this feature: HD-flag
consumers, letterbox mode callers, RT-struct map, scissor handler, root-7
identity); AGENTS.md Key Entry Points row + config section entry; learnings
entry (H1 refuted; StretchRect vs sys_copy; root 7 == render-list manager);
mark the research doc's H1–H5 with outcomes; record the Phase-2 shader-scaler
decision (do / defer) after the maintainer judges Tier-A softness.

**Tests.** `./scripts/validate_signatures.sh` re-run; `cargo fmt`; `./build.sh`.

**Integration.** None (documentation).

**Demo.** A fresh agent can find the feature, its config, and its gotchas from
AGENTS.md alone.
