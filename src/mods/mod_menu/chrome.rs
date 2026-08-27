//! Pure modal-chrome synthesis for the mod-menu overlay: the rounded-corner
//! panel texture (theme gradient + configured opacity baked in) and the small
//! white rounded strip reused (stretched + tinted) for the tab indicator,
//! selection bar, scrollbar track/thumb, and header/banner backing — plus the
//! `overlay_menu.opacity` clamp+snap rule and the cache-key / texture-stem
//! material the impure loader feeds to the hash-sidecar cache.
//!
//! Deliberately **dependency-free except the `image` crate** (no `crate::`
//! imports) so its tests run on any host via the temp-crate harness
//! (`scripts/validate_mod_menu.sh`). The impure side (cache dir, background
//! encode thread, asset_loader, widgets) lives in `chrome_loader.rs`
//! (task-02). The pure layer never logs — callers turn
//! [`ChromeError::describe`] into their one WARN.
//!
//! Design: overlay-menu rewrite detailed design §4.5 (chrome & layout),
//! §6 (error ladder), §5 (config).

use image::RgbaImage;

// ── Geometry / versioning constants ─────────────────────────────────

/// Bumped on any geometry/appearance change so cached PNGs regenerate
/// (folded into [`cache_key_material`]).
pub const LAYOUT_VERSION: u32 = 1;

/// Panel texture width (the modal footprint, design §4.5).
pub const PANEL_W: u32 = 1160;
/// Panel texture height.
pub const PANEL_H: u32 = 600;
/// Panel corner radius in pixels.
pub const PANEL_CORNER_RADIUS: f32 = 20.0;

/// Strip texture width (stretched arbitrarily by the widgets).
pub const STRIP_W: u32 = 64;
/// Strip texture height.
pub const STRIP_H: u32 = 16;
/// Strip corner radius in pixels.
pub const STRIP_CORNER_RADIUS: f32 = 6.0;

// ── Opacity ──────────────────────────────────────────────────────────

/// Configured-opacity floor (percent).
pub const OPACITY_MIN: i32 = 25;
/// Configured-opacity ceiling (percent).
pub const OPACITY_MAX: i32 = 100;
/// Snap step for configured opacity (percent).
pub const OPACITY_STEP: i32 = 5;

/// Normalize a raw `overlay_menu.opacity` config value: clamp to
/// 25..=100, half-up snap to the nearest multiple of 5, re-clamp
/// (the `snap_rate_percent` formula).
#[must_use]
pub fn clamp_opacity(raw: i32) -> i32 {
    let clamped = raw.clamp(OPACITY_MIN, OPACITY_MAX);
    let snapped = (clamped + OPACITY_STEP / 2).div_euclid(OPACITY_STEP) * OPACITY_STEP;
    snapped.clamp(OPACITY_MIN, OPACITY_MAX)
}

/// Percent → texture alpha byte (`round(percent · 255 / 100)`). Public so
/// the renderer's solid-fallback rung tints the stretched strip with the
/// same mapping the panel bakes in.
#[must_use]
pub fn opacity_alpha(percent: i32) -> u8 {
    (((percent * 255) + 50) / 100).clamp(0, 255) as u8
}

// ── Gradient ─────────────────────────────────────────────────────────

/// Vertical two-stop panel gradient. RGB channels are used; the alpha
/// byte of each stop is IGNORED — panel alpha comes from the opacity
/// parameter alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PanelGradient {
    pub top: [u8; 4],
    pub bottom: [u8; 4],
}

/// Pre-theme panel gradient (dark blue-grey; the THEME step replaces
/// this with per-theme lookups — visual tuning knob for the maintainer).
pub const DEFAULT_GRADIENT: PanelGradient = PanelGradient {
    top: [22, 28, 46, 255],
    bottom: [8, 10, 18, 255],
};

// ── Errors ───────────────────────────────────────────────────────────

