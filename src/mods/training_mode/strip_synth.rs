//! Pure chart-strip synthesis (Training Mode Step 6, design R7 as amended
//! 2026-08-14): turns (note vector, arrow sheet, palette, layout params)
//! into the finished strip image bytes. Four independent pure layers the
//! engine-facing driver (task-02) composes:
//!
//! 1. [`extract_sheet`] — `2d_arrowNN.arc` bytes → the 768×192 indexed
//!    RGBA sheet (ARC parse → AVSLZ decompress → DDS validation).
//! 2. [`StripLayout`] — strip geometry + the `content_ms → y` mapping
//!    (+ [`format_mss`] and the marker-fraction helpers task-03 reuses).
//! 3. [`render_strip`] — the glyph rasterizer (cell select, rotate,
//!    palette resolve, box-downscale, alpha-blend stamp).
//! 4. [`encode_png`] — RGBA8 → PNG bytes.
//!
//! Zero engine calls; everything injected (the palette rows and per-note
//! palette-row choices come from task-02's live evaluator/selector walk —
//! this layer NEVER computes the game's color math, per the maintainer's
//! future-proofing constraint). Host-tested through the harness mount
//! (section_math's model).
//!
//! Format facts (verified against real assets 2026-08-14 — research
//! `docs/chart_strip_hud_research.md` §3): the DDS payload is
//! uncompressed A8R8G8B8 768×192 whose RGB channels carry PALETTE
//! INDICES (red = palette U); 96×96 cells — tap `[0..96]²` (baked LEFT),
//! freeze head `[96..192]×[0..96]`, shock variants at x=192/288, freeze
//! bottom caps at `x = col·96`, rows `[96..192]` (direction baked),
//! freeze body columns at `x = col·96 + 384` (direction baked, tiles
//! vertically). Color identity is entirely `palette[row][atlas.red]`
//! with coverage in atlas alpha — so color is resolved BEFORE any
//! downscale (blending indices is the classic palette-art bug the
//! shader-fixes work exists to avoid).

use image::RgbaImage;

use crate::core::arc;

// ── Sheet geometry constants ─────────────────────────────────────────

/// Arrow sheet pixel width (validated on extraction).
pub const SHEET_W: u32 = 768;
/// Arrow sheet pixel height.
pub const SHEET_H: u32 = 192;
/// Glyph cell edge (the sheet is a grid of 96×96 cells).
pub const CELL: u32 = 96;

// ── Errors ───────────────────────────────────────────────────────────

/// Extraction/encode failure. The pure layer never logs — callers turn
/// [`describe`](StripError::describe) into their one WARN.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StripError {
    /// ARC parse/extract failed (bad magic, corrupt cue, AVSLZ failure,
    /// or no inner entry).
    Arc,
    /// The inner DDS is not the expected uncompressed A8R8G8B8 768×192.
    Dds(&'static str),
    /// PNG encoding failed.
    Png,
}

impl StripError {
    /// Static description for the caller's WARN line.
    #[must_use]
    pub fn describe(self) -> &'static str {
        match self {
            StripError::Arc => "arc parse/extract failed",
            StripError::Dds(what) => what,
            StripError::Png => "png encode failed",
        }
    }
}

// ── §1 Sheet extraction ──────────────────────────────────────────────

/// Extract the arrow sheet from raw `2d_arrowNN.arc` bytes: first ARC
/// entry → AVSLZ decompress (via [`arc::extract`]) → DDS validation →
/// indexed RGBA image (memory-order BGRA → RGBA; the "colors" are
/// palette indices passed through untouched).
pub fn extract_sheet(arc_bytes: &[u8]) -> Result<RgbaImage, StripError> {
    let entries = arc::parse(arc_bytes).ok_or(StripError::Arc)?;
    let entry = entries.first().ok_or(StripError::Arc)?;
    let dds = arc::extract(arc_bytes, entry).ok_or(StripError::Arc)?;
    decode_dds(&dds, SHEET_W, SHEET_H)
}

/// Extract frame 0 of the stock shock-lightning strike from raw
/// `2d_shock_effect00.arc` bytes: the arc carries three
/// `shock_effect00_{s,m,l}.dds` size variants (768×384 A8R8G8B8, a 2×4
/// grid of 384×96 frames — ONE contiguous horizontal strike spanning
/// all four panels per frame; the engine overlays it additively across
/// the whole shock row). `size_suffix` picks the variant
/// ([`shock_size_suffix`]'s output). TRUE-COLOR art — no palette.
pub fn extract_shock_lightning(
    arc_bytes: &[u8],
    size_suffix: char,
) -> Result<RgbaImage, StripError> {
    let entries = arc::parse(arc_bytes).ok_or(StripError::Arc)?;
    let wanted = format!("_{size_suffix}.dds");
    let entry = entries
        .iter()
        .find(|entry| entry.path.ends_with(&wanted))
        .ok_or(StripError::Arc)?;
    let dds = arc::extract(arc_bytes, entry).ok_or(StripError::Arc)?;
    let grid = decode_dds(&dds, SHOCK_FX_W, SHOCK_FX_H)?;
    Ok(crop_cell(&grid, 0, 0, SHOCK_STRIKE_W, CELL))
}

/// Shock-effect grid texture width (2 columns of 384-px strikes).
pub const SHOCK_FX_W: u32 = 768;
/// Shock-effect grid texture height (4 rows of 96-px strikes).
pub const SHOCK_FX_H: u32 = 384;
/// One strike frame's width — exactly four 96-px panels.
pub const SHOCK_STRIKE_W: u32 = 384;

/// DDS header field offsets (DDS_HEADER after the 4-byte magic).
const DDS_MAGIC: u32 = 0x2053_4444; // "DDS "
const DDS_HEADER_SIZE: u32 = 124;
const DDS_DATA_OFFSET: usize = 128;
const DDPF_ALPHAPIXELS: u32 = 0x1;
const DDPF_FOURCC: u32 = 0x4;
const DDPF_RGB: u32 = 0x40;

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

/// Validate + decode an uncompressed A8R8G8B8 DDS payload of the exact
/// expected dimensions. Anything else is rejected with one static error
/// (research §3: the stock sheets/effects are exactly this shape; a
/// mismatch means the format drifted and the strip must fail open, not
/// guess).
fn decode_dds(dds: &[u8], want_w: u32, want_h: u32) -> Result<RgbaImage, StripError> {
    if dds.len() < DDS_DATA_OFFSET {
        return Err(StripError::Dds("dds shorter than header"));
    }
    if read_u32(dds, 0) != Some(DDS_MAGIC) {
        return Err(StripError::Dds("bad dds magic"));
    }
    if read_u32(dds, 4) != Some(DDS_HEADER_SIZE) {
        return Err(StripError::Dds("bad dds header size"));
    }
    let height = read_u32(dds, 12).ok_or(StripError::Dds("truncated header"))?;
    let width = read_u32(dds, 16).ok_or(StripError::Dds("truncated header"))?;
    if (width, height) != (want_w, want_h) {
        return Err(StripError::Dds("unexpected sheet dimensions"));
    }
    let pf_flags = read_u32(dds, 80).ok_or(StripError::Dds("truncated header"))?;
    if pf_flags & DDPF_FOURCC != 0 || pf_flags & DDPF_RGB == 0 || pf_flags & DDPF_ALPHAPIXELS == 0 {
        return Err(StripError::Dds("not uncompressed rgba"));
    }
    if read_u32(dds, 88) != Some(32) {
        return Err(StripError::Dds("not 32bpp"));
    }
    let masks = (
        read_u32(dds, 92),
        read_u32(dds, 96),
        read_u32(dds, 100),
        read_u32(dds, 104),
    );
    if masks
        != (
            Some(0x00ff_0000),
            Some(0x0000_ff00),
            Some(0x0000_00ff),
            Some(0xff00_0000),
        )
    {
        return Err(StripError::Dds("unexpected channel masks"));
    }

    let payload = &dds[DDS_DATA_OFFSET..];
    let expected = (want_w * want_h * 4) as usize;
    if payload.len() < expected {
        return Err(StripError::Dds("truncated pixel payload"));
    }

    let mut img = RgbaImage::new(want_w, want_h);
    for (i, px) in img.pixels_mut().enumerate() {
        let o = i * 4;
        // A8R8G8B8 little-endian memory order is B, G, R, A.
        *px = image::Rgba([payload[o + 2], payload[o + 1], payload[o], payload[o + 3]]);
    }
    Ok(img)
}

// ── §2 Layout math ───────────────────────────────────────────────────

/// Strip geometry + the content-time → pixel mapping. The strip's axis
/// is the CONTENT domain (raw ms, 0..chart_end) — rate-independent, so
/// seeks/loops/scrobbles are pure cursor moves. Forward scroll runs
/// top-to-bottom (start at the top); [`with_reverse`](Self::with_reverse)
/// flips the axis to match a reverse-scroll lane (start at the bottom).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StripLayout {
    /// Note columns (4 = singles, 8 = doubles).
    pub columns: u32,
    /// Per-column width in px == the square glyph edge.
    pub column_px: u32,
    /// Strip height in px (the full 0..chart_end span).
    pub height_px: u32,
    /// Chart end in raw ms (> 0 — enforced by [`StripLayout::new`]).
    pub chart_end_ms: i32,
    /// Reverse scroll: the timeline runs bottom-to-top.
    pub reverse: bool,
}

impl StripLayout {
    /// `None` on a degenerate chart end (≤ 0) or any zero geometry
    /// param — the caller falls open to "no strip" per design §6.
    #[must_use]
    pub fn new(columns: u32, column_px: u32, height_px: u32, chart_end_ms: i32) -> Option<Self> {
        if chart_end_ms <= 0 || columns == 0 || column_px == 0 || height_px == 0 {
            return None;
        }
        Some(Self {
            columns,
            column_px,
            height_px,
            chart_end_ms,
            reverse: false,
        })
    }

    /// Flip the axis for a reverse-scroll lane (start at the bottom,
    /// matching the arrows' travel direction).
    #[must_use]
    pub fn with_reverse(mut self, reverse: bool) -> Self {
        self.reverse = reverse;
        self
    }

    /// Total strip width in px.
    #[must_use]
    pub fn width_px(&self) -> u32 {
        self.columns * self.column_px
    }

    /// Content ms → strip y (rounded half-up, clamped to
    /// `[0, height_px]`). Linear over `0..chart_end`; forward = start at
    /// the top, reverse = start at the bottom.
    #[must_use]
    pub fn y_for_ms(&self, ms: i32) -> i32 {
        let ms = i64::from(ms.clamp(0, self.chart_end_ms));
        let height = i64::from(self.height_px);
        let end = i64::from(self.chart_end_ms);
        let y = ((ms * height + end / 2) / end) as i32;
        if self.reverse {
            self.height_px as i32 - y
        } else {
            y
        }
    }

    /// Content ms → fraction of the strip axis (clamped 0..=1, same
    /// direction as [`y_for_ms`](Self::y_for_ms)) — the cursor / A-B
    /// marker math task-03 reuses.
    #[must_use]
    pub fn fraction_for_ms(&self, ms: i32) -> f32 {
        let ms = ms.clamp(0, self.chart_end_ms);
        let fraction = ms as f32 / self.chart_end_ms as f32;
        if self.reverse {
            1.0 - fraction
        } else {
            fraction
        }
    }

