//! Enable-time asset staging for the S-Marvelous gameplay flash: the
//! donor-anchored atlas clone of the word texture, the rewritten geo, the
//! AFP geo-mapping registration, and the deterministic pre-computation of
//! the character ids the `dance_judge` AP2 patch will allocate.
//!
//! Everything here is best-effort IO run once per enable (cache-guarded
//! where the shipped pipeline provides it): any failure logs one WARN with
//! the reason and returns `None`, leaving the patch unstaged — the game
//! then streams stock bytes (AC-3 fail-open).
//!
//! The §10 word-chain derivation (research display-side-re.md) drives every
//! name: nothing about shape/sprite ids or region names is hardcoded — the
//! chain is re-resolved from the stock template + geo each enable, and the
//! ids come from a dry run of the REAL patch recipe
//! ([`Ap2Doc::clone_word_segment_with_new_shape`]) on the stock bytes, so
//! the staged names always match what the patch fn will allocate at stream
//! time (allocation is `max_character_id()+1` at call time — deterministic
//! for fixed input bytes).

use crate::core::ap2::Ap2Doc;
use crate::core::{afp, arc, geo, ifs};
use crate::services::avs_layeredfs::atlas_cloner::{
    generate_cloned_atlases_cached, load_stock_texturelist, AtlasSet, BatchResult, OwnedTextureSpec,
};
use crate::services::avs_layeredfs::{ifs_textures, mod_paths};
use crate::{log_info, log_warn};

// ── Names (§10 derivation — see the module docs and plan.md D4) ─────

/// The arc carrying the LIVE gameplay judgement package. Cabinet-observed
/// (deploy #2, 2026-08-30): the game loads the UNSUFFIXED `dance_judge_v3`
/// arc — the `dance_judge0000_v0`-style arcs are skin/revision variants it
/// never opened. The v3 template is a parallel structure (word chain
/// PlaceObject(46) → sprite 46 → shape 41 → region `daju_marvelous`) that
/// the dynamic resolution absorbs unchanged.
pub const DANCE_JUDGE_ARC: &str = "data/arc/bm2d/dance_judge_v3.arc";
/// The IFS inside the arc (suffix-matched against the arc's entry paths).
pub const DANCE_JUDGE_IFS: &str = "dance_judge_v3.ifs";
/// Normalized IFS path key the game's opens resolve to (arc-contained IFS
/// paths normalize to the BARE `<name>.ifs/...` form — the
/// folder_expansion precedent), used for the geo MD5 mapping registration.
pub const DANCE_JUDGE_IFS_PATH: &str = "dance_judge_v3.ifs";
/// Mod-folder-relative IFS directory (`.ifs → _ifs`, the ifs_textures rule).
pub const IFS_MOD_PATH: &str = "dance_judge_v3_ifs";
/// The mod's data root (a LayeredFS mod folder).
pub const MOD_ROOT: &str = "./data_mods/s_marvelous";
/// Shared LayeredFS cache root.
pub const CACHE_ROOT: &str = "./data_mods/_cache";
/// The template's exported name — the afp_patcher patch key.
pub const TEMPLATE_NAME: &str = "dance_judge";
/// Stock labeled segment / the new segment (§10 label map).
pub const SRC_LABEL: &str = "in_marvelous";
pub const NEW_LABEL: &str = "in_smarvelous";
/// The donor region is detected by this suffix on the word geo's label
/// (`daju_marvelous` on the live v3 package); the new region substitutes
/// `smarvelous` for the suffix word (`daju_smarvelous`).
const REGION_SUFFIX_OLD: &str = "marvelous";
const REGION_SUFFIX_NEW: &str = "smarvelous";
/// The maintainer-supplied word art (260×90 — the v3 donor uvrect exactly).
pub const WORD_PNG: &str = "./data_mods/s_marvelous/dance_judge/smarvelous.png";
/// Cloned-atlas name prefix (short + unique per atlas_cloner docs).
const ATLAS_PREFIX: &str = "smarv_dj";

/// Everything the patch fn needs, staged at enable.
pub struct StagedPatch {
    /// The stock template, descrambled — the byte-exact input the patch fn
    /// expects at the afp_patcher seam (the v1 skin gate compares against
    /// this).
    pub stock_bytes: Vec<u8>,
    /// The word shape resolved from the §10 chain (geo label ends
    /// `_marvelous`).
    pub word_shape_id: u16,
    /// Ids the patch WILL allocate (dry-run of the real recipe).
    pub new_shape_id: u16,
    pub new_sprite_id: u16,
    /// The injected texture region (`dance_judge0000_smarvelous` on 0000).
    pub new_region: String,
}

/// BSI byteswap + string-table cipher removal — produces the same "fully
/// descrambled" shape the game's afp_patcher seam sees (docs/afp_system.md
/// §1; identical to the validate harness's descramble).
fn descramble(mut afp: Vec<u8>, bsi: &[u8]) -> Option<Vec<u8>> {
    afp::apply_bsi(&mut afp, bsi);
    let st_off = u32::from_le_bytes(afp.get(48..52)?.try_into().ok()?) as usize;
    let st_size = u32::from_le_bytes(afp.get(52..56)?.try_into().ok()?) as usize;
    let table = afp.get(st_off..st_off.checked_add(st_size)?)?.to_vec();
    let plain = crate::core::ap2::decode_string_table(&table);
    afp[st_off..st_off + st_size].copy_from_slice(&plain);
    Some(afp)
}

/// The impure half of the §10 word-chain resolution: walk the `in_marvelous`
/// segment's placements for a sprite whose nested section places a shape
/// whose GEO (extracted from the IFS) carries a `*_marvelous` region label.
struct WordChain {
    word_shape_id: u16,
    donor_geo: Vec<u8>,
    donor_region: String,
}

fn resolve_word_chain(doc: &Ap2Doc, ifs_data: &[u8]) -> Option<WordChain> {
    // Geo-first resolution via the SHARED core/ap2 resolver (deploy #3 fix
    // — the previous sprite-walk resolver diverged from the harness and
    // broke on the live v3 template's 3-deep nesting). The closure feeds
    // geos out of the IFS and remembers the bytes of whichever geo wins so
    // the clone step can rewrite them.
    let donor_geo = std::cell::RefCell::new(None::<(String, Vec<u8>)>);
    let (word_shape_id, donor_region) =
        doc.find_word_shape_by_geo(SRC_LABEL, REGION_SUFFIX_OLD, |geo_name| {
            let extracted =
                ifs::extract_files(ifs_data, "geo", std::slice::from_ref(&geo_name.to_string()));
            let (_, geo_bytes) = extracted.into_iter().next()?;
            let labels = geo::labels(&geo_bytes)?;
            donor_geo.replace(Some((geo_name.to_string(), geo_bytes)));
            Some(labels)
        })?;
    // The winning geo is the LAST successful lookup only when unambiguous —
    // re-extract by the resolved id to be exact.
    let geo_name = format!("{}_shape{}", doc.exported_name(), word_shape_id);
    let donor_geo = match donor_geo.into_inner() {
        Some((name, bytes)) if name == geo_name => bytes,
        _ => {
            let extracted = ifs::extract_files(ifs_data, "geo", std::slice::from_ref(&geo_name));
            extracted.into_iter().next()?.1
        }
    };
    Some(WordChain {
        word_shape_id,
        donor_geo,
        donor_region,
    })
}

