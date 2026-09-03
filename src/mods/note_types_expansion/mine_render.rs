//! Dedicated mine render pass.
//!
//! Subscribes to the shared `render_notes` dispatcher
//! (`services::render_notes_hook`) and appends a mine-specific pass after
//! the vanilla shock + normal passes complete (post callback @ Normal).
//! The mine pass renders two layers per mine:
//!
//! **Layer 1 — Silver glyph**: A monochrome arrow-atlas sample using the
//! engine's default shader (the same shader the shock-arrow glyph uses).
//! The default shader applies the palette color-shift animation, producing
//! the silver shimmer that distinguishes shock-type visuals from the
//! colored regular-arrow palette.
//!
//! **Layer 2 — Lightning overlay**: An additive-blend pass using the mine
//! texture (a 2×4 animation grid at 192×384). The shock-cadence UV
//! formula `frame = (musicCount / 33) % 8` selects the current animation
//! cell. This matches the engine's own lightning overlay on shock arrows,
//! but sized to a single panel (96×96) instead of full-lane width.
//!
//! Both layers inherit the ArrowRenderer's current appearance (HIDDEN /
//! SUDDEN / STEALTH alpha), reverse state, speed, and boost from the
//! `this` pointer, which is the same ArrowRenderer object the vanilla
//! passes just used.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::OnceLock;

use crate::core::memory;
use crate::mods::note_types_expansion::hooks::registry;
use crate::mods::note_types_expansion::texture_loader::MineTextureLoader;
use crate::services::render_notes_hook;
use crate::types::game_note::{for_each_result, kind, GameNote};
use crate::{log_info, log_warn};

// ── ArrowRenderer field offsets (observed via Ghidra) ────────────

const ARROW_SIZE: f32 = 96.0;

/// ArrowRenderer member offsets, confirmed from Ghidra on the
/// render_notes function body (field reads via `[RSI + offset]`).
mod actor {
    /// Arrow atlas gs::TextureData pointer (from the agcs::Sprite base).
    pub const ARROW_TEXTURE: usize = 0x20;
    /// Arrow shader pointer (gs::Shader*). Used for the colored regular-
    /// arrow palette — distinct from the default shader used for shock/mine.
    pub const ARROW_SHADER: usize = 0xC0;
    /// Blend mode enum (u32). 1 = SRC_INVSRC (normal), 2 = SRC_ONE (additive).
    pub const BLEND_MODE: usize = 0x2C;
    /// Y position offset (f32). Used as the base Y for the spot row.
    pub const POS_Y: usize = 0x34;
    /// Texture UV array (4 × f32) at +0x54..+0x64 on the ArrowSprite.
    pub const UV_BASE: usize = 0x54;
    /// Color RGBA bytes at +0x64..+0x68.
    pub const COLOR: usize = 0x64;
    /// Appearance flag byte.
    pub const APPEARANCE_FLAG: usize = 0x68;
    /// Rotation angle (f32) — set by set_direction (writes to this+0x50).
    pub const TWIST: usize = 0x50;
    /// Speed (f32) — scroll multiplier.
    pub const SPEED: usize = 0xA0;
    /// Boost enum (i32). 0=NORMAL, 1=BOOST, 2=BRAKE, 3=WAVE.
    pub const BOOST: usize = 0xA4;
    /// Beat count (i32) — current playhead in beat space.
    pub const BEAT_COUNT: usize = 0xA8;
    /// Music count (i32) — current playhead in music-count space.
    pub const MUSIC_COUNT: usize = 0xAC;
    /// Mode enum (i32). 0=SINGLE, 1=DOUBLE.
    pub const MODE: usize = 0xB0;
    /// Results vector reference — begin pointer at +0xB8, stored as
    /// a reference (pointer to the vector struct). The vector struct
    /// has begin at +0x00 and end at +0x08.
    pub const RESULTS_REF: usize = 0xB8;
    /// Judged option enum.
    pub const JUDGED: usize = 0xEC;
    /// Offset Y (i32) — vertical pixel offset for the spot row.
    pub const OFFSET_Y: usize = 0xF4;
}

/// Blend mode values written to actor::BLEND_MODE.
const BLEND_SRC_INVSRC: u32 = 1;
const BLEND_SRC_ONE: u32 = 2;