    /// Panel index → the column's left x. Panels fold modulo the column
    /// count (P2-side solo flags at 4..7 land on a 4-column strip;
    /// doubles' 8 columns map identically).
    #[must_use]
    pub fn column_x(&self, panel: usize) -> u32 {
        (panel as u32 % self.columns) * self.column_px
    }
}

/// Content ms → `m:ss` (floored seconds, negatives clamp to "0:00").
#[must_use]
pub fn format_mss(ms: i32) -> String {
    let total_s = ms.max(0) / 1_000;
    format!("{}:{:02}", total_s / 60, total_s % 60)
}

/// The active-section veil span (task-03; re-demo amendment 2026-08-15:
/// the veil ALWAYS shows the active region — markers absent means the
/// whole song is active, so the whole strip shades). Returns the
/// ordered, chart-clamped `[a or 0, b or chart_end]` in raw ms;
/// `None` only for a degenerate chart (`chart_end_ms <= 0`).
#[must_use]
pub fn section_veil(a_ms: Option<i32>, b_ms: Option<i32>, chart_end_ms: i32) -> Option<(i32, i32)> {
    if chart_end_ms <= 0 {
        return None;
    }
    let start = a_ms.unwrap_or(0).clamp(0, chart_end_ms);
    let end = b_ms.unwrap_or(chart_end_ms).clamp(0, chart_end_ms);
    Some((start.min(end), start.max(end)))
}

// ── §3 Rasterizer ────────────────────────────────────────────────────

/// Palette rows × columns, RGBA — the shape of the game's composed
/// 256×32 palette texture (research §4). Task-02 fills the rows it
/// needs by calling the game's own generators; unfilled rows stay
/// whatever the caller left there (the pure layer just indexes).
pub type StripPalette = [[[u8; 4]; 256]; 32];

/// Number of palette rows ([`StripPalette`]'s first dimension).
pub const PALETTE_ROWS: usize = 32;

/// One chart note as the strip consumes it — mirrors
/// `song_reset::seek::NoteView`'s judged fields (kind +0x00, raw ms
/// +0x08, per-panel flags +0x1C, per-panel freeze lengths +0x3C) plus
/// the two palette rows task-02's live selector walk injects. No engine
/// types so the harness compiles this standalone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StripNote {
    /// Note-kind discriminator (0 = arrow, 2 = freeze tail, 20 = mine;
    /// everything else renders nothing).
    pub kind: i8,
    /// Raw music-count ms — the strip's axis position.
    pub raw_time: i32,
    /// Per-panel participation flags (nonzero = panel set).
    pub panel_flags: [i32; 8],
    /// Per-panel freeze lengths (> 0 = freeze participant; the SPAN
    /// comes from the paired kind-2 tail, not from these units).
    pub durations: [i32; 8],
    /// Palette row for tap art (the game's row selector's output for
    /// this note's beat). Shock/mine art ignores it — those cells are
    /// TRUE-COLOR (the game's shock pass binds the default shader, not
    /// the palette-indexed arrow shader).
    pub tap_row: u8,
    /// Palette row for freeze head/body/cap art.
    pub freeze_row: u8,
}

impl StripNote {
    /// The engine's shock shape: all four panels of either side
    /// participating with flag value 1 (`NoteView::shock_shaped`).
    #[must_use]
    fn shock_shaped(&self) -> bool {
        self.panel_flags[..4].iter().all(|&flag| flag == 1)
            || self.panel_flags[4..].iter().all(|&flag| flag == 1)
    }

    /// Panel-participation mask — the freeze head↔tail pairing key
    /// (`NoteView::participation_mask`'s rule: nonzero-ness per panel).
    #[must_use]
    fn participation_mask(&self) -> u8 {
        self.panel_flags
            .iter()
            .enumerate()
            .fold(
                0u8,
                |mask, (panel, &flag)| {
                    if flag != 0 {
                        mask | (1 << panel)
                    } else {
                        mask
                    }
                },
            )
    }

    /// Whether any panel participates in a freeze.
    #[must_use]
    fn freeze_participant(&self) -> bool {
        self.durations.iter().any(|&duration| duration > 0)
    }
}

/// Everything the rasterizer stamps besides the sheet/palette: the note
/// vector, the measure guidelines, the optional lightning frame, and the
/// canvas fill.
pub struct StripScene<'a> {
    /// Decoded chart notes (any order — the rasterizer stamps in
    /// ascending time).
    pub notes: &'a [StripNote],
    /// Raw-ms positions of the measure guidelines — one line per
    /// measure/bar, drawn UNDER the notes (matching the in-game
    /// guideline layer). The tick→raw-ms conversion is the caller's
    /// (task-02 enumerates 4096-tick measures through the shipped
    /// `seek::raw_for_display` interpolation — this layer never
    /// duplicates the chart's time mapping).
    pub guideline_ms: &'a [i32],
    /// Guideline color (straight RGBA; alpha blends over the
    /// background).
    pub guideline_rgba: [u8; 4],
    /// One 384×96 contiguous lightning STRIKE (frame 0 of the stock
    /// shock effect — [`extract_shock_lightning`]) composited
    /// additively across a shocked side's four columns as ONE stamp:
    /// in-game the strike runs horizontally across all four arrows as
    /// a single strip, not per-panel copies. `None` ⇒ silver-only
    /// shock rows.
    pub shock_lightning: Option<&'a RgbaImage>,
    /// One 96×96 lightning frame composited ADDITIVELY over every MINE
    /// glyph (the mine mod's per-arrow shock skinning — its texture is
    /// the strike chopped into per-panel sections; the strip freezes
    /// frame 0 via [`lightning_frame0`]). `None` ⇒ silver-only mines.
    pub mine_lightning: Option<&'a RgbaImage>,
    /// Canvas fill (straight RGBA — transparent black for an overlay
    /// strip).
    pub background: [u8; 4],
}

/// Render the strip: measure guidelines under every renderable note,
/// notes stamped as noteskin glyphs at their content-time positions,
/// later notes on top. The sheet must be the [`extract_sheet`] shape;
/// palette rows are indexed per note.
#[must_use]
pub fn render_strip(
    layout: &StripLayout,
    scene: &StripScene<'_>,
    sheet: &RgbaImage,
    palette: &StripPalette,
) -> RgbaImage {
    let notes = scene.notes;
    let mut strip = RgbaImage::from_pixel(
        layout.width_px(),
        layout.height_px,
        image::Rgba(scene.background),
    );
    let glyph = layout.column_px;

    // Measure guidelines first — the notes stamp over them, like the
    // game's own layer order (guidelines render under the arrows).
    for &line_ms in scene.guideline_ms {
        let y = layout.y_for_ms(line_ms).min(layout.height_px as i32 - 1);
        for x in 0..layout.width_px() {
            let px = strip.get_pixel_mut(x, y as u32);
            px.0 = blend_px(px.0, scene.guideline_rgba);
        }
    }

    // Pair freeze heads to their kind-2 tails (rebuild_expectations'
    // rule; shared with the bar rasterizer).
    let freeze_end_ms = pair_freeze_tails(notes);

    // Stamp in ascending time (stable — equal times keep input order),
    // so later notes land on top of earlier ones.
    let mut order: Vec<usize> = (0..notes.len()).collect();
    order.sort_by_key(|&index| notes[index].raw_time);

    // The lightning layers, downscaled once. Mines get a per-panel
    // glyph-sized frame; shocks get the contiguous 4-panel strike
    // scaled to four columns wide.
    let mine_lightning_glyph = scene
        .mine_lightning
        .map(|frame| downscale_box(frame, glyph, glyph));
    let shock_strike = scene
        .shock_lightning
        .map(|strike| downscale_box(strike, glyph * 4, glyph));

    for index in order {
        let note = &notes[index];
        let y = layout.y_for_ms(note.raw_time);
        match note.kind {
            20 => {
                // Mine: the shipped mine mod's convention — the shock
                // noteskin applied per arrow (its textures are the
                // shock art chopped into per-panel sections). Same
                // true-color path as shocks, at the mine's own panels,
                // with the per-panel lightning frame over each glyph.
                for panel in 0..8 {
                    if note.panel_flags[panel] != 0 {
                        let cell = shock_glyph(sheet, panel, glyph);
                        let x = layout.column_x(panel);
                        stamp_centered(&mut strip, &cell, x, y);
                        if let Some(frame) = &mine_lightning_glyph {
                            additive_stamp(
                                &mut strip,
                                frame,
                                x as i32,
                                y - frame.height() as i32 / 2,
                            );
                        }
                    }
                }
            }
            0 if note.shock_shaped() => {
                // Shock: the full-width row — one quad per column, like
                // the game's shock pass. That pass binds the DEFAULT
                // shader (research §2.2), NOT the palette-indexed arrow
                // shader: the shock cells are TRUE-COLOR art (the
                // silver arrows with the baked glow), so they bypass
                // the palette entirely. The lightning strike then runs
                // across the shocked side's four columns as ONE
                // contiguous stamp — in-game it is a single horizontal
                // strip, not per-panel copies.
                let shocked_left = note.panel_flags[..4].iter().all(|&flag| flag == 1);
                let side_base = if shocked_left { 0usize } else { 4 };
                for offset in 0..4usize {
                    let panel = side_base + offset;
                    let cell = shock_glyph(sheet, panel, glyph);
                    stamp_centered(&mut strip, &cell, layout.column_x(panel), y);
                }
                if let Some(strike) = &shock_strike {
                    additive_stamp(
                        &mut strip,
                        strike,
                        layout.column_x(side_base) as i32,
                        y - strike.height() as i32 / 2,
                    );
                }
            }
            0 => {
                for panel in 0..8 {
                    if note.panel_flags[panel] == 0 {
                        continue;
                    }
                    let x = layout.column_x(panel);
                    if note.durations[panel] > 0 {
                        // Freeze panel: body bar (under) + cap + head.
                        if let Some(end_ms) = freeze_end_ms[index] {
                            let y_end = layout.y_for_ms(end_ms);
                            stamp_freeze_body(
                                &mut strip,
                                sheet,
                                palette,
                                note.freeze_row,
                                panel,
                                x,
                                y,
                                y_end,
                                glyph,
                            );
                            let cap = glyph_from_cell(
                                sheet,
                                (panel as u32 % 4) * CELL,
                                CELL,
                                note.freeze_row,
                                palette,
                                glyph,
                            );
                            stamp_centered(&mut strip, &cap, x, y_end);
                        }
                        let head = rotate_for_panel(
                            &resolve_cell(sheet, CELL, 0, CELL, CELL, note.freeze_row, palette),
                            panel,
                        );
                        stamp_centered(&mut strip, &downscale_box(&head, glyph, glyph), x, y);
                    } else {
                        let tap = rotate_for_panel(
                            &resolve_cell(sheet, 0, 0, CELL, CELL, note.tap_row, palette),
                            panel,
                        );
                        stamp_centered(&mut strip, &downscale_box(&tap, glyph, glyph), x, y);
                    }
                }
            }
            _ => {} // tails render via their head; THINOUT/control skipped
        }
    }

    strip
}

/// Resolve + downscale one square cell to the glyph size (unrotated —
/// freeze-cap art).
fn glyph_from_cell(
    sheet: &RgbaImage,
    x0: u32,
    y0: u32,
    row: u8,
    palette: &StripPalette,
    glyph: u32,
) -> RgbaImage {
    downscale_box(
        &resolve_cell(sheet, x0, y0, CELL, CELL, row, palette),
        glyph,
        glyph,
    )
}