/// Stage the full dance_judge asset chain. Any failure WARNs with the
/// reason and returns `None` (stock behavior — AC-3).
pub fn stage() -> Option<StagedPatch> {
    if !std::path::Path::new(WORD_PNG).exists() {
        log_warn!(
            "SMarvelous: word art missing at {} — dance_judge patch not staged",
            WORD_PNG
        );
        return None;
    }

    // ── Extract + descramble the stock template ─────────────────────
    let arc_data = match std::fs::read(DANCE_JUDGE_ARC) {
        Ok(d) => d,
        Err(e) => {
            log_warn!("SMarvelous: can't read {}: {}", DANCE_JUDGE_ARC, e);
            return None;
        }
    };
    let Some(entries) = arc::parse(&arc_data) else {
        log_warn!("SMarvelous: failed to parse {}", DANCE_JUDGE_ARC);
        return None;
    };
    let ifs_entry = match entries.iter().find(|e| e.path.ends_with(DANCE_JUDGE_IFS)) {
        Some(e) => e,
        None => {
            log_warn!(
                "SMarvelous: {} not found in {}",
                DANCE_JUDGE_IFS,
                DANCE_JUDGE_ARC
            );
            return None;
        }
    };
    let Some(ifs_data) = arc::extract(&arc_data, ifs_entry) else {
        log_warn!("SMarvelous: failed to extract {}", DANCE_JUDGE_IFS);
        return None;
    };

    let tpl_name = TEMPLATE_NAME.to_string();
    let afp_files = ifs::extract_files(&ifs_data, "afp", std::slice::from_ref(&tpl_name));
    let bsi_files = ifs::extract_files(&ifs_data, "afp/bsi", std::slice::from_ref(&tpl_name));
    let (Some((_, afp_raw)), Some((_, bsi_raw))) =
        (afp_files.into_iter().next(), bsi_files.into_iter().next())
    else {
        log_warn!("SMarvelous: dance_judge AFP/BSI not found in the IFS");
        return None;
    };
    let Some(stock_bytes) = descramble(afp_raw, &bsi_raw) else {
        log_warn!("SMarvelous: dance_judge descramble failed");
        return None;
    };
    let Some(doc) = Ap2Doc::parse(&stock_bytes) else {
        log_warn!("SMarvelous: stock dance_judge did not parse — patch not staged");
        return None;
    };
    if doc.exported_name() != TEMPLATE_NAME {
        log_warn!(
            "SMarvelous: unexpected exported name '{}' — patch not staged",
            doc.exported_name()
        );
        return None;
    }

    // ── Resolve the word chain (§10) ────────────────────────────────
    let Some(chain) = resolve_word_chain(&doc, &ifs_data) else {
        log_warn!(
            "SMarvelous: dance_judge word chain unresolved (unknown structure) — patch not staged"
        );
        return None;
    };
    let Some(stem) = chain.donor_region.strip_suffix(REGION_SUFFIX_OLD) else {
        log_warn!(
            "SMarvelous: donor region '{}' has no '{}' suffix — patch not staged",
            chain.donor_region,
            REGION_SUFFIX_OLD
        );
        return None;
    };
    let new_region = format!("{}{}", stem, REGION_SUFFIX_NEW);

    // ── Dry-run the REAL recipe to learn the ids the patch allocates ─
    let mut scratch = doc.clone();
    let Some(ids) =
        scratch.clone_word_segment_with_new_shape(SRC_LABEL, NEW_LABEL, chain.word_shape_id)
    else {
        log_warn!("SMarvelous: dance_judge patch dry-run failed — patch not staged");
        return None;
    };
    if scratch.serialize().is_none() {
        log_warn!("SMarvelous: patched dance_judge does not serialize — patch not staged");
        return None;
    }

    // ── Rewritten geo: donor bytes, region label re-aimed ───────────
    let Some(new_geo) = geo::rewrite_labels(&chain.donor_geo, |l| {
        if l == chain.donor_region {
            Some(new_region.clone())
        } else {
            None
        }
    }) else {
        log_warn!("SMarvelous: geo label rewrite failed — patch not staged");
        return None;
    };
    let geo_name = format!("{}_shape{}", doc.exported_name(), ids.new_shape_id);
    let geo_dir = format!("{}/{}/geo", MOD_ROOT, IFS_MOD_PATH);
    if let Err(e) = std::fs::create_dir_all(&geo_dir) {
        log_warn!("SMarvelous: mkdir {}: {} — patch not staged", geo_dir, e);
        return None;
    }
    let geo_path = format!("{}/{}", geo_dir, geo_name);
    if let Err(e) = std::fs::write(&geo_path, &new_geo) {
        log_warn!(
            "SMarvelous: can't write {}: {} — patch not staged",
            geo_path,
            e
        );
        return None;
    }

    // ── Donor-anchored atlas clone (cache-guarded) ──────────────────
    let Some(texlist) = load_stock_texturelist(DANCE_JUDGE_ARC, DANCE_JUDGE_IFS) else {
        log_warn!("SMarvelous: stock dance_judge texturelist unavailable — patch not staged");
        return None;
    };
    let batch = [AtlasSet {
        atlas_prefix: ATLAS_PREFIX.to_string(),
        specs: vec![OwnedTextureSpec {
            new_name: new_region.clone(),
            donor_name: chain.donor_region.clone(),
            png_path: WORD_PNG.to_string(),
        }],
        fresh: false, // donor-anchored: cloned geo UVs must stay valid
    }];
    match generate_cloned_atlases_cached(&texlist, IFS_MOD_PATH, CACHE_ROOT, MOD_ROOT, &batch) {
        BatchResult::Nothing => {
            log_warn!("SMarvelous: word atlas injection produced nothing — patch not staged");
            return None;
        }
        BatchResult::Cached | BatchResult::Rebuilt => {}
    }

    // ── Serve the new geo by MD5 name ───────────────────────────────
    ifs_textures::register_afp_geo_mapping(DANCE_JUDGE_IFS_PATH, &geo_name);

    // The AFP runtime loads geos strictly from the afplist `<geo>` id list
    // at IFS mount (deploy #4: `afp-mip: can not find geo id` — no
    // on-demand fallback). Extend the existing dance_judge entry so the
    // stream registers our new shape.
    ifs_textures::register_afplist_geo_extension(
        DANCE_JUDGE_IFS_PATH,
        TEMPLATE_NAME,
        &[ids.new_shape_id],
    );

    // Per-image texture data: this IFS family stores texture data one file
    // per IMAGE (deploy #4: the game opened tex/md5("daju_smarvelous") and
    // never the cloned atlas blob). handle_texture serves it from a PNG at
    // `{ifs_mod_path}/tex/{image_name}.png` (folder_expansion's shipped
    // pattern), converting + padding to the imgrect dims from the merged
    // texturelist. Stage a copy of the word art under the image name.
    let tex_dir = format!("{}/{}/tex", MOD_ROOT, IFS_MOD_PATH);
    if let Err(e) = std::fs::create_dir_all(&tex_dir) {
        log_warn!("SMarvelous: mkdir {}: {} — patch not staged", tex_dir, e);
        return None;
    }
    let image_png = format!("{}/{}.png", tex_dir, new_region);
    if let Err(e) = std::fs::copy(WORD_PNG, &image_png) {
        log_warn!(
            "SMarvelous: can't stage {}: {} — patch not staged",
            image_png,
            e
        );
        return None;
    }

    // The mod-paths file cache was scanned at layeredfs init — BEFORE this
    // enable ran. If the merged texturelist / geo weren't on disk at scan
    // time (first boot after deploy), rescan once so this boot sees them
    // (music_wheel_song_length precedent).
    let merged_rel = format!("{}/tex/texturelist.merged.xml", IFS_MOD_PATH);
    let geo_rel = format!("{}/geo/{}", IFS_MOD_PATH, geo_name);
    let tex_rel = format!("{}/tex/{}.png", IFS_MOD_PATH, new_region);
    if mod_paths::find_first_modfile(&merged_rel).is_none()
        || mod_paths::find_first_modfile(&geo_rel).is_none()
        || mod_paths::find_first_modfile(&tex_rel).is_none()
    {
        log_info!("SMarvelous: staged assets not in mod-path cache — rescanning");
        mod_paths::init_mod_paths();
    }

    log_info!(
        "SMarvelous: dance_judge patch staged (word sprite {}, shape {} -> new shape {} / sprite {}, geo '{}', region '{}')",
        ids.word_sprite_id,
        chain.word_shape_id,
        ids.new_shape_id,
        ids.new_sprite_id,
        geo_name,
        new_region
    );

    Some(StagedPatch {
        stock_bytes,
        word_shape_id: chain.word_shape_id,
        new_shape_id: ids.new_shape_id,
        new_sprite_id: ids.new_sprite_id,
        new_region,
    })
}

