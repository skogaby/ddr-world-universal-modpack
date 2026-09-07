//! TextWidget — Wraps a native `kt::BmpfontSimpleString` pointer.
//!
//! Created via `widget_renderer::create_text_widget()`. The text widget is allocated
//! by the game's own widget factory and registered in the render list, so it's drawn
//! by the game's native font renderer with proper outlines and glyph rendering.
//!
//! ## Usage
//!
//! ```rust
//! let widget = widget_renderer::create_text_widget().unwrap();
//! widget.set_text("Hello World");
//! widget.set_position(100.0, 50.0);
//! widget.set_color(1.0, 1.0, 1.0, 1.0); // white
//! widget.set_scale(1.5, 1.5);
//! widget.show();
//! ```

use crate::core::memory;

/// The game's own "system text" outline colour — the 25 % grey entry of the
/// static colour ramp (`DAT_18047cb28` on 20260825, `DAT_180443d88` on
/// 20250805; byte-identical on both) that EVERY stock `agcs::BmpString`
/// setup site applies (footer credit/PASELI/ONLINE lines, "PAIRING: OK",
/// version text, …). Pure black reads noticeably heavier at 4K output
/// (one logical unit of outline = 3 px), which is why our widgets looked
/// different from the game's text next to them.
pub const SYSTEM_OUTLINE: (f32, f32, f32, f32) = (0.25, 0.25, 0.25, 1.0);

/// The stock outline width (descriptor default; every system site keeps it).
pub const SYSTEM_OUTLINE_WIDTH: i32 = 1;

/// The stock system-text font scale (`0x3f0a3d71`), applied to x and y by
/// every stock setup site.
pub const SYSTEM_FONT_SCALE: f32 = 0.54;

/// Outline direction bits for `desc+0x70`. The glyph emitter
/// (`FUN_18020dbb0` on 20260825) walks these four bits over the offset
/// table `{(-1,-1), (+1,-1), (-1,+1), (+1,+1)}` — the stamps are the four
/// DIAGONALS in logical 1280×720 units; there are no axis-aligned bits.
pub const OUTLINE_UP_LEFT: u32 = 0x1;
pub const OUTLINE_UP_RIGHT: u32 = 0x2;
pub const OUTLINE_DOWN_LEFT: u32 = 0x4;
pub const OUTLINE_DOWN_RIGHT: u32 = 0x8;
pub const OUTLINE_ALL: u32 = 0xF;

/// Text encoding mode for the native `setText` (vtable[2]).
/// 1 = UTF-8 (raw copy). Other stock values (2/4/6 code-page converts, 8 =
/// wide) are never needed from Rust.
const SET_TEXT_MODE_UTF8: i32 = 1;

/// `kt::BmpfontSimpleString::setText(this, const char* text, int mode)` —
/// vtable slot 2 (`+0x10`). Resizes the game-owned string vector at
/// `desc+0x00..+0x10` on the game heap, copies, marks dirty and re-runs
/// the layout pass. (The widget-system doc used to call this slot
/// `getText`; the footer renderer `FUN_180009630` calls it with
/// `(str, 1)` every frame.)
type SetTextFn = unsafe extern "C" fn(*mut u8, *const u8, i32) -> *const u8;

/// Text alignment within the widget's bounding area.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum TextAlignment {
    Left = 0,
    Center = 1,
    Right = 2,
}

/// A text widget backed by the game's native `kt::BmpfontSimpleString`.
/// Supports multi-line text (use `\n`), color, scale, alignment, and outlines.
pub struct TextWidget {
    native_ptr: *mut u8,
    /// The `agcs::BmpString` WRAPPER registered in the render list (the
    /// render-list node's identity; `native_ptr` is its `child_array[0]`).
    /// Null for widgets constructed without one.
    wrapper: *mut u8,
    destroyed: bool,
}

unsafe impl Send for TextWidget {}
unsafe impl Sync for TextWidget {}

impl TextWidget {
    pub fn new(native_ptr: *mut u8) -> Self {
        Self {
            native_ptr,
            wrapper: std::ptr::null_mut(),
            destroyed: false,
        }
    }

    /// Like [`new`](Self::new), recording the render-list wrapper so the
    /// widget can take part in `widget_renderer::bring_to_front`.
    pub fn with_wrapper(native_ptr: *mut u8, wrapper: *mut u8) -> Self {
        Self {
            native_ptr,
            wrapper,
            destroyed: false,
        }
    }

    pub fn native_ptr(&self) -> *mut u8 {
        self.native_ptr
    }

