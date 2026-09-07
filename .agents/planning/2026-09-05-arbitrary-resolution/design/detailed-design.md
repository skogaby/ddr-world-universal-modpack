# Custom Resolution — Detailed Design

Status: Approved 2026-09-05 (maintainer pre-authorized design → plan → implementation
up to the first cabinet checkpoint after confirming readiness; see the planning
register for the decision record).

## 1. Overview

DDR World renders a fixed 1280×720 frame. This feature lets the operator run the
game at other resolutions on two axes:

- **16:9 native rendering** at 1920×1080, 2560×1440, 3840×2160 (or any 16:9 size
  ≤ 8192): every render surface, list viewport and the back-buffer are created at
  the target size, so all geometry — AFP shapes, HUD quads, arrows through the
  modpack's AA/perspective shaders, 3D backgrounds — rasterises at native density.
  Bitmaps remain the stock 720p assets, magnified by the GPU at draw time.
- **4:3 SD-cabinet output** (640×480): the game keeps rendering its 16:9 canvas at
  1280×720 and presents it through the engine's own SD path (960-px centre crop or
  letterbox) into a 640×480 back-buffer — restoring the CRT cabinets that DDR World
  officially dropped.

A third axis, **render ≠ output** (e.g. render 1280×720, present 1920×1080), reuses
the engine's letterboxed `StretchRect` and is the perf escape hatch for weak GPUs
(CrossOver/D3DMetal at 4K) and the mechanism the SD mode rides on.

No assets are changed. No per-frame work is added except a scissor-record rescale.
Everything is boot-time byte patching of immediates the game reads once, plus two
small detours — the pattern proven by the existing `fps-unlock` mod.

### Engine facts the design rests on (static RE, all four supported builds)

- The AGCS renderer separates a **logical 1280×720 canvas** from the physical
  render target: the 2D-context handler converts canvas coordinates with
  `ndc = (x / canvas_w + offset_x / rt_w) · 2 − 1` — RT size enters only through
  the offset term, so a 1280-unit canvas always spans the whole viewport. All
  screen-command content is resolution-independent for free.
- Physical sizes are hard-coded in exactly six boot-time sites plus one walker
  handler: the back-buffer dims (`FUN_1801ef6d0`, two imm32 per HD/SD branch), the
  render-surface constructor (`FUN_1801f01a0`, hoisted `R15D=0x500`/`ESI=0x2d0` +
  six RT-struct dim immediates), the eight `ScreenCommandList` viewports
  (`FUN_1801f5d10`, an immediate table), the letterbox source rect
  (`FUN_1801f3f60`, two imm32), the scissor handler (tag 0x0C copies canvas px raw
  into `SetScissorRect`), and the PRESENT RT struct, which is re-pointed at the
  back-buffer every frame but keeps ctor-time 1280×720 dims.
- The present chain is: RENDER_2D → `render_color` (1280×720) → COPYVIEWPORT
  `StretchRect` (POINT when `screen_w == 1280`, else LINEAR letterbox/crop) →
  `display` (screen-sized) → ENDVIEWPORT `sys_copy` 1:1 quad → back-buffer.
- Layer-table roots 1/3/5/6/7 are sized to screen dims; root 7 (SYSTEM list) is
  where every modpack widget lives. Its `ScreenRoot` object is the render-list
  manager `widget_renderer::render_list_manager()` already resolves; vtable slot 1
  is `set_size(this, float w, float h)`.
- `Application::onBoot` derives an HD flag `machineType ∉ {0,1}`; only the
  back-buffer selector consumes it. World has no SD boot gate — only the SD
  *assets* were removed. Stock SD cabinets bind a 720p depth to a 640×480 colour
  surface, so a depth larger than the colour target is legal on this engine.
- The `gs_screencommand_bm2d_default` vertex shader applies `c50–c53`
  (`flash_parameter.viewProjection`), built by the AFP projection callback from
  screen dims and the BM2DGroup rect (also screen dims). With render == output it
  is self-consistent; with render ≠ output it introduces a `0.5·render/output`
  render-px UV bias on AFP bitmaps.
- spice2x forwards the game's `BackBufferWidth/Height` verbatim unless
  `-forceres`, `-forceresswap`, or `-w` with `-windowresize`/screenresize are set.

