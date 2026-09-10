//! Pure resolution model for the Custom Resolution mod (design §4.1, revised
//! 2026-09-09 to a single knob).
//!
//! Everything here is a function of config strings and integer dimensions —
//! no game memory, no `crate::` imports — so the module is a dependency-free
//! leaf that `scripts/validate_custom_resolution.sh` mounts into a host crate
//! and unit-tests. The impure layers (`patches`, `present`, `scissor`,
//! `letterbox`, `logical_screen`) consume the [`Plan`] this module produces
//! and never re-derive any of its rules.
//!
//! Vocabulary: **output** = the D3D9 back-buffer / display surface size (what
//! the panel receives); **render** = the size of the game's internal 1280×720
//! surfaces and list viewports (what geometry rasterises at). The stock game
//! has both at 1280×720; the logical 1280×720 *canvas* never changes.
//!
//! The ONE operator knob is the output size. The render size is derived, not
//! chosen: a 16:9 output renders natively (render == output — the engine's
//! own `screen_w == render_w` 1:1 branch, no scaler, no extra full-screen
//! passes), a 4:3 output keeps the stock 1280×720 render and goes through the
//! engine's SD crop/letterbox scaler. The render ≠ output "perf mode" was
//! removed on purpose: it forced the game out of its direct-mode present
//! chain (below) on every real cabinet, and the linear upscale was soft.
//!
//! AA policy is equally opinionated. The game's AA config (`display struct
//! +0x18`) is 0 (offscreen composite) or, on pcType-2..4 HD cabinets — every
//! cabinet and spice2x setup seen so far — 3 ("direct" mode: 3D and 2D render
//! straight into the screen-sized `display` surface, the COPYVIEWPORT
//! `StretchRect` is skipped, one in-place `sys_copy_aa` pass + one present
//! quad). Mode 3 is the CHEAPER present chain, so the plan leaves the game's
//! choice alone whenever render == output. It is forced to 0 only for the 4:3
//! path, which needs the mode-0 crop/letterbox scaler (stock SD cabinets run 0
//! anyway — onBoot only picks 3 when the HD flag is set).

/// Stock render/output size — the only configuration that is a literal no-op.
pub const STOCK: Dims = Dims { w: 1280, h: 720 };

/// Largest side any surface, viewport or scissor rect can carry: every
/// dimension in the engine's RT/viewport/scissor structs is a `u16`, and 8K
/// (7680×4320) already sits at the edge of what D3D9-era surfaces allow.
pub const MAX_SIDE: u32 = 8192;

/// Smallest accepted output height (a 640×360 window is the smallest thing the
/// letterbox math degrades gracefully into).
pub const MIN_HEIGHT: u32 = 360;

/// Integer pixel dimensions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dims {
    pub w: u32,
    pub h: u32,
}

impl Dims {
    pub const fn new(w: u32, h: u32) -> Self {
        Self { w, h }
    }
    /// `true` when `self` is at least as large as `other` in BOTH dimensions —
    /// the condition under which a depth surface of `self`'s size may legally
    /// back a colour target of `other`'s size (stock SD cabinets bind a 720p
    /// depth to a 640×480 colour surface; the 4:3 plan relies on it).
    pub fn covers(self, other: Dims) -> bool {
        self.w >= other.w && self.h >= other.h
    }
}

/// Output aspect class. Anything else is rejected (design R3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Aspect {
    Wide16x9,
    Sd4x3,
}

/// The engine's two SD present modes (`letterbox_rect_fn` `mode` argument):
/// `Crop` = mode 1 (960-px centre crop, what SD cabinets shipped with),
/// `Letterbox` = mode 0 (width-fit letterbox, the TEST menu's choice).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SdPresent {
    Crop,
    Letterbox,
}

