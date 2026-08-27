//! Runtime generation of Mods-tab atlas textures.
//!
//! Two texture categories:
//!
//! 1. **Static tab textures** — `seop_tab_icon_mods` and
//!    `seop_tab_title_mods`, hardcoded here because they're required for
//!    the Mods tab itself to appear; not per-option. Generated once at
//!    `custom_options::init()` time.
//!
//! 2. **Per-option row labels** — `seop_item_<option_id>`, registered
//!    automatically by [`register_label_for`] when a mod calls
//!    `register_option`. The registered list accumulates and drives an
//!    idempotent rebuild of every language's options atlas at flush time.
//!
//! Every label texture clones the same donor (`seop_item_appearance`)
//! for its atlas slot. Textures are injected **per language**: the game
//! loads `select_music_option_lang_<code>_v3.ifs` (code ∈ eng/jpn/kor) for
//! the player's language selection, so the flush builds all three languages'
//! atlases (see [`OPTION_LANGS`]) — each sourced from its own mod folder at
//! `data_mods/custom_options/select_music_option_lang_<code>_v3_ifs/tex/`
//! and cloned against its own stock ARC's texturelist. A language whose
//! stock data or mod folder is missing is skipped with one WARN; the others
//! still build.
//!
//! Each rebuild regenerates the full merged.xml + cached atlas from
//! scratch using the current spec list — cheap (~ms per rebuild) and
//! idempotent, so re-running it per-option during mod registration is
//! fine. The alternative (accumulate then generate once after all mods
//! enable) would require an explicit finalize hook in the main init
//! sequence; on-demand rebuild keeps the API tight and the caller-side
//! contract simple: register your option, the label atlas is ready.

use once_cell::sync::Lazy;
use std::path::Path;
use std::sync::Mutex;

use crate::services::avs_layeredfs::atlas_cloner::{
    generate_cloned_atlases, load_stock_texturelist, NewTextureSpec,
};
use crate::{log_info, log_warn};

const MOD_ROOT: &str = "./data_mods/custom_options";
const CACHE_ROOT: &str = "./data_mods/_cache";

/// One game UI language whose options IFS receives the injected Mods-tab
/// textures. The game loads `select_music_option_lang_<code>_v3.ifs` for the
/// player's per-user language selection; the DLL prepares all three IFSes'
/// injections at init and the game opens exactly one — no runtime language
/// detection needed. Shared with `webui_options::preview_gen`, which derives
/// its per-language template/chrome directories from the same table.
pub(crate) struct OptionLang {
    /// The IFS path language code (`eng` / `jpn` / `kor`).
    pub ifs_code: &'static str,
    /// Stock ARC carrying this language's options IFS.
    pub arc_path: &'static str,
    /// IFS name inside the ARC (also the game-side open path).
    pub ifs_name: &'static str,
    /// `data_mods` mod-folder name LayeredFS maps onto the IFS.
    pub ifs_mod_path: &'static str,
    /// Atlas prefix for the base cloned atlas (tab title + labels +
    /// ribbons). Language-distinct so no atlas texture name is shared
    /// between two languages' outputs.
    pub atlas_prefix: &'static str,
    /// Atlas prefix for the fresh preview-image atlases. Preview images are
    /// large (~368x172) and there can be hundreds across all enum options,
    /// so they don't share the base atlas: they go into one fresh `AtlasSet`
    /// under this prefix and the cloner spills into additional atlases
    /// (`copt_prev_eng_000`, `_001`, …) automatically when one fills. See
    /// `docs/option_preview_image_box.md`.
    pub preview_atlas_prefix: &'static str,
}

/// The three languages DDR World's options menu ships in. Written out
/// longhand (not string-formatted at runtime) so every path stays greppable;
/// the unit test below pins the naming scheme.
pub(crate) const OPTION_LANGS: [OptionLang; 3] = [
    OptionLang {
        ifs_code: "eng",
        arc_path: "data/arc/bm2d/select_music_option_lang_eng_v3.arc",
        ifs_name: "select_music_option_lang_eng_v3.ifs",
        ifs_mod_path: "select_music_option_lang_eng_v3_ifs",
        atlas_prefix: "copt_mods_lang_eng",
        preview_atlas_prefix: "copt_prev_eng",
    },
    OptionLang {
        ifs_code: "jpn",
        arc_path: "data/arc/bm2d/select_music_option_lang_jpn_v3.arc",
        ifs_name: "select_music_option_lang_jpn_v3.ifs",
        ifs_mod_path: "select_music_option_lang_jpn_v3_ifs",
        atlas_prefix: "copt_mods_lang_jpn",
        preview_atlas_prefix: "copt_prev_jpn",
    },
    OptionLang {
        ifs_code: "kor",
        arc_path: "data/arc/bm2d/select_music_option_lang_kor_v3.arc",
        ifs_name: "select_music_option_lang_kor_v3.ifs",
        ifs_mod_path: "select_music_option_lang_kor_v3_ifs",
        atlas_prefix: "copt_mods_lang_kor",
        preview_atlas_prefix: "copt_prev_kor",
    },
];

