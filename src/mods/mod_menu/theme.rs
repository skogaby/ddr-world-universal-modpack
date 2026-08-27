//! Pure theme layer for the mod-menu overlay: the built-in theme table
//! (identity + palette + background kind) and id-resolution helpers.
//!
//! Deliberately dependency-light — the only import is the sibling
//! [`super::chrome`] module (for [`PanelGradient`]), which the host-test
//! harness (`scripts/validate_mod_menu.sh`) mounts alongside this module,
//! so `super::` resolves identically in the real crate and the harness.
//! No `crate::` paths, no logging: the impure shell (`chrome_loader.rs`,
//! `render.rs`, `tabs.rs`, `input.rs`) reads the table and owns all I/O.
//!
//! Design: overlay-menu rewrite detailed design §4.6 (theme system),
//! §4.4 (`overlay_menu` config). Palette values are the agent-authored
//! first cut (maintainer tunes at the Step 7 demo).

use super::chrome::PanelGradient;

// ── Palette ─────────────────────────────────────────────────────────

/// Every color the renderer draws with, one field per use site. Text
/// colors are linear RGB `[f32; 3]` (fed to `TextWidget::set_color`);
/// tint colors are RGB `[u8; 3]` (the renderer supplies its fixed alpha
/// bytes and packs ABGR); the panel gradient stops are `[u8; 4]` with
/// the alpha byte ignored (see [`PanelGradient`] — panel alpha comes
/// from the configured opacity alone).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    // Text colors.
    pub title: [f32; 3],
    pub tab_active: [f32; 3],
    pub tab_inactive: [f32; 3],
    pub header: [f32; 3],
    pub label: [f32; 3],
    pub value: [f32; 3],
    pub greyed: [f32; 3],
    pub on_value: [f32; 3],
    pub off_value: [f32; 3],
    pub footer: [f32; 3],
    pub hints: [f32; 3],
    // Tint RGBs (selection bar + tab underline + cursor share `accent`).
    pub accent: [u8; 3],
    pub header_bar: [u8; 3],
    pub banner_back: [u8; 3],
    // Panel gradient stops (alpha ignored).
    pub panel_top: [u8; 4],
    pub panel_bottom: [u8; 4],
}

impl Palette {
    /// The panel-synthesis gradient for this palette.
    #[must_use]
    pub fn gradient(&self) -> PanelGradient {
        PanelGradient {
            top: self.panel_top,
            bottom: self.panel_bottom,
        }
    }
}

// ── Theme ───────────────────────────────────────────────────────────

/// Which synthesized theme program a shader-backed theme binds. The
/// concrete DEFAULT-container program index comes from the synthesis
/// export (`overlay_draw::theme_program_indices()` — in
/// [`ThemeProgram::slot`] order); this enum keeps the pure table
/// decoupled from the numbering. `slot()` order MUST match the
/// `THEME_BLOBS` PS order in `shader_synthesis` (the SetShader handler
/// has no bounds check — the host tests pin this lockstep).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeProgram {
    Bubbles,
    Terminal,
    Waveform,
    Spectrum,
    Tunnel,
    Xmb,
    Squares,
    CardSwirl,
    Blobs,
    Ps2,
    PrimeCube,
}

impl ThemeProgram {
    /// Position in the synthesis export's published index array.
    #[must_use]
    pub fn slot(self) -> usize {
        match self {
            ThemeProgram::Bubbles => 0,
            ThemeProgram::Terminal => 1,
            ThemeProgram::Waveform => 2,
            ThemeProgram::Spectrum => 3,
            ThemeProgram::Tunnel => 4,
            ThemeProgram::Xmb => 5,
            ThemeProgram::Squares => 6,
            ThemeProgram::CardSwirl => 7,
            ThemeProgram::Blobs => 8,
            ThemeProgram::Ps2 => 9,
            ThemeProgram::PrimeCube => 10,
        }
    }
}

/// How a theme fills the space behind the modal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Background {
    /// Gradient-only (baked into the synthesized panel texture).
    Static,
    /// Animated command-list background bound to a synthesized theme
    /// program (falls back to `Static` when the shader path is
    /// unavailable — design §6).
    Shader { program: ThemeProgram },
}

/// One built-in theme.
pub struct Theme {
    /// Config value (`overlay_menu.theme`) and chrome cache-stem
    /// component — must satisfy the engine texture-name charset
    /// (lowercase ASCII alphanumeric / underscore).
    pub id: &'static str,
    /// THEME tab row label.
    pub display: &'static str,
    pub palette: Palette,
    pub background: Background,
}