/// The shock/mine glyph for a panel: TRUE-COLOR (the game's shock pass
/// binds the default shader — research §2.2 — so the shock cells carry
/// final silver-plus-glow colors, not palette indices). The sheet bakes
/// TWO direction variants (x=192 = left art, x=288 = down art); the
/// vertical panels use the down art, the horizontal ones the left art,
/// each 180°-flipped for its opposite direction.
fn shock_glyph(sheet: &RgbaImage, panel: usize, glyph: u32) -> RgbaImage {
    let (variant_x, flip) = match panel % 4 {
        1 => (3 * CELL, false), // down: down art as baked
        2 => (3 * CELL, true),  // up: down art flipped
        3 => (2 * CELL, true),  // right: left art flipped
        _ => (2 * CELL, false), // left: left art as baked
    };
    let cell = crop_cell(sheet, variant_x, 0, CELL, CELL);
    let cell = if flip {
        image::imageops::rotate180(&cell)
    } else {
        cell
    };
    downscale_box(&cell, glyph, glyph)
}

/// Extract frame 0 (top-left 96×96) from a mine-mod lightning texture —
/// a 192×384 PNG laid out as a 2×4 grid of 96×96 animation frames
/// (`note_types_expansion/mine_render.rs`'s shock-cadence grid). `None`
/// on a decode failure or any other shape (fail-open to silver-only
/// shock/mine glyphs).
#[must_use]
pub fn lightning_frame0(png_bytes: &[u8]) -> Option<RgbaImage> {
    let img = image::load_from_memory(png_bytes).ok()?.into_rgba8();
    if img.dimensions() != (2 * CELL, 4 * CELL) {
        return None;
    }
    Some(crop_cell(&img, 0, 0, CELL, CELL))
}

/// Arrow-shape index (0–7) → the lightning texture's size-variant
/// suffix (`note_types_mine00_<suffix>.png`). MIRRORS
/// `note_types_expansion::texture_loader::SHOCK_SIZE_TABLE` (the
/// game-observed shape→shock-effect-size mapping) — that module is
/// engine-coupled and cannot be imported from this pure layer; keep the
/// two tables in sync if the game's mapping ever drifts.
#[must_use]
pub fn shock_size_suffix(arrow_shape: u8) -> char {
    const SHOCK_SIZE_TABLE: [u8; 8] = [2, 2, 2, 2, 1, 0, 0, 2]; // 0=SMALL, 1=MEDIUM, 2=LARGE
    match SHOCK_SIZE_TABLE[(arrow_shape as usize) % 8] {
        0 => 's',
        1 => 'm',
        _ => 'l',
    }
}

/// Crop a sheet rect verbatim — the true-color path (shock/mine art
/// drawn by the default shader; no palette involvement).
fn crop_cell(sheet: &RgbaImage, x0: u32, y0: u32, w: u32, h: u32) -> RgbaImage {
    image::imageops::crop_imm(sheet, x0, y0, w, h).to_image()
}

/// Additive stamp (the game's BLEND_SRC_ONE overlay pass): channel sums
/// in premultiplied space, clamped; coverage accumulates. Used only for
/// the lightning layer over shock/mine glyphs.
fn additive_stamp(dst: &mut RgbaImage, src: &RgbaImage, x: i32, y: i32) {
    let (dw, dh) = dst.dimensions();
    for (sx, sy, src_px) in src.enumerate_pixels() {
        let tx = x + sx as i32;
        let ty = y + sy as i32;
        if tx < 0 || ty < 0 || tx >= dw as i32 || ty >= dh as i32 {
            continue;
        }
        let [sr, sg, sb, sa] = src_px.0;
        if sa == 0 {
            continue;
        }
        let dst_px = dst.get_pixel_mut(tx as u32, ty as u32);
        let [dr, dg, db, da] = dst_px.0;
        let out_a = (u32::from(sa) + u32::from(da)).min(255);
        let channel = |s: u8, d: u8| -> u8 {
            let premul_sum = div255_round(u32::from(s) * u32::from(sa))
                + div255_round(u32::from(d) * u32::from(da));
            let premul_sum = premul_sum.min(out_a);
            if out_a == 0 {
                0
            } else {
                ((premul_sum * 255 + out_a / 2) / out_a).min(255) as u8
            }
        };
        dst_px.0 = [
            channel(sr, dr),
            channel(sg, dg),
            channel(sb, db),
            out_a as u8,
        ];
    }
}

/// Stamp the freeze body bar for one panel between the two mapped span
/// endpoints (either order — reverse layouts map the span upside down):
/// the direction-baked body column (96×192, tiles vertically) resolved,
/// downscaled to glyph width, and tiled over the span.
#[allow(clippy::too_many_arguments)]
fn stamp_freeze_body(
    strip: &mut RgbaImage,
    sheet: &RgbaImage,
    palette: &StripPalette,
    row: u8,
    panel: usize,
    x: u32,
    y_start: i32,
    y_end: i32,
    glyph: u32,
) {
    let (y_start, y_end) = (y_start.min(y_end), y_start.max(y_end));
    if y_end <= y_start {
        return;
    }
    let column = panel as u32 % 4;
    let tile = downscale_box(
        &resolve_cell(
            sheet,
            4 * CELL + column * CELL,
            0,
            CELL,
            SHEET_H,
            row,
            palette,
        ),
        glyph,
        glyph * 2, // 96×192 keeps its 1:2 aspect
    );
    let span = (y_end - y_start) as u32;
    let mut bar = RgbaImage::new(glyph, span);
    for y in 0..span {
        let src_y = y % tile.height();
        for x_px in 0..glyph {
            bar.put_pixel(x_px, y, *tile.get_pixel(x_px, src_y));
        }
    }
    blend_stamp(strip, &bar, x as i32, y_start);
}

/// Resolve a sheet rect's palette indices to straight RGBA: the atlas
/// RED channel is the palette U index, the row is the palette V, and
/// coverage composes `atlas.a · palette.a` (research §4 — the arrow PS
/// in one CPU loop). MUST run before any downscale: palette indices are
/// not interpolable.
fn resolve_cell(
    sheet: &RgbaImage,
    x0: u32,
    y0: u32,
    w: u32,
    h: u32,
    row: u8,
    palette: &StripPalette,
) -> RgbaImage {
    let row = &palette[(row as usize).min(PALETTE_ROWS - 1)];
    let mut out = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let texel = sheet.get_pixel(x0 + x, y0 + y).0;
            let [r, g, b, a] = row[texel[0] as usize];
            let coverage = div255_round(u32::from(texel[3]) * u32::from(a)) as u8;
            out.put_pixel(x, y, image::Rgba([r, g, b, coverage]));
        }
    }
    out
}

/// Quarter-turn a resolved cell for its panel direction. The sheets
/// bake LEFT (verified visually against the real art, 2026-08-14);
/// panel order is Left, Down, Up, Right (the button-bit convention), so
/// down = 90° CCW, up = 90° CW, right = 180°.
fn rotate_for_panel(cell: &RgbaImage, panel: usize) -> RgbaImage {
    match panel % 4 {
        1 => image::imageops::rotate270(cell),
        2 => image::imageops::rotate90(cell),
        3 => image::imageops::rotate180(cell),
        _ => cell.clone(),
    }
}

/// Box-filter downscale over premultiplied RGBA (straight averaging
/// bleeds RGB out of transparent texels). Boxes are the integer spans
/// `[x·sw/dw, (x+1)·sw/dw)` — the exact box average whenever the ratio
/// is integral (the strip's glyph sizes divide the 96-px cells).
fn downscale_box(src: &RgbaImage, dst_w: u32, dst_h: u32) -> RgbaImage {
    let (sw, sh) = src.dimensions();
    let mut out = RgbaImage::new(dst_w, dst_h);
    for dy in 0..dst_h {
        let y0 = (dy * sh) / dst_h;
        let y1 = (((dy + 1) * sh) / dst_h).max(y0 + 1);
        for dx in 0..dst_w {
            let x0 = (dx * sw) / dst_w;
            let x1 = (((dx + 1) * sw) / dst_w).max(x0 + 1);
            let mut sum_premul = [0u64; 3];
            let mut sum_a = 0u64;
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let [r, g, b, a] = src.get_pixel(sx, sy).0;
                    let a = u64::from(a);
                    sum_premul[0] += u64::from(r) * a;
                    sum_premul[1] += u64::from(g) * a;
                    sum_premul[2] += u64::from(b) * a;
                    sum_a += a;
                }
            }
            let area = u64::from((x1 - x0) * (y1 - y0));
            let a = ((sum_a + area / 2) / area) as u8;
            let px = if sum_a == 0 {
                [0, 0, 0, 0]
            } else {
                [
                    ((sum_premul[0] + sum_a / 2) / sum_a) as u8,
                    ((sum_premul[1] + sum_a / 2) / sum_a) as u8,
                    ((sum_premul[2] + sum_a / 2) / sum_a) as u8,
                    a,
                ]
            };
            out.put_pixel(dx, dy, image::Rgba(px));
        }
    }
    out
}

/// Stamp a glyph with its vertical CENTER on `y` (glyph boxes straddle
/// their time position; the head/cap of a freeze straddle the span
/// boundaries symmetrically).
fn stamp_centered(strip: &mut RgbaImage, glyph: &RgbaImage, x: u32, y: i32) {
    blend_stamp(strip, glyph, x as i32, y - glyph.height() as i32 / 2);
}

/// Src-over alpha blend of `src` onto `dst` at (x, y), clipping at the
/// dst bounds.
fn blend_stamp(dst: &mut RgbaImage, src: &RgbaImage, x: i32, y: i32) {
    let (dw, dh) = dst.dimensions();
    for (sx, sy, src_px) in src.enumerate_pixels() {
        let tx = x + sx as i32;
        let ty = y + sy as i32;
        if tx < 0 || ty < 0 || tx >= dw as i32 || ty >= dh as i32 {
            continue;
        }
        if src_px.0[3] == 0 {
            continue;
        }
        let dst_px = dst.get_pixel_mut(tx as u32, ty as u32);
        dst_px.0 = blend_px(dst_px.0, src_px.0);
    }
}

/// One src-over composite (straight RGBA in and out; integer
/// premultiplied-form arithmetic, rounded).
fn blend_px(dst: [u8; 4], src: [u8; 4]) -> [u8; 4] {
    let [sr, sg, sb, sa] = src;
    if sa == 0 {
        return dst;
    }
    let [dr, dg, db, da] = dst;
    let sa32 = u32::from(sa);
    let inv = 255 - sa32;
    // Premultiplied: P_out = P_src + P_dst·(1−a_s); back to straight.
    let out_a = sa32 + div255_round(u32::from(da) * inv);
    let channel = |s: u8, d: u8| -> u8 {
        let premul_src = u32::from(s) * sa32;
        let premul_dst = div255_round(u32::from(d) * u32::from(da) * inv);
        let premul_out = premul_src + premul_dst;
        if out_a == 0 {
            0
        } else {
            ((premul_out + out_a / 2) / out_a) as u8
        }
    };
    [
        channel(sr, dr),
        channel(sg, dg),
        channel(sb, db),
        out_a.min(255) as u8,
    ]
}

/// Rounded `x / 255`.
fn div255_round(x: u32) -> u32 {
    (x + 127) / 255
}

