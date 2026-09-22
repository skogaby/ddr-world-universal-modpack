//! Pure layout + encoding half of the viewport-pass compositor (design
//! §4.5): the mod-owned clear viewport's `#[repr(C)]` image, the gd Clear
//! record encoder, the canvas→render-target rect mapping and the per-side
//! constant tables. Dependency-free (std only) so the host harness mounts
//! it; `viewport_pass.rs` is the engine-facing half.
//!
//! Engine facts (research `preview-compositing.md`, Ghidra 2026-09-21): a
//! `gs::Viewport::Base` sub-object is `{vtable*, +8 rect{x,y,w,h}, +0x18
//! minZ, +0x1C maxZ, +0x20 name hash, +0x24 flags}` and the MODEL passes
//! continue with `+0x28 proj[16], +0x68 view[16]`. Flags: bit0 = DISABLED
//! (the dispatcher skips the viewport), bit1 = skip the camera-matrix upload
//! (the worker still applies the rect). The dispatcher's own list Clear is
//! `{u16 tag 0, u16 size 0x14, u32 D3DCLEAR flags, u32 D3DCOLOR (ARGB), f32
//! z, u32 stencil}`; D3D9 `Clear` with no rects clears the CURRENT viewport,
//! which the worker set to ours one call earlier.

use std::mem::{offset_of, size_of};

/// The 1280×720 logical canvas every AFP layer is authored in.
pub const CANVAS_W: f32 = 1280.0;
pub const CANVAS_H: f32 = 720.0;

/// RENDER_2D priorities: the three 2D layer lists sit at 0x65..0x67, the
/// per-side triple `clear / opaque / trans` follows at `base, +1, +2`.
pub const PRIO_BASE: [u32; 2] = [0x68, 0x6B];
/// Private node-mask bits per side — clear in every stock pass filter
/// (`0x01 / 0x56 / 0x10 / 0x46`), verified live by `viewport_pass`.
pub const FILTER_BIT: [u32; 2] = [0x08, 0x20];
/// Every stock filter bit — the live check refuses a build whose passes
/// use a private bit.
pub const STOCK_FILTER_UNION: u32 = 0x01 | 0x56 | 0x10 | 0x46;

/// Viewport flags.
pub const VP_FLAG_DISABLED: u32 = 1;
pub const VP_FLAG_SKIP_CAMERA: u32 = 2;

/// D3DCLEAR flags.
pub const D3DCLEAR_TARGET: u32 = 1;
pub const D3DCLEAR_ZBUFFER: u32 = 2;
pub const D3DCLEAR_STENCIL: u32 = 4;

/// The gd Clear record header (`u16 tag 0, u16 size 0x14`) and size.
pub const CLEAR_TAG: u32 = 0x0014_0000;
pub const CLEAR_RECORD_SIZE: usize = 0x14;

/// The mod-owned "clear" viewport object: the engine's `Viewport::Base`
/// header (through `flags`) followed by the Clear payload our render
/// callback emits. 0x40 bytes; `flags` carries `VP_FLAG_SKIP_CAMERA` so the
/// worker applies the rect and nothing else before calling slot 0.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ClearViewport {
    pub vtable: usize,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub min_z: f32,
    pub max_z: f32,
    pub name_hash: u32,
    pub flags: u32,
    pub clear_flags: u32,
    pub color_argb: u32,
    pub z: f32,
    pub stencil: u32,
    pub _pad: [u32; 2],
}

pub const CLEAR_VIEWPORT_SIZE: usize = 0x40;
pub const VP_RECT_OFF: usize = 8;
pub const VP_FLAGS_OFF: usize = 0x24;
pub const VP_PAYLOAD_OFF: usize = 0x28;

const _: () = assert!(size_of::<ClearViewport>() == CLEAR_VIEWPORT_SIZE);
const _: () = assert!(offset_of!(ClearViewport, x) == VP_RECT_OFF);
const _: () = assert!(offset_of!(ClearViewport, min_z) == 0x18);
const _: () = assert!(offset_of!(ClearViewport, max_z) == 0x1C);
const _: () = assert!(offset_of!(ClearViewport, name_hash) == 0x20);
const _: () = assert!(offset_of!(ClearViewport, flags) == VP_FLAGS_OFF);
const _: () = assert!(offset_of!(ClearViewport, clear_flags) == VP_PAYLOAD_OFF);

/// A rectangle in RENDER-TARGET pixels (the D3D viewport the pass renders
/// into).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RtRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl RtRect {
    /// Map a 1280×720-canvas rect onto a `dims`-sized render target
    /// (rounded to whole pixels; degenerate sizes clamp to 1×1).
    pub fn from_canvas(x: f32, y: f32, w: f32, h: f32, dims: (u32, u32)) -> RtRect {
        let sx = dims.0 as f32 / CANVAS_W;
        let sy = dims.1 as f32 / CANVAS_H;
        let x0 = (x * sx).round();
        let y0 = (y * sy).round();
        let x1 = ((x + w) * sx).round();
        let y1 = ((y + h) * sy).round();
        RtRect {
            x: x0 as i32,
            y: y0 as i32,
            w: ((x1 - x0) as i32).max(1),
            h: ((y1 - y0) as i32).max(1),
        }
    }

    /// Width / height ratio (the camera aspect).
    pub fn aspect(&self) -> f32 {
        if self.h <= 0 {
            1.0
        } else {
            self.w as f32 / self.h as f32
        }
    }
}

