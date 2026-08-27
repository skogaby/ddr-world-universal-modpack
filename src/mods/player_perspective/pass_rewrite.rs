//! The C1 pass-rewrite mechanism: per-side VS-constant upload + SetShader
//! program-field rewrite, on the render_notes dispatcher.
//!
//! Pre callback (@ Normal, before the original runs):
//!   1. Bind the renderer to a play side; early-return unless that side
//!      latched a perspective preset (zero footprint in the OVERHEAD state).
//!   2. Emit a tag-0x14 SetVSConstantF record carrying the side's
//!      perspective block (c48/c49) — in list order it precedes every draw
//!      of the pass, so the worker thread uploads the constants first.
//!   3. Snapshot the CommandList write pointer (window start).
//!
//! Post callback (@ Late — after mine_render's @ Normal, so the mine pass's
//! records fall inside the window):
//!   Walk `[start .. write_ptr]` and flip the `program` field of every
//!   tag-0x13 SetShader record that binds the pass's arrow shader or the
//!   default shader from 0 → 1 — but ONLY if that shader object's parsed
//!   program count is ≥ 2. The engine's SetShader handler has NO bounds
//!   check (Ghidra-verified): an out-of-range index forwards a garbage
//!   handle to the render thread. The gate is mandatory, not defensive.
//!
//! Record layouts (all Ghidra-verified, stable across builds):
//!   0x13 SetShader:      {u16 tag, u16 size=0x18, u32 pad, u64 shaderObj@+8,
//!                         u32 programIdx@+0x10}
//!   0x14 SetVSConstantF: {u16 tag, u16 size=0x18+n*0x10, u32 regOff@+4,
//!                         u32 nRegs@+8, u32 pad@+0xC, u64 ptr@+0x10,
//!                         inline float4 payload @+0x18} — register base c48.

use std::cell::Cell;
use std::ptr::{addr_of, addr_of_mut};
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};

use retour::GenericDetour;

use crate::core::signatures::SignatureStore;
use crate::services::render_notes_hook;
use crate::{log_info, log_warn};

// ── ArrowRenderer field offsets (Ghidra; see mine_render's table) ────
const OFF_POS_X: usize = 0x30;
const OFF_POS_Y: usize = 0x34;
const OFF_VBPTR: usize = 0x80;
const OFF_MODE: usize = 0xB0;
const OFF_ARROW_SHADER: usize = 0xC0;

// ── SpotRenderer field offsets (same ArrowSprite base; Ghidra) ───────
const OFF_SPOT_MODE: usize = 0x98;
const OFF_SPOT_SHADER: usize = 0xA0;

// ── JudgeEffectRenderer field offsets (same ArrowSprite base; Ghidra,
// constructor + draw verified on 20260721/20260324) ──────────────────
const OFF_JUDGE_FX_SHADER: usize = 0x98;

const X_SPLIT: f32 = 640.0;

/// The `default_shader` derived global (pointer to the shader-object
/// pointer), resolved at mod init. Shock and mine passes bind it.
static DEFAULT_SHADER_GLOBAL: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());

/// One-shot per-boot gate warnings (a blocked rewrite means the extended
/// container isn't loaded — degrade silently to stock visuals after the
/// first report).
static WARNED_ARROW_GATE: AtomicBool = AtomicBool::new(false);
static WARNED_DEFAULT_GATE: AtomicBool = AtomicBool::new(false);
static WARNED_SPOT_GATE: AtomicBool = AtomicBool::new(false);
static WARNED_JUDGE_FX_GATE: AtomicBool = AtomicBool::new(false);
/// One-shot per-song "the rewrite is live" log for deploy observability.
static LOGGED_FIRST_REWRITE: AtomicBool = AtomicBool::new(false);
/// One-shot per-boot spot-pass observability log (shader object identity).
static LOGGED_SPOT_PASS: AtomicBool = AtomicBool::new(false);
/// One-shot per-boot judge-effect-pass observability log.
static LOGGED_JUDGE_FX_PASS: AtomicBool = AtomicBool::new(false);

