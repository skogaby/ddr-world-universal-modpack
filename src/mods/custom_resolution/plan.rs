//! Pure resolution model for the Custom Resolution mod (design §4.1).
//!
//! Everything here is a function of config strings and integer dimensions —
//! no game memory, no `crate::` imports — so the module is a dependency-free
//! leaf that `scripts/validate_custom_resolution.sh` mounts into a host crate
//! and unit-tests. The impure layers (`patches`, `present`, `scissor`,
//! `canvas_fix`) consume the [`Plan`] this module produces and never re-derive
//! any of its rules.
//!
//! Vocabulary: **output** = the D3D9 back-buffer / display surface size (what
//! the panel receives); **render** = the size of the game's internal 1280×720
//! surfaces and list viewports (what geometry rasterises at). The stock game
//! has both at 1280×720; the logical 1280×720 *canvas* never changes.

/// Stock render/output size — the only configuration that is a literal no-op.
pub const STOCK: Dims = Dims { w: 1280, h: 720 };

/// Largest side any surface, viewport or scissor rect can carry: every
/// dimension in the engine's RT/viewport/scissor structs is a `u16`, and 8K
/// (7680×4320) already sits at the edge of what D3D9-era surfaces allow.
pub const MAX_SIDE: u32 = 8192;

/// Smallest accepted output height (a 640×360 window is the smallest thing the
/// letterbox math degrades gracefully into).
pub const MIN_HEIGHT: u32 = 360;

/// Feature gates flipped by the plan's steps: without the letterbox present
/// policy (plan Step 5) a 16:9 output with a smaller render would be CROPPED
/// by the game's per-scene mode-1 selection; without the native render set
/// (Step 6) no surface can be created at a non-stock size. The pure layer
/// refuses those configurations while a gate is off so a cabinet never sees
/// a half-implemented picture. Both shipped as of Step 6; the gates stay so
/// a build can be cut back to a known-good subset by flipping one constant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gates {
    pub letterbox_policy: bool,
    pub native_render: bool,
}

/// The gates as shipped by the current build.
pub const GATES: Gates = Gates {
    letterbox_policy: true,
    native_render: true,
};

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
    /// depth to a 640×480 colour surface).
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

/// The engine's two SD present modes (`FUN_1801f3f60` `mode` argument):
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
    /// 1:1 POINT copy regardless of mode — no detour needed.
    Stock,
    /// 16:9 output larger/smaller than the render: every request becomes mode
    /// 0 so the width-fit letterbox (full-screen for matching aspect) is used
    /// and the 960-px crop can never fire.
    ForceLetterbox,
    /// 4:3 output: mode-1 requests become the operator's choice; mode 0 (the
    /// TEST menu) is always honoured.
    Sd(SdPresent),
}

/// The game's AA config (`display struct +0x18` → `DAT_1806f050c`): 0 none,
/// 1 = 2× MSAA, 2 = 4× MSAA on the RENDER surfaces, 3 = "direct mode" (no
/// MSAA; RENDER/PRESENT target the screen-sized display surface and the
/// COPYVIEWPORT scaler is SKIPPED — only ever selected for `1 < pcType < 5`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AaPolicy {
    /// Leave whatever onBoot chose (0, or 3 on pcType-2..4 cabinets).
    Stock,
    /// Write this value into the display struct before graphics init.
    Force(u8),
}

pub const AA_OFF: u8 = 0;
pub const AA_2X: u8 = 1;
pub const AA_4X: u8 = 2;
pub const AA_DIRECT: u8 = 3;

/// Parse the config's `msaa` string. `"auto"` is the pre-2026-09-07 name for
/// `"off"`; unknown strings fall back to `"off"` (the safe choice).
pub fn parse_msaa(s: &str) -> AaPolicy {
    let s = s.trim();
    if s.eq_ignore_ascii_case("stock") {
        AaPolicy::Stock
    } else if s.eq_ignore_ascii_case("2x") {
        AaPolicy::Force(AA_2X)
    } else if s.eq_ignore_ascii_case("4x") {
        AaPolicy::Force(AA_4X)
    } else {
        AaPolicy::Force(AA_OFF)
    }
}

/// What to do with the PRESENT render-target's depth surface (design R12).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresentDepth {
    /// The stock 720p `render_depth` stays bound (render covers output).
    Stock,
    /// Output exceeds the render in some dimension: create an output-sized
    /// depth surface for the PRESENT rt.
    CreateOutputSized,
}