// ── Arrow shape resolution ──────────────────────────────────────
//
// The per-side player-work table stores one wrapper pointer per play
// side (1P, 2P). Each wrapper exposes a PlayerWork pointer at its
// first qword, and the PlayerWork object embeds the player's Option
// struct at +0xE0. The arrow-shape field sits inside that inlined
// Option struct at +0x60 and is an i32 in the range 0..=7.
//
// Offsets confirmed via Ghidra:
//   - GamePlayActor[+0x84] = playSide (i32), already used by
//     `mods::autoplay` for the per-side AutoFootPanel swap.
//   - wrapper[+0x00] = PlayerWork* — observed in the accessor
//     anchored by the `player_work_table_anchor` signature (the
//     deref-and-check-byte sequence reads the first qword of each
//     table entry).
//   - PlayerWork[+0xE0] = Option (inlined) — observed in the
//     gameplay asset-loader path that formats the shock-effect
//     texture name, and echoed by the Option vtable getter for the
//     arrow-shape field (`MOV EAX, [RCX+0x60]; RET`, confirming the
//     shape field is at +0x60 inside the Option struct).
//
// The shape is locked at the moment the player enters a song (the
// engine exposes no in-song option editor), so we resolve once on
// the first pre-judge callback of each chart and cache the result
// until `reset_cache()` is called from the scene-exit dispatcher.

const ACTOR_PLAY_SIDE: usize = 0x84;
const WRAPPER_PLAYER_WORK: usize = 0x00;
// PlayerWork's inlined Option offset is build-dependent (0xE0 / 0xF0) —
// `stage_records::player_option_offset()`.
const OPTION_ARROW_SHAPE: usize = 0x60;
const MAX_ARROW_SHAPE: i32 = 7;

/// Shock-size default used when the pointer chain can't be walked
/// (any null link, an out-of-range playSide, or an out-of-range
/// shape value). 0 maps to the LARGE variant via the shock-size
/// table in `texture_loader` — matching the engine's own fallback
/// when the Option struct is unpopulated.
const FALLBACK_ARROW_SHAPE: u32 = 0;

/// Sentinel stored in `CACHED_ARROW_SHAPE` to indicate "not yet
/// resolved for the current chart". `u32::MAX` is outside the valid
/// arrow-shape range so it can't collide with a real resolved value.
const ARROW_SHAPE_UNRESOLVED: u32 = u32::MAX;

static CACHED_ARROW_SHAPE: AtomicU32 = AtomicU32::new(ARROW_SHAPE_UNRESOLVED);

// ── Function pointer types ──────────────────────────────────────

/// Per-quad sprite filler (final overload).
/// (this, &sprite, x, y, w, h, &uv[4], twist, &color)
type RenderSpriteFinalFn = unsafe extern "C" fn(
    this: *mut u8,
    sprite: *mut u8,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    uv: *const f32,
    twist: f32,
    color: *const u8,
);

/// Direction setter. (this, dir)
type SetDirectionFn = unsafe extern "C" fn(this: *mut u8, dir: i32);

/// Render state flusher. (this) — reads blend mode from this+0x2C
/// and emits the corresponding CommandList command.
type SetRenderStateFn = unsafe extern "C" fn(this: *mut u8);

/// Scroll-Y pure function. Args: (dBeatCount, speed_int, boost, musicCount).
/// Speed is passed as an integer (speed × 100 — 275 = 2.75x).
/// Returns the pixel offset from the spot row as a float in XMM0.
type GetOffsetYFn =
    unsafe extern "C" fn(d_beat_count: i64, speed: i32, boost: i32, music_count: i32) -> f32;

// ── Statics ─────────────────────────────────────────────────────

struct RenderContext {
    render_sprite: RenderSpriteFinalFn,
    set_direction: SetDirectionFn,
    set_render_state: SetRenderStateFn,
    get_offset_y: GetOffsetYFn,
    screen_renderer_state: *const u8,
    default_shader: *const u8,
    player_work_table: *const u8,
}

unsafe impl Send for RenderContext {}
unsafe impl Sync for RenderContext {}

static RENDER_CTX: OnceLock<RenderContext> = OnceLock::new();

