//! ImageWidget — Wraps a native `agcs::Sprite` struct registered in the game's render list.
//!
//! Created via `widget_renderer::create_image_widget(config)`. The sprite is allocated
//! in game memory and registered in the game's render list, so it's drawn by the game's
//! own render pipeline alongside all other game UI — no per-frame draw calls needed.
//!
//! ## Texture loading
//!
//! If `texture_name` is set in the config, texture resolution happens automatically
//! on the game thread in the background. The widget starts hidden and the texture is
//! resolved via the BM2D `get_bitmap_info` callback. The mod controls visibility
//! via `show()`/`hide()`.
//!
//! ## Sprite memory layout (0x68 bytes)
//!
//! | Offset | Type | Field |
//! |--------|------|-------|
//! | +0x00 | ptr | vtable (agcs::Sprite) |
//! | +0x08 | u32 | z-order (0x7FFFFFFF = front) |
//! | +0x10 | u16 | flags |
//! | +0x12 | u8 | visible (0=hidden, 1=shown) |
//! | +0x28 | i32 | texture bind ID |
//! | +0x2C | i32 | blend mode (0=none, 1=alpha) |
//! | +0x30 | f32 | x position |
//! | +0x34 | f32 | y position |
//! | +0x38 | f32 | width |
//! | +0x3C | f32 | height |
//! | +0x40 | f32 | scale X |
//! | +0x44 | f32 | scale Y |
//! | +0x48 | f32 | anchor X |
//! | +0x4C | f32 | anchor Y |
//! | +0x50 | f32 | rotation (radians) |
//! | +0x54 | f32 | UV left |
//! | +0x58 | f32 | UV top |
//! | +0x5C | f32 | UV right |
//! | +0x60 | f32 | UV bottom |
//! | +0x64 | u32 | color (ABGR packed) |

use crate::core::memory;

/// Configuration for creating an image widget. All fields have sensible defaults
/// via `Default::default()` — only set what you need.
pub struct ImageWidgetConfig {
    /// X position in screen coordinates (0 = left edge, 1280 = right edge).
    pub x: f32,
    /// Y position in screen coordinates (0 = top edge, 720 = bottom edge).
    pub y: f32,
    /// Display width in pixels.
    pub width: f32,
    /// Display height in pixels.
    pub height: f32,
    /// Bare texture asset name (e.g., "paseli_logo"). If set, the texture is
    /// resolved asynchronously on the game thread. The widget starts hidden
    /// until the mod explicitly calls `show()`.
    pub texture_name: Option<String>,
    /// Tint color in ABGR packed format. Default: `0xFFFFFFFF` (white, fully opaque).
    /// The color is multiplied with the texture's pixel colors during rendering.
    pub color: u32,
    /// Opacity from 0.0 (fully transparent) to 1.0 (fully opaque). Default: 1.0.
    /// Applied by modifying the alpha channel of the color field.
    pub opacity: f32,
    /// Blend mode: 0 = no blending, 1 = alpha blend (default).
    pub blend_mode: i32,
    /// Horizontal scale factor. Default: 1.0.
    pub scale_x: f32,
    /// Vertical scale factor. Default: 1.0.
    pub scale_y: f32,
    /// Anchor X for rotation/scaling pivot (0.0 = left, 1.0 = right). Default: 0.0.
    pub anchor_x: f32,
    /// Anchor Y for rotation/scaling pivot (0.0 = top, 1.0 = bottom). Default: 0.0.
    pub anchor_y: f32,
    /// Rotation in radians. Default: 0.0.
    pub rotation: f32,
}

impl Default for ImageWidgetConfig {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
            texture_name: None,
            color: 0xFFFFFFFF,
            opacity: 1.0,
            blend_mode: 1,
            scale_x: 1.0,
            scale_y: 1.0,
            anchor_x: 0.0,
            anchor_y: 0.0,
            rotation: 0.0,
        }
    }
}

pub struct ImageWidget {
    native_ptr: *mut u8,
    destroyed: bool,
}

unsafe impl Send for ImageWidget {}
unsafe impl Sync for ImageWidget {}

impl ImageWidget {
    pub fn new(native_ptr: *mut u8) -> Self {
        Self {
            native_ptr,
            destroyed: false,
        }
    }

    pub fn native_ptr(&self) -> *mut u8 {
        self.native_ptr
    }

    pub fn set_position(&self, x: f32, y: f32) {
        if self.destroyed {
            return;
        }
        unsafe {
            memory::write_f32(self.native_ptr.add(0x30), x);
            memory::write_f32(self.native_ptr.add(0x34), y);
        }
    }

    pub fn set_size(&self, w: f32, h: f32) {
        if self.destroyed {
            return;
        }
        unsafe {
            memory::write_f32(self.native_ptr.add(0x38), w);
            memory::write_f32(self.native_ptr.add(0x3C), h);
        }
    }

    pub fn set_scale(&self, x: f32, y: f32) {
        if self.destroyed {
            return;
        }
        unsafe {
            memory::write_f32(self.native_ptr.add(0x40), x);
            memory::write_f32(self.native_ptr.add(0x44), y);
        }
    }

    pub fn set_color(&self, abgr: u32) {
        if self.destroyed {
            return;
        }
        unsafe { memory::write_u32(self.native_ptr.add(0x64), abgr) };
    }

    pub fn set_texture_id(&self, id: i32) {
        if self.destroyed {
            return;
        }
        unsafe { memory::write_i32(self.native_ptr.add(0x28), id) };
    }

    pub fn set_uv(&self, left: f32, top: f32, right: f32, bottom: f32) {
        if self.destroyed {
            return;
        }
        unsafe {
            memory::write_f32(self.native_ptr.add(0x54), left);
            memory::write_f32(self.native_ptr.add(0x58), top);
            memory::write_f32(self.native_ptr.add(0x5C), right);
            memory::write_f32(self.native_ptr.add(0x60), bottom);
        }
    }

    pub fn set_rotation(&self, radians: f32) {
        if self.destroyed {
            return;
        }
        unsafe { memory::write_f32(self.native_ptr.add(0x50), radians) };
    }

    pub fn set_anchor(&self, x: f32, y: f32) {
        if self.destroyed {
            return;
        }
        unsafe {
            memory::write_f32(self.native_ptr.add(0x48), x);
            memory::write_f32(self.native_ptr.add(0x4C), y);
        }
    }

    pub fn set_blend_mode(&self, mode: i32) {
        if self.destroyed {
            return;
        }
        unsafe { memory::write_i32(self.native_ptr.add(0x2C), mode) };
    }

    pub fn set_opacity(&self, opacity: f32) {
        if self.destroyed {
            return;
        }
        unsafe {
            let current = memory::read_u32(self.native_ptr.add(0x64) as *const u8);
            let alpha = (opacity.clamp(0.0, 1.0) * 255.0) as u32;
            // ABGR format: alpha is the high byte
            let new_color = (current & 0x00FFFFFF) | (alpha << 24);
            memory::write_u32(self.native_ptr.add(0x64), new_color);
        }
    }

    pub fn show(&self) {
        if self.destroyed {
            return;
        }
        unsafe { memory::write_u8(self.native_ptr.add(0x12), 1) };
    }

    pub fn hide(&self) {
        if self.destroyed {
            return;
        }
        unsafe { memory::write_u8(self.native_ptr.add(0x12), 0) };
    }

    pub fn destroy(&mut self) {
        if self.destroyed {
            return;
        }
        self.hide();
        self.destroyed = true;
    }
}