/// Policy applied by the letterbox-rect detour to the game's requested mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresentPolicy {
    /// render == output: the engine's `screen_w == render_w` branch takes the
    /// 1:1 POINT copy regardless of mode (and mode 3 skips the copy entirely)
    /// — no detour needed.
    Stock,
    /// 4:3 output: mode-1 requests become the operator's choice; mode 0 (the
    /// TEST menu) is always honoured.
    Sd(SdPresent),
}

/// The game's AA config values (`display struct +0x18` → the AA global).
pub const AA_OFF: u32 = 0;
pub const AA_2X: u32 = 1;
pub const AA_4X: u32 = 2;
pub const AA_DIRECT: u32 = 3;

/// What the mod does with the game's AA config.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AaPolicy {
    /// Leave whatever onBoot chose (0, or direct mode 3 on pcType-2..4 HD
    /// cabinets). Used whenever render == output.
    Stock,
    /// Force 0: the render ≠ output (4:3) path needs the mode-0 present-chain
    /// scaler that direct mode skips.
    ForceOff,
}

/// The fully resolved boot plan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub output: Dims,
    pub render: Dims,
    pub aspect: Aspect,
    pub aa: AaPolicy,
    pub present_policy: PresentPolicy,
}

impl Plan {
    /// `true` when any surface/viewport/letterbox-src immediate must change.
    pub fn render_is_stock(&self) -> bool {
        self.render == STOCK
    }
    /// `true` when the back-buffer immediates must change.
    pub fn output_is_stock(&self) -> bool {
        self.output == STOCK
    }
    /// The onBoot `MOV [RSP+d],3` immediate must become 0 (the imm patch
    /// covers the pcType-2..4 branch; the graphics-init detour's struct write
    /// covers every branch).
    pub fn force_aa_zero(&self) -> bool {
        self.aa == AaPolicy::ForceOff
    }
}

/// Result of [`compute`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Stock configuration — zero patches, zero detours.
    Inert,
    /// Unusable configuration; the string is the WARN text.
    Rejected(String),
    Plan(Plan),
}

/// The config strings, exactly as read from `mod-config.json`.
#[derive(Clone, Copy, Debug)]
pub struct PlanInput<'a> {
    pub output: &'a str,
    pub sd_present: &'a str,
}

/// Parse `"WxH"` (either case of `x`, surrounding whitespace tolerated).
/// Rejects zero and odd dimensions — every surface/viewport dim is a `u16`
/// pair and the letterbox math halves heights.
pub fn parse_dims(s: &str) -> Option<Dims> {
    let s = s.trim();
    let (w, h) = s.split_once(['x', 'X'])?;
    let w: u32 = w.trim().parse().ok()?;
    let h: u32 = h.trim().parse().ok()?;
    if w == 0 || h == 0 || w % 2 != 0 || h % 2 != 0 {
        return None;
    }
    Some(Dims { w, h })
}

/// Classify an output size. 16:9 tolerance `|9w − 16h| ≤ 16` (1366×768-style
/// panels), 4:3 tolerance `|3w − 4h| ≤ 12`.
pub fn classify_aspect(d: Dims) -> Option<Aspect> {
    let w = d.w as i64;
    let h = d.h as i64;
    if (9 * w - 16 * h).abs() <= 16 {
        Some(Aspect::Wide16x9)
    } else if (3 * w - 4 * h).abs() <= 12 {
        Some(Aspect::Sd4x3)
    } else {
        None
    }
}

/// The render size an output implies: native for 16:9, the stock 1280×720
/// for 4:3 (the SD present path is a crop/letterbox OF a 720p picture).
pub fn render_for(output: Dims, aspect: Aspect) -> Dims {
    match aspect {
        Aspect::Wide16x9 => output,
        Aspect::Sd4x3 => STOCK,
    }
}

