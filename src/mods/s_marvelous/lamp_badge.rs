//! Song-select S-MFC lamp badges (server-upload design §4.5, plan Step 8):
//! post-original detours on the two song-select refreshers that draw a
//! clear-kind lamp, re-binding the lamp bitmap to a net-new violet S-MFC
//! texture when the chart is in the side's S-MFC set ([`super::lamp`]):
//!
//! 1. the header card (`selectmusic_card_refresh`) — widget
//!    `fullcombo_<n>p_usr`, `muca_card_fc_mfc` → `muca_card_fc_smfc` (20×8);
//! 2. the difficulty panel's CLEAR RANK column
//!    (`selectmusic_difficulty_panel_refresh` = `DifficultyPanel::Reflesh`)
//!    — one lamp per displayed difficulty row, widget
//!    `difficulty<n>p_usr/dif<rr>_usr/fc_usr`, `muca_dif_fc_mfc` →
//!    `muca_dif_fc_smfc` (40×16). The row → difficulty map is the panel's
//!    own `vector<int>` at `this+0x1B8..0x1C0`;
//! 3. the ALWAYS-VISIBLE side-info table (`selectmusic_record_panel_refresh`
//!    = `RecordPanel::Refresh`, package `select_music_side`) — the
//!    DIFFICULTY / LEVEL / BEST SCORE / CLEAR RANK rows beside the wheel,
//!    widget `side_<n>p_usr/info_<n>p_usr/item_<rr>_usr/fc_usr`, rows =
//!    difficulties 0..4 directly, `musi_dif_fc_mfc` → `musi_dif_fc_smfc`
//!    (40×16). Cabinet deploys #2/#3: the jacket showed S-MFC while this
//!    column still showed MFC — the DifficultyPanel (2) is the
//!    difficulty-PICKER sub-mode and never ran while resting on a song.
//!
//! Mechanism (RE 20260825 `FUN_18015a450` / `FUN_180115ea0` /
//! `FUN_18019b9f0`): all three stock lamp blocks look the wire clear kind
//! up in a pointer table
//! (`fc_mfc`/`fc_pfc`/…), format the texture name, and run the layout's
//! bitmap setter — `find_child(layer_id, widget) → for each traversal(6)
//! sibling → load_bitmap(id, texture)`. We replicate exactly that setter
//! through the `bm2d_api` wrappers, after the original ran, only for S-MFC
//! charts. The stock clear kind is never touched; a stock lamp always
//! remains underneath.
//!
//! Fail-open everywhere: unresolved signature ⇒ no detour; unstaged texture,
//! unreadable layer, missing widget, implausible rows ⇒ stock lamp + one
//! WARN per class.
//!
//! PANIC SAFETY: the detour bodies run in `catch_unwind`; every game-memory
//! read is `memory::is_readable`-probed (no AOB pins the layer/rows fields
//! beyond the signatures' own `CMP [RCX+0xD0]` / `CMP [RCX+0xC0]`).

use std::sync::atomic::{AtomicBool, Ordering};

use once_cell::sync::OnceCell;
use retour::GenericDetour;

use crate::core::memory;
use crate::core::signatures::SignatureStore;
use crate::services::avs_layeredfs::atlas_cloner::{
    generate_cloned_atlases_cached, load_stock_texturelist, AtlasSet, BatchResult, OwnedTextureSpec,
};
use crate::services::avs_layeredfs::mod_paths;
use crate::services::{bm2d_api, scene_manager, selectmusic_highlight, stage_records};
use crate::types::scenes::scene;
use crate::{log_info, log_warn};

use super::assets::{CACHE_ROOT, MOD_ROOT};
use super::lamp;
use super::upload::chart_index;

/// `fn(this, flag: u8)` — the header-card refresh.
type CardRefreshFn = unsafe extern "C" fn(*mut u8, u8);
/// `fn(this)` — `DifficultyPanel::Reflesh` and `RecordPanel::Refresh`.
type PanelRefreshFn = unsafe extern "C" fn(*mut u8);

