//! Jacket composition — one 512² RGBA square from whatever art a StepMania
//! song folder offers (design §4.6, feasibility §10.1).
//!
//! Source priority is the caller's (`compose_square` takes the candidates in
//! order and uses the first that decodes): `#JACKET` → `#BACKGROUND` →
//! `#BANNER` → filename heuristics ([`classify_name`]) → the pack-level
//! art → the shipped placeholder (owned by `super`). Backgrounds rank above
//! banners because a 4:3 background is far closer to the square DDR draws
//! than a 418×164 banner (maintainer, ITG1/ITG2 packs, 2026-10-23). Per kind:
//! - `Jacket`: centre-crop to a square (most are square already), resize;
//! - `Background` and `Banner`: the whole image is FITTED inside the square
//!   (aspect kept, centred) over a blurred, darkened, cover-scaled copy of
//!   itself — the "blurred backdrop" StepMania themes use — so nothing of the
//!   art is cropped away and the letterbox bands read as part of the image.
//!
//! Alpha is flattened onto black (jacket PNGs render over the stage panel
//! and results screen as opaque art). Animated GIFs decode to their first
//! frame via `image::load_from_memory`. Every function here returns
//! `Option`/values — a corrupt image is a `None`, never a panic, because the
//! runtime composes on a FileManager I/O thread inside an open hook.
//!
//! Pure: `image` only; `scripts/validate_stepmania.sh` mounts this file.

use image::imageops::{self, FilterType};
use image::{DynamicImage, GenericImageView, Rgba, RgbaImage};

pub use super::source::SourceKind;

/// Jacket edge length in pixels (stock `<b>_jk.png` are 512×512).
pub const JACKET_SIZE: u32 = 512;
/// Largest source image decoded (bytes); bigger files are treated as absent.
pub const MAX_SOURCE_BYTES: usize = 16 * 1024 * 1024;
/// Largest source dimension accepted (a 16k² decode would be 1 GiB).
pub const MAX_SOURCE_DIM: u32 = 8192;

/// One candidate image: its kind and raw file bytes.
pub struct ArtSource<'a> {
    pub kind: SourceKind,
    pub bytes: &'a [u8],
}

/// Decode `bytes` (first frame of an animation) into RGBA, rejecting
/// oversize inputs.
#[must_use]
pub fn decode(bytes: &[u8]) -> Option<RgbaImage> {
    if bytes.is_empty() || bytes.len() > MAX_SOURCE_BYTES {
        return None;
    }
    let img = image::load_from_memory(bytes).ok()?;
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 || w > MAX_SOURCE_DIM || h > MAX_SOURCE_DIM {
        return None;
    }
    Some(img.into_rgba8())
}

/// Compose the first decodable candidate into a [`JACKET_SIZE`] square.
/// `None` when no candidate decodes (the caller falls back to the
/// placeholder).
#[must_use]
pub fn compose_square(sources: &[ArtSource<'_>]) -> Option<RgbaImage> {
    sources
        .iter()
        .find_map(|s| decode(s.bytes).map(|img| square_from(s.kind, &img)))
}

/// Turn one decoded image into the jacket square according to its kind.
#[must_use]
pub fn square_from(kind: SourceKind, img: &RgbaImage) -> RgbaImage {
    let flat = flatten_alpha(img);
    match kind {
        SourceKind::Jacket => {
            let cropped = centre_crop_square(&flat);
            imageops::resize(&cropped, JACKET_SIZE, JACKET_SIZE, FilterType::Lanczos3)
        }
        SourceKind::Background | SourceKind::Banner => fit_on_blur(&flat),
    }
}

/// Composite onto opaque black.
#[must_use]
pub fn flatten_alpha(img: &RgbaImage) -> RgbaImage {
    let mut out = img.clone();
    for p in out.pixels_mut() {
        let a = u32::from(p[3]);
        if a != 255 {
            for c in 0..3 {
                p[c] = ((u32::from(p[c]) * a + 127) / 255) as u8;
            }
            p[3] = 255;
        }
    }
    out
}

/// The largest centred square of `img` (a view copy).
#[must_use]
pub fn centre_crop_square(img: &RgbaImage) -> RgbaImage {
    let (w, h) = img.dimensions();
    let side = w.min(h).max(1);
    let x = (w - side) / 2;
    let y = (h - side) / 2;
    imageops::crop_imm(img, x, y, side, side).to_image()
}

/// Scale `img` so it COVERS `side`×`side` (aspect kept), then centre-crop.
#[must_use]
pub fn cover_square(img: &RgbaImage, side: u32) -> RgbaImage {
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        return RgbaImage::from_pixel(side, side, Rgba([0, 0, 0, 255]));
    }
    let scale = f64::from(side) / f64::from(w.min(h));
    let nw = ((f64::from(w) * scale).ceil() as u32).max(side);
    let nh = ((f64::from(h) * scale).ceil() as u32).max(side);
    let scaled = imageops::resize(img, nw, nh, FilterType::Triangle);
    imageops::crop_imm(&scaled, (nw - side) / 2, (nh - side) / 2, side, side).to_image()
}

