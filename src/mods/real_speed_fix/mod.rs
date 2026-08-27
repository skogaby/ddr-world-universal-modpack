pub mod logf_stub;
pub mod real_speed;

use crate::mods::mod_trait::{Mod, ModContext};
use crate::{log_info, log_warn};

pub struct RealSpeedFixMod {
    real_speed_ready: bool,
    logf_stub_ready: bool,
    active: bool,
}

impl RealSpeedFixMod {
    pub fn new() -> Self {
        Self {
            real_speed_ready: false,
            logf_stub_ready: false,
            active: false,
        }
    }
}

impl Mod for RealSpeedFixMod {
    fn id(&self) -> &str {
        "real-speed-fix"
    }
    fn name(&self) -> &str {
        "Real Speed Calculation Fix"
    }
    fn description(&self) -> &str {
        "Real Speed display uses Core BPM instead of Max BPM"
    }
    fn required_signatures(&self) -> &[&str] {
        &[]
    }

    fn init(&mut self, ctx: &ModContext) -> bool {
        self.real_speed_ready = real_speed::init(ctx.signatures);
        self.logf_stub_ready = logf_stub::init(ctx.signatures);
        if !self.real_speed_ready {
            log_warn!("RealSpeedFix: real_speed_bpm_anchor missing — disabled");
        }
        if !self.logf_stub_ready {
            log_warn!("RealSpeedFix: real_speed_logf_anchor missing — logf guard disabled");
        }
        true
    }

    fn enable(&mut self) {
        if self.real_speed_ready {
            real_speed::enable();
            if self.logf_stub_ready {
                logf_stub::enable();
            }
            self.active = true;
            log_info!("RealSpeedFix: enabled");
        }
    }

    fn disable(&mut self) {
        if self.active {
            real_speed::disable();
            logf_stub::disable();
            self.active = false;
        }
        log_info!("RealSpeedFix: disabled");
    }
}
