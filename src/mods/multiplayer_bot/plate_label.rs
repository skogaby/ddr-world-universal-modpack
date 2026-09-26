//! The secondary `TARGET BOT` label over a NAMED Target Score replay's name
//! plate — gameplay HUD and stage results (engine-facing; glyph tables and
//! placement math are pure in `session.rs`).
//!
//! The plate itself carries the target's own name (`PlayerWork+0xC` is an
//! inline 8-character buffer read by ~10 inline game routines, so there is
//! no room for a marker), and a screenshot must still not read as that
//! player's real play. So a second, smaller row of the plate's OWN glyphs
//! sits just above it:
//!
//! * **Gameplay** — the bot side's `sequence::dance::ScoreActor`
//!   (`score_actor_vtable`, found by a bounded walk of the live DPS's actor
//!   tree, side = `**(actor + side_off)`) holds its `dance_name` clip at
//!   `name_clip_off` (both decoded by `derive_ddr_sel_score`); the stock
//!   name is a `sequence::SpriteLayer` anchored on that clip's `name_usr`
//!   in the `cote_edge_*` glyphs, fit to the anchor, aligned left/top.
//! * **Results** — the `ResultSequence` main clip (found by content: it
//!   holds both `player_Np_info_usr/profile_usr/player_name_usr` anchors)
//!   carries each side's name as a SpriteLayer on that path in the
//!   `cote_shadow_*` glyphs, fit, centred.
//!
//! For each surface this module owns ONE process-lifetime SpriteLayer (the
//! `music_wheel_song_length` pattern: our allocation, the game's ctor, the
//! game's SetBitmaps, per-frame layout through the object's own vtable
//! slot 0) anchored on the same child with a fixed scale of
//! [`LABEL_RATIO`] × the anchor's live scaled height, top-aligned
//! [`LABEL_GAP`] px above it (re-measured every frame, so the label follows
//! the anchor's intro animation and fades with its alpha like the plate).
//! The glyph sets have no parentheses, so the text is `TARGET BOT`.
//!
//! Lifecycle: `impersonation` arms the bot side when the plate got a real
//! name and disarms at restore. An `input_manager::on_frame` callback (game
//! thread) binds the gameplay label in GAMEPLAY and the results label in
//! RESULTS_DETAIL (bind attempts throttled while unbound), re-validates the
//! bound parent every frame (pool slots recycle — a stale slot is static
//! memory, and one that no longer resolves the anchor unbinds us), and
//! blanks (empty names = CBitmaps back to the pool) the moment the surface,
//! the scene or the arm goes away — blank FIRST while the parent field
//! still points at the (static) slot, then clear it: layout dereferences
//! the parent with no null check, so it never runs unbound.
//!
//! Fail-open: missing SpriteLayer signatures ⇒ no label anywhere; missing
//! ScoreActor sites ⇒ no gameplay label (one WARN each). Idle cost: two
//! atomic loads per frame. The frame callback stays registered across a
//! mod disable (the disable disarms; the next frame blanks on the game
//! thread — SetBitmaps is game-thread-only).

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::Mutex;

use super::session::{self, LABEL_GLYPHS_GAMEPLAY, LABEL_GLYPHS_RESULTS};
use crate::core::memory;
use crate::core::msvc::{MsvcString, MsvcVec};
use crate::core::signatures::SignatureStore;
use crate::services::{bm2d_api, scene_manager, song_reset};
use crate::types::scenes::scene;
use crate::{log_info, log_warn};

type CtorFn = unsafe extern "C" fn(*mut u8) -> *mut u8;
type SetNamesFn = unsafe extern "C" fn(*mut u8, *const MsvcVec<MsvcString>) -> *mut u8;
type LayoutFn = unsafe extern "C" fn(*mut u8);

// ── sequence::SpriteLayer (ctor-attested; music_wheel research §3) ──────
const SL_SIZE: usize = 0xF8;
const SL_BITMAPS_BEGIN: usize = 0x08;
const SL_BITMAPS_END: usize = 0x10;
/// Bitmap record `{CBitmap*, w f64, h f64, pad}`, stride 0x20.
const SL_BITMAP_H: usize = 0x10;
const SL_PARENT: usize = 0x60;
const SL_ANCHOR_NAME: usize = 0x68;
const SL_PRIORITY: usize = 0x94;
const SL_GROUP: usize = 0x98;
const SL_ALIGN_X: usize = 0x9C;
const SL_ALIGN_Y: usize = 0xA0;
const SL_OFFSET_X: usize = 0xC0;
const SL_OFFSET_Y: usize = 0xC8;
const SL_FIT_TO_ANCHOR: usize = 0xD8;
const SL_FIXED_SCALE: usize = 0xE0;
const SL_SPACING: usize = 0xE8;

