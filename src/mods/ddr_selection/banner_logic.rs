//! Pure rules of the legacy end banners (Step 6): which A3 art a song end
//! gets, and the overlay clip's life inside World's ShutterActor state
//! machine. Dependency-free (host-tested by
//! `scripts/validate_ddr_selection.sh`); the engine side is `banner.rs`.
//!
//! RE: `.agents/planning/2026-09-22-ddr-selection/research/`
//! `end-banners-sel-movies.md` §1–§2, §5.
//!
//! * World requests ShutterActor kind CLEARED (a side alive) or FAILED (every
//!   side dead) at the song end (DancePlaySequence step 8): kinds 4 / 5 on
//!   20260721+, 2 / 3 on the old layout — always stage kind + 1 / + 2. World
//!   draws one root clip per kind and fills nothing in it.
//! * A3 (`FUN_180039650`, kind art `FUN_1800306c0`) drew TWO clips from the
//!   era package `common_shutter000N`: a root (`shutter_clear` /
//!   `shutter_failed`, `CLayer` priority 3) and an overlay on top
//!   (`00_cleared` / `00_failed`, priority 2); Tohoku EVOLVED (mcode 37789)
//!   got `00_prayforall` on the CLEARED root. Both play `in` at the swap and
//!   `out` together; both die at the release.
//! * Only skin 4 (X) ships a `00_prayforall` that draws anything; the
//!   others fall back to `00_cleared`. Skin 1's is labelled but places no
//!   art at all — it only plays `vo_stage_clear`, `STG_APP02` and
//!   `STG_APP03` over the shutter (the arc is byte-identical to A3's, so A3
//!   showed a wordless banner there; cabinet 2026-09-24 — maintainer:
//!   show the era's CLEARED word instead). Skin 2's is an empty, label-less
//!   stub; skins 3 and 5 have none.
//!
//! The engine hosts the root through World's own named-package path (a
//! one-update row patch, as the stage panel) and owns only the overlay: it is
//! created parked when World's pending clip exists (state 1 → 2), started at
//! World's swap (the root's `in`, state 2 → 3), sent `out` when World plays
//! the root's `out` (state 5 → 8), and destroyed at its own `end` — long
//! before World's state 8 releases the package (overlay `out` → `end` = 17
//! frames, root `out` → `end` = 67). A pre-original check destroys it anyway
//! when the root is about to reach its release frame.

use super::panel_logic::{ST_COVERED, ST_DRAIN_TAIL, ST_IN, ST_SWAP};

/// A3's song end: Tohoku EVOLVED (`toho1`, `0x939D`) gets PRAY FOR ALL.
pub const TOHOKU_EVOLVED_MCODE: i32 = 37789;
/// A3 `SetPriority(3)` on the banner root (World's named path sets the same).
pub const ROOT_PRIORITY: u16 = 3;
/// A3 `SetPriority(2)` on the overlay: raw 98, drawn after (on top of) the
/// root's raw 97.
pub const OVERLAY_PRIORITY: u16 = 2;
/// Frames before the root's release frame at which the pre-original safety
/// net destroys a still-live overlay (World releases the package in the
/// update where the root reaches `max(out_end, end)`).
pub const RELEASE_MARGIN_FRAMES: u32 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Cleared,
    Failed,
}

impl Outcome {
    pub fn name(self) -> &'static str {
        match self {
            Outcome::Cleared => "CLEARED",
            Outcome::Failed => "FAILED",
        }
    }
}

/// The ShutterActor kind World requested, as an end-banner outcome (`None`
/// for every other kind).
pub fn outcome_of(kind: i32, cleared_kind: i32, failed_kind: i32) -> Option<Outcome> {
    if kind < 0 {
        None
    } else if kind == cleared_kind {
        Some(Outcome::Cleared)
    } else if kind == failed_kind {
        Some(Outcome::Failed)
    } else {
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overlay {
    Cleared,
    Failed,
    PrayForAll,
}

impl Overlay {
    /// The overlay clip's export name in `common_shutter000N`.
    pub fn clip(self) -> &'static str {
        match self {
            Overlay::Cleared => "00_cleared",
            Overlay::Failed => "00_failed",
            Overlay::PrayForAll => "00_prayforall",
        }
    }
}

/// A song end's legacy art.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Art {
    /// The root clip World creates through the patched row.
    pub root: &'static str,
    pub overlay: Overlay,
    /// Tohoku EVOLVED on an era without PRAY FOR ALL art (CLEARED instead).
    pub pray_fallback: bool,
}

