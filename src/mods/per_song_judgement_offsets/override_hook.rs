//! The gameplay override + restore core for Per-Song Judgement Offsets
//! (plan Step 5; design §Architecture Overview → override lifecycle;
//! register D21 — Course/Dan and Training Mode APPLY overrides).
//!
//! Lifecycle per song, per side:
//!
//! 1. **Identity** — two sources, freshest wins at consume time:
//!    the wheel latch (scene-26 entry copies [`super::ui::current_code`]
//!    into `LOCKED_CODE`) and the **SSQ-open observer** ([`on_ssq_open`],
//!    fed by the LayeredFS `avs_fs_open` hook): every stage load — normal
//!    play, each course/dan stage, training mode — opens
//!    `.../ssq/<basename>[_N].ssq`, and the basename overwrites
//!    `LOCKED_CODE`. The observer is what keeps course stages 2+ correct;
//!    the wheel latch covers any path that skips a fresh SSQ open (in-place
//!    restarts reuse the loaded chart — the code correctly stays).
//! 2. **Arm** — scene-28 entry marks each entered side PENDING (event-mode
//!    belt-and-braces only; value resolution is deferred because the
//!    stage's SSQ may not be open yet at `createNextSequence(28)` time).
//! 3. **Write** — the side's FIRST judge dispatch consumes PENDING: resolve
//!    the offset from `LOCKED_CODE` via the store (no entry ⇒ nothing to
//!    do), read the stock value from `Option+0x24`
//!    (`*(*(player_option_table + side*8)) + 0xE0 + 0x24`), sanity-refuse
//!    outside ±100, cache it, write the override, set `ACTIVE`.
//!    Re-armed on EVERY scene-28 entry, so each course stage re-resolves
//!    against its own SSQ-published code; the restore between stages
//!    (layer 4) has already returned the previous song's stock value.
//! 4. **Restore** — the scene change with `prev == GAMEPLAY` (fires
//!    synchronously inside `createNextSequence`, an entire loader scene
//!    before the savekind-2 marshal; covers every exit shape including the
//!    quick-restart/fail redirects and the course inter-stage transitions)
//!    writes the cached stock value back and clears `ACTIVE`.
//! 5. **Sweep** — SONG_SELECT entry restores-and-WARNs if anything survived
//!    (unreachable by design). The save trampoline's tree fix
//!    ([`leaked_stock`] + `custom_options_persistence::replace_option_s32`)
//!    is the final independent layer.
//!
//! In-place restarts (song_reset) never leave scene 28 — the override
//! correctly persists across them (same song).

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};
use std::sync::Mutex;

use super::store;
use crate::mods::mod_trait::ModContext;
use crate::services::judge_hook::{self, Priority};
use crate::services::{scene_manager, stage_records};
use crate::types::scenes::scene;
use crate::{log_info, log_warn};

/// `ddr::player::Option` is inlined into the side context (PlayerWork) at
/// this offset; `timing_music` (JUDGEMENT OFFSET) lives at +0x24 inside it.
const CTX_OPTION_OFFSET: usize = 0xE0;
const OPTION_TIMING_MUSIC: usize = 0x24;
/// GamePlayActor's play-side field (same read real_speed/judge consumers use).
const ACTOR_PLAY_SIDE_OFFSET: usize = 0x84;
/// The stock option domain; a read outside it means the chain is wrong.
const TIMING_LIMIT: i32 = 100;

static PLAYER_OPTION_TABLE: AtomicUsize = AtomicUsize::new(0);
static REGISTERED: AtomicBool = AtomicBool::new(false);

/// Armed at every scene-28 entry per entered side; the VALUE is resolved
/// lazily at the side's first judge dispatch from the freshest LOCKED_CODE
/// (D21: at 28-entry a course stage's SSQ may not be open yet).
static PENDING: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
static ACTIVE: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
static STOCK_CACHE: [AtomicI32; 2] = [AtomicI32::new(0), AtomicI32::new(0)];
static LOCKED_CODE: Mutex<Option<String>> = Mutex::new(None);

/// One-shot warning latches (per class, not per song — log hygiene).
static WARNED_BAD_STOCK: AtomicBool = AtomicBool::new(false);
static WARNED_RESTORE_FAIL: AtomicBool = AtomicBool::new(false);

/// Stash the write target (called from the mod's `init`).
pub fn init(ctx: &ModContext) {
    let table = ctx.signatures.require_address("player_option_table");
    PLAYER_OPTION_TABLE.store(table as usize, Ordering::Release);
}