// ── afp_mc_get_param (the SpriteLayer layout's own reads) ───────────────
const MC_PARAM_SCALE: i32 = 0x100D;
const MC_PARAM_HEIGHT: i32 = 0x1016;

// ── Actor tree ──────────────────────────────────────────────────────────
const ACTOR_PROBE_LEN: usize = 0x20;
const MAX_TREE_DEPTH: usize = 8;
const MAX_TREE_NODES: usize = 2048;
/// Pool-wrapper layer id.
const WRAPPER_LAYER_ID: usize = 0x08;

/// Label glyph height as a fraction of the plate's.
pub const LABEL_RATIO: f64 = 0.55;
/// Gap between the label's bottom and the plate box's top (px).
pub const LABEL_GAP: f64 = 2.0;
/// The `cote_*` glyph textures are 72 px tall (used until the first bitmap
/// row reports its own height).
const GLYPH_H_FALLBACK: f64 = 72.0;
/// Frames between bind attempts while a surface is wanted but unbound.
const BIND_RETRY_FRAMES: u32 = 15;

const GAMEPLAY_ANCHOR: &str = "name_usr";
const GAMEPLAY_CHILDREN: &[&str] = &[GAMEPLAY_ANCHOR];
/// Results anchors, NUL-terminated for the SpriteLayer's heap-form anchor
/// string (the layout hands the bytes to the game as a C string).
const RESULTS_ANCHOR_P1: &str = "player_1p_info_usr/profile_usr/player_name_usr\0";
const RESULTS_ANCHOR_P2: &str = "player_2p_info_usr/profile_usr/player_name_usr\0";
const RESULTS_CHILDREN: &[&str] = &[
    "player_1p_info_usr/profile_usr/player_name_usr",
    "player_2p_info_usr/profile_usr/player_name_usr",
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Surface {
    Gameplay,
    Results,
}

impl Surface {
    fn scene(self) -> i32 {
        match self {
            Surface::Gameplay => scene::GAMEPLAY,
            Surface::Results => scene::RESULTS_DETAIL,
        }
    }
    fn glyphs(self) -> &'static [&'static str] {
        match self {
            Surface::Gameplay => &LABEL_GLYPHS_GAMEPLAY,
            Surface::Results => &LABEL_GLYPHS_RESULTS,
        }
    }
    /// The stock plate's priority / group (its setup writes).
    fn priority(self) -> i32 {
        match self {
            Surface::Gameplay => 1,
            Surface::Results => 0,
        }
    }
    /// Horizontal alignment: gameplay plate left, results plate centred.
    fn align_x(self) -> i32 {
        match self {
            Surface::Gameplay => 0,
            Surface::Results => 1,
        }
    }
    fn anchor(self, side: usize) -> &'static str {
        match (self, side) {
            (Surface::Gameplay, _) => GAMEPLAY_ANCHOR,
            (Surface::Results, 0) => RESULTS_ANCHOR_P1,
            (Surface::Results, _) => RESULTS_ANCHOR_P2,
        }
    }
    fn children(self) -> &'static [&'static str] {
        match self {
            Surface::Gameplay => GAMEPLAY_CHILDREN,
            Surface::Results => RESULTS_CHILDREN,
        }
    }
}

struct Label {
    surface: Surface,
    sprite: *mut u8,
    layout: Option<LayoutFn>,
    /// Bound parent pool slot (null = unbound).
    wrapper: *mut u8,
    /// Anchor child MC id under the bound parent.
    anchor_mc: Option<u32>,
    displayed: bool,
    retry: u32,
    names: [MsvcString; 10],
    logged_bind: bool,
}

impl Label {
    fn new(surface: Surface) -> Self {
        Self {
            surface,
            sprite: std::ptr::null_mut(),
            layout: None,
            wrapper: std::ptr::null_mut(),
            anchor_mc: None,
            displayed: false,
            retry: 0,
            names: [const { MsvcString::empty() }; 10],
            logged_bind: false,
        }
    }
}

/// The ScoreActor fields the gameplay bind needs.
#[derive(Clone, Copy)]
struct ScoreActorSites {
    vtable: *const u8,
    side_off: usize,
    name_clip_off: usize,
}