static DETOUR: OnceCell<GenericDetour<CardRefreshFn>> = OnceCell::new();
static PANEL_DETOUR: OnceCell<GenericDetour<PanelRefreshFn>> = OnceCell::new();
static RECORD_DETOUR: OnceCell<GenericDetour<PanelRefreshFn>> = OnceCell::new();
/// Mod enabled (activate/deactivate).
static ACTIVE: AtomicBool = AtomicBool::new(false);
/// The S-MFC textures are staged in the select_music_card package.
static TEXTURE_READY: AtomicBool = AtomicBool::new(false);

static WARN_LAYER: AtomicBool = AtomicBool::new(false);
static WARN_WIDGET: AtomicBool = AtomicBool::new(false);
static WARN_BITMAP: AtomicBool = AtomicBool::new(false);
static WARN_PANEL_LAYER: AtomicBool = AtomicBool::new(false);
static WARN_PANEL_ROWS: AtomicBool = AtomicBool::new(false);
static WARN_PANEL_WIDGET: AtomicBool = AtomicBool::new(false);
static WARN_RECORD_LAYER: AtomicBool = AtomicBool::new(false);
static WARN_RECORD_WIDGET: AtomicBool = AtomicBool::new(false);
static FIRST_SWAP_LOGGED: AtomicBool = AtomicBool::new(false);
static FIRST_PANEL_SWAP_LOGGED: AtomicBool = AtomicBool::new(false);
static FIRST_RECORD_SWAP_LOGGED: AtomicBool = AtomicBool::new(false);

/// The card object's layer-object field (the `CMP [RCX+0xD0]` the signature
/// pins) and the layer object's BM2D layer id (`*(layer+0x08)`, the field the
/// stock setter passes to `find_child`).
const CARD_LAYER_OBJ: usize = 0xD0;
const LAYER_ID: usize = 0x08;
/// The card's OWN song holder pointer (`MOV RCX,[this+0x148]; CALL
/// FUN_18010f790` — the first thing the refresh does; identical on
/// 20250805/20260224/20260825). Every visible wheel card is a separate
/// object drawing its own song — never key a card on the highlight
/// (deploy #5: cards flipped between MFC/S-MFC with the cursor).
const CARD_SONG_HOLDER: usize = 0x148;

/// DifficultyPanel fields (RE 20260825 + 20250805; `+0xC0` pinned by the
/// signatures' `CMP [RCX+0xC0]`, the rows vector read in the row loop as
/// `MOV RAX,[this+0x1C0]; SUB RAX,[this+0x1B8]; SAR RAX,2` on both): the
/// layer object, and the displayed-rows `vector<int>` (begin/end) whose
/// element `r` is the difficulty index (0 beg … 4 cha) shown on row `r`
/// (widget `dif%02d` with `r+1`).
const PANEL_LAYER_OBJ: usize = 0xC0;
const PANEL_ROWS_BEGIN: usize = 0x1B8;
const PANEL_ROWS_END: usize = 0x1C0;
/// The panel never shows more than the five difficulties.
const PANEL_MAX_ROWS: usize = 5;

/// RecordPanel fields (RE 20260825/20250805/20260224, pinned by the
/// signature's `CMP [RCX+0x118]` / `MOV EBX,[RCX+0x140]` /
/// `CMP [RCX+0x148]`): the layer object, this panel's side, and the
/// highlighted `shared_ptr<music::Info>`.
const RECORD_LAYER_OBJ: usize = 0x118;
const RECORD_SIDE: usize = 0x140;

/// PlayerWork field: style (0 single / 1 double). (`+0x54` is the COMMITTED
/// mcode, not the wheel highlight — see [`current_song`]; the difficulty
/// comes from the song holder via the game's own KIND resolution, not the
/// raw `+0x5C` cursor.)
const PW_STYLE: usize = 0x50;

