//! Label art for the enhanced VERSION layout.
//!
//! Cells use texture `sefi_version_<texture>_<N>col` (N = `num_columns`).
//! `select_music_option_v3.ifs` serves its textures per image name, so a new
//! name needs two things: a texturelist `<image>` entry declaring it (merged
//! texturelist, via the cached atlas-clone batch in fresh mode) and the
//! per-image blob LayeredFS serves for it (`_cache/<ifs>/md5(name)`, written
//! here with `ifs_textures::prebuild_texture`). Source PNGs live in
//! `data_mods/custom_series/series_labels/` — outside every `_ifs` folder, so
//! LayeredFS never auto-injects the unused widths.
//!
//! Resolution per texture: `sefi_version_<texture>_<N>col.png`, then
//! `sefi_version_<texture>.png`; a source that isn't exactly the width's
//! canvas × 20 is cropped/padded (top-left) into
//! `data_mods/_cache/custom_series_labels/`. Only the active width is
//! declared. The batch never latches the boot "reboot" warning: the options
//! IFS mounts at the CAUTION preload, after `enable()`, so a rebuild is live
//! the same boot.

use super::model::{canvas_width, label_donor, source_candidates, texture_name, EnhancedPlan};
use crate::services::avs_layeredfs::atlas_cloner::{
    generate_cloned_atlases_cached_with, load_stock_texturelist, write_merged_texturelist,
    AtlasSet, BatchOptions, BatchResult, OwnedTextureSpec,
};
use crate::services::avs_layeredfs::{ifs_textures, mod_paths};
use crate::{log_info, log_warn};
use std::path::{Path, PathBuf};

const MOD_ROOT: &str = "./data_mods/custom_series";
const SOURCE_DIR: &str = "./data_mods/custom_series/series_labels";
const NORMALISED_DIR: &str = "./data_mods/_cache/custom_series_labels";
const CACHE_ROOT: &str = "./data_mods/_cache";
const OPTION_ARC: &str = "data/arc/bm2d/select_music_option_v3.arc";
const OPTION_IFS: &str = "select_music_option_v3.ifs";
const OPTION_IFS_MOD_PATH: &str = "select_music_option_v3_ifs";
/// Distinct from legacy `cser_version` (atlas blob names must not collide).
const ATLAS_PREFIX: &str = "cser_enh";
const SIDECAR_FILE: &str = "custom_series_enhanced.atlasbatch.md5";
const CANVAS_H: u32 = 20;
/// What `write_merged_texturelist(.., "")` writes.
const EMPTY_MERGED: &str = "<texturelist>\n</texturelist>\n";

fn find_source(texture: &str, columns: u8) -> Option<PathBuf> {
    source_candidates(texture, columns)
        .into_iter()
        .map(|name| Path::new(SOURCE_DIR).join(name))
        .find(|p| p.is_file())
}

fn is_newer(a: &Path, b: &Path) -> bool {
    match (
        std::fs::metadata(a).and_then(|m| m.modified()),
        std::fs::metadata(b).and_then(|m| m.modified()),
    ) {
        (Ok(ta), Ok(tb)) => ta > tb,
        _ => true,
    }
}

/// Path of a `width`×20 version of `source`: the source itself when it
/// already has that size, else a cropped/padded copy (rewritten only when
/// the source is newer).
fn normalised(source: &Path, stem: &str, width: u32) -> Option<PathBuf> {
    let (w, h) = match image::image_dimensions(source) {
        Ok(d) => d,
        Err(e) => {
            log_warn!(
                "SeriesExpansion[enhanced]: can't read {}: {}",
                source.display(),
                e
            );
            return None;
        }
    };
    if (w, h) == (width, CANVAS_H) {
        return Some(source.to_path_buf());
    }
    log_warn!(
        "SeriesExpansion[enhanced]: {} is {}x{}, not {}x{} — cropped/padded",
        source.display(),
        w,
        h,
        width,
        CANVAS_H
    );
    let out = Path::new(NORMALISED_DIR).join(format!("{}.png", stem));
    if out.is_file() && !is_newer(source, &out) {
        return Some(out);
    }
    let img = match image::open(source) {
        Ok(i) => i.into_rgba8(),
        Err(e) => {
            log_warn!(
                "SeriesExpansion[enhanced]: can't load {}: {}",
                source.display(),
                e
            );
            return None;
        }
    };
    let mut canvas = image::RgbaImage::new(width, CANVAS_H);
    image::imageops::overlay(&mut canvas, &img, 0, 0);
    if std::fs::create_dir_all(NORMALISED_DIR).is_err() || canvas.save(&out).is_err() {
        log_warn!("SeriesExpansion[enhanced]: can't write {}", out.display());
        return None;
    }
    Some(out)
}