struct Runtime {
    ctor: CtorFn,
    set_names: SetNamesFn,
    score: Option<ScoreActorSites>,
    gameplay: Label,
    results: Label,
}

// Raw game pointers valid for the process lifetime; touched on the game
// thread only (the frame callback).
unsafe impl Send for Runtime {}

static RUNTIME: Mutex<Option<Runtime>> = Mutex::new(None);
/// The side the label is armed for (−1 = none) — with [`SHOWING`], the only
/// thing the idle frame path reads.
static ARMED: AtomicI32 = AtomicI32::new(-1);
/// Some label still holds glyphs (so a disarm must still be blanked).
static SHOWING: AtomicBool = AtomicBool::new(false);

/// Capture the signatures (mod init). Fail-open, one WARN per missing part.
pub fn init(signatures: &SignatureStore) {
    let (Some(ctor), Some(set_names)) = (
        signatures.get_address("spritelayer_ctor"),
        signatures.get_address("spritelayer_set_names"),
    ) else {
        log_warn!("MultiplayerBot: SpriteLayer signatures unresolved -- no TARGET BOT label");
        return;
    };
    let score = match (
        signatures.get_address("score_actor_vtable"),
        signatures.ddr_sel_score_sites(),
    ) {
        (Some(vtable), Some(s)) => Some(ScoreActorSites {
            vtable,
            side_off: s.side_off,
            name_clip_off: s.name_clip_off,
        }),
        _ => {
            log_warn!(
                "MultiplayerBot: ScoreActor sites unresolved -- no TARGET BOT label in gameplay"
            );
            None
        }
    };
    let rt = Runtime {
        ctor: unsafe { std::mem::transmute::<*const u8, CtorFn>(ctor) },
        set_names: unsafe { std::mem::transmute::<*const u8, SetNamesFn>(set_names) },
        score,
        gameplay: Label::new(Surface::Gameplay),
        results: Label::new(Surface::Results),
    };
    *RUNTIME.lock().unwrap_or_else(|p| p.into_inner()) = Some(rt);
}

pub fn is_available() -> bool {
    RUNTIME.lock().map(|g| g.is_some()).unwrap_or(false)
}

/// Show the label over `side`'s plate from now on (the flip, Target mode
/// with a named target).
pub fn arm(side: usize) {
    ARMED.store(side as i32 & 1, Ordering::Release);
}

/// Stop showing it (restore). The next frame blanks whatever is up.
pub fn disarm() {
    ARMED.store(-1, Ordering::Release);
}

/// Per-frame driver (game thread; the dispatcher panic-contains it).
pub fn on_frame() {
    let armed = ARMED.load(Ordering::Acquire);
    if armed < 0 && !SHOWING.load(Ordering::Acquire) {
        return;
    }
    let mut guard = match RUNTIME.try_lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    let Some(rt) = guard.as_mut() else {
        return;
    };
    let current = scene_manager::current_scene();
    let (ctor, set_names, score) = (rt.ctor, rt.set_names, rt.score);
    for label in [&mut rt.gameplay, &mut rt.results] {
        let wanted = armed >= 0 && current == label.surface.scene();
        unsafe { drive(label, wanted, armed as usize, ctor, set_names, score) };
    }
    SHOWING.store(
        rt.gameplay.displayed || rt.results.displayed,
        Ordering::Release,
    );
}

/// One surface's frame: release when unwanted or stale, bind when wanted
/// and unbound (throttled), then measure + lay out.
unsafe fn drive(
    label: &mut Label,
    wanted: bool,
    side: usize,
    ctor: CtorFn,
    set_names: SetNamesFn,
    score: Option<ScoreActorSites>,
) {
    if !wanted {
        release(label, set_names);
        label.retry = 0;
        return;
    }
    if !label.wrapper.is_null()
        && !bm2d_api::wrapper_has_children(label.wrapper, label.surface.children())
    {
        release(label, set_names);
    }
    if label.wrapper.is_null() {
        if label.retry > 0 {
            label.retry -= 1;
            return;
        }
        label.retry = BIND_RETRY_FRAMES;
        if !bind(label, side, ctor, set_names, score) {
            return;
        }
    }
    measure(label);
    if let Some(layout) = label.layout {
        layout(label.sprite);
    }
}