/// Every row-label texture clones this donor's atlas slot. Using a
/// single donor for all option labels keeps the API surface to "just
/// pass your option id" and avoids per-option donor configuration.
const LABEL_DONOR: &str = "seop_item_appearance";

/// Every preview-image texture clones this donor's atlas slot. All
/// `seop_image_*` textures occupy the same preview-box-sized region, so a
/// single stock donor's imgrect/uvrect is correct for every custom preview.
/// `seop_image_scroll_speed` is a stock single-image (scalar) preview that
/// reliably exists in every language's IFS. See
/// `docs/option_preview_image_box.md`.
const PREVIEW_DONOR: &str = "seop_image_scroll_speed";

/// Every net-new value-ribbon chip (`seop_op_*`) clones this donor's atlas
/// slot. All value-ribbon labels share one chip size, so the stock
/// `seop_op_on` slot's imgrect/uvrect is correct for any custom ribbon. Only
/// labels NOT already in the stock atlas need injecting — `seop_op_on`,
/// `seop_op_off`, etc. resolve natively and are skipped (see
/// [`register_op_ribbons`]).
const RIBBON_DONOR: &str = "seop_op_on";

/// Stock value-ribbon names that already exist in the game atlas, so a mod
/// referencing them needs no injection. Net-new ribbon names not in this set
/// get cloned off [`RIBBON_DONOR`]. Conservative on purpose: a stock name
/// wrongly omitted here just gets a redundant (harmless) clone; a net-new
/// name wrongly included here would render blank. Extend as more stock
/// labels are confirmed resident.
const STOCK_RIBBONS: &[&str] = &[
    "seop_op_on",
    "seop_op_off",
    // Stock atlas members used by training_mode's TIMELINE PLACEMENT —
    // confirmed resident (their omission only cost redundant clones plus
    // 6 atlas-REBUILD boot WARNs; Step 9 fix).
    "seop_op_left",
    "seop_op_right",
];

/// Accumulated per-option label textures, populated by
/// [`register_label_for`] as mods call `register_option`. Each entry
/// becomes one `NewTextureSpec` at atlas-rebuild time.
static LABEL_REGISTRATIONS: Lazy<Mutex<Vec<String>>> = Lazy::new(|| Mutex::new(Vec::new()));

/// Accumulated preview-image texture NAMES (`seop_image_<id>` and per-value
/// `seop_image_<id>_<key>`), populated by [`register_preview_images`]
/// alongside the labels. Each becomes a `NewTextureSpec` cloning
/// [`PREVIEW_DONOR`] at atlas-rebuild time. Lives in the same language IFSes
/// as the labels, so it rides the same rebuild passes. Stores full texture names
/// (not option ids) because one enum option contributes several.
static PREVIEW_REGISTRATIONS: Lazy<Mutex<Vec<String>>> = Lazy::new(|| Mutex::new(Vec::new()));

/// Accumulated net-new value-ribbon texture NAMES (`seop_op_<key>`),
/// populated by [`register_op_ribbons`] for enum values whose ribbon isn't a
/// stock label. Each becomes a `NewTextureSpec` cloning [`RIBBON_DONOR`] in
/// the same per-language atlas passes. Deduplicated; one ribbon name may be shared
/// by several options (the namespace is flat), so this is keyed by the full
/// `seop_op_*` name, not the option id.
static RIBBON_REGISTRATIONS: Lazy<Mutex<Vec<String>>> = Lazy::new(|| Mutex::new(Vec::new()));

/// Preview-image texture names whose source PNG actually existed at atlas
/// flush time (i.e. that were genuinely injected and are resolvable in-game).
/// Populated by `flush_label_atlas`. The IOptionElement slot-0 getter
/// consults this so it returns `""` (→ native binder hides the preview box)
/// for a value whose PNG wasn't shipped, instead of a name that fails to bind
/// and leaves the previous row's image showing. See
/// `docs/option_preview_image_box.md`.
static AVAILABLE_PREVIEWS: Lazy<Mutex<Vec<String>>> = Lazy::new(|| Mutex::new(Vec::new()));