/// The select_music_card package + the net-new textures.
const SELECT_MUSIC_CARD_ARC: &str = "data/arc/bm2d/select_music_card_v3.arc";
const SELECT_MUSIC_CARD_IFS: &str = "select_music_card_v3.ifs";
const SELECT_MUSIC_CARD_IFS_MOD_PATH: &str = "select_music_card_v3_ifs";
/// Atlas prefix — distinct from music_wheel_song_length's `mwsl` clone of the
/// same package (both mods contribute to one IFS through their own mod roots).
const ATLAS_PREFIX: &str = "smarv_smc";
/// Header-card lamp (20×8).
const DONOR_TEXTURE: &str = "muca_card_fc_mfc";
pub const SMFC_TEXTURE: &str = "muca_card_fc_smfc";
const SMFC_PNG: &str = "./data_mods/s_marvelous/select_music/muca_card_fc_smfc.png";
/// Difficulty-panel row lamp (40×16) — same package, own donor.
const DIF_DONOR_TEXTURE: &str = "muca_dif_fc_mfc";
pub const DIF_SMFC_TEXTURE: &str = "muca_dif_fc_smfc";
const DIF_SMFC_PNG: &str = "./data_mods/s_marvelous/select_music/muca_dif_fc_smfc.png";
/// The side-info table lives in a DIFFERENT package: select_music_side.
const SELECT_MUSIC_SIDE_ARC: &str = "data/arc/bm2d/select_music_side_v3.arc";
const SELECT_MUSIC_SIDE_IFS: &str = "select_music_side_v3.ifs";
const SELECT_MUSIC_SIDE_IFS_MOD_PATH: &str = "select_music_side_v3_ifs";
const SIDE_ATLAS_PREFIX: &str = "smarv_sms";
const SIDE_DONOR_TEXTURE: &str = "musi_dif_fc_mfc";
pub const SIDE_SMFC_TEXTURE: &str = "musi_dif_fc_smfc";
const SIDE_SMFC_PNG: &str = "./data_mods/s_marvelous/select_music/musi_dif_fc_smfc.png";

fn warn_once(latch: &AtomicBool, msg: &str) {
    if !latch.swap(true, Ordering::Relaxed) {
        log_warn!(
            "SMarvelous: lamp badge — {} (stock lamp stands; latched)",
            msg
        );
    }
}

// ── Install (mod init) ───────────────────────────────────────────────

/// Resolve the two refreshers and install the post-original detours. The
/// card lamp is the gate (`false` ⇒ badge inert, stock lamps everywhere);
/// the difficulty-panel lamps are best-effort on top of it. The upload half
/// is independent of both.
pub fn install(signatures: &SignatureStore) -> bool {
    if !selectmusic_highlight::is_available() {
        log_warn!("SMarvelous: selectmusic_highlight unavailable -- S-MFC lamp badge inert");
        return false;
    }
    let Some(target) = signatures.get_address("selectmusic_card_refresh") else {
        log_warn!("SMarvelous: selectmusic_card_refresh unresolved -- S-MFC lamp badge inert");
        return false;
    };
    let target: CardRefreshFn = unsafe { std::mem::transmute(target) };
    match unsafe { GenericDetour::new(target, card_refresh_hook) } {
        Ok(detour) => {
            if unsafe { detour.enable() }.is_err() {
                log_warn!(
                    "SMarvelous: card-refresh detour enable failed -- S-MFC lamp badge inert"
                );
                return false;
            }
            let _ = DETOUR.set(detour);
            log_info!("SMarvelous: song-select card-refresh detour installed (S-MFC lamp)");
        }
        Err(e) => {
            log_warn!(
                "SMarvelous: card-refresh detour failed: {:?} -- S-MFC lamp badge inert",
                e
            );
            return false;
        }
    }

    if let Some(panel) = signatures.get_address("selectmusic_difficulty_panel_refresh") {
        let panel: PanelRefreshFn = unsafe { std::mem::transmute(panel) };
        match unsafe { GenericDetour::new(panel, panel_refresh_hook) } {
            Ok(detour) if unsafe { detour.enable() }.is_ok() => {
                let _ = PANEL_DETOUR.set(detour);
                log_info!(
                    "SMarvelous: song-select difficulty-panel detour installed (S-MFC row lamps)"
                );
            }
            Ok(_) => log_warn!(
                "SMarvelous: difficulty-panel detour enable failed -- CLEAR RANK lamps stay stock"
            ),
            Err(e) => log_warn!(
                "SMarvelous: difficulty-panel detour failed: {:?} -- CLEAR RANK lamps stay stock",
                e
            ),
        }
    } else {
        log_warn!(
            "SMarvelous: selectmusic_difficulty_panel_refresh unresolved -- CLEAR RANK lamps stay stock"
        );
    }

    if let Some(record) = signatures.get_address("selectmusic_record_panel_refresh") {
        let record: PanelRefreshFn = unsafe { std::mem::transmute(record) };
        match unsafe { GenericDetour::new(record, record_refresh_hook) } {
            Ok(detour) if unsafe { detour.enable() }.is_ok() => {
                let _ = RECORD_DETOUR.set(detour);
                log_info!(
                    "SMarvelous: song-select record-panel detour installed (S-MFC side-info lamps)"
                );
            }
            Ok(_) => log_warn!(
                "SMarvelous: record-panel detour enable failed -- side-info CLEAR RANK lamps stay stock"
            ),
            Err(e) => log_warn!(
                "SMarvelous: record-panel detour failed: {:?} -- side-info CLEAR RANK lamps stay stock",
                e
            ),
        }
    } else {
        log_warn!(
            "SMarvelous: selectmusic_record_panel_refresh unresolved -- side-info CLEAR RANK lamps stay stock"
        );
    }
    true
}