/// Blank + unbind (idempotent).
unsafe fn release(label: &mut Label, set_names: SetNamesFn) {
    if label.displayed && !label.sprite.is_null() && !label.wrapper.is_null() {
        // Parent still set (a static pool slot) — SetBitmaps' trailing
        // layout may dereference it.
        apply_names(label, set_names, &[]);
    }
    label.displayed = false;
    if !label.sprite.is_null() {
        memory::write_ptr(label.sprite.add(SL_PARENT), std::ptr::null());
    }
    label.wrapper = std::ptr::null_mut();
    label.anchor_mc = None;
}

/// Find the parent, construct the SpriteLayer on first use, anchor + style
/// it and give it the label glyphs.
unsafe fn bind(
    label: &mut Label,
    side: usize,
    ctor: CtorFn,
    set_names: SetNamesFn,
    score: Option<ScoreActorSites>,
) -> bool {
    if !bm2d_api::is_available() {
        return false;
    }
    let wrapper = match label.surface {
        Surface::Gameplay => {
            let Some(score) = score else {
                return false;
            };
            match gameplay_wrapper(score, side) {
                Some(w) => w,
                None => return false,
            }
        }
        Surface::Results => match bm2d_api::find_wrapper_by_children(RESULTS_CHILDREN) {
            Some(w) => w,
            None => return false,
        },
    };
    if !bm2d_api::wrapper_has_children(wrapper, label.surface.children()) {
        return false;
    }
    let layer = memory::read_u32(wrapper.add(WRAPPER_LAYER_ID));
    let anchor_path = label.surface.anchor(side).trim_end_matches('\0');
    let Some(anchor_mc) = bm2d_api::layer_find_child(layer, anchor_path) else {
        return false;
    };

    if label.sprite.is_null() {
        let mem = memory::alloc_zeroed(SL_SIZE);
        if mem.is_null() {
            return false;
        }
        ctor(mem);
        let vtable = memory::read_ptr(mem) as *const usize;
        if vtable.is_null() {
            return false;
        }
        label.layout = Some(std::mem::transmute::<usize, LayoutFn>(*vtable));
        label.sprite = mem;
    }
    let sl = label.sprite;
    write_anchor(sl, label.surface.anchor(side));
    memory::write_i32(sl.add(SL_PRIORITY), label.surface.priority());
    memory::write_i32(sl.add(SL_GROUP), 0x7FFF_FFFF);
    memory::write_i32(sl.add(SL_ALIGN_X), label.surface.align_x());
    memory::write_i32(sl.add(SL_ALIGN_Y), 0);
    memory::write_u8(sl.add(SL_FIT_TO_ANCHOR), 0);
    write_f64(sl.add(SL_SPACING), 0.0);
    write_f64(sl.add(SL_OFFSET_X), 0.0);
    memory::write_ptr(sl.add(SL_PARENT), wrapper);
    label.wrapper = wrapper;
    label.anchor_mc = Some(anchor_mc);
    // Placement before the setter's trailing layout.
    measure(label);
    apply_names(label, set_names, label.surface.glyphs());
    label.displayed = true;
    if !label.logged_bind {
        label.logged_bind = true;
        log_info!(
            "MultiplayerBot: TARGET BOT label bound ({:?}, side {})",
            label.surface,
            side
        );
    }
    true
}

/// Re-measure the anchor and place the label above it.
unsafe fn measure(label: &mut Label) {
    let Some(anchor) = label.anchor_mc else {
        return;
    };
    if label.sprite.is_null() {
        return;
    }
    let name_h = match (
        bm2d_api::mc_get_param(anchor, MC_PARAM_HEIGHT),
        bm2d_api::mc_get_vec2(anchor, MC_PARAM_SCALE),
    ) {
        (Some(h), Some((_, sy))) => h as f64 * sy as f64,
        _ => return,
    };
    let glyph_h = first_glyph_height(label.sprite).unwrap_or(GLYPH_H_FALLBACK);
    if let Some((scale, dy)) = session::label_geometry(name_h, glyph_h, LABEL_RATIO, LABEL_GAP) {
        write_f64(label.sprite.add(SL_FIXED_SCALE), scale);
        write_f64(label.sprite.add(SL_OFFSET_Y), dy);
    }
}

/// The first bitmap's texture height, once SetBitmaps populated the row.
unsafe fn first_glyph_height(sl: *mut u8) -> Option<f64> {
    let begin = memory::read_ptr(sl.add(SL_BITMAPS_BEGIN));
    let end = memory::read_ptr(sl.add(SL_BITMAPS_END));
    if begin.is_null() || (end as usize) <= (begin as usize) {
        return None;
    }
    if !memory::is_readable(begin, 0x20) {
        return None;
    }
    let h = (begin.add(SL_BITMAP_H) as *const f64).read_unaligned();
    (h.is_finite() && h > 0.0).then_some(h)
}

