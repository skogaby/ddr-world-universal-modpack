//! Pure shader-container layout decisions for the runtime synthesis
//! (`shader_synthesis.rs`) — which containers a configuration overlays,
//! how the DEFAULT container's program table is laid out once the
//! mod-menu theme programs participate, and the two facts the lit MODEL
//! containers (Background Dancers "Dancer Lighting") depend on: the
//! model pass's 4-entry program table and the FNV-1 name hash.
//!
//! Deliberately **dependency-free** (no `crate::` imports) so its tests
//! run on any host via the temp-crate harness
//! (`scripts/validate_overlay_draw.sh`). The impure synthesis consumes
//! these functions verbatim — the layout here IS the contract:
//!
//! - program 0 is always the stock pair;
//! - the player-perspective program, when enabled, is EXACTLY program 1
//!   ([`PERSP_PROGRAM_INDEX`] — `player_perspective::pass_rewrite`
//!   hardcodes it positionally);
//! - the [`THEME_PROGRAM_COUNT`] theme programs (bubbles, terminal,
//!   waveform, spectrum, tunnel, xmb, squares, card_swirl, blobs, ps2,
//!   prime_cube — the overlay-menu animated backgrounds, design §4.7)
//!   are appended LAST, in that order
//!   (== `ThemeProgram::slot()` order in `mod_menu::theme` and the
//!   `THEME_BLOBS` order in `shader_synthesis`), in every configuration
//!   that carries them;
//! - a MODEL container carries [`MODEL_PROGRAM_ENTRIES`] identical
//!   `(0, 0, 0)` entries — the model pass binds `programs[stage]` with
//!   `stage ∈ {0, 2}` in production and NO bounds check
//!   (`docs/background_dancers_research.md` §4.2), exactly like every
//!   stock `gs_model_*` / `mdl_*` container;
//! - the model VARIANT containers are `<material_name><suffix>` for every
//!   [`MODEL_VARIANTS`] shape × [`StyleFamily`] (lit, cel, and the nine DDR
//!   Universe families — [`StyleFamily::served`]); the six sphere-map styles
//!   share one family, so the Lighting Style count and the container count
//!   differ by design.

/// The player-perspective program's index in every container that has
/// one — a positional contract with `pass_rewrite` (never move it).
pub const PERSP_PROGRAM_INDEX: u8 = 1;

/// How many shader-backed menu themes ride the DEFAULT container. Must
/// stay in lockstep with `ThemeProgram` (`mod_menu::theme`) and
/// `THEME_BLOBS` (`shader_synthesis`).
pub const THEME_PROGRAM_COUNT: u8 = 11;

/// Program entries of a MODEL container (the stock `gs_model_*` / `mdl_*`
/// shape). The pass indexes the handle array by a per-record "stage" that
/// is 2 for ordinary records and 0 for `rec+0x28` bit-31 records, without
/// a bounds check — fewer than four entries reads past the array.
pub const MODEL_PROGRAM_ENTRIES: u8 = 4;

/// The program index bound for draw records carrying flag bit 31 — the
/// DLL's inverted-hull records (`docs/background_dancers_research.md` §4.6).
pub const MODEL_HULL_PROGRAM_INDEX: u8 = 0;

/// How the 3D scene's models are shaded (`background_dancers.style`).
/// `Stock` re-points nothing (the engine's own shaders — unlit). `Lit`
/// applies a fixed world-space key light smoothly per vertex; `Cel` is
/// Dancing Stage Unleashed's toon shading (its 2-band `toon.tga` ramp on
/// per-vertex N·L under its default light, `shaders/src/mdl_cel.hlsl`), plus
/// DSU's black hull outlines when enabled. The rest are DanceDanceRevolution
/// Universe's per-song character render styles
/// (`docs/ddr_universe_render_styles_research.md`, `shaders/src/mdl_universe.hlsl`):
/// they restyle DANCERS only ([`SceneStyle::stage_style`] = CEL), some bind a
/// style texture ([`SceneStyle::texture`]) and some draw additively
/// ([`SceneStyle::additive`]). `Outline` (Universe's OUTLINE: only the black
/// ink, the body invisible but writing depth) needs the dancers' hulls.
/// `Random` is not a look of its own: the dancers take a different preset
/// every chart bar ([`SceneStyle::random_pool`]; the session picks).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneStyle {
    Random,
    Stock,
    Lit,
    Cel,
    Contrast,
    Highlight,
    Shadow,
    Negative,
    Mono,
    Sepia,
    Outline,
    Stencil,
    StencilRainbow,
    Thermo,
    Mercury,
    Candy,
    Bubble,
    Crystal,
    Xray,
}

/// Historical alias (the first cabinet builds called it the dancers' style).
pub type DancerStyle = SceneStyle;

/// The program family a style draws with — one synthesized container per
/// (model shape, family), named `<material_name><suffix>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StyleFamily {
    Lit,
    Cel,
    Uni(UniFamily),
}

/// The Universe pixel-shader families (`mdl_universe.hlsl` `UNI_*`). The six
/// sphere-map styles share [`UniFamily::Sphere`] and STENCIL RAINBOW is
/// [`UniFamily::ScreenTex`]: they differ only in the style texture the DLL
/// binds and (for four of them) the additive record flags. `Invisible` is
/// OUTLINE ONLY's depth-only body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UniFamily {
    Contrast,
    Highlight,
    Shadow,
    Negative,
    Mono,
    Sepia,
    Stencil,
    ScreenTex,
    Sphere,
    Invisible,
}

impl UniFamily {
    pub const ALL: [UniFamily; 10] = [
        UniFamily::Contrast,
        UniFamily::Highlight,
        UniFamily::Shadow,
        UniFamily::Negative,
        UniFamily::Mono,
        UniFamily::Sepia,
        UniFamily::Stencil,
        UniFamily::ScreenTex,
        UniFamily::Sphere,
        UniFamily::Invisible,
    ];

    /// The blob / container tag (`mdl_uni_<tag>.ps.d3dbc`, `_u<tag>`).
    pub fn tag(self) -> &'static str {
        match self {
            UniFamily::Contrast => "con",
            UniFamily::Highlight => "hil",
            UniFamily::Shadow => "sha",
            UniFamily::Negative => "neg",
            UniFamily::Mono => "mon",
            UniFamily::Sepia => "sep",
            UniFamily::Stencil => "ste",
            UniFamily::ScreenTex => "scr",
            UniFamily::Sphere => "sph",
            UniFamily::Invisible => "inv",
        }
    }

    /// Whether the family shades the material colour (and so has a `_c`
    /// constant-colour PS variant).
    pub fn uses_material_colour(self) -> bool {
        matches!(
            self,
            UniFamily::Negative | UniFamily::Mono | UniFamily::Sepia
        )
    }
}

impl StyleFamily {
    /// Every family a model container exists for.
    pub fn all() -> Vec<StyleFamily> {
        Self::served(true)
    }

    /// The families synthesized this boot: lit + cel always, the Universe
    /// ones only when their blobs resolved (`universe`).
    pub fn served(universe: bool) -> Vec<StyleFamily> {
        let mut v = vec![StyleFamily::Lit, StyleFamily::Cel];
        if universe {
            v.extend(UniFamily::ALL.iter().map(|&f| StyleFamily::Uni(f)));
        }
        v
    }

    /// Container-name suffix.
    pub fn suffix(self) -> String {
        match self {
            StyleFamily::Lit => "_lit".to_string(),
            StyleFamily::Cel => "_cel".to_string(),
            StyleFamily::Uni(f) => format!("_u{}", f.tag()),
        }
    }
}

