# Research: Widget Pool, Z-Order, Runtime PNG Synthesis

Date: 2026-08-24. Sources cited inline; gathered for the overlay menu rewrite.

## 1. Widget pool capacity

- Widgets are NOT pooled by the DLL — each `create_text_widget`/`create_image_widget`
  pops a node from the **game's pre-allocated render-list free pool** at
  `*(scene_manager+0xB0)` (`src/services/widget_renderer.rs:270-298`; structure in
  `docs/widget_registration_system.md:85-104`). One shared pool for both widget types.
- **Pool size unknown** — game-side pre-allocation, not a constant we own. Nodes are
  permanently consumed: `destroy()` only hides (`src/widgets/image_widget.rs:223-229`);
  unlink/return-to-pool is documented as possible but unimplemented.
- Exhaustion: WARN "render list node pool exhausted", registration fails, but `create_*`
  still returns `Some(widget)` that never renders (`widget_renderer.rs:261-293`) —
  silent-invisible failure mode.
- Current worst-case consumption ≈ **49 widgets** (splash 4, mod_menu 25, hello_world 3,
  autoplay 1, PUS 2, training toast 1 + strip_hud 7 + scrub 2, WebUI preview 4).
- +50–80 more: feasible-unknown. **Cheap de-risk: walk the free list (+0x18 → sentinel)
  once at boot and log the count** — the structure is fully mapped. Design rule stands:
  allocate once, reuse.

## 2. Z-order

- `register_in_render_list` appends at the active-list tail; render walks head→tail.
  **Creation order = z order (later = on top)** — cabinet-validated twice
  (`src/mods/training_mode/strip_hud.rs:1284-1291`, 2026-08-15). No re-order API;
  allocation order = desired z-order. (Docs contain a conflicting "sorts by priority"
  claim — observed behavior is strict list order.)
- **The widget render list draws AFTER all BM2D/AFP groups and stays on top** —
  cabinet-confirmed (`.agents/planning/20260708-background-preview-overlay/progress.md:45-49`).
  So an AFP-clip theme background always sits UNDER our ImageWidgets/text — exactly the
  layering the menu wants.
- Risk: game code registering its own BmpString wrappers later would append after ours;
  no observed instance of game UI overdrawing the current menu.

## 3. Runtime PNG synthesis

- Encoder: the `image = "0.25"` crate (Cargo.toml:32); `strip_synth::encode_png` is
  `RgbaImage::write_to(Cursor, Png)` (`src/mods/training_mode/strip_synth.rs:1071-1079`),
  RGBA8, no size limits found.
- Pipeline precedent (training strip HUD): synthesize on a **background thread** → write
  PNG under `data_mods/_cache/` → `asset_loader::load(path, stem)` on the game thread
  (non-blocking; FileManager load resolves in ~43 frames ≈ 0.7 s; engine-refcounted,
  exactly-once release via consuming `AssetHandle`). Loose PNGs load directly — no
  IFS/arc packing.
- A 1280×720-class rounded-corner alpha panel is hitch-free: encode off-thread at boot
  (or first open), async load, bind on resolve.

## 4. Per-frame animation hooks

- `set_uv` = 4 raw f32 writes (`image_widget.rs:160-170`) — trivially cheap per frame.
- Frame hooks available: `input_manager::on_frame(cb)` (dispatched panic-contained at the
  top of `poll()`, every render frame) or self-rescheduling `run_on_render_thread` pumps
  (bg_preview_overlay pattern). Either drives sprite-based background animation.
