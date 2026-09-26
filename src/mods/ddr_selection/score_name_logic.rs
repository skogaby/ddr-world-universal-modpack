//! DDR SELECTION themes — the gameplay dancer name (pure, host-tested): A3's
//! text rule, binding maths and style constants for the `agcs::BmpString`
//! `score_name.rs` binds to a theme difficulty frame's `name_usr`
//! placeholder (A3 ScoreActor init `FUN_180055390`, binding `FUN_180100480`).
//!
//! Dependency-free on purpose: `scripts/validate_ddr_selection.sh` mounts this
//! file into a throwaway host crate (with `score_set_logic`, whose World name
//! rule this reuses).

use super::score_set_logic;

/// `2d_font_player` (the font table order is A3's in World).
pub const FONT_PLAYER: i32 = 6;
/// A3's node sort key: right after BM2D group 1 (`0x7FFFFFFA`, the
/// difficulty frame) and before group 2 (`0x7FFFFFFC`, the P1 score frame).
pub const SORT_KEY: u32 = 0x7FFF_FFFB;
/// A3's colour, slot 2 `{B, G, R, A}` bytes.
pub const COLOR_BGRA: u32 = 0xFFFF_EB08;
/// `FUN_180100480(…, halign 1, valign 3, 1.0)`.
pub const HALIGN: i32 = 1;
pub const VALIGN: i32 = 3;
const BOX_SCALE: f32 = 1.0;
/// A3 `DAT_180288c60` (HD base, and the y factor), `DAT_1802942ac` (SD
/// base), `DAT_1802942a8` / `DAT_180294188` (x factor without / with a
/// profile).
const BASE_HD: f32 = 0.8;
const BASE_SD: f32 = 0.576;
const Y_FACTOR: f32 = 0.8;
const X_GUEST: f32 = 1.16;
const X_PROFILE: f32 = 1.6;
/// A3's colour byte scale (`DAT_1802888e4`).
const BYTE: f32 = 1.0 / 255.0;

/// The name for a side's `PlayerWork` (World's rule — `PLAYER1` / `PLAYER2`
/// for an empty name; the bot side's `PlayerWork` as-is). Non-ASCII bytes
/// (never in a DDR name) become `?`.
pub fn name_text(raw: &[u8], side_index: u32) -> String {
    score_set_logic::player_name(raw, side_index)
        .iter()
        .map(|&b| {
            if b.is_ascii_graphic() || b == b' ' {
                b as char
            } else {
                '?'
            }
        })
        .collect()
}

/// A3's style: colour and scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style {
    /// (r, g, b, a) — `desc+0x20..+0x2C`.
    pub color: (f32, f32, f32, f32),
    /// (x, y) — `desc+0x58 / +0x5C`.
    pub scale: (f32, f32),
}

/// A3: the colour from [`COLOR_BGRA`]; scale y = base × 0.8, x = base ×
/// (1.6 with a profile — A3's `PlayerWork+1`, World's `+5` — else 1.16);
/// base 0.8 (0.576 on SD cabinets, machine types 0 / 1).
pub fn style(profile: bool, sd_cabinet: bool) -> Style {
    let b = COLOR_BGRA.to_le_bytes();
    let base = if sd_cabinet { BASE_SD } else { BASE_HD };
    let x = if profile { X_PROFILE } else { X_GUEST };
    Style {
        color: (
            b[2] as f32 * BYTE,
            b[1] as f32 * BYTE,
            b[0] as f32 * BYTE,
            b[3] as f32 * BYTE,
        ),
        scale: (x * base, base * Y_FACTOR),
    }
}

/// The placeholder's MovieClip reads (`afp_mc_get_param`): position
/// `0x1008`, width `0x1015`, height `0x1016`, scale `0x100D`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub sx: f32,
    pub sy: f32,
}

/// Where the string goes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Binding {
    /// `set_position` (slot 0: `desc+0x4C / +0x50`).
    pub position: (f32, f32),
    /// The layout box (`desc+0x68 / +0x6C`).
    pub box_lr: (f32, f32),
    pub halign: i32,
    pub valign: i32,
}