// ── Combo digits (Step 5) ────────────────────────────────────────────

/// The arc/IFS carrying the live combo package (same unsuffixed-v3 rule as
/// dance_judge — deploy #2 finding; the `dance_combo0000_v0`-style arcs are
/// skin variants the game never opens by default).
pub const DANCE_COMBO_ARC: &str = "data/arc/bm2d/dance_combo_v3.arc";
pub const DANCE_COMBO_IFS: &str = "dance_combo_v3.ifs";
pub const COMBO_IFS_MOD_PATH: &str = "dance_combo_v3_ifs";
/// Digit art source (maintainer-supplied, 100×118 — stock digits are
/// 104×120 imgrect / 102×118 uvrect; FRESH-mode rects are emitted from the
/// PNG dims so the entries stay self-consistent).
const COMBO_PNG_DIR: &str = "./data_mods/s_marvelous/dance_combo";
/// Injected texture-name prefix — the digit-refresh walk loads
/// `daco_combo_smarvelous_%d` (design §4.5), parallel to the stock
/// `daco_combo_marvelous_%d` set.
const COMBO_REGION_PREFIX: &str = "daco_combo_smarvelous";
/// Stock donor image (encoding/compression reference for the FRESH set).
const COMBO_DONOR: &str = "daco_combo_marvelous_0";
const COMBO_ATLAS_PREFIX: &str = "smarv_dc";

/// Stage the S-Marvelous combo digit textures: FRESH-mode merged
/// texturelist entries + per-image PNGs under the mod IFS folder (this
/// package family serves texture data ONE FILE PER IMAGE — the game opens
/// `tex/md5(image_name)`; deploy #4/#5 lessons). No geo, no afplist, no
/// AP2 patch: `afp_mc_load_bitmap` binds by texturelist image name alone.
/// Fail-open: any failure ⇒ `false` (combo stays stock, one WARN upstream).
pub fn stage_combo_digits() -> bool {
    // Per-image PNGs at the serving path.
    let tex_dir = format!("{}/{}/tex", MOD_ROOT, COMBO_IFS_MOD_PATH);
    if let Err(e) = std::fs::create_dir_all(&tex_dir) {
        log_warn!(
            "SMarvelous: mkdir {}: {} — combo digits unstaged",
            tex_dir,
            e
        );
        return false;
    }
    let mut specs = Vec::with_capacity(10);
    for d in 0..10u32 {
        let src = format!("{}/smarvelous_{}.png", COMBO_PNG_DIR, d);
        let new_name = format!("{}_{}", COMBO_REGION_PREFIX, d);
        let dst = format!("{}/{}.png", tex_dir, new_name);
        if let Err(e) = std::fs::copy(&src, &dst) {
            log_warn!(
                "SMarvelous: can't stage {}: {} — combo digits unstaged",
                dst,
                e
            );
            return false;
        }
        specs.push(OwnedTextureSpec {
            new_name,
            donor_name: COMBO_DONOR.to_string(),
            png_path: src,
        });
    }

    // FRESH-mode merged texturelist (cache-guarded like the judge batch).
    let Some(texlist) = load_stock_texturelist(DANCE_COMBO_ARC, DANCE_COMBO_IFS) else {
        log_warn!("SMarvelous: stock dance_combo texturelist unavailable — combo digits unstaged");
        return false;
    };
    let batch = [AtlasSet {
        atlas_prefix: COMBO_ATLAS_PREFIX.to_string(),
        specs,
        fresh: true, // net-new digit set — no donor slots to preserve
    }];
    match generate_cloned_atlases_cached(&texlist, COMBO_IFS_MOD_PATH, CACHE_ROOT, MOD_ROOT, &batch)
    {
        BatchResult::Nothing => {
            log_warn!("SMarvelous: combo digit atlas injection produced nothing — unstaged");
            false
        }
        BatchResult::Cached | BatchResult::Rebuilt => {
            // First-boot visibility: rescan the mod-path cache if the new
            // files postdate the init-time scan (same rule as the judge
            // staging).
            let merged_rel = format!("{}/tex/texturelist.merged.xml", COMBO_IFS_MOD_PATH);
            let probe_rel = format!("{}/tex/{}_0.png", COMBO_IFS_MOD_PATH, COMBO_REGION_PREFIX);
            if mod_paths::find_first_modfile(&merged_rel).is_none()
                || mod_paths::find_first_modfile(&probe_rel).is_none()
            {
                log_info!("SMarvelous: combo assets not in mod-path cache — rescanning");
                mod_paths::init_mod_paths();
            }
            log_info!("SMarvelous: combo digit textures staged (10 images, fresh atlas)");
            true
        }
    }
}