/// The fully resolved boot plan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub output: Dims,
    pub render: Dims,
    pub aspect: Aspect,
    pub aa: AaPolicy,
    pub present_policy: PresentPolicy,
    pub present_depth: PresentDepth,
    pub redirect_afp_projection: bool,
    /// The operator asked for a non-720p render with a 4:3 output; it was
    /// coerced back to 1280×720 (one INFO at boot).
    pub coerced_render: bool,
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
        self.aa == AaPolicy::Force(AA_OFF)
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
    pub render: &'a str,
    pub sd_present: &'a str,
    pub msaa: &'a str,
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

/// Resolve the `render` spec against the output: `"output"`, `"WxH"`, or
/// `"NN%"` (1..=200, rounded to the nearest even pixel).
pub fn resolve_render(spec: &str, output: Dims) -> Option<Dims> {
    let spec = spec.trim();
    if spec.eq_ignore_ascii_case("output") {
        return Some(output);
    }
    if let Some(pct) = spec.strip_suffix('%') {
        let pct: u32 = pct.trim().parse().ok()?;
        if pct == 0 || pct > 200 {
            return None;
        }
        return Some(Dims {
            w: scale_even(output.w, pct),
            h: scale_even(output.h, pct),
        });
    }
    parse_dims(spec)
}

