//! Pure layer of the S-Marvelous receptor burst (`receptor.rs`): the
//! operator's flash-colour choice, the lane bitset the game's pusher takes,
//! the white-quad discriminator, and the violet recolour. std-only —
//! mounted by `scripts/validate_s_marvelous.sh` for its host tests.

/// Maximum panels per side (doubles) — the pusher takes a `u8` bitset.
pub const MAX_LANES: u32 = 8;

/// What the receptor shows on an S-Marvelous hit. Chosen by the "Receptor
/// Flash Color" overlay row / `s_marvelous.receptor_flash`.
///
/// The `dance_effect` bomb is stock in BOTH modes (S-Marv's bomb IS the
/// white Marvelous bomb); the choice is only whether the mod ALSO pushes
/// its violet `JudgeEffectRenderer` burst on top of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReceptorFlash {
    /// Push the type-7 burst and recolour it violet (the 2026-09-12 look).
    Purple,
    /// Push nothing — the receptor shows exactly what stock Marvelous shows
    /// (the white bomb, no burst).
    White,
}

impl ReceptorFlash {
    pub const DEFAULT: ReceptorFlash = ReceptorFlash::Purple;
    /// Overlay-row values (parallel to the row's labels).
    pub const ALL: [ReceptorFlash; 2] = [ReceptorFlash::Purple, ReceptorFlash::White];

    /// Config key (`s_marvelous.receptor_flash`).
    pub fn key(self) -> &'static str {
        match self {
            ReceptorFlash::Purple => "purple",
            ReceptorFlash::White => "white",
        }
    }
    /// Overlay-row label.
    pub fn label(self) -> &'static str {
        match self {
            ReceptorFlash::Purple => "PURPLE",
            ReceptorFlash::White => "WHITE",
        }
    }
    pub fn from_key(key: &str) -> Option<ReceptorFlash> {
        ReceptorFlash::ALL.into_iter().find(|c| c.key() == key)
    }
    /// Overlay-row value = index into `ALL`.
    pub fn index(self) -> i32 {
        ReceptorFlash::ALL
            .iter()
            .position(|c| *c == self)
            .map(|i| i as i32)
            .unwrap_or(0)
    }
    pub fn from_index(i: i32) -> Option<ReceptorFlash> {
        usize::try_from(i)
            .ok()
            .and_then(|i| ReceptorFlash::ALL.get(i).copied())
    }
    /// Whether this choice pushes the violet burst at all.
    pub fn pushes_burst(self) -> bool {
        matches!(self, ReceptorFlash::Purple)
    }
}

/// The S-Marvelous violet as a per-channel multiplier on the white burst.
///
/// The burst is drawn ADDITIVELY (alpha stays 0xFF while the colour fades
/// to black), so the same saturation rule the retired bomb tint learned on
/// the cabinet applies: keep the added light on the red/blue axis (green
/// near zero, blue full) or it washes to white on bright lanes. Not the
/// pastel art-language hue (`0xB05CE0`).
pub const VIOLET_RGB: u32 = 0xA030FF;

/// The pusher's `u8` lane bitset from the info struct's lane word (bits at
/// or above the panel count are dropped).
pub fn lane_bits(info_lanes: u32) -> u8 {
    (info_lanes & ((1u32 << MAX_LANES) - 1)) as u8
}

/// Whether a `JudgeEffectRenderer` COLOR4B is OUR burst quad (type ≥ 7 —
/// base colour `(f,f,f)`, never overridden) — greyscale and not fully
/// faded. Stock types 1..=6 are yellow / green / blue (some channel is `f/4`
/// or `3f/4` for every `f > 0`); only `(0,0,0)` is shared, and it maps to
/// itself anyway.
pub fn is_burst_white(rgb: [u8; 3]) -> bool {
    rgb[0] == rgb[1] && rgb[1] == rgb[2] && rgb[0] != 0
}

/// The fill-time recolour. `white` = the burst's `(c,c,c)` colour byte;
/// returns `c × violet` per channel (rounded). The alpha byte is the
/// caller's (the fill multiplies it by the appearance fade afterwards).
pub fn violet_for(white: u8, alpha: u8) -> [u8; 4] {
    let scale = |ch: u32| ((white as u32 * ch + 127) / 255) as u8;
    [
        scale((VIOLET_RGB >> 16) & 0xFF),
        scale((VIOLET_RGB >> 8) & 0xFF),
        scale(VIOLET_RGB & 0xFF),
        alpha,
    ]
}

