//! DDR light state → SMX stage payloads (pure).
//!
//! Direct port of SpiceManiaX's `lights_utils.cpp` stage mapping, fed from
//! the `arkMDX*` capture instead of SpiceAPI:
//!
//! - **Arrow panels** (UP/DOWN/LEFT/RIGHT): 25 tape LEDs from the matching
//!   `pN_foot_*` device, 1:1 (both PCBs have 25 LEDs in the same physical
//!   order: 16 outer then 9 inner).
//! - **Corner panels** (UL/UR/DL/DR): the outer 4×4 grid renders an L-shape
//!   lit by the DDR stage-corner light value; every other LED is static gold
//!   to simulate a gold pad.
//! - **Center panel**: static gold.
//!
//! Corner sources (Ghidra-confirmed, see the feature progress.md): the DDR
//! stage corners ride `arkMDXChangeDimlamp` ids 21..=24 (P1) and 25..=28
//! (P2), order per side = `[UP_RIGHT, DOWN_LEFT, UP_LEFT, DOWN_RIGHT]`
//! (the mdxf `set_output_level` a2 0..3 table), values 0..255.

use super::protocol::{PadLights, PAD_LIGHT_BYTES};

/// Number of tape devices (spice2x `DDR_TAPELEDS[11]`):
/// `0..=3` P1 foot up/right/left/down, `4..=7` P2 foot, `8` top panel,
/// `9`/`10` monitor left/right.
pub const TAPE_DEVICES: usize = 11;
/// LEDs per tape device buffer (foot devices use the first 25).
pub const TAPE_LEDS: usize = 50;
/// Total dimlamp ids carried by `arkMDXChangeDimlamp` (0..=28).
pub const DIMLAMP_COUNT: usize = 29;

/// First stage-corner dimlamp id per side (P1 = 21..=24, P2 = 25..=28).
pub const CORNER_DIMLAMP_BASE: [usize; 2] = [21, 25];

/// The shared DDR light frame the `lights_read` detours accumulate into and
/// the transport's 30 Hz drain maps from.
#[derive(Clone)]
pub struct DdrLightFrame {
    /// Per-LED RGB for the 11 tape devices.
    pub tape: [[[u8; 3]; TAPE_LEDS]; TAPE_DEVICES],
    /// The 29 dimlamp values (stage corners at ids 21..=28; the woofer
    /// corners and menu lamps also live here for Step 2's cabinet lights).
    pub dimlamps: [u8; DIMLAMP_COUNT],
}

impl DdrLightFrame {
    pub const fn new() -> Self {
        Self {
            tape: [[[0; 3]; TAPE_LEDS]; TAPE_DEVICES],
            dimlamps: [0; DIMLAMP_COUNT],
        }
    }
}

/// Static accent used for the un-driven pad regions, selectable to match
/// the cabinet generation (deploy #21): GOLD is SpiceManiaX's `kPadRed/
/// kPadGreen/kPadBlue`; PLATINUM is a cool silver/chrome for the
/// identically-shaped Platinum cabinets.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PadStyle {
    Gold,
    Platinum,
}

impl PadStyle {
    /// The static accent RGB for the un-driven pad regions.
    pub const fn accent(self) -> [u8; 3] {
        match self {
            PadStyle::Gold => [0xBB, 0xBB, 0x00],
            PadStyle::Platinum => [0x8C, 0x96, 0xA8],
        }
    }
}

/// SMX panel indices, reading order (matches `input_map` bit positions).
const PANEL_UP_LEFT: usize = 0;
const PANEL_UP: usize = 1;
const PANEL_UP_RIGHT: usize = 2;
const PANEL_LEFT: usize = 3;
const PANEL_CENTER: usize = 4;
const PANEL_RIGHT: usize = 5;
const PANEL_DOWN_LEFT: usize = 6;
const PANEL_DOWN: usize = 7;
const PANEL_DOWN_RIGHT: usize = 8;

