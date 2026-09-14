//! Pure layer of the foot-panel swap service: the per-side controller
//! arbitration, the `GamePlayActor` field offsets the swap reads, the size of
//! the DLL-owned panel objects, and the flag block a bot filler hands back.
//!
//! Dependency-free on purpose (no `crate::` imports, no `unsafe`) so the
//! multiplayer-bot host harness can mount this file with `#[path]` and run the
//! `#[cfg(test)]` suite on a non-x86 host. Everything engine-facing lives in
//! `mod.rs`.

/// Which controller drives a side's foot panel this frame.
///
/// Arbitration is fixed: an armed bot outranks the autoplay (`Perfect`)
/// request, which outranks nothing. A side's cached `autoplay` option may be
/// ON while that side is a bot (per-side option values outlive the player), so
/// the bot must win without autoplay having to know about it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Controller {
    /// The game's own panel object stays in the slot.
    Off,
    /// Autoplay: the stock `AutoFootPanel` object, driven by the game's own
    /// `update` (every note Marvelous).
    Perfect,
    /// A DLL-owned panel object whose flag block a filler writes per frame.
    Bot,
}

/// The arbitration rule (Bot > Perfect > Off).
pub fn effective_controller(bot_armed: bool, perfect: bool) -> Controller {
    if bot_armed {
        Controller::Bot
    } else if perfect {
        Controller::Perfect
    } else {
        Controller::Off
    }
}

/// Size of every DLL-owned panel object. The stock `AutoFootPanel` is 0x58
/// bytes on 20260721+ (`qword[8]` press-time stamps at +0x18) and 0x40 on
/// 20250805 / 20260224 (`dword[8]`); the game's `update` writes through the
/// larger layout on the newer builds, so the object handed to it must cover it.
pub const PANEL_OBJECT_SIZE: usize = 0x58;

/// `GamePlayActor` play side (i32: 0 = left / P1, 1 = right / P2). In doubles
/// the single actor reports 0.
pub const ACTOR_SIDE: usize = 0x84;
/// `GamePlayActor` Results vector `begin` pointer (0x40-stride entries; the
/// `end` pointer follows at +0xB8). The stock `update` takes `actor + this`.
pub const ACTOR_RESULTS_BEGIN: usize = 0xB0;
/// `GamePlayActor` current beat position (i32) — the freeze-hold comparand the
/// stock `update` receives as its third argument.
pub const ACTOR_CUR_BEAT: usize = 0x168;

/// The flag block a bot filler produces for one judge frame. Field order and
/// widths mirror the DLL-owned panel object (`is_held` at +0x08,
/// `was_just_pressed` at +0x10, `event_mc` at +0x18) so the service can copy
/// it in verbatim.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BotPanelFlags {
    /// Read by the stock `isHeld` slot: non-zero = the panel is down.
    pub is_held: [u8; 8],
    /// Read by the stock `wasJustPressed` slot: non-zero = pressed this frame.
    pub was_just_pressed: [u8; 8],
    /// The planned judge-event music count per panel; the bot `getPressAge`
    /// slot returns `current_mc - event_mc[panel]`.
    pub event_mc: [i32; 8],
}

/// Number of virtual slots in `AutoFootPanel`'s vtable (every build).
pub const VTABLE_SLOTS: usize = 7;
/// `getPressAge(this, panel) -> i32` — replaced on the bot object.
pub const SLOT_GET_PRESS_AGE: usize = 5;
/// `consumePress(this, panel)` — replaced on the bot object.
pub const SLOT_CONSUME_PRESS: usize = 6;

/// The DLL-owned panel object the `Bot` controller writes into the actor's
/// `IFootPanel*` slot. The first three fields mirror the stock `AutoFootPanel`
/// layout the judge reads through slots 2/3 (`wasJustPressed → +0x10`,
/// `isHeld → +0x08`); `event_mc` sits where the stock press-time stamps live
/// and is read only by OUR slot 5 / zeroed by OUR slot 6, so its element
/// width is ours to choose. Padded to the largest stock object.
#[repr(C)]
pub struct BotFootPanel {
    /// Cloned vtable (slots 5/6 ours); `image.as_ptr() + 8` (COL at `[-1]`).
    pub vtable: *const *const u8,
    pub is_held: [u8; 8],
    pub was_just_pressed: [u8; 8],
    pub event_mc: [i32; 8],
    _reserve: [u8; PANEL_OBJECT_SIZE - 0x38],
}

impl BotFootPanel {
    /// An all-zero object with a null vtable (the service installs the real
    /// one once the clone exists).
    pub const fn zeroed() -> Self {
        BotFootPanel {
            vtable: std::ptr::null(),
            is_held: [0; 8],
            was_just_pressed: [0; 8],
            event_mc: [0; 8],
            _reserve: [0; PANEL_OBJECT_SIZE - 0x38],
        }
    }

    /// Copy one frame's flags in (never touches the vtable).
    pub fn apply(&mut self, flags: &BotPanelFlags) {
        self.is_held = flags.is_held;
        self.was_just_pressed = flags.was_just_pressed;
        self.event_mc = flags.event_mc;
    }
}