/// A texture a Universe style samples at PS s1 (material slot 1). The DLL
/// loads [`STYLE_TEXTURE_ARC`] with the dancers when the style needs one and
/// binds the texture the game registered under FNV-1 of the stem (lower-case,
/// no `_` — `scripts/gen_universe_style_textures.py` writes exactly these).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StyleTexture {
    Rainbow,
    Thermo,
    Chrome,
    Crystal,
    Xray,
}

/// The style-texture arc (`data_mods/background_dancers/arc/bdfxuniverse.arc`).
pub const STYLE_TEXTURE_ARC: &str = "data/arc/bdfxuniverse.arc";

impl StyleTexture {
    pub const ALL: [StyleTexture; 5] = [
        StyleTexture::Rainbow,
        StyleTexture::Thermo,
        StyleTexture::Chrome,
        StyleTexture::Crystal,
        StyleTexture::Xray,
    ];

    pub fn stem(self) -> &'static str {
        match self {
            StyleTexture::Rainbow => "bdfxunirainbow",
            StyleTexture::Thermo => "bdfxunithermo",
            StyleTexture::Chrome => "bdfxunichrome",
            StyleTexture::Crystal => "bdfxunicrystal",
            StyleTexture::Xray => "bdfxunixray",
        }
    }
}

impl SceneStyle {
    /// Row order (= `row_value`).
    pub const ALL: [SceneStyle; 19] = [
        SceneStyle::Random,
        SceneStyle::Stock,
        SceneStyle::Lit,
        SceneStyle::Cel,
        SceneStyle::Contrast,
        SceneStyle::Highlight,
        SceneStyle::Shadow,
        SceneStyle::Negative,
        SceneStyle::Mono,
        SceneStyle::Sepia,
        SceneStyle::Outline,
        SceneStyle::Stencil,
        SceneStyle::StencilRainbow,
        SceneStyle::Thermo,
        SceneStyle::Mercury,
        SceneStyle::Candy,
        SceneStyle::Bubble,
        SceneStyle::Crystal,
        SceneStyle::Xray,
    ];

    /// Config spelling (`"stock"` / `"lit"` / `"cel"` / the Universe names,
    /// case-insensitive, Universe's aliases accepted). Unknown ⇒ `None` (the
    /// caller WARNs and falls back).
    pub fn parse(s: &str) -> Option<SceneStyle> {
        let k = s.trim().to_ascii_lowercase().replace([' ', '-'], "_");
        match k.as_str() {
            "random" | "random_every_bar" | "random_(every_bar)" => Some(SceneStyle::Random),
            "stock" | "off" | "none" => Some(SceneStyle::Stock),
            "lit" | "lambert" | "enhanced" => Some(SceneStyle::Lit),
            "cel" | "toon" | "enhanced_cel" => Some(SceneStyle::Cel),
            "contrast" => Some(SceneStyle::Contrast),
            "highlight" => Some(SceneStyle::Highlight),
            "shadow" => Some(SceneStyle::Shadow),
            "negative" | "blackgloss" => Some(SceneStyle::Negative),
            "mono" | "monochrome" => Some(SceneStyle::Mono),
            "sepia" => Some(SceneStyle::Sepia),
            "outline" | "outline_only" | "line_art" | "pencil" => Some(SceneStyle::Outline),
            "stencil" | "fill_in" => Some(SceneStyle::Stencil),
            "stencil_rainbow" => Some(SceneStyle::StencilRainbow),
            "thermo" => Some(SceneStyle::Thermo),
            "mercury" | "t1000" => Some(SceneStyle::Mercury),
            "candy" => Some(SceneStyle::Candy),
            "bubble" | "stone" => Some(SceneStyle::Bubble),
            "crystal" => Some(SceneStyle::Crystal),
            "xray" | "x_ray" | "alpha_light" => Some(SceneStyle::Xray),
            _ => None,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            SceneStyle::Random => "random",
            SceneStyle::Stock => "stock",
            SceneStyle::Lit => "lit",
            SceneStyle::Cel => "cel",
            SceneStyle::Contrast => "contrast",
            SceneStyle::Highlight => "highlight",
            SceneStyle::Shadow => "shadow",
            SceneStyle::Negative => "negative",
            SceneStyle::Mono => "mono",
            SceneStyle::Sepia => "sepia",
            SceneStyle::Outline => "outline",
            SceneStyle::Stencil => "stencil",
            SceneStyle::StencilRainbow => "stencil_rainbow",
            SceneStyle::Thermo => "thermo",
            SceneStyle::Mercury => "mercury",
            SceneStyle::Candy => "candy",
            SceneStyle::Bubble => "bubble",
            SceneStyle::Crystal => "crystal",
            SceneStyle::Xray => "xray",
        }
    }

