//! DDR SELECTION legacy stage panel — the A3 rules and the per-panel state
//! machine (pure, host-tested).
//!
//! Dependency-free on purpose: `scripts/validate_ddr_selection.sh` mounts this
//! file into a throwaway host crate.
//!
//! A3 (`gamemdx_20240402`) showed a code-built composite between song select
//! and the lanes: the root `shutter_choice_hd_root` of `common_choice` (v2)
//! with the era's `choice_stage` (from `common_choice000N`),
//! `choice_background` and — skins 3–5 — `choice_jacket` (from
//! `common_shutter000N`) loaded into its placeholders, preceded by the era
//! cut-in (`common_choice_cutin000N`, SE `sele_*`) and followed by a per-skin
//! stage call. World's ShutterActor kind 3 hosts it (`panel.rs`); this file
//! holds everything that does not touch the engine. RE record:
//! `.agents/planning/2026-09-22-ddr-selection/research/stage-panel.md`.

/// The package that carries the eras' panel root (requested by FULL name: the
/// bare `common_choice` resolves `_v0`, the DDR A generation) — see
/// [`root_package_cstr`] for the themes'.
pub const ROOT_PACKAGE: &str = "common_choice_v2";
/// A3's HD root clip (machine types ≥ 2; World scales it on SD cabinets).
pub const ROOT_CLIP: &str = "shutter_choice_hd_root";
/// A3's cut-in background package (textures only).
pub const CUTIN_BG_PACKAGE: &str = "common_choice_cutinbg";
/// A3: the cut-in may be skipped once it is past this frame (and before `out`).
pub const CUTIN_SKIP_MIN_FRAME: u32 = 0x3B;
/// A3's display priority of the stage-choice kind (other kinds: 3) and of the
/// cut-in, both in group 5.
pub const ROOT_PRIORITY: u16 = 6;
pub const CUTIN_PRIORITY: u16 = 7;
/// Frames the drain may sit in state 8 before the stall fallback
/// (`shutter::unblock_drain`). The legacy `out` → `end` run is 31 frames.
pub const DRAIN_STALL_FRAMES: u32 = 180;

/// The raw `afp_layer_set_priority` value of a BM2D `CLayer::SetPriority(p)`
/// (A3 `0x1801b8790`, World 20260825 `0x180259710` — identical code):
/// `p <= 100 ? 100 - p : p`. The display draws ascending raw priorities, so a
/// HIGHER `CLayer` priority draws EARLIER (further back): World's shutter
/// kinds (`SetPriority(3)`) sit on top at 97, A3's ReadyGo clips (5) at 95,
/// the stage-choice root (6) at 94, the cut-in (7) at 93.
pub fn clayer_priority(p: u16) -> u16 {
    if p <= 100 {
        100 - p
    } else {
        p
    }
}

/// The four A3 packages a skin's panel needs (the root package is World's).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Packages {
    pub choice: String,
    pub shutter: String,
    pub cutin: String,
    pub cutin_bg: &'static str,
}

/// A3 `FUN_1800328c0`: `common_choice%04d`, `common_shutter%04d`,
/// `common_choice_cutin%04d` + `common_choice_cutinbg`. `None` outside the
/// eras (1..=5; a theme's panel loads nothing into its root).
pub fn packages(skin: u8) -> Option<Packages> {
    if !(1..=5).contains(&skin) {
        return None;
    }
    Some(Packages {
        choice: format!("common_choice{:04}", skin),
        shutter: format!("common_shutter{:04}", skin),
        cutin: format!("common_choice_cutin{:04}", skin),
        cutin_bg: CUTIN_BG_PACKAGE,
    })
}

/// The session state the stage rules read (World `GameWork`: stage `+0xC`,
/// final-stage override `+0x10`, course `+0x70`, event mode `+0xD0`; the
/// operator's max stage). All 0-based except `override_stage`.
#[derive(Clone, Copy, Debug, Default)]
pub struct StageCtx {
    pub stage: i32,
    pub max_stage: i32,
    pub override_stage: i32,
    pub course: bool,
    pub event_mode: i32,
}

fn event_chain(c: &StageCtx) -> bool {
    c.event_mode == 1 || c.event_mode == 2
}

/// A3 `FUN_180123af0` / World's `vo_stage_extra` rule: the stage after the
/// operator's normal count, not named by the final-stage override.
pub fn is_extra_stage(c: &StageCtx) -> bool {
    !c.course && !event_chain(c) && c.stage != c.override_stage && c.stage == c.max_stage + 1
}

/// A3 `FUN_180123a20` (non-course): see `intro_logic::is_final_stage`; event
/// chains are never final.
pub fn is_final_stage(c: &StageCtx) -> bool {
    if event_chain(c) {
        return false;
    }
    let n = c.stage + 1;
    n == c.override_stage || (n != c.override_stage && n == c.max_stage + 1)
}

