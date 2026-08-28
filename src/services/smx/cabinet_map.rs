//! DDR light state → SMX **cabinet** payloads (pure).
//!
//! Direct port of SpiceManiaX's `lights_utils.cpp` cabinet half
//! (`HandleMarqueeLightsUpdate` / `HandleVerticalStripLightsUpdate` /
//! `HandleSpotlightLightsUpdate`), fed from the captured [`DdrLightFrame`]
//! instead of SpiceAPI:
//!
//! - **Marquee**: the 40-LED `top_panel` tape strip resampled onto the 12
//!   physical SMX marquee LEDs (many→few) with a prefer-lit,
//!   coverage-scaled box filter — see [`map_marquee`]. The payload is the
//!   SDK's 24-triplet logical size; the physical LEDs live at payload
//!   slots 0..=11 (hardware-probed).
//! - **Vertical strips**: the 26-LED `monitor_left`/`monitor_right` tape
//!   strips onto 28 SMX LEDs each (few→many: linear interpolation — see
//!   [`map_strip`]).
//! - **Spotlights**: the GOLD P1/P2 *woofer-corner* lamp brightness → 8
//!   white LEDs per side (P1 = left spotlights, P2 = right).
//!
//! Woofer-corner source (Ghidra-confirmed, `arkmdxbio2_20260721`): the
//! woofer corners ride `arkMDXChangeDimlamp` ids **19 (P1)** and **20 (P2)**
//! — the GOLD flush's staging table (`0x1800f7a60`, `{group, idx, id}`
//! triples) stages id 19 → slot (0,11) and id 20 → slot (1,11), which
//! `FUN_180085bc0` maps (via the `0x180115c90` pair table) to BI2A LED
//! indices 31/32 = spice2x's `GOLD P1/P2 Woofer Corner`. We read the raw
//! 0..255 dimlamp value; note SpiceManiaX's SpiceAPI read was effectively
//! binary (spice2x normalizes those two lamps with `max=0` → inf → clamped
//! to 1.0), so proportional brightness here is a strict improvement.
//!
//! Port fidelity notes (D6 — SpiceManiaX mapping as the validated baseline;
//! maintainer-approved improvements 2026-08-27, each landed only after a
//! cabinet deploy confirmed parity/behavior of what it replaced):
//!
//! - **Marquee resampler** (deploy #14): SpiceManiaX's integer `MapValue`
//!   binning + iterative pairwise averaging was order-biased (the
//!   last-arriving source LED in a bin carried 50% of the result) and
//!   stepped sweeps between bins. Replaced by the prefer-lit,
//!   coverage-scaled box filter in [`map_marquee`].
//! - **Marquee payload placement** (deploy #14, hardware-probed with
//!   `smx_marquee_probe`): the physical marquee is 12 LEDs at payload slots
//!   **0..=11** (slot 0 = right edge, slot 11 = left edge; 12..=31 drive
//!   nothing). SpiceManiaX's `MapValue(…, 12, 0)` wrote slots 1..=12 — the
//!   right-edge LED never lit and the DDR-start bin landed on the void
//!   slot 12. We map bins 0..=11 → slots 11..=0: same visual direction
//!   (DDR start → left edge), all 12 LEDs live.
//! - **Strip upsampler** (deploy #15): SpiceManiaX point-sampled each SMX
//!   LED from one DDR LED via `MapValue(smx_i, 0, 28, 25, 0)` — uneven
//!   nearest-neighbor duplication (gradient banding, jumpy sweeps) that
//!   also never read DDR LED 0 (the `25` constant vs the 26 physical
//!   monitor LEDs). Replaced by linear interpolation over the full 26-LED
//!   strip in [`map_strip`], same direction (SMX 0 ↔ DDR end).
//! - The spotlights remain a verbatim port (modulo the proportional
//!   brightness noted above).

use super::light_map::TAPE_LEDS;

/// `DdrLightFrame.tape` indices for the cabinet strips (spice2x
/// `DDR_TAPELEDS`: 8 = top panel, 9/10 = monitor left/right).
pub const TAPE_TOP_PANEL: usize = 8;
pub const TAPE_MONITOR_LEFT: usize = 9;
pub const TAPE_MONITOR_RIGHT: usize = 10;

