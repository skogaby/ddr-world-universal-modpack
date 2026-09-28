//! Label art for the per-letter MUSIC TITLE menu.
//!
//! The new cells use `sefi_title_<a..z>_5col` (32×20) and
//! `sefi_title_other_2col` (104×20); the kana lines keep their stock art.
//! `select_music_option_v3.ifs` serves its textures per image name, so each
//! new name needs a texturelist `<image>` entry declaring it (a merged
//! texturelist in this mod's LayeredFS folder, via the cached fresh-mode
//! atlas-clone batch) and the per-image blob LayeredFS serves for it
//! (`_cache/<ifs>/md5(name)`, written with `ifs_textures::prebuild_texture`).
//!
//! Sources: `data_mods/improved_song_title_sorting/title_labels/`, generated
//! by `scripts/gen_title_labels.py` — outside every `_ifs` folder, so
//! LayeredFS never auto-injects them. A missing or mis-sized source is
//! skipped with one WARN (that cell renders without a label). The batch never
//! latches the boot "reboot" warning: the options IFS mounts at the CAUTION
//! preload, after `enable()`, so a rebuild is live the same boot.

use super::layout::{label_donor, new_textures};
use crate::services::avs_layeredfs::atlas_cloner::{
    generate_cloned_atlases_cached_with, load_stock_texturelist, AtlasSet, BatchOptions,
    BatchResult, OwnedTextureSpec,
};
use crate::services::avs_layeredfs::{ifs_textures, mod_paths};
use crate::{log_info, log_warn};
use std::path::{Path, PathBuf};

const MOD_ROOT: &str = "./data_mods/improved_song_title_sorting";
const SOURCE_DIR: &str = "./data_mods/improved_song_title_sorting/title_labels";
const CACHE_ROOT: &str = "./data_mods/_cache";
const OPTION_ARC: &str = "data/arc/bm2d/select_music_option_v3.arc";
const OPTION_IFS: &str = "select_music_option_v3.ifs";
const OPTION_IFS_MOD_PATH: &str = "select_music_option_v3_ifs";
/// Unique across every mod injecting into this IFS (atlas blob names are
/// `md5(<prefix>_NNN)`): custom_options `copt_mods`, series `cser_*`.
const ATLAS_PREFIX: &str = "tsort_lbl";
const SIDECAR_FILE: &str = "improved_song_title_sorting.atlasbatch.md5";
const CANVAS_H: u32 = 20;

fn merged_path() -> PathBuf {
    Path::new(MOD_ROOT)
        .join(OPTION_IFS_MOD_PATH)
        .join("tex")
        .join("texturelist.merged.xml")
}

fn merged_declares(stems: &[String]) -> bool {
    match std::fs::read_to_string(merged_path()) {
        Ok(xml) => stems
            .iter()
            .all(|s| xml.contains(&format!("name=\"{}\"", s))),
        Err(_) => false,
    }
}

/// Convert and declare every new label. Call from `enable()`, before the
/// options IFS mounts. Returns the number of labels declared.
pub fn prepare() -> usize {
    let mut specs = Vec::new();
    for (stem, width) in new_textures() {
        let source = Path::new(SOURCE_DIR).join(format!("{}.png", stem));
        match image::image_dimensions(&source) {
            Ok((w, h)) if (w, h) == (width, CANVAS_H) => {}
            Ok((w, h)) => {
                log_warn!(
                    "ImprovedTitleSorting: {} is {}x{}, not {}x{} — cell has no label (regenerate with scripts/gen_title_labels.py)",
                    source.display(),
                    w,
                    h,
                    width,
                    CANVAS_H
                );
                continue;
            }
            Err(e) => {
                log_warn!(
                    "ImprovedTitleSorting: no label art {}: {} — cell has no label",
                    source.display(),
                    e
                );
                continue;
            }
        }
        let png = source.to_string_lossy().into_owned();
        if !ifs_textures::prebuild_texture(OPTION_IFS_MOD_PATH, &stem, &png, width, CANVAS_H) {
            log_warn!(
                "ImprovedTitleSorting: could not convert {} — cell has no label",
                png
            );
            continue;
        }
        specs.push(OwnedTextureSpec {
            new_name: stem,
            donor_name: label_donor(width).to_string(),
            png_path: png,
        });
    }
    if specs.is_empty() {
        log_warn!("ImprovedTitleSorting: no label art declared — new cells render blank");
        return 0;
    }

    let Some(texlist) = load_stock_texturelist(OPTION_ARC, OPTION_IFS) else {
        log_warn!(
            "ImprovedTitleSorting: could not load the stock texturelist from {} — labels disabled",
            OPTION_ARC
        );
        return 0;
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
        // Someone edited or truncated the merged texturelist since.
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
                "ImprovedTitleSorting: declared {} label(s) (rebuilt)",
                stems.len()
            );
            stems.len()
        }
        BatchResult::Cached => {
            log_info!("ImprovedTitleSorting: {} label(s) unchanged", stems.len());
            stems.len()
        }
        BatchResult::Nothing => {
            log_warn!("ImprovedTitleSorting: label texturelist generation produced nothing");
            0
        }
    }
}