    /// The LIGHTING STYLE row text.
    pub fn label(self) -> &'static str {
        match self {
            SceneStyle::Random => "RANDOM (EVERY BAR)",
            SceneStyle::Stock => "STOCK (UNLIT)",
            SceneStyle::Lit => "SMOOTH SHADING",
            SceneStyle::Cel => "CEL SHADING",
            SceneStyle::Contrast => "CONTRAST",
            SceneStyle::Highlight => "HIGHLIGHT",
            SceneStyle::Shadow => "SHADOW",
            SceneStyle::Negative => "NEGATIVE",
            SceneStyle::Mono => "MONOCHROME",
            SceneStyle::Sepia => "SEPIA",
            SceneStyle::Outline => "OUTLINE ONLY",
            SceneStyle::Stencil => "STENCIL",
            SceneStyle::StencilRainbow => "STENCIL RAINBOW",
            SceneStyle::Thermo => "THERMO",
            SceneStyle::Mercury => "MERCURY",
            SceneStyle::Candy => "CANDY",
            SceneStyle::Bubble => "BUBBLE",
            SceneStyle::Crystal => "CRYSTAL",
            SceneStyle::Xray => "X-RAY",
        }
    }

    /// Overlay-row value (row index in [`SceneStyle::ALL`]).
    pub fn row_value(self) -> i32 {
        SceneStyle::ALL
            .iter()
            .position(|&s| s == self)
            .map_or(0, |i| i as i32)
    }

    pub fn from_row_value(v: i32) -> SceneStyle {
        usize::try_from(v)
            .ok()
            .and_then(|i| SceneStyle::ALL.get(i).copied())
            .unwrap_or(SceneStyle::Stock)
    }

    /// A DDR Universe render style (dancers only). `Random` counts: it
    /// restyles the dancers only too.
    pub fn is_universe(self) -> bool {
        !matches!(self, SceneStyle::Stock | SceneStyle::Lit | SceneStyle::Cel)
    }

    /// The program family a DANCER is drawn with (`None` = stock shaders, and
    /// `Random`, which resolves to a preset per bar first).
    pub fn family(self) -> Option<StyleFamily> {
        Some(match self {
            SceneStyle::Stock | SceneStyle::Random => return None,
            SceneStyle::Lit => StyleFamily::Lit,
            SceneStyle::Cel => StyleFamily::Cel,
            SceneStyle::Contrast => StyleFamily::Uni(UniFamily::Contrast),
            SceneStyle::Highlight => StyleFamily::Uni(UniFamily::Highlight),
            SceneStyle::Shadow => StyleFamily::Uni(UniFamily::Shadow),
            SceneStyle::Negative => StyleFamily::Uni(UniFamily::Negative),
            SceneStyle::Mono => StyleFamily::Uni(UniFamily::Mono),
            SceneStyle::Sepia => StyleFamily::Uni(UniFamily::Sepia),
            SceneStyle::Outline => StyleFamily::Uni(UniFamily::Invisible),
            SceneStyle::Stencil => StyleFamily::Uni(UniFamily::Stencil),
            SceneStyle::StencilRainbow => StyleFamily::Uni(UniFamily::ScreenTex),
            SceneStyle::Thermo
            | SceneStyle::Mercury
            | SceneStyle::Candy
            | SceneStyle::Bubble
            | SceneStyle::Crystal
            | SceneStyle::Xray => StyleFamily::Uni(UniFamily::Sphere),
        })
    }

    /// The style STAGE props take (and the Winx Club wipe containers, which
    /// exist for stock / lit / cel only): CEL for every Universe style.
    pub fn stage_style(self) -> SceneStyle {
        if self.is_universe() {
            SceneStyle::Cel
        } else {
            self
        }
    }

    /// The style texture a dancer material samples at s1.
    pub fn texture(self) -> Option<StyleTexture> {
        match self {
            SceneStyle::StencilRainbow => Some(StyleTexture::Rainbow),
            SceneStyle::Thermo | SceneStyle::Candy => Some(StyleTexture::Thermo),
            SceneStyle::Mercury | SceneStyle::Bubble => Some(StyleTexture::Chrome),
            SceneStyle::Crystal => Some(StyleTexture::Crystal),
            SceneStyle::Xray => Some(StyleTexture::Xray),
            _ => None,
        }
    }

    /// Universe draws these additively (blend ONE/ONE, z-write off, no cull):
    /// the dancer's restyled records are rewritten
    /// (`render_item_layout::additive_record_flags`).
    pub fn additive(self) -> bool {
        matches!(
            self,
            SceneStyle::Candy | SceneStyle::Bubble | SceneStyle::Crystal | SceneStyle::Xray
        )
    }

    /// Whether dancers get the black hull outline (when SCENE OUTLINES is on):
    /// the World styles, and the three Universe styles that draw Universe's
    /// outline pass (NEGATIVE / MONOCHROME / SEPIA). OUTLINE ONLY's hull is
    /// the style itself ([`SceneStyle::needs_hulls`]), not this switch.
    pub fn outlines(self) -> bool {
        matches!(
            self,
            SceneStyle::Lit
                | SceneStyle::Cel
                | SceneStyle::Negative
                | SceneStyle::Mono
                | SceneStyle::Sepia
        )
    }

    /// The dancers must have hulls whatever SCENE OUTLINES says: OUTLINE ONLY
    /// is nothing but the hull (and RANDOM may pick it).
    pub fn needs_hulls(self) -> bool {
        matches!(self, SceneStyle::Outline | SceneStyle::Random)
    }

    /// The presets RANDOM (EVERY BAR) picks from, in row order: every look
    /// except STOCK and RANDOM itself, minus what this boot cannot serve —
    /// the DDR Universe containers (`universe`), the style-texture arc
    /// (`textures`), the dancers' hulls (`hulls`, OUTLINE ONLY).
    pub fn random_pool(universe: bool, textures: bool, hulls: bool) -> Vec<SceneStyle> {
        SceneStyle::ALL
            .iter()
            .copied()
            .filter(|s| !matches!(s, SceneStyle::Random | SceneStyle::Stock))
            .filter(|s| universe || !s.is_universe())
            .filter(|s| textures || s.texture().is_none())
            .filter(|s| hulls || *s != SceneStyle::Outline)
            .collect()
    }
}

/// A stock model shader NAME the stage/character arcs use, with the blobs
/// its two style variants and outline pair are built from. Every field is a
/// committed blob file name under `data_mods/shader_fixes/blobs/` except
/// `lit_ps_donor`, the STOCK container whose PS the LIT variant pairs with
/// (sliced from `shader.arc` at synthesis — the lit factor rides the VS's
/// COLOR0 output, so each name keeps its own stock pixel path bit-exact).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelVariant {
    /// The stock shader name (`fnv1_32` of it = the hash the converter
    /// stored in every material that names it — for the dancers'
    /// `mdl_*_lambert`, which have no container, the FALLBACK's name).
    pub stock_name: &'static str,
    /// The material shader name this variant set is FOR (differs from
    /// `stock_name` only for the lambert fallbacks).
    pub material_name: &'static str,
    pub lit_vs: &'static str,
    pub lit_ps_donor: &'static str,
    pub cel_vs: &'static str,
    pub cel_ps: &'static str,
    /// The Universe-style VS of this shape (`mdl_universe.hlsl`); the PS is
    /// per family ([`ModelVariant::uni_ps`]).
    pub uni_vs: &'static str,
    pub outline_vs: &'static str,
    pub outline_ps: &'static str,
}