/// Register the judge + scene callbacks (idempotent; callbacks gate on
/// [`super::is_active`], so mod disable makes them inert without
/// unregistration).
///
/// `Priority::Early` is load-bearing (no other pre subscriber uses it):
/// assist_tick's `tick_clock` (`Priority::Normal`) reads `Option+0x24` when
/// it builds the song's tick list on the SAME first dispatch — the override
/// must already be in place so the claps mark the TRUE judgement moment
/// (maintainer requirement, deploy #3c). real_speed (`Normal`) reads only
/// speed fields; autoplay pre is `Late`.
pub fn enable() {
    if REGISTERED.swap(true, Ordering::AcqRel) {
        return;
    }
    if judge_hook::register_pre(Priority::Early, on_judge).is_none() {
        log_warn!("judgement_offsets: judge dispatcher unavailable -- overrides will not apply");
        // Scene callback still registers: restore/sweep must exist even if
        // arming can never complete (defensive symmetry; PENDING just rots).
    }
    scene_manager::on_scene_change(Box::new(on_scene_change));
    log_info!("judgement_offsets: override lifecycle armed (judge + scene callbacks)");
}

/// Mod disable: restore any live override immediately.
pub fn on_mod_disable() {
    restore_all("mod disable");
    PENDING[0].store(false, Ordering::Release);
    PENDING[1].store(false, Ordering::Release);
}

/// Dance-bank create observer (D21, the course fix): the game creates one
/// streaming dance bank (`sound/win/dance/<code>.xwb`) per stage load —
/// normal play, EACH course/dan stage, and training mode. Courses
/// batch-preload all SSQs at course start (deploy #3c evidence: zero
/// per-stage SSQ opens; stage 2+ misidentified), so this is the
/// authoritative per-stage identity; it fires strictly after the SSQ batch
/// and before the stage's first judge dispatch. Song-select preview creates
/// also land here — harmless (they track the highlighted song, same as the
/// wheel latch). Called from the wavebank create detour (game loader
/// thread).
pub fn on_dance_bank(code: &str) {
    if !super::is_active() || code.is_empty() {
        return;
    }
    if let Ok(mut guard) = LOCKED_CODE.lock() {
        if guard.as_deref() != Some(code) {
            log_info!("judgement_offsets: song identity '{}' (dance bank)", code);
            *guard = Some(code.to_string());
        }
    }
}

/// LayeredFS `avs_fs_open` observer (D21): every stage load opens the
/// song's chart as `mdb_apx/ssq/<basename>[_<level>].ssq` (normalized
/// path) — normal play, each course/dan stage, and training mode alike.
/// Publishing the basename here keeps `LOCKED_CODE` correct for course
/// stages 2+, which the wheel latch can't know. Runs on the game's loader
/// thread; kept allocation-free until the path IS an SSQ.
pub fn on_ssq_open(norm_path: &str) {
    if !super::is_active() {
        return;
    }
    let Some(rest) = norm_path.strip_prefix("mdb_apx/ssq/") else {
        return;
    };
    let Some(stem) = rest.strip_suffix(".ssq") else {
        return;
    };
    if stem.is_empty() || stem.contains('/') {
        return;
    }
    // Split-chart files are `<basename>_<level>` with level 1..=5.
    let basename = match stem.rsplit_once('_') {
        Some((base, level))
            if !base.is_empty()
                && level.len() == 1
                && matches!(level.as_bytes()[0], b'1'..=b'5') =>
        {
            base
        }
        _ => stem,
    };
    if let Ok(mut guard) = LOCKED_CODE.lock() {
        if guard.as_deref() != Some(basename) {
            log_info!("judgement_offsets: song identity '{}' (ssq open)", basename);
            *guard = Some(basename.to_string());
        }
    }
}

/// The save trampoline's leak probe: `Some(stock)` when the given side still
/// has an override in PlayerWork at save-build time (should be unreachable —
/// the scene restore precedes every marshal). The caller rewrites
/// `<timing_music>` in the built tree with the returned stock value.
pub fn leaked_stock(side: usize) -> Option<i32> {
    let side = side.min(1);
    if !ACTIVE[side].load(Ordering::Acquire) {
        return None;
    }
    Some(STOCK_CACHE[side].load(Ordering::Acquire))
}

// ── Scene lifecycle ──────────────────────────────────────────────────────

fn on_scene_change(prev: i32, next: i32) {
    if !super::is_active() {
        return;
    }

    // Restore FIRST: prev==28 must win over anything else this transition
    // means (it precedes the savekind-2 marshal by a whole loader scene).
    if prev == scene::GAMEPLAY {
        restore_all("gameplay exit");
    }

    if next == scene::SONG_TO_STAGE_INTERSTITIAL {
        // Lock the wheel code for the upcoming song.
        let code = super::ui::current_code();
        if let Ok(mut guard) = LOCKED_CODE.lock() {
            *guard = code;
        }
    } else if next == scene::GAMEPLAY {
        arm_sides();
    } else if next == scene::SONG_SELECT {
        // Sweep: nothing may survive back to the wheel.
        for side in 0..2 {
            if ACTIVE[side].load(Ordering::Acquire) {
                log_warn!(
                    "judgement_offsets: P{} override survived to song select -- restoring (sweep)",
                    side + 1
                );
                restore_side(side);
            }
            PENDING[side].store(false, Ordering::Release);
        }
    }
}

