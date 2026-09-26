//! DDR SELECTION legacy life-gauge fill math (pure, host-tested).
//!
//! Dependency-free on purpose: `scripts/validate_ddr_selection.sh` mounts this
//! file into a throwaway host crate and runs the `#[cfg(test)]` suite there.
//!
//! A port of A3's two gauge fills (`gamemdx_20240402`): the continuous fill
//! `FUN_1800544b0` (skins 2–4 and every FLARE state) and the segmented fill
//! `FUN_180054050` (skins 1 and 5, non-FLARE — skin 0 too in A3, which World
//! replaced with its own art and continuous fill; the themes, A3's own skin-0
//! UI, take it again). Both write the `fill _usr`
//! clip's `{x, y, w, h}` scissor (MovieClip param `0x1023`) in screen space
//! and mirror it for the 2P gauge (A3 draws the 2P gauge at `SetScale(-1,1)`).
//! The segmented fill also crops the partial cell `fill _2_usr` (skin 5).
//!
//! Only the HD constants are ported: World always creates the HD export
//! (`00_dance_gauge`), never A3's SD one.
//!
//! RE: `.agents/planning/2026-09-22-ddr-selection/research/legacy-gauge.md`.

/// How A3 fills a legacy gauge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FillMode {
    Continuous,
    /// `cells` × `cell_w` px; `partial` = the partial-cell crop is drawn.
    Segmented {
        cells: i32,
        cell_w: f32,
        partial: bool,
    },
}

/// A3's HD cell width of the 1st-5th gauge (`DAT_180288d60` = 0x40DF7DF8).
pub const SKIN1_CELL_W: f32 = 6.984127;
/// A3's HD cell width of the other segmented gauges (`DAT_180288d58`).
pub const CELL_W: f32 = 17.0;
/// A3's HD cell counts (skin 1 / the others).
pub const SKIN1_CELLS: i32 = 63;
pub const CELLS: i32 = 26;

/// The fill A3 uses for `skin` (1..=5 eras, 6..=8 themes) with the gauge's
/// current state label (World's `vt+0x58(state)`: 1 normal, 2 rainbow,
/// 3 danger, 4 grade, 5 check, 6..=16 the FLARE labels): `skin - 2 < 3` or a
/// FLARE label ⇒ continuous, else segmented (skin 1's 63 thin cells; skin 5
/// and A3's own skin 0 — the themes — 26 cells with the partial cell).
pub fn fill_mode(skin: u8, label: i32) -> FillMode {
    if (2..=4).contains(&skin) || (6..=16).contains(&label) {
        return FillMode::Continuous;
    }
    match skin {
        1 => FillMode::Segmented {
            cells: SKIN1_CELLS,
            cell_w: SKIN1_CELL_W,
            partial: false,
        },
        // 5 and the themes (A3's "skins 0 / 5").
        _ => FillMode::Segmented {
            cells: CELLS,
            cell_w: CELL_W,
            partial: true,
        },
    }
}

/// The `fill _usr` MovieClip's reads: screen position (param `0x1008`),
/// truncated width / height (`0x1015` / `0x1016`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FillClip {
    pub x: f32,
    pub y: f32,
    pub w: i32,
    pub h: i32,
}

/// One fill frame: `None` = the gauge is full (both clips hidden), else the
/// `fill _usr` scissor and, when drawn, the `fill _2_usr` crop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fill {
    pub main: [i32; 4],
    pub partial: Option<[i32; 4]>,
}

/// A3's clamp of the displayed value (`+0x94`) to 0..=1.
pub fn clamp01(v: f32) -> f32 {
    if v > 1.0 {
        1.0
    } else if v < 0.0 {
        0.0
    } else {
        v
    }
}

