//! Legacy element positions — A3's layout-root markers over World's.
//!
//! World's gameplay `LayoutActor` fills its marker maps from World's own
//! layout root once per song (`hud_layout_builder_entry`, after every package
//! on its load list is resident); every HUD actor positions itself from those
//! keys at its init. This module is the builder's POST subscriber
//! (`services::hud_layout_hooks`): on a legacy song it opens the skin's own
//! root (`dance_common000N` / skin 1 `dance_common0000_v2`, pushed onto the
//! `LayoutActor` load list by the package helper — the actor loads and
//! releases it like its own), reads A3's markers exactly the way World's
//! builder reads World's (`afp_layer_mc_refer`, `afp_mc_get_param` 0x1008 /
//! 0x1015 / 0x1016 / 0x100d, the two lane-content loads) and overwrites the
//! keys whose element is legacy-drawn or root-identical ([`marker_keys`])
//! through `hud_layout_hooks::set_marker` — so center-arrows' lane shift
//! applies after us. World-only elements (BPM display, player name) are
//! parked off screen instead ([`marker_keys::HIDDEN_KEYS`]).
//!
//! The transient root layer is created, read and destroyed inside the
//! builder call (World does the same with its own root). Game thread only.
//! RE: `.agents/planning/2026-09-22-ddr-selection/research/
//! hud-layout-stage-frame.md`.

use std::ffi::CString;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

use crate::core::memory;
use crate::core::signatures::HudLayoutSideSites;
use crate::services::{bm2d_api, bm2d_package, hud_layout_hooks, stage_records};
use crate::{log_info, log_warn};

use super::marker_keys::{self, Map, Source};

const PARAM_POSITION: i32 = 0x1008;
const PARAM_WIDTH: i32 = 0x1015;
const PARAM_HEIGHT: i32 = 0x1016;
const PARAM_SCALE: i32 = 0x100d;
/// The standard post-create layer attribute World's `CMovieClip::Create` sets.
const ATTR_DISPLAY_SETUP: u32 = 0x200;
/// Per-side marker parent stride (pinned by the helper's record-map math).
const SIDE_STRIDE: usize = 0x48;

#[derive(Clone, Copy)]
struct Offsets {
    side: HudLayoutSideSites,
    shared_parent_off: usize,
    side_parent_off: usize,
}

static OFFSETS: OnceLock<Offsets> = OnceLock::new();
static CAPABLE: AtomicBool = AtomicBool::new(false);
/// One WARN per class per session.
static WARNED: AtomicU32 = AtomicU32::new(0);

/// The pending post-pass for one `LayoutActor` (set by the package helper
/// when it requests `dance_common` on an armed song).
#[derive(Clone)]
struct Pending {
    actor: usize,
    skin: u8,
    /// The legacy root pushed onto the actor's load list (None: probe miss —
    /// only the World-only hides run).
    root: Option<&'static str>,
}

static PENDING: Mutex<Option<Pending>> = Mutex::new(None);

fn warn_once(bit: u32, msg: std::fmt::Arguments) {
    if WARNED.fetch_or(bit, Ordering::Relaxed) & bit == 0 {
        log_warn!("DDR SELECTION: {}", msg);
    }
}

/// Resolve offsets (mod init). `shared_parent_off` / `side_parent_off` are
/// the package helper's record-map parents — the same objects the marker
/// setter receives.
pub fn init(shared_parent_off: usize, side_parent_off: usize) -> bool {
    let Some(side) = hud_layout_hooks::side_sites() else {
        log_warn!("DDR SELECTION: HUD layout side offsets unresolved -- legacy element positions unavailable");
        return false;
    };
    let _ = OFFSETS.set(Offsets {
        side,
        shared_parent_off,
        side_parent_off,
    });
    true
}

/// Install the shared layout hooks and subscribe (mod enable).
pub fn start() -> bool {
    if CAPABLE.load(Ordering::Acquire) {
        return true;
    }
    if OFFSETS.get().is_none()
        || !bm2d_api::afp_layers_available()
        || !bm2d_api::mc_load_movie_available()
        || !bm2d_package::is_available()
    {
        log_warn!("DDR SELECTION: AFP layer / package API unavailable -- legacy element positions unavailable");
        return false;
    }
    if !hud_layout_hooks::acquire() {
        log_warn!(
            "DDR SELECTION: HUD layout hooks unavailable -- legacy element positions unavailable"
        );
        return false;
    }
    hud_layout_hooks::subscribe_builder_post(on_builder_post);
    CAPABLE.store(true, Ordering::Release);
    log_info!("DDR SELECTION: legacy element positions ready (layout builder post-pass)");
    true
}

/// Whether the post-pass runs on this boot (the `Markers` adapter).
pub fn capable() -> bool {
    CAPABLE.load(Ordering::Acquire)
}

