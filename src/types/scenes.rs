//! Scene ID constants and lookup for DDR World.
//! Scene IDs are 0-indexed.

use once_cell::sync::Lazy;
use std::collections::HashMap;

static SCENE_NAMES: Lazy<HashMap<i32, &'static str>> = Lazy::new(|| {
    let mut m = HashMap::new();
    m.insert(0, "NOW_LOADING");
    m.insert(1, "HARDWARE_CHECK");
    m.insert(2, "PRE_SPLASH_BLACK_1");
    m.insert(5, "PRE_SPLASH_BLACK_2");
    m.insert(6, "PRE_SPLASH_BLACK_3");
    m.insert(7, "WARNING_SPLASH");
    m.insert(8, "KONAMI_SPLASH");
    m.insert(9, "BEMANI_SPLASH");
    m.insert(10, "EAMUSEMENT_SPLASH");
    m.insert(11, "RSA_SPLASH");
    m.insert(12, "SONG_LICENSES");
    m.insert(14, "TITLE_SCREEN");
    m.insert(16, "ATTRACT_DEMO");
    m.insert(18, "LANGUAGE_TO_MODE_INTERSTITIAL");
    m.insert(20, "MODE_SELECT");
    m.insert(21, "CAUTION");
    m.insert(24, "CAUTION_TO_SONG_INTERSTITIAL");
    m.insert(25, "SONG_SELECT");
    m.insert(26, "SONG_TO_STAGE_INTERSTITIAL");
    m.insert(27, "STAGE_INDICATOR");
    m.insert(28, "GAMEPLAY");
    m.insert(29, "STAGE_RESULT");
    m.insert(30, "RESULTS_DETAIL");
    m.insert(32, "FINAL_RESULTS");
    m.insert(33, "FINAL_TO_THANKS_INTERSTITIAL");
    m.insert(34, "EAM_EXIT");
    m.insert(35, "THANK_YOU");
    m.insert(40, "START_TO_LANGUAGE_INTERSTITIAL");
    m.insert(41, "LANGUAGE_SELECT");
    m
});

pub fn get_scene_name(scene_id: i32) -> String {
    SCENE_NAMES
        .get(&scene_id)
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("UNKNOWN_{}", scene_id))
}

#[allow(dead_code)]
pub mod scene {
    pub const NOW_LOADING: i32 = 0;
    pub const HARDWARE_CHECK: i32 = 1;
    pub const WARNING_SPLASH: i32 = 7;
    pub const TITLE_SCREEN: i32 = 14;
    pub const ATTRACT_DEMO: i32 = 16;
    /// The select-music `LoadingSequence` (kind 2, load mask 0x6000):
    /// `getNextID(0x19) = 0x1A` → SONG_SELECT. Runs both after CAUTION and on
    /// every Wait→select hop of the play cycle. Quick Fail's skip-results
    /// path redirects 29 → 24 to land here straight from the FAILED banner.
    pub const CAUTION_TO_SONG_INTERSTITIAL: i32 = 24;
    pub const SONG_SELECT: i32 = 25;
    pub const SONG_TO_STAGE_INTERSTITIAL: i32 = 26;
    pub const STAGE_INDICATOR: i32 = 27;
    pub const GAMEPLAY: i32 = 28;
    // Naming note (no rename — existing mods depend on the current names and
    // values): 29 `STAGE_RESULT` is actually the post-song LoadingSequence
    // (the only loader that loads the `scene_result` BM2D package), and 30
    // `RESULTS_DETAIL` is the real ResultSequence. The quick-logout mod relies
    // on 29's loader behaviour: it triggers `finish(child, 30₁ᵢₙdₑₓ)` (= 0-idx
    // 29) plus a one-shot redirect 30 → 32 to reach FINAL_RESULTS with the
    // package resident.
    pub const STAGE_RESULT: i32 = 29;
    pub const RESULTS_DETAIL: i32 = 30;
    /// TOTAL RESULTS session summary (`TotalResultSequence`). Requires the
    /// `scene_result` BM2D package to be resident — only the 0-idx 29 loader
    /// loads it. Never jump here directly from song select.
    pub const FINAL_RESULTS: i32 = 32;
    pub const FINAL_TO_THANKS_INTERSTITIAL: i32 = 33;
    /// `EAmExitRootSequence` — credit/PASELI expire + the `savekind == 3`
    /// e-amusement logout save (per entered side). The logout-save sanitiser
    /// fires on entry to this scene.
    pub const EAM_EXIT: i32 = 34;
    pub const THANK_YOU: i32 = 35;
}

pub const ATTRACT_SCENE_MIN: i32 = 2;
pub const ATTRACT_SCENE_MAX: i32 = 16;