/// Every model shader shape the shipped World stage/character arcs use
/// (`docs/background_dancers_research.md` §4.4/§4.7): the dancers'
/// `mdl_ch_lambert` (68 materials) / `mdl_bg_lambert` (90) resolve to the
/// engine's `gs_model_skinning_default` / `gs_model_default` fallbacks; the
/// stages use `mdl_ch_constant_vc` (217), `mdl_bg_constant_vc` (96),
/// `mdl_ch_constant_c_vc` (7), `mdl_bg_constant` (2 + the shadow),
/// `mdl_ch_constant_c` (1), `mdl_bg_constant_c_vc` (1),
/// `mdl_ch_constant_vc_notex` (1). `ch` = skinned VS, `bg` = static, `vc` =
/// COLOR0 multiplied in, `c` = `vConstatntColor`/`vOffsetColor` applied,
/// `notex` = untextured. The three unused stock names get no variant.
pub const MODEL_VARIANTS: [ModelVariant; 9] = [
    ModelVariant {
        stock_name: "gs_model_default",
        material_name: "mdl_bg_lambert",
        lit_vs: "mdl_bg_lambert.vs.d3dbc",
        lit_ps_donor: "gs_model_default",
        cel_vs: "mdl_bg_cel.vs.d3dbc",
        cel_ps: "mdl_cel.ps.d3dbc",
        uni_vs: "mdl_bg_uni.vs.d3dbc",
        outline_vs: "mdl_bg_outline.vs.d3dbc",
        outline_ps: "mdl_outline.ps.d3dbc",
    },
    ModelVariant {
        stock_name: "gs_model_skinning_default",
        material_name: "mdl_ch_lambert",
        lit_vs: "mdl_ch_lambert.vs.d3dbc",
        lit_ps_donor: "gs_model_default", // byte-identical to the skinning one's
        cel_vs: "mdl_ch_cel.vs.d3dbc",
        cel_ps: "mdl_cel.ps.d3dbc",
        uni_vs: "mdl_ch_uni.vs.d3dbc",
        outline_vs: "mdl_ch_outline.vs.d3dbc",
        outline_ps: "mdl_outline.ps.d3dbc",
    },
    ModelVariant {
        stock_name: "mdl_bg_constant",
        material_name: "mdl_bg_constant",
        lit_vs: "mdl_bg_lit_uv3.vs.d3dbc",
        lit_ps_donor: "mdl_bg_constant",
        cel_vs: "mdl_bg_cel.vs.d3dbc",
        cel_ps: "mdl_cel.ps.d3dbc",
        uni_vs: "mdl_bg_uni.vs.d3dbc",
        outline_vs: "mdl_bg_outline.vs.d3dbc",
        outline_ps: "mdl_outline.ps.d3dbc",
    },
    ModelVariant {
        stock_name: "mdl_bg_constant_vc",
        material_name: "mdl_bg_constant_vc",
        lit_vs: "mdl_bg_lit_uv3_vc.vs.d3dbc",
        lit_ps_donor: "mdl_bg_constant_vc",
        cel_vs: "mdl_bg_cel_vc.vs.d3dbc",
        cel_ps: "mdl_cel.ps.d3dbc",
        uni_vs: "mdl_bg_uni_vc.vs.d3dbc",
        outline_vs: "mdl_bg_outline.vs.d3dbc",
        outline_ps: "mdl_outline.ps.d3dbc",
    },
    ModelVariant {
        stock_name: "mdl_bg_constant_c_vc",
        material_name: "mdl_bg_constant_c_vc",
        lit_vs: "mdl_bg_lit_uv3_vc.vs.d3dbc",
        lit_ps_donor: "mdl_bg_constant_c_vc",
        cel_vs: "mdl_bg_cel_vc.vs.d3dbc",
        cel_ps: "mdl_cel_c.ps.d3dbc",
        uni_vs: "mdl_bg_uni_vc.vs.d3dbc",
        outline_vs: "mdl_bg_outline.vs.d3dbc",
        outline_ps: "mdl_outline.ps.d3dbc",
    },
    ModelVariant {
        stock_name: "mdl_ch_constant_vc",
        material_name: "mdl_ch_constant_vc",
        lit_vs: "mdl_ch_lit_uv3_vc.vs.d3dbc",
        lit_ps_donor: "mdl_ch_constant_vc",
        cel_vs: "mdl_ch_cel_vc.vs.d3dbc",
        cel_ps: "mdl_cel.ps.d3dbc",
        uni_vs: "mdl_ch_uni_vc.vs.d3dbc",
        outline_vs: "mdl_ch_outline.vs.d3dbc",
        outline_ps: "mdl_outline.ps.d3dbc",
    },
    ModelVariant {
        stock_name: "mdl_ch_constant_c_vc",
        material_name: "mdl_ch_constant_c_vc",
        lit_vs: "mdl_ch_lit_uv3_vc.vs.d3dbc",
        lit_ps_donor: "mdl_ch_constant_c_vc",
        cel_vs: "mdl_ch_cel_vc.vs.d3dbc",
        cel_ps: "mdl_cel_c.ps.d3dbc",
        uni_vs: "mdl_ch_uni_vc.vs.d3dbc",
        outline_vs: "mdl_ch_outline.vs.d3dbc",
        outline_ps: "mdl_outline.ps.d3dbc",
    },
    ModelVariant {
        stock_name: "mdl_ch_constant_c",
        material_name: "mdl_ch_constant_c",
        lit_vs: "mdl_ch_lit_uv3.vs.d3dbc",
        lit_ps_donor: "mdl_ch_constant_c",
        cel_vs: "mdl_ch_cel.vs.d3dbc",
        cel_ps: "mdl_cel_c.ps.d3dbc",
        uni_vs: "mdl_ch_uni.vs.d3dbc",
        outline_vs: "mdl_ch_outline.vs.d3dbc",
        outline_ps: "mdl_outline.ps.d3dbc",
    },
    ModelVariant {
        stock_name: "mdl_ch_constant_vc_notex",
        material_name: "mdl_ch_constant_vc_notex",
        lit_vs: "mdl_ch_lit_notex_vc.vs.d3dbc",
        lit_ps_donor: "mdl_ch_constant_vc_notex",
        cel_vs: "mdl_ch_cel_vc.vs.d3dbc",
        cel_ps: "mdl_cel_notex.ps.d3dbc",
        uni_vs: "mdl_ch_uni_vc.vs.d3dbc",
        outline_vs: "mdl_ch_outline.vs.d3dbc",
        outline_ps: "mdl_outline_notex.ps.d3dbc",
    },
];

/// Suffix of a style-variant container name: `<material_name><suffix>`
/// (`None` for stock — no container).
pub fn variant_suffix(style: DancerStyle) -> Option<String> {
    style.family().map(StyleFamily::suffix)
}

/// The synthesized container name for `variant` in `style` (`None` for stock).
pub fn variant_container_name(variant: &ModelVariant, style: DancerStyle) -> Option<String> {
    style.family().map(|f| family_container_name(variant, f))
}

/// The synthesized container name for `variant` drawn with `family`.
pub fn family_container_name(variant: &ModelVariant, family: StyleFamily) -> String {
    format!("{}{}", variant.material_name, family.suffix())
}

impl ModelVariant {
    /// The untextured shape (`_notex`).
    pub fn is_notex(&self) -> bool {
        self.material_name.ends_with("_notex")
    }

    /// A constant-colour shape (`_c` — `vConstatntColor` / `vOffsetColor`).
    pub fn is_const_colour(&self) -> bool {
        self.material_name.contains("_c_") || self.material_name.ends_with("_c")
    }

    /// The Universe PS of this shape for `family`: the `_notex` variant for
    /// the untextured shape, the `_c` one where the family shades the
    /// material colour of a constant-colour shape, else the base one.
    pub fn uni_ps(&self, family: UniFamily) -> &'static str {
        let notex = self.is_notex();
        let c = self.is_const_colour() && family.uses_material_colour();
        match (family, notex, c) {
            (UniFamily::Contrast, true, _) => "mdl_uni_con_notex.ps.d3dbc",
            (UniFamily::Contrast, false, _) => "mdl_uni_con.ps.d3dbc",
            (UniFamily::Highlight, true, _) => "mdl_uni_hil_notex.ps.d3dbc",
            (UniFamily::Highlight, false, _) => "mdl_uni_hil.ps.d3dbc",
            (UniFamily::Shadow, true, _) => "mdl_uni_sha_notex.ps.d3dbc",
            (UniFamily::Shadow, false, _) => "mdl_uni_sha.ps.d3dbc",
            (UniFamily::Negative, true, _) => "mdl_uni_neg_notex.ps.d3dbc",
            (UniFamily::Negative, false, true) => "mdl_uni_neg_c.ps.d3dbc",
            (UniFamily::Negative, false, false) => "mdl_uni_neg.ps.d3dbc",
            (UniFamily::Mono, true, _) => "mdl_uni_mon_notex.ps.d3dbc",
            (UniFamily::Mono, false, true) => "mdl_uni_mon_c.ps.d3dbc",
            (UniFamily::Mono, false, false) => "mdl_uni_mon.ps.d3dbc",
            (UniFamily::Sepia, true, _) => "mdl_uni_sep_notex.ps.d3dbc",
            (UniFamily::Sepia, false, true) => "mdl_uni_sep_c.ps.d3dbc",
            (UniFamily::Sepia, false, false) => "mdl_uni_sep.ps.d3dbc",
            (UniFamily::Stencil, true, _) => "mdl_uni_ste_notex.ps.d3dbc",
            (UniFamily::Stencil, false, _) => "mdl_uni_ste.ps.d3dbc",
            (UniFamily::ScreenTex, true, _) => "mdl_uni_scr_notex.ps.d3dbc",
            (UniFamily::ScreenTex, false, _) => "mdl_uni_scr.ps.d3dbc",
            (UniFamily::Sphere, true, _) => "mdl_uni_sph_notex.ps.d3dbc",
            (UniFamily::Sphere, false, _) => "mdl_uni_sph.ps.d3dbc",
            (UniFamily::Invisible, true, _) => "mdl_uni_inv_notex.ps.d3dbc",
            (UniFamily::Invisible, false, _) => "mdl_uni_inv.ps.d3dbc",
        }
    }
}