// ── §3b Bar-mode rasterizer (the shipped HUD style) ──────────────────
//
// The 2026-08-14 density finding (cabinet probe on real expert charts):
// noteskin glyphs overlap into an unreadable wall at expert density. The
// maintainer-approved replacement renders the note layer as flat rects —
// taps and freeze heads as 1-px full-column-width bars in their
// quantization colors, freeze bodies as solid rectangles spanning the
// hold, shocks as full-width and mines as per-panel bars in a fixed
// bright blue-white. Colors still come from the injected palette rows
// (the live path resolves them from the game's own generators, so a
// future quantization hack propagates); no sheet is needed at all —
// which also removes the sheet-extraction failure mode from the default
// path. The noteskin rasterizer above remains available (and tested)
// for future zoomed/alternate views.

/// Bar height for taps/heads/shocks/mines (px).
pub const BAR_H: u32 = 1;
/// Shock/mine bar color (maintainer-specified: bright blue, almost white).
pub const SHOCK_MINE_RGBA: [u8; 4] = [190, 230, 255, 255];
/// Freeze BODY intensity relative to the freeze head color (the approved
/// experiment's body/head ratio — reads as one shape).
const FREEZE_BODY_SCALE: u32 = 179; // ≈ 0.7 × 256

/// One palette row's representative bar color: the row's
/// maximum-luminance entry (the ramp's bright end — the note art's color
/// identity; exact for the flat-ramp fallback, and robust to either ramp
/// direction on the live generators).
#[must_use]
pub fn row_bar_color(palette: &StripPalette, row: u8) -> [u8; 4] {
    let row = &palette[(row as usize).min(PALETTE_ROWS - 1)];
    let mut best = [0u8, 0, 0, 0];
    let mut best_luma = 0u32;
    for &[r, g, b, a] in row.iter() {
        if a == 0 {
            continue;
        }
        // Integer Rec.601-ish luma, alpha-weighted so faint entries lose.
        let luma = (2 * u32::from(r) + 5 * u32::from(g) + u32::from(b)) * u32::from(a);
        if luma > best_luma {
            best_luma = luma;
            best = [r, g, b, a];
        }
    }
    best
}

/// Render the strip in bar mode: background + guidelines (shared with
/// the noteskin path), then freeze bodies as solid rects (under), then
/// taps/heads/shocks/mines as [`BAR_H`]-px bars in time order. The
/// scene's lightning inputs are ignored (bars carry the fixed
/// [`SHOCK_MINE_RGBA`] treatment).
#[must_use]
pub fn render_strip_bars(
    layout: &StripLayout,
    scene: &StripScene<'_>,
    palette: &StripPalette,
) -> RgbaImage {
    let notes = scene.notes;
    let mut strip = RgbaImage::from_pixel(
        layout.width_px(),
        layout.height_px,
        image::Rgba(scene.background),
    );

    // Guidelines first — the note layer stamps over them.
    for &line_ms in scene.guideline_ms {
        let y = layout.y_for_ms(line_ms).min(layout.height_px as i32 - 1);
        for x in 0..layout.width_px() {
            let px = strip.get_pixel_mut(x, y as u32);
            px.0 = blend_px(px.0, scene.guideline_rgba);
        }
    }

    // Freeze spans (kind-2 tail pairing — the noteskin path's rule).
    let freeze_end_ms = pair_freeze_tails(notes);

    let mut order: Vec<usize> = (0..notes.len()).collect();
    order.sort_by_key(|&index| notes[index].raw_time);

    // Pass 1: freeze bodies (solid rects, under every bar).
    for &index in &order {
        let note = &notes[index];
        if note.kind != 0 {
            continue;
        }
        let Some(end_ms) = freeze_end_ms[index] else {
            continue;
        };
        let head_color = row_bar_color(palette, note.freeze_row);
        let body_color = [
            ((u32::from(head_color[0]) * FREEZE_BODY_SCALE) >> 8) as u8,
            ((u32::from(head_color[1]) * FREEZE_BODY_SCALE) >> 8) as u8,
            ((u32::from(head_color[2]) * FREEZE_BODY_SCALE) >> 8) as u8,
            head_color[3],
        ];
        let y0 = layout.y_for_ms(note.raw_time);
        let y1 = layout.y_for_ms(end_ms);
        // Reverse layouts map the span upside down — normalize.
        let (top, span) = (y0.min(y1), (y1 - y0).abs().max(1));
        for panel in 0..8 {
            if note.durations[panel] > 0 {
                fill_rect(
                    &mut strip,
                    layout.column_x(panel),
                    layout.column_px,
                    top,
                    span,
                    body_color,
                );
            }
        }
    }

    // Pass 2: bars in time order (later on top).
    for &index in &order {
        let note = &notes[index];
        let y = layout.y_for_ms(note.raw_time);
        match note.kind {
            20 => {
                for panel in 0..8 {
                    if note.panel_flags[panel] != 0 {
                        fill_bar(
                            &mut strip,
                            layout.column_x(panel),
                            layout.column_px,
                            y,
                            SHOCK_MINE_RGBA,
                        );
                    }
                }
            }
            0 if note.shock_shaped() => {
                fill_bar(&mut strip, 0, layout.width_px(), y, SHOCK_MINE_RGBA);
            }
            0 => {
                for panel in 0..8 {
                    if note.panel_flags[panel] == 0 {
                        continue;
                    }
                    let row = if note.durations[panel] > 0 {
                        note.freeze_row
                    } else {
                        note.tap_row
                    };
                    let color = row_bar_color(palette, row);
                    if color[3] == 0 {
                        continue; // empty palette row — nothing sane to draw
                    }
                    fill_bar(
                        &mut strip,
                        layout.column_x(panel),
                        layout.column_px,
                        y,
                        color,
                    );
                }
            }
            _ => {}
        }
    }

    strip
}

/// The kind-2 tail pairing (rebuild_expectations' mask rule), shared by
/// both rasterizers.
fn pair_freeze_tails(notes: &[StripNote]) -> Vec<Option<i32>> {
    let mut freeze_end_ms: Vec<Option<i32>> = vec![None; notes.len()];
    for (tail_index, tail) in notes.iter().enumerate() {
        if tail.kind != 2 {
            continue;
        }
        let mask = tail.participation_mask();
        let head_index = notes[..tail_index]
            .iter()
            .enumerate()
            .rev()
            .find(|(head_index, head)| {
                head.kind == 0
                    && head.freeze_participant()
                    && head.participation_mask() == mask
                    && freeze_end_ms[*head_index].is_none()
            })
            .map(|(head_index, _)| head_index);
        if let Some(head_index) = head_index {
            freeze_end_ms[head_index] = Some(tail.raw_time);
        }
    }
    freeze_end_ms
}

/// Opaque rect write, clipped (bars are solid — no blending needed).
fn fill_rect(strip: &mut RgbaImage, x0: u32, w: u32, y0: i32, h: i32, color: [u8; 4]) {
    let (iw, ih) = strip.dimensions();
    for dy in 0..h {
        let y = y0 + dy;
        if y < 0 || y >= ih as i32 {
            continue;
        }
        for x in x0..(x0 + w).min(iw) {
            strip.put_pixel(x, y as u32, image::Rgba(color));
        }
    }
}

/// A [`BAR_H`]-px bar centered on `y`.
fn fill_bar(strip: &mut RgbaImage, x0: u32, w: u32, y: i32, color: [u8; 4]) {
    fill_rect(strip, x0, w, y - (BAR_H as i32) / 2, BAR_H as i32, color);
}

// ── §4 PNG encode ────────────────────────────────────────────────────