/// Shared text colors (every built-in currently agrees on these; kept
/// per-theme via struct-update so a future theme can diverge freely).
const BASE_TEXT: Palette = Palette {
    title: [1.0, 1.0, 1.0],
    tab_active: [1.0, 1.0, 1.0], // overridden per theme
    tab_inactive: [0.55, 0.55, 0.55],
    header: [1.0, 1.0, 1.0], // overridden per theme
    label: [1.0, 1.0, 1.0],
    value: [1.0, 1.0, 1.0],
    greyed: [0.45, 0.45, 0.45],
    on_value: [0.2, 1.0, 0.2],
    off_value: [1.0, 0.3, 0.3],
    footer: [0.75, 0.75, 0.75],
    hints: [0.55, 0.55, 0.55],
    accent: [255, 255, 255],      // overridden per theme
    header_bar: [255, 255, 255],  // overridden per theme
    banner_back: [16, 18, 28],    // overridden per theme
    panel_top: [0, 0, 0, 255],    // overridden per theme
    panel_bottom: [0, 0, 0, 255], // overridden per theme
};

/// The built-in themes, in THEME-row enum order. Index 0 is the default.
/// Shader-backed entries are Shadertoy ports (attribution in each
/// `shaders/src/themes/theme_*.hlsl` header); MINIMAL stays static and
/// last.
pub const THEMES: &[Theme] = &[
    // Dark teal with a warm orange accent.
    Theme {
        id: "bubbles",
        display: "BUBBLES",
        palette: Palette {
            tab_active: [1.0, 0.7, 0.35],
            header: [0.4, 0.85, 0.8],
            accent: [255, 170, 80],
            header_bar: [70, 200, 190],
            banner_back: [8, 22, 24],
            panel_top: [10, 40, 44, 255],
            panel_bottom: [4, 16, 20, 255],
            ..BASE_TEXT
        },
        background: Background::Shader {
            program: ThemeProgram::Bubbles,
        },
    },
    // Green phosphor on near-black (broken-CRT digit wall).
    Theme {
        id: "terminal",
        display: "TERMINAL",
        palette: Palette {
            tab_active: [0.35, 1.0, 0.45],
            header: [0.5, 0.95, 0.55],
            accent: [60, 255, 120],
            header_bar: [40, 220, 100],
            banner_back: [6, 16, 8],
            panel_top: [10, 26, 14, 255],
            panel_bottom: [3, 10, 5, 255],
            ..BASE_TEXT
        },
        background: Background::Shader {
            program: ThemeProgram::Terminal,
        },
    },
    // Neon pink/violet over deep indigo (raymarched sine ocean).
    Theme {
        id: "waveform",
        display: "WAVEFORM",
        palette: Palette {
            tab_active: [1.0, 0.5, 0.85],
            header: [0.75, 0.6, 1.0],
            accent: [255, 120, 210],
            header_bar: [150, 110, 255],
            banner_back: [14, 10, 26],
            panel_top: [24, 18, 46, 255],
            panel_bottom: [8, 6, 20, 255],
            ..BASE_TEXT
        },
        background: Background::Shader {
            program: ThemeProgram::Waveform,
        },
    },
    // Amber over navy-black (frequency-bar visualizer).
    Theme {
        id: "spectrum",
        display: "SPECTRUM",
        palette: Palette {
            tab_active: [1.0, 0.8, 0.3],
            header: [1.0, 0.75, 0.35],
            accent: [255, 190, 70],
            header_bar: [255, 160, 60],
            banner_back: [8, 10, 20],
            panel_top: [14, 18, 38, 255],
            panel_bottom: [5, 6, 16, 255],
            ..BASE_TEXT
        },
        background: Background::Shader {
            program: ThemeProgram::Spectrum,
        },
    },
    // Mint green over dark blue-green (raymarched ring tunnel).
    Theme {
        id: "tunnel",
        display: "TUNNEL",
        palette: Palette {
            tab_active: [0.55, 1.0, 0.7],
            header: [0.6, 0.95, 0.75],
            accent: [130, 255, 170],
            header_bar: [110, 230, 150],
            banner_back: [8, 16, 14],
            panel_top: [14, 30, 26, 255],
            panel_bottom: [5, 12, 11, 255],
            ..BASE_TEXT
        },
        background: Background::Shader {
            program: ThemeProgram::Tunnel,
        },
    },
    // Classic XMB blue (PS3 wave ribbon).
    Theme {
        id: "xmb",
        display: "XMB",
        palette: Palette {
            tab_active: [0.65, 0.9, 1.0],
            header: [0.7, 0.9, 1.0],
            accent: [160, 220, 255],
            header_bar: [90, 180, 220],
            banner_back: [10, 26, 34],
            panel_top: [24, 66, 84, 255],
            panel_bottom: [8, 28, 38, 255],
            ..BASE_TEXT
        },
        background: Background::Shader {
            program: ThemeProgram::Xmb,
        },
    },
    // Deep navy with drifting squares (lighter-blue accent).
    Theme {
        id: "squares",
        display: "SQUARES",
        palette: Palette {
            tab_active: [0.45, 0.7, 1.0],
            header: [0.55, 0.75, 1.0],
            accent: [90, 170, 255],
            header_bar: [40, 120, 220],
            banner_back: [4, 14, 30],
            panel_top: [5, 22, 48, 255],
            panel_bottom: [2, 10, 26, 255],
            ..BASE_TEXT
        },
        background: Background::Shader {
            program: ThemeProgram::Squares,
        },
    },
    // Sunlit fractal: warm gold accent over sea-green.
    // (The MANDELBULB theme was cut here 2026-08-25: its raymarcher —
    // even flattened to shallow flow control — failed D3DMetal's
    // buildPipelineState under CrossOver, dropping the whole renderer
    // to software. Its `mandelbulb` id degrades via the unknown-id
    // path.)
    // Balatro card back: red accent, blue header, dark slate panel.
    Theme {
        id: "card_swirl",
        display: "CARD SWIRL",
        palette: Palette {
            tab_active: [1.0, 0.45, 0.4],
            header: [0.35, 0.75, 1.0],
            accent: [255, 95, 85],
            header_bar: [0, 157, 255],
            banner_back: [16, 18, 20],
            panel_top: [30, 36, 38, 255],
            panel_bottom: [12, 15, 16, 255],
            ..BASE_TEXT
        },
        background: Background::Shader {
            program: ThemeProgram::CardSwirl,
        },
    },
    // Gooey purple metaballs with a mint merge-glow.
    Theme {
        id: "blobs",
        display: "BLOBS",
        palette: Palette {
            tab_active: [0.85, 0.55, 0.9],
            header: [0.6, 0.95, 0.75],
            accent: [200, 120, 220],
            header_bar: [150, 90, 170],
            banner_back: [16, 8, 18],
            panel_top: [30, 14, 34, 255],
            panel_bottom: [12, 5, 14, 255],
            ..BASE_TEXT
        },
        background: Background::Shader {
            program: ThemeProgram::Blobs,
        },
    },
    // PS2 startup: icy blue orbs on near-black.
    Theme {
        id: "ps2",
        display: "PS2",
        palette: Palette {
            tab_active: [0.6, 0.88, 1.0],
            header: [0.7, 0.9, 1.0],
            accent: [143, 219, 255],
            header_bar: [100, 170, 220],
            banner_back: [8, 12, 20],
            panel_top: [14, 20, 34, 255],
            panel_bottom: [5, 8, 14, 255],
            ..BASE_TEXT
        },
        background: Background::Shader {
            program: ThemeProgram::Ps2,
        },
    },
    // Tumbling prime-number voxel lattice: green blaze on blue-violet.
    Theme {
        id: "prime_cube",
        display: "PRIME CUBE",
        palette: Palette {
            tab_active: [0.45, 1.0, 0.5],
            header: [0.6, 0.65, 1.0],
            accent: [90, 255, 110],
            header_bar: [110, 100, 230],
            banner_back: [12, 12, 22],
            panel_top: [22, 22, 44, 255],
            panel_bottom: [9, 9, 18, 255],
            ..BASE_TEXT
        },
        background: Background::Shader {
            program: ThemeProgram::PrimeCube,
        },
    },
    // Neutral dark grey with a white accent.
    Theme {
        id: "minimal",
        display: "MINIMAL",
        palette: Palette {
            tab_active: [1.0, 1.0, 1.0],
            header: [0.85, 0.85, 0.85],
            accent: [255, 255, 255],
            header_bar: [200, 200, 200],
            banner_back: [18, 18, 20],
            panel_top: [34, 34, 38, 255],
            panel_bottom: [14, 14, 16, 255],
            ..BASE_TEXT
        },
        background: Background::Static,
    },
];