/// Texture loader reference — set by the mod's enable() path.
/// The mine render pass reads this to get the TextureData pointer
/// for the current arrow shape.
static mut TEXTURE_LOADER: Option<*const MineTextureLoader> = None;

/// Register the mine pass on the shared `render_notes` dispatcher and stash
/// resolved function pointers. Returns false on failure.
#[allow(clippy::too_many_arguments)]
pub fn install(
    render_sprite_addr: *const u8,
    set_direction_addr: *const u8,
    set_render_state_addr: *const u8,
    get_offset_y_addr: *const u8,
    screen_renderer_state_addr: *const u8,
    default_shader_addr: *const u8,
    player_work_table_addr: *const u8,
) -> bool {
    if render_sprite_addr.is_null() || set_direction_addr.is_null() {
        log_warn!("mine_render: install called with null address(es)");
        return false;
    }

    let ctx = RenderContext {
        render_sprite: unsafe {
            std::mem::transmute::<*const u8, RenderSpriteFinalFn>(render_sprite_addr)
        },
        set_direction: unsafe {
            std::mem::transmute::<*const u8, SetDirectionFn>(set_direction_addr)
        },
        set_render_state: unsafe {
            std::mem::transmute::<*const u8, SetRenderStateFn>(set_render_state_addr)
        },
        get_offset_y: unsafe { std::mem::transmute::<*const u8, GetOffsetYFn>(get_offset_y_addr) },
        screen_renderer_state: screen_renderer_state_addr,
        default_shader: default_shader_addr,
        player_work_table: player_work_table_addr,
    };
    if RENDER_CTX.set(ctx).is_err() {
        log_warn!("mine_render: render context already set");
        return false;
    }

    // Post @ Normal: the mine pass appends records after the vanilla passes.
    // player_perspective's window rewrite runs post @ Late so the mine
    // records fall inside its captured window.
    match render_notes_hook::register_post(render_notes_hook::Priority::Normal, mine_pass_post) {
        Some(_) => {
            log_info!("mine_render: registered mine pass on render_notes dispatcher");
            true
        }
        None => {
            log_warn!("mine_render: render_notes dispatcher unavailable");
            false
        }
    }
}

/// Set the texture loader reference for the mine render pass.
/// Called from the mod's enable() path.
///
/// # Safety
/// The pointer must remain valid for the lifetime of the mod.
pub unsafe fn set_texture_loader(loader: *const MineTextureLoader) {
    TEXTURE_LOADER = Some(loader);
}

// ── Arrow shape cache ──────────────────────────────────────────

/// Walk the per-side player-work table and return the currently-
/// selected arrow shape for the given gameplay actor. Returns
/// `FALLBACK_ARROW_SHAPE` if any link in the chain is null, the
/// playSide is outside 0..=1, or the shape field value is outside
/// 0..=`MAX_ARROW_SHAPE`.
unsafe fn resolve_arrow_shape(actor: *mut u8, ctx: &RenderContext) -> u32 {
    if actor.is_null() || ctx.player_work_table.is_null() {
        log_warn!(
            "mine_render: resolve_arrow_shape: null input (actor={:p}, table={:p})",
            actor,
            ctx.player_work_table,
        );
        return FALLBACK_ARROW_SHAPE;
    }
    let play_side = memory::read_i32(actor.add(ACTOR_PLAY_SIDE));
    if !(0..=1).contains(&play_side) {
        log_warn!(
            "mine_render: resolve_arrow_shape: playSide out of range ({})",
            play_side
        );
        return FALLBACK_ARROW_SHAPE;
    }
    let entry = ctx
        .player_work_table
        .add(play_side as usize * std::mem::size_of::<*const u8>());
    let wrapper = *(entry as *const *const u8);
    if wrapper.is_null() {
        log_warn!(
            "mine_render: resolve_arrow_shape: wrapper null (table={:p}, playSide={}, entry={:p})",
            ctx.player_work_table,
            play_side,
            entry,
        );
        return FALLBACK_ARROW_SHAPE;
    }
    let player_work = *(wrapper.add(WRAPPER_PLAYER_WORK) as *const *const u8);
    if player_work.is_null() {
        log_warn!(
            "mine_render: resolve_arrow_shape: player_work null (wrapper={:p})",
            wrapper
        );
        return FALLBACK_ARROW_SHAPE;
    }
    let Some(option_off) = crate::services::stage_records::player_option_offset() else {
        return FALLBACK_ARROW_SHAPE;
    };
    let option = player_work.add(option_off);
    let shape = memory::read_i32(option.add(OPTION_ARROW_SHAPE));
    if !(0..=MAX_ARROW_SHAPE).contains(&shape) {
        log_warn!(
            "mine_render: resolve_arrow_shape: shape out of range ({})",
            shape
        );
        return FALLBACK_ARROW_SHAPE;
    }
    shape as u32
}

