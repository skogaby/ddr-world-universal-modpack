//! DDR Universe per-bar logic — PURE (std only, harness-mounted by
//! `scripts/validate_background_dancers.sh`): the Render Param colour
//! palette, the STENCIL colour cycle and RANDOM (EVERY BAR)'s per-bar pick.
//! RE: `docs/ddr_universe_render_styles_research.md` §1 (palette builder
//! `0x82180978`, HSV `0x822521F8`).
//!
//! Both per-bar choices come from a shuffled cycle: bar `b` lies in block
//! `b div n`, whose order is a Fisher–Yates shuffle seeded by
//! `(seed, block)`, so every choice shows once per `n` bars and two
//! consecutive bars never repeat (a block whose first entry equals the
//! previous block's last swaps its first two entries). Deterministic: the
//! same song seed and bar give the same pick every frame, after a rewind,
//! and in both players' previews.

/// Universe's byte-scale HSV → RGB (`0x822521F8`, verbatim integer math):
/// `hue` 0..255 maps to six 43-wide sectors.
pub fn hsv_rgb(hue: u8, sat: u8, val: u8) -> [u8; 3] {
    let (h, s, v) = (hue as i32, sat as i32, val as i32);
    if s == 0 {
        return [val; 3];
    }
    let x = h * 6;
    let f = x & 0xFF;
    let sector = x >> 8;
    let q = (((255 - ((s * f) >> 8)) * v) / 255) & 0xFF;
    let p = (((255 - s) * v) / 255) & 0xFF;
    let t = (((255 - (((256 - f) * s) >> 8)) * v) / 255) & 0xFF;
    let (r, g, b) = match sector {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    [r as u8, g as u8, b as u8]
}

/// Palette entries (17 columns × 9 rows).
pub const PALETTE_LEN: usize = 153;

/// Universe's Render Param palette entry `index` (a CSV cell `-index`): row
/// `index / 17`, column `index % 17`. Columns 0–15 are hues `15·c`; rows 0–3
/// saturation `50·(r+1)` at full value, rows 4–8 full saturation at value
/// 255 / 155 / 105 / 55 / 25; column 16 is grey from white (row 0) to black
/// (row 8). Out-of-range indices read as white.
pub fn palette_rgb(index: usize) -> [u8; 3] {
    if index >= PALETTE_LEN {
        return [255; 3];
    }
    let (row, col) = ((index / 17) as i32, (index % 17) as i32);
    if col == 16 {
        let v = 255 - ((row * 256 - row) >> 3);
        return hsv_rgb(0, 0, v as u8);
    }
    let (sat, val) = if row < 4 {
        ((row + 1) * 50, 255)
    } else {
        let v = match row {
            4 => 255,
            8 => 25,
            r => (r * -50 + 155) & 0xFF,
        };
        (255, v)
    };
    hsv_rgb((15 * col) as u8, sat as u8, val as u8)
}

/// The palette entries Universe's charts set as a uniform STENCIL fill, most
/// used first (the 83 song CSVs: `-16` white 81 bars, `-152` black 50, `-68`
/// red 24, …). STENCIL cycles through them, one per bar.
pub const STENCIL_PALETTE: [usize; 14] = [16, 152, 68, 37, 53, 70, 55, 71, 60, 54, 72, 56, 82, 81];

/// STENCIL's fill colour for `bar` (linear 0..1 RGB, the item tint).
pub fn stencil_colour(seed: u64, bar: i64) -> [f32; 3] {
    let i = shuffled_pick(seed ^ 0x5737_E4C1_1F11_0000, bar, STENCIL_PALETTE.len());
    let rgb = palette_rgb(STENCIL_PALETTE[i]);
    [
        rgb[0] as f32 / 255.0,
        rgb[1] as f32 / 255.0,
        rgb[2] as f32 / 255.0,
    ]
}

/// SplitMix64 finaliser.
fn mix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

/// Block `block`'s shuffle of `0..n` before the no-repeat fix.
fn block_order(seed: u64, block: i64, n: usize) -> Vec<usize> {
    let mut v: Vec<usize> = (0..n).collect();
    let mut state = mix(seed ^ (block as u64).wrapping_mul(0xD1B5_4A32_D192_ED03));
    for i in (1..n).rev() {
        state = mix(state);
        let j = (state % (i as u64 + 1)) as usize;
        v.swap(i, j);
    }
    v
}

/// The choice (`0..n`) for `bar` — see the module docs. `n == 0` ⇒ 0.
pub fn shuffled_pick(seed: u64, bar: i64, n: usize) -> usize {
    if n <= 1 {
        return 0;
    }
    if n == 2 {
        // Alternate (the block fix below would move a 2-block's last entry).
        return (bar.rem_euclid(2) as usize + (mix(seed) & 1) as usize) % 2;
    }
    let n_i = n as i64;
    let block = bar.div_euclid(n_i);
    let pos = bar.rem_euclid(n_i) as usize;
    let mut order = block_order(seed, block, n);
    // The previous block's LAST entry is never moved by its own fix (that
    // only swaps entries 0 and 1, and n ≥ 3 here).
    let prev_last = block_order(seed, block - 1, n)[n - 1];
    if order[0] == prev_last {
        order.swap(0, 1);
    }
    order[pos]
}

/// The chart bar at measure position `ticks` (4096 per bar; a fractional
/// position floors, negative positions — before music 0 — count down).
pub fn bar_at_ticks(ticks: f64) -> i64 {
    (ticks / 4096.0).floor() as i64
}

/// The bar without a tempo map: a 120-BPM bar (2 s) of `seconds`.
pub fn bar_at_seconds(seconds: f64) -> i64 {
    (seconds / 2.0).floor() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_matches_universes_named_entries() {
        assert_eq!(palette_rgb(16), [255, 255, 255]);
        assert_eq!(palette_rgb(152), [0, 0, 0]);
        assert_eq!(palette_rgb(68), [255, 0, 0]);
        // Values computed from the decompiled builder (research §1).
        assert_eq!(palette_rgb(37), [247, 255, 105]);
        assert_eq!(palette_rgb(60), [55, 223, 255]);
        assert_eq!(palette_rgb(82), [236, 0, 255]);
        assert_eq!(palette_rgb(999), [255, 255, 255]);
        // The grey column runs white → black.
        let greys: Vec<u8> = (0..9).map(|r| palette_rgb(r * 17 + 16)[0]).collect();
        assert!(greys.windows(2).all(|w| w[0] > w[1]));
        assert!(palette_rgb(4 * 17 + 3).iter().any(|&c| c == 255));
    }

    #[test]
    fn shuffled_pick_covers_every_choice_per_block_and_never_repeats() {
        for n in [2usize, 3, 7, 14, 18] {
            for seed in [0u64, 1, 0xDEAD_BEEF, u64::MAX] {
                let picks: Vec<usize> = (-40..200).map(|b| shuffled_pick(seed, b, n)).collect();
                assert!(picks.iter().all(|&p| p < n));
                assert!(
                    picks.windows(2).all(|w| w[0] != w[1]),
                    "n={} seed={:#x}: {:?}",
                    n,
                    seed,
                    picks
                );
                // Every aligned block of n bars shows every choice once.
                for block in 0..5i64 {
                    let mut seen: Vec<usize> = (0..n as i64)
                        .map(|k| shuffled_pick(seed, block * n as i64 + k, n))
                        .collect();
                    seen.sort_unstable();
                    assert_eq!(seen, (0..n).collect::<Vec<_>>());
                }
            }
        }
        assert_eq!(shuffled_pick(7, 3, 0), 0);
        assert_eq!(shuffled_pick(7, 3, 1), 0);
        // Deterministic.
        assert_eq!(shuffled_pick(42, 17, 9), shuffled_pick(42, 17, 9));
    }

    #[test]
    fn stencil_cycles_universe_colours() {
        let seed = 0x1234_5678;
        let cols: Vec<[f32; 3]> = (0..14).map(|b| stencil_colour(seed, b)).collect();
        // One block = every palette entry once (white and black included).
        assert!(cols.contains(&[1.0, 1.0, 1.0]));
        assert!(cols.contains(&[0.0, 0.0, 0.0]));
        assert!(cols.windows(2).all(|w| w[0] != w[1]));
        // Distinct entries really are distinct colours.
        let mut rgb: Vec<[u8; 3]> = STENCIL_PALETTE.iter().map(|&i| palette_rgb(i)).collect();
        rgb.sort_unstable();
        rgb.dedup();
        assert_eq!(rgb.len(), STENCIL_PALETTE.len());
    }

    #[test]
    fn bars() {
        assert_eq!(bar_at_ticks(0.0), 0);
        assert_eq!(bar_at_ticks(4095.9), 0);
        assert_eq!(bar_at_ticks(4096.0), 1);
        assert_eq!(bar_at_ticks(-1.0), -1);
        assert_eq!(bar_at_seconds(3.9), 1);
        assert_eq!(bar_at_seconds(-0.5), -1);
    }
}