/// The frame's fill for `value` (the actor's displayed gauge, clamped here),
/// `side` 0 = 1P / else 2P (mirrored).
pub fn fill(mode: FillMode, value: f32, side: u8, clip: FillClip) -> Option<Fill> {
    let g = clamp01(value);
    if g >= 1.0 {
        return None;
    }
    let cx = (clip.x + 0.5) as i32;
    let cy = (clip.y + 0.5) as i32;
    let (w, h) = (clip.w, clip.h);
    let y = cy - h / 2;
    // A3's `x` for `filled` px from the gauge's origin edge.
    let left = |filled: f32| -> f32 {
        if side == 0 {
            (cx - w / 2) as f32 + filled
        } else {
            ((w / 2 + cx) as f32 - filled) - w as f32
        }
    };
    match mode {
        FillMode::Continuous => Some(Fill {
            main: [left(w as f32 * g) as i32, y, w, h],
            partial: None,
        }),
        FillMode::Segmented {
            cells,
            cell_w,
            partial,
        } => {
            let t = cells as f32 * g;
            let whole = (t as i32) as f32;
            let frac = t - whole;
            let mut x = left(whole * cell_w) as i32;
            let mut crop = None;
            if frac > 0.0 && partial {
                let cw = (cell_w + 0.5) as i32;
                let px = if side == 0 { x } else { (w - cw) + x };
                crop = Some([px, y, cw, ((1.0 - frac) * h as f32) as i32]);
                x = if side == 0 { x + cw } else { x - cw };
            }
            Some(Fill {
                main: [x, y, w, h],
                partial: crop,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLIP: FillClip = FillClip {
        x: 277.6,
        y: 36.2,
        w: 442,
        h: 20,
    };

    #[test]
    fn mode_follows_a3() {
        assert_eq!(fill_mode(2, 1), FillMode::Continuous);
        assert_eq!(fill_mode(4, 3), FillMode::Continuous);
        assert_eq!(fill_mode(1, 6), FillMode::Continuous);
        assert_eq!(fill_mode(5, 16), FillMode::Continuous);
        assert!(matches!(
            fill_mode(1, 1),
            FillMode::Segmented {
                cells: 63,
                partial: false,
                ..
            }
        ));
        assert!(matches!(
            fill_mode(5, 5),
            FillMode::Segmented {
                cells: 26,
                partial: true,
                ..
            }
        ));
        // grade / check labels stay segmented
        assert!(matches!(fill_mode(5, 4), FillMode::Segmented { .. }));
    }

    #[test]
    fn constants_match_a3_bits() {
        assert_eq!(SKIN1_CELL_W.to_bits(), 0x40DF_7DF8);
        assert_eq!(CELL_W.to_bits(), 0x4188_0000);
    }

    #[test]
    fn full_gauge_hides_both() {
        assert_eq!(fill(FillMode::Continuous, 1.0, 0, CLIP), None);
        assert_eq!(fill(fill_mode(1, 1), 1.3, 1, CLIP), None);
    }

    #[test]
    fn continuous_moves_the_scissor_by_the_value() {
        // cx = 278, cy = 36, left edge = 278 - 221 = 57
        let f = fill(FillMode::Continuous, 0.5, 0, CLIP).unwrap();
        assert_eq!(f.main, [57 + 221, 36 - 10, 442, 20]);
        assert_eq!(f.partial, None);
        // 2P mirrored: right edge 278 + 221 = 499, minus filled, minus w
        let f = fill(FillMode::Continuous, 0.5, 1, CLIP).unwrap();
        assert_eq!(f.main, [499 - 221 - 442, 26, 442, 20]);
        // empty gauge: the scissor starts at the origin edge
        assert_eq!(
            fill(FillMode::Continuous, -0.3, 0, CLIP).unwrap().main[0],
            57
        );
    }

    #[test]
    fn skin1_segments_whole_cells_only() {
        // 63 * 0.5 = 31.5 -> 31 cells, no partial cell on skin 1
        let f = fill(fill_mode(1, 1), 0.5, 0, CLIP).unwrap();
        assert_eq!(f.main[0], (57.0 + 31.0 * SKIN1_CELL_W) as i32);
        assert_eq!(f.partial, None);
    }

    #[test]
    fn segmented_partial_cell_shifts_the_scissor() {
        // 26 * 0.5 = 13 exactly: no partial cell
        let f = fill(fill_mode(5, 1), 0.5, 0, CLIP).unwrap();
        assert_eq!(f.main[0], 57 + 13 * 17);
        assert_eq!(f.partial, None);
        // 26 * 0.25 = 6.5: 6 cells + a half-height crop one cell wide
        let f = fill(fill_mode(5, 1), 0.25, 0, CLIP).unwrap();
        let x = 57 + 6 * 17;
        assert_eq!(f.partial, Some([x, 26, 17, 10]));
        assert_eq!(f.main[0], x + 17);
        // 2P: crop at the far end of the cell, scissor moves left
        let f = fill(fill_mode(5, 1), 0.25, 1, CLIP).unwrap();
        let x = 499 - 6 * 17 - 442;
        assert_eq!(f.partial, Some([(442 - 17) + x, 26, 17, 10]));
        assert_eq!(f.main[0], x - 17);
    }

    #[test]
    fn themes_fill_like_a3_skin_0() {
        // A3's "skins 0 / 5" segmented fill: 26 cells of 17 px with the
        // partial cell; every FLARE label continuous.
        for skin in 6..=8 {
            for label in [1, 2, 3, 4, 5] {
                assert_eq!(
                    fill_mode(skin, label),
                    FillMode::Segmented {
                        cells: CELLS,
                        cell_w: CELL_W,
                        partial: true,
                    },
                    "skin {skin} label {label}"
                );
            }
            for label in 6..=16 {
                assert_eq!(fill_mode(skin, label), FillMode::Continuous);
            }
        }
    }
}