/// Compute the boot plan.
pub fn compute(input: &PlanInput) -> Outcome {
    let Some(output) = parse_dims(input.output) else {
        return Outcome::Rejected(format!(
            "resolution.output '{}' is not WxH with even dimensions",
            input.output
        ));
    };
    if output.w > MAX_SIDE || output.h > MAX_SIDE {
        return Outcome::Rejected(format!(
            "resolution.output {}x{} exceeds the {} px per-side limit",
            output.w, output.h, MAX_SIDE
        ));
    }
    if output.h < MIN_HEIGHT {
        return Outcome::Rejected(format!(
            "resolution.output {}x{} is below the {} px minimum height",
            output.w, output.h, MIN_HEIGHT
        ));
    }
    let Some(aspect) = classify_aspect(output) else {
        return Outcome::Rejected(format!(
            "resolution.output {}x{} is neither 16:9 nor 4:3",
            output.w, output.h
        ));
    };
    let render = render_for(output, aspect);
    if output == STOCK {
        return Outcome::Inert;
    }
    // render == output takes the engine's 1:1 branch (or direct mode, which
    // has no copy at all); the 4:3 path needs the mode-0 scaler, so the
    // game's possible direct-mode choice is overridden there and only there.
    let (present_policy, aa) = match aspect {
        Aspect::Wide16x9 => (PresentPolicy::Stock, AaPolicy::Stock),
        Aspect::Sd4x3 => (
            PresentPolicy::Sd(parse_sd_present(input.sd_present)),
            AaPolicy::ForceOff,
        ),
    };
    debug_assert!(render.covers(output) || aspect == Aspect::Wide16x9);
    Outcome::Plan(Plan {
        output,
        render,
        aspect,
        aa,
        present_policy,
    })
}

fn parse_sd_present(s: &str) -> SdPresent {
    if s.trim().eq_ignore_ascii_case("letterbox") {
        SdPresent::Letterbox
    } else {
        SdPresent::Crop
    }
}

/// The letterbox-rect detour's mode override (design R4/R5).
pub fn present_mode(policy: PresentPolicy, requested: i32) -> i32 {
    match policy {
        PresentPolicy::Stock | PresentPolicy::Sd(SdPresent::Crop) => requested,
        PresentPolicy::Sd(SdPresent::Letterbox) => {
            if requested == 1 {
                0
            } else {
                requested
            }
        }
    }
}

/// Human-readable per-frame present-chain shape implied by an AA config
/// value, for the boot log (RE of the 20260825 surface ctor + the three
/// `AfterRenderConditionImpl` vfuncs; `docs/custom_resolution.md` §3a).
/// "Full-screen ops" counts the fixed-cost passes beyond content: blits,
/// full-screen quads and clears of a screen-sized surface.
pub fn present_chain_shape(aa_config: u32) -> &'static str {
    match aa_config {
        AA_DIRECT => {
            "direct (3): 3D+2D -> display, in-place sys_copy_aa quad, present quad; 2 full-screen ops"
        }
        AA_OFF => {
            "offscreen composite (0): 3D -> RENDER, StretchRect -> render_color, sys_copy_depth quad, clear + StretchRect -> display, present quad; 5 full-screen ops"
        }
        AA_2X | AA_4X => {
            "MSAA (1/2): as offscreen composite plus a resolve StretchRect; 6 full-screen ops on multisampled surfaces"
        }
        _ => "unknown AA config",
    }
}