/// The variant whose STOCK container (or fallback) hashes to `stock_hash` —
/// what the DLL reads back out of a material copy's shader object to decide
/// which variant to re-point it at.
pub fn variant_for_stock_hash(stock_hash: u32) -> Option<&'static ModelVariant> {
    MODEL_VARIANTS
        .iter()
        .find(|v| fnv1_32(v.stock_name) == stock_hash)
}

/// Every distinct blob file the variant set references, plus the donor
/// stock names — what the synthesis must resolve/slice.
pub fn variant_blob_names() -> Vec<&'static str> {
    let mut v: Vec<&'static str> = Vec::new();
    for m in MODEL_VARIANTS.iter() {
        let mut blobs = vec![
            m.lit_vs,
            m.cel_vs,
            m.cel_ps,
            m.uni_vs,
            m.outline_vs,
            m.outline_ps,
        ];
        blobs.extend(UniFamily::ALL.iter().map(|&f| m.uni_ps(f)));
        for b in blobs {
            if !v.contains(&b) {
                v.push(b);
            }
        }
    }
    v
}

/// A Universe-style blob (`uni_vs` / `uni_ps`) — these soft-degrade on their
/// own: a missing one drops only the Universe families.
pub fn is_universe_blob(name: &str) -> bool {
    MODEL_VARIANTS
        .iter()
        .any(|m| m.uni_vs == name || UniFamily::ALL.iter().any(|&f| m.uni_ps(f) == name))
}

/// A model shader NAME whose materials can take the Winx Club
/// transformation WIPE (Background Dancers, `docs/wii_ddr_winx_club_research.md`
/// §7.3): `<name>_wipe` (stock look), `<name>_lit_wipe`, `<name>_cel_wipe`.
/// Every one pairs OUR VS (the stock VS has no clip-position interpolator;
/// the `_wipe` one IS the stock `mdl_ch_constant_vc` math, `mdl_lambert.hlsl`
/// `UNLIT`) with a PS that discards on one side of the line in PS c2.w. No
/// outline pair (hull items never point at a wipe container).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WipeVariant {
    pub material_name: &'static str,
    pub stock_vs: &'static str,
    pub lit_vs: &'static str,
    pub cel_vs: &'static str,
    /// The PS of the stock and lit wipes (`mdl_*_vc`'s `tex · COLOR0`).
    pub uv3_ps: &'static str,
    pub cel_ps: &'static str,
}

/// The ported dancers' (Winx Club) only body shader.
pub const WIPE_VARIANTS: [WipeVariant; 1] = [WipeVariant {
    material_name: "mdl_ch_constant_vc",
    stock_vs: "mdl_ch_unlit_uv3_vc_wipe.vs.d3dbc",
    lit_vs: "mdl_ch_lit_uv3_vc_wipe.vs.d3dbc",
    cel_vs: "mdl_ch_cel_vc_wipe.vs.d3dbc",
    uv3_ps: "mdl_uv3_wipe.ps.d3dbc",
    cel_ps: "mdl_cel_wipe.ps.d3dbc",
}];

/// Suffix of a wipe container: `_wipe` / `_lit_wipe` / `_cel_wipe` (a
/// Universe style takes the CEL wipe — [`SceneStyle::stage_style`]).
pub fn wipe_suffix(style: DancerStyle) -> &'static str {
    match style.stage_style() {
        DancerStyle::Lit => "_lit_wipe",
        DancerStyle::Cel => "_cel_wipe",
        _ => "_wipe",
    }
}

pub fn wipe_container_name(variant: &WipeVariant, style: DancerStyle) -> String {
    format!("{}{}", variant.material_name, wipe_suffix(style))
}

/// The wipe variant for a material whose (STOCK) shader object hashes to
/// `stock_hash`.
pub fn wipe_variant_for_stock_hash(stock_hash: u32) -> Option<&'static WipeVariant> {
    WIPE_VARIANTS
        .iter()
        .find(|v| fnv1_32(v.material_name) == stock_hash)
}

/// The `(VS, PS)` blobs of a wipe container.
pub fn wipe_blobs(variant: &WipeVariant, style: DancerStyle) -> (&'static str, &'static str) {
    match style.stage_style() {
        DancerStyle::Lit => (variant.lit_vs, variant.uv3_ps),
        DancerStyle::Cel => (variant.cel_vs, variant.cel_ps),
        _ => (variant.stock_vs, variant.uv3_ps),
    }
}

/// Every distinct blob the wipe set references.
pub fn wipe_blob_names() -> Vec<&'static str> {
    let mut v: Vec<&'static str> = Vec::new();
    for w in WIPE_VARIANTS.iter() {
        for b in [w.stock_vs, w.lit_vs, w.cel_vs, w.uv3_ps, w.cel_ps] {
            if !v.contains(&b) {
                v.push(b);
            }
        }
    }
    v
}

/// Which containers a configuration synthesizes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlannedContainers {
    pub arrow: bool,
    pub judge: bool,
    pub default: bool,
    /// Every `MODEL_VARIANTS` × [`StyleFamily::all`] container.
    pub model_variants: bool,
}

/// Container set for a configuration (the minimal-overlay rule): arrow +
/// judge when AA or perspective is on; default when perspective needs
/// its VS there OR the theme programs ride it; the model style variants
/// iff the 3D scene can use them (independent of the rest).
pub fn planned(aa: bool, persp: bool, themes: bool, variants: bool) -> PlannedContainers {
    PlannedContainers {
        arrow: aa || persp,
        judge: aa || persp,
        default: persp || themes,
        model_variants: variants,
    }
}

/// Program table of a MODEL container: [`MODEL_PROGRAM_ENTRIES`] entries.
/// Without an outline pair every entry is `(0, 0, 0)` (the engine's GSPW
/// parser dedupes identical triples onto one created program). With one,
/// program [`MODEL_HULL_PROGRAM_INDEX`] = `(0, 1, 1)` — VS table `[style,
/// outline]`, PS table `[style, outline]` — and the ordinary indices 1..3
/// stay the style pair, so every `programs[stage]` read is in bounds and
/// program 0 is inert unless a bit-31 record exists.
pub fn model_programs(outline: bool) -> Vec<(u8, u8, u8)> {
    let mut v = vec![(0, 0, 0); MODEL_PROGRAM_ENTRIES as usize];
    if outline {
        v[MODEL_HULL_PROGRAM_INDEX as usize] = (0, 1, 1);
    }
    v
}

/// Expected MODEL container table sizes `(vs_count, ps_count)`.
pub fn model_table_counts(outline: bool) -> (u8, u8) {
    let n = 1 + outline as u8;
    (n, n)
}

/// FNV-1 32-bit (multiply THEN xor) over the bare shader name — the hash
/// the engine's `DAT_1806f2040` hasher computes over a material's shader
/// name and compares against the GSPW header at `+0x04`. Byte-for-byte
/// `scripts/gsp_pack.py::fnv1_32`. Used only for containers that have no
/// stock header to copy the hash from (the lit model containers).
pub fn fnv1_32(name: &str) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in name.as_bytes() {
        h = h.wrapping_mul(0x0100_0193);
        h ^= b as u32;
    }
    h
}