/// Per-pass capture handed from the pre callback to the post callback.
/// Thread-synchronous: both run on the same thread within one dispatcher
/// invocation (the guideline_hook PASS_STATE pattern).
#[derive(Clone, Copy)]
struct PassCapture {
    start: *mut u8,
    arrow_shader: *const u8,
}

thread_local! {
    static PASS_STATE: Cell<Option<PassCapture>> = const { Cell::new(None) };
}

/// Resolve the default-shader global + install the receptor-pass (spot
/// draw) detour. Called from the mod's `init` (detours are one-shot; the
/// callback gates on the per-song latch, so presence is free when idle).
pub(super) fn resolve(signatures: &SignatureStore) {
    if let Some(a) = signatures.get_address("default_shader") {
        DEFAULT_SHADER_GLOBAL.store(a as *mut u8, Ordering::Release);
    }
    match signatures.get_address("spot_render") {
        Some(addr) => unsafe {
            let target: SpotRenderFn = std::mem::transmute(addr);
            match crate::core::hooks::install_enabled(
                addr_of_mut!(SPOT_HOOK),
                target,
                spot_render_cb,
            ) {
                Ok(()) => {
                    log_info!("player-perspective: spot (receptor) detour installed @ {addr:p}")
                }
                Err(e) => log_warn!(
                    "player-perspective: spot detour install failed: {e} — receptors stay flat"
                ),
            }
        },
        None => {
            log_warn!("player-perspective: spot_render signature unresolved — receptors stay flat")
        }
    }
    match signatures.get_address("judge_effect_render") {
        Some(addr) => unsafe {
            let target: JudgeFxRenderFn = std::mem::transmute(addr);
            match crate::core::hooks::install_enabled(
                addr_of_mut!(JUDGE_FX_HOOK),
                target,
                judge_fx_render_cb,
            ) {
                Ok(()) => log_info!(
                    "player-perspective: judge-effect (hit/hold glow) detour installed @ {addr:p}"
                ),
                Err(e) => log_warn!(
                    "player-perspective: judge-effect detour install failed: {e} — hit/hold glow stays flat"
                ),
            }
        },
        None => log_warn!(
            "player-perspective: judge_effect_render signature unresolved — hit/hold glow stays flat"
        ),
    }
}

/// Song-boundary reset (GAMEPLAY enter/exit, mod disable).
pub(super) fn reset_song_state() {
    LOGGED_FIRST_REWRITE.store(false, Ordering::Release);
    PASS_STATE.with(|c| c.set(None));
    super::clear_published();
}

/// Side binding for one render_notes invocation: doubles owns side 0; the
/// side-offset singles layout splits at screen center. (One renderer = one
/// side; versus runs two invocations per frame.)
fn bind_side(mode: i32, pos_x: f32) -> u8 {
    if mode == 1 {
        0
    } else if pos_x < X_SPLIT {
        0
    } else {
        1
    }
}

/// Read the ArrowSprite-base reverse flag and map it to the VS's y_dir:
/// `*(u8*)(this + 0x80 + *(i32*)(*(u64*)(this+0x80) + 4))` (the exact read
/// render_sprite_final performs). Null vb → normal scroll.
unsafe fn read_y_dir(renderer: *const u8) -> f32 {
    let vb = *(renderer.add(OFF_VBPTR) as *const *const u8);
    if vb.is_null() {
        return 1.0;
    }
    let disp = *(vb.add(4) as *const i32);
    if *renderer.add(OFF_VBPTR + disp as usize) != 0 {
        -1.0
    } else {
        1.0
    }
}

