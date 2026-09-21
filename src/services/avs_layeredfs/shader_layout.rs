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
//!   stock `gs_model_*` / `mdl_*` container.

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
/// `Stock` re-points nothing (the engine's own shaders — unlit). `Lit` and
/// `Cel` share ONE fixed world-space key light; `Lit` applies it smoothly per
/// vertex, `Cel` quantizes the same N·L into bands per pixel and adds rim ink
/// (+ the hull outlines when enabled) — there is no "stock light + cel".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneStyle {
    Stock,
    Lit,
    Cel,
}

/// Historical alias (the first cabinet builds called it the dancers' style).
pub type DancerStyle = SceneStyle;

impl SceneStyle {
    /// Config spelling (`"stock"` / `"lit"` / `"cel"`, case-insensitive).
    /// Unknown ⇒ `None` (the caller WARNs and falls back).
    pub fn parse(s: &str) -> Option<SceneStyle> {
        match s.trim().to_ascii_lowercase().as_str() {
            "stock" | "off" | "none" => Some(SceneStyle::Stock),
            "lit" | "lambert" | "enhanced" => Some(SceneStyle::Lit),
            "cel" | "toon" | "enhanced_cel" => Some(SceneStyle::Cel),
            _ => None,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            SceneStyle::Stock => "stock",
            SceneStyle::Lit => "lit",
            SceneStyle::Cel => "cel",
        }
    }

    /// Overlay-row value (row index) — STOCK / LIT / CEL.
    pub fn row_value(self) -> i32 {
        match self {
            SceneStyle::Stock => 0,
            SceneStyle::Lit => 1,
            SceneStyle::Cel => 2,
        }
    }

    pub fn from_row_value(v: i32) -> SceneStyle {
        match v {
            1 => SceneStyle::Lit,
            2 => SceneStyle::Cel,
            _ => SceneStyle::Stock,
        }
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
        outline_vs: "mdl_ch_outline.vs.d3dbc",
        outline_ps: "mdl_outline_notex.ps.d3dbc",
    },
];

/// Suffix of a style-variant container name: `<material_name><suffix>`.
pub fn variant_suffix(style: DancerStyle) -> Option<&'static str> {
    match style {
        DancerStyle::Stock => None,
        DancerStyle::Lit => Some("_lit"),
        DancerStyle::Cel => Some("_cel"),
    }
}

/// The synthesized container name for `variant` in `style` (`None` for stock).
pub fn variant_container_name(variant: &ModelVariant, style: DancerStyle) -> Option<String> {
    variant_suffix(style).map(|sfx| format!("{}{}", variant.material_name, sfx))
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
        for b in [m.lit_vs, m.cel_vs, m.cel_ps, m.outline_vs, m.outline_ps] {
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
    /// Every `MODEL_VARIANTS` × {lit, cel} container.
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
        // 18 committed blob files are referenced.
        assert_eq!(variant_blob_names().len(), 18);
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
        for s in [SceneStyle::Stock, SceneStyle::Lit, SceneStyle::Cel] {
            assert_eq!(SceneStyle::parse(s.key()), Some(s));
            assert_eq!(SceneStyle::from_row_value(s.row_value()), s);
        }
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
