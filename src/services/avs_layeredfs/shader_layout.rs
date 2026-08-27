//! Pure shader-container layout decisions for the runtime synthesis
//! (`shader_synthesis.rs`) — which containers a configuration overlays
//! and how the DEFAULT container's program table is laid out once the
//! mod-menu theme programs participate.
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
//!   that carries them.

/// The player-perspective program's index in every container that has
/// one — a positional contract with `pass_rewrite` (never move it).
pub const PERSP_PROGRAM_INDEX: u8 = 1;

/// How many shader-backed menu themes ride the DEFAULT container. Must
/// stay in lockstep with `ThemeProgram` (`mod_menu::theme`) and
/// `THEME_BLOBS` (`shader_synthesis`).
pub const THEME_PROGRAM_COUNT: u8 = 11;

/// Which containers a configuration synthesizes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlannedContainers {
    pub arrow: bool,
    pub judge: bool,
    pub default: bool,
}

/// Container set for a configuration (the minimal-overlay rule): arrow +
/// judge when AA or perspective is on; default when perspective needs
/// its VS there OR the theme programs ride it.
pub fn planned(aa: bool, persp: bool, themes: bool) -> PlannedContainers {
    PlannedContainers {
        arrow: aa || persp,
        judge: aa || persp,
        default: persp || themes,
    }
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
        // (aa, persp, themes) → (arrow, judge, default)
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
        for ((aa, persp, themes), (arrow, judge, default)) in cases {
            assert_eq!(
                planned(aa, persp, themes),
                PlannedContainers {
                    arrow,
                    judge,
                    default
                },
                "aa={aa} persp={persp} themes={themes}"
            );
        }
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