/// Whether preview image `name` was injected (its PNG existed at flush time)
/// and is therefore safe to hand to the native preview binder. Returns
/// `false` for any name not shipped — the getter then emits `""` so the box
/// hides rather than showing a stale texture.
pub(crate) fn preview_is_available(name: &str) -> bool {
    AVAILABLE_PREVIEWS.lock().unwrap().iter().any(|n| n == name)
}

/// Static tab-icon texture required for the Mods tab to appear at all.
/// Generated once at init time; independent of per-option registrations.
/// `seop_tab_icon_mods` lives in the BASE options IFS
/// (`select_music_option_v3.ifs`).
///
/// The companion `seop_tab_title_mods` (in the LANGUAGE-specific IFS) is NOT
/// built here — it's part of [`flush_label_atlas`]'s base atlas set, built
/// once after all options register. Building the language atlases from two
/// places (here with zero previews, then again at flush with all previews)
/// caused (a) a redundant double-build every boot and (b) the two passes to
/// perpetually invalidate each other's input-hash, defeating the warm-boot
/// cache skip. Keeping the language atlases owned solely by `flush_label_atlas`
/// fixes both.
pub(crate) fn generate_static_tab_assets() -> usize {
    let tab_icon_xml = match load_stock_texturelist(
        "data/arc/bm2d/select_music_option_v3.arc",
        "select_music_option_v3.ifs",
    ) {
        Some(x) => x,
        None => return 0,
    };
    let tab_icon_png_path = format!(
        "{}/select_music_option_v3_ifs/tex/seop_tab_icon_mods.png",
        MOD_ROOT
    );
    let tab_icon_specs = [NewTextureSpec {
        new_name: "seop_tab_icon_mods",
        donor_name: "seop_tab_icon_basic",
        png_path: &tab_icon_png_path,
    }];

    let mut total = 0;
    if generate_cloned_atlases(
        &tab_icon_xml,
        "select_music_option_v3_ifs",
        CACHE_ROOT,
        MOD_ROOT,
        "copt_mods",
        &tab_icon_specs,
    ) {
        total += 1;
        log_info!("custom_options/asset_gen: generated base tab-icon atlas");
    }

    // NOTE: the per-language atlases (tab title + labels + ribbons +
    // previews) are built solely by `flush_label_atlas`, once, after all
    // options register.
    total
}

/// Record that an option with `option_id` wants a row-label texture at
/// `seop_item_<option_id>`. Append-only: does NOT rebuild the atlas. Call
/// `flush_label_atlas` once after all options are registered to perform
/// a single rebuild that captures every label.
///
/// This append-only design avoids an O(N²) rebuild storm — earlier
/// versions rebuilt the entire atlas after every registration, which
/// caused webui-options' `enable()` to take ~2.5s on the cabinet.
///
/// Called automatically from `register_option`; mods don't invoke this
/// directly. Safe to re-register the same `option_id` (deduplicated).
pub(crate) fn register_label_for(option_id: &str) {
    let mut registrations = LABEL_REGISTRATIONS.lock().unwrap();
    if !registrations.iter().any(|id| id == option_id) {
        registrations.push(option_id.to_string());
    }
}

/// Record the preview-image texture names an option can display (shown in
/// the options menu's preview box when the row is focused). Pass the full
/// names — `seop_image_<id>` and/or per-value `seop_image_<id>_<key>` — as
/// produced by `RegisteredOption::preview_image_names()`. Append-only and
/// deduplicated, same contract as [`register_label_for`]; captured in the
/// same `flush_label_atlas` rebuild because preview images share the
/// language IFSes with the labels.
///
/// Called automatically from `register_option`; mods don't invoke this
/// directly. A name with no shipped `<name>.png` simply gets a blank preview
/// box — the clone is best-effort per the atlas cloner.
pub(crate) fn register_preview_images(names: &[String]) {
    let mut registrations = PREVIEW_REGISTRATIONS.lock().unwrap();
    for name in names {
        if !registrations.iter().any(|n| n == name) {
            registrations.push(name.clone());
        }
    }
}

