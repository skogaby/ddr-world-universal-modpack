//! Button types and constants for the InputManager.

use once_cell::sync::Lazy;
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Player {
    P1 = 0,
    P2 = 1,
}

#[allow(dead_code)]
pub mod button {
    pub const START: u32 = 1 << 0;
    pub const MENU_UP: u32 = 1 << 1;
    pub const MENU_DOWN: u32 = 1 << 2;
    pub const MENU_LEFT: u32 = 1 << 3;
    pub const MENU_RIGHT: u32 = 1 << 4;
    pub const NUM_0: u32 = 1 << 5;
    pub const NUM_1: u32 = 1 << 6;
    pub const NUM_2: u32 = 1 << 7;
    pub const NUM_3: u32 = 1 << 8;
    pub const NUM_4: u32 = 1 << 9;
    pub const NUM_5: u32 = 1 << 10;
    pub const NUM_6: u32 = 1 << 11;
    pub const NUM_7: u32 = 1 << 12;
    pub const NUM_8: u32 = 1 << 13;
    pub const NUM_9: u32 = 1 << 14;
    pub const NUM_STAR: u32 = 1 << 15;
    pub const NUM_HASH: u32 = 1 << 16;
    // Dance-pad stage panels (distinct from the MENU_* cabinet buttons).
    // Only reported while a consumer has opted in via
    // `input_manager::set_panel_polling(true)`.
    pub const PANEL_UP: u32 = 1 << 17;
    pub const PANEL_DOWN: u32 = 1 << 18;
    pub const PANEL_LEFT: u32 = 1 << 19;
    pub const PANEL_RIGHT: u32 = 1 << 20;
}

pub static BUTTON_NAMES: Lazy<HashMap<u32, &'static str>> = Lazy::new(|| {
    let mut m = HashMap::new();
    m.insert(button::START, "Start");
    m.insert(button::MENU_UP, "Menu-Up");
    m.insert(button::MENU_DOWN, "Menu-Down");
    m.insert(button::MENU_LEFT, "Menu-Left");
    m.insert(button::MENU_RIGHT, "Menu-Right");
    m.insert(button::NUM_0, "0");
    m.insert(button::NUM_1, "1");
    m.insert(button::NUM_2, "2");
    m.insert(button::NUM_3, "3");
    m.insert(button::NUM_4, "4");
    m.insert(button::NUM_5, "5");
    m.insert(button::NUM_6, "6");
    m.insert(button::NUM_7, "7");
    m.insert(button::NUM_8, "8");
    m.insert(button::NUM_9, "9");
    m.insert(button::NUM_STAR, "*");
    m.insert(button::NUM_HASH, "#");
    m.insert(button::PANEL_UP, "Panel-Up");
    m.insert(button::PANEL_DOWN, "Panel-Down");
    m.insert(button::PANEL_LEFT, "Panel-Left");
    m.insert(button::PANEL_RIGHT, "Panel-Right");
    m
});

#[derive(Clone, Debug)]
pub struct InputEvent {
    pub player: Player,
    pub button: u32,
    pub button_name: String,
    pub event_type: InputEventType,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputEventType {
    Pressed,
    Released,
}

pub type InputEventCallback = Box<dyn Fn(&InputEvent) + Send + Sync>;
/// Exclusive consumer returns true to consume (suppress) the event.
pub type ExclusiveConsumerCallback = Box<dyn Fn(&InputEvent) -> bool + Send + Sync>;
