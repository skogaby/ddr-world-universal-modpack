//! Timer Freeze Mod — Freezes the countdown timer and hides its display.
//!
//! Two independent byte patches on `sequence::common::TimerActor::onUpdate`:
//!
//! 1. **Freeze** (`timer_update_jz`, required): JZ → JMP at the "displayed
//!    value changed" compare, so the remaining-seconds field (actor+0xB8)
//!    never updates — the countdown (and every scene timeout driven by it)
//!    is frozen at its starting value.
//! 2. **Hide** (`timer_show_call`, optional): the freeze alone leaves the
//!    timer art on screen, and the per-frame digit refresh picks its texture
//!    from the actor's LIVE clock accumulator — the frozen digits turn red
//!    ("hazard" art) when the timer *would* have been running out. The hide
//!    patch zeroes the visible flag fed to the state-1 show call (the only
//!    site in the binary that makes the `timer_root` layer visible), so the
//!    whole timer clip — frame art and digit children — never renders. The
//!    state machine, countdown, and timeout semantics stay stock.
//!
//! Fail-soft: if `timer_show_call` doesn't resolve (or its patch bytes don't
//! verify), the mod degrades to freeze-only with one WARN. Live-toggle note:
//! the hide takes effect at the next timer show (scene entry / timer reset) —
//! a timer already on screen when the mod is toggled keeps its current
//! visibility until then.

use crate::core::memory;
use crate::mods::mod_trait::{Mod, ModContext};
use crate::{log_info, log_warn};

const JZ_OFFSET: usize = 28;
const JZ_SIZE: usize = 6;

/// Offset of `MOVZX EDX, byte [RBP+0xBC]` inside the `timer_show_call` match.
const SHOW_OFFSET: usize = 62;
const SHOW_SIZE: usize = 7;
/// The MOVZX instruction the hide patch replaces (verified at init).
const SHOW_EXPECTED: [u8; SHOW_SIZE] = [0x0F, 0xB6, 0x95, 0xBC, 0x00, 0x00, 0x00];
/// XOR EDX,EDX + 5 NOPs — the show helper is always called with visible=0.
const SHOW_PATCH: [u8; SHOW_SIZE] = [0x33, 0xD2, 0x90, 0x90, 0x90, 0x90, 0x90];

pub struct TimerFreezeMod {
    patch_addr: *mut u8,
    original_bytes: [u8; JZ_SIZE],
    /// Hide-patch site (null = unresolved, freeze-only).
    hide_addr: *mut u8,
}

unsafe impl Send for TimerFreezeMod {}

impl TimerFreezeMod {
    pub fn new() -> Self {
        Self {
            patch_addr: std::ptr::null_mut(),
            original_bytes: [0; JZ_SIZE],
            hide_addr: std::ptr::null_mut(),
        }
    }
}

impl Mod for TimerFreezeMod {
    fn id(&self) -> &str {
        "timer-freeze"
    }
    fn name(&self) -> &str {
        "Timer Freeze"
    }
    fn description(&self) -> &str {
        "Freeze all in-game timers and hide the timer display"
    }
    fn required_signatures(&self) -> &[&str] {
        &["timer_update_jz"]
    }

    fn init(&mut self, ctx: &ModContext) -> bool {
        let match_addr = ctx.signatures.require_address("timer_update_jz");
        self.patch_addr = unsafe { match_addr.add(JZ_OFFSET) as *mut u8 };

        unsafe {
            if *self.patch_addr != 0x0F || *self.patch_addr.add(1) != 0x84 {
                log_warn!("TimerFreeze: expected JZ (0F 84) at patch address");
                return false;
            }
            for i in 0..JZ_SIZE {
                self.original_bytes[i] = *self.patch_addr.add(i);
            }
        }

        // Optional hide patch — degrade to freeze-only if unresolved.
        match ctx.signatures.get_address("timer_show_call") {
            Some(show_match) => {
                let addr = unsafe { show_match.add(SHOW_OFFSET) as *mut u8 };
                let bytes_ok = (0..SHOW_SIZE).all(|i| unsafe { *addr.add(i) } == SHOW_EXPECTED[i]);
                if bytes_ok {
                    self.hide_addr = addr;
                } else {
                    log_warn!(
                        "TimerFreeze: timer_show_call bytes mismatch at patch site -- timer will freeze but stay visible"
                    );
                }
            }
            None => {
                log_warn!(
                    "TimerFreeze: timer_show_call signature not resolved -- timer will freeze but stay visible"
                );
            }
        }
        true
    }

    fn enable(&mut self) {
        unsafe {
            let jz_disp = memory::read_i32(self.patch_addr.add(2) as *const u8);
            let jmp_disp = jz_disp + 1;

            let old = memory::make_writable(self.patch_addr as *const u8, JZ_SIZE);
            memory::write_u8(self.patch_addr, 0xE9); // JMP rel32
            memory::write_i32(self.patch_addr.add(1), jmp_disp);
            memory::write_u8(self.patch_addr.add(5), 0x90); // NOP
            memory::restore_protection(self.patch_addr as *const u8, JZ_SIZE, old);
        }

        if !self.hide_addr.is_null() {
            unsafe {
                let old = memory::make_writable(self.hide_addr as *const u8, SHOW_SIZE);
                for (i, b) in SHOW_PATCH.iter().enumerate() {
                    memory::write_u8(self.hide_addr.add(i), *b);
                }
                memory::restore_protection(self.hide_addr as *const u8, SHOW_SIZE, old);
            }
            log_info!(
                "TimerFreeze: enabled -- timer frozen and hidden (hide applies at next timer show)"
            );
        } else {
            log_info!("TimerFreeze: enabled -- timer display frozen (hide patch unavailable)");
        }
    }

    fn disable(&mut self) {
        unsafe {
            let old = memory::make_writable(self.patch_addr as *const u8, JZ_SIZE);
            for i in 0..JZ_SIZE {
                memory::write_u8(self.patch_addr.add(i), self.original_bytes[i]);
            }
            memory::restore_protection(self.patch_addr as *const u8, JZ_SIZE, old);
        }

        if !self.hide_addr.is_null() {
            unsafe {
                let old = memory::make_writable(self.hide_addr as *const u8, SHOW_SIZE);
                for (i, b) in SHOW_EXPECTED.iter().enumerate() {
                    memory::write_u8(self.hide_addr.add(i), *b);
                }
                memory::restore_protection(self.hide_addr as *const u8, SHOW_SIZE, old);
            }
        }
        log_info!("TimerFreeze: disabled -- timer display restored");
    }
}