/// Record net-new value-ribbon texture names (`seop_op_<key>`) that need
/// atlas injection. Pass the full `seop_op_*` names an enum option uses;
/// stock labels (see [`STOCK_RIBBONS`]) are filtered out here since they
/// already resolve from the game atlas. Append-only and deduplicated, same
/// contract as the label/preview registrations; captured in the same
/// `flush_label_atlas` rebuild. A net-new name with no shipped
/// `<name>.png` → blank ribbon, not an error.
pub(crate) fn register_op_ribbons(names: &[String]) {
    let mut registrations = RIBBON_REGISTRATIONS.lock().unwrap();
    for name in names {
        if STOCK_RIBBONS.contains(&name.as_str()) {
            continue;
        }
        if !registrations.iter().any(|n| n == name) {
            registrations.push(name.clone());
        }
    }
}

/// Rebuild every language's options atlas using all currently-registered
/// labels/ribbons/previews. Idempotent — safe to call multiple times.
/// Returns `true` if at least one language's merged texturelist is present
/// afterward (freshly rebuilt or served from cache).
///
/// Called from `lib.rs::init` exactly once, after every mod has finished
/// its `enable()`. This guarantees that every label registration is
/// captured in a single rebuild pass per language.
///
/// Per-language failure isolation: a language whose stock ARC can't be read
/// (or whose cloner pass fails) logs one WARN and is skipped — the other
/// languages still build. The registered name set is language-agnostic; each
/// language's pass sources its PNGs from its own mod folder
/// (`data_mods/custom_options/select_music_option_lang_<code>_v3_ifs/tex/`).
pub fn flush_label_atlas() -> bool {
    let label_count = LABEL_REGISTRATIONS.lock().unwrap().len();
    let preview_count = PREVIEW_REGISTRATIONS.lock().unwrap().len();
    let ribbon_count = RIBBON_REGISTRATIONS.lock().unwrap().len();

    // Record which preview PNGs actually exist on disk, so the getter can
    // return "" (→ box hidden) for values whose art wasn't shipped, instead
    // of a name that fails to bind and leaves the prior row's image up. This
    // runs every boot regardless of the atlas cache (it's cheap — just
    // is_file checks — and the in-memory set must be repopulated each run).
    // One pass against the eng folder only: availability is per texture NAME,
    // and every language ships the same file set (the generator writes all
    // families for all languages), so eng is authoritative for all three.
    {
        let eng = &OPTION_LANGS[0];
        let preview_names = PREVIEW_REGISTRATIONS.lock().unwrap().clone();
        let mut available = AVAILABLE_PREVIEWS.lock().unwrap();
        available.clear();
        for name in &preview_names {
            let png = format!("{}/{}/tex/{}.png", MOD_ROOT, eng.ifs_mod_path, name);
            if Path::new(&png).is_file() {
                available.push(name.clone());
            }
        }
    }

    let mut any_ok = false;
    for lang in &OPTION_LANGS {
        let xml = match load_stock_texturelist(lang.arc_path, lang.ifs_name) {
            Some(x) => x,
            None => {
                log_warn!(
                    "custom_options/asset_gen: can't load lang_{} texturelist for atlas flush — skipping that language",
                    lang.ifs_code
                );
                continue;
            }
        };
        if rebuild_lang_atlas(lang, &xml) {
            any_ok = true;
            log_info!(
                "custom_options/asset_gen: flushed lang_{} atlas with {} label(s) + {} preview image(s) + {} ribbon(s)",
                lang.ifs_code,
                label_count,
                preview_count,
                ribbon_count
            );
        } else {
            log_warn!(
                "custom_options/asset_gen: lang_{} atlas flush produced nothing — skipping that language",
                lang.ifs_code
            );
        }
    }
    any_ok
}