/// Synthesis/encode failure. The pure layer never logs — callers turn
/// [`describe`](ChromeError::describe) into their one WARN.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChromeError {
    /// PNG encoding failed.
    Png,
}

impl ChromeError {
    /// Static description for the caller's WARN line.
    #[must_use]
    pub fn describe(self) -> &'static str {
        match self {
            ChromeError::Png => "png encode failed",
        }
    }
}

// ── Synthesis ────────────────────────────────────────────────────────

/// Anti-aliased rounded-rect coverage for the pixel at `(x, y)` in a
/// `w`×`h` image with corner radius `r`: 1.0 in the interior, 0.0
/// outside, a smooth ~1 px band along the boundary (signed distance to
/// the rounded-rect edge, sampled at the pixel center).
fn rounded_rect_coverage(x: u32, y: u32, w: u32, h: u32, r: f32) -> f32 {
    let px = x as f32 + 0.5;
    let py = y as f32 + 0.5;
    let hw = w as f32 / 2.0;
    let hh = h as f32 / 2.0;
    let qx = (px - hw).abs() - (hw - r);
    let qy = (py - hh).abs() - (hh - r);
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    let inside = qx.max(qy).min(0.0);
    let d = outside + inside - r;
    (0.5 - d).clamp(0.0, 1.0)
}

/// Synthesize the modal panel: [`PANEL_W`]×[`PANEL_H`], rounded corners
/// ([`PANEL_CORNER_RADIUS`]), vertical `gradient.top → gradient.bottom`
/// RGB interpolation, alpha = `opacity_percent` (mapped to 0..=255)
/// scaled by the corner coverage.
#[must_use]
pub fn synthesize_panel(gradient: &PanelGradient, opacity_percent: i32) -> RgbaImage {
    let base_alpha = opacity_alpha(opacity_percent) as f32;
    let mut img = RgbaImage::new(PANEL_W, PANEL_H);
    for y in 0..PANEL_H {
        let t = y as f32 / (PANEL_H - 1) as f32;
        let mut rgb = [0u8; 3];
        for (c, out) in rgb.iter_mut().enumerate() {
            let top = gradient.top[c] as f32;
            let bottom = gradient.bottom[c] as f32;
            *out = (top + (bottom - top) * t).round() as u8;
        }
        for x in 0..PANEL_W {
            let coverage = rounded_rect_coverage(x, y, PANEL_W, PANEL_H, PANEL_CORNER_RADIUS);
            let a = (base_alpha * coverage).round() as u8;
            img.put_pixel(x, y, image::Rgba([rgb[0], rgb[1], rgb[2], a]));
        }
    }
    img
}

/// Synthesize the reusable solid strip: [`STRIP_W`]×[`STRIP_H`] opaque
/// white rounded rect ([`STRIP_CORNER_RADIUS`]), AA edges, transparent
/// outside — widgets stretch and tint it (ABGR incl. alpha).
#[must_use]
pub fn synthesize_strip() -> RgbaImage {
    let mut img = RgbaImage::new(STRIP_W, STRIP_H);
    for y in 0..STRIP_H {
        for x in 0..STRIP_W {
            let coverage = rounded_rect_coverage(x, y, STRIP_W, STRIP_H, STRIP_CORNER_RADIUS);
            let a = (255.0 * coverage).round() as u8;
            img.put_pixel(x, y, image::Rgba([255, 255, 255, a]));
        }
    }
    img
}

/// Encode an image as PNG bytes (the caller writes the cache file).
pub fn encode_png(img: &RgbaImage) -> Result<Vec<u8>, ChromeError> {
    let mut bytes = Vec::new();
    img.write_to(
        &mut std::io::Cursor::new(&mut bytes),
        image::ImageFormat::Png,
    )
    .map_err(|_| ChromeError::Png)?;
    Ok(bytes)
}

// ── Cache keys / texture stems ───────────────────────────────────────