/// Whether the skin's `00_prayforall` draws the PRAY FOR ALL art: skin 4
/// among the eras (skin 1's is sounds without art — see the module doc), and
/// every theme (A3's own `common_shutter_vN`, all three with art).
pub fn has_pray_for_all(skin: u8) -> bool {
    skin == 4 || super::policy::is_theme(skin)
}

/// A3's banner for a legacy song end (`None` outside skins
/// 1..=[`super::policy::SKIN_MAX`]).
pub fn art(skin: u8, outcome: Outcome, mcode: i32) -> Option<Art> {
    package(skin)?;
    Some(match outcome {
        Outcome::Failed => Art {
            root: "shutter_failed",
            overlay: Overlay::Failed,
            pray_fallback: false,
        },
        Outcome::Cleared => {
            let tohoku = mcode == TOHOKU_EVOLVED_MCODE;
            let pray = tohoku && has_pray_for_all(skin);
            Art {
                root: "shutter_clear",
                overlay: if pray {
                    Overlay::PrayForAll
                } else {
                    Overlay::Cleared
                },
                pray_fallback: tohoku && !pray,
            }
        }
    })
}

/// The banner packages by skin 1..=8, NUL-terminated (the engine hands the
/// pointers to World's kind-art loader, which keeps them until its done
/// callback — static): A3's `common_shutter%04d` for the eras, the theme
/// generation's own `common_shutter_vN` for the themes.
const PACKAGES: [&str; 8] = [
    "common_shutter0001\0",
    "common_shutter0002\0",
    "common_shutter0003\0",
    "common_shutter0004\0",
    "common_shutter0005\0",
    "common_shutter_v0\0",
    "common_shutter_v2\0",
    "common_shutter_v1\0",
];

/// The package the banner art comes from, NUL-terminated (static).
pub fn package_cstr(skin: u8) -> Option<&'static str> {
    PACKAGES.get((skin as usize).checked_sub(1)?).copied()
}

/// The package the banner art comes from.
pub fn package(skin: u8) -> Option<&'static str> {
    package_cstr(skin).map(|s| s.trim_end_matches('\0'))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// World read our row; its pending clip does not exist yet.
    Requested,
    /// Our overlay exists, parked invisible at frame 0.
    Parked,
    /// World's swap played the root's `in`; the overlay plays with it.
    Showing,
    /// World played the root's `out`; so does the overlay.
    Out,
    /// No overlay (destroyed, or never created); World still owns the root.
    Released,
    /// World released the root: the session is over.
    Done,
}

/// The overlay's playhead (for the destroy at its own `end`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OverlayView {
    pub frame: u32,
    pub end: Option<u32>,
}

/// What one ShutterActor update did, seen post-original.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    pub pre_state: i32,
    pub post_state: i32,
    /// The pending / active kind is this banner's kind.
    pub pending_is_ours: bool,
    pub active_is_ours: bool,
    pub overlay: Option<OverlayView>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Create the overlay, parked invisible (World's pending clip exists).
    CreateOverlay,
    /// goto+play `in`, visible (World played the root's `in`).
    StartOverlay,
    /// goto+play `out` (World played the root's `out`).
    OverlayOut,
    /// Destroy the overlay layer.
    DestroyOverlay,
    /// World released the root; forget the session.
    Finished,
}

/// One hosted end banner's overlay life (engine: `banner.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Banner {
    phase: Phase,
}

impl Default for Banner {
    fn default() -> Self {
        Self::new()
    }
}

