//! Touch-overlay button model — pure geometry and hit-testing (no game
//! state, no IO). Ported from SpiceManiaX's `overlay_utils.cpp` /
//! `overlay_button.h` (same 1280×720 coordinate space and layout math, so
//! the button positions match what the maintainer's cabinet ran for years),
//! with two fixes over the original:
//!
//! - Hit-testing here is a plain point-in-(rotated-)rect check (SpiceManiaX
//!   round-tripped through cached Direct2D geometry objects).
//! - The model knows nothing about press state; callers track presses per
//!   touch contact so a release OUTSIDE a button still releases the button
//!   it pressed (SpiceManiaX hit-tested the release point — a drag off a
//!   button left it stuck pressed).
//!
//! Button identity: `bit` is the button's index into the per-player shared
//! state bitmask (`overlay::HELD`), stable across rebuilds:
//! 0..=4 menu (Start/Up/Down/Left/Right — matching `inject_slot::MENU_*`),
//! 5..=16 pinpad (bit = 5 + 10-key buffer index), 17 card-in, 18 toggle.

/// Menu-nav button index — matches `input_manager::inject_slot::MENU_*`
/// order (Start, Up, Down, Left, Right).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MenuButton {
    Start = 0,
    Up = 1,
    Down = 2,
    Left = 3,
    Right = 4,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ButtonKind {
    /// Menu navigation (injected additively into the ark menu-button bytes).
    Menu(MenuButton),
    /// Pinpad key; payload = the `arkMDXGet10Key` buffer index
    /// (0..=9 digits, 10 = "00", 11 = decimal point).
    Pinpad(u8),
    /// Insert-card (fires a one-shot card scan episode on press).
    CardIn,
    /// Per-player overlay show/hide toggle (local, never injected).
    Visibility,
}

/// Shared-state bit indices (see module docs).
pub const BIT_MENU_BASE: u32 = 0;
pub const BIT_PINPAD_BASE: u32 = 5;
pub const BIT_CARD_IN: u32 = 17;
pub const BIT_VISIBILITY: u32 = 18;
pub const BIT_COUNT: usize = 19;

#[derive(Clone, Debug)]
pub struct Button {
    pub player: usize,
    pub kind: ButtonKind,
    /// Shared-state bit index (see module docs).
    pub bit: u32,
    /// Label text ("" = unlabeled — the rotated menu diamonds).
    pub label: &'static str,
    pub cx: f32,
    pub cy: f32,
    pub w: f32,
    pub h: f32,
    /// Rotated 45° about its center (the menu-nav diamonds).
    pub rotated: bool,
    /// The button's CLUSTER ANCHOR — the screen corner its cluster scales
    /// about (overlay-scale setting): clusters grow toward screen center
    /// and shrink toward their corner. Pinpad/utility clusters anchor at
    /// the top corners, menu-nav clusters at the bottom corners.
    pub anchor: [f32; 2],
}

impl Button {
    /// The four corner points tracing the button's perimeter (quad order)
    /// at overlay scale `scale` (1.0 = authored layout), optionally
    /// inflated by `inflate` px per side in AUTHORED units (scaled with
    /// the button). The scale transform is about the cluster anchor:
    /// `p' = anchor + (p - anchor) * scale`.
    pub fn corners(&self, scale: f32, inflate: f32) -> [[f32; 2]; 4] {
        let hw = self.w / 2.0 + inflate;
        let hh = self.h / 2.0 + inflate;
        let local = [[-hw, -hh], [hw, -hh], [hw, hh], [-hw, hh]];
        let (sin, cos) = if self.rotated {
            // 45°
            (
                std::f32::consts::FRAC_1_SQRT_2,
                std::f32::consts::FRAC_1_SQRT_2,
            )
        } else {
            (0.0, 1.0)
        };
        let mut out = [[0.0f32; 2]; 4];
        for (i, [x, y]) in local.into_iter().enumerate() {
            let px = self.cx + x * cos - y * sin;
            let py = self.cy + x * sin + y * cos;
            out[i] = [
                self.anchor[0] + (px - self.anchor[0]) * scale,
                self.anchor[1] + (py - self.anchor[1]) * scale,
            ];
        }
        out
    }