/// The DEFAULT container's program tuples `(flags, vs_idx, ps_idx)`.
///
/// Tables the synthesis assembles to match:
/// - VS: `[stock]` + persp VS (when persp) + theme passthrough VS
///   (when themes) — so the theme VS index is `1 + persp`.
/// - PS: `[stock]` + one PS per shader-backed theme (when themes) —
///   theme PS indices `1..=THEME_PROGRAM_COUNT`.
///
/// Empty when the container isn't synthesized at all.
pub fn default_programs(persp: bool, themes: bool) -> Vec<(u8, u8, u8)> {
    if !persp && !themes {
        return Vec::new();
    }
    let mut programs: Vec<(u8, u8, u8)> = vec![(0, 0, 0)];
    if persp {
        programs.push((0, 1, 0));
    }
    if themes {
        let theme_vs = 1 + persp as u8;
        for theme_ps in 1..=THEME_PROGRAM_COUNT {
            programs.push((0, theme_vs, theme_ps));
        }
    }
    programs
}

/// The theme programs' indices in the DEFAULT container's program table
/// (in `ThemeProgram::slot()` order) — what the overlay-draw emitter
/// binds. `None` when themes don't participate.
pub fn default_theme_indices(
    persp: bool,
    themes: bool,
) -> Option<[u8; THEME_PROGRAM_COUNT as usize]> {
    if !themes {
        return None;
    }
    let first = 1 + persp as u8;
    let mut idx = [0u8; THEME_PROGRAM_COUNT as usize];
    for (i, slot) in idx.iter_mut().enumerate() {
        *slot = first + i as u8;
    }
    Some(idx)
}