    /// The render-list wrapper address (see `widget_renderer::bring_to_front`),
    /// or 0 when unknown/destroyed.
    pub fn render_wrapper(&self) -> usize {
        if self.destroyed {
            0
        } else {
            self.wrapper as usize
        }
    }

    fn line_desc(&self) -> *mut u8 {
        unsafe { *(self.native_ptr.add(0x08) as *const *mut u8) }
    }

    fn render_state(&self) -> *mut u8 {
        unsafe { *(self.native_ptr.add(0x10) as *const *mut u8) }
    }

    fn set_dirty(&self) {
        unsafe { memory::write_u8(self.render_state().add(0x68), 1) };
    }

    /// Force the widget dirty (render_state+0x68 — the byte `set_text`
    /// sets). The overlay-draw anchor re-arms this post-render so the
    /// game's walk keeps dispatching the anchor's `wrapper_render` every
    /// frame even when its text never changes.
    pub fn mark_dirty(&self) {
        if !self.destroyed {
            self.set_dirty();
        }
    }

    /// Address of the dirty-flag byte (render_state+0x68), for the
    /// anchor's post-render re-arm. Null when unresolvable.
    pub fn dirty_flag_addr(&self) -> *mut u8 {
        if self.destroyed {
            return std::ptr::null_mut();
        }
        let rs = self.render_state();
        if rs.is_null() {
            return std::ptr::null_mut();
        }
        unsafe { rs.add(0x68) }
    }