    /// Point-in-(rotated-)rect test in the 1280×720 model space at
    /// overlay scale `scale` (inverse-maps the point about the cluster
    /// anchor, then tests the authored geometry).
    pub fn contains(&self, scale: f32, x: f32, y: f32) -> bool {
        if scale <= 0.0 {
            return false;
        }
        let ux = self.anchor[0] + (x - self.anchor[0]) / scale;
        let uy = self.anchor[1] + (y - self.anchor[1]) / scale;
        let dx = ux - self.cx;
        let dy = uy - self.cy;
        let (lx, ly) = if self.rotated {
            // Inverse-rotate the point by 45° about the center.
            let c = std::f32::consts::FRAC_1_SQRT_2;
            (dx * c + dy * c, -dx * c + dy * c)
        } else {
            (dx, dy)
        };
        lx.abs() <= self.w / 2.0 && ly.abs() <= self.h / 2.0
    }
}

// ── Layout constants (SpiceManiaX overlay_utils.cpp, 1280×720) ──────
// Values replicate the original's arithmetic including its int casts so
// the buttons land on the exact pixels the maintainer's cabinet had.

const MENU_NAV: f32 = 50.0; // kMenuNavButtonWidth/Height
const PINPAD: f32 = 30.0; // kPinpadButtonWidth/Height
const UTIL_W: f32 = 120.0; // kUtilityButtonWidth
const UTIL_H: f32 = 30.0; // kUtilityButtonHeight

/// Per-player Menu-Up center X (everything else anchors off this).
const MENU_UP_CX: [f32; 2] = [100.0, 1072.0];
const MENU_UP_CY: f32 = 575.0;
/// `static_cast<int>(575 + 50 * (1.75 / 2))` = 618
const MENU_LRS_CY: f32 = 618.0;
/// `static_cast<int>(575 + 50 * 1.75)` = 662
const MENU_DOWN_CY: f32 = 662.0;
/// `static_cast<int>(50 * 3.25)` = 162
const MENU_START_DX: f32 = 162.0;

const PINPAD_EDGE: f32 = 20.0; // pinpad_edge_to_screen
const PINPAD_SPACING: f32 = 10.0;
const PINPAD_WIDTH: f32 = PINPAD * 3.0 + 2.0 * PINPAD_SPACING; // 110
/// X of the top-left pinpad key center per player.
const PINPAD_FIRST_CX: [f32; 2] = [
    PINPAD_EDGE + PINPAD / 2.0,                         // 35
    1280.0 - PINPAD_WIDTH - PINPAD_EDGE + PINPAD / 2.0, // 1165
];
/// The pinpad COLUMN center per player (middle key column) — the
/// toggle/card utility buttons center on it (deploy #20 feedback: the
/// whole top cluster reads as one aligned column).
const COLUMN_CX: [f32; 2] = [
    PINPAD_FIRST_CX[0] + PINPAD + PINPAD_SPACING, // 75
    PINPAD_FIRST_CX[1] + PINPAD + PINPAD_SPACING, // 1205
];

/// Top cluster stack (deploy #20 layout): toggle on top, Insert-Card
/// below it, pinpad below that — all centered on COLUMN_CX with 10 px
/// gaps.
const TOGGLE_CY: f32 = PINPAD_EDGE + UTIL_H / 2.0; // 35
const CARD_CY: f32 = TOGGLE_CY + UTIL_H + PINPAD_SPACING; // 75
/// Y of the top pinpad row center: below the card slot (the pinpad sits
/// here whether or not a card button exists — stable layout).
const PINPAD_FIRST_CY: f32 = CARD_CY + UTIL_H / 2.0 + PINPAD_SPACING + PINPAD / 2.0; // 115

/// Cluster scale anchors (see `Button::anchor`): top corners for the
/// pinpad/utility stacks, bottom corners for the menu-nav clusters.
const TOP_ANCHOR: [[f32; 2]; 2] = [[0.0, 0.0], [1280.0, 0.0]];
const BOTTOM_ANCHOR: [[f32; 2]; 2] = [[0.0, 720.0], [1280.0, 720.0]];

/// Pinpad rows top→bottom, columns left→right: the key's 10-key buffer
/// index (0..=9 digits, 10 = "00", 11 = decimal) and its label (the
/// decimal key is BLANK on Konami cabinet pinpads — maintainer feedback,
/// deploy #17).
const PINPAD_LAYOUT: [[(u8, &str); 3]; 4] = [
    [(7, "7"), (8, "8"), (9, "9")],
    [(4, "4"), (5, "5"), (6, "6")],
    [(1, "1"), (2, "2"), (3, "3")],
    [(0, "0"), (10, "00"), (11, "")],
];