/// A3's legacy stage band texture (`scene_choice_stage%04d_{extra, final,
/// 2nd, 1st}` — the legacy sheets have only those four; stages 3–4 show
/// `1st`, as in A3).
pub fn stage_texture(skin: u8, c: &StageCtx) -> String {
    let suffix = if is_extra_stage(c) {
        "extra"
    } else if is_final_stage(c) {
        "final"
    } else if c.stage == 1 {
        "2nd"
    } else {
        "1st"
    };
    format!("scene_choice_stage{:04}_{}", skin, suffix)
}

/// Which of A3's two panels a skin shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    /// The eras: A3's legacy fill (era `choice_stage` / `choice_background`
    /// / `choice_jacket` loaded into `common_choice_v2`'s root, the era
    /// cut-in).
    Era,
    /// The themes: A3's own skin-0 fill on the theme generation's root
    /// (`FUN_180030d10`'s skin-0 branch) — no packages, no cut-in.
    Theme,
}

pub fn variant(skin: u8) -> Option<Variant> {
    if super::policy::is_era(skin) {
        Some(Variant::Era)
    } else if super::policy::is_theme(skin) {
        Some(Variant::Theme)
    } else {
        None
    }
}

/// The package holding the root clip, NUL-terminated (the row patch hands
/// the pointer to World's kind-art loader — static): `common_choice_v2` for
/// the eras (A3's white-cabinet root) and the theme generation's own
/// `common_choice_vN` for a theme.
pub fn root_package_cstr(skin: u8) -> Option<&'static str> {
    match (variant(skin)?, super::policy::theme(skin).map(|t| t.suffix)) {
        (Variant::Era, _) => Some("common_choice_v2\0"),
        (Variant::Theme, Some("_v0")) => Some("common_choice_v0\0"),
        (Variant::Theme, Some("_v1")) => Some("common_choice_v1\0"),
        (Variant::Theme, Some("_v2")) => Some("common_choice_v2\0"),
        (Variant::Theme, _) => None,
    }
}

/// [`root_package_cstr`] without the NUL.
pub fn root_package(skin: u8) -> Option<&'static str> {
    root_package_cstr(skin).map(|s| s.trim_end_matches('\0'))
}

/// A3's event-only special stages (outside courses and event chains): the
/// stage the final-stage override names by index, or past the extra stage.
/// A3 drew event art there (a loader-owned package World does not have);
/// the theme panel shows EXTRA. Exactly the stages the stage call skips.
pub fn special_stage(c: &StageCtx) -> bool {
    !c.course && !event_chain(c) && (c.stage == c.override_stage || c.stage > c.max_stage + 1)
}

/// A3's skin-0 stage band (`scene_choice_stage_*` of the theme's own
/// `common_choice_vN`, written into the root's own `choice_stage_usr`):
/// extra (or special) → `extra`, final → `final`, else stage index 3 / 2 / 1
/// → `4th` / `3rd` / `2nd`, else `1st`.
pub fn theme_stage_texture(c: &StageCtx) -> String {
    let suffix = if special_stage(c) || is_extra_stage(c) {
        "extra"
    } else if is_final_stage(c) {
        "final"
    } else {
        match c.stage {
            3 => "4th",
            2 => "3rd",
            1 => "2nd",
            _ => "1st",
        }
    };
    format!("scene_choice_stage_{}", suffix)
}

/// A3's stage call (`FUN_18002e210` / `FUN_18002e060`): skin 1 silent;
/// skins 2–3 `sn2_etc{73 extra, a7 final, a<stage+2> for stages 0..=3}`;
/// skins 4–5 and the themes (A3's own skin 0) A3's `vo_stage_{extra, final,
/// NN}`. Nothing when the stage is the override's own index or past the
/// normal count (outside courses and event chains).
pub fn stage_voice(skin: u8, c: &StageCtx) -> Option<String> {
    if !(2..=super::policy::SKIN_MAX).contains(&skin) {
        return None;
    }
    let ev = event_chain(c);
    let allowed =
        c.course || ((ev || c.stage != c.override_stage) && (ev || c.stage <= c.max_stage + 1));
    if !allowed {
        return None;
    }
    let extra = is_extra_stage(c);
    let fin = is_final_stage(c);
    if skin <= 3 {
        if extra {
            Some("sn2_etc73".into())
        } else if fin {
            Some("sn2_etca7".into())
        } else if (0..=3).contains(&c.stage) {
            Some(format!("sn2_etca{}", c.stage + 2))
        } else {
            None
        }
    } else if extra {
        Some("vo_stage_extra".into())
    } else if fin {
        Some("vo_stage_final".into())
    } else {
        Some(format!("vo_stage_{:02}", c.stage + 1))
    }
}

