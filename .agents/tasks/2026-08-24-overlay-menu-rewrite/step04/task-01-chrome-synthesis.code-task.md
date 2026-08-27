# Task: Chrome synthesis — pure panel/strip PNG generation + cache keys

## Description
Create the pure, host-testable synthesis layer for the mod-menu modal chrome:
runtime-generated RGBA8 images (a full-size rounded-corner panel with a baked theme
gradient at a baked opacity, and a small white rounded-rect strip reused stretched +
tinted for every other chrome element), PNG encoding, the `overlay_menu.opacity`
clamp+snap rule, and stable cache-key material (theme id + opacity + layout version)
for the hash-sidecar cache. Zero game dependencies; tested via
`scripts/validate_mod_menu.sh` (which this task extends to mount the module and pull
the `image` crate into the temp harness).

## Background
Step 4 makes the text-only tabbed shell (Step 3) look like a modal (design §4.5). All
chrome is synthesized at runtime, cached under `data_mods/_cache/mod_menu/` with hash
sidecars, and loaded as loose PNGs via `asset_loader` — but this task is ONLY the pure
layer: pixels in, RGBA/PNG bytes + cache-key strings out. Task-02 owns the impure
glue (threads, fs, cache_hasher, asset_loader, widgets).

Two synthesized pieces (design §4.5):
- **Panel**: 1160×600, rounded corners r≈20 px, vertical two-stop theme gradient
  baked in, alpha = configured opacity everywhere inside the rounded rect (0 outside,
  smooth/anti-aliased corner edge). The panel doubles as the static-background
  degrade path for Step 8, so opacity is baked into the texture — the widget tint
  stays 0xFFFFFFFF.
- **Strip**: a small white rounded-rect (suggest 64×16, corner r≈6, fully opaque
  white inside, AA edges) that integration stretches + tints (ABGR incl. alpha) for
  the tab active-indicator, selection bar, scrollbar track/thumb, and header/banner
  backing. White so tinting works multiplicatively.

