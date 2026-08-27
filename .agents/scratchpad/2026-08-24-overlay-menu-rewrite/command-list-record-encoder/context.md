# Context — command-list-record-encoder

Task: .agents/tasks/2026-08-24-overlay-menu-rewrite/step02/task-01-command-list-record-encoder.code-task.md
Mode: auto. Approval chain verified (same as step01 tasks; plan + design both
`Status: Approved 2026-08-24`).

## Layout facts (verified this session)

Sources: `src/mods/note_types_expansion/mine_render.rs:609-695` (0x13/0x11/0x04
emitters + arena mechanics), `src/mods/player_perspective/pass_rewrite.rs:483-520`
(0x14 emitter), `docs/custom_arrow_renderer_research.md` §3 tag table, and — new this
session — Ghidra decompilation of the 20260616 walker handlers:

- **Tag 0x03 (untextured quads), handler `FUN_180268090`:** header
  `{u16 tag, u16 size, u32 count @+4, u64 payload_ptr @+8}` (same shape as 0x04);
  payload = count × 0x24 `{x0,y0,x1,y1,x2,y2,x3,y3: f32, color: u32}`. The handler
  expands each quad to a 6-vertex triangle list `(p0,p1,p2)(p2,p3,p0)` — corners must
  trace the perimeter in order. The u32 color is copied verbatim into each vertex
  (byte-for-byte LE; D3DCOLOR 0xAARRGGBB per the doc). Coordinates pass through the
  walker's current 2D-context transform (tag 0x07 state: `(x*sx+ox)*ndc_sx+ndc_ox`) —
  pixel-space in, NDC out.
- **Tag 0x0C (scissor), handler `FUN_180269080`:** payload `{u16 enable @+4, u16 x @+6,
  u16 y @+8, u16 w @+0xA, u16 h @+0xC}` — record content ends at +0xE; we emit
  size 0x10 with zeroed tail (the walker chains purely by the size field).
- **Tag 0x13 (SetShader):** `{tag, size=0x18, u32 pad @+4, u64 shader_ptr @+8,
  u32 program @+0x10}` + 4 pad bytes (mine_render writes program as the "param" field —
  pass_rewrite's rewrite pokes the same +0x10 dword).
- **Tag 0x11 (SetTexture):** `{tag, size=0x1C, u32 slot @+4, u32 handle @+8,
  f32[4] param @+0xC}` — exact, no pad.
- **Tag 0x14 (SetVSConstantF):** `{tag, size=0x18+n*0x10, u32 reg_off @+4, u32 n @+8,
  u32 pad @+0xC, u64 payload_ptr @+0x10}` + n×float4 payload inline at +0x18;
  `payload_ptr` is the ABSOLUTE address of that inline payload (self-contained record —
  the walker runs later on a worker thread).
- **Arena mechanics** (emitter side, task-02's job): `size @cl+0x0C`,
  `write ptr @cl+0x10`, `base @cl+0x18`; bump = `cmd = *write; *size += total;
  *write = base + *size`.

**Design consequence:** three record types (0x03/0x04/0x14) embed ABSOLUTE in-arena
pointers to their own payloads. The pure encoder therefore takes the destination base
address (`u64`) at construction and computes payload pointers as `base + offset`; the
emitter must reserve the arena block FIRST, then encode straight at that address (or
encode into a scratch Vec built with the final base and memcpy). Host tests use an
arbitrary base and assert pointer fields.

## Harness

Plain `cargo test` cannot compile `retour` on this ARM host (verified again this
session). Host tests run via the repo's temp-crate `#[path]` harness pattern —
new `scripts/validate_overlay_draw.sh` modeled on
`scripts/validate_judgement_offsets.sh`. `encode.rs` must therefore stay 100 %
dependency-free (no `crate::` imports).

## Build gates

`cargo check --target x86_64-pc-windows-msvc` → `cargo fmt` → `./build.sh`;
`./scripts/validate_overlay_draw.sh` green.