/// Physical image of the cloned vtable: `[COL, slot0 .. slot6]` — index 0 is
/// the RTTI complete-object-locator MSVC keeps at `vtable[-1]`, so the
/// installed vtable pointer is `image.as_ptr() + 1`. Slots 5 and 6 are
/// replaced, everything else is the donor's (the `two_player_bpl_mode` /
/// `custom_options` clone shape).
pub fn bot_vtable_image(
    donor: &[usize; VTABLE_SLOTS],
    col: usize,
    get_press_age: usize,
    consume_press: usize,
) -> [usize; VTABLE_SLOTS + 1] {
    let mut img = [0usize; VTABLE_SLOTS + 1];
    img[0] = col;
    img[1..].copy_from_slice(donor);
    img[1 + SLOT_GET_PRESS_AGE] = get_press_age;
    img[1 + SLOT_CONSUME_PRESS] = consume_press;
    img
}

/// Mask a panel argument from the judge into 0..=7 so a hostile value can
/// never index out of the flag arrays.
#[inline]
pub fn panel_index(panel: i32) -> usize {
    (panel as usize) & 7
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bot_outranks_perfect() {
        assert_eq!(effective_controller(true, true), Controller::Bot);
    }

    #[test]
    fn bot_alone() {
        assert_eq!(effective_controller(true, false), Controller::Bot);
    }

    #[test]
    fn perfect_alone() {
        assert_eq!(effective_controller(false, true), Controller::Perfect);
    }

    #[test]
    fn neither_is_off() {
        assert_eq!(effective_controller(false, false), Controller::Off);
    }

    #[test]
    fn panel_object_size_covers_every_stock_build() {
        // 0x58 on 20260721+ (qword press-time stamps), 0x40 on 20250805 /
        // 20260224 (dword stamps). The DLL-owned object must be at least the
        // largest, and the design pins it to exactly the largest.
        assert_eq!(PANEL_OBJECT_SIZE, 0x58);
        assert!(PANEL_OBJECT_SIZE >= 0x40);
    }

    #[test]
    fn actor_offsets_pinned() {
        assert_eq!(ACTOR_SIDE, 0x84);
        assert_eq!(ACTOR_RESULTS_BEGIN, 0xB0);
        assert_eq!(ACTOR_CUR_BEAT, 0x168);
    }

    #[test]
    fn bot_panel_flags_default_is_all_zero() {
        let f = BotPanelFlags::default();
        assert!(f.is_held.iter().all(|&b| b == 0));
        assert!(f.was_just_pressed.iter().all(|&b| b == 0));
        assert!(f.event_mc.iter().all(|&e| e == 0));
    }

    #[test]
    fn bot_foot_panel_layout_matches_the_stock_object() {
        use std::mem::{offset_of, size_of};
        assert_eq!(offset_of!(BotFootPanel, is_held), 0x08);
        assert_eq!(offset_of!(BotFootPanel, was_just_pressed), 0x10);
        assert_eq!(offset_of!(BotFootPanel, event_mc), 0x18);
        assert_eq!(size_of::<BotFootPanel>(), PANEL_OBJECT_SIZE);
    }

    #[test]
    fn bot_vtable_image_replaces_only_slots_5_and_6() {
        let donor: [usize; VTABLE_SLOTS] = [10, 11, 12, 13, 14, 15, 16];
        let img = bot_vtable_image(&donor, 0xC01, 0xAAAA, 0xBBBB);
        assert_eq!(img[0], 0xC01, "COL at [-1]");
        for (i, &d) in donor.iter().enumerate() {
            match i {
                SLOT_GET_PRESS_AGE => assert_eq!(img[1 + i], 0xAAAA),
                SLOT_CONSUME_PRESS => assert_eq!(img[1 + i], 0xBBBB),
                _ => assert_eq!(img[1 + i], d, "slot {i} verbatim"),
            }
        }
        assert_eq!(img.len(), VTABLE_SLOTS + 1);
    }

    #[test]
    fn apply_copies_every_flag() {
        let mut obj = BotFootPanel::zeroed();
        let mut f = BotPanelFlags::default();
        for p in 0..8 {
            f.is_held[p] = (p % 2) as u8;
            f.was_just_pressed[p] = 1;
            f.event_mc[p] = 1000 + p as i32;
        }
        obj.apply(&f);
        assert_eq!(obj.is_held, f.is_held);
        assert_eq!(obj.was_just_pressed, f.was_just_pressed);
        assert_eq!(obj.event_mc, f.event_mc);
        assert!(obj.vtable.is_null(), "apply never touches the vtable");
    }

    #[test]
    fn panel_index_is_masked() {
        assert_eq!(panel_index(0), 0);
        assert_eq!(panel_index(7), 7);
        assert_eq!(panel_index(8), 0);
        assert_eq!(panel_index(-1), 7);
        assert_eq!(panel_index(i32::MIN), 0);
    }
}
