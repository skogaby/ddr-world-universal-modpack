# Plan — command-list-record-encoder

Status: Approved 2026-08-24 (auto mode; verified approval chain per context.md)

## Test scenarios (host, temp-crate harness)

1. **Byte-exact layouts** — for each record type, encode with known inputs and compare
   against a hand-computed byte vector (tag, size, fields, pad zeroing, payload
   placement). Inputs: scissor_on(200,100,880,520); scissor_off(); set_shader(ptr
   0xDEAD_BEEF_0000, prog 1); set_vs_const_f(reg 0, 2 regs of known floats);
   set_texture(0, 0x42, [1,1,0,0]); untextured_quads(1 quad, corners
   (10,20)(30,20)(30,40)(10,40), color 0x80FF00FF).
2. **Absolute payload pointers** — with base 0x1000, the 0x14 record's ptr field =
   0x1000 + record_offset + 0x18; the 0x03/0x04 ptr = base + offset + 0x10. Encoding
   the same sequence at a different base changes ONLY the pointer fields.
3. **Chain integrity** — the full POC sequence (scissor-on → constants → set_shader →
   1 quad → set_shader → scissor-off) walks back exactly as emitted via `walk()`,
   ending precisely at buffer end; corrupt cases (truncated buffer, zero-size record)
   return errors.
4. **Quad expansion contract** — multi-quad encode places quad N at
   header+0x10+N*0x24 (documented perimeter corner order).

Tests fail before implementation exists (module + harness are new).

## Implementation

- `src/services/overlay_draw/encode.rs` — dependency-free: tag/size consts,
  `RecordWriter { buf: Vec<u8>, base: u64 }` with the six builders + `bytes()`/`len()`,
  `Quad` struct, `walk(&[u8]) -> Result<Vec<(u16, usize)>, WalkError>`.
- `src/services/overlay_draw/mod.rs` — module docs + `pub mod encode;` (emitter lands
  in task-02); `pub mod overlay_draw;` added to `src/services/mod.rs`.
- `scripts/validate_overlay_draw.sh` — temp-crate harness mounting `encode.rs` via
  `#[path]` (validate_judgement_offsets.sh pattern).
