# Plan — task-03 emitter-promotion

Status: Approved 2026-08-25 (auto mode — approved-planning descent; the
anchor-widget refinement implements the approved "layer-identity frame
gate" recommendation within the same mechanism; see context.md)

## Implementation order

1. encode.rs: production-sequence host test (context/scissor/
   set_vs_const_f(0,[c48,c49])/set_shader(idx)/quad/restore/scissor-off
   walks back exactly; c48/c49 payload bytes checked) — red first
   against a helper that doesn't exist? (encode API already complete —
   the test IS the spec for the emitter's sequence; goes green
   immediately, acceptable: it pins the byte contract.)
2. overlay_draw/mod.rs rework: production state feed
   (`set_background(Option<BackgroundParams>)`, `set_emit_anchor`),
   anchor-identity + per-list gates, time source, c48/c49 emission,
   theme-program bind behind `progs >= idx+1`, 60-consecutive failure
   latch, POC removal (env var, POC_RECT/POC_COLOR/poc_emit), module
   doc rewrite. `on_wrapper_render(this: *mut u8)` (signature change;
   widget_renderer call site passes `this`).
3. widget_renderer: `create_text_widget` refactored to expose the
   wrapper pointer for the anchor variant.
4. theme.rs: `ThemeProgram` enum + `Background::Shader { program }`;
   arrows/bubbles/wavefield flip; guard test replaced.
5. mod_menu: anchor widget (first-allocated, shown/hidden with the
   menu), `update_background_feed()` (open/close/theme-change/animate
   sites), tabs.rs availability gate.
6. Gates: all harnesses → check → fmt → build.
7. Cabinet: boot, open menu on each theme (screenshots), toggle
   animate, MINIMAL greyed, scene churn, close-stops-emission,
   log-verify zero unexpected WARNs.

## Test scenarios

- encode: production sequence byte-walk + c48/c49 payload.
- theme.rs: background mapping (arrows/bubbles/wavefield Shader with
  distinct programs; minimal Static).
- model/tabs pure logic unchanged (greyed is a boolean OR at the
  callsite).
- Cabinet: acceptance criteria 1–5 from the task file.