/// L-shape on/off flag grids for the outer 4×4 LEDs of each corner panel
/// (SpiceManiaX `kPad*Leds`). 1 = follows the corner light; 0 = static gold.
const L_UP_LEFT: [[u8; 4]; 4] = [[1, 1, 1, 1], [1, 0, 0, 0], [1, 0, 0, 0], [1, 0, 0, 0]];
const L_UP_RIGHT: [[u8; 4]; 4] = [[1, 1, 1, 1], [0, 0, 0, 1], [0, 0, 0, 1], [0, 0, 0, 1]];
const L_DOWN_LEFT: [[u8; 4]; 4] = [[1, 0, 0, 0], [1, 0, 0, 0], [1, 0, 0, 0], [1, 1, 1, 1]];
const L_DOWN_RIGHT: [[u8; 4]; 4] = [[0, 0, 0, 1], [0, 0, 0, 1], [0, 0, 0, 1], [1, 1, 1, 1]];

/// Per-side corner dimlamp id offsets in mdxf order `a2 = 0..3`:
/// `[UP_RIGHT, DOWN_LEFT, UP_LEFT, DOWN_RIGHT]`.
const CORNER_OFFSET_UP_RIGHT: usize = 0;
const CORNER_OFFSET_DOWN_LEFT: usize = 1;
const CORNER_OFFSET_UP_LEFT: usize = 2;
const CORNER_OFFSET_DOWN_RIGHT: usize = 3;

/// Map the captured DDR light frame onto both SMX pads' stage lights.
/// `pads[0]` = P1, `pads[1]` = P2. `style` picks the static accent for
/// the un-driven regions (Gold or Platinum cabinet colors).
pub fn map_stage(frame: &DdrLightFrame, style: PadStyle) -> [PadLights; 2] {
    let accent = style.accent();
    let mut pads = [[0u8; PAD_LIGHT_BYTES]; 2];
    for (pad_index, pad) in pads.iter_mut().enumerate() {
        let mut out = PadWriter { buf: pad, pos: 0 };
        for panel in 0..9 {
            match panel {
                PANEL_UP => arrow_panel(&mut out, frame, pad_index * 4),
                PANEL_RIGHT => arrow_panel(&mut out, frame, pad_index * 4 + 1),
                PANEL_LEFT => arrow_panel(&mut out, frame, pad_index * 4 + 2),
                PANEL_DOWN => arrow_panel(&mut out, frame, pad_index * 4 + 3),
                PANEL_UP_LEFT => corner_panel(
                    &mut out,
                    frame,
                    pad_index,
                    CORNER_OFFSET_UP_LEFT,
                    &L_UP_LEFT,
                    accent,
                ),
                PANEL_UP_RIGHT => corner_panel(
                    &mut out,
                    frame,
                    pad_index,
                    CORNER_OFFSET_UP_RIGHT,
                    &L_UP_RIGHT,
                    accent,
                ),
                PANEL_DOWN_LEFT => corner_panel(
                    &mut out,
                    frame,
                    pad_index,
                    CORNER_OFFSET_DOWN_LEFT,
                    &L_DOWN_LEFT,
                    accent,
                ),
                PANEL_DOWN_RIGHT => corner_panel(
                    &mut out,
                    frame,
                    pad_index,
                    CORNER_OFFSET_DOWN_RIGHT,
                    &L_DOWN_RIGHT,
                    accent,
                ),
                PANEL_CENTER => {
                    for _ in 0..25 {
                        out.push(accent);
                    }
                }
                _ => unreachable!(),
            }
        }
    }
    pads
}

struct PadWriter<'a> {
    buf: &'a mut PadLights,
    pos: usize,
}

impl PadWriter<'_> {
    #[inline]
    fn push(&mut self, rgb: [u8; 3]) {
        self.buf[self.pos..self.pos + 3].copy_from_slice(&rgb);
        self.pos += 3;
    }
}

/// 25 tape LEDs from the given foot device, 1:1.
fn arrow_panel(out: &mut PadWriter, frame: &DdrLightFrame, device: usize) {
    for led in 0..25 {
        out.push(frame.tape[device][led]);
    }
}

/// Outer 4×4 = L-shape lit by the corner value over the static accent;
/// inner 3×3 = accent.
fn corner_panel(
    out: &mut PadWriter,
    frame: &DdrLightFrame,
    pad_index: usize,
    corner_offset: usize,
    flags: &[[u8; 4]; 4],
    accent: [u8; 3],
) {
    let value = frame.dimlamps[CORNER_DIMLAMP_BASE[pad_index] + corner_offset];
    for row in flags {
        for &flag in row {
            if flag != 0 {
                out.push([value, value, value]);
            } else {
                out.push(accent);
            }
        }
    }
    for _ in 0..9 {
        out.push(accent);
    }
}
