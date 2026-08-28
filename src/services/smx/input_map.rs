//! SMX panel mask → DDR arrow panels (pure).
//!
//! The SMX input mask is a 9-bit panel bitfield in reading order
//! `012/345/678` (bit 1 = Up, bit 3 = Left, bit 4 = Center, bit 5 = Right,
//! bit 7 = Down; the corners are bits 0/2/6/8). DDR consumes only the four
//! cardinal arrows — the same subset SpiceManiaX forwarded
//! (`input_utils.h` `kPanelIndices = {1, 7, 3, 5}`).

pub const BIT_UP_LEFT: u16 = 1 << 0;
pub const BIT_UP: u16 = 1 << 1;
pub const BIT_UP_RIGHT: u16 = 1 << 2;
pub const BIT_LEFT: u16 = 1 << 3;
pub const BIT_CENTER: u16 = 1 << 4;
pub const BIT_RIGHT: u16 = 1 << 5;
pub const BIT_DOWN_LEFT: u16 = 1 << 6;
pub const BIT_DOWN: u16 = 1 << 7;
pub const BIT_DOWN_RIGHT: u16 = 1 << 8;

/// A DDR stage direction (the four arrow panels the game reads through
/// `arkMDXGetPanelUp/Down/Left/Right`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelDir {
    Up,
    Down,
    Left,
    Right,
}

impl PanelDir {
    /// The SMX input-mask bit carrying this DDR panel.
    #[inline]
    pub fn smx_bit(self) -> u16 {
        match self {
            PanelDir::Up => BIT_UP,
            PanelDir::Down => BIT_DOWN,
            PanelDir::Left => BIT_LEFT,
            PanelDir::Right => BIT_RIGHT,
        }
    }
}

/// Whether the given DDR panel is held in an SMX input mask.
#[inline]
pub fn panel_held(mask: u16, dir: PanelDir) -> bool {
    mask & dir.smx_bit() != 0
}
