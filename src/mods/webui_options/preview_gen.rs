//! Preview-template utilities for the WebUI Options preview system: chrome
//! generation, template marker scanning, gamma correction, and source-arc
//! lookup.
//!
//! Historically this module *composited* one preview texture per discovered
//! asset (~500–600 `seop_image_<id>_item_<NNN>.png` across categories) at DLL
//! init and injected them all via the LayeredFS atlas — which bloated the
//! song-select loading screen from 2–3 s to ~15 s. That per-asset compositing
//! is gone: the engine now renders only the per-category base chrome, and
//! `preview_overlay` draws the focused value's real art over it on demand.
//! What remains here are the pieces both the chrome and the overlay still
//! need:
//!
//! - [`generate_chrome`] — builds each language's base
//!   `seop_image_<option_id>.png` from that language's shipped
//!   `seop_image_<option_id>_TEMPLATE.png` by clearing its marker boxes to
//!   transparent (the engine renders this into the preview box for every
//!   value of an indexed option; templates carry baked per-language text, so
//!   the chrome is per-language — see
//!   `custom_options::asset_gen::OPTION_LANGS`).
//! - [`find_marker`] / [`marker_rect_for`] / [`MarkerRect`] — locate the
//!   solid-color marker rectangles on a template. The chrome generator clears
//!   them; `preview_overlay` reuses the same rects as overlay placement, so
//!   the on-demand art lands exactly where the old composited art did.
//!   Marker geometry is language-invariant, so lookups use one authoritative
//!   template (eng first, falling back across languages).
//! - [`apply_gamma`] — the Photoshop-convention gamma curve, used by
//!   `preview_overlay`'s pre-brightened lane arc cache.
//! - [`find_asset_arc`] — the game-root + `data_mods/` overlay search for a
//!   category's source `.arc`, shared by the lane cache.
//!
//! **Placement is driven entirely by the template.** A marker is a solid
//! rectangle of an exact color ([`MarkerColor`]) drawn on the template — so
//! adding/retargeting a preview region is just "draw a box of the right
//! color", no code change. A template can carry several boxes of different
//! colors fed by different source arcs (APPEAL BOARD: green ← the `_result`
//! art, red ← the base art).

use std::path::{Path, PathBuf};

use crate::services::custom_options::asset_gen::{OptionLang, OPTION_LANGS};
use crate::{log_info, log_warn};

use super::discovery::MarkerColor;

/// Search roots, in order, under which an asset category's `scan_dir` is
/// looked for. Mirrors `discovery`'s scan (game data dir first, then each
/// `data_mods/<mod>/` overlay). The first root that has the asset's `.arc`
/// wins.
const GAME_ROOT: &str = ".";
const DATA_MODS_ROOT: &str = "./data_mods";

/// The tex directory templates are read from and generated chrome PNGs are
/// written to, for one language (the same per-language dirs the `asset_gen`
/// atlas flush reads — chrome carries baked per-language template text, so
/// the output is per-language).
fn preview_dir(lang: &OptionLang) -> String {
    format!("./data_mods/custom_options/{}/tex", lang.ifs_mod_path)
}

/// The `seop_image_<option_id>_TEMPLATE.png` path for an option in one
/// language's tex dir. Shared by the chrome generator and the marker lookup.
fn template_path_in(lang: &OptionLang, option_id: &str) -> String {
    format!(
        "{}/seop_image_{}_TEMPLATE.png",
        preview_dir(lang),
        option_id
    )
}

/// The opaque RGBA pixel that marks a [`MarkerColor`]'s box on a template.
fn marker_rgba(color: MarkerColor) -> [u8; 4] {
    match color {
        MarkerColor::Green => [0, 255, 0, 255],
        MarkerColor::Red => [255, 0, 0, 255],
    }
}

fn marker_name(color: MarkerColor) -> &'static str {
    match color {
        MarkerColor::Green => "green",
        MarkerColor::Red => "red",
    }
}

