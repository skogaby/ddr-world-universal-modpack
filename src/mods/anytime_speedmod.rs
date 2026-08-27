//! Anytime Speedmod Adjustment — removes the ~10-second window limiting when
//! the player can change their speed mod (arrow scroll multiplier) with the
//! cabinet navigation buttons during gameplay.
//!
//! Stock behavior: each song, `sequence::dance::GamePlayActor` spawns a
//! `sequence::dance::ControlSpeedActor` that polls the menu Left/Right buttons
//! and steps the multiplier ±0.25×. Every frame the gameplay sequence
//! broadcasts the elapsed song time (msg 0x1045); once it reaches 10 000 ms
//! the actor self-destructs — that single compare IS the time limit. The
//! downstream apply path (msg 0x1042 → GamePlayActor speed lerp) is
//! time-agnostic, so keeping the actor alive makes adjustment work for the
//! whole song, including the stock smooth speed-change animation.
//!
//! Mechanism: rewrite the gate's imm32 (`CMP dword [R8+8], 0x2710`) to
//! `0x7FFFFFFF` on enable, restore `0x2710` on disable. Elapsed song ms never
//! approaches INT_MAX, so the actor lives until the normal msg-0x104A
//! song-end kill (untouched — per-song cleanup still happens).
//!
//! Cabinet-wide (both players); live toggle. Enabling mid-song only helps if
//! the stock window hasn't already expired for the current song (a dead actor
//! can't be resurrected); disabling mid-song leaves an already-alive actor
//! alive until song end. Both edges are harmless. Known cosmetic side effect:
//! the stock "speed change available" footer hint stays visible for the whole
//! song (it is accurate — adjustment IS available).
//!
//! RE notes: docs/anytime_speedmod_research.md

use crate::core::memory;
use crate::mods::mod_trait::{Mod, ModContext};
use crate::{log_info, log_warn};

/// Offset of the CMP imm32 within the `speedmod_window_gate` match
/// (`41 81 78 08 | 10 27 00 00 | 0F 8C`).
const IMM_OFFSET: usize = 4;
/// Stock gate value: 10 000 ms.
const STOCK_LIMIT_MS: u32 = 10_000;
/// Patched gate value: never reached by elapsed song time.
const UNLIMITED_MS: u32 = 0x7FFF_FFFF;

pub struct AnytimeSpeedmodMod {
    imm_addr: *mut u8,
}

unsafe impl Send for AnytimeSpeedmodMod {}

impl AnytimeSpeedmodMod {
    pub fn new() -> Self {
        Self {
            imm_addr: std::ptr::null_mut(),
        }
    }

    /// Rewrite the gate imm32 with page protection handled. The write lands
    /// inside an instruction executed every frame during gameplay, but both
    /// values keep the instruction well-formed and either constant is
    /// acceptable on any given frame, so no suspension is needed.
    unsafe fn write_gate(&self, value: u32) {
        let old = memory::make_writable(self.imm_addr as *const u8, 4);
        memory::write_u32(self.imm_addr, value);
        memory::restore_protection(self.imm_addr as *const u8, 4, old);
    }
}

impl Mod for AnytimeSpeedmodMod {
    fn id(&self) -> &str {
        "anytime-speedmod"
    }
    fn name(&self) -> &str {
        "Anytime Speedmod Adjustment"
    }
    fn description(&self) -> &str {
        "Adjust your speed mod at any point during gameplay (stock: first 10s only)"
    }
    fn required_signatures(&self) -> &[&str] {
        &["speedmod_window_gate"]
    }

    fn init(&mut self, ctx: &ModContext) -> bool {
        // The gate pattern is short; require it to be unique so a coincidental
        // second match on a future build can't get patched blindly.
        let matches = ctx.signatures.get_all_matches("speedmod_window_gate");
        if matches.len() != 1 {
            log_warn!(
                "AnytimeSpeedmod: expected exactly 1 gate match, found {} -- disabled",
                matches.len()
            );
            return false;
        }

        let imm_addr = unsafe { matches[0].add(IMM_OFFSET) as *mut u8 };
        if unsafe { memory::read_u32(imm_addr) } != STOCK_LIMIT_MS {
            log_warn!("AnytimeSpeedmod: gate imm32 is not the stock 10000 -- disabled");
            return false;
        }
        self.imm_addr = imm_addr;
        true
    }

    fn enable(&mut self) {
        unsafe { self.write_gate(UNLIMITED_MS) };
        log_info!("AnytimeSpeedmod: enabled -- in-song speed-adjust window unlimited");
    }

    fn disable(&mut self) {
        unsafe { self.write_gate(STOCK_LIMIT_MS) };
        log_info!("AnytimeSpeedmod: disabled -- stock 10s speed-adjust window restored");
    }
}
