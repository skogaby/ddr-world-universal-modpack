//! Skip Intros Mod — Redirects scene 7 (WARNING) → scene 14 (TITLE_SCREEN).

use crate::log_info;
use crate::mods::mod_trait::{Mod, ModContext};
use crate::services::scene_manager;
use crate::types::scenes::scene;

pub struct SkipIntrosMod {
    initialized: bool,
}

impl SkipIntrosMod {
    pub fn new() -> Self {
        Self { initialized: false }
    }
}

impl Mod for SkipIntrosMod {
    fn id(&self) -> &str {
        "skip-intros"
    }
    fn name(&self) -> &str {
        "Skip Intros"
    }
    fn description(&self) -> &str {
        "Skip the initial splash screens after game boot"
    }
    fn required_signatures(&self) -> &[&str] {
        &[]
    }

    fn init(&mut self, _ctx: &ModContext) -> bool {
        if !scene_manager::is_available() {
            return false;
        }
        self.initialized = true;
        true
    }

    fn enable(&mut self) {
        scene_manager::add_redirect(scene::WARNING_SPLASH, scene::TITLE_SCREEN);
        log_info!("SkipIntros: enabled -- scene 7 -> 14 redirect active");
    }

    fn disable(&mut self) {
        scene_manager::remove_redirect(scene::WARNING_SPLASH);
        log_info!("SkipIntros: disabled");
    }
}