// ── Results score tab (Step 7) ───────────────────────────────────────

pub const SCENE_RESULT_ARC: &str = "data/arc/bm2d/scene_result_v3.arc";
pub const SCENE_RESULT_IFS: &str = "scene_result_v3.ifs";
pub const RESULT_IFS_MOD_PATH: &str = "scene_result_v3_ifs";
/// The judgement-count tab template (exported name = the afp_patcher key).
/// ONE template serves BOTH tabs: kind 6 "Details" plays `loop_registered`,
/// kind 1 "Simple results" plays `loop_guest` (same rows, moved +125px by
/// f127 translate updates).
pub const RESULT_TEMPLATE: &str = "body_tab_detail_result";
/// The 7-row label sheets (maintainer-approved 2026-08-30): STOCK-NAME
/// replacements of the 108×118 judgement-word sheets. `_judge` is the one
/// the template's shape 74 actually draws (maintainer screenshot-verified);
/// `_base` ships too as a just-in-case (no consumer found in the package or
/// gamemdx, but it is the same sheet family).
pub const RESULT_SHEETS: [&str; 2] = ["scre_tab_detail_judge", "scre_tab_detail_base"];
const RESULT_PNG_DIR: &str = "./data_mods/s_marvelous/scene_result";

/// Stage the results score-tab assets: copy the 7-row label sheets to the
/// stock-texture replacement serving path and extract + descramble the
/// stock tab template for the patch's byte gate. `None` ⇒ WARN already
/// logged, results tab stays stock (the caller must then NOT register the
/// patch — a 7-row sheet without the row repositioning misaligns).
pub struct StagedResultPatch {
    pub stock_bytes: Vec<u8>,
}

pub fn stage_results(dry_run_rows: impl Fn(&Ap2Doc) -> bool) -> Option<StagedResultPatch> {
    // Sheet PNGs first — missing art fails the whole staging (sheet and
    // row positions must move together).
    for sheet in RESULT_SHEETS {
        let src = format!("{}/{}.png", RESULT_PNG_DIR, sheet);
        if !std::path::Path::new(&src).exists() {
            log_warn!(
                "SMarvelous: results sheet missing at {} — results tab stays stock",
                src
            );
            return None;
        }
    }

    // ── Extract + descramble the stock tab template ─────────────────
    let arc_data = match std::fs::read(SCENE_RESULT_ARC) {
        Ok(d) => d,
        Err(e) => {
            log_warn!("SMarvelous: can't read {}: {}", SCENE_RESULT_ARC, e);
            return None;
        }
    };
    let Some(entries) = arc::parse(&arc_data) else {
        log_warn!("SMarvelous: failed to parse {}", SCENE_RESULT_ARC);
        return None;
    };
    let Some(ifs_entry) = entries.iter().find(|e| e.path.ends_with(SCENE_RESULT_IFS)) else {
        log_warn!(
            "SMarvelous: {} not in {}",
            SCENE_RESULT_IFS,
            SCENE_RESULT_ARC
        );
        return None;
    };
    let Some(ifs_data) = arc::extract(&arc_data, ifs_entry) else {
        log_warn!("SMarvelous: failed to extract {}", SCENE_RESULT_IFS);
        return None;
    };
    let tpl = RESULT_TEMPLATE.to_string();
    let afp_files = ifs::extract_files(&ifs_data, "afp", std::slice::from_ref(&tpl));
    let bsi_files = ifs::extract_files(&ifs_data, "afp/bsi", std::slice::from_ref(&tpl));
    let (Some((_, afp_raw)), Some((_, bsi_raw))) =
        (afp_files.into_iter().next(), bsi_files.into_iter().next())
    else {
        log_warn!(
            "SMarvelous: {} AFP/BSI not found in the IFS",
            RESULT_TEMPLATE
        );
        return None;
    };
    let Some(stock_bytes) = descramble(afp_raw, &bsi_raw) else {
        log_warn!("SMarvelous: {} descramble failed", RESULT_TEMPLATE);
        return None;
    };
    let Some(doc) = Ap2Doc::parse(&stock_bytes) else {
        log_warn!("SMarvelous: stock {} did not parse", RESULT_TEMPLATE);
        return None;
    };
    if doc.exported_name() != RESULT_TEMPLATE {
        log_warn!(
            "SMarvelous: unexpected exported name '{}' — results tab stays stock",
            doc.exported_name()
        );
        return None;
    }

    // Dry-run the row repositioning the patch fn will apply (the caller
    // supplies the transform so patch and staging share ONE code path).
    if !dry_run_rows(&doc) {
        log_warn!(
            "SMarvelous: {} row-shift dry run failed (layout drift?) — results tab stays stock",
            RESULT_TEMPLATE
        );
        return None;
    }

    // ── Stage the sheets at the serving path (enable-gated stock-name
    // replacement; init/disable purge them — see purge_results) ──────
    if !restage_result_sheets() {
        purge_results();
        return None;
    }

    log_info!(
        "SMarvelous: results tab staged ({} sheets, template {} bytes)",
        RESULT_SHEETS.len(),
        stock_bytes.len()
    );
    Some(StagedResultPatch { stock_bytes })
}