/// Fallback theme (design §6: unknown `overlay_menu.theme` ⇒ default +
/// one WARN, logged by the impure caller).
pub const DEFAULT_THEME_INDEX: usize = 0;

// ── Resolution ──────────────────────────────────────────────────────

/// Resolve a configured theme id to a table index. Returns
/// `(index, known)`: `None` (key absent) resolves to the default and
/// counts as known; an unknown id resolves to the default with
/// `known == false` so the caller can WARN once.
#[must_use]
pub fn resolve_theme_index(id: Option<&str>) -> (usize, bool) {
    match id {
        None => (DEFAULT_THEME_INDEX, true),
        Some(id) => match THEMES.iter().position(|t| t.id == id) {
            Some(index) => (index, true),
            None => (DEFAULT_THEME_INDEX, false),
        },
    }
}

/// The theme at `index`, clamped to the table (never panics — a stale
/// persisted index degrades to the last entry rather than a hook-path
/// panic).
#[must_use]
pub fn theme(index: usize) -> &'static Theme {
    &THEMES[index.min(THEMES.len() - 1)]
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_integrity() {
        assert_eq!(THEMES.len(), 12);
        let ids: Vec<&str> = THEMES.iter().map(|t| t.id).collect();
        assert_eq!(
            ids,
            [
                "bubbles",
                "terminal",
                "waveform",
                "spectrum",
                "tunnel",
                "xmb",
                "squares",
                "card_swirl",
                "blobs",
                "ps2",
                "prime_cube",
                "minimal"
            ]
        );
        let displays: Vec<&str> = THEMES.iter().map(|t| t.display).collect();
        assert_eq!(
            displays,
            [
                "BUBBLES",
                "TERMINAL",
                "WAVEFORM",
                "SPECTRUM",
                "TUNNEL",
                "XMB",
                "SQUARES",
                "CARD SWIRL",
                "BLOBS",
                "PS2",
                "PRIME CUBE",
                "MINIMAL"
            ]
        );
        for (i, t) in THEMES.iter().enumerate() {
            // Unique ids/displays.
            for other in &THEMES[i + 1..] {
                assert_ne!(t.id, other.id);
                assert_ne!(t.display, other.display);
            }
            // Stem-charset-safe ids (engine texture-name rule).
            assert!(
                t.id.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
                "theme id {:?} is not stem-safe",
                t.id
            );
            // Non-degenerate gradient.
            assert_ne!(
                t.palette.panel_top, t.palette.panel_bottom,
                "theme {:?} has a degenerate gradient",
                t.id
            );
        }
    }

    #[test]
    fn gradient_maps_stops() {
        let g = THEMES[0].palette.gradient();
        assert_eq!(g.top, [10, 40, 44, 255]);
        assert_eq!(g.bottom, [4, 16, 20, 255]);
    }

    #[test]
    fn resolution() {
        assert_eq!(resolve_theme_index(None), (DEFAULT_THEME_INDEX, true));
        // BUBBLES is the default (index 0).
        assert_eq!(THEMES[DEFAULT_THEME_INDEX].id, "bubbles");
        for (i, t) in THEMES.iter().enumerate() {
            assert_eq!(resolve_theme_index(Some(t.id)), (i, true));
        }
        assert_eq!(
            resolve_theme_index(Some("bogus")),
            (DEFAULT_THEME_INDEX, false)
        );
        // The retired themes resolve like any unknown id (old configs
        // degrade to the default with one WARN).
        assert_eq!(
            resolve_theme_index(Some("arrows")),
            (DEFAULT_THEME_INDEX, false)
        );
        assert_eq!(
            resolve_theme_index(Some("wavefield")),
            (DEFAULT_THEME_INDEX, false)
        );
        assert_eq!(
            resolve_theme_index(Some("mandelbulb")),
            (DEFAULT_THEME_INDEX, false)
        );
        // Clamp, never panic.
        assert_eq!(theme(usize::MAX).id, "minimal");
        assert_eq!(theme(1).id, "terminal");
    }

    #[test]
    fn background_mapping() {
        // Every theme except the last (MINIMAL) is shader-backed with a
        // DISTINCT program whose export slot equals its table position;
        // MINIMAL stays Static (its ANIMATED BACKGROUND row is greyed
        // even with shaders available).
        let shader_backed = THEMES.len() - 1;
        for (slot, t) in THEMES[..shader_backed].iter().enumerate() {
            match t.background {
                Background::Shader { program } => assert_eq!(
                    program.slot(),
                    slot,
                    "{} must bind export slot {slot}",
                    t.id
                ),
                Background::Static => panic!("{} should be shader-backed", t.id),
            }
        }
        assert_eq!(THEMES[shader_backed].background, Background::Static);
        assert_eq!(THEMES[shader_backed].id, "minimal");
    }
}