/// Convert a canvas-space scissor rect to render-target pixels the way the
/// 2D draw handlers convert vertices: `px = x · rt / canvas + offset_px`.
/// Rounds to nearest, clamps `x/y` into the RT and `w/h` so the rect never
/// leaves it (the engine builds `RECT{x, y, x+w, y+h}` and hands it to
/// `SetScissorRect`, which rejects out-of-target rects).
pub fn scissor_scale(
    x: u16,
    y: u16,
    w: u16,
    h: u16,
    rt: Dims,
    canvas: (f32, f32),
    offset_px: (f32, f32),
) -> (u16, u16, u16, u16) {
    if canvas.0 <= 0.0 || canvas.1 <= 0.0 || rt.w == 0 || rt.h == 0 {
        return (x, y, w, h);
    }
    let sx = rt.w as f32 / canvas.0;
    let sy = rt.h as f32 / canvas.1;
    let px0 = (x as f32 * sx + offset_px.0).round();
    let py0 = (y as f32 * sy + offset_px.1).round();
    let px1 = ((x as f32 + w as f32) * sx + offset_px.0).round();
    let py1 = ((y as f32 + h as f32) * sy + offset_px.1).round();
    let clamp = |v: f32, max: u32| -> u32 { v.max(0.0).min(max as f32) as u32 };
    let x0 = clamp(px0, rt.w);
    let y0 = clamp(py0, rt.h);
    let x1 = clamp(px1, rt.w).max(x0);
    let y1 = clamp(py1, rt.h).max(y0);
    (
        x0.min(u16::MAX as u32) as u16,
        y0.min(u16::MAX as u32) as u16,
        (x1 - x0).min(u16::MAX as u32) as u16,
        (y1 - y0).min(u16::MAX as u32) as u16,
    )
}

/// Screen height the ark draw-callback API (TEST menu / hardware check /
/// error screens) was tuned against: its `createFont` / `createSprite`
/// pick a fixed pixel scale from the MACHINE TYPE (0/1 = SD cabinets →
/// the 480-line table, anything else → the 720-line table) and never look
/// at the actual back-buffer, so the text is a constant pixel size.
pub const DEBUG_UI_REF_H_SD: u32 = 480;
pub const DEBUG_UI_REF_H_HD: u32 = 720;

/// Operator multiplier bounds for `resolution.test_menu_scale`.
pub const TEST_MENU_SCALE_MIN: f32 = 0.25;
pub const TEST_MENU_SCALE_MAX: f32 = 4.0;