/// Walk `[start..end]` and flip every tag-0x13 record binding one of
/// `targets` to program 1 (behind the mandatory ≥2-programs gate — the
/// engine's SetShader handler has NO bounds check). Returns the rewrite
/// count; aborts on a corrupt record.
unsafe fn rewrite_window(
    start: *const u8,
    end: *const u8,
    targets: &[(*const u8, &AtomicBool, &str)],
) -> u32 {
    let mut rewrites = 0u32;
    let mut p = start;
    while p < end {
        let tag = (p as *const u16).read_unaligned();
        let size = (p.add(2) as *const u16).read_unaligned() as usize;
        if size < 4 || p.add(size) > end {
            log_warn!(
                "player-perspective: corrupt record @ {:p} (tag=0x{:X} size=0x{:X}) -- aborting window walk",
                p, tag, size
            );
            return rewrites;
        }
        if tag == 0x13 {
            let shader = *(p.add(8) as *const *const u8);
            if !shader.is_null() {
                if let Some((_, warned, label)) = targets
                    .iter()
                    .find(|(t, _, _)| *t == shader && !t.is_null())
                {
                    // The mandatory gate: only rewrite when the object's
                    // parsed program count (shaderObj+4) admits program 1.
                    let prog_count = *(shader.add(4) as *const u32);
                    if prog_count >= 2 {
                        (p.add(0x10) as *mut u32).write_unaligned(1);
                        rewrites += 1;
                    } else if !warned.swap(true, Ordering::AcqRel) {
                        log_warn!(
                            "player-perspective: {} shader has {} program(s) -- extended container not loaded; leaving stock (degraded visuals)",
                            label,
                            prog_count
                        );
                    }
                }
            }
        }
        p = p.add(size);
    }
    rewrites
}

/// Pre @ Normal: constants + window snapshot. Zero footprint unless this
/// renderer's side latched a perspective preset.
pub(super) fn pre_render_notes(renderer: *mut u8) {
    if renderer.is_null() || !super::any_side_latched() {
        return;
    }
    // Drain any captured receptor-flash clips (playfield_styling's lane
    // queue; one atomic load when idle). The fill hook drives this too, but
    // only while playfield_styling is enabled — the perspective lane pass
    // is the drain site that always runs when a perspective side is live.
    crate::mods::playfield_styling::lane_apply_pending();
    unsafe {
        let mode = *(renderer.add(OFF_MODE) as *const i32);
        let pos_x = *(renderer.add(OFF_POS_X) as *const f32);
        let pos_y = *(renderer.add(OFF_POS_Y) as *const f32);
        let side = bind_side(mode, pos_x);
        let params = match super::latched_params(side) {
            Some(p) => p,
            None => return,
        };

        // Reverse flag via the ArrowSprite virtual base.
        let y_dir = read_y_dir(renderer);
        let c =
            super::compute_constants(&params, pos_y, super::lane_center(mode == 1, pos_x), y_dir);
        // Publish for CPU-side consumers outside the lane pass (the
        // receptor hit flash in playfield_styling::lane_hook).
        super::publish_constants(side, &c);

        let cl = render_notes_hook::active_command_list();
        if cl.is_null() {
            return;
        }
        emit_persp_constants(cl, &c);

        let start = render_notes_hook::write_ptr(cl);
        if start.is_null() {
            return;
        }
        let arrow_shader = *(renderer.add(OFF_ARROW_SHADER) as *const *const u8);
        PASS_STATE.with(|c| {
            c.set(Some(PassCapture {
                start,
                arrow_shader,
            }))
        });
    }
}

/// Post @ Late: rewrite the captured window's SetShader records to the
/// perspective program (behind the mandatory ≥2-programs gate).
pub(super) fn post_render_notes(_renderer: *mut u8) {
    let cap = match PASS_STATE.with(|c| c.replace(None)) {
        Some(c) => c,
        None => return,
    };
    unsafe {
        let cl = render_notes_hook::active_command_list();
        let end = render_notes_hook::write_ptr(cl);
        if end.is_null() || end < cap.start {
            return;
        }
        let default_shader = {
            let g = DEFAULT_SHADER_GLOBAL.load(Ordering::Acquire);
            if g.is_null() {
                std::ptr::null()
            } else {
                *(g as *const *const u8)
            }
        };

        let rewrites = rewrite_window(
            cap.start,
            end,
            &[
                (cap.arrow_shader, &WARNED_ARROW_GATE, "arrow"),
                (default_shader, &WARNED_DEFAULT_GATE, "default"),
            ],
        );

        if rewrites > 0 && !LOGGED_FIRST_REWRITE.swap(true, Ordering::AcqRel) {
            log_info!(
                "player-perspective: perspective pass live ({} SetShader record(s) rewritten to program 1)",
                rewrites
            );
        }
    }
}