/// Non-square treatment: a blurred, darkened cover-scaled copy as the
/// backdrop, the image itself fitted inside the square (aspect kept) and
/// centred on top. A 4:3 background gets thin bands top and bottom; a
/// 418×164 banner a wide pair.
#[must_use]
pub fn fit_on_blur(img: &RgbaImage) -> RgbaImage {
    let side = JACKET_SIZE;
    // Blur a quarter-size backdrop and upscale it: the same visual result as
    // blurring at full size for a fraction of the cost (this runs inside an
    // open hook on the FileManager thread).
    let small = cover_square(img, side / 4);
    let blurred = imageops::fast_blur(&small, 6.0);
    let mut canvas = imageops::resize(&blurred, side, side, FilterType::Triangle);
    darken(&mut canvas, 0.65);

    let (w, h) = img.dimensions();
    if w > 0 && h > 0 {
        let (fit_w, fit_h) = if w >= h {
            let fh = ((u64::from(h) * u64::from(side)) / u64::from(w)).clamp(1, u64::from(side));
            (side, fh as u32)
        } else {
            let fw = ((u64::from(w) * u64::from(side)) / u64::from(h)).clamp(1, u64::from(side));
            (fw as u32, side)
        };
        let fitted = imageops::resize(img, fit_w, fit_h, FilterType::Lanczos3);
        let x = i64::from((side - fit_w) / 2);
        let y = i64::from((side - fit_h) / 2);
        imageops::overlay(&mut canvas, &fitted, x, y);
    }
    canvas
}

/// Kept for callers that spell out the banner case.
#[must_use]
pub fn banner_on_blur(banner: &RgbaImage) -> RgbaImage {
    fit_on_blur(banner)
}

fn darken(img: &mut RgbaImage, factor: f32) {
    let f = factor.clamp(0.0, 1.0);
    for p in img.pixels_mut() {
        for c in 0..3 {
            p[c] = (f32::from(p[c]) * f) as u8;
        }
    }
}

/// Encode as PNG (RGBA, the stock jacket pixel format). `None` only on an
/// encoder error, which `image` does not produce for a valid buffer.
#[must_use]
pub fn encode_png(img: &RgbaImage) -> Option<Vec<u8>> {
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

/// Convenience: a `DynamicImage` → the jacket square for `kind`.
#[must_use]
pub fn square_from_dynamic(kind: SourceKind, img: DynamicImage) -> RgbaImage {
    square_from(kind, &img.into_rgba8())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, rgba: [u8; 4]) -> Vec<u8> {
        let img = RgbaImage::from_pixel(w, h, Rgba(rgba));
        encode_png(&img).expect("png")
    }

    #[test]
    fn compose_uses_first_decodable_source_and_yields_512_square() {
        let bad = [0u8; 10];
        let good = solid(100, 60, [10, 200, 30, 255]);
        let out = compose_square(&[
            ArtSource {
                kind: SourceKind::Jacket,
                bytes: &bad,
            },
            ArtSource {
                kind: SourceKind::Background,
                bytes: &good,
            },
        ])
        .expect("second source decodes");
        assert_eq!(out.dimensions(), (JACKET_SIZE, JACKET_SIZE));
        assert_eq!(out.get_pixel(256, 256)[1], 200);
        assert!(compose_square(&[ArtSource {
            kind: SourceKind::Jacket,
            bytes: &bad,
        }])
        .is_none());
        assert!(compose_square(&[]).is_none());
    }

    #[test]
    fn banner_is_centred_over_a_darker_backdrop() {
        let banner = RgbaImage::from_pixel(400, 100, Rgba([255, 255, 255, 255]));
        let out = square_from(SourceKind::Banner, &banner);
        assert_eq!(out.dimensions(), (JACKET_SIZE, JACKET_SIZE));
        // Strip occupies the middle 128 rows at full width.
        assert_eq!(out.get_pixel(10, 256)[0], 255);
        // Backdrop above it is the darkened blur (< 255).
        assert!(out.get_pixel(10, 20)[0] < 255);
        assert_eq!(out.get_pixel(10, 20)[3], 255);
    }

    #[test]
    fn alpha_flattens_onto_black() {
        let img = RgbaImage::from_pixel(8, 8, Rgba([200, 100, 50, 128]));
        let flat = flatten_alpha(&img);
        let p = flat.get_pixel(0, 0);
        assert_eq!(p[3], 255);
        assert!((99..=101).contains(&p[0]));
    }

    #[test]
    fn oversize_and_empty_inputs_are_rejected() {
        assert!(decode(&[]).is_none());
        assert!(decode(&vec![0u8; MAX_SOURCE_BYTES + 1]).is_none());
    }
}