/// `arkMDXChangeDimlamp` ids of the GOLD woofer-corner lamps
/// (`[P1, P2]` — P1 drives the LEFT spotlights, P2 the RIGHT).
pub const WOOFER_DIMLAMP: [usize; 2] = [19, 20];

/// SMX marquee logical LED count (`kSmxMarqueeLogicalLedCount`) — the
/// payload size the dedicated-cabinet-lights API expects for MARQUEE.
pub const SMX_MARQUEE_LEDS: usize = 24;
/// SMX marquee *physical* LED count (`kSmxMarqueePhysicalLedCount`,
/// hardware-probed) — the resampler's output bins, written to payload
/// slots 11..=0.
pub const SMX_MARQUEE_MAPPED: usize = 12;
/// SMX vertical-strip LED count (`kSmxVerticalStripLedCount`).
pub const SMX_STRIP_LEDS: usize = 28;
/// SMX spotlight LED count per side (`kSmxSpotlightLedCount`).
pub const SMX_SPOTLIGHT_LEDS: usize = 8;

/// DDR top-panel LED count (`kDdrTopPanelLedCount`).
pub const DDR_TOP_PANEL_LEDS: usize = 40;
/// DDR monitor-strip physical LED count (the ark's GOLD per-group LED-count
/// table `[50,50,50,50, 0, 40, 26, 26]` — 26 per monitor). SpiceManiaX's
/// map constant was 25, skipping LED 0; the interpolating [`map_strip`]
/// spans all 26.
pub const DDR_STRIP_LEDS: usize = 26;

/// Map the 40 DDR top-panel LEDs onto the SMX marquee (24-triplet payload;
/// the 12 physical LEDs live at slots 0..=11 — hardware-probed, slot 0 =
/// right edge — written here as bins 0..=11 → slots 11..=0 so DDR's start
/// renders at the left edge, the direction the SpiceManiaX baseline
/// established).
///
/// **Prefer-lit, coverage-scaled box resampler** (supersedes the verbatim
/// SpiceManiaX blend; see the module docs). In source-LED units, source
/// LED `s` occupies `[s, s+1)` and marquee LED `m` the window
/// `[m·R, (m+1)·R)` with `R = 40/12`. Dark sources contribute nothing;
/// each lit source adds its fractional window overlap `w`:
///
/// ```text
/// out[m] = Σ(w · rgb) / max(Σ w, 1.0)      (lit sources only)
/// ```
///
/// Consequences:
/// - a bin holding ≥ 1 source LED's worth of lit coverage renders the
///   full-brightness weighted mean — dark neighbors never dilute a sweep
///   (SpiceManiaX's prefer-lit idea, kept);
/// - sub-LED lit coverage scales brightness linearly, so a sweeping pixel
///   cross-fades between adjacent marquee LEDs instead of stepping every
///   3–4 source positions;
/// - order-independent — the old iterative pairwise average weighted the
///   bin's last-arriving source at 50% and each earlier one exponentially
///   less.
pub fn map_marquee(top_panel: &[[u8; 3]; TAPE_LEDS]) -> [u8; SMX_MARQUEE_LEDS * 3] {
    /// Source LEDs covered by one marquee LED.
    const R: f32 = DDR_TOP_PANEL_LEDS as f32 / SMX_MARQUEE_MAPPED as f32;

    let mut sum = [[0.0f32; 3]; SMX_MARQUEE_MAPPED];
    let mut lit_weight = [0.0f32; SMX_MARQUEE_MAPPED];

    for s in 0..DDR_TOP_PANEL_LEDS {
        let [r, g, b] = top_panel[s];
        if r == 0 && g == 0 && b == 0 {
            continue; // prefer-lit: dark sources contribute nothing
        }
        let src_lo = s as f32;
        let src_hi = src_lo + 1.0;
        // The 1-wide source span overlaps at most two R-wide bins; the
        // w <= 0 guard absorbs any float-boundary over-inclusion.
        let first = ((src_lo / R) as usize).min(SMX_MARQUEE_MAPPED);
        let last = ((src_hi / R).ceil() as usize).min(SMX_MARQUEE_MAPPED);
        for m in first..last {
            let bin_lo = m as f32 * R;
            let w = (src_hi.min(bin_lo + R) - src_lo.max(bin_lo)).max(0.0);
            if w <= 0.0 {
                continue;
            }
            sum[m][0] += w * r as f32;
            sum[m][1] += w * g as f32;
            sum[m][2] += w * b as f32;
            lit_weight[m] += w;
        }
    }

    let mut out = [0u8; SMX_MARQUEE_LEDS * 3];
    for m in 0..SMX_MARQUEE_MAPPED {
        if lit_weight[m] <= 0.0 {
            continue;
        }
        // Physical placement (hardware-probed 2026-08-27, smx_marquee_probe):
        // the marquee has exactly 12 LEDs at payload slots 0..=11, slot 0 =
        // RIGHT edge, slot 11 = LEFT edge; slots 12..=31 drive nothing.
        // bin 0 (DDR start) → slot 11 (left edge), bin 11 (DDR end) →
        // slot 0 (right edge) — same visual direction as the validated
        // SpiceManiaX baseline, minus its off-by-one (it wrote slots 1..=12:
        // the right-edge LED never lit and the DDR-start bin went to the
        // void slot 12).
        let o = (SMX_MARQUEE_MAPPED - 1 - m) * 3;
        let denom = lit_weight[m].max(1.0);
        for c in 0..3 {
            out[o + c] = (sum[m][c] / denom).round().clamp(0.0, 255.0) as u8;
        }
    }
    out
}

