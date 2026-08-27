use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use crate::log_info;

/// A colored placeholder box drawn on an indexed option's preview template
/// (`seop_image_<id>_TEMPLATE.png`). The template ships with these solid-color
/// marker rectangles; at enable time the chrome generator clears every marker
/// to transparent and injects the result as the base `seop_image_<id>` chrome,
/// and the preview overlay draws the focused value's live art at the marker's
/// rect (each [`PreviewLayer`] names the marker color its art fills).
/// `preview_gen` locates each box by scanning for its exact color.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MarkerColor {
    /// Pure green `#00FF00`.
    Green,
    /// Pure red `#FF0000`.
    Red,
}

/// One live-overlay recipe for a category: binds a template marker color to
/// the source-arc suffix whose image is drawn there. The arc filename is
/// `<file_prefix><id:04><arc_suffix>.arc`; an empty suffix selects the base
/// arc (e.g. APPEAL BOARD's `appeal_board_<id>.arc`). The preview overlay
/// loads the arc on demand and draws its inner PNG at the marker's rect,
/// over the chrome.
///
/// `gamma`, when `Some(g)`, routes the layer through a pre-brightened cached
/// arc (alpha untouched), using the **Photoshop convention**
/// (`out = in^(1/g)`): `g > 1.0` brightens, `< 1.0` darkens, `1.0` is
/// identity — so a value dialed in Photoshop transfers here directly. Used to
/// brighten the otherwise-dark lane art (`Some(6.0)`, overridable via
/// `custom_options.lane_gamma_correction`); `None` for layers that need no
/// correction.
pub struct PreviewLayer {
    pub color: MarkerColor,
    pub arc_suffix: &'static str,
    pub gamma: Option<f32>,
}

/// One filesystem-discovered cosmetic category. Every category renders as a
/// numeric scalar row: the value renders through the game's native digit
/// text path as a short label + the **1-based** position (e.g. `"Char #3"`
/// — see `ScalarFormat::PrefixedIndex`; the prefix comes from
/// [`CategoryDef::value_prefix`], the internal value stays the 0-based
/// index). No per-value ribbon or preview textures; the preview box shows
/// the single base `seop_image_<id>` chrome generated from the category's
/// `_TEMPLATE` (`preview_gen::generate_chrome`), and categories with a
/// non-empty [`CategoryDef::overlay_layers`] get the focused value's real
/// art drawn live over that chrome by `preview_overlay`.
///
/// (The one former non-scalar row, VIDEO SIZE's fixed enum, moved to the
/// standalone `movie_size_customization` mod.)
pub struct CategoryDef {
    pub option_id: &'static str,
    pub display_name: &'static str,
    pub scan_dir: Option<&'static str>,
    pub file_prefix: Option<&'static str>,
    pub customize_field_offset: u8,
    /// Display-only label prepended to the row's rendered value (e.g.
    /// `"Char #"` → `"Char #3"`). Kept SHORT on purpose: prefix + digits
    /// must stay inside the game string's 15-byte SSO inline buffer even
    /// for 5-digit positions (see `ScalarFormat::PrefixedIndex`).
    pub value_prefix: &'static str,
    /// The live-overlay recipe(s) for this category: which source-arc suffix
    /// (and optional gamma) supplies the real art `preview_overlay` draws over
    /// the chrome, at which template marker rect. Empty = chrome only, no live
    /// overlay — notably BACKGROUND ×2 (animated multi-sprite composites have
    /// no single static asset to preview, so they keep their generic explainer
    /// image).
    pub overlay_layers: &'static [PreviewLayer],
    /// True for the two BACKGROUND rows, which are driven by
    /// `bg_preview_overlay` (animated AFP-layer previews from the game's own
    /// runtime) rather than the static `preview_overlay`. Their
    /// `overlay_layers` stay empty (so the static overlay ignores them); this
    /// flag is how `bg_preview_overlay` finds the rows it owns and reads their
    /// `scan_dir` / `file_prefix` / discovered `asset_ids`.
    pub bg_overlay: bool,
}

impl CategoryDef {
    /// The overlay recipe(s) for this category (see the field docs). Kept as
    /// an accessor so overlay-side callers are insulated from the data shape.
    pub fn overlay_layers(&self) -> &'static [PreviewLayer] {
        self.overlay_layers
    }
}