/// Arm entered sides for the upcoming song. Value resolution is DEFERRED to
/// the first judge dispatch (D21): at `createNextSequence(28)` time a course
/// stage's SSQ hasn't been opened yet, so any lookup here would use the
/// previous stage's identity. Event mode (1/2) never runs plain GAMEPLAY,
/// but check belt-and-braces when the session-state decode is available.
fn arm_sides() {
    let event = stage_records::event_mode().unwrap_or(0);
    if event != 0 {
        PENDING[0].store(false, Ordering::Release);
        PENDING[1].store(false, Ordering::Release);
        return;
    }
    for side in 0..2 {
        let entered = stage_records::side_entered(side).unwrap_or(false);
        PENDING[side].store(entered, Ordering::Release);
    }
}

// ── Judge-dispatch write ─────────────────────────────────────────────────

/// O(1) steady state (one consumed-latch check); the once-per-song work is
/// one short LOCKED_CODE lock + store lookup + a handful of guarded
/// reads/stores. No allocation on the steady path.
fn on_judge(actor: *mut u8, _music_count: i32) {
    if actor.is_null() || !super::is_active() {
        return;
    }
    let raw_side = unsafe { *(actor.add(ACTOR_PLAY_SIDE_OFFSET) as *const i32) };
    let side = if raw_side == 1 { 1usize } else { 0usize };
    if !PENDING[side].swap(false, Ordering::AcqRel) {
        return;
    }

    // Lazy value resolution (D21): the freshest song identity — the SSQ
    // observer has fired by now for every flow that loads a chart (normal,
    // course stages, training), and the wheel latch covers the rest.
    let code = LOCKED_CODE.lock().ok().and_then(|g| g.clone());
    let Some(code) = code else {
        return; // no identity — stock applies silently
    };
    let Some(offset) = store::with_store(|s| s.lookup(side, &code)) else {
        return; // no entry for this song — stock applies (the normal case)
    };
    let offset = offset as i32;

    let Some(field) = timing_field_ptr(side) else {
        warn_once(
            &WARNED_BAD_STOCK,
            "judgement_offsets: option chain unreadable at arm -- override skipped",
        );
        return;
    };
    unsafe {
        let stock = *(field as *const i32);
        if stock.abs() > TIMING_LIMIT {
            warn_once(
                &WARNED_BAD_STOCK,
                "judgement_offsets: stock timing outside +/-100 -- chain distrusted, override skipped",
            );
            return;
        }
        STOCK_CACHE[side].store(stock, Ordering::Release);
        *(field as *mut i32) = offset;
        ACTIVE[side].store(true, Ordering::Release);
        log_info!(
            "judgement_offsets: P{} override applied ({}ms for '{}', stock {}ms cached)",
            side + 1,
            offset,
            code,
            stock
        );
    }
}

// ── Restore ──────────────────────────────────────────────────────────────

fn restore_all(reason: &str) {
    for side in 0..2 {
        if ACTIVE[side].load(Ordering::Acquire) {
            if restore_side(side) {
                log_info!(
                    "judgement_offsets: P{} stock timing restored ({})",
                    side + 1,
                    reason
                );
            }
        }
    }
}

/// Write the cached stock value back. Clears ACTIVE only on success — a
/// failed walk leaves the leak flag set so the trampoline tree fix (the
/// final layer) still fires.
fn restore_side(side: usize) -> bool {
    let Some(field) = timing_field_ptr(side) else {
        warn_once(
            &WARNED_RESTORE_FAIL,
            "judgement_offsets: option chain unreadable at RESTORE -- relying on save-tree fix",
        );
        return false;
    };
    unsafe {
        *(field as *mut i32) = STOCK_CACHE[side].load(Ordering::Acquire);
    }
    ACTIVE[side].store(false, Ordering::Release);
    true
}

/// `&Option+0x24` for a side, fully guarded (None on any null hop).
fn timing_field_ptr(side: usize) -> Option<*mut u8> {
    let table = PLAYER_OPTION_TABLE.load(Ordering::Acquire) as *const u8;
    if table.is_null() {
        return None;
    }
    unsafe {
        let holder = *(table.add(side.min(1) * 8) as *const *const u8);
        if holder.is_null() {
            return None;
        }
        let ctx = *(holder as *const *const u8);
        if ctx.is_null() {
            return None;
        }
        Some(ctx.add(CTX_OPTION_OFFSET + OPTION_TIMING_MUSIC) as *mut u8)
    }
}

fn warn_once(latch: &AtomicBool, message: &str) {
    if !latch.swap(true, Ordering::AcqRel) {
        log_warn!("{}", message);
    }
}
