//! Options-menu UI for Per-Song Judgement Offsets (plan Step 4; design
//! §Components → DLL: new mod, Detailed Requirements 2).
//!
//! Owns the two option rows and the song-wheel selection poll:
//!
//! - `adjust_song_offset` — parent bool ("ADJUST OFFSET FOR CURRENT SONG").
//! - `current_song_offset` — child scalar −100..+100 ms, visible while the
//!   parent is ON.
//!
//! Every frame at SONG_SELECT the poll tracks the highlighted song (the
//! shipped `music_wheel_song_length` pattern: `selectmusic_model` global →
//! weak_ptr at `+0x1B0/+0x1B8` → guarded inner vtable getter for the code)
//! and re-seeds both rows per entered side via `set_value_silent` (no
//! callbacks). User edits flow the other way through the `on_change`
//! handlers into the [`store`](super::store) and the CSV writer — to ONE
//! side normally, or to BOTH sides when the operator sets
//! `per_song_judgement_offsets.mirror_players` in `mod-config.json`
//! (last writer wins; see [`apply_edit`]).
//!
//! Threading: the poll and the `on_change` handlers both run on the game's
//! render thread (input_manager frame callbacks and the options rows live
//! there), so `CURRENT_CODE` is a plain Mutex touched only from that thread;
//! the store Mutex is never held across a custom_options call.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

use super::{bootstrap, store};
use crate::core::memory;
use crate::mods::mod_trait::ModContext;
use crate::services::custom_options::{self, PersistMode, RegisterSpec, ScalarFormat, ShowWhen};
use crate::services::{input_manager, scene_manager};
use crate::types::scenes::scene;
use crate::{log_info, log_warn};

pub const OPT_PARENT: &str = "adjust_song_offset";
pub const OPT_CHILD: &str = "current_song_offset";

type MusicCodeGetterFn = unsafe extern "C" fn(this: *mut u8) -> *const u8;

/// Resolved at `init` from the signature store.
static MODEL_GLOBAL: AtomicUsize = AtomicUsize::new(0);
static MODULE_BASE: AtomicUsize = AtomicUsize::new(0);
static MODULE_SIZE: AtomicUsize = AtomicUsize::new(0);

/// Both rows registered — the gate for every callback in this module.
static ROWS_READY: AtomicBool = AtomicBool::new(false);

/// Config knob `per_song_judgement_offsets.mirror_players` (default false):
/// an edit by EITHER side applies to BOTH sides in-sync — both session maps,
/// both CSV columns, and the other side's rows (last writer wins). For solo
/// home players without a persisting backend who swap cabinet sides.
static MIRROR_PLAYERS: AtomicBool = AtomicBool::new(false);

/// The highlighted song's code (None off-scene / on a folder row). Written
/// by the poll, read by the `on_change` handlers — same thread.
static CURRENT_CODE: Mutex<Option<String>> = Mutex::new(None);

/// Poll state (render thread only).
struct PollState {
    last_selection: usize,
    /// Code still unread for the current selection (late-read retry).
    pending: bool,
}

static POLL: Mutex<PollState> = Mutex::new(PollState {
    last_selection: 0,
    pending: false,
});

/// Stash the signature addresses (called from the mod's `init`).
pub fn init(ctx: &ModContext) {
    let model = ctx.signatures.require_address("selectmusic_model");
    MODEL_GLOBAL.store(model as usize, Ordering::Release);
    MODULE_BASE.store(ctx.game_module.base as usize, Ordering::Release);
    MODULE_SIZE.store(ctx.game_module.size, Ordering::Release);
}