/// Rebuild one language's options atlas (tab-title + row labels +
/// value-ribbons + preview images), but only when an input changed — the
/// cache guard in `generate_cloned_atlases_cached` (keyed per
/// `ifs_mod_path`, so each language caches independently) skips the
/// expensive decode/pack/convert entirely on an unchanged boot. Returns
/// `true` if the language's merged texturelist is present afterward (whether
/// freshly rebuilt or served from cache).
fn rebuild_lang_atlas(lang: &OptionLang, xml: &str) -> bool {
    use crate::services::avs_layeredfs::atlas_cloner::{AtlasSet, BatchResult, OwnedTextureSpec};

    let label_ids = LABEL_REGISTRATIONS.lock().unwrap().clone();
    let preview_names = PREVIEW_REGISTRATIONS.lock().unwrap().clone();
    let ribbon_names = RIBBON_REGISTRATIONS.lock().unwrap().clone();

    let tex = |name: &str| format!("{}/{}/tex/{}.png", MOD_ROOT, lang.ifs_mod_path, name);

    // Base atlas set: tab-title + row labels + value-ribbon chips. Small and
    // few, so they share one cloned atlas under the base prefix.
    let mut base = AtlasSet {
        atlas_prefix: lang.atlas_prefix.to_string(),
        specs: Vec::with_capacity(label_ids.len() + ribbon_names.len() + 1),
        // Donor-slot mode: labels/ribbons clone specific stock slots
        // (seop_item_appearance / seop_op_on) and are small; cloning the
        // donor atlas footprint is cheap and keeps their UV conventions.
        fresh: false,
    };
    base.specs.push(OwnedTextureSpec {
        new_name: "seop_tab_title_mods".to_string(),
        donor_name: "seop_tab_title_basic".to_string(),
        png_path: tex("seop_tab_title_mods"),
    });
    for id in &label_ids {
        base.specs.push(OwnedTextureSpec {
            new_name: format!("seop_item_{id}"),
            donor_name: LABEL_DONOR.to_string(),
            png_path: tex(&format!("seop_item_{id}")),
        });
    }
    for name in &ribbon_names {
        base.specs.push(OwnedTextureSpec {
            new_name: name.clone(),
            donor_name: RIBBON_DONOR.to_string(),
            png_path: tex(name),
        });
    }

    let mut batch = vec![base];

    // Preview images: large and potentially hundreds across all enum options.
    // Hand the whole set to ONE fresh AtlasSet — the cloner packs them into a
    // tight new atlas and spills into additional atlases
    // (`copt_prev_<code>_000`, `_001`, …) automatically when one fills, so
    // there's no per-caller chunk size to guess and no silent overflow as
    // more options add previews.
    if !preview_names.is_empty() {
        batch.push(AtlasSet {
            atlas_prefix: lang.preview_atlas_prefix.to_string(),
            specs: preview_names
                .iter()
                .map(|name| OwnedTextureSpec {
                    new_name: name.clone(),
                    donor_name: PREVIEW_DONOR.to_string(),
                    png_path: tex(name),
                })
                .collect(),
            // Fresh-atlas mode: dozens/hundreds of self-contained previews
            // pack into tight new atlas(es) instead of cloning the crowded
            // 2048² stock atlas and ballooning to 4096² (the 20s compress).
            fresh: true,
        });
    }

    !matches!(
        crate::services::avs_layeredfs::atlas_cloner::generate_cloned_atlases_cached(
            xml,
            lang.ifs_mod_path,
            CACHE_ROOT,
            MOD_ROOT,
            &batch,
        ),
        BatchResult::Nothing
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The language table drives every per-language path and atlas namespace;
    /// these invariants are what keeps the three languages' outputs (and
    /// their atlas-cloner caches) disjoint. See the localization design:
    /// .agents/planning/2026-08-17-options-texture-localization/design/detailed-design.md
    #[test]
    fn option_langs_table_invariants() {
        assert_eq!(OPTION_LANGS.len(), 3);
        let codes: Vec<&str> = OPTION_LANGS.iter().map(|l| l.ifs_code).collect();
        assert_eq!(codes, ["eng", "jpn", "kor"]);

        let mut seen_prefixes: Vec<&str> = Vec::new();
        for lang in &OPTION_LANGS {
            let code = lang.ifs_code;
            assert_eq!(
                lang.arc_path,
                format!("data/arc/bm2d/select_music_option_lang_{code}_v3.arc")
            );
            assert_eq!(
                lang.ifs_name,
                format!("select_music_option_lang_{code}_v3.ifs")
            );
            assert_eq!(
                lang.ifs_mod_path,
                format!("select_music_option_lang_{code}_v3_ifs")
            );
            assert_eq!(lang.atlas_prefix, format!("copt_mods_lang_{code}"));
            assert_eq!(lang.preview_atlas_prefix, format!("copt_prev_{code}"));
            seen_prefixes.push(lang.atlas_prefix);
            seen_prefixes.push(lang.preview_atlas_prefix);
        }
        // Every atlas namespace pairwise distinct — no texture-name collision
        // is possible between two languages' generated atlases.
        let unique: std::collections::HashSet<&&str> = seen_prefixes.iter().collect();
        assert_eq!(unique.len(), seen_prefixes.len());
    }
}