// ── Receptor (spot) pass rewrite ─────────────────────────────────────
//
// The receptor row spans ~96 px of track depth, so the perspective map
// foreshortens it exactly like a note sitting at the row (slight trapezoid,
// far edge converged) — leaving receptors screen-flat while the lane
// recedes looks pasted-on. The spot draw is its own function emitting its
// own SetShader (spot shader @ this+0xA0, program 0) + one quad batch, so
// it gets its own detour: constants + snapshot before, window rewrite
// after. Same latch, same gate. Best-effort: install failure just leaves
// receptors flat.

type SpotRenderFn = unsafe extern "C" fn(this: *mut u8);

static mut SPOT_HOOK: Option<GenericDetour<SpotRenderFn>> = None;

unsafe extern "C" fn spot_render_cb(this: *mut u8) {
    let hook = match &*addr_of!(SPOT_HOOK) {
        Some(h) => h,
        None => return,
    };
    if this.is_null() || !super::any_side_latched() {
        return hook.call(this);
    }

    // Plan the pass (side bind + constants). Any anomaly → stock call.
    let plan = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mode = *(this.add(OFF_SPOT_MODE) as *const i32);
        let pos_x = *(this.add(OFF_POS_X) as *const f32);
        let pos_y = *(this.add(OFF_POS_Y) as *const f32);
        let side = bind_side(mode, pos_x);
        let params = super::latched_params(side)?;
        Some(super::compute_constants(
            &params,
            pos_y,
            super::lane_center(mode == 1, pos_x),
            read_y_dir(this),
        ))
    }))
    .unwrap_or(None);

    let c = match plan {
        Some(p) => p,
        None => return hook.call(this),
    };

    let cl = render_notes_hook::active_command_list();
    if cl.is_null() {
        return hook.call(this);
    }
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        emit_persp_constants(cl, &c);
    }));
    let start = render_notes_hook::write_ptr(cl);

    hook.call(this);

    if start.is_null() {
        return;
    }
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let end = render_notes_hook::write_ptr(cl);
        if end.is_null() || end < start {
            return;
        }
        let spot_shader = *(this.add(OFF_SPOT_SHADER) as *const *const u8);
        let n = rewrite_window(start, end, &[(spot_shader, &WARNED_SPOT_GATE, "spot")]);
        if n > 0 && !LOGGED_SPOT_PASS.swap(true, Ordering::AcqRel) {
            log_info!(
                "player-perspective: receptor pass live (spot shader {:p}, {} record(s) rewritten)",
                spot_shader,
                n
            );
        }
    }));
}

// ── JudgeEffect (tap hit-burst + freeze-hold glow) pass rewrite ──────
//
// `screen::JudgeEffectRenderer` draws arrow-sheet cells at the receptor
// row (records carry a type field: taps and hold-glow refreshes) through
// its OWN per-frame draw with its OWN SetShader (judge shader @ this+0x98,
// program hardcoded 0) — outside every pass we already rewrite, so without
// this detour the glow renders flat at the STOCK receptor position
// (cabinet-diagnosed under DISTANT). Same recipe as the spot pass:
// constants + snapshot before, window rewrite after.
//
// Side binding: presence-first (the game's own player-array flags — the
// lane_hook/guideline precedent; a posX split alone breaks under
// center-arrows-1P where the single lane is centered exactly at 640).
// - True versus (both present): lanes are guaranteed left/right, so the
//   per-renderer posX split is valid — and there is NO cross-side
//   fallback (the other side may be OVERHEAD and must stay flat).
// - Single/doubles (one present): exactly ONE lane pass runs, and its
//   published block is by definition the lane this glow overlays — take
//   whichever side published, ignoring presence's side index (doubles
//   binds side 0 regardless of which pad carded in; a centered single
//   lane publishes under the side its own bind_side chose).
// The constants come from the side's PUBLISHED block (the notes pass
// derives + publishes them every frame — no independent re-derivation
// here; the judge object has no verified mode field), and a glow record
// only exists after a hit, i.e. after lane passes have run.

type JudgeFxRenderFn = unsafe extern "C" fn(this: *mut u8);

static mut JUDGE_FX_HOOK: Option<GenericDetour<JudgeFxRenderFn>> = None;