/// Search the game root then each `data_mods/<mod>/` overlay for
/// `<root>/<scan_dir>/<arc_name>`. Returns the first existing path, silently —
/// used for probing name variants that mostly won't exist (the caller decides
/// what a total miss means). See [`find_asset_arc`] for the warning wrapper.
pub(super) fn find_asset_arc_opt(scan_dir: &str, arc_name: &str) -> Option<PathBuf> {
    let game = Path::new(GAME_ROOT).join(scan_dir).join(arc_name);
    if game.is_file() {
        return Some(game);
    }
    if let Ok(entries) = std::fs::read_dir(DATA_MODS_ROOT) {
        for entry in entries.flatten() {
            let candidate = entry.path().join(scan_dir).join(arc_name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Search the game root then each `data_mods/<mod>/` overlay for
/// `<root>/<scan_dir>/<arc_name>`. Returns the first existing path. Used by
/// `preview_overlay`'s lane brighten cache to read the same source arcs the
/// old compositor did.
pub(super) fn find_asset_arc(scan_dir: &str, arc_name: &str) -> Option<PathBuf> {
    let found = find_asset_arc_opt(scan_dir, arc_name);
    if found.is_none() {
        log_warn!(
            "webui_options/preview_gen: source arc {}/{} not found in any root — skipping",
            scan_dir,
            arc_name
        );
    }
    found
}

/// A target region detected on a template, in template pixel coordinates.
#[derive(Clone, Copy, Debug)]
pub struct MarkerRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// Load an option's template and return the marker rect (template pixels) for
/// `color`, or `None` if no language's template is readable or none has a
/// clean solid rectangle of that color. Used by `preview_overlay` to derive
/// each overlay layer's placement from the same template the chrome is
/// generated from — so the on-demand art lands exactly in the cleared marker
/// region.
///
/// Marker GEOMETRY is language-invariant by construction (the generator
/// emits byte-identical marker rects in every language's template), so one
/// authoritative template suffices: languages are tried in [`OPTION_LANGS`]
/// order (eng first) and the first clean hit wins, with a log line when the
/// hit wasn't eng (a missing eng template is worth noticing).
pub fn marker_rect_for(option_id: &str, color: MarkerColor) -> Option<MarkerRect> {
    for lang in &OPTION_LANGS {
        let template_path = template_path_in(lang, option_id);
        let template = match image::open(&template_path) {
            Ok(img) => img.into_rgba8(),
            Err(_) => continue,
        };
        if let Some(rect) = find_marker(&template, color) {
            if lang.ifs_code != OPTION_LANGS[0].ifs_code {
                log_info!(
                    "webui_options/preview_gen: marker rect for {} resolved from lang_{} template (eng template missing/markerless)",
                    option_id,
                    lang.ifs_code
                );
            }
            return Some(rect);
        }
    }
    log_warn!(
        "webui_options/preview_gen: no language's template yields a {} marker rect for {}",
        marker_name(color),
        option_id
    );
    None
}

/// Generate the base chrome image for an indexed option, **per language**:
/// each language's `seop_image_<option_id>_TEMPLATE.png` with **every**
/// marker box cleared to transparent, written as `seop_image_<option_id>.png`
/// in that language's tex dir so the `asset_gen` atlas flush injects it under
/// the base name for that language. This is what the preview box shows for
/// every value of the option (the slot-0 getter returns the base name for
/// keyless enum values); the preview overlay draws the focused value's live
/// art into the now-transparent marker regions on top.
///
/// All marker colors are cleared regardless of which ones carry overlay
/// layers — the raw green/red must never render. Skip-if-exists per
/// language: a base image already on disk is left alone, which both makes
/// this a boot-time no-op after the first run AND naturally skips categories
/// that ship authored base chrome (e.g.
/// `seop_image_customize_background.png`). Delete a generated PNG to force
/// that language's regeneration after changing its template. A language
/// whose template is missing/unreadable is skipped with one warn; the other
/// languages still generate. Returns whether a base image exists (shipped,
/// pre-existing, or freshly generated) for at least one language.
pub(super) fn generate_chrome(option_id: &str) -> bool {
    let mut any_ok = false;
    for lang in &OPTION_LANGS {
        let out_path = format!("{}/seop_image_{}.png", preview_dir(lang), option_id);
        if Path::new(&out_path).is_file() {
            any_ok = true; // shipped or previously generated
            continue;
        }

        let template_path = template_path_in(lang, option_id);
        let mut img = match image::open(&template_path) {
            Ok(i) => i.into_rgba8(),
            Err(e) => {
                log_warn!(
                    "webui_options/preview_gen: no lang_{} base chrome for {} and its template is missing/unreadable ({}) — that language's preview box will be blank",
                    lang.ifs_code,
                    option_id,
                    e
                );
                continue;
            }
        };

        for color in [MarkerColor::Green, MarkerColor::Red] {
            if let Some(rect) = find_marker(&img, color) {
                for yy in rect.y..(rect.y + rect.h).min(img.height()) {
                    for xx in rect.x..(rect.x + rect.w).min(img.width()) {
                        img.put_pixel(xx, yy, image::Rgba([0, 0, 0, 0]));
                    }
                }
            }
        }

        match img.save(&out_path) {
            Ok(()) => {
                log_info!(
                    "webui_options/preview_gen: generated lang_{} base chrome for {}",
                    lang.ifs_code,
                    option_id
                );
                any_ok = true;
            }
            Err(e) => {
                log_warn!("webui_options/preview_gen: can't write {}: {}", out_path, e);
            }
        }
    }
    any_ok
}

/// Apply a gamma curve to `img` in place, using the **Photoshop convention**
/// (`out = in^(1/gamma)` on the normalized 0–1 RGB channels). Alpha is left
/// untouched. `gamma > 1.0` brightens midtones, `< 1.0` darkens, `1.0` is
/// identity — this matches the gamma slider in Photoshop / image editors, so a
/// value authored there transfers here directly. A 256-entry lookup table is
/// built once per call (gamma is constant for the whole image) instead of a
/// `powf` per pixel/channel. Used by `preview_overlay`'s pre-brightened lane
/// arc cache (the on-demand path applies the same correction the old
/// compositor did).
pub(super) fn apply_gamma(img: &mut image::RgbaImage, gamma: f32) {
    // Photoshop convention: brighten when gamma > 1. Guard against a
    // non-positive gamma (would divide by zero / flip the curve) → identity.
    let exp = if gamma > 0.0 { 1.0 / gamma } else { 1.0 };
    let mut lut = [0u8; 256];
    for (i, e) in lut.iter_mut().enumerate() {
        let v = (i as f32 / 255.0).powf(exp);
        *e = (v * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
    }
    for px in img.pixels_mut() {
        px.0[0] = lut[px.0[0] as usize];
        px.0[1] = lut[px.0[1] as usize];
        px.0[2] = lut[px.0[2] as usize];
    }
}

/// Find the solid `color` (fully opaque) target rectangle on a template,
/// returning its bounding box in template pixels. The bounding box of all
/// matching pixels must be *completely filled* with that color (every pixel
/// inside it matching) to qualify — this rejects stray pixels or non-rect
/// shapes, which would otherwise produce a misplaced/oversized target. Returns
/// `None` if there are no matching pixels or they don't form a solid rectangle.
fn find_marker(template: &image::RgbaImage, color: MarkerColor) -> Option<MarkerRect> {
    let want = marker_rgba(color);
    let matches = |p: &image::Rgba<u8>| p.0 == want;

    let (mut x_min, mut y_min) = (u32::MAX, u32::MAX);
    let (mut x_max, mut y_max) = (0u32, 0u32);
    let mut count = 0u64;
    for (x, y, px) in template.enumerate_pixels() {
        if matches(px) {
            x_min = x_min.min(x);
            y_min = y_min.min(y);
            x_max = x_max.max(x);
            y_max = y_max.max(y);
            count += 1;
        }
    }
    if count == 0 {
        return None;
    }

    let w = x_max - x_min + 1;
    let h = y_max - y_min + 1;
    // Require the bounding box to be a solid rectangle of the color; otherwise
    // it's not a clean marker and we shouldn't trust the placement.
    if count != (w as u64) * (h as u64) {
        log_warn!(
            "webui_options/preview_gen: {} pixels don't form a solid rectangle \
             ({} px in {}x{} bbox) — ignoring marker",
            marker_name(color),
            count,
            w,
            h
        );
        return None;
    }

    Some(MarkerRect {
        x: x_min,
        y: y_min,
        w,
        h,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::custom_options::asset_gen::OPTION_LANGS;

    /// The per-language template/chrome directories must derive from the
    /// same `OPTION_LANGS` table `asset_gen`'s atlas flush reads from — one
    /// source of truth for the language set. See the localization design:
    /// .agents/planning/2026-08-17-options-texture-localization/design/detailed-design.md
    #[test]
    fn preview_paths_derive_from_option_langs() {
        for lang in &OPTION_LANGS {
            let code = lang.ifs_code;
            assert_eq!(
                preview_dir(lang),
                format!("./data_mods/custom_options/select_music_option_lang_{code}_v3_ifs/tex")
            );
            assert_eq!(
                template_path_in(lang, "customize_background"),
                format!(
                    "./data_mods/custom_options/select_music_option_lang_{code}_v3_ifs/tex/seop_image_customize_background_TEMPLATE.png"
                )
            );
        }
    }
}