/// Populate the cached arrow shape from a gameplay actor pointer.
/// Intended to be called every frame from the mod's pre-judge
/// callback — it early-exits after the first successful resolve,
/// so repeated calls are cheap (one atomic load).
///
/// # Safety
/// `actor` must be a valid GamePlayActor pointer or null. A null
/// actor causes the function to fall back to the LARGE variant and
/// cache that.
pub unsafe fn prime_arrow_shape(actor: *mut u8) {
    if CACHED_ARROW_SHAPE.load(Ordering::Relaxed) != ARROW_SHAPE_UNRESOLVED {
        return;
    }
    let ctx = match RENDER_CTX.get() {
        Some(c) => c,
        None => return,
    };
    let shape = resolve_arrow_shape(actor, ctx);
    CACHED_ARROW_SHAPE.store(shape, Ordering::Relaxed);
    log_info!(
        "mine_render: arrow_shape resolved to {} (size variant {})",
        shape,
        MineTextureLoader::size_index_for_shape(shape),
    );
}

/// Reset the cached arrow shape to the "unresolved" sentinel. The
/// mod's scene-exit callback invokes this so the next chart re-
/// resolves the shape against the current Option state.
pub fn reset_cache() {
    CACHED_ARROW_SHAPE.store(ARROW_SHAPE_UNRESOLVED, Ordering::Relaxed);
}

/// Read the cached arrow shape, returning the fallback if the
/// pre-judge callback hasn't primed the cache yet for this chart.
fn cached_arrow_shape() -> u32 {
    match CACHED_ARROW_SHAPE.load(Ordering::Relaxed) {
        ARROW_SHAPE_UNRESOLVED => FALLBACK_ARROW_SHAPE,
        v => v,
    }
}

// ── Dispatcher callback ─────────────────────────────────────────

/// Post @ Normal on the render_notes dispatcher: the vanilla passes (shock +
/// normal) have already run; append the mine pass if any mines are active.
fn mine_pass_post(this: *mut u8) {
    // 1. Check if we have mines to render.
    let has_mines = match registry().lock() {
        Ok(g) => !g.is_empty() && g.handles_kind(kind::MINE),
        Err(_) => false,
    };
    if !has_mines {
        return;
    }

    // 2. Emit the mine pass.
    let ctx = match RENDER_CTX.get() {
        Some(c) => c,
        None => return,
    };
    unsafe {
        let loader = match *std::ptr::addr_of!(TEXTURE_LOADER) {
            Some(l) => &*l,
            None => return,
        };
        emit_mine_pass(this, ctx, loader);
    }
}

// ── Mine pass emission ──────────────────────────────────────────

/// Round to nearest integer (matching the engine's arrow position correction).
fn correct_arrow_pos(pos: f32) -> f32 {
    pos.round()
}