/// Copy the 7-row sheets to the serving path + drop any stale converted
/// cache. Factored from [`stage_results`] because a disable→re-enable
/// cycle purges the sheets but keeps the staged template bytes.
pub fn restage_result_sheets() -> bool {
    let tex_dir = format!("{}/{}/tex", MOD_ROOT, RESULT_IFS_MOD_PATH);
    if let Err(e) = std::fs::create_dir_all(&tex_dir) {
        log_warn!(
            "SMarvelous: mkdir {}: {} — results tab stays stock",
            tex_dir,
            e
        );
        return false;
    }
    for sheet in RESULT_SHEETS {
        let src = format!("{}/{}.png", RESULT_PNG_DIR, sheet);
        let dst = format!("{}/{}.png", tex_dir, sheet);
        if let Err(e) = std::fs::copy(&src, &dst) {
            log_warn!(
                "SMarvelous: can't stage {}: {} — results tab stays stock",
                dst,
                e
            );
            return false;
        }
        // A stale converted-cache copy of the STOCK art may exist from a
        // session where the mod was disabled; drop it so the next open
        // reconverts from the staged 7-row PNG.
        ifs_textures::purge_texture_replacement(RESULT_IFS_MOD_PATH, sheet);
    }

    // First-boot mod-path visibility (house rule).
    let probe_rel = format!("{}/tex/{}.png", RESULT_IFS_MOD_PATH, RESULT_SHEETS[0]);
    if mod_paths::find_first_modfile(&probe_rel).is_none() {
        log_info!("SMarvelous: results assets not in mod-path cache — rescanning");
        mod_paths::init_mod_paths();
    }
    true
}

/// Remove the staged results sheets + their converted-cache entries. Runs
/// at mod INIT (covers the config-disabled boot — the PNGs are stock-name
/// replacements that LayeredFS serves passively from disk, so a disabled
/// mod must not leave them staged: 7-row art under 6-row stock positions
/// misaligns) and at disable (live toggle: future opens revert; textures
/// already mounted this session stay, consistent with the in-memory
/// template patch persisting).
pub fn purge_results() {
    let tex_dir = format!("{}/{}/tex", MOD_ROOT, RESULT_IFS_MOD_PATH);
    for sheet in RESULT_SHEETS {
        let png = format!("{}/{}.png", tex_dir, sheet);
        if std::path::Path::new(&png).exists() {
            match std::fs::remove_file(&png) {
                Ok(()) => log_info!("SMarvelous: unstaged {}", png),
                Err(e) => log_warn!("SMarvelous: can't unstage {}: {}", png, e),
            }
        }
        ifs_textures::purge_texture_replacement(RESULT_IFS_MOD_PATH, sheet);
    }
}

// ── Full-combo splash (Step 6) ───────────────────────────────────────

pub const DANCE_FC_ARC: &str = "data/arc/bm2d/dance_fullcombo_v3.arc";
pub const DANCE_FC_IFS: &str = "dance_fullcombo_v3.ifs";
pub const FC_IFS_MOD_PATH: &str = "dance_fullcombo_v3_ifs";
/// The four splash templates (all carry `marbelous_in` in root + inner
/// timeline — Appendix-B dump, 2026-08-30).
pub const FC_TEMPLATES: [&str; 4] = [
    "01_fullcombo_single_normal",
    "01_fullcombo_single_reverse",
    "02_fullcombo_double_normal",
    "02_fullcombo_double_reverse",
];
pub const FC_SRC_LABEL: &str = "marbelous_in"; // sic — Konami's typo
pub const FC_NEW_LABEL: &str = "s_marbelous_in";
const FC_PNG_DIR: &str = "./data_mods/s_marvelous/dance_fullcombo";
const FC_ATLAS_PREFIX: &str = "smarv_fc";

/// Marvelous-art region → (new region, mod art file). The rename rule:
/// prefix `s` onto the last underscore token iff it starts with `mar`
/// (`dafu_eff_mar`→`dafu_eff_smar`, `dafu_light_marvelous`→
/// `dafu_light_smarvelous`) — the shipped art filenames follow it exactly.
fn fc_region_rename(region: &str) -> Option<String> {
    let (head, tail) = region.rsplit_once('_')?;
    if !tail.starts_with("mar") {
        return None;
    }
    Some(format!("{}_s{}", head, tail))
}

/// Mod art path for a NEW region name (`dafu_eff_smar` →
/// `dance_fullcombo/dafu_eff_smar.png`). Note the shipped files are named
/// by their historical short names; normalize via a lookup.
fn fc_art_path(new_region: &str) -> String {
    format!("{}/{}.png", FC_PNG_DIR, new_region)
}

/// Everything one splash template's patch needs.
pub struct StagedFcPatch {
    pub template: &'static str,
    pub stock_bytes: Vec<u8>,
    /// Donor art shape ids in resolution order.
    pub shape_ids: Vec<u16>,
    /// Expected new ids from the dry run (patch-time verification).
    pub expected: crate::core::ap2::MultiShapeSegmentClone,
}