/// A3's cut-in SE per era (the themes have no cut-in).
pub fn cutin_se(skin: u8) -> Option<&'static str> {
    match skin {
        1 => Some("sele_1st"),
        2 => Some("sele_ext"),
        3 => Some("sele_sn2"),
        4 => Some("sele_x2"),
        5 => Some("sele_2013"),
        _ => None,
    }
}

/// What the panel's jacket frame shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Jacket {
    /// Skins 1–2: no jacket frame (A3 hides and pauses it).
    Hidden,
    /// The song's jacket (World already loaded it for its own panel).
    Song,
    /// Skin 3: the song's SuperNOVA 2 banner (`data/arc/banner/`).
    Banner,
}

/// A3 state 3/4. `has_banner` = `banner_sn2_<basename>` exists; A3 showed an
/// empty frame for a skin-3 song without one, this falls back to the jacket.
pub fn jacket(skin: u8, has_banner: bool) -> Jacket {
    match skin {
        1 | 2 => Jacket::Hidden,
        3 if has_banner => Jacket::Banner,
        _ => Jacket::Song,
    }
}

/// The SuperNOVA 2 banner texture stem for a song basename.
pub fn banner_stem(basename: &str) -> String {
    format!("banner_sn2_{}", basename)
}

// ── Per-panel state machine ─────────────────────────────────────────────

/// ShutterActor states (`services::shutter`).
pub const ST_IDLE: i32 = 0;
pub const ST_ART_READY: i32 = 1;
pub const ST_SWAP: i32 = 2;
pub const ST_IN: i32 = 3;
pub const ST_COVERED: i32 = 4;
pub const ST_REVEAL: i32 = 5;
pub const ST_PARKED: i32 = 6;
pub const ST_DRAIN: i32 = 7;
pub const ST_DRAIN_TAIL: i32 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Hosted for this window; World has not filled a stage panel yet.
    Waiting,
    /// World filled the stage panel (state 1 → 2) and the root was adopted,
    /// but the era packages are not ready yet: the legacy fill waits (at the
    /// latest until World's swap).
    FillWait,
    /// The legacy fill ran; waiting for World's swap.
    Filled,
    /// Swapped; the root is held at frame 0 while the cut-in plays.
    CutinHold,
    /// The root plays `in` → `loop` (covered).
    Showing,
    /// `frame_out` played (World state 6); waiting for READY.
    Parked,
    /// `out` → `end` (our READY dismiss, or World's own drain).
    Draining,
    /// World released the panel.
    Gone,
}