/// Emit the dedicated mine render pass (Layer 1 + Layer 2).
unsafe fn emit_mine_pass(this: *mut u8, ctx: &RenderContext, loader: &MineTextureLoader) {
    // Read ArrowRenderer state. Note: speed is stored as an integer
    // (speed*100 — e.g. 275 = 2.75x), NOT as a float.
    let beat_count = memory::read_i32(this.add(actor::BEAT_COUNT));
    let music_count = memory::read_i32(this.add(actor::MUSIC_COUNT));
    let speed = memory::read_i32(this.add(actor::SPEED));
    let boost = memory::read_i32(this.add(actor::BOOST));
    let offset_y = memory::read_i32(this.add(actor::OFFSET_Y));
    let judged = memory::read_i32(this.add(actor::JUDGED));
    // Top-window bound: stock 720.0 (the DDR render height), or the
    // playfield-styling mod's extended `720/min(scale)` while a shrunken
    // song is latched. The mod detours `render_sprite_final` — the same
    // entry point this pass calls — so mine quads inherit its scale/opacity
    // transform automatically; this window widen is the only integration
    // needed to keep shrunken mines from popping in mid-screen, exactly in
    // lockstep with the game's own (patched) collector cull. The bottom
    // margin check below stays raw on purpose — it mirrors the collector's
    // own unscaled bottom cull, so mines pop out exactly when arrows do.
    let render_height = crate::mods::playfield_styling::cull_bound();

    // Read the Results vector from the ArrowRenderer's reference.
    let results_ref = *(this.add(actor::RESULTS_REF) as *const *const u8);
    if results_ref.is_null() {
        return;
    }
    let results_begin = *(results_ref as *const *mut u8);
    let results_end = *(results_ref.add(8) as *const *mut u8);
    if results_begin.is_null() || results_end.is_null() || results_end <= results_begin {
        return;
    }

    // Collect mine entries that are in the render window.
    // Each entry: (dir, y_position, result_ptr, note_ptr)
    let mut mine_quads: Vec<(i32, f32, *mut u8, *mut GameNote)> = Vec::new();

    for_each_result(results_begin, results_end, |entry, note| {
        let n = &*note;
        if n.kind != kind::MINE {
            return;
        }

        let d_beat_count = (n.beat_count - beat_count) as i64;
        let fy = (ctx.get_offset_y)(d_beat_count, speed, boost, music_count);

        // Off-screen check (below the render area).
        if fy > render_height {
            return;
        }

        // Off-screen check (above, with some margin for passed notes).
        if fy + ARROW_SIZE + offset_y as f32 + ARROW_SIZE < 0.0 {
            return;
        }

        // Visibility check: skip judged mines unless REMAIN option is set.
        let grade = memory::read_u32(entry.add(0x0C));
        if grade != 0xFF
            && judged != 1 // 1 = REMAIN
            && grade != 5
        {
            // grade 5 = MISS (show missed mines)
            return;
        }

        // Find which panel this mine is on.
        for dir in 0..8i32 {
            if n.state[dir as usize] != 0 {
                let y = correct_arrow_pos(fy);
                mine_quads.push((dir, y, entry, note));
            }
        }
    });

    if mine_quads.is_empty() {
        return;
    }

    // Get the CommandList.
    let cl = get_command_list(ctx.screen_renderer_state);
    if cl.is_null() {
        log_warn!("mine_render: CommandList is null");
        return;
    }

    // ── Layer 1: Silver shock-arrow glyph ───────────────────────
    let default_shader = *(ctx.default_shader as *const *const u8);
    if default_shader.is_null() {
        log_warn!("mine_render: default shader is null");
        return;
    }
    emit_set_shader(cl, default_shader);

    // Allocate sprite batch.
    let sprites = emit_draw_rotate_sprites(cl, mine_quads.len() as u32);
    if sprites.is_null() {
        return;
    }

    // Fill each sprite entry.
    for (i, &(dir, y, _entry, _note)) in mine_quads.iter().enumerate() {
        let x = ARROW_SIZE * dir as f32;
        let sprite_ptr = sprites.add(i * 0x34);

        // The shock-arrow columns of the arrow atlas only contain two
        // shape variants (a "left" and a "down"), not four. The engine's
        // shock pass pairs variant selection with rotation to produce
        // each of the four panel directions:
        //   left  → variant 0, 0°
        //   down  → variant 1, 0°
        //   up    → variant 1, 180°
        //   right → variant 0, 180°
        // We mirror that mapping here so each panel gets a distinct look.
        let dir2 = dir % 4;
        let dir_for_rotation = (dir2 / 2) * 3;
        (ctx.set_direction)(this, dir_for_rotation);
        let offset = if dir2 == 0 || dir2 == 3 { 0 } else { 1 };
        let tex_data = *(this.add(actor::ARROW_TEXTURE) as *const *const u8);
        let (tw, th) = if !tex_data.is_null() {
            let w = *(tex_data.add(0x08) as *const u16) as f32;
            let h = *(tex_data.add(0x0A) as *const u16) as f32;
            (w, h)
        } else {
            (384.0, 96.0) // fallback
        };
        let u0 = ARROW_SIZE * (2.0 + offset as f32) / tw;
        let v0 = 0.0f32;
        let u1 = u0 + ARROW_SIZE / tw;
        let v1 = ARROW_SIZE / th;
        let uv = [u0, v0, u1, v1];

        // Read color from the ArrowRenderer.
        let color = [
            memory::read_u8(this.add(actor::COLOR)),
            memory::read_u8(this.add(actor::COLOR + 1)),
            memory::read_u8(this.add(actor::COLOR + 2)),
            memory::read_u8(this.add(actor::COLOR + 3)),
        ];

        let twist = f32::from_bits(memory::read_u32(this.add(actor::TWIST)));

        (ctx.render_sprite)(
            this,
            sprite_ptr,
            x,
            y,
            ARROW_SIZE,
            ARROW_SIZE,
            uv.as_ptr(),
            twist,
            color.as_ptr(),
        );
    }

    // ── Layer 2: Lightning overlay ──────────────────────────────
    // Arrow-shape selection: the cache is primed each frame by the
    // mod's pre-judge callback (which receives the gameplay actor
    // pointer), so by the time the first render_notes fires the
    // value is already resolved for this chart.
    let mine_td = loader.get_texture_data_for_shape(cached_arrow_shape());
    if mine_td.is_null() {
        // Texture not loaded yet — skip Layer 2 gracefully.
        return;
    }

    // Switch to additive blend.
    memory::write_u32(this.add(actor::BLEND_MODE), BLEND_SRC_ONE);
    (ctx.set_render_state)(this);

    // Bind the mine texture.
    let mine_handle = memory::read_u32(mine_td.add(0x04));
    let param = [1.0f32, 1.0, 0.0, 0.0];
    emit_set_texture(cl, 0, mine_handle, &param);

    // Allocate sprite batch for lightning.
    let sprites2 = emit_draw_rotate_sprites(cl, mine_quads.len() as u32);
    if !sprites2.is_null() {
        // Mine texture is 192×384, 2-col × 4-row grid of 96×96 frames.
        let u = ARROW_SIZE / 192.0; // 0.5
        let v = ARROW_SIZE / 384.0; // 0.25
        let frame = ((music_count / 33) % 8) as u32;
        let uv2 = [
            (frame % 2) as f32 * u,
            (frame / 2) as f32 * v,
            (frame % 2) as f32 * u + u,
            (frame / 2) as f32 * v + v,
        ];

        let color = [
            memory::read_u8(this.add(actor::COLOR)),
            memory::read_u8(this.add(actor::COLOR + 1)),
            memory::read_u8(this.add(actor::COLOR + 2)),
            memory::read_u8(this.add(actor::COLOR + 3)),
        ];

        for (i, &(dir, y, _entry, _note)) in mine_quads.iter().enumerate() {
            let x = ARROW_SIZE * dir as f32;
            let sprite_ptr = sprites2.add(i * 0x34);

            // Lightning overlay has no rotation (twist = 0).
            (ctx.render_sprite)(
                this,
                sprite_ptr,
                x,
                y,
                ARROW_SIZE,
                ARROW_SIZE,
                uv2.as_ptr(),
                0.0,
                color.as_ptr(),
            );
        }
    }

    // Restore normal blend.
    memory::write_u32(this.add(actor::BLEND_MODE), BLEND_SRC_INVSRC);
    (ctx.set_render_state)(this);

    // Restore the arrow shader and arrow atlas texture so downstream
    // draw calls (spot renderer, etc.) that inherit CommandList state
    // see the same bindings the vanilla normal-arrow pass would have
    // left behind.
    let arrow_shader = *(this.add(actor::ARROW_SHADER) as *const *const u8);
    if !arrow_shader.is_null() {
        emit_set_shader(cl, arrow_shader);
    }
    let arrow_tex = *(this.add(actor::ARROW_TEXTURE) as *const *const u8);
    if !arrow_tex.is_null() {
        let handle = memory::read_u32(arrow_tex.add(0x04));
        let param = [1.0f32, 1.0, 0.0, 0.0];
        emit_set_texture(cl, 0, handle, &param);
    }
}

