# Plan — overlay-draw-poc

Status: Approved 2026-08-24 (auto mode; verified approval chain per context.md)

## Test scenarios

1. Encoder extension (set_context_2d): byte-exact host test + POC-sequence walk test
   updated — via scripts/validate_overlay_draw.sh.
2. Diagnostics (autonomous boot, env unset): exactly one
   `overlay_draw diag: scene=N ...` INFO per scene id entered; `bump_ok=true` and a
   non-null list expected in the attract band; `progs=` shows the stock default
   container's program count.
3. POC (autonomous boot, env set): no crash through ≥2 attract cycles; heartbeat
   `POC alive` INFO lines advancing ~600 emissions apart; no unexpected gate WARNs.
4. Maintainer session: visual + z-probe (task AC-4) — quad visible in attract, song
   select, gameplay; layering recorded.

## Implementation

Per context.md facts: `overlay_draw::init(signatures)` at lib.rs 6c2;
`on_wrapper_render()` from `wrapper_render_hook` (before original); diag_tick behind a
relaxed scene compare + bounded once-per-scene-id set; poc_emit with the gate ladder,
`RecordWriter::new(write_addr)` block build, single copy+bump, arena-reset frame gate,
latched WARN classes, 600-emission heartbeat.