unsafe extern "C" fn judge_fx_render_cb(this: *mut u8) {
    let hook = match &*addr_of!(JUDGE_FX_HOOK) {
        Some(h) => h,
        None => return,
    };
    if this.is_null() || !super::any_side_latched() {
        return hook.call(this);
    }

    let plan = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        match crate::mods::playfield_styling::read_presence() {
            (false, false) => None,
            (true, true) => {
                let pos_x = *(this.add(OFF_POS_X) as *const f32);
                super::published_constants(if pos_x < X_SPLIT { 0 } else { 1 })
            }
            _ => super::published_constants(0).or_else(|| super::published_constants(1)),
        }
    }))
    .unwrap_or(None);

    let c = match plan {
        Some(c) => c,
        None => return hook.call(this),
    };

    let cl = render_notes_hook::active_command_list();
    if cl.is_null() {
        return hook.call(this);
    }
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        emit_persp_constants(cl, &c);
    }));
    let start = render_notes_hook::write_ptr(cl);

    hook.call(this);

    if start.is_null() {
        return;
    }
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let end = render_notes_hook::write_ptr(cl);
        if end.is_null() || end < start {
            return;
        }
        let judge_shader = *(this.add(OFF_JUDGE_FX_SHADER) as *const *const u8);
        let n = rewrite_window(
            start,
            end,
            &[(judge_shader, &WARNED_JUDGE_FX_GATE, "judge-effect")],
        );
        if n > 0 && !LOGGED_JUDGE_FX_PASS.swap(true, Ordering::AcqRel) {
            log_info!(
                "player-perspective: judge-effect pass live (shader {:p}, {} record(s) rewritten)",
                judge_shader,
                n
            );
        }
    }));
}

// ── Raw record emission ─────────────────────────────────────────────

/// Emit the resolved perspective constant block as the pass's c48/c49:
/// c48 = {anchor_y, cx, k, dir}; c49 = {d_min, z0, ty, 0}. `z0` is
/// load-bearing — the perspective VS multiplies its scale by c49.y, so it
/// must always carry the latched value (1.0 for the presets that don't
/// zoom), never 0. `ty` (c49.z) is the receptor-row realignment shift.
unsafe fn emit_persp_constants(cl: *mut u8, c: &super::PerspConstants) {
    emit_set_vs_const_f(
        cl,
        0,
        &[[c.anchor_y, c.cx, c.k, c.dir], [c.d_min, c.z0, c.ty, 0.0]],
    );
}

/// Emit SetVSConstantF(c48+reg_off, n) with the float payload copied inline
/// into the list arena (self-contained: the walker consumes records later on
/// a worker thread). Mirrors the game emitter's layout exactly
/// (`research/ghidra-verification.md` Q1).
unsafe fn emit_set_vs_const_f(cl: *mut u8, reg_off: u32, regs: &[[f32; 4]]) {
    let n = regs.len() as u32;
    if n == 0 {
        return;
    }
    const HEADER: u32 = 0x18;
    let total = HEADER + n * 0x10;

    // Shared arena-bump preamble (mine_render's emitters): size @+0x0C,
    // write ptr @+0x10, arena base @+0x18.
    let size_ptr = cl.add(0x0C) as *mut u32;
    let write_ptr = cl.add(0x10) as *mut *mut u8;
    let base_ptr = cl.add(0x18) as *const *const u8;
    let cmd = *write_ptr;
    *size_ptr += total;
    *write_ptr = (*base_ptr).add(*size_ptr as usize) as *mut u8;

    (cmd as *mut u16).write_unaligned(0x14);
    (cmd.add(2) as *mut u16).write_unaligned(total as u16);
    (cmd.add(4) as *mut u32).write_unaligned(reg_off);
    (cmd.add(8) as *mut u32).write_unaligned(n);
    (cmd.add(0x0C) as *mut u32).write_unaligned(0); // pad (game leaves garbage; we don't)
    let payload = cmd.add(HEADER as usize);
    (cmd.add(0x10) as *mut u64).write_unaligned(payload as u64);
    std::ptr::copy_nonoverlapping(regs.as_ptr() as *const u8, payload, (n * 0x10) as usize);
}