/// Stage the S-MFC splash chain: per template — geo-first art-shape
/// resolution, dry-run of the multi-shape recipe, rewritten geos, geo MD5
/// mappings + afplist extensions; once per IFS — donor-anchored atlas
/// clone + per-image PNGs. Returns the staged patches (empty = fully
/// unstaged, one WARN per failure; per-template failures skip that
/// template only).
pub fn stage_fullcombo() -> Vec<StagedFcPatch> {
    let arc_data = match std::fs::read(DANCE_FC_ARC) {
        Ok(d) => d,
        Err(e) => {
            log_warn!(
                "SMarvelous: can't read {}: {} — splash unstaged",
                DANCE_FC_ARC,
                e
            );
            return Vec::new();
        }
    };
    let Some(entries) = arc::parse(&arc_data) else {
        log_warn!(
            "SMarvelous: failed to parse {} — splash unstaged",
            DANCE_FC_ARC
        );
        return Vec::new();
    };
    let Some(ifs_entry) = entries.iter().find(|e| e.path.ends_with(DANCE_FC_IFS)) else {
        log_warn!(
            "SMarvelous: {} not in {} — splash unstaged",
            DANCE_FC_IFS,
            DANCE_FC_ARC
        );
        return Vec::new();
    };
    let Some(ifs_data) = arc::extract(&arc_data, ifs_entry) else {
        log_warn!(
            "SMarvelous: failed to extract {} — splash unstaged",
            DANCE_FC_IFS
        );
        return Vec::new();
    };

    let geo_dir = format!("{}/{}/geo", MOD_ROOT, FC_IFS_MOD_PATH);
    if let Err(e) = std::fs::create_dir_all(&geo_dir) {
        log_warn!("SMarvelous: mkdir {}: {} — splash unstaged", geo_dir, e);
        return Vec::new();
    }

    // Region set across templates (they share the four art regions) for
    // the one-time texture staging below.
    let mut regions: Vec<(String, String)> = Vec::new(); // (donor, new)
    let mut staged: Vec<StagedFcPatch> = Vec::new();

    for template in FC_TEMPLATES {
        let tpl = template.to_string();
        let afp_files = ifs::extract_files(&ifs_data, "afp", std::slice::from_ref(&tpl));
        let bsi_files = ifs::extract_files(&ifs_data, "afp/bsi", std::slice::from_ref(&tpl));
        let (Some((_, afp_raw)), Some((_, bsi_raw))) =
            (afp_files.into_iter().next(), bsi_files.into_iter().next())
        else {
            log_warn!("SMarvelous: {} AFP/BSI missing — skipped", template);
            continue;
        };
        let Some(stock_bytes) = descramble(afp_raw, &bsi_raw) else {
            log_warn!("SMarvelous: {} descramble failed — skipped", template);
            continue;
        };
        let Some(doc) = Ap2Doc::parse(&stock_bytes) else {
            log_warn!("SMarvelous: {} did not parse — skipped", template);
            continue;
        };

        // Geo-first art-shape resolution: every Shape whose geo carries a
        // region the rename rule accepts. (No suffix assumption — covers
        // `_mar` AND `_marvelous`.)
        let mut shape_ids: Vec<u16> = Vec::new();
        let mut donor_geos: Vec<(u16, Vec<u8>, String, String)> = Vec::new();
        for tag in &doc.root.tags {
            let crate::core::ap2::Tag::Shape(shape) = tag else {
                continue;
            };
            let geo_name = format!("{}_shape{}", doc.exported_name(), shape.id);
            let extracted = ifs::extract_files(&ifs_data, "geo", std::slice::from_ref(&geo_name));
            let Some((_, geo_bytes)) = extracted.into_iter().next() else {
                continue;
            };
            let Some(labels) = geo::labels(&geo_bytes) else {
                continue;
            };
            for l in &labels {
                if let Some(new_region) = fc_region_rename(l) {
                    shape_ids.push(shape.id);
                    donor_geos.push((shape.id, geo_bytes.clone(), l.clone(), new_region));
                    break;
                }
            }
        }
        if shape_ids.len() != 4 {
            log_warn!(
                "SMarvelous: {} resolved {} art shapes (want 4) — skipped",
                template,
                shape_ids.len()
            );
            continue;
        }

        // Dry-run the recipe for the allocated ids.
        let mut scratch = doc.clone();
        let Some(expected) =
            scratch.clone_segment_with_new_shapes(FC_SRC_LABEL, FC_NEW_LABEL, &shape_ids)
        else {
            log_warn!("SMarvelous: {} dry-run failed — skipped", template);
            continue;
        };
        if scratch.serialize().is_none() {
            log_warn!(
                "SMarvelous: {} patched doc does not serialize — skipped",
                template
            );
            continue;
        }

        // Rewritten geos + MD5 mappings, named by the NEW shape ids.
        let mut ok = true;
        let mut new_geo_names: Vec<String> = Vec::new();
        for ((old_id, geo_bytes, donor_region, new_region), (old2, new_id)) in
            donor_geos.iter().zip(expected.shapes.iter())
        {
            debug_assert_eq!(old_id, old2);
            let Some(new_geo) = geo::rewrite_labels(geo_bytes, |l| {
                if l == donor_region {
                    Some(new_region.clone())
                } else {
                    None
                }
            }) else {
                log_warn!(
                    "SMarvelous: {} geo rewrite failed ({})",
                    template,
                    donor_region
                );
                ok = false;
                break;
            };
            let geo_name = format!("{}_shape{}", template, new_id);
            if let Err(e) = std::fs::write(format!("{}/{}", geo_dir, geo_name), &new_geo) {
                log_warn!("SMarvelous: can't write {}: {}", geo_name, e);
                ok = false;
                break;
            }
            new_geo_names.push(geo_name);
            if !regions.iter().any(|(d, _)| d == donor_region) {
                regions.push((donor_region.clone(), new_region.clone()));
            }
        }
        if !ok {
            continue;
        }
        for geo_name in &new_geo_names {
            ifs_textures::register_afp_geo_mapping(DANCE_FC_IFS, geo_name);
        }
        let new_ids: Vec<u16> = expected.shapes.iter().map(|(_, n)| *n).collect();
        ifs_textures::register_afplist_geo_extension(DANCE_FC_IFS, template, &new_ids);

        staged.push(StagedFcPatch {
            template,
            stock_bytes,
            shape_ids,
            expected,
        });
    }

    if staged.is_empty() {
        return staged;
    }

    // ── One-time texture staging (shared across templates) ──────────
    let tex_dir = format!("{}/{}/tex", MOD_ROOT, FC_IFS_MOD_PATH);
    if let Err(e) = std::fs::create_dir_all(&tex_dir) {
        log_warn!("SMarvelous: mkdir {}: {} — splash unstaged", tex_dir, e);
        return Vec::new();
    }
    let mut specs = Vec::new();
    for (donor, new_region) in &regions {
        let src = fc_art_path(new_region);
        if !std::path::Path::new(&src).exists() {
            log_warn!(
                "SMarvelous: splash art missing at {} — splash unstaged",
                src
            );
            return Vec::new();
        }
        let dst = format!("{}/{}.png", tex_dir, new_region);
        if let Err(e) = std::fs::copy(&src, &dst) {
            log_warn!("SMarvelous: can't stage {}: {} — splash unstaged", dst, e);
            return Vec::new();
        }
        specs.push(OwnedTextureSpec {
            new_name: new_region.clone(),
            donor_name: donor.clone(),
            png_path: src,
        });
    }
    let Some(texlist) = load_stock_texturelist(DANCE_FC_ARC, DANCE_FC_IFS) else {
        log_warn!("SMarvelous: stock dance_fullcombo texturelist unavailable — splash unstaged");
        return Vec::new();
    };
    let batch = [AtlasSet {
        atlas_prefix: FC_ATLAS_PREFIX.to_string(),
        specs,
        fresh: false, // donor-anchored: cloned geo UVs must stay valid
    }];
    match generate_cloned_atlases_cached(&texlist, FC_IFS_MOD_PATH, CACHE_ROOT, MOD_ROOT, &batch) {
        BatchResult::Nothing => {
            log_warn!("SMarvelous: splash atlas injection produced nothing — splash unstaged");
            return Vec::new();
        }
        BatchResult::Cached | BatchResult::Rebuilt => {}
    }

    // First-boot mod-path visibility (same rule as the other stagings).
    let merged_rel = format!("{}/tex/texturelist.merged.xml", FC_IFS_MOD_PATH);
    let probe_geo = format!(
        "{}/geo/{}_shape{}",
        FC_IFS_MOD_PATH, staged[0].template, staged[0].expected.shapes[0].1
    );
    if mod_paths::find_first_modfile(&merged_rel).is_none()
        || mod_paths::find_first_modfile(&probe_geo).is_none()
    {
        log_info!("SMarvelous: splash assets not in mod-path cache — rescanning");
        mod_paths::init_mod_paths();
    }

    log_info!(
        "SMarvelous: splash staged ({} template(s), {} region(s))",
        staged.len(),
        regions.len()
    );
    staged
}