/// Map one 26-LED monitor strip onto the 28 SMX vertical-strip LEDs
/// (few→many upsample).
///
/// **Linear interpolation** (supersedes SpiceManiaX's nearest-neighbor
/// `MapValue` repeat; see the module docs): each SMX LED's center is placed
/// at its fractional position along the reversed source strip and blends
/// the two neighboring DDR LEDs. Uniform fills reproduce exactly, gradients
/// lose the uneven duplication banding, and a sweeping pixel cross-fades
/// between SMX LEDs instead of jumping in irregular 1-or-2-LED steps.
/// Prefer-lit deliberately does NOT apply here: with at most two sources
/// per output, blending toward a dark neighbor IS the cross-fade.
///
/// Direction matches the validated baseline (SMX LED 0 ↔ DDR strip end);
/// unlike the SpiceManiaX map it spans ALL 26 physical LEDs (the old `25`
/// constant never displayed DDR LED 0).
pub fn map_strip(monitor: &[[u8; 3]; TAPE_LEDS]) -> [u8; SMX_STRIP_LEDS * 3] {
    let mut out = [0u8; SMX_STRIP_LEDS * 3];
    let last = DDR_STRIP_LEDS as i32 - 1;
    for smx_i in 0..SMX_STRIP_LEDS {
        // SMX LED center in [0, 1), reversed onto the source strip, then
        // into source LED-CENTER coordinates (LED i's center sits at i).
        let t = 1.0 - (smx_i as f32 + 0.5) / SMX_STRIP_LEDS as f32;
        let p = t * DDR_STRIP_LEDS as f32 - 0.5;
        let p0 = p.floor();
        let frac = p - p0;
        // Clamp at the strip ends (the half-LED overhang past the first/
        // last source center resolves to that end LED).
        let i0 = (p0 as i32).clamp(0, last) as usize;
        let i1 = (p0 as i32 + 1).clamp(0, last) as usize;
        let a = monitor[i0];
        let b = monitor[i1];
        let o = smx_i * 3;
        for c in 0..3 {
            let v = a[c] as f32 * (1.0 - frac) + b[c] as f32 * frac;
            out[o + c] = v.round().clamp(0.0, 255.0) as u8;
        }
    }
    out
}

/// One side's spotlights: 8 white LEDs at the woofer-corner brightness
/// (`HandleSpotlightLightsUpdate`).
pub fn map_spotlights(brightness: u8) -> [u8; SMX_SPOTLIGHT_LEDS * 3] {
    [brightness; SMX_SPOTLIGHT_LEDS * 3]
}