## 2. Detailed Requirements

Consolidated from the accepted decision register.

| # | Requirement |
|---|---|
| R1 | Any 16:9 output ≠ 1280×720 renders natively (all surfaces/viewports at the render size) unless the operator sets a different render size. |
| R2 | `render` and `output` are independent settings; `render` defaults to `output`. When they differ, the engine's COPYVIEWPORT `StretchRect` (LINEAR) performs the scale. |
| R3 | Accepted outputs: any 16:9 `W×H` with `360 ≤ H`, `W ≤ 8192`; and 4:3 (SD). Any other aspect → one WARN, stock behaviour. |
| R4 | 4:3 output forces `render = 1280×720` and uses the engine's SD present modes: `sd_present = "crop"` (default, stock SD behaviour: 960-px centre crop) or `"letterbox"`. The game re-selects crop on every scene transition, so the chosen mode is re-asserted by policy, never by a one-shot write. The TEST menu's own letterbox request is always honoured. |
| R5 | 16:9 with render ≠ output always presents letterboxed (mode 0 — full-screen for matching aspect); the crop mode must never fire there. |
| R6 | Root 7's `ScreenRoot` canvas is set to 1280×720 whenever output ≠ 1280×720, so widgets/toasts/mod menu keep canvas semantics. Roots 1/3/5/6 are left alone (cabinet gate). |
| R7 | Scissor records (tag 0x0C) are scaled from canvas px to render px before reaching `SetScissorRect`. |
| R8 | AA config is forced to 0 whenever output ≠ 1280×720 unless `msaa = "stock"`. |
| R9 | Config section `resolution` {`output`, `render`, `presets`, `sd_present`, `msaa`}; overlay GLOBAL SETTINGS rows RESOLUTION and RENDER SCALE; changes persist immediately and take effect next launch. Mod id `custom-resolution`, default OFF; `output = "1280x720"` + `render = "output"` is a literal no-op (zero patches). |
| R10 | Fail-safe: in fullscreen, the requested `(W, H, Hz)` must be an enumerable display mode or the mod logs one WARN and leaves stock. In windowed mode any size ≤ the desktop is accepted. If onBoot has already run when `early_apply` executes, WARN "effective next launch" and skip. |
| R11 | Every immediate is read back and compared with its stock value before it is written; any mismatch disables that patch group with a WARN. Missing signatures degrade the whole mod to inert (`required_signatures` empty; `is_active()` reports the truth). |
| R12 | PRESENT RT struct dims = output; its depth is left stock when `render ≥ output` in both dimensions, otherwise replaced by an output-sized depth surface (fail-open to a nulled depth + WARN). |
| R13 | When render ≠ output, the AFP projection callback and BM2DGroup sizing read the render dims (fail-open polish). Skipped when render == output. |
| R14 | One WARN when the device's actual back-buffer differs from the request (spice2x override detection); README documents the conflicting spice2x flags. |
| R15 | Out of scope: hi-res assets, KBF fonts, arrow-sheet density, ultrawide, 8K, movie plane, SD-specific text tweaks / `_sd` advertise arcs, machine-type spoofing. |