/// Encode the strip as PNG bytes (the caller writes the cache file).
pub fn encode_png(img: &RgbaImage) -> Result<Vec<u8>, StripError> {
    let mut bytes = Vec::new();
    img.write_to(
        &mut std::io::Cursor::new(&mut bytes),
        image::ImageFormat::Png,
    )
    .map_err(|_| StripError::Png)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::avs_layeredfs::avslz;

    // ── Fixture helpers (synthetic only — no game pixel data) ────────

    /// Build a synthetic DDS with the stock sheet's header shape and a
    /// caller-defined pixel function returning (r, g, b, a) — written in
    /// the file's BGRA memory order.
    fn synthetic_dds(
        width: u32,
        height: u32,
        pf_flags: u32,
        bits: u32,
        masks: [u32; 4],
        pixel: impl Fn(u32, u32) -> [u8; 4],
    ) -> Vec<u8> {
        let mut d = Vec::with_capacity(DDS_DATA_OFFSET + (width * height * 4) as usize);
        d.extend_from_slice(&DDS_MAGIC.to_le_bytes());
        d.extend_from_slice(&DDS_HEADER_SIZE.to_le_bytes());
        d.extend_from_slice(&0x0000_100Fu32.to_le_bytes()); // flags (caps|h|w|pitch|pf)
        d.extend_from_slice(&height.to_le_bytes());
        d.extend_from_slice(&width.to_le_bytes());
        d.extend_from_slice(&(width * height * 4).to_le_bytes()); // pitchOrLinearSize
        d.extend_from_slice(&0u32.to_le_bytes()); // depth
        d.extend_from_slice(&0u32.to_le_bytes()); // mipmaps
        d.extend_from_slice(&[0u8; 44]); // reserved1
        d.extend_from_slice(&32u32.to_le_bytes()); // pf.size
        d.extend_from_slice(&pf_flags.to_le_bytes());
        d.extend_from_slice(&0u32.to_le_bytes()); // fourCC
        d.extend_from_slice(&bits.to_le_bytes());
        for m in masks {
            d.extend_from_slice(&m.to_le_bytes());
        }
        d.extend_from_slice(&[0u8; 16]); // caps1..4
        d.extend_from_slice(&0u32.to_le_bytes()); // reserved2
        assert_eq!(d.len(), DDS_DATA_OFFSET);
        for y in 0..height {
            for x in 0..width {
                let [r, g, b, a] = pixel(x, y);
                d.extend_from_slice(&[b, g, r, a]); // BGRA memory order
            }
        }
        d
    }

    fn stock_shaped_dds(pixel: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
        synthetic_dds(
            SHEET_W,
            SHEET_H,
            DDPF_RGB | DDPF_ALPHAPIXELS,
            32,
            [0x00ff_0000, 0x0000_ff00, 0x0000_00ff, 0xff00_0000],
            pixel,
        )
    }

    /// Hand-rolled ARC v1 with one entry, optionally AVSLZ-compressed
    /// (the real sheets are; `compressed == decompressed` size means
    /// stored raw per `arc::extract`).
    fn synthetic_arc(inner_path: &str, payload: &[u8], compress: bool) -> Vec<u8> {
        let stored: Vec<u8> = if compress {
            avslz::compress(payload)
        } else {
            payload.to_vec()
        };
        let cue_start = 16u32;
        let str_offset = cue_start + 16;
        let data_offset = (str_offset + inner_path.len() as u32 + 1 + 63) & !63;

        let mut out = Vec::new();
        out.extend_from_slice(&0x1975_1120u32.to_le_bytes()); // magic
        out.extend_from_slice(&1u32.to_le_bytes()); // version
        out.extend_from_slice(&1u32.to_le_bytes()); // file count
        out.extend_from_slice(&if compress { 2u32 } else { 0u32 }.to_le_bytes());
        out.extend_from_slice(&str_offset.to_le_bytes());
        out.extend_from_slice(&data_offset.to_le_bytes());
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes()); // unpacked
        out.extend_from_slice(&(stored.len() as u32).to_le_bytes()); // packed
        out.extend_from_slice(inner_path.as_bytes());
        out.push(0);
        out.resize(data_offset as usize, 0);
        out.extend_from_slice(&stored);
        out
    }

    /// Hand-rolled ARC v1 with multiple uncompressed entries (the
    /// shock-effect arc shape).
    fn synthetic_multi_arc(entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let count = entries.len() as u32;
        let cue_start = 16u32;
        let str_start = cue_start + count * 16;
        let mut str_offsets = Vec::new();
        let mut cursor = str_start;
        for (path, _) in entries {
            str_offsets.push(cursor);
            cursor += path.len() as u32 + 1;
        }
        let mut data_offsets = Vec::new();
        let mut data_cursor = (cursor + 63) & !63;
        for (_, payload) in entries {
            data_offsets.push(data_cursor);
            data_cursor = (data_cursor + payload.len() as u32 + 63) & !63;
        }

        let mut out = Vec::new();
        out.extend_from_slice(&0x1975_1120u32.to_le_bytes()); // magic
        out.extend_from_slice(&1u32.to_le_bytes()); // version
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes()); // uncompressed
        for (i, (_, payload)) in entries.iter().enumerate() {
            out.extend_from_slice(&str_offsets[i].to_le_bytes());
            out.extend_from_slice(&data_offsets[i].to_le_bytes());
            out.extend_from_slice(&(payload.len() as u32).to_le_bytes()); // unpacked
            out.extend_from_slice(&(payload.len() as u32).to_le_bytes()); // packed
        }
        for (path, _) in entries {
            out.extend_from_slice(path.as_bytes());
            out.push(0);
        }
        for (i, (_, payload)) in entries.iter().enumerate() {
            out.resize(data_offsets[i] as usize, 0);
            out.extend_from_slice(payload);
        }
        out
    }

    /// A test pattern with a distinct value per texel (mod 251, a prime,
    /// so BGRA↔RGBA swaps are detectable everywhere).
    fn pattern(x: u32, y: u32) -> [u8; 4] {
        let v = (x * 7 + y * 13) % 251;
        [v as u8, (v + 1) as u8, (v + 2) as u8, (v + 3) as u8]
    }

    // ── §1 extraction (AC1) ──────────────────────────────────────────

    #[test]
    fn extracts_compressed_synthetic_arc_pixel_exact() {
        let dds = stock_shaped_dds(pattern);
        let arc_bytes = synthetic_arc("data/2d/arrow00/arrow00.dds", &dds, true);
        let img = extract_sheet(&arc_bytes).expect("extraction");
        assert_eq!((img.width(), img.height()), (SHEET_W, SHEET_H));
        for y in [0u32, 1, 95, 96, 191] {
            for x in [0u32, 1, 95, 96, 383, 384, 767] {
                let [r, g, b, a] = pattern(x, y);
                assert_eq!(
                    img.get_pixel(x, y).0,
                    [r, g, b, a],
                    "pixel mismatch at {x},{y}"
                );
            }
        }
    }

    #[test]
    fn extracts_stored_uncompressed_entry() {
        let dds = stock_shaped_dds(pattern);
        let arc_bytes = synthetic_arc("data/2d/arrow00/arrow00.dds", &dds, false);
        let img = extract_sheet(&arc_bytes).expect("extraction");
        assert_eq!(img.get_pixel(5, 5).0, pattern(5, 5));
    }

    #[test]
    fn rejects_malformed_sheets_without_panic() {
        // Wrong dimensions.
        let wrong_dims = synthetic_dds(
            256,
            256,
            DDPF_RGB | DDPF_ALPHAPIXELS,
            32,
            [0x00ff_0000, 0x0000_ff00, 0x0000_00ff, 0xff00_0000],
            pattern,
        );
        assert_eq!(
            extract_sheet(&synthetic_arc("x.dds", &wrong_dims, false)),
            Err(StripError::Dds("unexpected sheet dimensions"))
        );

        // Compressed format (FOURCC flag set).
        let fourcc = synthetic_dds(
            SHEET_W,
            SHEET_H,
            DDPF_FOURCC,
            32,
            [0x00ff_0000, 0x0000_ff00, 0x0000_00ff, 0xff00_0000],
            pattern,
        );
        assert_eq!(
            extract_sheet(&synthetic_arc("x.dds", &fourcc, false)),
            Err(StripError::Dds("not uncompressed rgba"))
        );

        // Wrong channel masks (RGBA order instead of ARGB).
        let wrong_masks = synthetic_dds(
            SHEET_W,
            SHEET_H,
            DDPF_RGB | DDPF_ALPHAPIXELS,
            32,
            [0xff00_0000, 0x00ff_0000, 0x0000_ff00, 0x0000_00ff],
            pattern,
        );
        assert_eq!(
            extract_sheet(&synthetic_arc("x.dds", &wrong_masks, false)),
            Err(StripError::Dds("unexpected channel masks"))
        );

        // Truncated payload.
        let mut truncated = stock_shaped_dds(pattern);
        truncated.truncate(DDS_DATA_OFFSET + 100);
        assert_eq!(
            extract_sheet(&synthetic_arc("x.dds", &truncated, false)),
            Err(StripError::Dds("truncated pixel payload"))
        );

        // Garbage arc bytes.
        assert_eq!(extract_sheet(&[0u8; 64]), Err(StripError::Arc));
        assert_eq!(extract_sheet(&[]), Err(StripError::Arc));
    }

    // ── §2 layout (AC2) ──────────────────────────────────────────────

    #[test]
    fn layout_maps_content_ms_linearly_top_down() {
        let layout = StripLayout::new(4, 12, 600, 120_000).expect("layout");
        // AC2's exact triple.
        assert_eq!(layout.y_for_ms(0), 0);
        assert_eq!(layout.y_for_ms(60_000), 300);
        assert_eq!(layout.y_for_ms(120_000), 600);
        // Clamped outside the chart.
        assert_eq!(layout.y_for_ms(-5_000), 0);
        assert_eq!(layout.y_for_ms(500_000), 600);
        // Rounding, not truncation (1 ms of a 120 s chart on 600 px is
        // 0.005 px → 0; the midpoint rounds up).
        assert_eq!(layout.y_for_ms(100), 1); // 0.5 px rounds up
        assert_eq!(layout.y_for_ms(99), 0); // 0.495 px rounds down
    }

    #[test]
    fn layout_rejects_degenerate_params() {
        assert_eq!(StripLayout::new(4, 12, 600, 0), None);
        assert_eq!(StripLayout::new(4, 12, 600, -1), None);
        assert_eq!(StripLayout::new(0, 12, 600, 120_000), None);
        assert_eq!(StripLayout::new(4, 0, 600, 120_000), None);
        assert_eq!(StripLayout::new(4, 12, 0, 120_000), None);
    }

    #[test]
    fn layout_columns_fold_modulo_and_width_follows() {
        let single = StripLayout::new(4, 12, 600, 120_000).expect("layout");
        assert_eq!(single.width_px(), 48);
        // P1 panels 0..3 → columns 0..3.
        assert_eq!(single.column_x(0), 0);
        assert_eq!(single.column_x(3), 36);
        // P2-side solo panels 4..7 fold onto the same 4 columns.
        assert_eq!(single.column_x(4), 0);
        assert_eq!(single.column_x(7), 36);

        let doubles = StripLayout::new(8, 12, 600, 120_000).expect("layout");
        assert_eq!(doubles.width_px(), 96);
        for panel in 0..8usize {
            assert_eq!(doubles.column_x(panel), panel as u32 * 12);
        }
    }

    #[test]
    fn layout_fraction_is_clamped() {
        let layout = StripLayout::new(4, 12, 600, 120_000).expect("layout");
        assert_eq!(layout.fraction_for_ms(0), 0.0);
        assert_eq!(layout.fraction_for_ms(60_000), 0.5);
        assert_eq!(layout.fraction_for_ms(120_000), 1.0);
        assert_eq!(layout.fraction_for_ms(-1), 0.0);
        assert_eq!(layout.fraction_for_ms(240_000), 1.0);
    }

    #[test]
    fn mss_formats_floored_seconds() {
        assert_eq!(format_mss(0), "0:00");
        assert_eq!(format_mss(60_000), "1:00");
        assert_eq!(format_mss(120_000), "2:00");
        // Floor, not round (123.4 s is 2:03).
        assert_eq!(format_mss(123_400), "2:03");
        assert_eq!(format_mss(59_999), "0:59");
        // Negatives clamp.
        assert_eq!(format_mss(-1), "0:00");
        // Two-digit seconds pad; minutes don't.
        assert_eq!(format_mss(65_000), "1:05");
        assert_eq!(format_mss(600_000), "10:00");
    }

    // ── §3 rasterizer fixtures ───────────────────────────────────────

    /// Palette indices painted per cell region by [`test_sheet`].
    const IDX_TAP: u8 = 10;
    /// The tap cell's left 8-px column — the rotation marker (baked
    /// LEFT: the marker column is the arrow-tip side).
    const IDX_TAP_MARKER: u8 = 11;
    /// A second tap index resolving to the SAME color as IDX_TAP — the
    /// colors-not-indices regression pair (their index midpoint is
    /// IDX_TAP_MARKER, which resolves to a very different color).
    const IDX_TAP_TWIN: u8 = 12;
    const IDX_HEAD: u8 = 20;
    const IDX_CAP: u8 = 40;
    const IDX_BODY: u8 = 50;

    const ROW_TAP: u8 = 1;
    const ROW_FREEZE: u8 = 8;

    const RED: [u8; 4] = [255, 0, 0, 255];
    const CYAN: [u8; 4] = [0, 255, 255, 255];
    const GREEN: [u8; 4] = [0, 255, 0, 255];
    const BLUE: [u8; 4] = [0, 0, 255, 255];
    const YELLOW: [u8; 4] = [255, 255, 0, 255];
    const WHITE: [u8; 4] = [255, 255, 255, 255];
    /// TRUE colors painted into the two shock variant cells (the shock
    /// pass bypasses the palette — these come through verbatim).
    const SHOCK_LEFT_ART: [u8; 4] = [200, 200, 210, 255];
    const SHOCK_DOWN_ART: [u8; 4] = [140, 160, 255, 255];
    /// The synthetic lightning frame's uniform color.
    const LIGHTNING_PX: [u8; 4] = [100, 120, 255, 255];
    /// The synthetic strike's right-half color (contiguity probe).
    const STRIKE_RIGHT_PX: [u8; 4] = [80, 40, 0, 255];
    /// Guideline color used by [`render_with`] (opaque so expectations
    /// are exact).
    const GUIDELINE: [u8; 4] = [90, 90, 90, 255];

    /// Synthetic indexed sheet: the palette-driven cell regions filled
    /// with a known palette index at full coverage; the two shock cells
    /// filled with TRUE colors (they bypass the palette); zero elsewhere.
    fn test_sheet() -> RgbaImage {
        let mut img = RgbaImage::new(SHEET_W, SHEET_H);
        let mut paint = |x0: u32, y0: u32, w: u32, h: u32, px: [u8; 4]| {
            for y in y0..y0 + h {
                for x in x0..x0 + w {
                    img.put_pixel(x, y, image::Rgba(px));
                }
            }
        };
        let indexed = |idx: u8| [idx, 0, 0, 255];
        paint(0, 0, CELL, CELL, indexed(IDX_TAP)); // tap cell (baked LEFT)
        paint(0, 0, 8, CELL, indexed(IDX_TAP_MARKER)); // tip-side marker column
        paint(CELL, 0, CELL, CELL, indexed(IDX_HEAD)); // freeze head
        paint(2 * CELL, 0, CELL, CELL, SHOCK_LEFT_ART); // shock variant 0 (true color)
        paint(3 * CELL, 0, CELL, CELL, SHOCK_DOWN_ART); // shock variant 1 (true color)
        for col in 0..4 {
            paint(col * CELL, CELL, CELL, CELL, indexed(IDX_CAP)); // caps (dir baked)
            paint(4 * CELL + col * CELL, 0, CELL, SHEET_H, indexed(IDX_BODY)); // bodies
        }
        img
    }

    fn test_palette() -> Box<StripPalette> {
        let mut palette: Box<StripPalette> = Box::new([[[0, 0, 0, 0]; 256]; 32]);
        palette[ROW_TAP as usize][IDX_TAP as usize] = RED;
        palette[ROW_TAP as usize][IDX_TAP_MARKER as usize] = CYAN;
        palette[ROW_TAP as usize][IDX_TAP_TWIN as usize] = RED;
        palette[ROW_FREEZE as usize][IDX_HEAD as usize] = GREEN;
        palette[ROW_FREEZE as usize][IDX_BODY as usize] = BLUE;
        palette[ROW_FREEZE as usize][IDX_CAP as usize] = YELLOW;
        // A second tap row for the time-order test.
        palette[4][IDX_TAP as usize] = WHITE;
        palette[4][IDX_TAP_MARKER as usize] = WHITE;
        palette
    }

    fn test_layout() -> StripLayout {
        StripLayout::new(4, 12, 600, 120_000).expect("layout")
    }

    fn tap(raw_time: i32, panel: usize) -> StripNote {
        let mut panel_flags = [0i32; 8];
        panel_flags[panel] = 1;
        StripNote {
            kind: 0,
            raw_time,
            panel_flags,
            durations: [0; 8],
            tap_row: ROW_TAP,
            freeze_row: ROW_FREEZE,
        }
    }

    /// The transparent-background, no-guideline render most tests use.
    fn render(notes: &[StripNote]) -> RgbaImage {
        render_with(notes, &[], [0, 0, 0, 0], &test_sheet())
    }

    fn render_with(
        notes: &[StripNote],
        guideline_ms: &[i32],
        background: [u8; 4],
        sheet: &RgbaImage,
    ) -> RgbaImage {
        render_scene(notes, guideline_ms, background, sheet, None, None)
    }

    fn render_scene(
        notes: &[StripNote],
        guideline_ms: &[i32],
        background: [u8; 4],
        sheet: &RgbaImage,
        shock_lightning: Option<&RgbaImage>,
        mine_lightning: Option<&RgbaImage>,
    ) -> RgbaImage {
        render_strip(
            &test_layout(),
            &StripScene {
                notes,
                guideline_ms,
                guideline_rgba: GUIDELINE,
                shock_lightning,
                mine_lightning,
                background,
            },
            sheet,
            &test_palette(),
        )
    }

    // ── §3 rasterizer (AC3) ──────────────────────────────────────────

    #[test]
    fn tap_lands_in_its_column_at_its_time_with_palette_color() {
        // Panel 0 tap at 60 s of a 120 s chart on a 600 px strip: glyph
        // centered at y=300 (rows 294..306), column 0 (x 0..12).
        let img = render(&[tap(60_000, 0)]);
        // Glyph body is the tap color (row 1, index 10 → RED); the
        // tip-side marker column resolves CYAN and sits at x=0
        // (identity rotation for LEFT).
        assert_eq!(img.get_pixel(6, 300).0, RED, "glyph body");
        assert_eq!(img.get_pixel(0, 300).0, CYAN, "tip marker at left");
        // Outside the glyph: untouched background.
        assert_eq!(img.get_pixel(6, 280).0, [0, 0, 0, 0], "above glyph");
        assert_eq!(img.get_pixel(20, 300).0, [0, 0, 0, 0], "next column");
        assert_eq!(img.get_pixel(6, 320).0, [0, 0, 0, 0], "below glyph");
    }

    #[test]
    fn rotation_follows_the_panel_direction() {
        // The marker column is the arrow TIP side (baked LEFT). Rotation
        // per panel: 0 left = identity (marker at glyph left edge),
        // 1 down = 90° CCW (marker at bottom), 2 up = 90° CW (marker at
        // top), 3 right = 180° (marker at right edge).
        // Glyph box per panel p: x in [12p, 12p+12), y in [294, 306).
        let expectations: [(usize, (u32, u32), (u32, u32)); 4] = [
            // (panel, marker probe (x,y), body probe (x,y))
            (0, (0, 300), (6, 300)),   // left: marker col x=0
            (1, (18, 305), (18, 298)), // down: marker row y=305 (bottom)
            (2, (30, 294), (30, 301)), // up: marker row y=294 (top)
            (3, (47, 300), (42, 300)), // right: marker col x=47
        ];
        for (panel, marker_at, body_at) in expectations {
            let img = render(&[tap(60_000, panel)]);
            assert_eq!(
                img.get_pixel(marker_at.0, marker_at.1).0,
                CYAN,
                "marker for panel {panel}"
            );
            assert_eq!(
                img.get_pixel(body_at.0, body_at.1).0,
                RED,
                "body for panel {panel}"
            );
        }
    }

    #[test]
    fn freeze_spans_head_to_paired_tail_with_cap_and_head_glyphs() {
        // Head on panel 2 at 30 s (y=150), kind-2 tail at 60 s (y=300).
        let mut head = tap(30_000, 2);
        head.durations[2] = 1; // participation (span comes from the tail)
        let mut tail = tap(60_000, 2);
        tail.kind = 2;
        let img = render(&[head, tail]);

        let col_x = 2 * 12 + 6; // column 2 center
                                // Body bar between head and tail (row 225 is mid-span).
        assert_eq!(img.get_pixel(col_x, 225).0, BLUE, "body mid-span");
        // Head glyph over the span start.
        assert_eq!(img.get_pixel(col_x, 150).0, GREEN, "head at start");
        // Cap centered on the span end (over the body).
        assert_eq!(img.get_pixel(col_x, 300).0, YELLOW, "cap at end");
        // Nothing above the head or below the cap.
        assert_eq!(img.get_pixel(col_x, 130).0, [0, 0, 0, 0], "above head");
        assert_eq!(img.get_pixel(col_x, 320).0, [0, 0, 0, 0], "below cap");
        // Other columns untouched.
        assert_eq!(img.get_pixel(6, 225).0, [0, 0, 0, 0], "other column");
    }

    #[test]
    fn tailless_head_renders_head_glyph_only() {
        let mut head = tap(30_000, 2);
        head.durations[2] = 1;
        let img = render(&[head]);
        let col_x = 2 * 12 + 6;
        assert_eq!(img.get_pixel(col_x, 150).0, GREEN, "head still renders");
        assert_eq!(img.get_pixel(col_x, 225).0, [0, 0, 0, 0], "no body");
    }

    #[test]
    fn jump_renders_both_panels() {
        let mut note = tap(90_000, 0);
        note.panel_flags[3] = 1;
        let img = render(&[note]);
        // y = 450 for 90 s; columns 0 and 3 carry glyphs, 1 and 2 don't.
        assert_eq!(img.get_pixel(6, 450).0, RED, "panel 0");
        assert_eq!(img.get_pixel(42, 450).0, RED, "panel 3");
        assert_eq!(img.get_pixel(18, 450).0, [0, 0, 0, 0], "panel 1 empty");
        assert_eq!(img.get_pixel(30, 450).0, [0, 0, 0, 0], "panel 2 empty");
    }

    #[test]
    fn shock_fills_the_full_row_true_color_and_mine_stays_on_its_panel() {
        // Shock: kind 0, all four panels flag 1 → shock art in EVERY
        // column at its row (y=60 for 12 s). The art comes through
        // TRUE-COLOR (the game's shock pass binds the default shader —
        // no palette): horizontal panels show the left-variant cell,
        // vertical panels the down-variant cell.
        let mut shock = tap(12_000, 0);
        shock.panel_flags = [1, 1, 1, 1, 0, 0, 0, 0];
        // Mine: kind 20, one panel (y=120 for 24 s) — the mine mod's
        // per-arrow shock skinning, same true-color path.
        let mut mine = tap(24_000, 1);
        mine.kind = 20;
        let img = render(&[shock, mine]);
        let expected = [
            SHOCK_LEFT_ART, // panel 0 left
            SHOCK_DOWN_ART, // panel 1 down
            SHOCK_DOWN_ART, // panel 2 up (down art flipped — same fill)
            SHOCK_LEFT_ART, // panel 3 right (left art flipped)
        ];
        for (col, want) in expected.into_iter().enumerate() {
            assert_eq!(
                img.get_pixel(col as u32 * 12 + 6, 60).0,
                want,
                "shock column {col}"
            );
        }
        assert_eq!(
            img.get_pixel(18, 120).0,
            SHOCK_DOWN_ART,
            "mine on its panel"
        );
        assert_eq!(img.get_pixel(6, 120).0, [0, 0, 0, 0], "mine not full-width");
    }

    #[test]
    fn shock_size_suffix_mirrors_the_mine_loader_table() {
        // Pin the mirror of texture_loader::SHOCK_SIZE_TABLE
        // ([2,2,2,2,1,0,0,2] — 0=s, 1=m, 2=l) so drift between the two
        // tables fails a test rather than silently mismatching sizes.
        let expected = ['l', 'l', 'l', 'l', 'm', 's', 's', 'l'];
        for (shape, want) in expected.into_iter().enumerate() {
            assert_eq!(shock_size_suffix(shape as u8), want, "shape {shape}");
        }
        // Out-of-range shapes fold modulo 8 (defensive).
        assert_eq!(shock_size_suffix(8), 'l');
    }

    #[test]
    fn lightning_frame0_extracts_the_first_grid_cell() {
        // A synthetic 2×4 grid PNG (the mine texture shape): frame 0
        // (top-left 96×96) painted a distinct color, the rest another.
        let mut grid = RgbaImage::from_pixel(192, 384, image::Rgba([9, 9, 9, 255]));
        for y in 0..96 {
            for x in 0..96 {
                grid.put_pixel(x, y, image::Rgba(LIGHTNING_PX));
            }
        }
        let png = encode_png(&grid).expect("encode grid");
        let frame = lightning_frame0(&png).expect("frame 0");
        assert_eq!(frame.dimensions(), (96, 96));
        assert_eq!(frame.get_pixel(0, 0).0, LIGHTNING_PX);
        assert_eq!(frame.get_pixel(95, 95).0, LIGHTNING_PX);

        // Wrong grid size and garbage bytes refuse.
        let small = RgbaImage::new(96, 96);
        assert!(lightning_frame0(&encode_png(&small).expect("encode")).is_none());
        assert!(lightning_frame0(&[1, 2, 3]).is_none());
    }

    #[test]
    fn lightning_composites_additively_over_shocks_and_mines() {
        let mut shock = tap(12_000, 0);
        shock.panel_flags = [1, 1, 1, 1, 0, 0, 0, 0];
        let mut mine = tap(24_000, 1);
        mine.kind = 20;
        // The shock strike is a CONTIGUOUS 384×96 strip: left half one
        // color, right half another — after the one-stamp composite,
        // columns 0..1 must carry the left color and 2..3 the right
        // (per-panel copies would repeat the left half everywhere).
        let mut strike = RgbaImage::from_pixel(384, 96, image::Rgba(LIGHTNING_PX));
        for y in 0..96 {
            for x in 192..384 {
                strike.put_pixel(x, y, image::Rgba(STRIKE_RIGHT_PX));
            }
        }
        let mine_frame = RgbaImage::from_pixel(96, 96, image::Rgba(LIGHTNING_PX));
        let img = render_scene(
            &[shock, mine],
            &[],
            [0, 0, 0, 0],
            &test_sheet(),
            Some(&strike),
            Some(&mine_frame),
        );
        // Additive (the game's BLEND_SRC_ONE overlay): channel-clamped
        // premultiplied sums at full alphas.
        // Column 1 (down art [140,160,255] + strike-left [100,120,255])
        // → [240, 255, 255, 255].
        assert_eq!(
            img.get_pixel(18, 60).0,
            [240, 255, 255, 255],
            "shock left half + strike left"
        );
        // Column 2 (down art [140,160,255] + strike-right [80,40,0])
        // → [220, 200, 255, 255] — proves the strike is one strip, not
        // a repeated per-panel frame.
        assert_eq!(
            img.get_pixel(30, 60).0,
            [220, 200, 255, 255],
            "shock right half + strike right"
        );
        // Mine (down art + per-panel frame) → [240, 255, 255, 255].
        assert_eq!(
            img.get_pixel(18, 120).0,
            [240, 255, 255, 255],
            "mine + lightning"
        );
        // Taps are untouched by either lightning layer.
        let tap_img = render_scene(
            &[tap(60_000, 0)],
            &[],
            [0, 0, 0, 0],
            &test_sheet(),
            Some(&strike),
            Some(&mine_frame),
        );
        assert_eq!(tap_img.get_pixel(6, 300).0, RED, "tap has no lightning");
    }

    #[test]
    fn shock_effect_arc_extracts_the_strike_frame() {
        // A synthetic 2d_shock_effect-shaped arc: three size-variant
        // DDS entries (768×384), frame 0's left texel column painted
        // distinctly per variant; extraction picks the requested
        // variant and crops frame 0 (384×96).
        let make_dds = |tint: u8| {
            synthetic_dds(
                SHOCK_FX_W,
                SHOCK_FX_H,
                DDPF_RGB | DDPF_ALPHAPIXELS,
                32,
                [0x00ff_0000, 0x0000_ff00, 0x0000_00ff, 0xff00_0000],
                move |x, y| {
                    if x < SHOCK_STRIKE_W && y < CELL {
                        [tint, 1, 2, 255] // frame 0
                    } else {
                        [9, 9, 9, 255]
                    }
                },
            )
        };
        // Multi-entry arc (uncompressed entries for simplicity).
        let entries = [
            ("data/2d/shock_effect00/shock_effect00_l.dds", make_dds(11)),
            ("data/2d/shock_effect00/shock_effect00_m.dds", make_dds(22)),
            ("data/2d/shock_effect00/shock_effect00_s.dds", make_dds(33)),
        ];
        let arc_bytes = synthetic_multi_arc(&entries);
        for (suffix, tint) in [('l', 11u8), ('m', 22), ('s', 33)] {
            let frame = extract_shock_lightning(&arc_bytes, suffix).expect("extract");
            assert_eq!(frame.dimensions(), (SHOCK_STRIKE_W, CELL));
            assert_eq!(
                frame.get_pixel(0, 0).0,
                [tint, 1, 2, 255],
                "variant {suffix}"
            );
            assert_eq!(
                frame.get_pixel(SHOCK_STRIKE_W - 1, CELL - 1).0,
                [tint, 1, 2, 255]
            );
        }
        // Unknown suffix refuses.
        assert_eq!(
            extract_shock_lightning(&arc_bytes, 'x'),
            Err(StripError::Arc)
        );
    }

    #[test]
    fn guidelines_draw_full_width_under_the_notes() {
        // Lines at 0 / 30 s / 60 s → rows 0 / 150 / 300; a tap at 60 s
        // stamps OVER its line (guidelines are the bottom layer, like
        // the game's own guideline pass).
        let img = render_with(
            &[tap(60_000, 0)],
            &[0, 30_000, 60_000],
            [0, 0, 0, 0],
            &test_sheet(),
        );
        for x in [0u32, 20, 47] {
            assert_eq!(img.get_pixel(x, 0).0, GUIDELINE, "top line at x={x}");
            assert_eq!(img.get_pixel(x, 150).0, GUIDELINE, "mid line at x={x}");
        }
        // The 60 s line survives outside the glyph's columns…
        assert_eq!(img.get_pixel(30, 300).0, GUIDELINE, "line beside glyph");
        // …but the tap covers it inside its column.
        assert_eq!(img.get_pixel(6, 300).0, RED, "note over line");
        // No line rows anywhere else.
        assert_eq!(img.get_pixel(20, 151).0, [0, 0, 0, 0], "below mid line");
    }

    #[test]
    fn alpha_composes_atlas_coverage_times_palette_alpha() {
        // A half-coverage tap cell (atlas a=128) with an opaque red
        // palette entry over an opaque black background: src-over gives
        // exactly [128, 0, 0, 255].
        let mut sheet = test_sheet();
        for y in 0..CELL {
            for x in 0..CELL {
                sheet.put_pixel(x, y, image::Rgba([IDX_TAP, 0, 0, 128]));
            }
        }
        let img = render_with(&[tap(60_000, 0)], &[], [0, 0, 0, 255], &sheet);
        assert_eq!(img.get_pixel(6, 300).0, [128, 0, 0, 255]);
    }

    #[test]
    fn downscale_blends_colors_never_indices() {
        // Alternate texels between two indices that both resolve RED;
        // their index midpoint resolves CYAN. Index-averaging would
        // produce cyan pixels; color-averaging keeps everything red.
        let mut sheet = test_sheet();
        for y in 0..CELL {
            for x in 0..CELL {
                let idx = if (x + y) % 2 == 0 {
                    IDX_TAP
                } else {
                    IDX_TAP_TWIN
                };
                sheet.put_pixel(x, y, image::Rgba([idx, 0, 0, 255]));
            }
        }
        let img = render_with(&[tap(60_000, 0)], &[], [0, 0, 0, 0], &sheet);
        for y in 294..306 {
            for x in 0..12 {
                assert_eq!(img.get_pixel(x, y).0, RED, "index blend at {x},{y}");
            }
        }
    }

    #[test]
    fn later_notes_stamp_on_top() {
        // Two taps on the same panel 200 ms apart (1 px of drift): the
        // glyph boxes overlap almost entirely; the overlap must show the
        // LATER note's color (palette row 4 → WHITE).
        let first = tap(60_000, 0); // rows 294..306, RED
        let mut second = tap(60_200, 0); // rows 295..307, WHITE
        second.tap_row = 4;
        // Input deliberately out of order — the rasterizer orders by time.
        let img = render(&[second, first]);
        assert_eq!(img.get_pixel(6, 300).0, WHITE, "overlap shows later note");
        assert_eq!(
            img.get_pixel(6, 294).0,
            RED,
            "first note's top edge remains"
        );
    }

    #[test]
    fn glyphs_clip_at_strip_edges_without_panic() {
        // Notes at the extremes: glyph boxes extend past the strip and
        // must clip cleanly.
        let img = render(&[tap(0, 0), tap(120_000, 3)]);
        assert_eq!(img.get_pixel(6, 2).0, RED, "top edge glyph (lower half)");
        assert_eq!(
            img.get_pixel(42, 597).0,
            RED,
            "bottom edge glyph (upper half)"
        );
    }

    // ── §3b bar mode (the shipped HUD style) ─────────────────────────

    fn render_bars(notes: &[StripNote], guideline_ms: &[i32]) -> RgbaImage {
        render_strip_bars(
            &test_layout(),
            &StripScene {
                notes,
                guideline_ms,
                guideline_rgba: GUIDELINE,
                shock_lightning: None,
                mine_lightning: None,
                background: [0, 0, 0, 0],
            },
            &test_palette(),
        )
    }

    #[test]
    fn row_bar_color_picks_the_ramp_peak() {
        let palette = test_palette();
        // The fixture palette rows are sparse (single entries); the
        // brightest entry IS the row color.
        assert_eq!(row_bar_color(&palette, ROW_TAP), CYAN); // marker CYAN outshines RED
        assert_eq!(row_bar_color(&palette, ROW_FREEZE), YELLOW);
        // A proper ramp resolves to its bright end.
        let mut ramp: Box<StripPalette> = Box::new([[[0, 0, 0, 0]; 256]; 32]);
        for idx in 0..256usize {
            let scale = |c: u8| ((u32::from(c) * idx as u32) / 255) as u8;
            ramp[1][idx] = [scale(255), scale(90), scale(130), 255];
        }
        assert_eq!(row_bar_color(&ramp, 1), [255, 90, 130, 255]);
        // An empty row yields transparent (caller falls back).
        assert_eq!(row_bar_color(&palette, 20), [0, 0, 0, 0]);
        // Out-of-range rows clamp, never panic.
        let _ = row_bar_color(&palette, 200);
    }

    #[test]
    fn bars_render_taps_as_one_px_rows() {
        // Tap on panel 0 at 60 s: a 1-px bar across column 0 at y=300,
        // colored by the row's bar color (row 1's peak = CYAN).
        let img = render_bars(&[tap(60_000, 0)], &[]);
        for x in 0..12 {
            assert_eq!(img.get_pixel(x, 300).0, CYAN, "bar at x={x}");
        }
        assert_eq!(img.get_pixel(6, 299).0, [0, 0, 0, 0], "above bar");
        assert_eq!(img.get_pixel(6, 301).0, [0, 0, 0, 0], "below bar");
        assert_eq!(img.get_pixel(18, 300).0, [0, 0, 0, 0], "next column");
    }

    #[test]
    fn bars_render_freezes_as_solid_rects() {
        // Head on panel 2 at 30 s (y=150) + tail at 60 s (y=300): a
        // solid body rect over the span (dimmed head color), the head
        // bar at the top edge in the freeze row's full color.
        let mut head = tap(30_000, 2);
        head.durations[2] = 1;
        head.freeze_row = ROW_FREEZE;
        let mut tail = tap(60_000, 2);
        tail.kind = 2;
        let img = render_bars(&[head, tail], &[]);
        let head_color = row_bar_color(&test_palette(), ROW_FREEZE);
        let body = img.get_pixel(30, 225).0;
        // Body: the dimmed head color, opaque, present mid-span.
        assert_eq!(body[3], 255, "body opaque");
        assert!(
            body[0] < head_color[0] || body[1] < head_color[1] || body[2] < head_color[2],
            "body dimmer than head"
        );
        assert_eq!(img.get_pixel(30, 299).0, body, "body at span end");
        // Head bar on top at the span start.
        assert_eq!(img.get_pixel(30, 150).0, head_color, "head bar");
        // Nothing outside the span/column.
        assert_eq!(img.get_pixel(30, 148).0, [0, 0, 0, 0], "above head");
        assert_eq!(img.get_pixel(30, 302).0, [0, 0, 0, 0], "below body");
        assert_eq!(img.get_pixel(6, 225).0, [0, 0, 0, 0], "other column");
        // A tailless head still draws its bar (no body).
        let mut lone = tap(30_000, 2);
        lone.durations[2] = 1;
        let img = render_bars(&[lone], &[]);
        assert_eq!(img.get_pixel(30, 150).0, head_color, "lone head bar");
        assert_eq!(img.get_pixel(30, 225).0, [0, 0, 0, 0], "no body");
    }

    #[test]
    fn bars_render_shocks_full_width_and_mines_per_panel() {
        let mut shock = tap(12_000, 0);
        shock.panel_flags = [1, 1, 1, 1, 0, 0, 0, 0];
        let mut mine = tap(24_000, 1);
        mine.kind = 20;
        let img = render_bars(&[shock, mine], &[]);
        // Shock: one bar across the whole strip width at y=60.
        for x in [0u32, 12, 24, 47] {
            assert_eq!(img.get_pixel(x, 60).0, SHOCK_MINE_RGBA, "shock at x={x}");
        }
        // Mine: its own column only at y=120.
        assert_eq!(img.get_pixel(18, 120).0, SHOCK_MINE_RGBA, "mine");
        assert_eq!(img.get_pixel(6, 120).0, [0, 0, 0, 0], "mine not full-width");
    }

    #[test]
    fn bars_keep_guidelines_under_the_notes() {
        let img = render_bars(&[tap(60_000, 0)], &[0, 30_000, 60_000]);
        assert_eq!(img.get_pixel(20, 150).0, GUIDELINE, "mid line");
        // The note's bar covers its line inside the column…
        assert_eq!(img.get_pixel(6, 300).0, CYAN, "bar over line");
        // …and the line survives beside it.
        assert_eq!(img.get_pixel(30, 300).0, GUIDELINE, "line beside bar");
    }

    #[test]
    fn layout_reverse_flips_the_axis() {
        // Reverse scroll (maintainer 2026-08-14): the timeline runs
        // bottom-to-top — song start at the BOTTOM edge, exactly like
        // the lane. Same clamping; fractions follow (task-03's cursor
        // and markers ride the same mapping).
        let layout = StripLayout::new(4, 12, 600, 120_000)
            .expect("layout")
            .with_reverse(true);
        assert_eq!(layout.y_for_ms(0), 600);
        assert_eq!(layout.y_for_ms(60_000), 300);
        assert_eq!(layout.y_for_ms(120_000), 0);
        assert_eq!(layout.y_for_ms(-5_000), 600, "clamp below start");
        assert_eq!(layout.y_for_ms(500_000), 0, "clamp past end");
        assert_eq!(layout.fraction_for_ms(0), 1.0);
        assert_eq!(layout.fraction_for_ms(120_000), 0.0);
        // Forward stays the default.
        let forward = StripLayout::new(4, 12, 600, 120_000).expect("layout");
        assert_eq!(forward.y_for_ms(0), 0);
    }

    #[test]
    fn reverse_strip_renders_bottom_to_top() {
        // A tap at 12 s on a reverse layout lands near the BOTTOM
        // (y = 600 − 60 = 540), and a freeze's body still spans between
        // its endpoints' mapped positions.
        let layout = StripLayout::new(4, 12, 600, 120_000)
            .expect("layout")
            .with_reverse(true);
        let mut head = tap(30_000, 2);
        head.durations[2] = 1;
        let mut tail = tap(60_000, 2);
        tail.kind = 2;
        let img = render_strip_bars(
            &layout,
            &StripScene {
                notes: &[tap(12_000, 0), head, tail],
                guideline_ms: &[],
                guideline_rgba: GUIDELINE,
                shock_lightning: None,
                mine_lightning: None,
                background: [0, 0, 0, 0],
            },
            &test_palette(),
        );
        assert_eq!(img.get_pixel(6, 540).0, CYAN, "tap near the bottom");
        // Freeze: head maps to y=450, tail to y=300 — body spans between.
        assert_eq!(img.get_pixel(30, 375).0[3], 255, "body mid-span");
        assert_eq!(
            img.get_pixel(30, 450).0,
            row_bar_color(&test_palette(), ROW_FREEZE),
            "head bar at its mapped position"
        );
        assert_eq!(img.get_pixel(30, 500).0, [0, 0, 0, 0], "below the head");
    }

    #[test]
    fn section_veil_spans_the_active_region() {
        // No markers ⇒ the whole song is active ⇒ whole-strip veil
        // (re-demo amendment 2026-08-15).
        assert_eq!(section_veil(None, None, 120_000), Some((0, 120_000)));
        // A only ⇒ [a, chart end].
        assert_eq!(
            section_veil(Some(30_000), None, 120_000),
            Some((30_000, 120_000))
        );
        // B only ⇒ [0, b].
        assert_eq!(section_veil(None, Some(90_000), 120_000), Some((0, 90_000)));
        // Both ⇒ [a, b].
        assert_eq!(
            section_veil(Some(30_000), Some(90_000), 120_000),
            Some((30_000, 90_000))
        );
        // Values clamp to the chart.
        assert_eq!(
            section_veil(Some(-5), Some(500_000), 120_000),
            Some((0, 120_000))
        );
        // Inverted markers come back ordered (defensive — the gesture
        // clamps normally prevent this).
        assert_eq!(
            section_veil(Some(90_000), Some(30_000), 120_000),
            Some((30_000, 90_000))
        );
        // Degenerate chart ⇒ no veil.
        assert_eq!(section_veil(Some(10), None, 0), None);
    }

    // ── §4 png ───────────────────────────────────────────────────────

    #[test]
    fn png_round_trips_pixel_exact() {
        let img = render(&[tap(60_000, 0)]);
        let png = encode_png(&img).expect("encode");
        let back = image::load_from_memory(&png).expect("decode").into_rgba8();
        assert_eq!(back.dimensions(), img.dimensions());
        assert!(back.pixels().eq(img.pixels()), "png round-trip differs");
    }

    // ── Real-asset preview (env-gated, maintainer visual review) ─────

    /// Renders viewable strips from the REAL arrow sheets when
    /// `DDR_WORLD_INSTALL` points at a game install (maintainer
    /// directive 2026-08-14: real assets come only from that variable —
    /// never a committed path, never committed pixel data). Skips
    /// silently when the variable is unset, so the synthetic suite
    /// stays the portable baseline. Output:
    /// `<temp>/ddr_strip_preview/strip_arrow0N.png`.
    ///
    /// The palette is a labeled STAND-IN (per-row color ramps over the
    /// index channel approximating the stock quantization tints) — the
    /// live palette arrives in task-02 by calling the game's own
    /// generators. This preview validates extraction, cell carving,
    /// rotation, freeze assembly, and layout against real art.
    #[test]
    fn render_preview_from_real_sheets() {
        let Ok(install) = std::env::var("DDR_WORLD_INSTALL") else {
            return; // no install available — synthetic tests cover the math
        };

        // Stand-in ramp palette: index → base tint scaled by index/255.
        let mut palette: Box<StripPalette> = Box::new([[[0, 0, 0, 0]; 256]; 32]);
        let tints: [(usize, [u8; 3]); 5] = [
            (1, [255, 90, 130]),  // 4th (Note4)
            (2, [255, 215, 80]),  // 16th (Note16)
            (3, [110, 150, 255]), // 8th (Note8)
            (4, [140, 255, 120]), // other
            (8, [130, 230, 140]), // freeze
        ];
        for (row, tint) in tints {
            for idx in 0..256usize {
                let scale = |c: u8| ((u32::from(c) * idx as u32) / 255) as u8;
                palette[row][idx] = [scale(tint[0]), scale(tint[1]), scale(tint[2]), 255];
            }
        }

        // A representative 30 s chart: 4th/8th/16th taps, a jump, two
        // freezes, a shock row, a mine.
        let tinted_tap = |t: i32, panel: usize, row: u8| {
            let mut note = tap(t, panel);
            note.tap_row = row;
            note
        };
        let mut notes: Vec<StripNote> = Vec::new();
        for beat in 0..8 {
            notes.push(tinted_tap(1_000 + beat * 500, (beat % 4) as usize, 1)); // 4th walk
        }
        for step in 0..8 {
            notes.push(tinted_tap(6_000 + step * 250, ((step * 3) % 4) as usize, 3));
            // 8th run
        }
        for step in 0..16 {
            notes.push(tinted_tap(9_000 + step * 125, (step % 4) as usize, 2)); // 16th burst
        }
        // Jump (L+R).
        let mut jump = tap(12_500, 0);
        jump.panel_flags[3] = 1;
        notes.push(jump);
        // Freeze on panel 1: head at 14 s, tail at 18 s.
        let mut head = tap(14_000, 1);
        head.durations[1] = 1;
        notes.push(head);
        let mut tail = tap(18_000, 1);
        tail.kind = 2;
        notes.push(tail);
        // Freeze on panel 2 overlapping taps: 19–22 s.
        let mut head2 = tap(19_000, 2);
        head2.durations[2] = 1;
        notes.push(head2);
        let mut tail2 = tap(22_000, 2);
        tail2.kind = 2;
        notes.push(tail2);
        notes.push(tinted_tap(20_000, 0, 1));
        notes.push(tinted_tap(21_000, 3, 1));
        // Shock row at 24 s.
        let mut shock = tap(24_000, 0);
        shock.panel_flags = [1, 1, 1, 1, 0, 0, 0, 0];
        notes.push(shock);
        // Mine at 26 s.
        let mut mine = tap(26_000, 1);
        mine.kind = 20;
        notes.push(mine);

        // Measure guidelines: a 120 BPM 4/4 chart has one bar every
        // 2 s (the live enumeration in task-02 walks the chart's own
        // tick→ms mapping; the preview uses a fixed tempo).
        let guideline_ms: Vec<i32> = (0..=15).map(|bar| bar * 2_000).collect();

        let layout = StripLayout::new(4, 16, 640, 30_000).expect("layout");
        let out_dir = std::env::temp_dir().join("ddr_strip_preview");
        std::fs::create_dir_all(&out_dir).expect("preview dir");

        // Shock-effect strike frame from the STOCK arc (one contiguous
        // 384×96 strike across all four panels) — size variant matched
        // to the noteskin. Read once; fail-open to silver-only.
        let shock_fx_arc =
            std::fs::read(std::path::Path::new(&install).join("data/arc/2d/2d_shock_effect00.arc"))
                .ok();

        for design in 0..8u32 {
            let arc_path = std::path::Path::new(&install)
                .join("data/arc/2d")
                .join(format!("2d_arrow{design:02}.arc"));
            let arc_bytes = std::fs::read(&arc_path).expect("read real sheet arc");
            let sheet = extract_sheet(&arc_bytes).expect("real sheet extracts");
            let suffix = shock_size_suffix(design as u8);
            // The contiguous strike for shocks…
            let strike = shock_fx_arc
                .as_deref()
                .and_then(|arc| extract_shock_lightning(arc, suffix).ok());
            // …and the mine mod's per-panel frame for mines.
            let mine_frame = std::fs::read(std::path::Path::new(&install).join(format!(
                "data_mods/note_types_expansion/tex/note_types_mine00_{suffix}.png"
            )))
            .ok()
            .and_then(|png| lightning_frame0(&png));
            let scene = StripScene {
                notes: &notes,
                guideline_ms: &guideline_ms,
                guideline_rgba: [255, 255, 255, 48],
                shock_lightning: strike.as_ref(),
                mine_lightning: mine_frame.as_ref(),
                background: [16, 16, 24, 235],
            };
            let strip = render_strip(&layout, &scene, &sheet, &palette);
            let png = encode_png(&strip).expect("encode");
            std::fs::write(out_dir.join(format!("strip_arrow{design:02}.png")), png)
                .expect("write preview");
        }
    }
}
