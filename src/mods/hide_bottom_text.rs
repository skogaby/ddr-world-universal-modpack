//! Hide Bottom Text — hides every status readout the game draws along the
//! bottom edge of the screen: the centre network-status word (`ONLINE`,
//! `CHECKING…`, `MAINTENANCE`, `OFFLINE MODE`, `LOCAL MODE`, …), the centre
//! `CREDIT` / `FREE PLAY` / `EVENT MODE` line and its COIN/TOKEN count, the
//! P1 / P2 `PASELI` / `EXTRA PASELI` balances in the corners, and the
//! `SOFTWARE ID` / `SYSTEM ID` / `HARDWARE ID` lines the idle attract screen
//! shows above them.
//!
//! ## Mechanism
//!
//! Thin contributor over `services::bottom_text` — this mod owns NO hook.
//! The service holds the single detour on the game's bottom-text renderer
//! and blanks the eight persistent text objects while any contributor asks
//! (`HideReason::HideBottomTextMod` here; power_user_statistics' BOTTOM LINE
//! widget layout is the other contributor — it hides the stock line for its
//! widgets' on-screen phase regardless of this toggle). RE:
//! `docs/hex_edit_porting.md` Hack 3 (with
//! the 2026-09-13 corrections — the doc's "detour → return" shape freezes
//! the last-drawn strings; the service blanks instead).
//!
//! Cabinet-wide, live toggle (Mods tab), no config section, no option
//! rows. **Default OFF** (`DEFAULT_OFF_MODS`): it removes operationally
//! useful information (credits, PASELI balance, ONLINE/MAINTENANCE state),
//! so a fresh install keeps the stock readouts until the operator opts in.
//!
//! ## Degradation
//!
//! `required_signatures` lists the renderer AOB plus the derived slot array,
//! so a miss on any build skips the mod cleanly (stock text, no toggle).

use crate::mods::mod_trait::{Mod, ModContext};
use crate::services::bottom_text::{self, HideReason};
use crate::{log_info, log_warn};

pub struct HideBottomTextMod;

impl HideBottomTextMod {
    pub fn new() -> Self {
        Self
    }
}

impl Mod for HideBottomTextMod {
    fn id(&self) -> &str {
        "hide-bottom-text"
    }

    fn name(&self) -> &str {
        "Hide Bottom Text"
    }

    fn description(&self) -> &str {
        "Hides the bottom-of-screen status text (online status, credits, PASELI balances, IDs)"
    }

    fn required_signatures(&self) -> &[&str] {
        &[
            "bottom_text_render",
            "bottom_text_blank_loop",
            "bottom_text_slots",
            "bottom_text_empty_str",
        ]
    }

    fn init(&mut self, _ctx: &ModContext) -> bool {
        true
    }

    fn enable(&mut self) {
        if !bottom_text::is_available() {
            log_warn!("HideBottomText: bottom_text service unavailable -- mod inactive");
            return;
        }
        bottom_text::set_hidden(HideReason::HideBottomTextMod, true);
        log_info!("HideBottomText: enabled (bottom status text hidden)");
    }

    fn disable(&mut self) {
        if bottom_text::is_available() {
            bottom_text::set_hidden(HideReason::HideBottomTextMod, false);
        }
        log_info!("HideBottomText: disabled (stock bottom status text)");
    }

    fn is_active(&self) -> bool {
        bottom_text::is_available()
    }
}