pub static CATEGORIES: &[CategoryDef] = &[
    CategoryDef {
        option_id: "customize_appeal_board",
        value_prefix: "Board #",
        display_name: "APPEAL BOARD",
        scan_dir: Some("data/arc/custom/appeal_board"),
        file_prefix: Some("appeal_board_"),
        customize_field_offset: 0x0C,
        bg_overlay: false,
        // Two live overlays over the chrome: the template's green box shows
        // the `_result` art (appeal_board_<id>_result.arc), the red box the
        // base art (appeal_board_<id>.arc).
        overlay_layers: &[
            PreviewLayer {
                color: MarkerColor::Green,
                arc_suffix: "_result",
                gamma: None,
            },
            PreviewLayer {
                color: MarkerColor::Red,
                arc_suffix: "",
                gamma: None,
            },
        ],
    },
    CategoryDef {
        option_id: "customize_background",
        value_prefix: "Background #",
        display_name: "BACKGROUND",
        scan_dir: Some("data/arc/custom/background"),
        file_prefix: Some("background_"),
        customize_field_offset: 0x10,
        bg_overlay: true,
        // No live overlay: backgrounds are animated multi-sprite composites
        // with no single static asset to preview. The box shows the shipped
        // generic seop_image_customize_background.png for every value.
        overlay_layers: &[],
    },
    CategoryDef {
        option_id: "customize_background_gameplay",
        value_prefix: "Background #",
        display_name: "BACKGROUND (GAMEPLAY)",
        scan_dir: Some("data/arc/custom/background"),
        file_prefix: Some("background_"),
        customize_field_offset: 0x14,
        bg_overlay: true,
        overlay_layers: &[],
    },
    CategoryDef {
        option_id: "customize_character_p1",
        value_prefix: "Character #",
        display_name: "CHARACTER (P1)",
        scan_dir: Some("data/arc/custom/character"),
        file_prefix: Some("character_"),
        customize_field_offset: 0x18,
        bg_overlay: false,
        // Live overlay from character_<id>_result_1p.arc at the template's
        // green box.
        overlay_layers: &[PreviewLayer {
            color: MarkerColor::Green,
            arc_suffix: "_result_1p",
            gamma: None,
        }],
    },
    CategoryDef {
        option_id: "customize_character_p2",
        value_prefix: "Character #",
        display_name: "CHARACTER (P2)",
        scan_dir: Some("data/arc/custom/character"),
        file_prefix: Some("character_"),
        customize_field_offset: 0x1C,
        bg_overlay: false,
        // Mirrors P1 but with the P2 result art (character_<id>_result_2p.arc)
        // so each value shows the player-2 result pose.
        overlay_layers: &[PreviewLayer {
            color: MarkerColor::Green,
            arc_suffix: "_result_2p",
            gamma: None,
        }],
    },
    CategoryDef {
        option_id: "customize_lane_single",
        value_prefix: "Lane #",
        display_name: "LANE (SINGLE)",
        scan_dir: Some("data/arc/custom/lane_single"),
        file_prefix: Some("lane_single_"),
        customize_field_offset: 0x20,
        bg_overlay: false,
        // Live overlay from the single flat PNG per arc (lane_single_<id>.arc)
        // at the green box, routed through the pre-brightened lane cache
        // (Photoshop convention, >1 brightens) so the dark lane art reads.
        overlay_layers: &[PreviewLayer {
            color: MarkerColor::Green,
            arc_suffix: "",
            gamma: Some(6.0),
        }],
    },
    CategoryDef {
        option_id: "customize_lane_double",
        value_prefix: "Lane #",
        display_name: "LANE (DOUBLE)",
        scan_dir: Some("data/arc/custom/lane_double"),
        file_prefix: Some("lane_double_"),
        customize_field_offset: 0x24,
        bg_overlay: false,
        overlay_layers: &[PreviewLayer {
            color: MarkerColor::Green,
            arc_suffix: "",
            gamma: Some(6.0),
        }],
    },
    CategoryDef {
        option_id: "customize_lanecover_single",
        value_prefix: "Lane Cover #",
        display_name: "LANE COVER (SINGLE)",
        scan_dir: Some("data/arc/custom/lane_cover_single"),
        file_prefix: Some("lane_cover_single_"),
        customize_field_offset: 0x28,
        bg_overlay: false,
        // Live overlay from the single flat PNG per arc
        // (lane_cover_single_<id>.arc) at the green box.
        overlay_layers: &[PreviewLayer {
            color: MarkerColor::Green,
            arc_suffix: "",
            gamma: None,
        }],
    },
    CategoryDef {
        option_id: "customize_lanecover_double",
        value_prefix: "Lane Cover #",
        display_name: "LANE COVER (DOUBLE)",
        scan_dir: Some("data/arc/custom/lane_cover_double"),
        file_prefix: Some("lane_cover_double_"),
        customize_field_offset: 0x2C,
        bg_overlay: false,
        overlay_layers: &[PreviewLayer {
            color: MarkerColor::Green,
            arc_suffix: "",
            gamma: None,
        }],
    },
];

pub struct DiscoveredCategory {
    pub def: &'static CategoryDef,
    pub asset_ids: Vec<u32>,
}

pub fn discover_all() -> Vec<DiscoveredCategory> {
    CATEGORIES
        .iter()
        .filter_map(|def| {
            let ids = discover_from_filesystem(def);
            if ids.is_empty() {
                return None;
            }
            log_info!(
                "webui_options: discovered {} assets for {}",
                ids.len(),
                def.option_id
            );
            Some(DiscoveredCategory {
                def,
                asset_ids: ids,
            })
        })
        .collect()
}

fn discover_from_filesystem(def: &CategoryDef) -> Vec<u32> {
    let scan_dir = match def.scan_dir {
        Some(d) => d,
        None => return Vec::new(),
    };
    let prefix = match def.file_prefix {
        Some(p) => p,
        None => return Vec::new(),
    };

    let mut ids = BTreeSet::new();

    scan_directory(scan_dir, prefix, &mut ids);

    if let Ok(entries) = fs::read_dir("./data_mods") {
        for entry in entries.flatten() {
            let mod_path = entry.path().join(scan_dir);
            if mod_path.is_dir() {
                if let Some(p) = mod_path.to_str() {
                    scan_directory(p, prefix, &mut ids);
                }
            }
        }
    }

    ids.into_iter().collect()
}

fn scan_directory(dir: &str, prefix: &str, ids: &mut BTreeSet<u32>) {
    let path = Path::new(dir);
    let entries = match fs::read_dir(path) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let name = match entry.file_name().into_string() {
            Ok(n) => n,
            Err(_) => continue,
        };

        if let Some(id) = extract_id(&name, prefix) {
            ids.insert(id);
        }
    }
}

fn extract_id(filename: &str, prefix: &str) -> Option<u32> {
    let stem = filename.strip_suffix(".arc")?;
    if !stem.starts_with(prefix) {
        return None;
    }
    let after_prefix = &stem[prefix.len()..];
    let numeric_part: String = after_prefix
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if numeric_part.is_empty() {
        return None;
    }
    numeric_part.parse().ok()
}