/// Register rows + the frame poll. Returns false when the mod must stay
/// inert (design D20).
pub fn enable() -> bool {
    let parent = RegisterSpec::bool_toggle(OPT_PARENT)
        .display_name("Adjust Offset for Current Song")
        .description("Use a per-song judgement offset for the highlighted song")
        .default_value(0)
        .persist_mode(PersistMode::None) // design D9: per-song data owns its own persistence
        .on_change(on_parent_change);
    if let Err(e) = custom_options::register_option(parent) {
        log_warn!("judgement_offsets: parent row registration failed: {e}");
        return false;
    }
    let child = RegisterSpec::scalar(
        OPT_CHILD,
        -100,
        100,
        1,
        // Stock timing-row display parity: "-41ms" / "+10ms" / "±0ms".
        ScalarFormat::SignedUnit { unit: "ms" },
    )
    .display_name("Current Song Offset")
    .description("Judgement offset applied to the highlighted song only")
    .step_coarse(10)
    .default_value(0)
    .persist_mode(PersistMode::None)
    .show_when(ShowWhen::Equals {
        parent_id: OPT_PARENT.into(),
        value: 1,
    })
    .on_change(on_child_change);
    if let Err(e) = custom_options::register_option(child) {
        // The parent row exists but can't be deregistered; leaving
        // ROWS_READY false keeps every callback (and thus the whole mod)
        // inert regardless.
        log_warn!("judgement_offsets: child row registration failed: {e} -- mod inert");
        return false;
    }

    input_manager::on_frame(std::sync::Arc::new(on_frame));

    let mirror = crate::mods::config::get()
        .and_then(|c| c.per_song_judgement_offsets.as_ref())
        .and_then(|c| c.mirror_players)
        .unwrap_or(false);
    MIRROR_PLAYERS.store(mirror, Ordering::Release);
    if mirror {
        log_info!("judgement_offsets: mirror_players enabled -- edits apply to both sides");
    }

    ROWS_READY.store(true, Ordering::Release);
    log_info!("judgement_offsets: option rows registered, wheel poll armed");
    true
}

/// The highlighted song's code, if any (consumed by later steps too).
pub fn current_code() -> Option<String> {
    CURRENT_CODE.lock().ok().and_then(|g| g.clone())
}

// ── Frame poll ───────────────────────────────────────────────────────────

fn on_frame() {
    if !ROWS_READY.load(Ordering::Acquire) || !super::is_active() {
        return;
    }
    if scene_manager::current_scene() != scene::SONG_SELECT {
        // Leaving the wheel: clear so gameplay latching (Step 5) reads the
        // scene-26 locked copy, not a stale pointer.
        let mut poll = match POLL.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        if poll.last_selection != 0 {
            poll.last_selection = 0;
            poll.pending = false;
            set_current_code(None);
        }
        return;
    }

    let mut poll = match POLL.lock() {
        Ok(g) => g,
        Err(_) => return,
    };

    // SAFETY: pointer walk mirrors the game's own per-frame card tick over
    // the same global; every hop is null/liveness/bounds guarded.
    let selection = unsafe { read_selection() };

    if selection != poll.last_selection {
        poll.last_selection = selection;
        poll.pending = selection != 0;
        set_current_code(None);
        seed_rows(None);
    }
    if poll.pending {
        if let Some(code) = unsafe { read_code(poll.last_selection as *mut u8) } {
            poll.pending = false;
            seed_rows(Some(&code));
            set_current_code(Some(code));
        }
    }
}

/// Highlighted-song holder pointer (0 = none/folder/dead).
unsafe fn read_selection() -> usize {
    let global = MODEL_GLOBAL.load(Ordering::Acquire) as *const u8;
    if global.is_null() {
        return 0;
    }
    let model_obj = memory::read_ptr(global);
    if model_obj.is_null() {
        return 0;
    }
    let obj = memory::read_ptr(model_obj.add(0x1B0)) as *mut u8;
    let ctrl = memory::read_ptr(model_obj.add(0x1B8));
    let strong = if ctrl.is_null() {
        0
    } else {
        memory::read_u32(ctrl.add(0x08))
    };
    if obj.is_null() || strong == 0 {
        0
    } else {
        obj as usize
    }
}