fn merged_path() -> PathBuf {
    Path::new(MOD_ROOT)
        .join(OPTION_IFS_MOD_PATH)
        .join("tex")
        .join("texturelist.merged.xml")
}

/// Whether the merged texturelist declares every stem (it is shared with the
/// legacy mode, which may have rewritten it on a previous boot).
fn merged_declares(stems: &[String]) -> bool {
    match std::fs::read_to_string(merged_path()) {
        Ok(xml) => stems
            .iter()
            .all(|s| xml.contains(&format!("name=\"{}\"", s))),
        Err(_) => false,
    }
}

/// Resolve, convert and declare the active width's labels. Call from
/// `enable()`, before the options IFS mounts.
pub fn prepare(plan: &EnhancedPlan) {
    let columns = plan.columns;
    let width = canvas_width(columns);
    let donor = label_donor(columns);

    let mut specs = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for cell in &plan.cells {
        if seen.contains(&cell.texture.as_str()) {
            continue;
        }
        seen.push(&cell.texture);
        let stem = texture_name(&cell.texture, columns);
        let Some(source) = find_source(&cell.texture, columns) else {
            log_warn!(
                "SeriesExpansion[enhanced]: no label art for '{}' in {} ({} or sefi_version_{}.png) — cell has no label",
                cell.texture,
                SOURCE_DIR,
                format_args!("{}.png", stem),
                cell.texture
            );
            continue;
        };
        let Some(png) = normalised(&source, &stem, width) else {
            continue;
        };
        let png = png.to_string_lossy().into_owned();
        if !ifs_textures::prebuild_texture(OPTION_IFS_MOD_PATH, &stem, &png, width, CANVAS_H) {
            log_warn!(
                "SeriesExpansion[enhanced]: could not convert {} — cell has no label",
                png
            );
            continue;
        }
        specs.push(OwnedTextureSpec {
            new_name: stem,
            donor_name: donor.to_string(),
            png_path: png,
        });
    }

    if specs.is_empty() {
        // Replace any previous declarations (legacy or another width).
        let already_empty = std::fs::read_to_string(merged_path())
            .map(|xml| xml == EMPTY_MERGED)
            .unwrap_or(false);
        if !already_empty && write_merged_texturelist(OPTION_IFS_MOD_PATH, MOD_ROOT, "") {
            mod_paths::init_mod_paths();
        }
        log_info!("SeriesExpansion[enhanced]: no label art declared");
        return;
    }

    let Some(texlist) = load_stock_texturelist(OPTION_ARC, OPTION_IFS) else {
        log_warn!(
            "SeriesExpansion[enhanced]: could not load the stock texturelist from {} — labels disabled",
            OPTION_ARC
        );
        return;
    };
    let stems: Vec<String> = specs.iter().map(|s| s.new_name.clone()).collect();
    let batch = [AtlasSet {
        atlas_prefix: ATLAS_PREFIX.to_string(),
        specs,
        fresh: true,
    }];
    let options = BatchOptions {
        sidecar_file: SIDECAR_FILE,
        latch_reboot: false,
    };
    let run = || {
        generate_cloned_atlases_cached_with(
            &texlist,
            OPTION_IFS_MOD_PATH,
            CACHE_ROOT,
            MOD_ROOT,
            &batch,
            &options,
        )
    };
    let mut result = run();
    if result == BatchResult::Cached && !merged_declares(&stems) {
        // The shared merged texturelist was rewritten since (legacy mode).
        let _ = std::fs::remove_file(
            Path::new(CACHE_ROOT)
                .join(OPTION_IFS_MOD_PATH)
                .join(SIDECAR_FILE),
        );
        result = run();
    }
    match result {
        BatchResult::Rebuilt => {
            mod_paths::init_mod_paths();
            log_info!(
                "SeriesExpansion[enhanced]: declared {} label(s) at {}x{} (rebuilt)",
                stems.len(),
                width,
                CANVAS_H
            );
        }
        BatchResult::Cached => log_info!(
            "SeriesExpansion[enhanced]: {} label(s) at {}x{} unchanged",
            stems.len(),
            width,
            CANVAS_H
        ),
        BatchResult::Nothing => {
            log_warn!("SeriesExpansion[enhanced]: label texturelist generation produced nothing")
        }
    }
}
