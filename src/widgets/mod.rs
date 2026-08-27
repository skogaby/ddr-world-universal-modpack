//! Widget types — Rust wrappers around the game's native UI primitives.
//!
//! - **TextWidget** wraps `kt::BmpfontSimpleString` — the game's bitmap font text
//!   renderer. Supports positioning, scaling, color, alignment, and outline width.
//!
//! - **ImageWidget** wraps `agcs::Sprite` — the game's sprite renderer. Supports
//!   positioning, scaling, rotation, color/opacity, blend modes, and texture
//!   assignment via the BM2D texture name resolution system.
//!
//! Both widget types are created through `widget_renderer::create_text_widget()`
//! and `widget_renderer::create_image_widget()`, which handle native memory
//! allocation and render list registration. Widgets are automatically rendered
//! by the game's own render pipeline — no per-frame draw calls needed.

pub mod bounce;
pub mod image_widget;
pub mod text_widget;