// ── Activate / deactivate (mod enable/disable) ───────────────────────

/// Stage the violet lamp textures (FRESH atlas clones of the MFC donors) and
/// arm the detours. Called from enable only when [`install`] succeeded.
pub fn activate() {
    TEXTURE_READY.store(stage_textures(), Ordering::Release);
    ACTIVE.store(true, Ordering::Release);
}

/// Disarm (the detours stay installed as passthroughs).
pub fn deactivate() {
    ACTIVE.store(false, Ordering::Release);
}

/// FRESH-mode injection of the S-MFC lamps into their packages
/// (select_music_card: card + difficulty-panel lamps; select_music_side: the
/// side-info table lamp). Each donor supplies its cell geometry; the PNGs are
/// violet recolors of those donors. Cache-guarded like the combo digits;
/// fail-open `false` (all-or-nothing per package; both must stage).
fn stage_textures() -> bool {
    let card = stage_package(
        SELECT_MUSIC_CARD_ARC,
        SELECT_MUSIC_CARD_IFS,
        SELECT_MUSIC_CARD_IFS_MOD_PATH,
        ATLAS_PREFIX,
        &[
            (SMFC_TEXTURE, DONOR_TEXTURE, SMFC_PNG),
            (DIF_SMFC_TEXTURE, DIF_DONOR_TEXTURE, DIF_SMFC_PNG),
        ],
    );
    let side = stage_package(
        SELECT_MUSIC_SIDE_ARC,
        SELECT_MUSIC_SIDE_IFS,
        SELECT_MUSIC_SIDE_IFS_MOD_PATH,
        SIDE_ATLAS_PREFIX,
        &[(SIDE_SMFC_TEXTURE, SIDE_DONOR_TEXTURE, SIDE_SMFC_PNG)],
    );
    card && side
}

/// One package's FRESH batch: `(new_name, donor, png)` triples.
fn stage_package(
    arc: &str,
    ifs: &str,
    ifs_mod_path: &str,
    atlas_prefix: &str,
    pairs: &[(&str, &str, &str)],
) -> bool {
    // Per-image serving copies beside the merged texturelist (the package
    // family serves one file per image — the combo digits' deploy lesson).
    let tex_dir = format!("{}/{}/tex", MOD_ROOT, ifs_mod_path);
    if let Err(e) = std::fs::create_dir_all(&tex_dir) {
        log_warn!(
            "SMarvelous: mkdir {}: {} -- S-MFC lamp textures unstaged",
            tex_dir,
            e
        );
        return false;
    }
    let mut specs = Vec::with_capacity(pairs.len());
    for &(name, donor, png) in pairs {
        if !std::path::Path::new(png).exists() {
            log_warn!(
                "SMarvelous: {} missing -- S-MFC lamp textures unstaged",
                png
            );
            return false;
        }
        let dst = format!("{}/{}.png", tex_dir, name);
        if let Err(e) = std::fs::copy(png, &dst) {
            log_warn!(
                "SMarvelous: can't stage {}: {} -- S-MFC lamp textures unstaged",
                dst,
                e
            );
            return false;
        }
        specs.push(OwnedTextureSpec {
            new_name: name.to_string(),
            donor_name: donor.to_string(),
            png_path: png.to_string(),
        });
    }
    let Some(texlist) = load_stock_texturelist(arc, ifs) else {
        log_warn!(
            "SMarvelous: stock {} texturelist unavailable -- S-MFC lamp textures unstaged",
            ifs
        );
        return false;
    };
    let names: Vec<&str> = pairs.iter().map(|p| p.0).collect();
    let batch = [AtlasSet {
        atlas_prefix: atlas_prefix.to_string(),
        specs,
        fresh: true,
    }];
    match generate_cloned_atlases_cached(&texlist, ifs_mod_path, CACHE_ROOT, MOD_ROOT, &batch) {
        BatchResult::Nothing => {
            log_warn!(
                "SMarvelous: S-MFC lamp atlas injection into {} produced nothing -- unstaged",
                ifs
            );
            false
        }
        BatchResult::Cached | BatchResult::Rebuilt => {
            let merged_rel = format!("{}/tex/texturelist.merged.xml", ifs_mod_path);
            let probe_rel = format!("{}/tex/{}.png", ifs_mod_path, names[names.len() - 1]);
            if mod_paths::find_first_modfile(&merged_rel).is_none()
                || mod_paths::find_first_modfile(&probe_rel).is_none()
            {
                log_info!(
                    "SMarvelous: S-MFC lamp assets ({}) not in mod-path cache -- rescanning",
                    ifs
                );
                mod_paths::init_mod_paths();
            }
            log_info!(
                "SMarvelous: S-MFC lamp textures staged in {} ({}; fresh atlas)",
                ifs,
                names.join(", ")
            );
            true
        }
    }
}