/// What the clear viewport clears: the depth buffer (always for a preview —
/// the AFP layers under us may have written depth) and optionally the
/// colour (an opaque backdrop behind a dancer; the stage's skydome fills
/// its box itself).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClearSpec {
    pub depth: bool,
    /// ARGB.
    pub color: Option<u32>,
}

impl ClearSpec {
    pub fn d3d_flags(&self) -> u32 {
        (if self.depth { D3DCLEAR_ZBUFFER } else { 0 })
            | (if self.color.is_some() {
                D3DCLEAR_TARGET
            } else {
                0
            })
    }
}

/// The 0x14-byte gd Clear record the callback appends: header, D3DCLEAR
/// flags, D3DCOLOR, z = 1.0, stencil 0 — the dispatcher's own shape.
pub fn encode_clear_record(spec: &ClearSpec) -> [u8; CLEAR_RECORD_SIZE] {
    let mut out = [0u8; CLEAR_RECORD_SIZE];
    out[0..4].copy_from_slice(&CLEAR_TAG.to_le_bytes());
    out[4..8].copy_from_slice(&spec.d3d_flags().to_le_bytes());
    out[8..12].copy_from_slice(&spec.color.unwrap_or(0).to_le_bytes());
    out[12..16].copy_from_slice(&1.0f32.to_le_bytes());
    out[16..20].copy_from_slice(&0u32.to_le_bytes());
    out
}

/// Whether the four live stock filters leave both private bits clear.
pub fn private_bits_free(filters: [u32; 4]) -> bool {
    let private = FILTER_BIT[0] | FILTER_BIT[1];
    filters.iter().all(|f| f & private == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clear_viewport_layout_pins() {
        assert_eq!(size_of::<ClearViewport>(), 0x40);
        assert_eq!(offset_of!(ClearViewport, vtable), 0);
        assert_eq!(offset_of!(ClearViewport, x), 8);
        assert_eq!(offset_of!(ClearViewport, h), 0x14);
        assert_eq!(offset_of!(ClearViewport, flags), 0x24);
        assert_eq!(offset_of!(ClearViewport, clear_flags), 0x28);
        assert_eq!(offset_of!(ClearViewport, stencil), 0x34);
    }

    #[test]
    fn clear_record_bytes() {
        let rec = encode_clear_record(&ClearSpec {
            depth: true,
            color: Some(0xFF20A0FF),
        });
        assert_eq!(
            rec,
            [
                0x00, 0x00, 0x14, 0x00, // tag 0, size 0x14
                0x03, 0x00, 0x00, 0x00, // TARGET | ZBUFFER
                0xFF, 0xA0, 0x20, 0xFF, // D3DCOLOR 0xFF20A0FF little-endian
                0x00, 0x00, 0x80, 0x3F, // z = 1.0
                0x00, 0x00, 0x00, 0x00, // stencil
            ]
        );
        let depth_only = encode_clear_record(&ClearSpec {
            depth: true,
            color: None,
        });
        assert_eq!(&depth_only[4..8], &[2, 0, 0, 0]);
        assert_eq!(&depth_only[8..12], &[0, 0, 0, 0]);
        assert_eq!(
            ClearSpec {
                depth: false,
                color: Some(1)
            }
            .d3d_flags(),
            D3DCLEAR_TARGET
        );
    }

    #[test]
    fn rect_mapping_at_three_resolutions() {
        // The P1 preview box: chrome origin (185, 463) + marker (191, 11, 170, 150).
        let (x, y, w, h) = (376.0, 474.0, 170.0, 150.0);
        assert_eq!(
            RtRect::from_canvas(x, y, w, h, (1280, 720)),
            RtRect {
                x: 376,
                y: 474,
                w: 170,
                h: 150
            }
        );
        assert_eq!(
            RtRect::from_canvas(x, y, w, h, (1920, 1080)),
            RtRect {
                x: 564,
                y: 711,
                w: 255,
                h: 225
            }
        );
        // 640×480 SD (4:3 output: the canvas is squeezed horizontally).
        let sd = RtRect::from_canvas(x, y, w, h, (640, 480));
        assert_eq!(
            sd,
            RtRect {
                x: 188,
                y: 316,
                w: 85,
                h: 100
            }
        );
        assert!(
            (RtRect::from_canvas(0.0, 0.0, 1280.0, 720.0, (1920, 1080)).aspect() - 16.0 / 9.0)
                .abs()
                < 1e-6
        );
        assert_eq!(RtRect::from_canvas(0.0, 0.0, 0.1, 0.1, (1280, 720)).w, 1);
    }

    #[test]
    fn constant_tables() {
        assert_eq!(PRIO_BASE, [0x68, 0x6B]);
        assert_eq!(FILTER_BIT, [0x08, 0x20]);
        assert_eq!(STOCK_FILTER_UNION & (FILTER_BIT[0] | FILTER_BIT[1]), 0);
        assert!(private_bits_free([0x01, 0x56, 0x10, 0x46]));
        assert!(!private_bits_free([0x01, 0x5E, 0x10, 0x46]));
        assert!(!private_bits_free([0x21, 0x56, 0x10, 0x46]));
        assert_eq!(CLEAR_TAG >> 16, CLEAR_RECORD_SIZE as u32);
    }
}
