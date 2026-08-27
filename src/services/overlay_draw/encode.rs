//! Pure command-list record encoders for the overlay-draw service.
//!
//! Builds byte-exact records for the game's screen command list — the
//! `{u16 tag, u16 size, payload}` stream consumed later by the worker-thread
//! walker. This layer is deliberately **dependency-free** (no `crate::`
//! imports, no game pointers beyond plain `u64` values) so its tests run on
//! any host via the temp-crate harness (`scripts/validate_overlay_draw.sh`);
//! the impure emitter copies finished byte blocks into the live arena.
//!
//! Layout authorities (do not edit offsets without re-verifying):
//! - `src/mods/note_types_expansion/mine_render.rs` — 0x13/0x11/0x04 emitters
//!   and the arena-bump mechanics.
//! - `src/mods/player_perspective/pass_rewrite.rs` — 0x14 emitter with the
//!   inline self-contained payload.
//! - `docs/custom_arrow_renderer_research.md` §3 — the full walker tag map.
//! - Ghidra (gamemdx 20260616) walker handlers, decoded 2026-08-24:
//!   `FUN_180268090` (tag 0x03: header `{count @+4, payload_ptr @+8}`, payload
//!   count × 0x24 `{x0,y0..x3,y3, u32 color}`, expanded as the triangle list
//!   `(p0,p1,p2)(p2,p3,p0)` — corners must trace the quad perimeter; the color
//!   dword is copied verbatim per vertex) and `FUN_180269080` (tag 0x0C:
//!   `{u16 enable @+4, x @+6, y @+8, w @+0xA, h @+0xC}`).
//!
//! Three record types (0x03, 0x04, 0x14) embed **absolute** pointers to their
//! own in-record payloads (the walker consumes records on a worker thread
//! after the frame — records must be self-contained). `RecordWriter` therefore
//! takes the destination base address up front and computes payload pointers
//! as `base + offset`; the emitter must reserve the arena block first and
//! encode against that address.
//!
//! CRASH CLASS: the walker parses purely by size-chaining — one wrong `size`
//! field desyncs every following record. Every size here is hand-matched to
//! the shipped emitters / decoded handlers and locked by host tests.

/// Untextured quad batch (count × 0x24 corner/color entries).
pub const TAG_QUADS_UNTEXTURED: u16 = 0x03;
/// Textured rotate-sprite batch (count × 0x34 entries) — the lane path.
pub const TAG_QUADS_TEXTURED: u16 = 0x04;
/// Set the walker's 2D context (virtual canvas + offset) for subsequent draws.
pub const TAG_SET_CONTEXT_2D: u16 = 0x07;
/// Bind texture to a stage (+ float param → VS c32+stage).
pub const TAG_SET_TEXTURE: u16 = 0x11;
/// Bind shader container + program index. NO bounds check in the handler —
/// callers must gate on the container's program count.
pub const TAG_SET_SHADER: u16 = 0x13;
/// Upload VS float4 constants at c48+reg_off (inline payload).
pub const TAG_SET_VS_CONST_F: u16 = 0x14;
/// Scissor rect enable/disable.
pub const TAG_SCISSOR: u16 = 0x0C;

/// One untextured quad: four corner points tracing the perimeter in order
/// (the walker expands `(p0,p1,p2)(p2,p3,p0)`), plus a D3DCOLOR
/// (`0xAARRGGBB`) copied verbatim into every vertex.
#[derive(Clone, Copy, Debug)]
pub struct Quad {
    pub corners: [[f32; 2]; 4],
    pub color: u32,
}

/// Errors from [`walk`].
#[derive(Debug, PartialEq, Eq)]
pub enum WalkError {
    /// A record header would run past the end of the buffer.
    Truncated { offset: usize },
    /// A record's size field is smaller than a header or not within the
    /// buffer (`size` chaining would desync the walker).
    BadSize {
        offset: usize,
        tag: u16,
        size: usize,
    },
}

/// Append-only record builder targeting a destination whose first byte will
/// land at absolute address `base` (used for the self-contained payload
/// pointers). Build the full block, then copy `bytes()` to `base` verbatim.
pub struct RecordWriter {
    buf: Vec<u8>,
    base: u64,
}