// ── Shared pieces ────────────────────────────────────────────────────

/// The stock setter's shape: `load_bitmap` on the found child and every
/// traversal-6 sibling of the same name. Returns the number bound.
fn rebind_lamp(first: u32, texture: &str) -> u32 {
    let mut id = Some(first);
    let mut bound = 0u32;
    while let Some(cur) = id {
        if bm2d_api::mc_load_bitmap(cur, texture) {
            bound += 1;
        } else {
            warn_once(&WARN_BITMAP, "load_bitmap(S-MFC lamp texture) refused");
        }
        id = bm2d_api::mc_traversal(cur, 6);
    }
    bound
}

/// `*(obj + layer_field) → *(layer + 0x08)`: the BM2D layer id a refresher
/// draws into, or `None` (with the given latch WARNed) when unreadable.
/// `Some(None)` = readable but no live layer this frame.
fn layer_id_of(obj: *mut u8, layer_field: usize, latch: &AtomicBool, what: &str) -> Option<i32> {
    if obj.is_null() || !memory::is_readable(unsafe { obj.add(layer_field) }, 8) {
        warn_once(latch, &format!("{what} object unreadable"));
        return None;
    }
    let layer_obj = unsafe { memory::read_ptr(obj.add(layer_field)) };
    if layer_obj.is_null() {
        return None; // the originals early-out on this too
    }
    if !memory::is_readable(unsafe { layer_obj.add(LAYER_ID) }, 4) {
        warn_once(latch, &format!("{what} layer object unreadable"));
        return None;
    }
    let id = unsafe { memory::read_i32(layer_obj.add(LAYER_ID)) };
    (id >= 0).then_some(id)
}

/// The side's style (0 single / 1 double) from its PlayerWork.
fn side_style(side: usize) -> Option<i32> {
    let pw = stage_records::player_work(side)? as *const u8;
    if !memory::is_readable(unsafe { pw.add(PW_STYLE) }, 4) {
        return None;
    }
    let style = unsafe { memory::read_i32(pw.add(PW_STYLE)) };
    (0..=1).contains(&style).then_some(style)
}

/// The wheel's HIGHLIGHTED song (mcode from the select-music model — what
/// the side-info table and the difficulty picker draw; `PlayerWork+0x54`
/// is the COMMITTED song and lags the cursor, deploys #1–#4) + the side's
/// style.
fn current_song(side: usize) -> Option<(i32, i32)> {
    let mcode = selectmusic_highlight::highlighted_mcode()?;
    Some((mcode, side_style(side)?))
}

// ── Header-card detour ───────────────────────────────────────────────

unsafe extern "C" fn card_refresh_hook(this: *mut u8, flag: u8) {
    if let Some(detour) = DETOUR.get() {
        detour.call(this, flag);
    }
    if !ACTIVE.load(Ordering::Acquire) || !TEXTURE_READY.load(Ordering::Acquire) {
        return;
    }
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| after_refresh(this)));
}