/// Whether the skin's legacy root arc exists (LayeredFS-aware game probe).
pub fn root_available(skin: u8) -> bool {
    let Some(name) = marker_keys::root_name(skin) else {
        return false;
    };
    let (Ok(dir), Ok(n)) = (CString::new("bm2d"), CString::new(name)) else {
        return false;
    };
    super::package_helper::probe_arc(&dir, &n)
}

/// The package helper saw `dance_common` for `actor` on an armed song:
/// returns the root to push onto the actor's load list (or None) and arms
/// the post-pass for that actor.
pub fn on_common_request(actor: *mut u8, skin: u8) -> Option<&'static str> {
    if !capable() {
        return None;
    }
    let root = marker_keys::root_name(skin).filter(|_| root_available(skin));
    if root.is_none() {
        warn_once(
            1,
            format_args!(
                "no legacy layout root for skin {} ({:?}) -- World's element positions",
                skin,
                marker_keys::root_name(skin)
            ),
        );
    }
    *PENDING.lock().unwrap_or_else(|e| e.into_inner()) = Some(Pending {
        actor: actor as usize,
        skin,
        root,
    });
    root
}

/// Forget a pending post-pass (disarm).
pub fn reset() {
    *PENDING.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// The side's Option `judge_position` (World's vslot), 0 when unreadable.
fn judge_position(side: u8, vslot: usize) -> i32 {
    let Some(opt_off) = stage_records::player_option_offset() else {
        return 0;
    };
    let Some(pw) = stage_records::player_work(side as usize) else {
        return 0;
    };
    unsafe {
        let opt = pw.add(opt_off);
        if !memory::is_readable(opt, 8) {
            return 0;
        }
        let vt = memory::read_ptr(opt);
        if !memory::is_readable(vt.add(vslot), 8) {
            return 0;
        }
        let f = memory::read_ptr(vt.add(vslot));
        if !memory::is_readable(f, 16) {
            return 0;
        }
        let getter: unsafe extern "C" fn(*const u8) -> i32 = std::mem::transmute(f);
        getter(opt)
    }
}

/// World's four reads of one marker. `None` when the marker is missing or
/// its position read fails.
fn read_marker(layer: u32, path: &str) -> Option<[i32; 6]> {
    let mc = bm2d_api::layer_find_child(layer, path)?;
    let pos = bm2d_api::mc_get_vec2(mc, PARAM_POSITION)?;
    let w = bm2d_api::mc_get_param(mc, PARAM_WIDTH).unwrap_or(0);
    let h = bm2d_api::mc_get_param(mc, PARAM_HEIGHT).unwrap_or(0);
    let scale = bm2d_api::mc_get_vec2(mc, PARAM_SCALE).unwrap_or((1.0, 1.0));
    Some(marker_keys::coord_from(pos, w, h, scale))
}

#[derive(Default)]
struct Tally {
    moved: Vec<String>,
    kept: Vec<String>,
    missing: Vec<String>,
}

fn write(parent: *mut u8, key: &str, coord: [i32; 6]) -> bool {
    match CString::new(key) {
        Ok(k) => hud_layout_hooks::set_marker(parent, &k, coord),
        Err(_) => false,
    }
}

/// One key: read from the legacy root, gate, write.
fn apply_key(
    layer: u32,
    parent: *mut u8,
    spec: &marker_keys::KeySpec,
    path: &str,
    reverse_for_arrow: bool,
    tally: &mut Tally,
    tag: &str,
) {
    if !marker_keys::gate_open(spec.gate, super::legacy_package) {
        tally.kept.push(format!("{}{}", tag, spec.key));
        return;
    }
    let Some(coord) = read_marker(layer, path) else {
        tally
            .missing
            .push(format!("{}{} ({})", tag, spec.key, path));
        return;
    };
    if write(parent, spec.key, coord) {
        tally.moved.push(format!("{}{}", tag, spec.key));
    }
    if spec.key == "arrow_raw" {
        write(
            parent,
            marker_keys::ARROW_KEY,
            marker_keys::arrow_coord(coord, reverse_for_arrow),
        );
    }
}

/// The post-pass over one opened legacy root.
fn apply_root(actor: *mut u8, o: &Offsets, layer: u32, pkg_id: u32, tally: &mut Tally) {
    let shared = unsafe { actor.add(o.shared_parent_off) };
    for spec in marker_keys::KEYS.iter().filter(|s| s.map == Map::Shared) {
        if let Source::Root(_) = spec.source {
            let path = marker_keys::marker_path(spec.source, 0, false, false);
            apply_key(layer, shared, spec, &path, false, tally, "");
        }
    }
    for side in 0..2u8 {
        let style = unsafe { memory::read_i32(actor.add(o.side.style_off + side as usize * 4)) };
        if style == 2 {
            continue;
        }
        let double = style != 0;
        let reverse = unsafe {
            memory::read_u8(actor.add(o.side.reverse_off + side as usize * SIDE_STRIDE)) != 0
        };
        let parent = unsafe { actor.add(o.side_parent_off + side as usize * SIDE_STRIDE) };
        let tag = format!("{}p:", side + 1);
        let judge_rev =
            marker_keys::judge_group_reverse(judge_position(side, o.side.judge_pos_vslot), reverse);
        let lane_path = marker_keys::lane_name(side, double);
        let lane = bm2d_api::layer_find_child(layer, &lane_path);

        // Root markers + the lane MC itself (read before any lane load).
        for spec in marker_keys::KEYS
            .iter()
            .filter(|s| s.map == Map::Side && matches!(s.source, Source::Root(_) | Source::Lane))
        {
            let path = marker_keys::marker_path(spec.source, side, double, reverse);
            apply_key(layer, parent, spec, &path, reverse, tally, &tag);
        }
        // The two lane groups, each after World's lane-content load.
        for (judge_group, variant_reverse) in [(true, judge_rev), (false, reverse)] {
            let loaded = lane.is_some_and(|mc| {
                bm2d_api::mc_load_movie(
                    mc,
                    pkg_id,
                    &marker_keys::lane_variant(double, variant_reverse),
                )
            });
            for spec in marker_keys::KEYS.iter().filter(|s| match s.source {
                Source::JudgeGroup(_) => judge_group,
                Source::ArrowGroup(_) => !judge_group,
                _ => false,
            }) {
                if !loaded && marker_keys::gate_open(spec.gate, super::legacy_package) {
                    tally.missing.push(format!(
                        "{}{} ({} not loadable)",
                        tag,
                        spec.key,
                        marker_keys::lane_variant(double, variant_reverse)
                    ));
                    continue;
                }
                if !loaded {
                    tally.kept.push(format!("{}{}", tag, spec.key));
                    continue;
                }
                let path = marker_keys::marker_path(spec.source, side, double, reverse);
                apply_key(layer, parent, spec, &path, reverse, tally, &tag);
            }
        }
        log_info!(
            "DDR SELECTION: legacy markers {} {} lane, reverse {}, judge-group lane {}",
            tag,
            if double { "double" } else { "single" },
            if reverse { "yes" } else { "no" },
            if judge_rev { "reverse" } else { "normal" }
        );
    }
}

/// Park the World-only elements off screen for every laid-out side.
fn hide_world_only(actor: *mut u8, o: &Offsets) -> Vec<String> {
    let mut hidden = Vec::new();
    for side in 0..2u8 {
        let style = unsafe { memory::read_i32(actor.add(o.side.style_off + side as usize * 4)) };
        if style == 2 {
            continue;
        }
        let parent = unsafe { actor.add(o.side_parent_off + side as usize * SIDE_STRIDE) };
        for key in marker_keys::HIDDEN_KEYS {
            if write(parent, key, marker_keys::HIDDEN_COORD) {
                hidden.push(format!("{}p:{}", side + 1, key));
            }
        }
    }
    hidden
}

/// `hud_layout_hooks` builder POST subscriber.
fn on_builder_post(actor: *mut u8) {
    if !capable() || super::armed_skin() == 0 || actor.is_null() {
        return;
    }
    let Some(o) = OFFSETS.get() else {
        return;
    };
    let pending = PENDING.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let Some(p) = pending.filter(|p| p.actor == actor as usize) else {
        warn_once(
            2,
            format_args!("layout builder ran for a LayoutActor the package helper never armed -- World's element positions"),
        );
        return;
    };
    if !memory::is_readable(actor, o.side.reverse_off + 2 * SIDE_STRIDE) {
        return;
    }
    let mut tally = Tally::default();
    let mut root_state = "no legacy root".to_string();
    if let Some(root) = p.root {
        let found = CString::new(root)
            .ok()
            .and_then(|c| bm2d_package::lookup_unowned(&c));
        match found.map(|pkg| {
            (
                pkg,
                bm2d_api::create_layer_from_package(pkg.afpu_package_id, "dance_root"),
            )
        }) {
            Some((pkg, Some(layer))) => {
                bm2d_api::layer_set_attribute(&layer, ATTR_DISPLAY_SETUP, ATTR_DISPLAY_SETUP);
                apply_root(actor, o, layer.id(), pkg.afpu_package_id, &mut tally);
                bm2d_api::layer_set_visible(&layer, false);
                if !bm2d_api::destroy_layer(layer) {
                    warn_once(
                        4,
                        format_args!("could not destroy the transient {} layer", root),
                    );
                }
                root_state = root.to_string();
            }
            Some((_, None)) => warn_once(
                8,
                format_args!(
                    "{} has no dance_root export -- World's element positions",
                    root
                ),
            ),
            None => warn_once(
                16,
                format_args!(
                    "{} is not resident at the layout build -- World's element positions",
                    root
                ),
            ),
        }
    }
    let hidden = hide_world_only(actor, o);
    log_info!(
        "DDR SELECTION: legacy element positions (skin {}, {}) -- moved [{}]; World's (package stock) [{}]; missing [{}]; hidden [{}]",
        p.skin,
        root_state,
        tally.moved.join(", "),
        tally.kept.join(", "),
        tally.missing.join(", "),
        hidden.join(", ")
    );
}