/// The bot side's `dance_name` clip wrapper: a bounded walk of the live
/// DPS's actor tree for the ScoreActor of `side`.
unsafe fn gameplay_wrapper(score: ScoreActorSites, side: usize) -> Option<*mut u8> {
    let dps = song_reset::live_dps()?;
    let mut stack: Vec<(*mut u8, usize)> = vec![(dps, 0)];
    let mut visited = 0usize;
    while let Some((node, depth)) = stack.pop() {
        visited += 1;
        if visited > MAX_TREE_NODES {
            return None;
        }
        if !memory::is_readable(node, ACTOR_PROBE_LEN) {
            continue;
        }
        if memory::read_ptr(node) == score.vtable {
            if let Some(w) = score_actor_clip(score, node, side) {
                return Some(w);
            }
            continue;
        }
        if depth >= MAX_TREE_DEPTH {
            continue;
        }
        let mut child = memory::read_ptr(node.add(song_reset::FIRST_CHILD_OFFSET)) as *mut u8;
        let mut hops = 0usize;
        while !child.is_null() && hops < 256 {
            stack.push((child, depth + 1));
            if !memory::is_readable(child, ACTOR_PROBE_LEN) {
                break;
            }
            child = memory::read_ptr(child.add(song_reset::NEXT_SIBLING_OFFSET)) as *mut u8;
            hops += 1;
        }
    }
    None
}

/// `actor`'s `dance_name` wrapper when it is `side`'s ScoreActor.
unsafe fn score_actor_clip(score: ScoreActorSites, actor: *mut u8, side: usize) -> Option<*mut u8> {
    let len = score.side_off.max(score.name_clip_off) + 8;
    if !memory::is_readable(actor, len) {
        return None;
    }
    let holder = memory::read_ptr(actor.add(score.side_off));
    if holder.is_null() || !memory::is_readable(holder, 4) {
        return None;
    }
    if memory::read_i32(holder) != side as i32 {
        return None;
    }
    let clip = memory::read_ptr(actor.add(score.name_clip_off)) as *mut u8;
    (!clip.is_null()).then_some(clip)
}

/// The anchor path as the SpriteLayer's MSVC string: SSO when it fits,
/// otherwise a heap-form view of the `'static`, NUL-terminated bytes (the
/// game never frees it — the SpriteLayer is never destroyed).
unsafe fn write_anchor(sl: *mut u8, anchor: &'static str) {
    let s = sl.add(SL_ANCHOR_NAME);
    let text = anchor.trim_end_matches('\0');
    let bytes = text.as_bytes();
    if bytes.len() <= 15 {
        for i in 0..16 {
            *s.add(i) = bytes.get(i).copied().unwrap_or(0);
        }
        memory::write_u64(s.add(0x10), bytes.len() as u64);
        memory::write_u64(s.add(0x18), 0xF);
    } else {
        for i in 0..16 {
            *s.add(i) = 0;
        }
        memory::write_ptr(s, anchor.as_ptr());
        memory::write_u64(s.add(0x10), bytes.len() as u64);
        memory::write_u64(s.add(0x18), bytes.len().max(16) as u64);
    }
}

/// Hand a glyph list to the game's setter (copy-assign; the source stays
/// ours). Empty = blank (every CBitmap back to the pool).
unsafe fn apply_names(label: &mut Label, set_names: SetNamesFn, glyphs: &[&'static str]) {
    if label.sprite.is_null() || memory::read_ptr(label.sprite.add(SL_PARENT)).is_null() {
        return;
    }
    let count = glyphs.len().min(label.names.len());
    for (slot, name) in label.names.iter_mut().zip(glyphs.iter().take(count)) {
        if name.len() <= 15 {
            slot.set(name);
        } else {
            *slot = MsvcString::heap_ref(name.as_bytes());
        }
    }
    let begin = label.names.as_ptr();
    let vec = MsvcVec::<MsvcString> {
        begin,
        end: begin.add(count),
        cap_end: begin.add(label.names.len()),
    };
    set_names(label.sprite, &vec);
}

unsafe fn write_f64(addr: *mut u8, value: f64) {
    (addr as *mut f64).write_unaligned(value);
}