/// A3 `FUN_180100480(placeholder, text, 1, 3, 1.0)`, instruction for
/// instruction (x87-free SSE: `cvttss2si` truncates toward zero):
/// `x = trunc(px + 0.5)`, `y = trunc(py + 0.5)`, half width
/// `trunc(w)·sx·0.5`, half height `trunc(h)·sy·0.5`; the centred box
/// `x ± half width`; `valign > 1` ⇒ `y = trunc(y + half height)`.
pub fn binding(r: &Rect) -> Binding {
    let x = (r.x + 0.5) as i32;
    let mut y = (r.y + 0.5) as i32;
    let half_w = (r.w as i32) as f32 * r.sx * 0.5;
    let half_h = (r.h as i32) as f32 * r.sy * 0.5;
    let xf = x as f32;
    let box_lr = (xf - half_w * BOX_SCALE, half_w * BOX_SCALE + xf);
    if VALIGN > 1 {
        y = (y as f32 + half_h) as i32;
    }
    Binding {
        position: (xf, y as f32),
        box_lr,
        halign: HALIGN,
        valign: VALIGN,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_rule() {
        assert_eq!(name_text(b"DANCER\0\0\0", 0), "DANCER");
        assert_eq!(name_text(b"\0\0\0\0\0\0\0\0\0", 0), "PLAYER1");
        assert_eq!(name_text(b"\0", 1), "PLAYER2");
        assert_eq!(name_text(b"", 2), "PLAYER");
        assert_eq!(name_text(b"A.B-C!?&", 0), "A.B-C!?&");
        assert_eq!(name_text(b"ABCDEFGHIJ", 1), "ABCDEFGH");
        assert_eq!(name_text(b"A\x80B\x01", 0), "A?B?");
    }

    #[test]
    fn style_constants_bit_exact() {
        let hd = style(true, false);
        assert_eq!(hd.color.0.to_bits(), 0x3f80_0000);
        assert_eq!(hd.color.1.to_bits(), 0x3f6b_ebed);
        assert_eq!(hd.color.2.to_bits(), 0x3d00_8081);
        assert_eq!(hd.color.3.to_bits(), 0x3f80_0000);
        assert_eq!(hd.scale.0.to_bits(), (1.6f32 * 0.8f32).to_bits());
        assert_eq!(hd.scale.1.to_bits(), (0.8f32 * 0.8f32).to_bits());
        assert!((hd.scale.0 - 1.28).abs() < 1e-6);
        assert!((hd.scale.1 - 0.64).abs() < 1e-6);
        let guest = style(false, false);
        assert!((guest.scale.0 - 0.928).abs() < 1e-6);
        assert_eq!(guest.scale.1, hd.scale.1);
        let sd = style(false, true);
        assert!((sd.scale.0 - 1.16 * 0.576).abs() < 1e-6);
        assert!((sd.scale.1 - 0.576 * 0.8).abs() < 1e-6);
        assert_eq!(sd.color, hd.color);
        assert_eq!(SORT_KEY, 0x7FFF_FFFB);
        assert_eq!(FONT_PLAYER, 6);
    }

    #[test]
    fn binding_matches_a3() {
        // Hand-computed from FUN_180100480: x = trunc(100.4 + 0.5) = 100,
        // y = trunc(50.7 + 0.5) = 51; half width = trunc(120.9)·1.0·0.5 = 60;
        // half height = trunc(20.9)·1.0·0.5 = 10 ⇒ y = trunc(51 + 10) = 61.
        let b = binding(&Rect {
            x: 100.4,
            y: 50.7,
            w: 120.9,
            h: 20.9,
            sx: 1.0,
            sy: 1.0,
        });
        assert_eq!(b.position, (100.0, 61.0));
        assert_eq!(b.box_lr, (40.0, 160.0));
        assert_eq!((b.halign, b.valign), (1, 3));

        // Scaled placeholder: x = trunc(640.1) = 640, y = trunc(199.99) = 199;
        // half width = 120·0.5·0.5 = 30, half height = 20·0.75·0.5 = 7.5 ⇒
        // y = trunc(199 + 7.5) = 206.
        let b = binding(&Rect {
            x: 639.6,
            y: 199.49,
            w: 120.0,
            h: 20.0,
            sx: 0.5,
            sy: 0.75,
        });
        assert_eq!(b.position, (640.0, 206.0));
        assert_eq!(b.box_lr, (610.0, 670.0));

        // Truncation toward zero (cvttss2si) on negative coordinates.
        let b = binding(&Rect {
            x: -10.9,
            y: -3.2,
            w: 10.0,
            h: 3.0,
            sx: 1.0,
            sy: 1.0,
        });
        // trunc(-10.4) = -10; trunc(-2.7) = -2; half h = 1.5 ⇒ trunc(-0.5) = 0.
        assert_eq!(b.position, (-10.0, 0.0));
        assert_eq!(b.box_lr, (-15.0, -5.0));
    }
}