/// Recolour decision on a COLOR4B `(R,G,B,A)`: `Some(violet)` for a white
/// burst quad, `None` = leave the game's colour alone.
pub fn recolor(rgba: [u8; 4]) -> Option<[u8; 4]> {
    if !is_burst_white([rgba[0], rgba[1], rgba[2]]) {
        return None;
    }
    Some(violet_for(rgba[0], rgba[3]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lane_bits_clips_to_eight_panels() {
        assert_eq!(lane_bits(0x05), 0x05);
        assert_eq!(lane_bits(0xFF), 0xFF);
        assert_eq!(lane_bits(0x1_05), 0x05); // bit 8 never maps to a panel
        assert_eq!(lane_bits(0xFFFF_FFFF), 0xFF);
        assert_eq!(lane_bits(0), 0);
    }

    #[test]
    fn white_discriminator() {
        assert!(is_burst_white([255, 255, 255])); // ours at f=255
        assert!(is_burst_white([120, 120, 120])); // ours, fading
        assert!(!is_burst_white([0, 0, 0])); // fully faded — leave alone
        assert!(!is_burst_white([200, 200, 50])); // type 1/4 yellow (f,f,f/4)
        assert!(!is_burst_white([50, 200, 50])); // type 2 green
        assert!(!is_burst_white([50, 200, 150])); // type 3 blue
        assert!(!is_burst_white([3, 3, 0])); // yellow at f=3 (f/4 == 0)
    }

    /// Every stock colour the game's routine can produce (types 1..=6 over
    /// the whole fade, incl. the flash-class doubling of 5/6) must be left
    /// alone; our type-7 colour `(f,f,f)` must be recoloured for every
    /// f > 0. Type 0 (the retired first cut, `(2f,2f,2f)` saturated) is
    /// kept in the sweep so the discriminator stays valid for either.
    #[test]
    fn discriminator_against_the_games_colour_table() {
        let sat2 = |v: u32| v.saturating_mul(2).min(255) as u8;
        for f in 0u32..=255 {
            let q = (f >> 2) as u8;
            let f8 = f as u8;
            let three_q = ((f * 3) >> 2) as u8;
            // type 1 / 4 (yellow), 2 (green), 3 (blue)
            let stock = [
                [f8, f8, q],
                [q, f8, q],
                [q, f8, three_q],
                // type 5 (bright yellow) and 6 (cyan): doubled + saturated
                [sat2(f), sat2(f), sat2(q as u32)],
                [sat2(q as u32), sat2(f), sat2(three_q as u32)],
            ];
            for c in stock {
                let is_black = c == [0, 0, 0];
                assert_eq!(
                    recolor([c[0], c[1], c[2], 0xFF]).is_some(),
                    false,
                    "stock colour {c:?} at f={f} must pass through (black={is_black})"
                );
            }
            // type >= 7 (ours): base (f, f, f), no override
            let ours = recolor([f8, f8, f8, 0xFF]);
            assert_eq!(ours.is_some(), f8 != 0, "type-7 white {f8} at f={f}");
            // type 0 (retired): (2f, 2f, 2f) saturated — still greyscale
            let w = sat2(f);
            assert_eq!(recolor([w, w, w, 0xFF]).is_some(), w != 0);
        }
    }

    #[test]
    fn violet_scales_with_the_fade_and_keeps_alpha() {
        let full = violet_for(255, 0xFF);
        assert_eq!(full, [0xA0, 0x30, 0xFF, 0xFF]);
        let half = violet_for(128, 0x80);
        assert_eq!(half[3], 0x80);
        assert!((half[0] as i32 - 0x50).abs() <= 1);
        assert!((half[1] as i32 - 0x18).abs() <= 1);
        assert!((half[2] as i32 - 0x80).abs() <= 1);
        // Additive-blend saturation contract: blue > red > green, green low.
        assert!(full[2] > full[0] && full[0] > full[1]);
        assert!(full[1] < 64);
        assert_eq!(violet_for(0, 0xFF), [0, 0, 0, 0xFF]);
        // Monotone in the fade (no rounding inversions).
        let mut prev = [0u8; 4];
        for w in 0..=255u8 {
            let v = violet_for(w, 0xFF);
            assert!(v[0] >= prev[0] && v[1] >= prev[1] && v[2] >= prev[2]);
            prev = v;
        }
    }

    #[test]
    fn recolor_passes_stock_colours_through() {
        assert_eq!(recolor([200, 200, 50, 255]), None);
        assert_eq!(
            recolor([255, 255, 255, 255]),
            Some([0xA0, 0x30, 0xFF, 0xFF])
        );
        assert_eq!(recolor([0, 0, 0, 255]), None);
    }

    #[test]
    fn receptor_flash_keys_indices_and_defaults() {
        assert_eq!(ReceptorFlash::DEFAULT, ReceptorFlash::Purple);
        assert!(ReceptorFlash::DEFAULT.pushes_burst());
        assert!(!ReceptorFlash::White.pushes_burst());
        for (i, m) in ReceptorFlash::ALL.iter().enumerate() {
            assert_eq!(m.index(), i as i32);
            assert_eq!(ReceptorFlash::from_index(i as i32), Some(*m));
            assert_eq!(ReceptorFlash::from_key(m.key()), Some(*m));
        }
        assert_eq!(
            ReceptorFlash::from_key("purple"),
            Some(ReceptorFlash::Purple)
        );
        assert_eq!(ReceptorFlash::from_key("white"), Some(ReceptorFlash::White));
        assert_eq!(ReceptorFlash::from_key("violet"), None);
        assert_eq!(ReceptorFlash::from_key(""), None);
        assert_eq!(ReceptorFlash::from_index(-1), None);
        assert_eq!(ReceptorFlash::from_index(2), None);
        // Row labels are the operator-facing words, upper-case like the
        // sibling rows'.
        assert_eq!(ReceptorFlash::Purple.label(), "PURPLE");
        assert_eq!(ReceptorFlash::White.label(), "WHITE");
    }
}