// ── FC emblems (Step 9) ──────────────────────────────────────────────

/// The results-scene ROOT template (exported name = the afp_patcher key).
/// Ghidra-verified (FUN_1800b84c0): the scene builds its ONE layer from
/// "result_root", whose self-contained fc timeline (sprite 243, labels
/// loop_fc..loop_assisted) serves BOTH player panes' `fc_usr` instances —
/// one patch covers 1P and 2P.
pub const EMBLEM_TEMPLATE: &str = "result_root";
pub const EMBLEM_SRC_LABEL: &str = "loop_mfc";
pub const EMBLEM_NEW_LABEL: &str = "loop_smfc";
/// Injected total-results badge texture (bitmap-load target under
/// `total_p%d_top_usr/fullcombo_usr` — name-only binding, no geo; the
/// combo-digit precedent). Name parallels the stock DAT_180486E80 family
/// ("scre_total_player_" + "fc_mfc").
pub const EMBLEM_TOTAL_TEXTURE: &str = "scre_total_player_fc_smfc";
const EMBLEM_TOTAL_DONOR: &str = "scre_total_player_fc_mfc";
const EMBLEM_PNG_DIR: &str = "./data_mods/s_marvelous/scene_result";
const EMBLEM_ATLAS_PREFIX: &str = "smarv_fce";

/// Everything the result_root emblem patch needs.
pub struct StagedEmblemPatch {
    pub stock_bytes: Vec<u8>,
    /// The word-art shape id (geo region `scre_fc_marvelous`) — exactly one.
    pub shape_ids: Vec<u16>,
    /// Expected new ids from the dry run (patch-time verification).
    pub expected: crate::core::ap2::MultiShapeSegmentClone,
}

/// The clone options the emblem patch runs with — shared between staging
/// dry-run, the patch fn, and the offline harness (deploy-#3 lesson: one
/// code path). `drop_hsl_updates_on_remapped`: the stock loop_mfc word
/// object carries per-frame HSL-rotation updates (the rainbow flow) that
/// would hue-cycle the violet art; `retarget_actions`: the segment loops
/// via a `gotoAndPlay("loop_mfc")` DoAction at its end — the clone's copy
/// must jump to `loop_smfc` or the emblem reverts to stock art after one
/// pass (~1.7 s).
pub const EMBLEM_CLONE_OPTS: crate::core::ap2::SegmentCloneOpts =
    crate::core::ap2::SegmentCloneOpts {
        drop_hsl_updates_on_remapped: true,
        retarget_actions: true,
    };