/// Deterministic cache-key material folding theme id, opacity, and
/// [`LAYOUT_VERSION`] — the impure loader feeds it to
/// `CacheHasher::add_str`. Distinct inputs produce distinct strings.
#[must_use]
pub fn cache_key_material(theme_id: &str, opacity_percent: i32) -> String {
    format!("chrome:v{LAYOUT_VERSION}:theme={theme_id}:opacity={opacity_percent}")
}

/// Bare texture-name stem for the panel PNG. Stems differ per
/// (theme, opacity) — the engine caches textures by name hash, so a
/// re-synthesized panel must arrive under a fresh name to swap cleanly.
#[must_use]
pub fn panel_file_stem(theme_id: &str, opacity_percent: i32) -> String {
    format!("mm_panel_{theme_id}_{opacity_percent}")
}

/// Bare texture-name stem for the strip PNG (appearance-invariant).
#[must_use]
pub fn strip_file_stem() -> String {
    "mm_strip".to_string()
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const PNG_MAGIC: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

    fn alpha_at(img: &RgbaImage, x: u32, y: u32) -> u8 {
        img.get_pixel(x, y).0[3]
    }

    fn rgb_at(img: &RgbaImage, x: u32, y: u32) -> [u8; 3] {
        let p = img.get_pixel(x, y).0;
        [p[0], p[1], p[2]]
    }

    // ── Dimensions ────────────────────────────────────────────────

    #[test]
    fn panel_dimensions() {
        let img = synthesize_panel(&DEFAULT_GRADIENT, 80);
        assert_eq!(img.dimensions(), (PANEL_W, PANEL_H));
    }

    #[test]
    fn strip_dimensions() {
        let img = synthesize_strip();
        assert_eq!(img.dimensions(), (STRIP_W, STRIP_H));
    }

    // ── Corner-alpha profile ──────────────────────────────────────

    #[test]
    fn panel_corner_profile() {
        let img = synthesize_panel(&DEFAULT_GRADIENT, 80);
        let full = 204u8; // round(80 · 255 / 100)

        // Extreme corner pixel: fully transparent.
        assert_eq!(alpha_at(&img, 0, 0), 0, "corner pixel must be transparent");
        // Well outside the corner arc.
        assert_eq!(
            alpha_at(&img, 4, 4),
            0,
            "outside-arc pixel must be transparent"
        );
        // Interior center: exactly the mapped opacity.
        assert_eq!(alpha_at(&img, PANEL_W / 2, PANEL_H / 2), full);
        // Straight-edge midpoints are interior for a corner-rounded rect.
        assert_eq!(alpha_at(&img, PANEL_W / 2, 0), full, "top edge midpoint");
        assert_eq!(alpha_at(&img, 0, PANEL_H / 2), full, "left edge midpoint");
        // Just inside the corner arc: fully covered.
        assert_eq!(alpha_at(&img, 16, 16), full);

        // The AA band exists: some pixel in the corner box is strictly
        // between transparent and full.
        let r = PANEL_CORNER_RADIUS as u32;
        let intermediate = (0..r)
            .flat_map(|y| (0..r).map(move |x| (x, y)))
            .any(|(x, y)| {
                let a = alpha_at(&img, x, y);
                a > 0 && a < full
            });
        assert!(intermediate, "corner must have an anti-aliased band");

        // All four corners transparent.
        assert_eq!(alpha_at(&img, PANEL_W - 1, 0), 0);
        assert_eq!(alpha_at(&img, 0, PANEL_H - 1), 0);
        assert_eq!(alpha_at(&img, PANEL_W - 1, PANEL_H - 1), 0);
    }

    #[test]
    fn strip_corner_profile() {
        let img = synthesize_strip();
        assert_eq!(alpha_at(&img, 0, 0), 0, "strip corner must be transparent");
        assert_eq!(alpha_at(&img, STRIP_W / 2, STRIP_H / 2), 255);
        assert_eq!(
            rgb_at(&img, STRIP_W / 2, STRIP_H / 2),
            [255, 255, 255],
            "strip interior must be white for multiplicative tinting"
        );
        let r = STRIP_CORNER_RADIUS as u32;
        let intermediate = (0..r)
            .flat_map(|y| (0..r).map(move |x| (x, y)))
            .any(|(x, y)| {
                let a = alpha_at(&img, x, y);
                a > 0 && a < 255
            });
        assert!(intermediate, "strip corner must have an anti-aliased band");
    }

    // ── Opacity mapping / clamp+snap ─────────────────────────────

    #[test]
    fn opacity_mapping() {
        for (percent, expected) in [(100, 255u8), (80, 204), (50, 128), (25, 64)] {
            let img = synthesize_panel(&DEFAULT_GRADIENT, percent);
            assert_eq!(
                alpha_at(&img, PANEL_W / 2, PANEL_H / 2),
                expected,
                "opacity {percent}%"
            );
        }
    }

    #[test]
    fn clamp_table() {
        for (raw, expected) in [
            (0, 25),
            (-10, 25),
            (24, 25),
            (25, 25),
            (60, 60),
            (82, 80),
            (83, 85),
            (100, 100),
            (101, 100),
            (1000, 100),
        ] {
            assert_eq!(clamp_opacity(raw), expected, "clamp_opacity({raw})");
        }
    }

    // ── Gradient ─────────────────────────────────────────────────

    #[test]
    fn gradient_endpoints() {
        let g = PanelGradient {
            top: [10, 20, 200, 255],
            bottom: [200, 100, 10, 255],
        };
        let img = synthesize_panel(&g, 100);
        let x = PANEL_W / 2;
        assert_eq!(rgb_at(&img, x, 0), [10, 20, 200], "top row == top stop");
        assert_eq!(
            rgb_at(&img, x, PANEL_H - 1),
            [200, 100, 10],
            "bottom row == bottom stop"
        );
        let mid = rgb_at(&img, x, PANEL_H / 2);
        for c in 0..3 {
            let (lo, hi) = if g.top[c] < g.bottom[c] {
                (g.top[c], g.bottom[c])
            } else {
                (g.bottom[c], g.top[c])
            };
            assert!(
                mid[c] > lo && mid[c] < hi,
                "mid-row channel {c} must interpolate strictly between stops"
            );
        }
    }

    // ── Cache keys / stems ───────────────────────────────────────

    #[test]
    fn cache_key_stability() {
        let a = cache_key_material("default", 80);
        let b = cache_key_material("default", 80);
        assert_eq!(a, b, "identical inputs must produce identical keys");
        assert_ne!(
            a,
            cache_key_material("default", 85),
            "opacity change must change the key"
        );
        assert_ne!(
            a,
            cache_key_material("arrows", 80),
            "theme change must change the key"
        );
        assert!(
            a.contains(&LAYOUT_VERSION.to_string()),
            "key must reflect the layout version"
        );
    }

    #[test]
    fn file_stems_distinct() {
        let a = panel_file_stem("default", 80);
        assert_eq!(a, panel_file_stem("default", 80));
        assert_ne!(a, panel_file_stem("default", 85));
        assert_ne!(a, panel_file_stem("arrows", 80));
        assert_ne!(a, strip_file_stem());
        for stem in [a, strip_file_stem()] {
            assert!(
                stem.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
                "stem {stem:?} must be lowercase/underscore-safe (engine texture name)"
            );
        }
    }

    // ── PNG encode ───────────────────────────────────────────────

    #[test]
    fn png_magic() {
        let img = synthesize_strip();
        let bytes = encode_png(&img).expect("encode must succeed");
        assert!(bytes.len() > 8);
        assert_eq!(&bytes[..8], &PNG_MAGIC);
    }
}
