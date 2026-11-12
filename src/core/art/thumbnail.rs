//! Wheel thumbnails — 192² BC1 (DXT1) DDS exactly as the stock
//! `data/jacket/thumbnail/<b>_tn.dds` members (feasibility §4.6: 18,560 bytes
//! = 128-byte header + 18,432 bytes of BC1 blocks).
//!
//! Header layout (verified against stock members and the Step 1 spike that
//! rendered on the cabinet): `"DDS "`, size 0x7C, flags `0x00081007`
//! (CAPS | HEIGHT | WIDTH | PIXELFORMAT | LINEARSIZE), height, width,
//! linear size, depth 0, mipmap count 1, 11 reserved dwords (any content —
//! stock carries an encoder tag), a 32-byte pixel format `{0x20, FOURCC,
//! "DXT1", 0×5}`, caps `0x1000` (TEXTURE), caps2..4 and reserved2 zero. The
//! BC1 blocks are standard D3D byte order — unlike the IFS DXT5 textures
//! `ifs_textures` emits, DDS members are NOT word-swapped.
//!
//! Pure: `image` + `texpresso` only; `scripts/validate_stepmania.sh` mounts
//! this file.

use image::imageops::{self, FilterType};
use image::RgbaImage;

/// Thumbnail edge length.
pub const THUMBNAIL_SIZE: u32 = 192;
/// DDS header length (magic + DDS_HEADER).
pub const DDS_HEADER_LEN: usize = 128;
/// BC1 payload for a 192² image: 48×48 blocks × 8 bytes.
pub const BC1_PAYLOAD_LEN: usize =
    (THUMBNAIL_SIZE as usize / 4) * (THUMBNAIL_SIZE as usize / 4) * 8;
/// Whole-file length of a stock thumbnail.
pub const THUMBNAIL_DDS_LEN: usize = DDS_HEADER_LEN + BC1_PAYLOAD_LEN;

/// `DDSD_CAPS | DDSD_HEIGHT | DDSD_WIDTH | DDSD_PIXELFORMAT | DDSD_LINEARSIZE`.
pub const DDS_FLAGS: u32 = 0x0008_1007;
/// `DDPF_FOURCC`.
pub const DDPF_FOURCC: u32 = 0x4;
/// `DDSCAPS_TEXTURE`.
pub const DDSCAPS_TEXTURE: u32 = 0x1000;
/// Reserved-field tag (stock members carry their encoder's name here; the
/// loader ignores the field).
const RESERVED_TAG: &[u8] = b"DDRWORLDHOOK";

/// Build the 128-byte DDS header for a BC1 image of `width`×`height` whose
/// payload is `linear_size` bytes.
#[must_use]
pub fn dds_header_bc1(width: u32, height: u32, linear_size: u32) -> [u8; DDS_HEADER_LEN] {
    let mut h = [0u8; DDS_HEADER_LEN];
    fn put(h: &mut [u8; DDS_HEADER_LEN], off: usize, v: u32) {
        h[off..off + 4].copy_from_slice(&v.to_le_bytes());
    }
    h[0..4].copy_from_slice(b"DDS ");
    put(&mut h, 4, 0x7C);
    put(&mut h, 8, DDS_FLAGS);
    put(&mut h, 12, height);
    put(&mut h, 16, width);
    put(&mut h, 20, linear_size);
    put(&mut h, 24, 0); // depth
    put(&mut h, 28, 1); // mipmap count
                        // reserved1[11] at 32..76
    let tag_len = RESERVED_TAG.len().min(44);
    h[32..32 + tag_len].copy_from_slice(&RESERVED_TAG[..tag_len]);
    // DDS_PIXELFORMAT at 76..108
    put(&mut h, 76, 0x20);
    put(&mut h, 80, DDPF_FOURCC);
    h[84..88].copy_from_slice(b"DXT1");
    // rgb bit count / masks (88..108) stay zero for FOURCC formats.
    put(&mut h, 108, DDSCAPS_TEXTURE);
    // caps2, caps3, caps4, reserved2 (112..128) zero.
    h
}

/// BC1-encode an RGBA image whose dimensions are multiples of 4.
#[must_use]
pub fn bc1_encode(img: &RgbaImage) -> Vec<u8> {
    let (w, h) = img.dimensions();
    let size = texpresso::Format::Bc1.compressed_size(w as usize, h as usize);
    let mut out = vec![0u8; size];
    texpresso::Format::Bc1.compress(
        img.as_raw(),
        w as usize,
        h as usize,
        texpresso::Params::default(),
        &mut out,
    );
    out
}