/// Multiplier applied to the debug-UI font/sprite scales the game chose so
/// they keep the SAME on-screen proportion at every output height:
/// `output_h / ref_h` where `ref_h` is the height the game's chosen table
/// was tuned for, times the operator's `test_menu_scale` (clamped; NaN or
/// non-positive ⇒ 1.0). Returns `None` when the result is the identity
/// (nothing to install).
pub fn debug_ui_scale(output_h: u32, machine_is_sd: bool, user_scale: f32) -> Option<f32> {
    if output_h == 0 {
        return None;
    }
    let ref_h = if machine_is_sd {
        DEBUG_UI_REF_H_SD
    } else {
        DEBUG_UI_REF_H_HD
    };
    let user = if user_scale.is_finite() && user_scale > 0.0 {
        user_scale.clamp(TEST_MENU_SCALE_MIN, TEST_MENU_SCALE_MAX)
    } else {
        1.0
    };
    let s = output_h as f32 / ref_h as f32 * user;
    if (s - 1.0).abs() < 1e-4 {
        None
    } else {
        Some(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_ui_scale_identity_cases() {
        assert_eq!(debug_ui_scale(720, false, 1.0), None);
        assert_eq!(debug_ui_scale(480, true, 1.0), None);
        assert_eq!(debug_ui_scale(0, false, 1.0), None);
        // Bad user multipliers degrade to 1.0.
        assert_eq!(debug_ui_scale(720, false, f32::NAN), None);
        assert_eq!(debug_ui_scale(720, false, 0.0), None);
        assert_eq!(debug_ui_scale(720, false, -3.0), None);
    }

    #[test]
    fn debug_ui_scale_tracks_output_height() {
        let close = |a: Option<f32>, b: f32| (a.unwrap() - b).abs() < 1e-5;
        assert!(close(debug_ui_scale(2160, false, 1.0), 3.0));
        assert!(close(debug_ui_scale(1080, false, 1.0), 1.5));
        // HD machine on a 4:3 480-line output: the 720 table shrinks to 2/3.
        assert!(close(debug_ui_scale(480, false, 1.0), 480.0 / 720.0));
        // SD machine driven at 4K: its 480 table grows 4.5×.
        assert!(close(debug_ui_scale(2160, true, 1.0), 4.5));
    }

    #[test]
    fn debug_ui_scale_user_multiplier_clamps() {
        let close = |a: Option<f32>, b: f32| (a.unwrap() - b).abs() < 1e-5;
        assert!(close(debug_ui_scale(720, false, 1.25), 1.25));
        assert!(close(
            debug_ui_scale(720, false, 100.0),
            TEST_MENU_SCALE_MAX
        ));
        assert!(close(debug_ui_scale(720, false, 0.01), TEST_MENU_SCALE_MIN));
        assert!(close(debug_ui_scale(2160, false, 0.5), 1.5));
    }

    fn input(output: &str) -> PlanInput<'_> {
        PlanInput {
            output,
            sd_present: "crop",
        }
    }

    fn plan(o: Outcome) -> Plan {
        match o {
            Outcome::Plan(p) => p,
            other => panic!("expected Plan, got {other:?}"),
        }
    }

    #[test]
    fn t1_parse_dims_accepts_wxh_any_case_and_whitespace() {
        assert_eq!(parse_dims("1920x1080"), Some(Dims::new(1920, 1080)));
        assert_eq!(parse_dims("3840X2160"), Some(Dims::new(3840, 2160)));
        assert_eq!(parse_dims(" 640x480 "), Some(Dims::new(640, 480)));
    }

    #[test]
    fn t2_parse_dims_rejects_junk_zero_and_odd() {
        assert_eq!(parse_dims("abc"), None);
        assert_eq!(parse_dims("1920"), None);
        assert_eq!(parse_dims("0x720"), None);
        assert_eq!(parse_dims("1921x1080"), None);
        assert_eq!(parse_dims("1920x1081"), None);
    }

    #[test]
    fn t3_render_is_derived_from_aspect() {
        assert_eq!(
            render_for(Dims::new(1920, 1080), Aspect::Wide16x9),
            Dims::new(1920, 1080)
        );
        assert_eq!(
            render_for(Dims::new(3840, 2160), Aspect::Wide16x9),
            Dims::new(3840, 2160)
        );
        assert_eq!(render_for(Dims::new(640, 480), Aspect::Sd4x3), STOCK);
        assert_eq!(render_for(Dims::new(1600, 1200), Aspect::Sd4x3), STOCK);
    }

    #[test]
    fn t7_stock_is_inert() {
        assert_eq!(compute(&input("1280x720")), Outcome::Inert);
        assert_eq!(compute(&input(" 1280X720 ")), Outcome::Inert);
    }

    #[test]
    fn t8_native_16x9_keeps_the_games_aa_and_present_chain() {
        for (o, d) in [
            ("1920x1080", Dims::new(1920, 1080)),
            ("2560x1440", Dims::new(2560, 1440)),
            ("3840x2160", Dims::new(3840, 2160)),
            ("1366x768", Dims::new(1366, 768)),
            ("640x360", Dims::new(640, 360)),
        ] {
            let p = plan(compute(&input(o)));
            assert_eq!(p.output, d, "{o}");
            assert_eq!(p.render, d, "{o}");
            assert_eq!(p.aspect, Aspect::Wide16x9, "{o}");
            assert_eq!(p.present_policy, PresentPolicy::Stock, "{o}");
            assert_eq!(p.aa, AaPolicy::Stock, "{o}");
            assert!(!p.force_aa_zero(), "{o}");
            assert!(!p.render_is_stock(), "{o}");
            assert!(!p.output_is_stock(), "{o}");
        }
    }

    #[test]
    fn t11_sd_crop_default_forces_aa_off_and_stock_render() {
        let p = plan(compute(&input("640x480")));
        assert_eq!(p.aspect, Aspect::Sd4x3);
        assert_eq!(p.render, STOCK);
        assert!(p.render_is_stock());
        assert!(!p.output_is_stock());
        assert_eq!(p.present_policy, PresentPolicy::Sd(SdPresent::Crop));
        assert_eq!(p.aa, AaPolicy::ForceOff);
        assert!(p.force_aa_zero());
        // The stock 720p depth legally backs the 480p colour target.
        assert!(p.render.covers(p.output));
    }

    #[test]
    fn t12_sd_letterbox() {
        let i = PlanInput {
            output: "640x480",
            sd_present: "LetterBox",
        };
        let p = plan(compute(&i));
        assert_eq!(p.render, STOCK);
        assert_eq!(p.present_policy, PresentPolicy::Sd(SdPresent::Letterbox));
        assert_eq!(p.aa, AaPolicy::ForceOff);
        // Unknown strings fall back to the stock crop.
        let i = PlanInput {
            output: "640x480",
            sd_present: "banana",
        };
        assert_eq!(
            plan(compute(&i)).present_policy,
            PresentPolicy::Sd(SdPresent::Crop)
        );
    }

    #[test]
    fn t13_t15_rejections() {
        assert!(matches!(compute(&input("2560x1080")), Outcome::Rejected(_)));
        assert!(matches!(
            compute(&input("10240x5760")),
            Outcome::Rejected(_)
        ));
        assert!(matches!(compute(&input("480x270")), Outcome::Rejected(_)));
        assert!(matches!(compute(&input("nope")), Outcome::Rejected(_)));
        assert!(matches!(compute(&input("1921x1080")), Outcome::Rejected(_)));
    }

    #[test]
    fn t18_present_mode_table() {
        for m in [0, 1, 2] {
            assert_eq!(present_mode(PresentPolicy::Stock, m), m);
            assert_eq!(present_mode(PresentPolicy::Sd(SdPresent::Crop), m), m);
        }
        assert_eq!(present_mode(PresentPolicy::Sd(SdPresent::Letterbox), 1), 0);
        assert_eq!(present_mode(PresentPolicy::Sd(SdPresent::Letterbox), 0), 0);
        assert_eq!(present_mode(PresentPolicy::Sd(SdPresent::Letterbox), 2), 2);
    }

    #[test]
    fn t19_scissor_identity() {
        assert_eq!(
            scissor_scale(100, 50, 300, 200, STOCK, (1280.0, 720.0), (0.0, 0.0)),
            (100, 50, 300, 200)
        );
    }

    #[test]
    fn t20_t21_scissor_scales_and_offsets() {
        assert_eq!(
            scissor_scale(
                100,
                50,
                300,
                200,
                Dims::new(1920, 1080),
                (1280.0, 720.0),
                (0.0, 0.0)
            ),
            (150, 75, 450, 300)
        );
        assert_eq!(
            scissor_scale(
                100,
                50,
                300,
                200,
                Dims::new(3840, 2160),
                (1280.0, 720.0),
                (10.0, 20.0)
            ),
            (310, 170, 900, 600)
        );
    }

    #[test]
    fn t22_scissor_clamps_to_rt() {
        let (x, y, w, h) = scissor_scale(
            1200,
            700,
            300,
            100,
            Dims::new(1920, 1080),
            (1280.0, 720.0),
            (0.0, 0.0),
        );
        assert_eq!((x, y), (1800, 1050));
        assert_eq!(x as u32 + w as u32, 1920);
        assert_eq!(y as u32 + h as u32, 1080);
        // Degenerate canvas → passthrough.
        assert_eq!(
            scissor_scale(1, 2, 3, 4, Dims::new(1920, 1080), (0.0, 720.0), (0.0, 0.0)),
            (1, 2, 3, 4)
        );
    }

    #[test]
    fn t23_present_chain_shape_names_every_mode() {
        assert!(present_chain_shape(AA_DIRECT).starts_with("direct (3)"));
        assert!(present_chain_shape(AA_OFF).starts_with("offscreen composite (0)"));
        assert!(present_chain_shape(AA_2X).starts_with("MSAA"));
        assert!(present_chain_shape(AA_4X).starts_with("MSAA"));
        assert_eq!(present_chain_shape(7), "unknown AA config");
    }
}