/// Read the selection's song code (guarded inner vtable getter — the
/// music_wheel_song_length shape).
unsafe fn read_code(holder: *mut u8) -> Option<String> {
    if holder.is_null() {
        return None;
    }
    let inner = memory::read_ptr(holder) as *mut u8;
    let inner_ctrl = memory::read_ptr(holder.add(0x08));
    if inner.is_null() || inner_ctrl.is_null() {
        return None;
    }
    if memory::read_u32(inner_ctrl.add(0x08)) == 0 {
        return None;
    }
    let base = MODULE_BASE.load(Ordering::Acquire);
    let size = MODULE_SIZE.load(Ordering::Acquire);
    let in_module = |p: usize| p >= base && p < base + size;
    let vtable = memory::read_ptr(inner) as *const usize;
    if vtable.is_null() || !in_module(vtable as usize) {
        return None;
    }
    let getter_addr = *vtable.add(1); // vt+0x08
    if !in_module(getter_addr) {
        return None;
    }
    let getter = std::mem::transmute::<usize, MusicCodeGetterFn>(getter_addr);
    let cstr = getter(inner);
    if cstr.is_null() {
        return None;
    }
    let mut out = Vec::with_capacity(16);
    for i in 0..32usize {
        let b = *cstr.add(i);
        if b == 0 {
            break;
        }
        out.push(b);
    }
    if out.is_empty() || out.len() >= 32 {
        return None;
    }
    String::from_utf8(out).ok()
}

fn set_current_code(code: Option<String>) {
    if let Ok(mut guard) = CURRENT_CODE.lock() {
        *guard = code;
    }
}

/// Seed both rows for both sides from the store (silent — no callbacks).
/// `None` = no song highlighted → OFF/0.
fn seed_rows(code: Option<&str>) {
    for side in 0u8..2 {
        let (parent, child) = match code {
            Some(code) => store::with_store(|s| s.row_seed(side as usize, code)),
            None => (0, 0),
        };
        custom_options::set_value_silent(OPT_PARENT, side, parent);
        custom_options::set_value_silent(OPT_CHILD, side, child);
    }
}

// ── Edit capture ─────────────────────────────────────────────────────────

/// Apply an options-menu edit (`None` = cleared) to the store + CSV, and —
/// when `mirror_players` is on — to the OTHER side too, keeping that side's
/// rows visually in-sync via silent seeds (never re-seed the editing side:
/// it would fight the in-flight edit).
fn apply_edit(side: u8, code: &str, value: Option<i8>) {
    let mirror = MIRROR_PLAYERS.load(Ordering::Acquire);
    let sides: &[usize] = if mirror { &[0, 1] } else { &[side as usize] };
    store::with_store(|s| {
        for &target in sides {
            match value {
                Some(v) => s.set_entry(target, code, v),
                None => s.clear_entry(target, code),
            }
        }
    });
    for &target in sides {
        bootstrap::queue_csv_upsert(code.to_string(), target, value);
    }
    if mirror {
        let other = 1 - side;
        let (parent, child) = match value {
            Some(v) => (1, v as i32),
            None => (0, 0),
        };
        custom_options::set_value_silent(OPT_PARENT, other, parent);
        custom_options::set_value_silent(OPT_CHILD, other, child);
    }
}

fn on_parent_change(side: u8, new_value: i32) {
    if !ROWS_READY.load(Ordering::Acquire) || !super::is_active() {
        return;
    }
    let Some(code) = current_code() else {
        return; // registration prime / no song highlighted
    };
    if new_value == 1 {
        // Adopt the child row's current value (0 for a fresh enable).
        let value = custom_options::get_value(side, OPT_CHILD)
            .unwrap_or(0)
            .clamp(-100, 100) as i8;
        apply_edit(side, &code, Some(value));
    } else {
        apply_edit(side, &code, None);
    }
}

fn on_child_change(side: u8, new_value: i32) {
    if !ROWS_READY.load(Ordering::Acquire) || !super::is_active() {
        return;
    }
    let Some(code) = current_code() else {
        return;
    };
    // Only meaningful while the parent is ON (the row is hidden otherwise,
    // but guard anyway — loads/primes can fire callbacks).
    if custom_options::get_value(side, OPT_PARENT) != Some(1) {
        return;
    }
    let value = new_value.clamp(-100, 100) as i8;
    apply_edit(side, &code, Some(value));
}