    /// Set the widget's text through the game's OWN setter (vtable[2],
    /// UTF-8 mode). The native path resizes the game-heap string vector in
    /// place and re-runs layout, so per-frame updaters (PUS stats, toast,
    /// training readout) no longer leak a VirtualAlloc block per call.
    /// Falls back to the legacy raw-pointer write only if the vtable slot
    /// can't be read (never observed; kept so text still shows).
    pub fn set_text(&self, text: &str) {
        if self.destroyed || self.native_ptr.is_null() {
            return;
        }
        let bytes = text.as_bytes();
        let new_len = bytes.len();

        // NUL-terminated copy on our side; the native setter copies it
        // into game-owned storage, so this buffer is transient.
        let mut owned: Vec<u8> = Vec::with_capacity(new_len + 1);
        owned.extend_from_slice(bytes);
        owned.push(0);

        if let Some(set_text) = self.native_set_text() {
            unsafe { set_text(self.native_ptr, owned.as_ptr(), SET_TEXT_MODE_UTF8) };
            return;
        }

        // Legacy fallback: hand the descriptor a leaked buffer.
        let buf = unsafe { memory::alloc_zeroed(new_len + 1) };
        if buf.is_null() {
            return;
        }
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, new_len);
            *buf.add(new_len) = 0;
        }

        let desc = self.line_desc();
        unsafe {
            memory::write_ptr(desc, buf as *const u8);
            memory::write_ptr(desc.add(0x08), buf.add(new_len) as *const u8);
            memory::write_ptr(desc.add(0x10), buf.add(new_len + 1) as *const u8);
        }

        self.set_dirty();
    }

    /// Resolve `kt::BmpfontSimpleString` vtable[2] (`setText`) from the live
    /// object, range-checked. `None` if any pointer is unreadable.
    fn native_set_text(&self) -> Option<SetTextFn> {
        if !memory::is_readable(self.native_ptr, 8) {
            return None;
        }
        let vtable = unsafe { memory::read_ptr(self.native_ptr) };
        if vtable.is_null() || !memory::is_readable(vtable, 0x18) {
            return None;
        }
        let slot = unsafe { memory::read_ptr(vtable.add(0x10)) };
        if slot.is_null() || !memory::is_readable(slot, 1) {
            return None;
        }
        Some(unsafe { std::mem::transmute::<*const u8, SetTextFn>(slot) })
    }

    pub fn set_position(&self, x: f32, y: f32) {
        if self.destroyed {
            return;
        }
        let desc = self.line_desc();
        unsafe {
            memory::write_f32(desc.add(0x4C), x);
            memory::write_f32(desc.add(0x50), y);
        }
    }

    pub fn set_color(&self, r: f32, g: f32, b: f32, a: f32) {
        if self.destroyed {
            return;
        }
        let desc = self.line_desc();
        unsafe {
            memory::write_f32(desc.add(0x20), r);
            memory::write_f32(desc.add(0x24), g);
            memory::write_f32(desc.add(0x28), b);
            memory::write_f32(desc.add(0x2C), a);
        }
    }

    pub fn set_scale(&self, x: f32, y: f32) {
        if self.destroyed {
            return;
        }
        let desc = self.line_desc();
        unsafe {
            memory::write_f32(desc.add(0x58), x);
            memory::write_f32(desc.add(0x5C), y);
        }
    }

    /// Horizontal per-line alignment about `set_position`'s x: the native
    /// renderer offsets each line by its own PRE-MEASURED width (exact
    /// glyph metrics from the layout pass) — Left = 0 offset, Center =
    /// −width/2, Right = −width. Text can change freely; centering stays
    /// exact with no caller-side width estimation.
    ///
    /// Field map (render fn, cabinet-verified 2026-08-13): HORIZONTAL
    /// alignment is `desc+0xA8`; `desc+0xAC` is the VERTICAL block
    /// alignment (this method originally wrote +0xAC, which is why
    /// "Center" appeared to left-anchor).
    pub fn set_alignment(&self, alignment: TextAlignment) {
        if self.destroyed {
            return;
        }
        let desc = self.line_desc();
        unsafe { memory::write_i32(desc.add(0xA8), alignment as i32) };
    }

    /// Outline: all four diagonal stamps (`OUTLINE_ALL`) at `width` logical
    /// units in the given colour. The emitter multiplies the outline alpha
    /// by the text alpha (`FUN_18020d5f0`), so a fading widget's outline
    /// follows `set_color`'s alpha with no extra bookkeeping.
    pub fn set_outline(&self, r: f32, g: f32, b: f32, a: f32, width: i32) {
        if self.destroyed {
            return;
        }
        let desc = self.line_desc();
        unsafe {
            memory::write_u32(desc.add(0x70), OUTLINE_ALL);
            memory::write_i32(desc.add(0x74), width);
            memory::write_f32(desc.add(0x78), r);
            memory::write_f32(desc.add(0x7C), g);
            memory::write_f32(desc.add(0x80), b);
            memory::write_f32(desc.add(0x84), a);
        }
    }

    /// The game's system-text outline (25 % grey, width 1) — see
    /// [`SYSTEM_OUTLINE`]. Apply this instead of black to match the footer
    /// text ("ONLINE", "EXTRA PASELI", credits) the widget sits beside.
    pub fn set_system_outline(&self) {
        let (r, g, b, a) = SYSTEM_OUTLINE;
        self.set_outline(r, g, b, a, SYSTEM_OUTLINE_WIDTH);
    }

    /// The full stock system-text recipe (`FUN_1800092d0` per object):
    /// white, scale 0.54, all-diagonals outline in the 25 % grey.
    pub fn set_system_style(&self) {
        self.set_color(1.0, 1.0, 1.0, 1.0);
        self.set_scale(SYSTEM_FONT_SCALE, SYSTEM_FONT_SCALE);
        self.set_system_outline();
    }

    /// Choose which of the four diagonal outline stamps render (`desc+0x70`,
    /// OR of the `OUTLINE_*` bits; 0 disables the outline entirely). The
    /// colour/width set by `set_outline` are kept.
    pub fn set_outline_mask(&self, mask: u32) {
        if self.destroyed {
            return;
        }
        let desc = self.line_desc();
        unsafe { memory::write_u32(desc.add(0x70), mask & OUTLINE_ALL) };
    }

    /// Blend mode selector (`desc+0xA0`), consumed by the render prep
    /// (`FUN_18020d5f0`) as the blend-state record: `2 → 0x1220225`,
    /// `0x22 → 0x1220265`, anything else → the default `0x1220625`
    /// (standard alpha blend). Stock text never changes it.
    pub fn set_blend_mode(&self, mode: i32) {
        if self.destroyed {
            return;
        }
        let desc = self.line_desc();
        unsafe { memory::write_i32(desc.add(0xA0), mode) };
    }

    /// Rotation angle (`desc+0xBC`, radians). The render prep computes
    /// `sin(a + π/2)` / `sin(a)` into `rs+0x11C/+0x120` (cos/sin) and the
    /// glyph emitter rotates every quad (and the outline stamps) about the
    /// widget origin; 0 (the default) is the identity.
    pub fn set_rotation(&self, radians: f32) {
        if self.destroyed {
            return;
        }
        let desc = self.line_desc();
        unsafe { memory::write_f32(desc.add(0xBC), radians) };
    }

    pub fn show(&self) {
        if self.destroyed {
            return;
        }
        let desc = self.line_desc();
        unsafe { memory::write_u8(desc.add(0x49), 1) };
    }

    pub fn hide(&self) {
        if self.destroyed {
            return;
        }
        let desc = self.line_desc();
        unsafe { memory::write_u8(desc.add(0x49), 0) };
    }

    pub fn destroy(&mut self) {
        if self.destroyed {
            return;
        }
        self.hide();
        self.destroyed = true;
    }
}