/// Stage the FC-emblem chain: extract + descramble `result_root`, resolve
/// the word-art shape geo-first (the Step-6 rename rule — last `_`-token
/// starting `mar`; unique on result_root: shape 202 → `scre_fc_marvelous`),
/// dry-run the clone recipe, write the rewritten geo + register the geo
/// MD5 mapping and afplist extension, and stage BOTH textures (the
/// donor-anchored violet word + the FRESH total-results badge) in one
/// atlas batch. `None` ⇒ WARN logged, emblems stay stock.
pub fn stage_emblems() -> Option<StagedEmblemPatch> {
    // Art first — either PNG missing fails the whole staging.
    let word_png = format!("{}/{}.png", EMBLEM_PNG_DIR, "scre_fc_smarvelous");
    let badge_png = format!("{}/{}.png", EMBLEM_PNG_DIR, EMBLEM_TOTAL_TEXTURE);
    for png in [&word_png, &badge_png] {
        if !std::path::Path::new(png.as_str()).exists() {
            log_warn!(
                "SMarvelous: emblem art missing at {} — emblems stay stock",
                png
            );
            return None;
        }
    }

    // ── Extract + descramble result_root ────────────────────────────
    let arc_data = match std::fs::read(SCENE_RESULT_ARC) {
        Ok(d) => d,
        Err(e) => {
            log_warn!(
                "SMarvelous: can't read {}: {} — emblems stay stock",
                SCENE_RESULT_ARC,
                e
            );
            return None;
        }
    };
    let Some(entries) = arc::parse(&arc_data) else {
        log_warn!(
            "SMarvelous: failed to parse {} — emblems stay stock",
            SCENE_RESULT_ARC
        );
        return None;
    };
    let Some(ifs_entry) = entries.iter().find(|e| e.path.ends_with(SCENE_RESULT_IFS)) else {
        log_warn!(
            "SMarvelous: {} not in {} — emblems stay stock",
            SCENE_RESULT_IFS,
            SCENE_RESULT_ARC
        );
        return None;
    };
    let Some(ifs_data) = arc::extract(&arc_data, ifs_entry) else {
        log_warn!(
            "SMarvelous: failed to extract {} — emblems stay stock",
            SCENE_RESULT_IFS
        );
        return None;
    };
    let tpl = EMBLEM_TEMPLATE.to_string();
    let afp_files = ifs::extract_files(&ifs_data, "afp", std::slice::from_ref(&tpl));
    let bsi_files = ifs::extract_files(&ifs_data, "afp/bsi", std::slice::from_ref(&tpl));
    let (Some((_, afp_raw)), Some((_, bsi_raw))) =
        (afp_files.into_iter().next(), bsi_files.into_iter().next())
    else {
        log_warn!(
            "SMarvelous: {} AFP/BSI not found — emblems stay stock",
            EMBLEM_TEMPLATE
        );
        return None;
    };
    let Some(stock_bytes) = descramble(afp_raw, &bsi_raw) else {
        log_warn!(
            "SMarvelous: {} descramble failed — emblems stay stock",
            EMBLEM_TEMPLATE
        );
        return None;
    };
    let Some(doc) = Ap2Doc::parse(&stock_bytes) else {
        log_warn!(
            "SMarvelous: stock {} did not parse — emblems stay stock",
            EMBLEM_TEMPLATE
        );
        return None;
    };
    if doc.exported_name() != EMBLEM_TEMPLATE {
        log_warn!(
            "SMarvelous: unexpected exported name '{}' — emblems stay stock",
            doc.exported_name()
        );
        return None;
    }

    // ── Geo-first word-shape resolution (Step-6 rename rule) ────────
    let mut word: Option<(u16, Vec<u8>, String, String)> = None;
    for tag in &doc.root.tags {
        let crate::core::ap2::Tag::Shape(shape) = tag else {
            continue;
        };
        let geo_name = format!("{}_shape{}", doc.exported_name(), shape.id);
        let extracted = ifs::extract_files(&ifs_data, "geo", std::slice::from_ref(&geo_name));
        let Some((_, geo_bytes)) = extracted.into_iter().next() else {
            continue;
        };
        let Some(labels) = geo::labels(&geo_bytes) else {
            continue;
        };
        for l in &labels {
            if let Some(new_region) = fc_region_rename(l) {
                if word.is_some() {
                    log_warn!("SMarvelous: result_root word shape ambiguous — emblems stay stock");
                    return None;
                }
                word = Some((shape.id, geo_bytes.clone(), l.clone(), new_region));
                break;
            }
        }
    }
    let Some((word_shape, word_geo, donor_region, new_region)) = word else {
        log_warn!("SMarvelous: result_root word shape unresolved — emblems stay stock");
        return None;
    };

    // ── Dry-run the recipe (allocated ids for patch-time verify) ────
    let shape_ids = vec![word_shape];
    let mut scratch = doc.clone();
    let Some(expected) = scratch.clone_segment_with_new_shapes_ex(
        EMBLEM_SRC_LABEL,
        EMBLEM_NEW_LABEL,
        &shape_ids,
        EMBLEM_CLONE_OPTS,
    ) else {
        log_warn!(
            "SMarvelous: {} emblem dry-run failed — emblems stay stock",
            EMBLEM_TEMPLATE
        );
        return None;
    };
    if scratch.serialize().is_none() {
        log_warn!(
            "SMarvelous: patched {} does not serialize — emblems stay stock",
            EMBLEM_TEMPLATE
        );
        return None;
    }

    // ── Rewritten geo + registrations ────────────────────────────────
    let geo_dir = format!("{}/{}/geo", MOD_ROOT, RESULT_IFS_MOD_PATH);
    if let Err(e) = std::fs::create_dir_all(&geo_dir) {
        log_warn!("SMarvelous: mkdir {}: {} — emblems stay stock", geo_dir, e);
        return None;
    }
    let Some(new_geo) = geo::rewrite_labels(&word_geo, |l| {
        if l == donor_region.as_str() {
            Some(new_region.clone())
        } else {
            None
        }
    }) else {
        log_warn!(
            "SMarvelous: emblem geo rewrite failed ({}) — emblems stay stock",
            donor_region
        );
        return None;
    };
    let (_, new_shape) = expected.shapes[0];
    let geo_name = format!("{}_shape{}", EMBLEM_TEMPLATE, new_shape);
    if let Err(e) = std::fs::write(format!("{}/{}", geo_dir, geo_name), &new_geo) {
        log_warn!(
            "SMarvelous: can't write {}: {} — emblems stay stock",
            geo_name,
            e
        );
        return None;
    }
    ifs_textures::register_afp_geo_mapping(SCENE_RESULT_IFS, &geo_name);
    ifs_textures::register_afplist_geo_extension(SCENE_RESULT_IFS, EMBLEM_TEMPLATE, &[new_shape]);

    // ── Textures: per-image PNGs + one atlas batch (donor-anchored word
    // + FRESH total badge) ───────────────────────────────────────────
    let tex_dir = format!("{}/{}/tex", MOD_ROOT, RESULT_IFS_MOD_PATH);
    if let Err(e) = std::fs::create_dir_all(&tex_dir) {
        log_warn!("SMarvelous: mkdir {}: {} — emblems stay stock", tex_dir, e);
        return None;
    }
    for (src, name) in [
        (&word_png, new_region.as_str()),
        (&badge_png, EMBLEM_TOTAL_TEXTURE),
    ] {
        let dst = format!("{}/{}.png", tex_dir, name);
        if let Err(e) = std::fs::copy(src, &dst) {
            log_warn!(
                "SMarvelous: can't stage {}: {} — emblems stay stock",
                dst,
                e
            );
            return None;
        }
    }
    let Some(texlist) = load_stock_texturelist(SCENE_RESULT_ARC, SCENE_RESULT_IFS) else {
        log_warn!("SMarvelous: stock scene_result texturelist unavailable — emblems stay stock");
        return None;
    };
    let batch = [
        AtlasSet {
            atlas_prefix: EMBLEM_ATLAS_PREFIX.to_string(),
            specs: vec![OwnedTextureSpec {
                new_name: new_region.clone(),
                donor_name: donor_region.clone(),
                png_path: word_png.clone(),
            }],
            fresh: false, // donor-anchored: the cloned geo's UVs are the donor's
        },
        AtlasSet {
            atlas_prefix: format!("{}_t", EMBLEM_ATLAS_PREFIX),
            specs: vec![OwnedTextureSpec {
                new_name: EMBLEM_TOTAL_TEXTURE.to_string(),
                donor_name: EMBLEM_TOTAL_DONOR.to_string(),
                png_path: badge_png.clone(),
            }],
            fresh: true, // net-new badge — bitmap-load binds by name alone
        },
    ];
    match generate_cloned_atlases_cached(
        &texlist,
        RESULT_IFS_MOD_PATH,
        CACHE_ROOT,
        MOD_ROOT,
        &batch,
    ) {
        BatchResult::Nothing => {
            log_warn!("SMarvelous: emblem atlas injection produced nothing — emblems stay stock");
            return None;
        }
        BatchResult::Cached | BatchResult::Rebuilt => {}
    }

    // First-boot mod-path visibility (house rule).
    let merged_rel = format!("{}/tex/texturelist.merged.xml", RESULT_IFS_MOD_PATH);
    let probe_geo = format!("{}/geo/{}", RESULT_IFS_MOD_PATH, geo_name);
    if mod_paths::find_first_modfile(&merged_rel).is_none()
        || mod_paths::find_first_modfile(&probe_geo).is_none()
    {
        log_info!("SMarvelous: emblem assets not in mod-path cache — rescanning");
        mod_paths::init_mod_paths();
    }

    log_info!(
        "SMarvelous: emblems staged (word shape {} -> {}, region {} -> {}, badge {})",
        word_shape,
        new_shape,
        donor_region,
        new_region,
        EMBLEM_TOTAL_TEXTURE
    );
    Some(StagedEmblemPatch {
        stock_bytes,
        shape_ids,
        expected,
    })
}
