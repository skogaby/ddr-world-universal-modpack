//! Shared DVD-screensaver bounce animation helpers.
//!
//! Extracted from the Hello World mod so other mods (e.g. Autoplay's
//! "Autoplay Enabled" watermark) can reuse the exact same bounce + rainbow
//! behavior. Pure math — no game memory access.

/// Screen dimensions the game renders its 2D UI at.
pub const SCREEN_W: f32 = 1280.0;
pub const SCREEN_H: f32 = 720.0;

/// A rectangle that bounces around the screen, DVD-screensaver style.
pub struct Bouncer {
    pub x: f32,
    pub y: f32,
    pub dx: f32,
    pub dy: f32,
    pub w: f32,
    pub h: f32,
}

impl Bouncer {
    /// Advance one animation step, reflecting off the screen edges.
    pub fn tick(&mut self) {
        self.x += self.dx;
        self.y += self.dy;
        if self.x <= 0.0 || self.x + self.w >= SCREEN_W {
            self.dx = -self.dx;
            self.x = self.x.max(0.0).min(SCREEN_W - self.w);
        }
        if self.y <= 0.0 || self.y + self.h >= SCREEN_H {
            self.dy = -self.dy;
            self.y = self.y.max(0.0).min(SCREEN_H - self.h);
        }
    }

    /// Re-randomize position and velocity (used when a bouncing widget is
    /// (re)shown so it doesn't always start from the same corner).
    pub fn randomize(&mut self) {
        self.x = rand_range(0.0, SCREEN_W - self.w);
        self.y = rand_range(0.0, SCREEN_H - self.h);
        self.dx = rand_range(1.0, 3.0) * rand_sign();
        self.dy = rand_range(1.0, 3.0) * rand_sign();
    }
}

/// HSV → RGB conversion for the rainbow color cycle. `h` in degrees [0, 360),
/// `s`/`v` in [0, 1]. Returns (r, g, b) in [0, 1].
pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (f32, f32, f32) {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    (r + m, g + m, b + m)
}

/// Cheap LCG random in [min, max). Not cryptographic — animation seeding only.
pub fn rand_range(min: f32, max: f32) -> f32 {
    static mut SEED: u64 = 0xDEADBEEF;
    unsafe {
        SEED = SEED
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let t = ((SEED >> 33) as f32) / (u32::MAX as f32);
        min + t * (max - min)
    }
}

/// Randomly returns -1.0 or 1.0.
pub fn rand_sign() -> f32 {
    if rand_range(0.0, 1.0) < 0.5 {
        -1.0
    } else {
        1.0
    }
}
