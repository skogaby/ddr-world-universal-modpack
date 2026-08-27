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
    destroyed: bool,
}

unsafe impl Send for TextWidget {}
unsafe impl Sync for TextWidget {}

impl TextWidget {
    pub fn new(native_ptr: *mut u8) -> Self {
        Self {
            native_ptr,
            destroyed: false,
        }
    }

    pub fn native_ptr(&self) -> *mut u8 {
        self.native_ptr
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

    pub fn set_text(&self, text: &str) {
        if self.destroyed {
            return;
        }
        let bytes = text.as_bytes();
        let new_len = bytes.len();

        let buf = unsafe { memory::alloc_zeroed(new_len + 1) };
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

    pub fn set_outline(&self, r: f32, g: f32, b: f32, a: f32, width: i32) {
        if self.destroyed {
            return;
        }
        let desc = self.line_desc();
        unsafe {
            memory::write_u32(desc.add(0x70), 0x0F); // full outline (4 directions)
            memory::write_i32(desc.add(0x74), width);
            memory::write_f32(desc.add(0x78), r);
            memory::write_f32(desc.add(0x7C), g);
            memory::write_f32(desc.add(0x80), b);
            memory::write_f32(desc.add(0x84), a);
        }
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