/// Post-original: this card draws ITS OWN song (`this+0x148`); for each
/// entered side, resolve that song's row for the side's KIND cursor exactly
/// like the game (`holder_difficulty`) and re-bind the side's lamp widget
/// when the chart is an S-MFC.
fn after_refresh(this: *mut u8) {
    if scene_manager::current_scene() != scene::SONG_SELECT {
        return;
    }
    let Some(layer_id) = layer_id_of(this, CARD_LAYER_OBJ, &WARN_LAYER, "card") else {
        return;
    };
    if !memory::is_readable(unsafe { this.add(CARD_SONG_HOLDER) }, 8) {
        warn_once(&WARN_LAYER, "card song holder unreadable");
        return;
    }
    let holder = unsafe { memory::read_ptr(this.add(CARD_SONG_HOLDER)) };
    if holder.is_null() {
        return; // empty wheel slot
    }
    let Some(mcode) = selectmusic_highlight::mcode_of_holder(holder) else {
        return;
    };
    for side in 0..2usize {
        if lamp::count(side) == 0 || stage_records::side_entered(side) != Some(true) {
            continue;
        }
        let (Some(style), Some(kind)) = (side_style(side), selectmusic_highlight::side_kind(side))
        else {
            continue;
        };
        let Some(difficulty) = selectmusic_highlight::holder_difficulty(holder, kind) else {
            continue;
        };
        let chart = chart_index(style, difficulty);
        if !lamp::is_smfc(side, mcode, chart) {
            continue;
        }
        let widget = format!("fullcombo_{}p_usr", side + 1);
        let Some(first) = bm2d_api::layer_find_child(layer_id as u32, &widget) else {
            warn_once(&WARN_WIDGET, "lamp widget not found on the card");
            continue;
        };
        let bound = rebind_lamp(first, SMFC_TEXTURE);
        if bound > 0 && !FIRST_SWAP_LOGGED.swap(true, Ordering::Relaxed) {
            log_info!(
                "SMarvelous: S-MFC lamp shown (side {}, mcode {}, chart {}, {} layer(s))",
                side,
                mcode,
                chart,
                bound
            );
        }
    }
}

// ── DifficultyPanel::Reflesh detour ──────────────────────────────────

unsafe extern "C" fn panel_refresh_hook(this: *mut u8) {
    if let Some(detour) = PANEL_DETOUR.get() {
        detour.call(this);
    }
    if !ACTIVE.load(Ordering::Acquire) || !TEXTURE_READY.load(Ordering::Acquire) {
        return;
    }
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| after_panel_refresh(this)));
}

/// Post-original: for each entered side with S-MFCs, walk the panel's
/// displayed rows and re-bind the CLEAR RANK lamp of every row whose
/// `(highlighted mcode, style, rows[r])` chart is an S-MFC.
fn after_panel_refresh(this: *mut u8) {
    if scene_manager::current_scene() != scene::SONG_SELECT {
        return;
    }
    let Some(layer_id) = layer_id_of(this, PANEL_LAYER_OBJ, &WARN_PANEL_LAYER, "difficulty panel")
    else {
        return;
    };
    let Some(rows) = panel_rows(this) else {
        warn_once(&WARN_PANEL_ROWS, "difficulty panel rows vector implausible");
        return;
    };
    for side in 0..2usize {
        if lamp::count(side) == 0 || stage_records::side_entered(side) != Some(true) {
            continue;
        }
        let Some((mcode, style)) = current_song(side) else {
            continue;
        };
        for (r, &difficulty) in rows.iter().enumerate() {
            let chart = chart_index(style, difficulty);
            if !lamp::is_smfc(side, mcode, chart) {
                continue;
            }
            let widget = format!("difficulty{}p_usr/dif{:02}_usr/fc_usr", side + 1, r + 1);
            let Some(first) = bm2d_api::layer_find_child(layer_id as u32, &widget) else {
                warn_once(&WARN_PANEL_WIDGET, "row lamp widget not found on the panel");
                continue;
            };
            let bound = rebind_lamp(first, DIF_SMFC_TEXTURE);
            if bound > 0 && !FIRST_PANEL_SWAP_LOGGED.swap(true, Ordering::Relaxed) {
                log_info!(
                    "SMarvelous: S-MFC row lamp shown (side {}, mcode {}, chart {}, row {}, {} layer(s))",
                    side,
                    mcode,
                    chart,
                    r + 1,
                    bound
                );
            }
        }
    }
}

