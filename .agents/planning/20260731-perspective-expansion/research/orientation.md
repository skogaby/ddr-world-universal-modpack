# Orientation: extension surface of the existing `player_perspective` mod

Status: research complete (from codebase exploration, 2026-07-31)

## What exists today

The Hallway port (`.agents/planning/20260721-player-perspective-hallway/`) shipped a
**screen-space hyperbolic perspective map**, not a camera/rotation port. Everything
below is the surface a new perspective type has to plug into.

### The map (perspective VS, program 1)

`shaders/src/gs_screencommand_arrow.hlsl:104-133` (`vs_persp_main`), duplicated with
the stock-PS contract in `gs_screencommand_default.hlsl:46-71`:

```
x_px = (ndc.x + 1) * 640            // NDC -> pixel, 1280x720 compiled in
y_px = (1 - ndc.y) * 360
d  = max((y_px − c48.x) · c48.w, c49.x)   // signed distance from anchor
s  = c48.z / (c48.z + d)                  // hyperbolic scale, k = c48.z
x' = c48.y + (x_px − c48.y) · s           // converge X toward cx
y' = c48.x + (y_px − c48.x) · s           // converge Y toward anchor
w  = 1/s                                  // REAL w -> perspective-correct UVs
```

Constants (tag-0x14 record, writable window starts at c48; `reg_off` param on the
emitter makes c50+ trivially reachable; **c49.y/z/w are reserved & unused**):

| reg | x | y | z | w |
|---|---|---|---|---|
| c48 | anchor Y (receptor row `posY`) | convergence X (lane center) | k (focal px) | y_dir (±1, reverse flag) |
| c49 | d_min = −0.5k (passed-note growth clamp, caps s at 2×) | reserved | reserved | reserved |

Key property: the horizon sits at `anchor + k·direction`; the anchor is the s=1
fixed point. The clamp `c49.x` is direction-agnostic (it guards the `k+d → 0`
explosion on whichever side d goes negative).

### Per-side flow

option enum row (`src/mods/player_perspective/mod.rs:90-109`, values clamped to
`[0,1]` at `mod.rs:68,83` — **clamps must widen**) → atomics → GAMEPLAY latch
(`mod.rs:148-179`, latches mode + `hallway_focal`, sets cull-window contribution)
→ pre @ Normal on the shared `render_notes_hook` dispatcher emits c48/c49 + snapshots
the CommandList window (`pass_rewrite.rs:200-251`) → post @ Late flips tag-0x13
SetShader records prog 0→1 behind the mandatory `*(u32*)(shaderObj+4) >= 2` gate
(`pass_rewrite.rs:153-196`). Receptors: dedicated `spot_render` detour does the same
recipe (`pass_rewrite.rs:293-374`). Guidelines: shared refcounted `guideline_hook`
applies the **identical map CPU-side** (`src/mods/playfield_styling/guideline_hook.rs:248-255`)
— **hardcoded to the hallway formula; must be generalized in lockstep**. Mines ride
the default container's prog 1 inside the same pass window — free.

`PerspParams` currently carries only `{k}` (`mod.rs:113-117`); `latched_params` and
`any_side_latched` check `mode == 1` literally (`mod.rs:126,137-138`).

### Cull window

`src/services/cull_window.rs`: one cabinet-wide float slot, typed multiplicative
contributions (`max(720,distance)/min(scale,1)`). player_perspective contributes
`hallway_draw_distance` (default 1600) when any side latches hallway. Needed because
hallway *compresses* the approach region (more content visible on screen). A preset
that *expands* the near field (Distant/Space) needs no extension — mapped positions
past the screen edge enter continuously (map is monotonic).

### Shader containers

- Format/packer/engine support up to 255 programs, but the accepted architecture
  (hallway design Appendix A, requirement R5) explicitly chose **one parameterized
  program + constant presets** over per-perspective programs. New perspectives were
  *anticipated as constant presets* (idea-honing Q5: "distant/incoming/space just
  another constant preset later").
- If VS math must change: edit `vs_persp_main`, recompile the two persp blobs via
  `scripts/build_shaders.sh` (fxc 9.29 golden path), recommit
  `data_mods/shader_fixes/blobs/*.d3dbc`; fingerprint cache regenerates containers
  automatically. Same program 1, no container geometry change, no
  `shader_synthesis.rs` plan changes.

### Enum row mechanics

`UiKind::Enum` cycles by index with wraparound; arbitrary value counts already
shipped elsewhere. Adding values = new `EnumValue::with_preview(N, "seop_op_<name>",
"<key>")` + chip PNG + 368×172 preview PNG under
`data_mods/custom_options/select_music_option_lang_eng_v3_ifs/tex/`, generated with
`scripts/gen_option_labels.py` — **gotcha: it rewrites ALL label PNGs with encoder
churn; keep only net-new files, `git restore` the rest**. Persistence stores raw i32
(`PersistMode::Full`) — appending values is wire-compatible.

## Constraints that bind extensions (from the hallway project's accepted decisions)

1. **Never** rewrite a SetShader program index without the `>= idx+1` program-count
   gate (engine handler has no bounds check).
2. **Off-state purity**: OVERHEAD must stay zero-footprint (no emissions, no rewrites;
   with AA off, literal stock bytecode).
3. Lane dressing (filter band / covers / danger / lane bg) stays **stock rectangular**
   — affine-only AFP cannot express a trapezoid. Hallway's mitigation: the converging
   field stays *inside* the stock rectangle (s ≤ 1 in the approach region). Any preset
   whose field exceeds the rectangle breaks this and needs an explicit decision.
4. Options apply next song (GAMEPLAY latch); doubles follows P1; one detour per
   target; cabinet-wide dev knobs in config, players only see the enum row.

## Files a preset-only extension touches

- `src/mods/player_perspective/mod.rs` — enum values, clamps, `PerspParams`
  (mode + full preset params), latch, cull contribution condition
- `src/mods/player_perspective/pass_rewrite.rs` — per-preset constant computation
  (anchor, cx, k, sign, clamp, zoom)
- `src/mods/playfield_styling/guideline_hook.rs` — generalized CPU map
- `src/mods/config.rs` — new tunables
- `shaders/src/gs_screencommand_{arrow,default}.hlsl` + recommitted blobs — only if
  a base-zoom constant is added (one-line `s *= c49.y`)
- label/preview PNGs