impl Banner {
    pub fn new() -> Self {
        Self {
            phase: Phase::Requested,
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// An overlay action's layer is alive in this phase.
    pub fn overlay_alive(&self) -> bool {
        matches!(self.phase, Phase::Parked | Phase::Showing | Phase::Out)
    }

    /// The engine could not create the overlay (World's root still shows).
    pub fn overlay_failed(&mut self) {
        if self.phase != Phase::Done {
            self.phase = Phase::Released;
        }
    }

    /// The engine destroyed the overlay out of band (the safety net).
    pub fn overlay_destroyed(&mut self) {
        if self.overlay_alive() {
            self.phase = Phase::Released;
        }
    }

    /// Drive the overlay from one update. Several steps may fire at once
    /// (e.g. a missed swap edge creates and starts in the same update).
    pub fn advance(&mut self, f: &Frame) -> Vec<Action> {
        let mut out = Vec::new();
        if self.phase == Phase::Done {
            return out;
        }
        if !f.pending_is_ours && !f.active_is_ours {
            // World released the root (state 8 → 0), or the request never
            // turned into a clip of ours.
            if self.overlay_alive() {
                out.push(Action::DestroyOverlay);
            }
            out.push(Action::Finished);
            self.phase = Phase::Done;
            return out;
        }
        if self.phase == Phase::Requested {
            if (ST_SWAP..=ST_COVERED).contains(&f.post_state) {
                out.push(Action::CreateOverlay);
                self.phase = Phase::Parked;
            } else if f.post_state > ST_COVERED {
                // Past the covered state without a clip seen: no overlay.
                self.phase = Phase::Released;
            }
        }
        if self.phase == Phase::Parked && f.active_is_ours && f.post_state >= ST_IN {
            out.push(Action::StartOverlay);
            self.phase = Phase::Showing;
        }
        if self.phase == Phase::Showing && f.active_is_ours && f.post_state == ST_DRAIN_TAIL {
            out.push(Action::OverlayOut);
            self.phase = Phase::Out;
            return out; // the playhead seen this update predates the goto
        }
        if self.phase == Phase::Out {
            let done = match f.overlay {
                Some(OverlayView {
                    frame,
                    end: Some(end),
                }) => frame >= end,
                // No `end` label (not A3's clip) or unreadable: drop it now.
                _ => true,
            };
            if done {
                out.push(Action::DestroyOverlay);
                self.phase = Phase::Released;
            }
        }
        out
    }
}

/// Pre-original: World is about to release the root in this update (state 8
/// and the root at / near `max(out_end, end)`), so a still-live overlay must
/// go first — the package release would find it on the stream. An unreadable
/// root frame or target counts as "about to".
pub fn must_destroy_before_release(
    pre_state: i32,
    active_is_ours: bool,
    root_frame: Option<u32>,
    root_target: Option<u32>,
) -> bool {
    if pre_state != ST_DRAIN_TAIL || !active_is_ours {
        return false;
    }
    match (root_frame, root_target) {
        (Some(f), Some(t)) => f.saturating_add(RELEASE_MARGIN_FRAMES) >= t,
        _ => true,
    }
}

/// The root's release frame (World's state 8: `max(out_end, end)`, a missing
/// label reads 0).
pub fn release_target(out_end: Option<u32>, end: Option<u32>) -> u32 {
    out_end.unwrap_or(0).max(end.unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::super::panel_logic::{ST_ART_READY, ST_DRAIN, ST_IDLE, ST_PARKED, ST_REVEAL};
    use super::*;

    fn frame(pre: i32, post: i32, pending: bool, active: bool) -> Frame {
        Frame {
            pre_state: pre,
            post_state: post,
            pending_is_ours: pending,
            active_is_ours: active,
            overlay: None,
        }
    }

    #[test]
    fn outcome_maps_both_layouts() {
        assert_eq!(outcome_of(4, 4, 5), Some(Outcome::Cleared));
        assert_eq!(outcome_of(5, 4, 5), Some(Outcome::Failed));
        assert_eq!(outcome_of(2, 2, 3), Some(Outcome::Cleared));
        assert_eq!(outcome_of(3, 2, 3), Some(Outcome::Failed));
        assert_eq!(outcome_of(3, 4, 5), None, "the stage panel is not a banner");
        assert_eq!(outcome_of(-1, 4, 5), None);
    }

    #[test]
    fn art_per_skin_and_outcome() {
        for skin in 1..=5u8 {
            let c = art(skin, Outcome::Cleared, 1).unwrap();
            assert_eq!((c.root, c.overlay), ("shutter_clear", Overlay::Cleared));
            assert!(!c.pray_fallback);
            let f = art(skin, Outcome::Failed, TOHOKU_EVOLVED_MCODE).unwrap();
            assert_eq!((f.root, f.overlay), ("shutter_failed", Overlay::Failed));
            assert!(!f.pray_fallback, "PRAY FOR ALL is a CLEARED banner only");
        }
        assert_eq!(art(0, Outcome::Cleared, 1), None);
        assert_eq!(art(9, Outcome::Failed, 1), None);
    }

    #[test]
    fn pray_for_all_only_where_the_art_exists() {
        for skin in 1..=5u8 {
            let a = art(skin, Outcome::Cleared, TOHOKU_EVOLVED_MCODE).unwrap();
            assert_eq!(a.root, "shutter_clear");
            if skin == 4 {
                assert_eq!(a.overlay, Overlay::PrayForAll, "skin {skin}");
                assert_eq!(a.overlay.clip(), "00_prayforall");
                assert!(!a.pray_fallback);
            } else {
                assert_eq!(a.overlay, Overlay::Cleared, "skin {skin}");
                assert!(a.pray_fallback);
            }
        }
    }

    #[test]
    fn package_names() {
        assert_eq!(package(1), Some("common_shutter0001"));
        assert_eq!(package(5), Some("common_shutter0005"));
        assert_eq!(package(0), None);
    }

    /// World: 0 → (row read) 1 → 2 (clip created) → 3 (swap, `in`) → 4
    /// (covered) … 5 → 8 (`out`) … → 0 (release).
    #[test]
    fn full_life_follows_world_states() {
        let mut b = Banner::new();
        // The update that read the row: 0 → 1, pending ours.
        assert!(b
            .advance(&frame(ST_IDLE, ST_ART_READY, true, false))
            .is_empty());
        // Package loading.
        assert!(b
            .advance(&frame(ST_ART_READY, ST_ART_READY, true, false))
            .is_empty());
        assert_eq!(
            b.advance(&frame(ST_ART_READY, ST_SWAP, true, false)),
            vec![Action::CreateOverlay]
        );
        assert!(b.overlay_alive());
        // The swap: active now ours, `in` played.
        assert_eq!(
            b.advance(&frame(ST_SWAP, ST_IN, false, true)),
            vec![Action::StartOverlay]
        );
        assert!(b.advance(&frame(ST_IN, ST_COVERED, false, true)).is_empty());
        assert!(b
            .advance(&frame(ST_COVERED, ST_COVERED, false, true))
            .is_empty());
        // ResultSequence opens it (0x1008): 4 → 5, then 5 → 8 plays `out`.
        assert!(b
            .advance(&frame(ST_COVERED, ST_REVEAL, false, true))
            .is_empty());
        assert_eq!(
            b.advance(&frame(ST_REVEAL, ST_DRAIN_TAIL, false, true)),
            vec![Action::OverlayOut]
        );
        // Overlay plays its `out` (97 → 114).
        let mut f = frame(ST_DRAIN_TAIL, ST_DRAIN_TAIL, false, true);
        f.overlay = Some(OverlayView {
            frame: 100,
            end: Some(114),
        });
        assert!(b.advance(&f).is_empty());
        f.overlay = Some(OverlayView {
            frame: 114,
            end: Some(114),
        });
        assert_eq!(b.advance(&f), vec![Action::DestroyOverlay]);
        assert!(!b.overlay_alive());
        // The root keeps draining, then World releases it.
        assert!(b
            .advance(&frame(ST_DRAIN_TAIL, ST_DRAIN_TAIL, false, true))
            .is_empty());
        assert_eq!(
            b.advance(&frame(ST_DRAIN_TAIL, ST_IDLE, false, false)),
            vec![Action::Finished]
        );
        assert_eq!(b.phase(), Phase::Done);
        assert!(b.advance(&frame(ST_IDLE, ST_IDLE, false, false)).is_empty());
    }

    #[test]
    fn missed_swap_edge_creates_and_starts_together() {
        let mut b = Banner::new();
        assert_eq!(
            b.advance(&frame(ST_SWAP, ST_IN, false, true)),
            vec![Action::CreateOverlay, Action::StartOverlay]
        );
    }

    #[test]
    fn release_while_overlay_alive_destroys_it_first() {
        let mut b = Banner::new();
        b.advance(&frame(ST_ART_READY, ST_SWAP, true, false));
        b.advance(&frame(ST_SWAP, ST_IN, false, true));
        assert_eq!(
            b.advance(&frame(ST_DRAIN_TAIL, ST_IDLE, false, false)),
            vec![Action::DestroyOverlay, Action::Finished]
        );
    }

    #[test]
    fn overlay_without_end_label_is_dropped_at_out() {
        let mut b = Banner::new();
        b.advance(&frame(ST_ART_READY, ST_SWAP, true, false));
        b.advance(&frame(ST_SWAP, ST_IN, false, true));
        b.advance(&frame(ST_REVEAL, ST_DRAIN_TAIL, false, true));
        let mut f = frame(ST_DRAIN_TAIL, ST_DRAIN_TAIL, false, true);
        f.overlay = Some(OverlayView {
            frame: 3,
            end: None,
        });
        assert_eq!(b.advance(&f), vec![Action::DestroyOverlay]);
    }

    #[test]
    fn creation_failure_leaves_world_the_root() {
        let mut b = Banner::new();
        assert_eq!(
            b.advance(&frame(ST_ART_READY, ST_SWAP, true, false)),
            vec![Action::CreateOverlay]
        );
        b.overlay_failed();
        assert!(b.advance(&frame(ST_SWAP, ST_IN, false, true)).is_empty());
        assert!(b
            .advance(&frame(ST_REVEAL, ST_DRAIN_TAIL, false, true))
            .is_empty());
        assert_eq!(
            b.advance(&frame(ST_DRAIN_TAIL, ST_IDLE, false, false)),
            vec![Action::Finished]
        );
    }

    #[test]
    fn a_request_that_never_became_ours_finishes() {
        let mut b = Banner::new();
        assert_eq!(
            b.advance(&frame(ST_IDLE, ST_IDLE, false, false)),
            vec![Action::Finished]
        );
    }

    #[test]
    fn stage_states_never_trigger_out() {
        // States 6 / 7 are the stage panel's; a banner never sees them, but
        // they must not be mistaken for World's `out`.
        let mut b = Banner::new();
        b.advance(&frame(ST_ART_READY, ST_SWAP, true, false));
        b.advance(&frame(ST_SWAP, ST_IN, false, true));
        assert!(b
            .advance(&frame(ST_COVERED, ST_PARKED, false, true))
            .is_empty());
        assert!(b
            .advance(&frame(ST_PARKED, ST_DRAIN, false, true))
            .is_empty());
        assert_eq!(b.phase(), Phase::Showing);
    }

    #[test]
    fn safety_net_fires_only_at_the_release_frame() {
        let t = Some(release_target(None, Some(642)));
        assert_eq!(t, Some(642));
        assert!(!must_destroy_before_release(
            ST_DRAIN_TAIL,
            true,
            Some(600),
            t
        ));
        assert!(must_destroy_before_release(
            ST_DRAIN_TAIL,
            true,
            Some(638),
            t
        ));
        assert!(must_destroy_before_release(
            ST_DRAIN_TAIL,
            true,
            Some(642),
            t
        ));
        assert!(must_destroy_before_release(ST_DRAIN_TAIL, true, None, t));
        assert!(!must_destroy_before_release(ST_COVERED, true, Some(642), t));
        assert!(!must_destroy_before_release(
            ST_DRAIN_TAIL,
            false,
            Some(642),
            t
        ));
        assert_eq!(release_target(Some(700), Some(642)), 700);
        assert_eq!(release_target(None, None), 0);
    }

    #[test]
    fn overlay_out_outruns_the_root_out() {
        // The shipped labels: overlay out 97 → end 114, root out 575 → end
        // 642 — the overlay's own end comes ~50 frames before World's release.
        let overlay = 114 - 97;
        let root = 642 - 575;
        assert!(overlay + RELEASE_MARGIN_FRAMES < root);
    }

    #[test]
    fn themes_use_their_own_shutter_and_pray_for_all() {
        for (skin, pkg) in [
            (6, "common_shutter_v0"),
            (7, "common_shutter_v2"),
            (8, "common_shutter_v1"),
        ] {
            assert_eq!(package(skin), Some(pkg));
            assert_eq!(package_cstr(skin), Some(format!("{pkg}\0").as_str()));
            let c = art(skin, Outcome::Cleared, 1).unwrap();
            assert_eq!((c.root, c.overlay), ("shutter_clear", Overlay::Cleared));
            let f = art(skin, Outcome::Failed, TOHOKU_EVOLVED_MCODE).unwrap();
            assert_eq!((f.root, f.overlay), ("shutter_failed", Overlay::Failed));
            // Every theme's `00_prayforall` has art (A3's own Tohoku rule).
            let p = art(skin, Outcome::Cleared, TOHOKU_EVOLVED_MCODE).unwrap();
            assert_eq!(p.overlay, Overlay::PrayForAll);
            assert!(!p.pray_fallback);
        }
        for skin in 1..=5 {
            assert_eq!(
                package_cstr(skin),
                Some(format!("common_shutter{:04}\0", skin).as_str())
            );
        }
        assert_eq!(package_cstr(0), None);
        assert_eq!(package_cstr(9), None);
    }
}