fn scale_even(v: u32, pct: u32) -> u32 {
    let scaled = (v as u64 * pct as u64 + 50) / 100;
    let scaled = scaled as u32;
    scaled - (scaled % 2)
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

/// Compute the boot plan with the build's shipped [`GATES`].
pub fn compute(input: &PlanInput) -> Outcome {
    compute_gated(input, GATES)
}

/// Compute the boot plan with explicit gates (tests exercise both states).
pub fn compute_gated(input: &PlanInput, gates: Gates) -> Outcome {
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
    let Some(requested_render) = resolve_render(input.render, output) else {
        return Outcome::Rejected(format!(
            "resolution.render '{}' is not 'output', WxH, or NN%",
            input.render
        ));
    };
    if requested_render.w > MAX_SIDE || requested_render.h > MAX_SIDE {
        return Outcome::Rejected(format!(
            "resolution.render {}x{} exceeds the {} px per-side limit",
            requested_render.w, requested_render.h, MAX_SIDE
        ));
    }

    let (render, coerced_render) = match aspect {
        Aspect::Sd4x3 => (STOCK, requested_render != STOCK),
        Aspect::Wide16x9 => (requested_render, false),
    };

    if output == STOCK && render == STOCK {
        return Outcome::Inert;
    }

    if render != STOCK && !gates.native_render {
        return Outcome::Rejected(format!(
            "resolution.render {}x{}: native (non-720p) rendering is not available in this build yet — set render to \"1280x720\" or wait for the native-render step",
            render.w, render.h
        ));
    }

    let present_policy = match aspect {
        Aspect::Sd4x3 => PresentPolicy::Sd(parse_sd_present(input.sd_present)),
        Aspect::Wide16x9 if render == output => PresentPolicy::Stock,
        Aspect::Wide16x9 => PresentPolicy::ForceLetterbox,
    };
    if present_policy == PresentPolicy::ForceLetterbox && !gates.letterbox_policy {
        return Outcome::Rejected(format!(
            "resolution.output {}x{} with render {}x{}: the letterbox present policy is not available in this build yet — a 16:9 output needs render == output",
            output.w, output.h, render.w, render.h
        ));
    }

    let aa = parse_msaa(input.msaa);
    if aa == AaPolicy::Stock && render != output {
        return Outcome::Rejected(format!(
            "resolution.msaa \"stock\" with render {}x{} != output {}x{}: a pcType-2..4 cabinet boots in AA \"direct\" mode (3), which skips the present-chain scaler the render/output split needs — use \"off\", \"2x\" or \"4x\"",
            render.w, render.h, output.w, output.h
        ));
    }

    let present_depth = if render.covers(output) {
        PresentDepth::Stock
    } else {
        PresentDepth::CreateOutputSized
    };
    Outcome::Plan(Plan {
        output,
        render,
        aspect,
        aa,
        present_policy,
        present_depth,
        redirect_afp_projection: render != output,
        coerced_render,
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
        PresentPolicy::ForceLetterbox => 0,
        PresentPolicy::Sd(SdPresent::Letterbox) => {
            if requested == 1 {
                0
            } else {
                requested
            }
        }
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

    const ALL_ON: Gates = Gates {
        letterbox_policy: true,
        native_render: true,
    };
    const ALL_OFF: Gates = Gates {
        letterbox_policy: false,
        native_render: false,
    };

    fn input<'a>(output: &'a str, render: &'a str) -> PlanInput<'a> {
        PlanInput {
            output,
            render,
            sd_present: "crop",
            msaa: "auto",
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
    fn t3_t6_resolve_render() {
        let p1080 = Dims::new(1920, 1080);
        assert_eq!(resolve_render("output", p1080), Some(p1080));
        assert_eq!(resolve_render("OUTPUT", p1080), Some(p1080));
        assert_eq!(resolve_render("75%", p1080), Some(Dims::new(1440, 810)));
        assert_eq!(
            resolve_render("50%", Dims::new(2560, 1440)),
            Some(Dims::new(1280, 720))
        );
        assert_eq!(
            resolve_render("1280x720", Dims::new(3840, 2160)),
            Some(STOCK)
        );
        assert_eq!(resolve_render("foo", p1080), None);
        assert_eq!(resolve_render("0%", p1080), None);
        assert_eq!(resolve_render("300%", p1080), None);
        // odd intermediate rounds down to even
        assert_eq!(
            resolve_render("33%", Dims::new(1280, 720)),
            Some(Dims::new(422, 238))
        );
    }

    #[test]
    fn t7_stock_is_inert() {
        assert_eq!(
            compute_gated(&input("1280x720", "output"), ALL_ON),
            Outcome::Inert
        );
        assert_eq!(
            compute_gated(&input("1280x720", "1280x720"), ALL_OFF),
            Outcome::Inert
        );
        assert_eq!(
            compute_gated(&input("1280x720", "100%"), ALL_OFF),
            Outcome::Inert
        );
    }

    #[test]
    fn t8_native_1080p() {
        let p = plan(compute_gated(&input("1920x1080", "output"), ALL_ON));
        assert_eq!(p.output, Dims::new(1920, 1080));
        assert_eq!(p.render, Dims::new(1920, 1080));
        assert_eq!(p.aspect, Aspect::Wide16x9);
        assert_eq!(p.present_policy, PresentPolicy::Stock);
        assert_eq!(p.present_depth, PresentDepth::Stock);
        assert!(p.force_aa_zero());
        assert!(!p.redirect_afp_projection);
        assert!(!p.coerced_render);
        assert!(!p.render_is_stock());
        assert!(!p.output_is_stock());
    }

    #[test]
    fn t9_1080p_output_720p_render_is_tier_a() {
        let p = plan(compute_gated(&input("1920x1080", "1280x720"), ALL_ON));
        assert_eq!(p.render, STOCK);
        assert_eq!(p.present_policy, PresentPolicy::ForceLetterbox);
        assert_eq!(p.present_depth, PresentDepth::CreateOutputSized);
        assert!(p.redirect_afp_projection);
        assert!(p.force_aa_zero());
        assert!(p.render_is_stock());
    }

    #[test]
    fn t10_4k_with_percent_render() {
        let p = plan(compute_gated(&input("3840x2160", "75%"), ALL_ON));
        assert_eq!(p.render, Dims::new(2880, 1620));
        assert_eq!(p.present_policy, PresentPolicy::ForceLetterbox);
        assert_eq!(p.present_depth, PresentDepth::CreateOutputSized);
        assert!(p.redirect_afp_projection);
    }

    #[test]
    fn t11_sd_crop_default() {
        let p = plan(compute_gated(&input("640x480", "output"), ALL_OFF));
        assert_eq!(p.aspect, Aspect::Sd4x3);
        assert_eq!(p.render, STOCK);
        assert_eq!(p.present_policy, PresentPolicy::Sd(SdPresent::Crop));
        assert_eq!(p.present_depth, PresentDepth::Stock);
        assert!(p.redirect_afp_projection);
        assert!(p.force_aa_zero());
        // "output" would be 640x480 ≠ 1280x720 → coerced (the operator did not
        // ask for anything, but the resolver still produced a non-stock size).
        assert!(p.coerced_render);
        let p = plan(compute_gated(&input("640x480", "1280x720"), ALL_OFF));
        assert!(!p.coerced_render);
    }

    #[test]
    fn t12_sd_letterbox_and_coercion() {
        let i = PlanInput {
            output: "640x480",
            render: "50%",
            sd_present: "letterbox",
            msaa: "auto",
        };
        let p = plan(compute_gated(&i, ALL_OFF));
        assert_eq!(p.render, STOCK);
        assert!(p.coerced_render);
        assert_eq!(p.present_policy, PresentPolicy::Sd(SdPresent::Letterbox));
    }

    #[test]
    fn t13_t15_rejections() {
        assert!(matches!(
            compute_gated(&input("2560x1080", "output"), ALL_ON),
            Outcome::Rejected(_)
        ));
        assert!(matches!(
            compute_gated(&input("10240x5760", "output"), ALL_ON),
            Outcome::Rejected(_)
        ));
        assert!(matches!(
            compute_gated(&input("480x270", "output"), ALL_ON),
            Outcome::Rejected(_)
        ));
        assert!(matches!(
            compute_gated(&input("640x360", "output"), ALL_ON),
            Outcome::Plan(_)
        ));
        assert!(matches!(
            compute_gated(&input("1366x768", "output"), ALL_ON),
            Outcome::Plan(_)
        ));
        assert!(matches!(
            compute_gated(&input("nope", "output"), ALL_ON),
            Outcome::Rejected(_)
        ));
        assert!(matches!(
            compute_gated(&input("1920x1080", "banana"), ALL_ON),
            Outcome::Rejected(_)
        ));
    }

    #[test]
    fn t16_msaa_policies() {
        let mk = |msaa: &'static str| PlanInput {
            output: "1920x1080",
            render: "output",
            sd_present: "crop",
            msaa,
        };
        assert_eq!(
            plan(compute_gated(&mk("stock"), ALL_ON)).aa,
            AaPolicy::Stock
        );
        assert!(!plan(compute_gated(&mk("stock"), ALL_ON)).force_aa_zero());
        assert_eq!(
            plan(compute_gated(&mk("auto"), ALL_ON)).aa,
            AaPolicy::Force(AA_OFF)
        );
        assert_eq!(
            plan(compute_gated(&mk("off"), ALL_ON)).aa,
            AaPolicy::Force(AA_OFF)
        );
        assert_eq!(
            plan(compute_gated(&mk("2X"), ALL_ON)).aa,
            AaPolicy::Force(AA_2X)
        );
        assert_eq!(
            plan(compute_gated(&mk("4x"), ALL_ON)).aa,
            AaPolicy::Force(AA_4X)
        );
        assert_eq!(
            plan(compute_gated(&mk("banana"), ALL_ON)).aa,
            AaPolicy::Force(AA_OFF)
        );
        // Stock AA can be direct mode (3), which has no scaler: refused when
        // the render and output differ.
        let split = PlanInput {
            output: "3840x2160",
            render: "1920x1080",
            sd_present: "crop",
            msaa: "stock",
        };
        assert!(matches!(
            compute_gated(&split, ALL_ON),
            Outcome::Rejected(_)
        ));
        let sd = PlanInput {
            output: "640x480",
            render: "output",
            sd_present: "crop",
            msaa: "stock",
        };
        assert!(matches!(compute_gated(&sd, ALL_ON), Outcome::Rejected(_)));
    }

    #[test]
    fn t17_gates_refuse_unshipped_paths() {
        match compute_gated(&input("1920x1080", "output"), ALL_OFF) {
            Outcome::Rejected(msg) => assert!(msg.contains("native"), "{msg}"),
            other => panic!("{other:?}"),
        }
        match compute_gated(&input("1920x1080", "1280x720"), ALL_OFF) {
            Outcome::Rejected(msg) => assert!(msg.contains("letterbox"), "{msg}"),
            other => panic!("{other:?}"),
        }
        // Letterbox gate alone unlocks Tier A but not native render.
        let lb_only = Gates {
            letterbox_policy: true,
            native_render: false,
        };
        assert!(matches!(
            compute_gated(&input("1920x1080", "1280x720"), lb_only),
            Outcome::Plan(_)
        ));
        assert!(matches!(
            compute_gated(&input("1920x1080", "output"), lb_only),
            Outcome::Rejected(_)
        ));
        // SD never needs either gate.
        assert!(matches!(
            compute_gated(&input("640x480", "output"), ALL_OFF),
            Outcome::Plan(_)
        ));
        // Shipped gates: letterbox policy (plan Step 5) and native render
        // (Step 6) both landed.
        assert_eq!(GATES, ALL_ON);
    }

    #[test]
    fn t18_present_mode_table() {
        for m in [0, 1, 2] {
            assert_eq!(present_mode(PresentPolicy::Stock, m), m);
            assert_eq!(present_mode(PresentPolicy::Sd(SdPresent::Crop), m), m);
            assert_eq!(present_mode(PresentPolicy::ForceLetterbox, m), 0);
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
}