/// Expected DEFAULT container table sizes `(vs_count, ps_count)` for a
/// configuration (validation aid).
pub fn default_table_counts(persp: bool, themes: bool) -> (u8, u8) {
    (
        1 + persp as u8 + themes as u8,
        1 + if themes { THEME_PROGRAM_COUNT } else { 0 },
    )
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn planned_matrix() {
        // (aa, persp, themes) → (arrow, judge, default); lit is orthogonal.
        let cases = [
            ((false, false, false), (false, false, false)),
            ((true, false, false), (true, true, false)),
            ((false, true, false), (true, true, true)),
            ((true, true, false), (true, true, true)),
            ((false, false, true), (false, false, true)), // themes-only
            ((true, false, true), (true, true, true)),
            ((false, true, true), (true, true, true)),
            ((true, true, true), (true, true, true)),
        ];
        for lit in [false, true] {
            for ((aa, persp, themes), (arrow, judge, default)) in cases {
                assert_eq!(
                    planned(aa, persp, themes, lit),
                    PlannedContainers {
                        arrow,
                        judge,
                        default,
                        model_variants: lit,
                    },
                    "aa={aa} persp={persp} themes={themes} lit={lit}"
                );
            }
        }
    }

    #[test]
    fn variants_only_plan_just_the_model_containers() {
        // The model variants never drag the screencommand containers in
        // (and vice versa): a dancers-only cabinet with AA/persp/menu off
        // overlays exactly the variant containers.
        let p = planned(false, false, false, true);
        assert_eq!(
            p,
            PlannedContainers {
                arrow: false,
                judge: false,
                default: false,
                model_variants: true,
            }
        );
    }

    #[test]
    fn variant_table_is_consistent() {
        // Distinct material names; the two lambert entries map the FALLBACK
        // stock names (no mdl_*_lambert container exists to hash).
        let mut names: Vec<&str> = MODEL_VARIANTS.iter().map(|v| v.material_name).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), MODEL_VARIANTS.len());
        let bg = MODEL_VARIANTS
            .iter()
            .find(|v| v.material_name == "mdl_bg_lambert")
            .unwrap();
        assert_eq!(bg.stock_name, "gs_model_default");
        let ch = MODEL_VARIANTS
            .iter()
            .find(|v| v.material_name == "mdl_ch_lambert")
            .unwrap();
        assert_eq!(ch.stock_name, "gs_model_skinning_default");
        // Every other entry is its own stock name.
        for v in MODEL_VARIANTS.iter() {
            if !v.material_name.ends_with("_lambert") {
                assert_eq!(v.stock_name, v.material_name);
            }
            // Skinned shapes use skinned blobs and vice versa.
            let skinned = v.material_name.starts_with("mdl_ch_");
            assert_eq!(
                v.lit_vs.starts_with("mdl_ch_"),
                skinned,
                "{}",
                v.material_name
            );
            assert_eq!(
                v.cel_vs.starts_with("mdl_ch_"),
                skinned,
                "{}",
                v.material_name
            );
            assert_eq!(
                v.outline_vs.starts_with("mdl_ch_"),
                skinned,
                "{}",
                v.material_name
            );
            assert_eq!(
                v.uni_vs.starts_with("mdl_ch_"),
                skinned,
                "{}",
                v.material_name
            );
            assert_eq!(
                v.uni_vs,
                v.cel_vs.replace("_cel", "_uni"),
                "{}",
                v.material_name
            );
            // `_vc` names take VCOLOR blobs; `_c` names take the `_c` cel PS;
            // `_notex` the notex PS pair.
            let vc = v.material_name.contains("_vc");
            assert_eq!(v.cel_vs.contains("_vc"), vc, "{}", v.material_name);
            let c = v.material_name.contains("_c_") || v.material_name.ends_with("_c");
            assert_eq!(v.cel_ps == "mdl_cel_c.ps.d3dbc", c, "{}", v.material_name);
            let notex = v.material_name.ends_with("_notex");
            assert_eq!(
                v.cel_ps == "mdl_cel_notex.ps.d3dbc",
                notex,
                "{}",
                v.material_name
            );
            assert_eq!(
                v.outline_ps == "mdl_outline_notex.ps.d3dbc",
                notex,
                "{}",
                v.material_name
            );
            assert_eq!(v.lit_vs.contains("notex"), notex, "{}", v.material_name);
        }
        // Stock-hash reverse lookup: each stock name resolves to its entry;
        // the shadow's `mdl_bg_constant` IS an entry (the DLL excludes the
        // shadow by instance kind, not by name).
        for v in MODEL_VARIANTS.iter() {
            assert_eq!(
                variant_for_stock_hash(fnv1_32(v.stock_name)).map(|x| x.material_name),
                Some(v.material_name)
            );
        }
        assert!(variant_for_stock_hash(fnv1_32("mdl_ch_constant")).is_none());
        assert_eq!(
            variant_container_name(&MODEL_VARIANTS[1], SceneStyle::Cel).as_deref(),
            Some("mdl_ch_lambert_cel")
        );
        assert_eq!(
            variant_container_name(&MODEL_VARIANTS[1], SceneStyle::Stock),
            None
        );
        // 18 lit / cel / outline blobs + 4 Universe VS + 23 Universe PS.
        assert_eq!(variant_blob_names().len(), 45);
    }

    #[test]
    fn universe_styles_are_dancer_only_with_their_own_families() {
        for s in SceneStyle::ALL {
            match s {
                SceneStyle::Stock => {
                    assert_eq!(s.family(), None);
                    assert_eq!(s.stage_style(), SceneStyle::Stock);
                }
                SceneStyle::Random => {
                    // Resolved per bar by the session: no look of its own.
                    assert_eq!(s.family(), None);
                    assert_eq!(s.texture(), None);
                    assert!(!s.additive() && !s.outlines() && s.needs_hulls());
                    assert_eq!(s.stage_style(), SceneStyle::Cel);
                }
                SceneStyle::Lit | SceneStyle::Cel => {
                    assert!(!s.is_universe());
                    assert_eq!(s.stage_style(), s);
                    assert!(s.outlines());
                }
                _ => {
                    assert!(s.is_universe(), "{:?}", s);
                    assert!(matches!(s.family(), Some(StyleFamily::Uni(_))), "{:?}", s);
                    assert_eq!(s.stage_style(), SceneStyle::Cel);
                }
            }
            // Additive styles always sample a sphere map; every texture
            // style draws one of the two texture-reading families.
            if s.additive() {
                assert_eq!(s.family(), Some(StyleFamily::Uni(UniFamily::Sphere)));
                assert!(!s.outlines());
            }
            match s.texture() {
                Some(StyleTexture::Rainbow) => {
                    assert_eq!(s.family(), Some(StyleFamily::Uni(UniFamily::ScreenTex)))
                }
                Some(_) => assert_eq!(s.family(), Some(StyleFamily::Uni(UniFamily::Sphere))),
                None => assert!(!matches!(
                    s.family(),
                    Some(StyleFamily::Uni(UniFamily::Sphere | UniFamily::ScreenTex))
                )),
            }
        }
        // Universe draws its outline pass only with NORMAL / NEGATIVE / MONO / SEPIA.
        let outlined: Vec<SceneStyle> = SceneStyle::ALL
            .iter()
            .copied()
            .filter(|s| s.is_universe() && s.outlines())
            .collect();
        assert_eq!(
            outlined,
            vec![SceneStyle::Negative, SceneStyle::Mono, SceneStyle::Sepia]
        );
        let additive: Vec<SceneStyle> = SceneStyle::ALL
            .iter()
            .copied()
            .filter(|s| s.additive())
            .collect();
        assert_eq!(
            additive,
            vec![
                SceneStyle::Candy,
                SceneStyle::Bubble,
                SceneStyle::Crystal,
                SceneStyle::Xray
            ]
        );
        // OUTLINE ONLY is the hull alone: no outline switch, no texture, not additive.
        assert_eq!(
            SceneStyle::Outline.family(),
            Some(StyleFamily::Uni(UniFamily::Invisible))
        );
        assert!(SceneStyle::Outline.needs_hulls() && !SceneStyle::Outline.outlines());
        assert!(SceneStyle::ALL
            .iter()
            .filter(|s| s.needs_hulls())
            .all(|s| matches!(s, SceneStyle::Outline | SceneStyle::Random)));
        // RANDOM's pool: every preset but STOCK / RANDOM, gated by what is served.
        let full = SceneStyle::random_pool(true, true, true);
        assert_eq!(full.len(), SceneStyle::ALL.len() - 2);
        assert!(!full.contains(&SceneStyle::Stock) && !full.contains(&SceneStyle::Random));
        assert_eq!(
            SceneStyle::random_pool(false, true, true),
            vec![SceneStyle::Lit, SceneStyle::Cel]
        );
        let no_tex = SceneStyle::random_pool(true, false, true);
        assert!(no_tex.iter().all(|s| s.texture().is_none()));
        assert!(no_tex.contains(&SceneStyle::Stencil) && no_tex.contains(&SceneStyle::Outline));
        assert!(!SceneStyle::random_pool(true, true, false).contains(&SceneStyle::Outline));
        // Style-texture stems are registry-key-safe (lower-case, no `_`).
        for t in [
            StyleTexture::Rainbow,
            StyleTexture::Thermo,
            StyleTexture::Chrome,
            StyleTexture::Crystal,
            StyleTexture::Xray,
        ] {
            let stem = t.stem();
            assert!(
                stem.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()),
                "{}",
                stem
            );
        }
        assert_eq!(STYLE_TEXTURE_ARC, "data/arc/bdfxuniverse.arc");
    }

    #[test]
    fn family_containers_and_blobs() {
        // 2 + 9 families, distinct suffixes.
        let fams = StyleFamily::all();
        assert_eq!(fams.len(), 12);
        let mut sfx: Vec<String> = fams.iter().map(|f| f.suffix()).collect();
        sfx.sort();
        sfx.dedup();
        assert_eq!(sfx.len(), 12);
        assert_eq!(
            StyleFamily::served(false),
            vec![StyleFamily::Lit, StyleFamily::Cel]
        );
        assert_eq!(StyleFamily::served(true), fams);
        // Exactly the 27 mdl_universe blobs classify as Universe blobs.
        let uni: Vec<&str> = variant_blob_names()
            .into_iter()
            .filter(|b| is_universe_blob(b))
            .collect();
        assert_eq!(uni.len(), 27);
        assert!(uni.iter().all(|b| b.contains("_uni")));
        assert!(!is_universe_blob("mdl_ch_unlit_uv3_vc_wipe.vs.d3dbc"));
        assert!(!is_universe_blob("mdl_cel.ps.d3dbc"));
        assert_eq!(StyleFamily::Uni(UniFamily::Sphere).suffix(), "_usph");
        // The six sphere styles share one container per shape.
        let ch = &MODEL_VARIANTS[1];
        for s in [
            SceneStyle::Thermo,
            SceneStyle::Mercury,
            SceneStyle::Candy,
            SceneStyle::Bubble,
            SceneStyle::Crystal,
            SceneStyle::Xray,
        ] {
            assert_eq!(
                variant_container_name(ch, s).as_deref(),
                Some("mdl_ch_lambert_usph")
            );
        }
        assert_eq!(
            variant_container_name(ch, SceneStyle::Lit).as_deref(),
            Some("mdl_ch_lambert_lit")
        );
        // PS selection: `_notex` shape → notex PS, `_c` shapes → `_c` PS only
        // for the material-colour families.
        let notex = MODEL_VARIANTS
            .iter()
            .find(|v| v.material_name == "mdl_ch_constant_vc_notex")
            .unwrap();
        let c = MODEL_VARIANTS
            .iter()
            .find(|v| v.material_name == "mdl_ch_constant_c_vc")
            .unwrap();
        for f in UniFamily::ALL {
            assert!(notex.uni_ps(f).ends_with("_notex.ps.d3dbc"), "{:?}", f);
            assert!(notex.uni_ps(f).contains(f.tag()));
            assert_eq!(
                c.uni_ps(f).ends_with("_c.ps.d3dbc"),
                f.uses_material_colour()
            );
            assert_eq!(ch.uni_ps(f), format!("mdl_uni_{}.ps.d3dbc", f.tag()));
        }
        assert!(notex.is_notex() && !notex.is_const_colour());
        assert!(c.is_const_colour() && !ch.is_const_colour());
        // Universe styles take the CEL wipe.
        let w = &WIPE_VARIANTS[0];
        assert_eq!(
            wipe_container_name(w, SceneStyle::Thermo),
            "mdl_ch_constant_vc_cel_wipe"
        );
        assert_eq!(wipe_blobs(w, SceneStyle::Xray), (w.cel_vs, w.cel_ps));
    }

    #[test]
    fn model_programs_are_four_entries_with_an_inert_hull_slot() {
        assert_eq!(
            MODEL_PROGRAM_ENTRIES, 4,
            "the pass reads programs[2] unchecked"
        );
        // No outline pair: the stock shape, four identical entries.
        let p = model_programs(false);
        assert_eq!(p.len(), 4);
        assert!(p.iter().all(|&e| e == (0, 0, 0)));
        assert_eq!(model_table_counts(false), (1, 1));
        // Outline pair: ONLY the hull index changes; 1..3 (incl. the
        // production stage 2 and the DAT_1806f1548==0 stage 3) stay the style.
        let p = model_programs(true);
        assert_eq!(p.len(), 4);
        assert_eq!(p[MODEL_HULL_PROGRAM_INDEX as usize], (0, 1, 1));
        for i in 1..4 {
            assert_eq!(p[i], (0, 0, 0), "program {i} must be the style pair");
        }
        assert_eq!(MODEL_HULL_PROGRAM_INDEX, 0, "bit-31 records bind program 0");
        let (vs, ps) = model_table_counts(true);
        assert_eq!((vs, ps), (2, 2));
        for &(_, vsi, psi) in &p {
            assert!(vsi < vs && psi < ps);
        }
    }

    #[test]
    fn scene_style_round_trips() {
        for (i, s) in SceneStyle::ALL.iter().enumerate() {
            assert_eq!(SceneStyle::parse(s.key()), Some(*s));
            if s.is_universe() {
                // Universe's own CSV spelling is the row label.
                assert_eq!(SceneStyle::parse(s.label()), Some(*s), "{}", s.label());
            }
            assert_eq!(s.row_value(), i as i32);
            assert_eq!(SceneStyle::from_row_value(s.row_value()), *s);
        }
        assert_eq!(SceneStyle::parse("T1000"), Some(SceneStyle::Mercury));
        assert_eq!(SceneStyle::parse("STONE"), Some(SceneStyle::Bubble));
        assert_eq!(SceneStyle::parse("Alpha Light"), Some(SceneStyle::Xray));
        assert_eq!(SceneStyle::parse("blackgloss"), Some(SceneStyle::Negative));
        assert_eq!(SceneStyle::parse("FILL IN"), Some(SceneStyle::Stencil));
        assert_eq!(
            SceneStyle::parse("stencil-rainbow"),
            Some(SceneStyle::StencilRainbow)
        );
        assert_eq!(SceneStyle::from_row_value(-1), SceneStyle::Stock);
        // RANDOM is the first row; the presets follow.
        assert_eq!(SceneStyle::ALL[0], SceneStyle::Random);
        assert_eq!(
            SceneStyle::parse("RANDOM (EVERY BAR)"),
            Some(SceneStyle::Random)
        );
        assert_eq!(SceneStyle::parse("pencil"), Some(SceneStyle::Outline));
        assert_eq!(SceneStyle::parse(" CEL "), Some(SceneStyle::Cel));
        assert_eq!(SceneStyle::parse("toon"), Some(SceneStyle::Cel));
        assert_eq!(SceneStyle::parse("enhanced"), Some(SceneStyle::Lit));
        assert_eq!(SceneStyle::parse("off"), Some(SceneStyle::Stock));
        assert_eq!(SceneStyle::parse("bogus"), None);
        assert_eq!(SceneStyle::from_row_value(99), SceneStyle::Stock);
    }

    #[test]
    fn fnv1_matches_the_stock_container_headers() {
        // Hashes read back from the World 20260915 shader.arc headers
        // (`gsp_pack.py inspect --expect-name`) — the engine's hasher and
        // gsp_pack.py's fnv1_32 agree on these; so must we.
        assert_eq!(fnv1_32("gs_screencommand_arrow"), 0x9E93_AC7B);
        assert_eq!(fnv1_32("gs_model_default"), 0x6CD7_F817);
        assert_eq!(fnv1_32("gs_model_skinning_default"), 0x55A0_AC03);
        assert_eq!(fnv1_32("mdl_bg_constant"), 0xBDFE_3C7B);
        // The two synthesized names (no stock header exists — these are
        // what gsp_pack.py pack --name computes for them).
        assert_eq!(fnv1_32("mdl_bg_lambert"), 0xB925_F2E2);
        assert_eq!(fnv1_32("mdl_ch_lambert"), 0x1C25_C5BE);
        // Sanity: FNV-1 (multiply-then-xor), not FNV-1a.
        assert_eq!(fnv1_32(""), 0x811C_9DC5);
        assert_eq!(
            fnv1_32("a"),
            0x811C_9DC5u32.wrapping_mul(0x0100_0193) ^ b'a' as u32
        );
    }

    #[test]
    fn default_programs_matrix() {
        let n = THEME_PROGRAM_COUNT;
        assert!(default_programs(false, false).is_empty());
        assert_eq!(default_programs(true, false), vec![(0, 0, 0), (0, 1, 0)]);
        // Themes-only: stock + one program per theme (theme VS at 1).
        let mut want: Vec<(u8, u8, u8)> = vec![(0, 0, 0)];
        want.extend((1..=n).map(|ps| (0, 1, ps)));
        assert_eq!(default_programs(false, true), want);
        // Persp + themes: stock, persp at 1, themes after (theme VS at 2).
        let mut want: Vec<(u8, u8, u8)> = vec![(0, 0, 0), (0, 1, 0)];
        want.extend((1..=n).map(|ps| (0, 2, ps)));
        assert_eq!(default_programs(true, true), want);
    }

    #[test]
    fn persp_is_always_program_one() {
        for themes in [false, true] {
            let programs = default_programs(true, themes);
            assert_eq!(
                programs[PERSP_PROGRAM_INDEX as usize],
                (0, 1, 0),
                "perspective must be program {PERSP_PROGRAM_INDEX} (themes={themes})"
            );
        }
    }

    #[test]
    fn theme_indices_are_the_last_entries() {
        let n = THEME_PROGRAM_COUNT as usize;
        assert_eq!(default_theme_indices(false, false), None);
        assert_eq!(default_theme_indices(true, false), None);
        // First theme program directly after stock (no persp) / after
        // persp; the rest consecutive.
        assert_eq!(default_theme_indices(false, true).unwrap()[0], 1);
        assert_eq!(default_theme_indices(true, true).unwrap()[0], 2);
        // Cross-check against the program table: the reported indices are
        // exactly the final THEME_PROGRAM_COUNT entries, consecutively.
        for persp in [false, true] {
            let programs = default_programs(persp, true);
            let idx = default_theme_indices(persp, true).unwrap();
            assert_eq!(idx[n - 1] as usize, programs.len() - 1);
            assert_eq!(idx[0] as usize, programs.len() - n);
            for w in idx.windows(2) {
                assert_eq!(w[1], w[0] + 1);
            }
        }
    }

    #[test]
    fn wipe_variants_cover_the_dancer_shader() {
        let v = wipe_variant_for_stock_hash(fnv1_32("mdl_ch_constant_vc")).unwrap();
        assert_eq!(
            wipe_container_name(v, SceneStyle::Stock),
            "mdl_ch_constant_vc_wipe"
        );
        assert_eq!(
            wipe_container_name(v, SceneStyle::Lit),
            "mdl_ch_constant_vc_lit_wipe"
        );
        assert_eq!(
            wipe_container_name(v, SceneStyle::Cel),
            "mdl_ch_constant_vc_cel_wipe"
        );
        assert!(wipe_variant_for_stock_hash(fnv1_32("mdl_bg_constant")).is_none());
        assert_eq!(wipe_blobs(v, SceneStyle::Cel), (v.cel_vs, v.cel_ps));
        assert_eq!(
            wipe_blobs(v, SceneStyle::Stock).1,
            wipe_blobs(v, SceneStyle::Lit).1
        );
        let names = wipe_blob_names();
        assert_eq!(names.len(), 5);
        // never a name the style variants already carry
        for n in &names {
            assert!(!variant_blob_names().contains(n), "{n}");
        }
    }

    #[test]
    fn table_counts_match_programs() {
        for persp in [false, true] {
            for themes in [false, true] {
                let (vs, ps) = default_table_counts(persp, themes);
                for &(_, vsi, psi) in &default_programs(persp, themes) {
                    assert!(vsi < vs, "vs idx in range (persp={persp} themes={themes})");
                    assert!(psi < ps, "ps idx in range (persp={persp} themes={themes})");
                }
            }
        }
    }
}
