//! DDR SELECTION era-sound cue manifest (pure, host-tested).
//!
//! Every XACT cue a legacy skin plays, by where it comes from. All of them live
//! in the A3-generation banks World ships but never loads
//! (`data/sound/win/voice_n.xwb`, `data/arc/soundbanks_n.arc` →
//! `voice_n.xsb` / `se_normal_n.xsb`, `data/arc/se_normal_n.arc` →
//! `se_normal_n.xwb`); the builder copies exactly these into the mod-owned
//! `dsel` bank. Source: `.agents/planning/2026-09-22-ddr-selection/research/`
//! `afp-sound-routing.md` §3 (AFP inventory) and `sounds-options-folder.md` §A
//! (A3 code sites).
//!
//! Dependency-free (mounted by `scripts/validate_ddr_selection.sh`).

/// Cues the legacy AFP clips play themselves (`asdlib.sound_play`). World's
/// AFP sound callback only ever asks slot 2 (`se_normal`) or slot 3 (`voice`),
/// neither of which holds them, so they are routed to the `dsel` bank.
pub const AFP_EMBEDDED: &[&str] = &[
    // dance_fullcombo000N
    "XAC_full_combo2",
    // dance_game_over000N; common_choice_v2
    "Plate_spin3_st",
    // common_shutter0004/5; common_choice0004/5
    "Plate_spin4_st",
    // dance_message000N `00_ready`; common_shutter000N banners
    "2nd_BIG2",
    "ACT2_1",
    "2nd_KANSEI_B",
    "sn2_mst09",
    "vo_ingame_ready",
    "ACT9",
    "STG_APP02",
    "STG_APP03",
    "STG_CLOSE01",
    "se_shutter_in",
    "se_shutter_out",
    "vo_stage_clear",
    "ext_failed",
    "sn2_gov",
    "end_door",
    "banner_in",
    // common_choice000N / common_choice_v2 stage panel
    "ACE_shutter_choice_exc",
    "ACE3_shutter_choice_savior",
    "ACE_TEPPAN",
];

/// Cues A3's code played for the legacy skins (announcer, crowd, HERE WE GO
/// voice, stage-choice cut-in and stage call). Played by DDR SELECTION's own
/// code into the `dsel` bank.
pub const CODE_PLAYED: &[&str] = &[
    // CallVoiceActor, skins 1–3
    "ACT6",
    "sn2_dgm25",
    "sn2_dgm26",
    "sn2_dgm27",
    "sn2_dgm28",
    "sn2_dgm29",
    "sn2_dgm30",
    "sn2_dgm31",
    "sn2_dgm32",
    "sn2_dgm33",
    "sn2_dgm34",
    "sn2_dgm_high",
    "sn2_dgm_middle",
    // crowd SEs (STG_APP02/03, 2nd_BIG2, 2nd_KANSEI_B are listed above)
    "STG_BOO",
    // A3's own announcer (skins 4–5 heard A3's current voice set)
    "vo_ingame_combo_100",
    "vo_ingame_combo_200",
    "vo_ingame_combo_300",
    "vo_ingame_combo_400",
    "vo_ingame_combo_500",
    "vo_ingame_combo_600",
    "vo_ingame_combo_700",
    "vo_ingame_combo_800",
    "vo_ingame_combo_900",
    "vo_ingame_combo_1000",
    "vo_ingame_combo_over",
    "vo_ingame_combo_gen",
    "vo_ingame_high",
    "vo_ingame_gen",
    "vo_ingame_regain",
    "vo_ingame_low_hard",
    "vo_ingame_low_easy",
    "vo_ingame_cheer",
    "vo_ingame_boo",
    // ReadyGoActor: skin 1 HERE WE GO voice (final stage: ACT4_2)
    "ACT3_1",
    "ACT4_2",
    // stage-choice cut-in SE per skin
    "sele_1st",
    "sele_ext",
    "sele_sn2",
    "sele_x2",
    "sele_2013",
    // stage call, skins 2–3
    "sn2_etca2",
    "sn2_etca3",
    "sn2_etca4",
    "sn2_etca5",
    "sn2_etca7",
    "sn2_etc73",
    // stage call, skins 4–5 (A3 takes)
    "vo_stage_01",
    "vo_stage_02",
    "vo_stage_03",
    "vo_stage_04",
    "vo_stage_final",
    "vo_stage_extra",
];

/// Every cue the `dsel` bank should carry, deduplicated, manifest order.
pub fn all() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::with_capacity(AFP_EMBEDDED.len() + CODE_PLAYED.len());
    for &c in AFP_EMBEDDED.iter().chain(CODE_PLAYED.iter()) {
        if !out.contains(&c) {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_is_unique_and_ascii() {
        let all = all();
        assert_eq!(all.len(), AFP_EMBEDDED.len() + CODE_PLAYED.len());
        for c in &all {
            assert!(!c.is_empty() && c.is_ascii() && c.len() < 64, "{c}");
        }
    }

    #[test]
    fn afp_set_matches_the_inventory() {
        assert_eq!(AFP_EMBEDDED.len(), 22);
        assert!(AFP_EMBEDDED.contains(&"XAC_full_combo2"));
        assert!(AFP_EMBEDDED.contains(&"Plate_spin3_st"));
    }
}