Theme system arrives in Step 7 — this task takes gradient colors as plain parameters
and defines ONE hardcoded pre-theme gradient constant (deep neutral dark, matching
render.rs's pre-theme `COL_*` palette direction) that integration passes until Step 7
replaces it with theme lookups. The cache key must already include a theme-id string
(pass `"default"` for now) so Step 7 is a parameter change, not a format change.

Precedents to read first:
- `src/mods/training_mode/strip_synth.rs` — pure synthesis + `encode_png` +
  `StripError::describe()` error pattern (the pure layer never logs).
- `src/mods/mod_menu/model.rs` — the dependency-free module shape this harness
  mounts (no `crate::` imports at all).
- `scripts/validate_mod_menu.sh` — the temp-crate harness; its generated Cargo.toml
  currently has no dependencies, so mounting chrome.rs requires adding
  `image = "0.25"` to the generated manifest (crate Cargo.toml already has it).

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-08-24-overlay-menu-rewrite/design/detailed-design.md (§4.5 chrome & layout, §6 error ladder, §5 config)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. New `src/mods/mod_menu/chrome.rs`, dependency-free except the `image` crate
   (no `crate::` imports — same rule as `model.rs`), holding:
   - `pub const LAYOUT_VERSION: u32` (start at 1; bump on any geometry change so
     stale cache files regenerate).
   - Panel geometry constants: `PANEL_W = 1160`, `PANEL_H = 600`,
     `PANEL_CORNER_RADIUS ≈ 20.0`; strip constants (`STRIP_W`, `STRIP_H`,
     `STRIP_CORNER_RADIUS`).
   - `pub fn clamp_opacity(raw: i32) -> i32` — clamp 25..=100 then half-up snap to
     step 5 then re-clamp (the `song_rate::lifecycle::snap_rate_percent` formula:
     `(clamped + STEP/2).div_euclid(STEP) * STEP`).
   - A gradient parameter type (e.g. `pub struct PanelGradient { pub top: [u8; 4],
     pub bottom: [u8; 4] }` — RGB used, alpha field ignored in favor of the opacity
     parameter) plus `pub const DEFAULT_GRADIENT: PanelGradient` (pre-theme).
   - `pub fn synthesize_panel(gradient: &PanelGradient, opacity_percent: i32)
     -> image::RgbaImage` — vertical linear interpolation top→bottom; every pixel's
     alpha = `round(opacity_percent * 255 / 100)` scaled by the rounded-corner
     coverage; coverage 0 outside the rounded rect, 1 in the interior, smooth over
     ~1px at the corner arc (signed-distance or supersample — implementer's choice,
     but the corner must not be a hard staircase).
   - `pub fn synthesize_strip() -> image::RgbaImage` — opaque white rounded rect,
     AA edges, transparent outside.
   - `pub fn encode_png(img: &image::RgbaImage) -> Result<Vec<u8>, ChromeError>`
     (strip_synth pattern) and `ChromeError` with a static `describe()` — the pure
     layer never logs.
   - `pub fn cache_key_material(theme_id: &str, opacity_percent: i32) -> String` —
     a deterministic string folding theme id, opacity, and `LAYOUT_VERSION`
     (task-02 feeds it to `CacheHasher::add_str`). Distinct inputs MUST produce
     distinct strings.
   - `pub fn panel_file_stem(theme_id: &str, opacity_percent: i32) -> String` and
     `pub fn strip_file_stem() -> String` — bare texture-name stems (these become
     ResourceManager names via the loose-PNG load, so keep them short, lowercase,
     collision-safe: e.g. `mm_panel_<theme>_<opacity>` / `mm_strip`). Stems for
     different (theme, opacity) MUST differ: the engine caches textures by name
     hash, so a re-synthesized panel must arrive under a fresh name to swap cleanly.
2. Host tests in-module (`#[cfg(test)]`) covering at minimum:
   - Panel dimensions (1160×600) and strip dimensions.
   - Corner-alpha profile: corner pixel (0,0) fully transparent; center pixel alpha
     == the mapped opacity; an interior edge midpoint (e.g. (PANEL_W/2, 0)) alpha ==
     mapped opacity (rounded edge only affects corners); a pixel just inside the
     corner arc is non-zero and less than or equal to the mapped opacity
     (AA gradient exists).
   - Opacity mapping: 100 ⇒ 255 center alpha; 25 ⇒ round(25·255/100); clamp+snap
     table (24⇒25, 0⇒25, 101⇒100, 82⇒80, 83⇒85, negative values, exact multiples
     pass through).
   - Gradient: top-row center RGB == `top`, bottom-row center RGB == `bottom`,
     mid-row between the two.
   - Cache-key stability: same inputs ⇒ identical string; opacity change ⇒ different
     string; theme change ⇒ different string; the string contains/reflects
     `LAYOUT_VERSION`.
   - `encode_png` round-trip: encodes without error, output starts with the PNG
     magic bytes.
3. Extend `scripts/validate_mod_menu.sh`: add `chrome.rs` to `MODULES` and add
   `image = "0.25"` to the generated temp-crate `[dependencies]`. All existing
   model.rs tests keep passing.
4. Wire `pub(super) mod chrome;` into `src/mods/mod_menu/mod.rs` — no consumers yet
   (task-02 integrates); crate still builds for the msvc target.

## Dependencies
- Step 3's module layout (present). `image = "0.25"` already in the crate
  Cargo.toml. No dependency on Step 2's overlay_draw.

## Implementation Approach
1. Read strip_synth.rs (§encode/error pattern) and design §4.5; fix the constants.
2. Implement clamp/snap + cache-key/stem helpers with tests first (cheap, pure).
3. Implement strip then panel synthesis (shared rounded-rect coverage helper);
   iterate under `./scripts/validate_mod_menu.sh`.
4. Gates: harness green → `cargo check --target x86_64-pc-windows-msvc` →
   `cargo fmt` (bare) → `./build.sh`.

## Acceptance Criteria

1. **Panel synthesis**
   - Given the default gradient and opacity 80
   - When `synthesize_panel` runs
   - Then the image is 1160×600, corners are transparent with a smooth AA arc,
     interior alpha is 204 (80 %), and the vertical gradient interpolates
     top→bottom.

2. **Opacity clamp+snap**
   - Given raw config values 0, 24, 82, 83, 100, 101
   - When `clamp_opacity` maps them
   - Then the results are 25, 25, 80, 85, 100, 100.

3. **Cache-key stability**
   - Given two calls with identical (theme, opacity) and one with opacity changed
   - When `cache_key_material` runs
   - Then the identical calls produce byte-identical strings and the changed call
     differs; file stems differ likewise.

4. **Host-testable purity**
   - Given the module
   - When `./scripts/validate_mod_menu.sh` runs on the host
   - Then chrome tests AND the existing model tests compile and pass with no
     game/hook dependency.

## Metadata
- **Complexity**: Medium
- **Labels**: mod-menu, pure-layer, chrome, synthesis
- **Required Skills**: Rust, image crate, repo host-test harness conventions
- **Generated By**: code-task-generator 2026-08-24
- **Source Plan**: .agents/planning/2026-08-24-overlay-menu-rewrite/implementation/plan.md
- **Plan Step**: Step 4: Modal chrome — synthesized panel, scrollbar, opacity
