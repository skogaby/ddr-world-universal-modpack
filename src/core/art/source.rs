//! Art-source classification — which file is a jacket, banner or
//! background, and how an untagged one is bucketed (feasibility §10.1).
//! `std`-only so the index/tool can mount it without the image stack.

/// What a source image is, which decides how it becomes a square.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    Jacket,
    Banner,
    Background,
}

impl SourceKind {
    /// Compact tag for the index record (0 is reserved for "auto / none").
    #[must_use]
    pub const fn tag(self) -> u8 {
        match self {
            Self::Jacket => 1,
            Self::Banner => 2,
            Self::Background => 3,
        }
    }

    #[must_use]
    pub const fn from_tag(tag: u8) -> Option<Self> {
        match tag {
            1 => Some(Self::Jacket),
            2 => Some(Self::Banner),
            3 => Some(Self::Background),
            _ => None,
        }
    }
}

/// Image file extensions the loader considers art.
pub const IMAGE_EXTS: [&str; 5] = ["png", "jpg", "jpeg", "gif", "bmp"];

/// Whether a file name (any case) has an image extension.
#[must_use]
pub fn is_image_name(name: &str) -> bool {
    name.rsplit_once('.')
        .map(|(_, ext)| IMAGE_EXTS.iter().any(|e| e.eq_ignore_ascii_case(ext)))
        .unwrap_or(false)
}

/// StepMania's filename heuristics: `*jacket*` → jacket, `*bn*`/`*banner*`
/// → banner, `*bg*`/`*background*` → background. `None` for anything else
/// (including non-image names).
#[must_use]
pub fn classify_name(name: &str) -> Option<SourceKind> {
    if !is_image_name(name) {
        return None;
    }
    let stem = name
        .rsplit_once('.')
        .map(|(s, _)| s)
        .unwrap_or(name)
        .to_ascii_lowercase();
    if stem.contains("jacket") {
        Some(SourceKind::Jacket)
    } else if stem.contains("banner") || stem.contains("bn") {
        Some(SourceKind::Banner)
    } else if stem.contains("background") || stem.contains("bg") {
        Some(SourceKind::Background)
    } else {
        None
    }
}

/// Aspect-ratio classification for an untagged image: wider than 2:1 is a
/// banner, roughly square (within 15 %) a jacket, anything else a background.
#[must_use]
pub fn classify_aspect(width: u32, height: u32) -> SourceKind {
    if width == 0 || height == 0 {
        return SourceKind::Background;
    }
    let ratio = f64::from(width) / f64::from(height);
    if ratio >= 2.0 {
        SourceKind::Banner
    } else if (0.85..=1.15).contains(&ratio) {
        SourceKind::Jacket
    } else {
        SourceKind::Background
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_names_follow_stepmania_heuristics() {
        assert_eq!(classify_name("song-jacket.png"), Some(SourceKind::Jacket));
        assert_eq!(classify_name("Song BN.PNG"), Some(SourceKind::Banner));
        assert_eq!(classify_name("banner.gif"), Some(SourceKind::Banner));
        assert_eq!(classify_name("song-bg.jpg"), Some(SourceKind::Background));
        assert_eq!(classify_name("cover.png"), None);
        assert_eq!(classify_name("song.ogg"), None);
        assert!(is_image_name("a.JPEG"));
        assert!(!is_image_name("a.ssc"));
    }

    #[test]
    fn classify_aspect_buckets() {
        assert_eq!(classify_aspect(418, 164), SourceKind::Banner);
        assert_eq!(classify_aspect(512, 512), SourceKind::Jacket);
        assert_eq!(classify_aspect(640, 480), SourceKind::Background);
        assert_eq!(classify_aspect(0, 10), SourceKind::Background);
    }

    #[test]
    fn tags_round_trip() {
        for k in [
            SourceKind::Jacket,
            SourceKind::Banner,
            SourceKind::Background,
        ] {
            assert_eq!(SourceKind::from_tag(k.tag()), Some(k));
        }
        assert_eq!(SourceKind::from_tag(0), None);
    }
}