// ── CommandList helpers ─────────────────────────────────────────

/// Read the active CommandList pointer from the ScreenRenderer state global.
/// Layout: `[global+0x68]` = active index, `[global + index*8 + 0x40]` = CommandList*.
unsafe fn get_command_list(screen_renderer_state: *const u8) -> *mut u8 {
    let state = *(screen_renderer_state as *const *const u8);
    if state.is_null() {
        return std::ptr::null_mut();
    }
    let index = *(state.add(0x68) as *const i32);
    *(state.add(0x40 + index as usize * 8) as *const *mut u8)
}

/// Emit a SetShader command into the CommandList.
/// Format: `{u16 tag=0x13, u16 size=0x18, u32 pad=0, u64 shader_ptr, u32 param=0}`
unsafe fn emit_set_shader(cl: *mut u8, shader: *const u8) {
    let size_ptr = cl.add(0x0C) as *mut u32;
    let write_ptr = cl.add(0x10) as *mut *mut u8;
    let base_ptr = cl.add(0x18) as *const *const u8;

    let cmd = *write_ptr;
    *size_ptr += 0x18;
    *write_ptr = (*base_ptr).add(*size_ptr as usize) as *mut u8;

    *(cmd as *mut u16) = 0x13; // tag
    *(cmd.add(2) as *mut u16) = 0x18; // size
    *(cmd.add(4) as *mut u32) = 0; // pad
    *(cmd.add(8) as *mut u64) = shader as u64; // shader ptr
    *(cmd.add(0x10) as *mut u32) = 0; // param
}