/// What the engine saw around one ShutterActor update.
#[derive(Clone, Copy, Debug, Default)]
pub struct Frame {
    pub pre_state: i32,
    pub post_state: i32,
    /// After the update: the pending / active kind is the stage kind.
    pub pending_is_stage: bool,
    pub active_is_stage: bool,
    /// Our cut-in layer exists: its frame and `out` / `close` label frames.
    pub cutin: Option<CutinView>,
    /// The active root's current frame and its `data_release` label frame.
    pub root_frame: Option<u32>,
    pub root_data_release: Option<u32>,
    /// The stage clip reached its `voice` label (engine-computed).
    pub voice_due: bool,
    /// The ControlMessageActor fired READY.
    pub ready_fired: bool,
    /// A player pressed START this frame (rising edge).
    pub skip_pressed: bool,
    /// Every era package the legacy fill loads from is ready.
    pub art_ready: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CutinView {
    pub frame: u32,
    pub out: Option<u32>,
    pub close: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// World's pending stage clip exists (state 1 → 2): verify it is A3's
    /// root and take it over (World's own stage voice / dismissal / dwell
    /// stand down from here). The engine abandons the panel if it is not.
    Adopt,
    /// Load the legacy sub-clips into the pending root, start the cut-in.
    Fill,
    /// Set the jacket texture (the swap made the jacket name current).
    SetJacket,
    /// Pause + hide the root at frame 0 (the cut-in is on screen).
    HoldRoot,
    /// Goto+play `in`, rate 1, visible (A3 state 4).
    ReleaseRoot,
    /// Cut-in to `out`, stop its SE.
    SkipCutin,
    DestroyCutin,
    PlayVoice,
    /// Deep `frame_out` on the root (A3 state 8).
    FrameOut,
    /// `0x100c` (A3's `0x100D` from ReadyGo at READY).
    Dismiss,
    /// The drain stalled in state 8.
    UnblockDrain,
    /// World released the panel: clean up.
    Finished,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Panel {
    phase: Phase,
    voice_done: bool,
    cutin_gone: bool,
    skipped: bool,
    drain_frames: u32,
    unblocked: bool,
}

impl Default for Panel {
    fn default() -> Self {
        Self::new()
    }
}

impl Panel {
    pub const fn new() -> Self {
        Panel {
            phase: Phase::Waiting,
            voice_done: false,
            cutin_gone: false,
            skipped: false,
            drain_frames: 0,
            unblocked: false,
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// Give up on this panel (the stage root is not A3's): no further action.
    pub fn abandon(&mut self) {
        self.phase = Phase::Gone;
    }

    /// Whether the panel is between the adoption and World's release.
    pub fn live(&self) -> bool {
        !matches!(self.phase, Phase::Waiting | Phase::Gone)
    }

    /// Advance on one update; returns every action due, in order.
    pub fn advance(&mut self, f: &Frame) -> Vec<Action> {
        let mut out = Vec::new();
        let cutin_live = f.cutin.is_some() && !self.cutin_gone;
        match self.phase {
            Phase::Waiting => {
                if f.pre_state == ST_ART_READY && f.post_state == ST_SWAP && f.pending_is_stage {
                    out.push(Action::Adopt);
                    if f.art_ready {
                        out.push(Action::Fill);
                        self.phase = Phase::Filled;
                    } else {
                        self.phase = Phase::FillWait;
                    }
                }
                return out;
            }
            Phase::Gone => return out,
            Phase::FillWait => {
                // World waits in state 2 for the song jacket; fill as soon as
                // the era packages are ready, at the latest when World swaps
                // the panel in (a partial fill beats none — the missing
                // placeholders keep their default content). A drain / release
                // before that skips the fill (handled below).
                if f.art_ready || (ST_IN..=ST_PARKED).contains(&f.post_state) {
                    out.push(Action::Fill);
                    self.phase = Phase::Filled;
                } else if f.post_state == ST_SWAP {
                    return out;
                }
            }
            _ => {}
        }

        // World released the panel (from any live phase).
        if f.post_state == ST_IDLE && !f.active_is_stage && !f.pending_is_stage {
            out.push(Action::Finished);
            self.phase = Phase::Gone;
            return out;
        }

        // World drained on its own (a banner request, quick restart's
        // dismiss): let the root run so `out` reaches `end`.
        if matches!(f.post_state, ST_DRAIN | ST_DRAIN_TAIL) && self.phase != Phase::Draining {
            if self.phase == Phase::CutinHold {
                out.push(Action::ReleaseRoot);
            }
            if cutin_live {
                out.push(Action::DestroyCutin);
                self.cutin_gone = true;
            }
            self.phase = Phase::Draining;
        }

        match self.phase {
            Phase::Filled => {
                if f.pre_state == ST_SWAP && f.post_state >= ST_IN && f.active_is_stage {
                    out.push(Action::SetJacket);
                    let holding = f
                        .cutin
                        .is_some_and(|c| c.close.is_none_or(|cl| c.frame < cl));
                    if holding {
                        out.push(Action::HoldRoot);
                        self.phase = Phase::CutinHold;
                    } else {
                        self.phase = Phase::Showing;
                    }
                }
            }
            Phase::CutinHold => match f.cutin {
                Some(c) => {
                    let before_out = c.out.is_none_or(|o| c.frame < o);
                    if f.skip_pressed
                        && !self.skipped
                        && c.frame > CUTIN_SKIP_MIN_FRAME
                        && before_out
                    {
                        out.push(Action::SkipCutin);
                        self.skipped = true;
                    }
                    if c.close.is_some_and(|cl| c.frame >= cl) {
                        out.push(Action::ReleaseRoot);
                        self.phase = Phase::Showing;
                    }
                }
                None => {
                    out.push(Action::ReleaseRoot);
                    self.phase = Phase::Showing;
                }
            },
            _ => {}
        }

        if matches!(self.phase, Phase::Showing | Phase::Parked) {
            if !self.voice_done && f.voice_due {
                out.push(Action::PlayVoice);
                self.voice_done = true;
            }
            if cutin_live
                && matches!((f.root_frame, f.root_data_release), (Some(r), Some(d)) if r >= d)
            {
                out.push(Action::DestroyCutin);
                self.cutin_gone = true;
            }
        }
        if self.phase == Phase::Showing && f.post_state == ST_PARKED {
            out.push(Action::FrameOut);
            self.phase = Phase::Parked;
        }
        if matches!(self.phase, Phase::Showing | Phase::Parked)
            && f.ready_fired
            && f.active_is_stage
            && matches!(f.post_state, ST_COVERED | ST_REVEAL | ST_PARKED)
        {
            if cutin_live && !out.contains(&Action::DestroyCutin) {
                out.push(Action::DestroyCutin);
                self.cutin_gone = true;
            }
            out.push(Action::Dismiss);
            self.phase = Phase::Draining;
        }

        if self.phase == Phase::Draining {
            if f.post_state == ST_DRAIN_TAIL {
                self.drain_frames += 1;
                if self.drain_frames > DRAIN_STALL_FRAMES && !self.unblocked {
                    out.push(Action::UnblockDrain);
                    self.unblocked = true;
                }
            } else {
                self.drain_frames = 0;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(stage: i32) -> StageCtx {
        StageCtx {
            stage,
            max_stage: 2,
            override_stage: -1,
            course: false,
            event_mode: 0,
        }
    }

    #[test]
    fn package_names() {
        let p = packages(3).unwrap();
        assert_eq!(p.choice, "common_choice0003");
        assert_eq!(p.shutter, "common_shutter0003");
        assert_eq!(p.cutin, "common_choice_cutin0003");
        assert_eq!(p.cutin_bg, "common_choice_cutinbg");
        assert!(packages(0).is_none());
        assert!(packages(6).is_none());
        for s in 1..=5 {
            let p = packages(s).unwrap();
            assert!(!p.choice.contains("0000") && !p.shutter.contains("0000"));
        }
    }

    #[test]
    fn stage_textures() {
        assert_eq!(stage_texture(1, &ctx(0)), "scene_choice_stage0001_1st");
        assert_eq!(stage_texture(1, &ctx(1)), "scene_choice_stage0001_2nd");
        assert_eq!(stage_texture(4, &ctx(2)), "scene_choice_stage0004_final");
        assert_eq!(stage_texture(4, &ctx(3)), "scene_choice_stage0004_extra");
        // A 4-stage session: stage 2 is neither 2nd nor final -> 1st (A3).
        let mut c = ctx(2);
        c.max_stage = 3;
        assert_eq!(stage_texture(5, &c), "scene_choice_stage0005_1st");
        // Event chains are never final / extra.
        let mut c = ctx(2);
        c.event_mode = 1;
        assert_eq!(stage_texture(2, &c), "scene_choice_stage0002_1st");
    }

    #[test]
    fn stage_voices() {
        for s in 0..=4 {
            assert_eq!(stage_voice(1, &ctx(s)), None, "skin 1 is silent");
        }
        assert_eq!(stage_voice(2, &ctx(0)).as_deref(), Some("sn2_etca2"));
        assert_eq!(stage_voice(3, &ctx(1)).as_deref(), Some("sn2_etca3"));
        assert_eq!(stage_voice(2, &ctx(2)).as_deref(), Some("sn2_etca7"));
        assert_eq!(stage_voice(3, &ctx(3)).as_deref(), Some("sn2_etc73"));
        assert_eq!(stage_voice(4, &ctx(0)).as_deref(), Some("vo_stage_01"));
        assert_eq!(stage_voice(5, &ctx(1)).as_deref(), Some("vo_stage_02"));
        assert_eq!(stage_voice(4, &ctx(2)).as_deref(), Some("vo_stage_final"));
        assert_eq!(stage_voice(5, &ctx(3)).as_deref(), Some("vo_stage_extra"));
        // Past the normal count (and not extra): silent.
        assert_eq!(stage_voice(4, &ctx(4)), None);
        // Four normal stages: stage 3 is final, stage 2 counts on.
        let mut c = ctx(3);
        c.max_stage = 3;
        assert_eq!(stage_voice(2, &c).as_deref(), Some("sn2_etca7"));
        c.stage = 2;
        assert_eq!(stage_voice(2, &c).as_deref(), Some("sn2_etca4"));
        // The stage the override names by index: silent.
        let mut c = ctx(1);
        c.override_stage = 1;
        assert_eq!(stage_voice(4, &c), None);
        // An override ending the session at stage 2 (1-based): final call.
        let mut c = ctx(1);
        c.override_stage = 2;
        assert_eq!(stage_voice(4, &c).as_deref(), Some("vo_stage_final"));
        // Stock skin: nothing.
        assert_eq!(stage_voice(0, &ctx(0)), None);
    }

    #[test]
    fn cutin_ses_and_jackets() {
        assert_eq!(cutin_se(1), Some("sele_1st"));
        assert_eq!(cutin_se(3), Some("sele_sn2"));
        assert_eq!(cutin_se(5), Some("sele_2013"));
        assert_eq!(cutin_se(0), None);
        assert_eq!(jacket(1, true), Jacket::Hidden);
        assert_eq!(jacket(2, false), Jacket::Hidden);
        assert_eq!(jacket(3, true), Jacket::Banner);
        assert_eq!(jacket(3, false), Jacket::Song);
        assert_eq!(jacket(4, true), Jacket::Song);
        assert_eq!(jacket(5, false), Jacket::Song);
        assert_eq!(banner_stem("cach"), "banner_sn2_cach");
    }

    fn f(pre: i32, post: i32) -> Frame {
        Frame {
            pre_state: pre,
            post_state: post,
            pending_is_stage: post < ST_IN,
            active_is_stage: post >= ST_IN,
            art_ready: true,
            ..Default::default()
        }
    }

    fn cut(frame: u32) -> Option<CutinView> {
        Some(CutinView {
            frame,
            out: Some(271),
            close: Some(360),
        })
    }

    #[test]
    fn full_panel_with_cutin() {
        let mut p = Panel::new();
        assert!(p.advance(&f(0, 1)).is_empty());
        assert_eq!(p.advance(&f(1, 2)), vec![Action::Adopt, Action::Fill]);
        assert_eq!(p.phase(), Phase::Filled);
        // Waiting for the jacket (state 2 stays).
        let mut w = f(2, 2);
        w.cutin = cut(3);
        assert!(p.advance(&w).is_empty());
        // Swap: jacket + hold.
        let mut s = f(2, 3);
        s.cutin = cut(10);
        assert_eq!(p.advance(&s), vec![Action::SetJacket, Action::HoldRoot]);
        assert_eq!(p.phase(), Phase::CutinHold);
        // Voice not due during the hold even if the clip says so.
        let mut h = f(3, 3);
        h.cutin = cut(100);
        h.voice_due = true;
        assert!(p.advance(&h).is_empty());
        // Close: release, and the voice goes in the same update.
        let mut c = f(3, 3);
        c.cutin = cut(360);
        c.voice_due = true;
        assert_eq!(p.advance(&c), vec![Action::ReleaseRoot, Action::PlayVoice]);
        // Voice only once.
        let mut v = f(3, 3);
        v.voice_due = true;
        v.cutin = cut(380);
        assert!(p.advance(&v).is_empty());
        // Root at data_release: the cut-in goes.
        let mut d = f(3, 4);
        d.cutin = cut(400);
        d.root_frame = Some(70);
        d.root_data_release = Some(70);
        assert_eq!(p.advance(&d), vec![Action::DestroyCutin]);
        // DPS step 5 -> World 5 -> 6: frame_out.
        assert!(p.advance(&f(4, 5)).is_empty());
        assert_eq!(p.advance(&f(5, 6)), vec![Action::FrameOut]);
        assert_eq!(p.phase(), Phase::Parked);
        // READY.
        let mut r = f(6, 6);
        r.ready_fired = true;
        assert_eq!(p.advance(&r), vec![Action::Dismiss]);
        assert_eq!(p.phase(), Phase::Draining);
        assert!(p.advance(&f(7, 8)).is_empty());
        let mut done = f(8, 0);
        done.active_is_stage = false;
        done.pending_is_stage = false;
        assert_eq!(p.advance(&done), vec![Action::Finished]);
        assert_eq!(p.phase(), Phase::Gone);
        assert!(p.advance(&f(1, 2)).is_empty(), "one panel per session");
    }

    #[test]
    fn no_cutin_shows_at_once() {
        let mut p = Panel::new();
        p.advance(&f(1, 2));
        assert_eq!(p.advance(&f(2, 3)), vec![Action::SetJacket]);
        assert_eq!(p.phase(), Phase::Showing);
        let mut v = f(3, 3);
        v.voice_due = true;
        assert_eq!(p.advance(&v), vec![Action::PlayVoice]);
    }

    #[test]
    fn cutin_skip_window() {
        let mut p = Panel::new();
        p.advance(&f(1, 2));
        let mut s = f(2, 3);
        s.cutin = cut(5);
        p.advance(&s);
        // Too early.
        let mut e = f(3, 3);
        e.cutin = cut(0x3B);
        e.skip_pressed = true;
        assert!(p.advance(&e).is_empty());
        // In the window.
        let mut k = f(3, 3);
        k.cutin = cut(0x3C);
        k.skip_pressed = true;
        assert_eq!(p.advance(&k), vec![Action::SkipCutin]);
        // Only once.
        k.cutin = cut(0x50);
        assert!(p.advance(&k).is_empty());
        // Past `out`: no skip for a fresh machine either.
        let mut q = Panel::new();
        q.advance(&f(1, 2));
        q.advance(&s);
        let mut late = f(3, 3);
        late.cutin = cut(271);
        late.skip_pressed = true;
        assert!(q.advance(&late).is_empty());
    }

    #[test]
    fn cutin_vanishing_releases_the_root() {
        let mut p = Panel::new();
        p.advance(&f(1, 2));
        let mut s = f(2, 3);
        s.cutin = cut(5);
        p.advance(&s);
        assert_eq!(p.advance(&f(3, 3)), vec![Action::ReleaseRoot]);
        assert_eq!(p.phase(), Phase::Showing);
    }

    #[test]
    fn world_drain_during_the_hold_resumes_the_root() {
        let mut p = Panel::new();
        p.advance(&f(1, 2));
        let mut s = f(2, 3);
        s.cutin = cut(5);
        p.advance(&s);
        let mut d = f(3, 7);
        d.cutin = cut(20);
        d.active_is_stage = true;
        assert_eq!(
            p.advance(&d),
            vec![Action::ReleaseRoot, Action::DestroyCutin]
        );
        assert_eq!(p.phase(), Phase::Draining);
    }

    #[test]
    fn ready_before_frame_out_still_dismisses() {
        let mut p = Panel::new();
        p.advance(&f(1, 2));
        p.advance(&f(2, 3));
        let mut r = f(4, 5);
        r.ready_fired = true;
        assert_eq!(p.advance(&r), vec![Action::Dismiss]);
    }

    #[test]
    fn ready_not_acted_on_before_covered() {
        let mut p = Panel::new();
        p.advance(&f(1, 2));
        p.advance(&f(2, 3));
        let mut r = f(3, 3);
        r.ready_fired = true;
        assert!(p.advance(&r).is_empty());
    }

    #[test]
    fn stalled_drain_is_unblocked_once() {
        let mut p = Panel::new();
        p.advance(&f(1, 2));
        p.advance(&f(2, 3));
        p.advance(&f(5, 6));
        let mut r = f(6, 6);
        r.ready_fired = true;
        p.advance(&r);
        let mut n = 0;
        for _ in 0..(DRAIN_STALL_FRAMES + 10) {
            n += p
                .advance(&f(8, 8))
                .iter()
                .filter(|a| **a == Action::UnblockDrain)
                .count();
        }
        assert_eq!(n, 1);
    }

    #[test]
    fn clayer_priorities() {
        assert_eq!(clayer_priority(3), 97);
        assert_eq!(clayer_priority(5), 95);
        assert_eq!(clayer_priority(ROOT_PRIORITY), 94);
        assert_eq!(clayer_priority(CUTIN_PRIORITY), 93);
        assert_eq!(clayer_priority(100), 0);
        assert_eq!(clayer_priority(150), 150);
        // A3 order: READY (5) over the stage-choice root (6) over the cut-in
        // (7); World's own shutter kinds (3) over all of them.
        assert!(clayer_priority(5) > clayer_priority(ROOT_PRIORITY));
        assert!(clayer_priority(ROOT_PRIORITY) > clayer_priority(CUTIN_PRIORITY));
        assert!(clayer_priority(3) > clayer_priority(5));
    }

    #[test]
    fn abandon_stops_everything() {
        let mut p = Panel::new();
        p.advance(&f(1, 2));
        p.abandon();
        assert_eq!(p.phase(), Phase::Gone);
        let mut r = f(6, 6);
        r.ready_fired = true;
        assert!(p.advance(&r).is_empty());
    }

    #[test]
    fn waiting_ignores_other_kinds() {
        let mut p = Panel::new();
        let mut other = f(1, 2);
        other.pending_is_stage = false;
        assert!(p.advance(&other).is_empty());
        assert_eq!(p.phase(), Phase::Waiting);
        assert!(!p.live());
    }

    #[test]
    fn fill_waits_for_the_era_packages() {
        let mut p = Panel::new();
        let mut e = f(1, 2);
        e.art_ready = false;
        assert_eq!(p.advance(&e), vec![Action::Adopt]);
        assert_eq!(p.phase(), Phase::FillWait);
        assert!(
            p.live(),
            "adopted: World's own stage voice / dismissal stand down"
        );
        // World still waits for the jacket: nothing.
        let mut w = f(2, 2);
        w.art_ready = false;
        assert!(p.advance(&w).is_empty());
        // Ready while World still waits: fill, then the swap as usual.
        let r = f(2, 2);
        assert_eq!(p.advance(&r), vec![Action::Fill]);
        assert_eq!(p.phase(), Phase::Filled);
        assert_eq!(p.advance(&f(2, 3)), vec![Action::SetJacket]);
    }

    #[test]
    fn swap_forces_a_partial_fill() {
        let mut p = Panel::new();
        let mut e = f(1, 2);
        e.art_ready = false;
        p.advance(&e);
        let mut s = f(2, 3);
        s.art_ready = false;
        assert_eq!(p.advance(&s), vec![Action::Fill, Action::SetJacket]);
        assert_eq!(p.phase(), Phase::Showing);
    }

    #[test]
    fn drain_before_the_fill_skips_it() {
        let mut p = Panel::new();
        let mut e = f(1, 2);
        e.art_ready = false;
        p.advance(&e);
        let mut d = f(2, 7);
        d.art_ready = false;
        d.active_is_stage = true;
        assert!(p.advance(&d).is_empty());
        assert_eq!(p.phase(), Phase::Draining);
        let mut done = f(8, 0);
        done.active_is_stage = false;
        done.pending_is_stage = false;
        done.art_ready = false;
        assert_eq!(p.advance(&done), vec![Action::Finished]);
    }

    #[test]
    fn themes_call_the_stage_like_a3() {
        for skin in 6..=8 {
            assert_eq!(stage_voice(skin, &ctx(0)).as_deref(), Some("vo_stage_01"));
            assert_eq!(stage_voice(skin, &ctx(1)).as_deref(), Some("vo_stage_02"));
            assert_eq!(
                stage_voice(skin, &ctx(2)).as_deref(),
                Some("vo_stage_final")
            );
            assert_eq!(
                stage_voice(skin, &ctx(3)).as_deref(),
                Some("vo_stage_extra")
            );
            assert_eq!(stage_voice(skin, &ctx(4)), None);
        }
        assert_eq!(stage_voice(9, &ctx(0)), None);
    }

    #[test]
    fn theme_variant_and_root_package() {
        for skin in 1..=5 {
            assert_eq!(variant(skin), Some(Variant::Era));
            assert_eq!(root_package(skin), Some(ROOT_PACKAGE));
        }
        for (skin, pkg) in [
            (6, "common_choice_v0"),
            (7, "common_choice_v2"),
            (8, "common_choice_v1"),
        ] {
            assert_eq!(variant(skin), Some(Variant::Theme));
            assert_eq!(root_package(skin), Some(pkg));
            assert_eq!(root_package_cstr(skin), Some(format!("{pkg}\0").as_str()));
            // A3's own UI had no era packages and no cut-in.
            assert!(packages(skin).is_none());
            assert_eq!(cutin_se(skin), None);
        }
        for skin in [0, 9] {
            assert_eq!(variant(skin), None);
            assert_eq!(root_package(skin), None);
        }
    }

    #[test]
    fn theme_band_follows_a3_skin_0() {
        let five = |stage| StageCtx {
            max_stage: 4,
            ..ctx(stage)
        };
        let t = |c: &StageCtx| theme_stage_texture(c);
        assert_eq!(t(&five(0)), "scene_choice_stage_1st");
        assert_eq!(t(&five(1)), "scene_choice_stage_2nd");
        assert_eq!(t(&five(2)), "scene_choice_stage_3rd");
        assert_eq!(t(&five(3)), "scene_choice_stage_4th");
        assert_eq!(t(&five(4)), "scene_choice_stage_final");
        assert_eq!(t(&five(5)), "scene_choice_stage_extra");
        // Three stages: 1st, 2nd, FINAL, EXTRA.
        assert_eq!(t(&ctx(0)), "scene_choice_stage_1st");
        assert_eq!(t(&ctx(1)), "scene_choice_stage_2nd");
        assert_eq!(t(&ctx(2)), "scene_choice_stage_final");
        assert_eq!(t(&ctx(3)), "scene_choice_stage_extra");
        // The final-stage override (1-based): stage index 1 is the final.
        let mut c = ctx(1);
        c.override_stage = 2;
        assert_eq!(t(&c), "scene_choice_stage_final");
        // A3's event-only special stages: the override's own index, or past
        // the extra stage — shown as EXTRA (A3's event art is not in World).
        let mut c = ctx(2);
        c.override_stage = 2;
        assert!(special_stage(&c));
        assert_eq!(t(&c), "scene_choice_stage_extra");
        assert!(special_stage(&ctx(4)));
        assert_eq!(t(&ctx(4)), "scene_choice_stage_extra");
        assert!(!special_stage(&ctx(3)), "the extra stage is not special");
        assert!(!special_stage(&ctx(0)));
        // Event chains: never final / extra / special.
        let mut c = ctx(4);
        c.event_mode = 2;
        assert!(!special_stage(&c));
        assert_eq!(t(&c), "scene_choice_stage_1st");
        // Special stages are exactly where the stage call stays silent.
        for stage in 0..=6 {
            for ov in [-1, 1, 2, 3] {
                let mut c = ctx(stage);
                c.override_stage = ov;
                assert_eq!(
                    special_stage(&c),
                    stage_voice(7, &c).is_none(),
                    "stage {stage} override {ov}"
                );
            }
        }
    }
}