/// Resize to [`THUMBNAIL_SIZE`]² (Lanczos; alpha is forced opaque — BC1
/// carries no usable alpha and stock thumbnails are opaque) and encode as a
/// complete DDS file of exactly [`THUMBNAIL_DDS_LEN`] bytes.
#[must_use]
pub fn thumbnail_dds(img: &RgbaImage) -> Vec<u8> {
    let mut small = if img.dimensions() == (THUMBNAIL_SIZE, THUMBNAIL_SIZE) {
        img.clone()
    } else {
        imageops::resize(img, THUMBNAIL_SIZE, THUMBNAIL_SIZE, FilterType::Lanczos3)
    };
    for p in small.pixels_mut() {
        p[3] = 255;
    }
    let payload = bc1_encode(&small);
    let mut out = Vec::with_capacity(DDS_HEADER_LEN + payload.len());
    out.extend_from_slice(&dds_header_bc1(
        THUMBNAIL_SIZE,
        THUMBNAIL_SIZE,
        payload.len() as u32,
    ));
    out.extend_from_slice(&payload);
    out
}

/// Whether `bytes` has the shape of a stock thumbnail DDS (used to validate
/// cached blobs before packing them into the series arc).
#[must_use]
pub fn is_thumbnail_dds(bytes: &[u8]) -> bool {
    if bytes.len() != THUMBNAIL_DDS_LEN || &bytes[0..4] != b"DDS " {
        return false;
    }
    let u32_at = |off: usize| {
        u32::from_le_bytes([bytes[off], bytes[off + 1], bytes[off + 2], bytes[off + 3]])
    };
    u32_at(4) == 0x7C
        && u32_at(8) == DDS_FLAGS
        && u32_at(12) == THUMBNAIL_SIZE
        && u32_at(16) == THUMBNAIL_SIZE
        && u32_at(20) as usize == BC1_PAYLOAD_LEN
        && &bytes[84..88] == b"DXT1"
        && u32_at(108) == DDSCAPS_TEXTURE
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn header_matches_stock_shape() {
        let h = dds_header_bc1(192, 192, 18_432);
        assert_eq!(&h[0..4], b"DDS ");
        assert_eq!(u32::from_le_bytes(h[4..8].try_into().unwrap()), 0x7C);
        assert_eq!(
            u32::from_le_bytes(h[8..12].try_into().unwrap()),
            0x0008_1007
        );
        assert_eq!(u32::from_le_bytes(h[12..16].try_into().unwrap()), 192);
        assert_eq!(u32::from_le_bytes(h[16..20].try_into().unwrap()), 192);
        assert_eq!(u32::from_le_bytes(h[20..24].try_into().unwrap()), 18_432);
        assert_eq!(u32::from_le_bytes(h[28..32].try_into().unwrap()), 1);
        assert_eq!(u32::from_le_bytes(h[76..80].try_into().unwrap()), 0x20);
        assert_eq!(u32::from_le_bytes(h[80..84].try_into().unwrap()), 0x4);
        assert_eq!(&h[84..88], b"DXT1");
        assert_eq!(u32::from_le_bytes(h[108..112].try_into().unwrap()), 0x1000);
        assert!(h[112..].iter().all(|&b| b == 0));
    }

    #[test]
    fn thumbnail_is_exactly_stock_length_from_any_source_size() {
        let img = RgbaImage::from_pixel(512, 512, Rgba([255, 0, 0, 255]));
        let dds = thumbnail_dds(&img);
        assert_eq!(dds.len(), 18_560);
        assert!(is_thumbnail_dds(&dds));
        let odd = RgbaImage::from_pixel(37, 91, Rgba([0, 255, 0, 10]));
        let dds = thumbnail_dds(&odd);
        assert_eq!(dds.len(), THUMBNAIL_DDS_LEN);
        // The payload is standard-order BC1: decoding the first block through
        // texpresso gives the solid red back (a word-swapped block would not).
        let red = thumbnail_dds(&RgbaImage::from_pixel(192, 192, Rgba([255, 0, 0, 255])));
        let mut px = vec![0u8; 64];
        texpresso::Format::Bc1.decompress(&red[128..136], 4, 4, &mut px);
        assert_eq!(
            &px[..4],
            &[255, 0, 0, 255],
            "block 0 = {:02x?}",
            &red[128..136]
        );
    }

    #[test]
    fn validator_rejects_wrong_shapes() {
        assert!(!is_thumbnail_dds(&[]));
        let mut dds = thumbnail_dds(&RgbaImage::from_pixel(4, 4, Rgba([0, 0, 0, 255])));
        dds[84] = b'5';
        assert!(!is_thumbnail_dds(&dds));
    }
}