/// Emit a SetTexture command into the CommandList.
/// Format: `{u16 tag=0x11, u16 size=0x1C, u32 slot, u32 tex_handle, float[4] param}`
unsafe fn emit_set_texture(cl: *mut u8, slot: u32, tex_handle: u32, param: &[f32; 4]) {
    let size_ptr = cl.add(0x0C) as *mut u32;
    let write_ptr = cl.add(0x10) as *mut *mut u8;
    let base_ptr = cl.add(0x18) as *const *const u8;

    let cmd = *write_ptr;
    *size_ptr += 0x1C;
    *write_ptr = (*base_ptr).add(*size_ptr as usize) as *mut u8;

    *(cmd as *mut u16) = 0x11; // tag
    *(cmd.add(2) as *mut u16) = 0x1C; // size
    *(cmd.add(4) as *mut u32) = slot;
    *(cmd.add(8) as *mut u32) = tex_handle;
    *(cmd.add(0x0C) as *mut f32) = param[0];
    *(cmd.add(0x10) as *mut f32) = param[1];
    *(cmd.add(0x14) as *mut f32) = param[2];
    *(cmd.add(0x18) as *mut f32) = param[3];
}

/// Emit a DrawRotateSprites command and return a pointer to the sprite
/// array. Each sprite is 0x34 bytes.
/// Format: `{u16 tag=0x04, u16 size=0x10+count*0x34, u32 count, u64 sprite_array_ptr}`
unsafe fn emit_draw_rotate_sprites(cl: *mut u8, count: u32) -> *mut u8 {
    if count == 0 {
        return std::ptr::null_mut();
    }

    let size_ptr = cl.add(0x0C) as *mut u32;
    let write_ptr = cl.add(0x10) as *mut *mut u8;
    let base_ptr = cl.add(0x18) as *const *const u8;

    // Header (0x10 bytes).
    let cmd = *write_ptr;
    let sprite_bytes = count * 0x34;
    let total_size = 0x10u32 + sprite_bytes;
    *size_ptr += total_size;
    *write_ptr = (*base_ptr).add(*size_ptr as usize) as *mut u8;

    *(cmd as *mut u16) = 0x04; // tag
    *(cmd.add(2) as *mut u16) = total_size as u16; // size

    // The sprite array is allocated from the CommandList arena right
    // after the header. The header's sprite_array_ptr field points to
    // the first sprite entry.
    let sprite_array = cmd.add(0x10);

    *(cmd.add(4) as *mut u32) = count;
    *(cmd.add(8) as *mut u64) = sprite_array as u64;

    // Zero the sprite array so any unfilled entries are invisible.
    std::ptr::write_bytes(sprite_array, 0, sprite_bytes as usize);

    sprite_array
}