impl RecordWriter {
    pub fn new(base: u64) -> Self {
        Self {
            buf: Vec::new(),
            base,
        }
    }

    pub fn bytes(&self) -> &[u8] {
        &self.buf
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    // ── primitive appends ───────────────────────────────────────────

    fn push_u16(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    fn push_u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    fn push_u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    fn push_f32(&mut self, v: f32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    /// Absolute address of the next byte to be written.
    fn cursor_addr(&self) -> u64 {
        self.base + self.buf.len() as u64
    }

    // ── records ─────────────────────────────────────────────────────

    /// Scissor record (tag 0x0C). Handler payload: `{u16 enable @+4, u16 x @+6,
    /// u16 y @+8, u16 w @+0xA, u16 h @+0xC}`; content ends at +0xE, emitted as
    /// size 0x10 with a zeroed tail (the walker chains by the size field).
    pub fn scissor(&mut self, enable: bool, x: u16, y: u16, w: u16, h: u16) {
        self.push_u16(TAG_SCISSOR);
        self.push_u16(0x10);
        self.push_u16(enable as u16);
        self.push_u16(x);
        self.push_u16(y);
        self.push_u16(w);
        self.push_u16(h);
        self.push_u16(0); // pad to size 0x10
    }

    pub fn scissor_on(&mut self, x: u16, y: u16, w: u16, h: u16) {
        self.scissor(true, x, y, w, h);
    }

    /// Disable scissor. The rect is ignored by the handler when disabling.
    pub fn scissor_off(&mut self) {
        self.scissor(false, 0, 0, 0, 0);
    }

    /// 2D-context record (tag 0x07, size 0x14): `{f32 canvas_w @+4,
    /// f32 canvas_h @+8, f32 offset_x @+0xC, f32 offset_y @+0x10}`.
    /// Handler (`FUN_180268c40`, 20260616): sets the walker's draw context to
    /// a `canvas_w × canvas_h` virtual canvas with a pixel offset — subsequent
    /// 2D draw records (0x01–0x04) map their pixel coordinates through it.
    /// `set_context_2d(1280.0, 720.0, 0.0, 0.0)` = the standard full-screen
    /// canvas.
    pub fn set_context_2d(&mut self, canvas_w: f32, canvas_h: f32, offset_x: f32, offset_y: f32) {
        self.push_u16(TAG_SET_CONTEXT_2D);
        self.push_u16(0x14);
        self.push_f32(canvas_w);
        self.push_f32(canvas_h);
        self.push_f32(offset_x);
        self.push_f32(offset_y);
    }

    /// SetShader record (tag 0x13, size 0x18):
    /// `{u32 pad, u64 shader_ptr @+8, u32 program @+0x10}` + 4 pad bytes.
    /// `shader_ptr` is the gs::Shader object; `program` indexes its program
    /// handle array — the handler has NO bounds check, the caller must gate
    /// on `*(u32*)(shader+4) >= program+1`.
    pub fn set_shader(&mut self, shader_ptr: u64, program: u32) {
        self.push_u16(TAG_SET_SHADER);
        self.push_u16(0x18);
        self.push_u32(0); // pad
        self.push_u64(shader_ptr);
        self.push_u32(program);
        self.push_u32(0); // tail pad to 0x18
    }

    /// SetTexture record (tag 0x11, size 0x1C):
    /// `{u32 stage, u32 tex_handle, f32[4] param}` (param → VS c32+stage).
    pub fn set_texture(&mut self, stage: u32, tex_handle: u32, param: [f32; 4]) {
        self.push_u16(TAG_SET_TEXTURE);
        self.push_u16(0x1C);
        self.push_u32(stage);
        self.push_u32(tex_handle);
        for v in param {
            self.push_f32(v);
        }
    }

    /// SetVSConstantF record (tag 0x14, size 0x18 + n*0x10):
    /// `{u32 reg_off @+4 (base register c48+reg_off), u32 n @+8, u32 pad,
    /// u64 payload_ptr @+0x10}` with the float4 payload inline at +0x18 and
    /// `payload_ptr` pointing at it (absolute — self-contained record).
    /// No-op for an empty register list.
    pub fn set_vs_const_f(&mut self, reg_off: u32, regs: &[[f32; 4]]) {
        let n = regs.len() as u32;
        if n == 0 {
            return;
        }
        let total = 0x18 + n as u16 * 0x10;
        let payload_addr = self.cursor_addr() + 0x18;
        self.push_u16(TAG_SET_VS_CONST_F);
        self.push_u16(total);
        self.push_u32(reg_off);
        self.push_u32(n);
        self.push_u32(0); // pad (game leaves garbage; we zero)
        self.push_u64(payload_addr);
        for reg in regs {
            for v in reg {
                self.push_f32(*v);
            }
        }
    }

    /// Untextured quad batch (tag 0x03, size 0x10 + count*0x24): header
    /// `{u32 count @+4, u64 payload_ptr @+8}`, payload = count × 0x24
    /// `{x0,y0..x3,y3, u32 color}` inline after the header (absolute pointer,
    /// like the game's own 0x04 emission). Coordinates are pixel-space; the
    /// walker applies its current 2D-context transform. No-op for an empty
    /// quad list.
    pub fn quads_untextured(&mut self, quads: &[Quad]) {
        let count = quads.len() as u32;
        if count == 0 {
            return;
        }
        let total = 0x10 + count as u16 * 0x24;
        let payload_addr = self.cursor_addr() + 0x10;
        self.push_u16(TAG_QUADS_UNTEXTURED);
        self.push_u16(total);
        self.push_u32(count);
        self.push_u64(payload_addr);
        for q in quads {
            for [x, y] in q.corners {
                self.push_f32(x);
                self.push_f32(y);
            }
            self.push_u32(q.color);
        }
    }
}

/// Parse a record block by size-chaining (the walker's own discipline) and
/// return the `(tag, size)` sequence. Errors when a header is truncated or a
/// size field could not have been produced by a well-formed emitter — the
/// exact conditions that would desync the real walker.
pub fn walk(buf: &[u8]) -> Result<Vec<(u16, usize)>, WalkError> {
    let mut records = Vec::new();
    let mut offset = 0usize;
    while offset < buf.len() {
        if offset + 4 > buf.len() {
            return Err(WalkError::Truncated { offset });
        }
        let tag = u16::from_le_bytes([buf[offset], buf[offset + 1]]);
        let size = u16::from_le_bytes([buf[offset + 2], buf[offset + 3]]) as usize;
        if size < 4 || offset + size > buf.len() {
            return Err(WalkError::BadSize { offset, tag, size });
        }
        records.push((tag, size));
        offset += size;
    }
    Ok(records)
}

// ── Tests ───────────────────────────────────────────────────────────
// Run via scripts/validate_overlay_draw.sh (temp-crate host harness; the DLL
// crate itself cannot `cargo test` on non-x86 hosts).

#[cfg(test)]
mod tests {
    use super::*;

    fn le32(buf: &[u8], off: usize) -> u32 {
        u32::from_le_bytes(buf[off..off + 4].try_into().unwrap())
    }

    fn le64(buf: &[u8], off: usize) -> u64 {
        u64::from_le_bytes(buf[off..off + 8].try_into().unwrap())
    }

    fn f32_at(buf: &[u8], off: usize) -> f32 {
        f32::from_le_bytes(buf[off..off + 4].try_into().unwrap())
    }

    #[test]
    fn scissor_on_layout_is_byte_exact() {
        let mut w = RecordWriter::new(0x1000);
        w.scissor_on(200, 100, 880, 520);
        #[rustfmt::skip]
        let expected: [u8; 0x10] = [
            0x0C, 0x00,             // tag
            0x10, 0x00,             // size
            0x01, 0x00,             // enable
            0xC8, 0x00,             // x = 200
            0x64, 0x00,             // y = 100
            0x70, 0x03,             // w = 880
            0x08, 0x02,             // h = 520
            0x00, 0x00,             // pad
        ];
        assert_eq!(w.bytes(), &expected);
    }

    #[test]
    fn scissor_off_layout_is_byte_exact() {
        let mut w = RecordWriter::new(0);
        w.scissor_off();
        #[rustfmt::skip]
        let expected: [u8; 0x10] = [
            0x0C, 0x00, 0x10, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        assert_eq!(w.bytes(), &expected);
    }

    #[test]
    fn set_context_2d_layout_is_byte_exact() {
        let mut w = RecordWriter::new(0);
        w.set_context_2d(1280.0, 720.0, 0.0, 0.0);
        #[rustfmt::skip]
        let expected: [u8; 0x14] = [
            0x07, 0x00,             // tag
            0x14, 0x00,             // size
            0x00, 0x00, 0xA0, 0x44, // 1280.0
            0x00, 0x00, 0x34, 0x44, // 720.0
            0x00, 0x00, 0x00, 0x00, // 0.0
            0x00, 0x00, 0x00, 0x00, // 0.0
        ];
        assert_eq!(w.bytes(), &expected);
    }

    #[test]
    fn set_shader_layout_is_byte_exact() {
        let mut w = RecordWriter::new(0);
        w.set_shader(0x1122_3344_5566_7788, 1);
        #[rustfmt::skip]
        let expected: [u8; 0x18] = [
            0x13, 0x00,             // tag
            0x18, 0x00,             // size
            0x00, 0x00, 0x00, 0x00, // pad
            0x88, 0x77, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11, // shader ptr (LE)
            0x01, 0x00, 0x00, 0x00, // program index
            0x00, 0x00, 0x00, 0x00, // tail pad
        ];
        assert_eq!(w.bytes(), &expected);
    }

    #[test]
    fn set_texture_layout_is_byte_exact() {
        let mut w = RecordWriter::new(0);
        w.set_texture(0, 0x42, [1.0, 1.0, 0.0, 0.0]);
        #[rustfmt::skip]
        let expected: [u8; 0x1C] = [
            0x11, 0x00,             // tag
            0x1C, 0x00,             // size
            0x00, 0x00, 0x00, 0x00, // stage
            0x42, 0x00, 0x00, 0x00, // texture handle
            0x00, 0x00, 0x80, 0x3F, // 1.0
            0x00, 0x00, 0x80, 0x3F, // 1.0
            0x00, 0x00, 0x00, 0x00, // 0.0
            0x00, 0x00, 0x00, 0x00, // 0.0
        ];
        assert_eq!(w.bytes(), &expected);
    }

    #[test]
    fn vs_const_f_layout_and_absolute_payload_ptr() {
        let mut w = RecordWriter::new(0x1000);
        w.set_vs_const_f(0, &[[1.0, 2.0, 3.0, 4.0], [5.0, 6.0, 7.0, 8.0]]);
        let b = w.bytes();
        assert_eq!(b.len(), 0x38);
        assert_eq!(u16::from_le_bytes([b[0], b[1]]), TAG_SET_VS_CONST_F);
        assert_eq!(u16::from_le_bytes([b[2], b[3]]), 0x38); // 0x18 + 2*0x10
        assert_eq!(le32(b, 4), 0); // reg_off
        assert_eq!(le32(b, 8), 2); // n
        assert_eq!(le32(b, 0xC), 0); // pad zeroed
        assert_eq!(le64(b, 0x10), 0x1000 + 0x18); // absolute payload ptr
        for (i, v) in [1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]
            .iter()
            .enumerate()
        {
            assert_eq!(f32_at(b, 0x18 + i * 4), *v);
        }
    }

    #[test]
    fn empty_constant_and_quad_lists_are_no_ops() {
        let mut w = RecordWriter::new(0);
        w.set_vs_const_f(0, &[]);
        w.quads_untextured(&[]);
        assert!(w.is_empty());
    }

    #[test]
    fn untextured_quads_layout_and_absolute_payload_ptr() {
        let mut w = RecordWriter::new(0x2000);
        w.quads_untextured(&[Quad {
            corners: [[10.0, 20.0], [30.0, 20.0], [30.0, 40.0], [10.0, 40.0]],
            color: 0x80FF00FF,
        }]);
        let b = w.bytes();
        assert_eq!(b.len(), 0x34);
        assert_eq!(u16::from_le_bytes([b[0], b[1]]), TAG_QUADS_UNTEXTURED);
        assert_eq!(u16::from_le_bytes([b[2], b[3]]), 0x34); // 0x10 + 1*0x24
        assert_eq!(le32(b, 4), 1); // count
        assert_eq!(le64(b, 8), 0x2000 + 0x10); // absolute payload ptr
        let quad = 0x10;
        for (i, v) in [10.0f32, 20.0, 30.0, 20.0, 30.0, 40.0, 10.0, 40.0]
            .iter()
            .enumerate()
        {
            assert_eq!(f32_at(b, quad + i * 4), *v);
        }
        assert_eq!(le32(b, quad + 0x20), 0x80FF00FF);
    }

    #[test]
    fn multi_quad_payload_stride() {
        let q = Quad {
            corners: [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            color: 0xFFFFFFFF,
        };
        let mut w = RecordWriter::new(0);
        w.quads_untextured(&[q, q, q]);
        let b = w.bytes();
        assert_eq!(b.len(), 0x10 + 3 * 0x24);
        assert_eq!(u16::from_le_bytes([b[2], b[3]]) as usize, b.len());
        // Quad N's color dword sits at header + 0x10 + N*0x24 + 0x20.
        for n in 0..3 {
            assert_eq!(le32(b, 0x10 + n * 0x24 + 0x20), 0xFFFFFFFF);
        }
    }

    #[test]
    fn base_address_changes_only_pointer_fields() {
        let build = |base: u64| {
            let mut w = RecordWriter::new(base);
            w.scissor_on(0, 0, 10, 10);
            w.set_vs_const_f(0, &[[1.0, 2.0, 3.0, 4.0]]);
            w.quads_untextured(&[Quad {
                corners: [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
                color: 1,
            }]);
            w.bytes().to_vec()
        };
        let a = build(0x1000);
        let b = build(0x9000);
        assert_eq!(a.len(), b.len());
        // Pointer fields: vs_const payload ptr at 0x10+0x10, quad payload ptr
        // at 0x10+0x28+0x08 (scissor is 0x10, vs_const is 0x28).
        let ptr_offsets = [0x10 + 0x10, 0x10 + 0x28 + 0x08];
        for off in ptr_offsets {
            assert_eq!(le64(&a, off) + 0x8000, le64(&b, off));
        }
        // Everything else byte-identical.
        for i in 0..a.len() {
            let in_ptr = ptr_offsets.iter().any(|&o| i >= o && i < o + 8);
            if !in_ptr {
                assert_eq!(a[i], b[i], "byte {i} differs outside pointer fields");
            }
        }
    }

    #[test]
    fn poc_sequence_walks_back_exactly() {
        let mut w = RecordWriter::new(0x4000);
        w.set_context_2d(1280.0, 720.0, 0.0, 0.0);
        w.scissor_on(200, 100, 880, 520);
        w.set_vs_const_f(0, &[[0.5, 200.0, 100.0, 0.0], [880.0, 520.0, 0.0, 0.0]]);
        w.set_shader(0xAABB_CCDD_0000, 0);
        w.quads_untextured(&[Quad {
            corners: [
                [200.0, 100.0],
                [1080.0, 100.0],
                [1080.0, 620.0],
                [200.0, 620.0],
            ],
            color: 0x8000_0000,
        }]);
        w.set_shader(0xAABB_CCDD_0000, 0);
        w.scissor_off();

        let records = walk(w.bytes()).expect("well-formed chain");
        assert_eq!(
            records,
            vec![
                (TAG_SET_CONTEXT_2D, 0x14),
                (TAG_SCISSOR, 0x10),
                (TAG_SET_VS_CONST_F, 0x38),
                (TAG_SET_SHADER, 0x18),
                (TAG_QUADS_UNTEXTURED, 0x34),
                (TAG_SET_SHADER, 0x18),
                (TAG_SCISSOR, 0x10),
            ]
        );
        // Walk consumed the buffer exactly (no trailing bytes).
        let total: usize = records.iter().map(|(_, s)| s).sum();
        assert_eq!(total, w.len());
    }

    /// The production animated-background block (overlay-menu rewrite
    /// Step 8): the modal rect 60/60/1160/600, c48/c49 constants
    /// {time, x, y, 0} / {w, h, p0, p1}, a THEME program bind, restore
    /// to program 0. Pins the byte contract the emitter copies into the
    /// arena.
    #[test]
    fn production_background_sequence_walks_back_exactly() {
        const BASE: u64 = 0x9000;
        let (rx, ry, rw_, rh) = (60u16, 60u16, 1160u16, 520u16 + 80);
        let c48 = [12.5f32, rx as f32, ry as f32, 0.0];
        let c49 = [rw_ as f32, rh as f32, 0.0, 0.0];
        let mut w = RecordWriter::new(BASE);
        w.set_context_2d(1280.0, 720.0, 0.0, 0.0);
        w.scissor_on(rx, ry, rw_, rh);
        w.set_vs_const_f(0, &[c48, c49]);
        w.set_shader(0xDEAD_0000, 2); // theme program (arrows @ 2 w/ persp)
        w.quads_untextured(&[Quad {
            corners: [
                [rx as f32, ry as f32],
                [(rx + rw_) as f32, ry as f32],
                [(rx + rw_) as f32, (ry + rh) as f32],
                [rx as f32, (ry + rh) as f32],
            ],
            color: 0xFF00_0000,
        }]);
        w.set_shader(0xDEAD_0000, 0); // restore stock program
        w.scissor_off();

        let records = walk(w.bytes()).expect("well-formed chain");
        assert_eq!(
            records,
            vec![
                (TAG_SET_CONTEXT_2D, 0x14),
                (TAG_SCISSOR, 0x10),
                (TAG_SET_VS_CONST_F, 0x38),
                (TAG_SET_SHADER, 0x18),
                (TAG_QUADS_UNTEXTURED, 0x34),
                (TAG_SET_SHADER, 0x18),
                (TAG_SCISSOR, 0x10),
            ]
        );
        let total: usize = records.iter().map(|(_, s)| s).sum();
        assert_eq!(total, w.len());

        // c48/c49 payload bytes: the 0x14 record starts after context+scissor
        // (0x24); floats inline at +0x18, reg_off 0, count 2.
        let buf = w.bytes();
        let rec = 0x24usize;
        assert_eq!(
            u32::from_le_bytes(buf[rec + 4..rec + 8].try_into().unwrap()),
            0
        );
        assert_eq!(
            u32::from_le_bytes(buf[rec + 8..rec + 12].try_into().unwrap()),
            2
        );
        let payload_ptr = u64::from_le_bytes(buf[rec + 0x10..rec + 0x18].try_into().unwrap());
        assert_eq!(payload_ptr, BASE + rec as u64 + 0x18);
        for (i, v) in c48.iter().chain(c49.iter()).enumerate() {
            let off = rec + 0x18 + i * 4;
            assert_eq!(
                f32::from_le_bytes(buf[off..off + 4].try_into().unwrap()),
                *v,
                "constant float {i}"
            );
        }
        // The theme bind then the restore carry different program indices.
        let bind = 0x24 + 0x38;
        let restore = bind + 0x18 + 0x34;
        assert_eq!(
            u32::from_le_bytes(buf[bind + 0x10..bind + 0x14].try_into().unwrap()),
            2
        );
        assert_eq!(
            u32::from_le_bytes(buf[restore + 0x10..restore + 0x14].try_into().unwrap()),
            0
        );
    }

    #[test]
    fn walk_rejects_truncation_and_bad_sizes() {
        let mut w = RecordWriter::new(0);
        w.set_shader(0x1234, 0);
        let full = w.bytes().to_vec();

        // Truncated mid-record.
        let cut = &full[..full.len() - 4];
        assert_eq!(
            walk(cut),
            Err(WalkError::BadSize {
                offset: 0,
                tag: TAG_SET_SHADER,
                size: 0x18
            })
        );

        // Truncated mid-header.
        assert_eq!(walk(&full[..2]), Err(WalkError::Truncated { offset: 0 }));

        // Zero-size record (would loop the real walker forever).
        let mut zeroed = full.clone();
        zeroed[2] = 0;
        zeroed[3] = 0;
        assert_eq!(
            walk(&zeroed),
            Err(WalkError::BadSize {
                offset: 0,
                tag: TAG_SET_SHADER,
                size: 0
            })
        );

        // Empty buffer is a valid (empty) chain.
        assert_eq!(walk(&[]), Ok(vec![]));
    }
}