/// Build both players' button sets. `has_card[p]` gates that player's
/// Insert-Card button (present only when a card id is configured).
pub fn build_buttons(has_card: [bool; 2]) -> Vec<Button> {
    let mut out = Vec::with_capacity(38);
    for player in 0..2usize {
        let up_cx = MENU_UP_CX[player];

        // Menu nav: Up / Down / Left / Right diamonds + Start square.
        let menu = [
            (MenuButton::Up, up_cx, MENU_UP_CY, true),
            (MenuButton::Down, up_cx, MENU_DOWN_CY, true),
            (MenuButton::Left, up_cx - MENU_NAV + 5.0, MENU_LRS_CY, true),
            (MenuButton::Right, up_cx + MENU_NAV - 5.0, MENU_LRS_CY, true),
            (MenuButton::Start, up_cx + MENU_START_DX, MENU_LRS_CY, false),
        ];
        for (btn, cx, cy, rotated) in menu {
            out.push(Button {
                player,
                kind: ButtonKind::Menu(btn),
                bit: BIT_MENU_BASE + btn as u32,
                label: if btn == MenuButton::Start {
                    "START"
                } else {
                    ""
                },
                cx,
                cy,
                w: MENU_NAV,
                h: MENU_NAV,
                rotated,
                anchor: BOTTOM_ANCHOR[player],
            });
        }

        // Pinpad grid.
        let first_cx = PINPAD_FIRST_CX[player];
        for (row, keys) in PINPAD_LAYOUT.iter().enumerate() {
            for (col, (key, label)) in keys.iter().enumerate() {
                out.push(Button {
                    player,
                    kind: ButtonKind::Pinpad(*key),
                    bit: BIT_PINPAD_BASE + *key as u32,
                    label,
                    cx: first_cx + (PINPAD + PINPAD_SPACING) * col as f32,
                    cy: PINPAD_FIRST_CY + (PINPAD + PINPAD_SPACING) * row as f32,
                    w: PINPAD,
                    h: PINPAD,
                    rotated: false,
                    anchor: TOP_ANCHOR[player],
                });
            }
        }

        // Visibility toggle (always present, always hit-testable).
        out.push(Button {
            player,
            kind: ButtonKind::Visibility,
            bit: BIT_VISIBILITY,
            label: "HIDE OVERLAY",
            cx: COLUMN_CX[player],
            cy: TOGGLE_CY,
            w: UTIL_W,
            h: UTIL_H,
            rotated: false,
            anchor: TOP_ANCHOR[player],
        });

        // Insert-card (only when a card id is configured for this player;
        // between the toggle and the pinpad, on the shared column — the
        // pinpad position is fixed either way).
        if has_card[player] {
            out.push(Button {
                player,
                kind: ButtonKind::CardIn,
                bit: BIT_CARD_IN,
                label: "INSERT CARD",
                cx: COLUMN_CX[player],
                cy: CARD_CY,
                w: UTIL_W,
                h: UTIL_H,
                rotated: false,
                anchor: TOP_ANCHOR[player],
            });
        }
    }
    out
}

/// Hit-test a point (1280×720 space) against the button set at overlay
/// scale `scale`, respecting per-player visibility: while a player's
/// overlay is hidden only their Visibility toggle responds (SpiceManiaX
/// pressed invisible buttons). Returns the index of the first hit.
pub fn hit_test(
    buttons: &[Button],
    visible: [bool; 2],
    scale: f32,
    x: f32,
    y: f32,
) -> Option<usize> {
    buttons.iter().position(|b| {
        (visible[b.player] || b.kind == ButtonKind::Visibility) && b.contains(scale, x, y)
    })
}

/// Parse a configured card id (16 hex chars, e.g. "E004010012345678")
/// into the 8 raw UID bytes the ark stores (byte k = hex pair k).
pub fn parse_card_id(s: &str) -> Option<[u8; 8]> {
    let s = s.trim();
    if s.len() != 16 || !s.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let mut uid = [0u8; 8];
    for (k, chunk) in s.as_bytes().chunks(2).enumerate() {
        let hi = (chunk[0] as char).to_digit(16)?;
        let lo = (chunk[1] as char).to_digit(16)?;
        uid[k] = ((hi << 4) | lo) as u8;
    }
    Some(uid)
}