Assumptions: DLL init completes before `onBoot`'s display init (the `fps-unlock`
precedent, cabinet-proven); the four supported gamemdx builds stay byte-identical
at every patch site (verified offline 2026-09-05 incl. the live install);
D3DMetal handles the target fill rate (operator's choice, like FPS unlock).

## 3. Architecture Overview

```mermaid
flowchart TD
    subgraph boot["DLL init (early_apply, before onBoot display init)"]
        CFG[config::resolution] --> PLAN[plan::compute<br/>pure, host-tested]
        PLAN -->|Inert| NOP[no patches]
        PLAN -->|Plan| CHECK[display_modes::validate<br/>EnumDisplaySettingsW]
        CHECK --> IMM[patches::apply<br/>imm32/imm16 sites, stock-verified]
        CHECK --> DET[install detours:<br/>graphics_init · letterbox_rect · scissor]
    end
    subgraph game["Game boot"]
        OB[Application::onBoot] --> GI[graphics_init]
        GI -->|pre: AA=0| BB[back-buffer select<br/>patched imms → output]
        GI --> SURF[surface ctor<br/>patched imms → render]
        GI --> VP[list viewports<br/>patched imms → render]
        GI -->|post: PRESENT rt dims/depth| FIX[present::fixup]
        OB --> LAYERS[layer set-size loop<br/>root 7 ← screen dims]
    end
    subgraph frame["Per frame"]
        ONF[input_manager::on_frame] -->|once| R7[canvas_fix: root7.set_size 1280×720]
        WALK[list walker tag 0x0C] --> SC[scissor detour: canvas→render px]
        SCENE[scene switch → letterbox_rect mode 1] --> LB[letterbox detour: policy(mode)]
    end
```

Module: `src/mods/custom_resolution/` (multi-file mod, `mods/note_types_expansion/`
reference layout). Signatures + derivations live in `src/core/signatures.rs`.

## 4. Components and Interfaces

### 4.1 `plan.rs` — pure resolution model (host-tested)

```rust
pub struct Dims { pub w: u32, pub h: u32 }
pub enum Aspect { Wide16x9, Sd4x3 }
pub enum SdPresent { Crop, Letterbox }
pub enum Msaa { Auto, Stock }

pub struct ResolutionConfig { output: String, render: String, presets: Vec<String>,
                              sd_present: SdPresent, msaa: Msaa }

pub struct Plan {
    pub output: Dims,            // back-buffer / display / DISPLAY viewport / SYSTEM lists
    pub render: Dims,            // all 1280×720 surfaces + list viewports; OFFSCREEN1 = render.w²
    pub aspect: Aspect,
    pub force_aa_zero: bool,     // R8
    pub present_policy: PresentPolicy,
    pub present_depth: PresentDepth,   // Stock | CreateOutputSized
    pub recanvas_root7: bool,    // output != 1280×720
    pub redirect_afp_projection: bool, // render != output (R13)
}
pub enum PresentPolicy { Stock, ForceLetterbox, Sd(SdPresent) }
pub enum Outcome { Inert, Rejected(String), Plan(Plan) }

pub fn parse_dims(s: &str) -> Option<Dims>;                 // "1920x1080", "3840X2160"
pub fn resolve_render(spec: &str, output: Dims) -> Option<Dims>; // "output" | "WxH" | "75%" | "50%"
pub fn compute(cfg: &ResolutionConfig) -> Outcome;
pub fn scissor_scale(x: u16, y: u16, w: u16, h: u16, rt: Dims, canvas: (f32, f32), offset_px: (f32, f32)) -> (u16, u16, u16, u16);
pub fn present_mode(policy: PresentPolicy, requested: i32) -> i32;  // R4/R5
```

Rules: `Inert` iff output == 1280×720 and render == output. `Rejected` for bad
strings, non-16:9/4:3 aspects (tolerance `|w·9 − h·16| ≤ 16`), `h < 360`,
`w > 8192`, odd dims, 4:3 with render ≠ 1280×720 (coerced with an INFO rather than
rejected). Percent renders round to even. `present_mode`: `Sd(Crop)` returns the
request unchanged; `Sd(Letterbox)` maps 1→0; `ForceLetterbox` maps any→0; `Stock`
identity. `scissor_scale` = `round(x·rt.w/canvas.w + offset.x)` etc., clamped to
the RT.

### 4.2 `mod.rs` — lifecycle

- `id = "custom-resolution"`, `required_signatures = &[]`.
- `early_apply(ctx)`: load config → `plan::compute` → on `Plan`: too-late check
  (`*screen_w_global != 0` ⇒ WARN + return), `display_modes::validate` (R10) →
  `patches::apply(plan)` → `present::install(plan)` → `scissor::install(plan)` →
  `canvas_fix::arm(plan)`. Records `applied`.
- `init`: registers the two overlay rows (always, even when inert — the operator
  needs the row to turn it on); re-resolves nothing (all work is boot-time).
- `enable/disable`: rows only; patches are boot-scoped (next-launch semantics,
  like `fps_unlock`), so `disable()` logs "takes effect next launch".
- `is_active()` = `applied`.

### 4.3 `config` additions (`src/mods/config.rs`)

```rust
#[derive(Deserialize, Clone, Debug)]
pub struct ResolutionConfig {
    #[serde(default = "default_res_output")]  pub output: String,   // "1280x720"
    #[serde(default = "default_res_render")]  pub render: String,   // "output"
    #[serde(default = "default_res_presets")] pub presets: Vec<String>,
    // ["640x480","1280x720","1920x1080","2560x1440","3840x2160"]
    #[serde(default = "default_sd_present")]  pub sd_present: String, // "crop"
    #[serde(default = "default_msaa")]        pub msaa: String,       // "auto"
}
```
`ConfigFile.resolution: Option<ResolutionConfig>` (+ both fallback literals).
Writes: `config::save_json_key("resolution", json!({…whole section…}))` from the
row callbacks (the section is DLL-owned like `fps_unlock`).

### 4.4 Overlay rows (`mod_menu::register_enum_row`)

- `resolution_output` "RESOLUTION": labels from `presets` (`640x480` shown as
  `640x480 (SD 4:3)`), plus the current `output` if not listed; on change →
  persist `output`, toast "applies next launch".
- `resolution_render` "RENDER SCALE": `= OUTPUT` / `75%` / `50%` / `1280x720`;
  on change → persist `render`. Hidden/ignored (INFO) when output is 4:3.

### 4.5 `signatures.rs` additions

Linear AOBs (visible in `EarlyContext`):

| name | pattern (verified unique on 20250805 / 20260224 / 20260721 / 20260825) | consumer reads |
|---|---|---|
| `display_backbuffer_dims` | `80 79 12 00 48 8B F1 74 16 C7 05 ?? ?? ?? ?? 00 05 00 00 C7 05 ?? ?? ?? ?? D0 02 00 00 EB 14 C7 05 ?? ?? ?? ?? 80 02 00 00 C7 05 ?? ?? ?? ?? E0 01 00 00` | imm32 at +15/+25/+37/+47; RIP disp32 of the first two stores → `screen_w_global` / `screen_h_global` (derived) |
| `render_surface_hoist` | `45 33 C9 45 8D 41 15 41 BF 00 05 00 00 41 8B D7 41 8B CF E8` | `MOV R15D` imm at +9; `MOV ESI,0x2d0` (`BE D0 02 00 00`) at +0x2B; `E8` at +0x13 → `surface_create` (derived); forward window ≤ 0x1200 for `C7 41 14 00 05 D0 02` (expect 3), `C7 41 14 00 05 00 05` (1), `C7 40 16 D0 02 00 00` (2, first = PRESENT) |
| `list_viewport_table` | `C7 85 ?? ?? 00 00 00 05 00 00 C7 85 ?? ?? 00 00 D0 02 00 00 48 8D 05 ?? ?? ?? ?? 48 89 85 ?? ?? 00 00 C7 85 ?? ?? 00 00 00 05 00 00 C7 85 ?? ?? 00 00 D0 02 00 00` | first hit only (2 overlapping); forward window ≤ 0x140: every `C7 85 d32 imm32` with imm ∈ {0x500, 0x2d0}, paired by `(disp, disp+4)`; expect 5 pairs (0x500,0x2d0) + 1 pair (0x500,0x500) |
| `letterbox_rect_fn` | prologue: `48 89 5C 24 08 48 89 74 24 10 48 89 7C 24 18 44 8B 0D ?? ?? ?? ?? 4C 8B 05 ?? ?? ?? ?? 33 C0 41 8B D2 48 8B D9 41 8B D0 45 85 C9 74 ?? 45 8B 10 41 8B 48 04 BA 00 05 00 00` | detour target = match; `MOV EDX,0x500` imm at +0x35; `C7 83 98 02 00 00 D0 02 00 00` within +0xE0 (expect 1) |
| `scissor_handler` | prologue: `48 89 5C 24 08 48 89 6C 24 10 48 89 74 24 18 57 48 83 EC 20 48 8B 01 33 F6 48 8B FA 48 8B D9 8D 6E 18 48 39 70 30 74 ?? 44 8B 40 24` | detour target = match |

Derived (in `resolve_derived`; consumed after `early_apply` by the detours'
`init`-time fallbacks — see 4.6 for why the detours themselves only need linear
hits): `graphics_init` = first `CALL rel32` after `fps_target_imm32 + 0x71`
(the `MOV dword [RSP+0x68],3` AA store at +0x69 is itself the `aa_config_imm`);
`screen_w_global`/`screen_h_global` from `display_backbuffer_dims`;
`surface_create` from `render_surface_hoist`; `present_depth_release`/
`present_depth_addref` = the 2nd/3rd `CALL rel32` after the first `C7 40 16` site
(the sequence `bind_colour; release; addref` seen in the ctor);
`afp_projection_screen_loads` = the two `MOV r32,[RIP+disp32]` loads of
`screen_w/h_global` inside the AFP projection callback and the BM2DGroup ctor /
ctx reset (content-verified against the derived globals; R13).

All of these are exercised by `scripts/validate_signatures.sh` + `shape_diff.py`
before any deploy (the letterbox/scissor prologue AOBs and the graphics_init
derivation are the new ones; the rest were byte-verified in planning).

### 4.6 `patches.rs` — immediate patch groups

Each group: resolve sites → read back stock bytes → write via
`memory::apply_checked_patch(expected, replacement)` (rollback on failure). A
group that fails verification is skipped with one WARN; the plan continues
(fail-open per group, R11). Groups:

1. **Back-buffer** (output): four imm32 → `output.w/h` in BOTH the HD and SD
   branches (so the machine type no longer matters — R4/D16). Skipped when output
   is stock.
2. **AA config** (`aa_config_imm` `03 00 00 00` → `00 00 00 00`) when
   `force_aa_zero`.
3. **Render surfaces** (render): `R15D` imm → `render.w`, `ESI` imm →
   `render.h`, the packed RT dims `0x02d00500` → `render.w | render.h << 16`
   (×3), `0x05000500` → `render.w | render.w << 16` (×1), `0x2d0` (`C7 40 16`,
   ×2) → `render.h`. Skipped when render is stock.
4. **List viewports** (render): the 12 imm32s from the paired scan. Skipped when
   render is stock.
5. **Letterbox src** (render): `MOV EDX` imm → `render.w`, `[RBX+0x298]` imm →
   `render.h`, so the `screen_w == render_w` equality branch (POINT 1:1) fires
   exactly when render == output. Skipped when render is stock.

Because `graphics_init`'s address is a derivation and `early_apply` runs before
`resolve_derived`, the mod performs that one derivation itself inside
`early_apply` (a `scan_first_call_rel32` from the linear `fps_target_imm32` hit) —
the same primitive `resolve_derived` publishes later for the boot log.

### 4.7 `present.rs` — graphics-init and letterbox detours

`GenericDetour<unsafe extern "C" fn(*mut u8)>` on `graphics_init`, installed in
`early_apply`:

- pre-original: nothing (AA is an imm patch — group 2 — so the struct already
  carries 0; the detour logs the struct's HD flag / AA / fps for diagnostics).
- post-original: `present::fixup(plan)` — locate the surface object through the
  derived `render_surfaces_global` (`DAT_1806f1ef0` on 20260616): onBoot's first
  `MOV RCX,[RIP+disp32]` (`48 8B 0D`) after the `graphics_init` CALL loads it
  (the code that immediately follows reads `+0x14C..+0x158` and `+0x80` from it —
  the PRESENT rt pointer — to build the present-chain parameter block). Then:
  `rt10 = *(surfaces+0x80)`; verify
  `*(u16*)(rt10+0x14) == render.w && *(u16*)(rt10+0x16) == render.h` (the values
  group 3 wrote) → write `output.w/h`. Depth (R12): if `render.w ≥ output.w &&
  render.h ≥ output.h` leave; else `new = surface_create(output.w, output.h,
  0x4b)`, `old = *(u32*)(rt10+0x10)`; if `old != 0` → `present_depth_release(old)`;
  `*(rt10+0x10) = new`; `present_depth_addref(new)`; on any missing derivation →
  `*(rt10+0x10) = 0` + WARN. Finally read `screen_w/h_global` and WARN if they no
  longer equal `output` (the game's mode fallback rewrote them — R14 signal).

`GenericDetour<unsafe extern "C" fn(*mut u8, i32)>` on `letterbox_rect_fn`:
`mode' = plan::present_mode(policy, mode)`, then the original. Installed in
`early_apply` (the ctor calls it once at init). `PresentPolicy::Stock` ⇒ the
detour is not installed at all.

### 4.8 `scissor.rs`

`GenericDetour<unsafe extern "C" fn(*mut *mut u8, *mut u8)>` on
`scissor_handler`, installed only when render ≠ 1280×720. Body (panic-free,
`catch_unwind`-wrapped):

```
ctx  = *walker;                 // 2D context: +0x00 offset {ox/rt_w, oy/rt_h}, +0x10 scale {1/cw, 1/ch}
gd   = walker[1];               // emitter: +0x144 u16 rt_w, +0x146 u16 rt_h
if record.enable (+4) != 0 && rt_w != 0 && rt_h != 0:
    saved = record[+6..+0xE]
    (x,y,w,h) = plan::scissor_scale(...)   // canvas → RT px
    write record[+6..+0xE]; call original; restore saved
else: call original
```
Records live in the list arena (heap, writable); the restore keeps the list
byte-identical for any later re-walk. The DLL's own `overlay_draw` scissor
encoder (test-only today) is covered by the same path.

### 4.9 `canvas_fix.rs` — root 7 re-canvas (R6)

Registered on `input_manager::on_frame`. Each frame until done: `mgr =
widget_renderer::render_list_manager()`; if non-null and
`memory::is_readable(mgr, 0x60)` and `*(f32*)(mgr+0x50) == output.w as f32 &&
*(f32*)(mgr+0x54) == output.h as f32` (the layout-identity gate — the game's
set-size loop wrote screen dims there) → `((*mgr)->slot1)(mgr, 1280.0, 720.0)`,
INFO, done. If after 600 frames the gate never matched → one WARN (the ScreenRoot
layout assumption failed) and stop. Skipped when output is stock.

### 4.10 `display_modes.rs` (R10)

`validate(output, fps_hint) -> Result<(), String>`: `EnumDisplaySettingsW(NULL,
ENUM_CURRENT_SETTINGS)` for the desktop; if spice2x windowed mode is detectable
(the game's own `+0x10` fullscreen request is 1 — the mod cannot see spice2x's
`-w`; so: accept if `output ≤ desktop`, otherwise iterate
`EnumDisplaySettingsW(NULL, i)` for an exact `(W,H)` match, with `Hz` matched
only when `fps_unlock` is active and its selected value ≠ 60). No match → `Err`
(WARN + stock). This is deliberately permissive in windowed mode.

### 4.11 `afp_projection.rs` (R13, render ≠ output only)

Redirect the disp32 of the `MOV r32,[RIP+disp32]` loads of
`screen_w/h_global` in the AFP projection callback, the BM2DGroup ctor and the
BM2D ctx reset to two mod-owned `u32` slots holding `render.w/h` (near-alloc via
`memory::alloc_near`; the `cull_window` disp32-redirect pattern, content-verified
first). Skipped when render == output; fail-open.

## 5. Data Models

```jsonc
"resolution": {
  "output": "1920x1080",       // "WxH"; "1280x720" = stock. 4:3 (e.g. "640x480") = SD mode
  "render": "output",          // "output" | "WxH" | "75%" | "50%"   (ignored → 1280x720 for 4:3)
  "presets": ["640x480", "1280x720", "1920x1080", "2560x1440", "3840x2160"],
  "sd_present": "crop",        // "crop" | "letterbox"
  "msaa": "auto"               // "auto" (forced 0 off-stock) | "stock"
}
```

Game structures touched (file-relative to `0x180000000`, 20260616 names; all
resolved at runtime):

| structure | field | use |
|---|---|---|
| display struct (onBoot stack, 0x20) | `+0x12` HD flag, `+0x18` AA, `+0x1C` fps | read for diagnostics; AA via imm |
| `DAT_1806f0524` / `DAT_1806f0520` | screen w / h | too-late check, fallback WARN |
| surface object (`DAT_1806f1ef0`, 0x170) | `+0x80` PRESENT rt ptr | fixup |
| RT struct (0x1C) | `+0x08` colour id, `+0x10` depth id, `+0x14` u16 w, `+0x16` u16 h, `+0x18` u8 msaa | dims/depth rewrite |
| walker 2D ctx (0x20) | `+0x00` offset vec, `+0x10` scale vec | scissor scale |
| gd emitter | `+0x144`/`+0x146` u16 viewport w/h | scissor scale |
| scissor record | `+4` u16 enable, `+6/+8/+A/+C` u16 x,y,w,h | rewrite |
| `ScreenRoot` (root 7 = render-list manager) | vtable slot 1 `set_size(f32,f32)`, `+0x50/+0x54` w/h | re-canvas gate + call |

Never modified: the `1280.0f`/`720.0f` rodata (logical canvas; `cull_window`
verifies it), `ScreenRoot` defaults, `DAT_18046091c/DAT_180464108`.

## 6. Error Handling

- Every failure path is fail-open toward **stock rendering**: bad config →
  `Rejected` WARN, mod inert; unresolved signature → that group skipped; stock
  byte mismatch → group skipped; display-mode miss → nothing applied; detour
  install failure → the dependent behaviour is skipped and the plan degrades
  (letterbox policy missing ⇒ `render ≠ output` at 16:9 would crop — so that
  combination is REFUSED entirely when the letterbox detour is unavailable;
  scissor detour missing ⇒ render ≠ stock is refused, since clipped menus are
  worse than 720p).
- Group interdependence rule: back-buffer + AA + present fixup form the OUTPUT
  set; surfaces + viewports + letterbox-src + scissor form the RENDER set. Either
  set applies atomically (all-or-nothing per set; rollback on partial failure via
  `apply_checked_patch`'s error path + re-writing already-applied sites).
- Hook callbacks are panic-free (`catch_unwind` around the scissor and present
  bodies; on panic the original is called with untouched arguments).
- Diagnostics (one-shot, INFO unless noted): the computed plan at boot; each group
  applied/skipped; struct HD/AA/fps at graphics init; PRESENT rt before/after;
  root-7 re-canvas; `screen_w/h_global` ≠ output (WARN, R14); too-late (WARN).
- Recovery from an unbootable resolution: edit/remove `resolution.output` in
  `mod-config.json` (documented in README).

## 7. Testing Strategy

- **Host tests** (`cargo test`, pure layer): `plan::parse_dims`,
  `resolve_render` (percent rounding to even, `output` keyword), `compute` (inert
  / rejected aspects / SD coercion / AA forcing / present policy / depth policy /
  redirect flag), `present_mode` table, `scissor_scale` (identity at 1280×720,
  1.5× and 3× scale, offset term, clamping, u16 overflow).
- **Signature sweep**: `./scripts/validate_signatures.sh ~/Desktop/ddr_modules`
  ALL GREEN + `shape_diff.py` on the five AOBs (every consumer reads `match+N`).
- **Cabinet checkpoints** (the only validation for engine-facing code):
  1. Output-only (Tier A): 1920×1080 output, 1280×720 render — image fills the
     panel via LINEAR StretchRect, mod menu/toasts/HUD widgets at correct places,
     loading art intact; then 640×480 SD (crop) on the CrossOver window.
  2. SD letterbox policy + TEST menu still letterboxed.
  3. Native render at 1080p / 4K: scissored menus (options, song wheel) clip
     correctly, results/photo path (H3), AA-3 cabinets (H5), CrossOver fill rate.
  4. render < output perf mode (depth replacement path).
- A `DDR_CUSTOM_RESOLUTION_DIAG=1` env (dev) raises the one-shot logs to per-scene
  sampling of the PRESENT rt / screen globals.

## Appendix A — Alternatives considered

- **Spoof machine type for SD** — rejected: it flips the 69-caller cabinet-class
  enum (lights/IO to the SD satellite path), collides with `cabinet_force`'s
  existing detour on the same export, and is redundant on real SD hardware.
- **Detour the surface/viewport constructors instead of patching immediates** —
  rejected: byte patches are simpler, have no early-detour race, and the
  immediates were byte-verified on all builds; only fixups needing runtime values
  use a detour.
- **Shader-based scaler (bicubic/area) in the present pass** — deferred (Phase
  2): the stock scaler is a fixed-function `StretchRect`; a shader scaler needs
  the ENDVIEWPORT quad rebound to `render_color` and COPYVIEWPORT skipped.
- **Ultrawide** — dropped for this feature (maintainer); the canvas is
  16:9-logical and the engine crops vertically for wider aspects.
