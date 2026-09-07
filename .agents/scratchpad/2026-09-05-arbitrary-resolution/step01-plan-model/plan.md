# Plan — Step 1

Status: Approved 2026-09-05 (inherits the approved PDD plan; maintainer pre-authorized).

## Test scenarios (all in `plan.rs` `#[cfg(test)]`, run via the harness)

| id | input | expected |
|---|---|---|
| T1 | `parse_dims("1920x1080")`, `"3840X2160"`, `" 640x480 "` | Some(dims) |
| T2 | `parse_dims("abc")`, `"1920"`, `"0x720"`, `"1921x1080"` (odd) | None |
| T3 | `resolve_render("output", 1080p)` | 1920×1080 |
| T4 | `resolve_render("75%", 1080p)` | 1440×810 |
| T5 | `resolve_render("50%", 1440p)` | 1280×720 |
| T6 | `resolve_render("1280x720", 4K)` | 1280×720; `"foo"` → None; `"0%"`/`"300%"` → None |
| T7 | compute stock 1280×720 / output | `Inert` |
| T8 | compute 1080p / output (native gate ON) | render 1080p, `Stock` policy, `Stock` depth, `force_aa_zero`, `recanvas_root7`, no redirect |
| T9 | compute 1080p / 1280x720 (letterbox gate ON) | `ForceLetterbox`, `CreateOutputSized`, redirect, aa zero |
| T10 | compute 4K / 75% | render 2880×1620, `ForceLetterbox`, depth `Stock` (render ≥ output? no — 2880 < 3840 ⇒ `CreateOutputSized`) |
| T11 | compute 640x480 crop | 4:3, render 1280×720, `Sd(Crop)`, depth `Stock` (render ≥ output), recanvas, redirect, `coerced_render == false` when render "output"?? — spec: 4:3 ignores render: `coerced_render = render spec resolved ≠ 1280×720` |
| T12 | compute 640x480 with render "50%" | render 1280×720, `coerced_render == true`, INFO note string |
| T13 | compute 2560x1080 (21:9) | `Rejected` |
| T14 | compute 7680x4320 | `Rejected` (w > 8192? 7680 ≤ 8192 → h ok; use 8192x4608 → w ok, so 8K rejection comes from… spec says ≤8192 ok. Use 10240x5760 → Rejected) |
| T15 | compute 640x360 (h == 360) | accepted; 480x270 → Rejected (h < 360) |
| T16 | compute 1080p with msaa "stock" | `force_aa_zero == false` |
| T17 | compute with gates OFF (simulate via `compute_gated(input, gates)`) 1080p/output | `Rejected` mentions native render; 1080p/720p ⇒ Rejected mentions letterbox |
| T18 | `present_mode` table (Stock, Sd(Crop), Sd(Letterbox), ForceLetterbox × requested 0/1/2) | per design |
| T19 | `scissor_scale` identity at rt 1280×720 canvas 1280×720 offset 0 | unchanged |
| T20 | `scissor_scale` (100,50,300,200) at rt 1920×1080 | (150,75,450,300) |
| T21 | `scissor_scale` at 3840×2160 with offset (10,20) px | (310,170,900,600) |
| T22 | `scissor_scale` clamp: x+w beyond rt | clamped so x+w ≤ rt.w |

## Implementation shape

- `plan.rs`: types per design §4.1; `compute(&PlanInput) -> Outcome` delegates to
  `compute_gated(&PlanInput, Gates)` where `Gates { letterbox_policy: bool, native_render: bool }`
  and `GATES` const = `{false,false}` for Step 3 (tests cover both).
- `PlanInput { output: &str, render: &str, sd_present: &str, msaa: &str }` (strings straight from config).
- `config.rs`: `ResolutionConfig` mirroring `FpsUnlockConfig` conventions.
- Harness script cloned from `validate_training_mode.sh`.