// ── RecordPanel::Refresh detour (the visible side-info table) ────────

unsafe extern "C" fn record_refresh_hook(this: *mut u8) {
    if let Some(detour) = RECORD_DETOUR.get() {
        detour.call(this);
    }
    if !ACTIVE.load(Ordering::Acquire) || !TEXTURE_READY.load(Ordering::Acquire) {
        return;
    }
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| after_record_refresh(this)));
}

/// Post-original: this panel is one side's table; for each difficulty row
/// 0..4 whose `(highlighted mcode, style, row)` chart is an S-MFC, re-bind
/// `side_<n>p_usr/info_<n>p_usr/item_<rr>_usr/fc_usr`.
fn after_record_refresh(this: *mut u8) {
    if scene_manager::current_scene() != scene::SONG_SELECT {
        return;
    }
    let Some(layer_id) = layer_id_of(this, RECORD_LAYER_OBJ, &WARN_RECORD_LAYER, "record panel")
    else {
        return;
    };
    if !memory::is_readable(unsafe { this.add(RECORD_SIDE) }, 4) {
        warn_once(&WARN_RECORD_LAYER, "record panel side unreadable");
        return;
    }
    let side = unsafe { memory::read_i32(this.add(RECORD_SIDE)) };
    if !(0..=1).contains(&side) {
        return;
    }
    let side = side as usize;
    if lamp::count(side) == 0 || stage_records::side_entered(side) != Some(true) {
        return;
    }
    let Some((mcode, style)) = current_song(side) else {
        return;
    };
    for row in 0..PANEL_MAX_ROWS {
        let chart = chart_index(style, row as i32);
        if !lamp::is_smfc(side, mcode, chart) {
            continue;
        }
        let widget = format!(
            "side_{n}p_usr/info_{n}p_usr/item_{:02}_usr/fc_usr",
            row + 1,
            n = side + 1
        );
        let Some(first) = bm2d_api::layer_find_child(layer_id as u32, &widget) else {
            warn_once(
                &WARN_RECORD_WIDGET,
                "row lamp widget not found on the record panel",
            );
            continue;
        };
        let bound = rebind_lamp(first, SIDE_SMFC_TEXTURE);
        if bound > 0 && !FIRST_RECORD_SWAP_LOGGED.swap(true, Ordering::Relaxed) {
            log_info!(
                "SMarvelous: S-MFC side-info lamp shown (side {}, mcode {}, chart {}, row {}, {} layer(s))",
                side,
                mcode,
                chart,
                row + 1,
                bound
            );
        }
    }
}

/// The panel's displayed rows (`vector<int>` at `this+0x1B8..0x1C0`), each a
/// difficulty index 0..=4; `None` on any structural surprise (empty is
/// `Some(vec![])`).
fn panel_rows(this: *mut u8) -> Option<Vec<i32>> {
    if !memory::is_readable(unsafe { this.add(PANEL_ROWS_BEGIN) }, 16) {
        return None;
    }
    let begin = unsafe { memory::read_ptr(this.add(PANEL_ROWS_BEGIN)) };
    let end = unsafe { memory::read_ptr(this.add(PANEL_ROWS_END)) };
    if begin.is_null() && end.is_null() {
        return Some(Vec::new());
    }
    if begin.is_null() || end.is_null() || (end as usize) < (begin as usize) {
        return None;
    }
    let bytes = end as usize - begin as usize;
    if bytes % 4 != 0 || bytes / 4 > PANEL_MAX_ROWS {
        return None;
    }
    if bytes > 0 && !memory::is_readable(begin, bytes) {
        return None;
    }
    let mut rows = Vec::with_capacity(bytes / 4);
    for i in 0..bytes / 4 {
        let d = unsafe { memory::read_i32(begin.add(i * 4)) };
        if !(0..=4).contains(&d) {
            return None;
        }
        rows.push(d);
    }
    Some(rows)
}
