//! What one scene shows — the [`Pick`] (stage row + camera lists + dancers
//! + playlists + accessory parts) and its builders. PURE (std +
//! `super::selection` only) so the host harness mounts it; the
//! engine-facing half — parsing the pick's arcs, building instances — is
//! `session.rs`, which re-exports everything here.
//!
//! Since 2026-09-21 (design §4.8) a pick may describe a STAGE-ONLY scene
//! (`stage: Some`, no dancers — the BACKGROUND STAGE preview) or a
//! DANCER-ONLY scene (`stage: None`, one dancer — the BACKGROUND DANCER
//! preview) as well as the gameplay stage + dancers. [`ParseOptions`]
//! carries the one parse-time switch the previews need (no floor shadow).

use super::selection::{
    camera_lists, dancer_x, pick_dancers, pick_stage, playlist, DancerCandidate, PickSource, Rng,
    Sex, StageCandidate, SHADOW_ARC,
};

/// Parse-time switches for a pick (design §4.8): gameplay parses the
/// `pl_shadow00` floor quad, previews never do (FR-8 "no shadow").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseOptions {
    pub shadow: bool,
}

impl ParseOptions {
    pub const GAMEPLAY: ParseOptions = ParseOptions { shadow: true };
    pub const PREVIEW: ParseOptions = ParseOptions { shadow: false };
}

/// What one scene shows. Immutable once made.
#[derive(Debug, Clone)]
pub struct Pick {
    pub seed: u64,
    /// `None` = a dancer-only scene (no stage arc, no camera set).
    pub stage: Option<StageCandidate>,
    /// `stage_camera_resources.rlist` fields of the stage's ROW (empty
    /// without a stage).
    pub camera_row: Vec<String>,
    /// The row's camera names split + shuffled (A3 stage mode): the main
    /// cycle and the `_non` cut-aways.
    pub camera_main: Vec<String>,
    pub camera_non: Vec<String>,
    /// The MOVIE camera set (`movie_camera.rs`) split + shuffled like the
    /// stage's: used instead of the stage cameras while Background Movies =
    /// FULLSCREEN has a movie as the backdrop. Empty unless the song window
    /// latched that mode and the folder holds clips for the dancer count.
    pub movie_camera_main: Vec<String>,
    pub movie_camera_non: Vec<String>,
    /// Side order: index 0 = the left dancer. May be empty (stage-only).
    pub dancers: Vec<DancerCandidate>,
    /// Per dancer: shuffled clip names (`mc_<sex>_<name>_exec`).
    pub playlists: Vec<Vec<String>>,
    /// Per dancer: the accessory parts whose arc exists (`head00`, …).
    pub parts: Vec<Vec<String>>,
    /// Selection came from `DDR_DANCERS_PIN`.
    pub pinned: bool,
    /// Provenance of the stage / of each dancer (summary INFO only).
    pub source_stage: PickSource,
    pub source_dancers: Vec<PickSource>,
}

impl Pick {
    /// Stamp the per-element provenance (the caller knows which elements the
    /// option rows / the pin decided; `assemble_pick` defaults to `Random`).
    /// A `source_dancers` shorter than `dancers` is padded with `Random`.
    pub fn with_sources(mut self, stage: PickSource, mut dancers: Vec<PickSource>) -> Pick {
        dancers.resize(self.dancers.len(), PickSource::Random);
        self.source_stage = stage;
        self.source_dancers = dancers;
        self
    }

    /// Attach the movie camera lists (already filtered + shuffled).
    pub fn with_movie_cameras(mut self, main: Vec<String>, non: Vec<String>) -> Pick {
        self.movie_camera_main = main;
        self.movie_camera_non = non;
        self
    }

    /// Game-relative arc paths, deduplicated, in load order (stage first,
    /// then bodies, motion, parts, the shared shadow quad).
    pub fn arcs(&self) -> Vec<String> {
        self.arcs_for(&ParseOptions::GAMEPLAY)
    }

    /// [`arcs`](Self::arcs) minus the shadow arc when `opts.shadow` is off
    /// (a preview never loads `pl_shadow00`).
    pub fn arcs_for(&self, opts: &ParseOptions) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        if let Some(stage) = &self.stage {
            out.push(format!("data/arc/{}", stage.arc_name()));
        }
        for d in &self.dancers {
            let a = format!("data/arc/{}", d.body_arc_name());
            if !out.contains(&a) {
                out.push(a);
            }
        }
        for d in &self.dancers {
            let a = format!("data/arc/{}.arc", d.sex.arc_stem());
            if !out.contains(&a) {
                out.push(a);
            }
        }
        for (i, d) in self.dancers.iter().enumerate() {
            for part in self.parts.get(i).into_iter().flatten() {
                let a = format!("data/arc/{}", d.part_arc_name(part));
                if !out.contains(&a) {
                    out.push(a);
                }
            }
        }
        if opts.shadow && !self.dancers.is_empty() {
            let a = format!("data/arc/{SHADOW_ARC}");
            if !out.contains(&a) {
                out.push(a);
            }
        }
        out
    }

    /// The one-line per-song INFO. Every element carries its provenance
    /// (`{random}` / `{option}` / `{pin}`) so a field log shows whether the
    /// option rows were honoured. A dancer-only pick reads `stage=none{…}`.
    pub fn summary(&self) -> String {
        let n = self.dancers.len();
        let source_of = |i: usize| {
            self.source_dancers
                .get(i)
                .copied()
                .unwrap_or_default()
                .tag()
        };
        let dancers: Vec<String> = self
            .dancers
            .iter()
            .enumerate()
            .map(|(i, d)| {
                format!(
                    "{}({}) x={:+.1}{{{}}}",
                    d.key,
                    match d.sex {
                        Sex::Male => "M",
                        Sex::Female => "F",
                    },
                    dancer_x(i, n),
                    source_of(i)
                )
            })
            .collect();
        let clips: Vec<String> = self
            .playlists
            .iter()
            .map(|p| {
                p.iter()
                    .take(3)
                    .map(|c| c.rsplit('_').nth(1).unwrap_or(c).to_string())
                    .collect::<Vec<_>>()
                    .join(">")
            })
            .collect();
        let parts: Vec<String> = self
            .parts
            .iter()
            .map(|p| {
                p.iter()
                    .map(|n| n.trim_end_matches(|c: char| c.is_ascii_digit()).to_string())
                    .collect::<Vec<_>>()
                    .join("+")
            })
            .collect();
        let stage = match &self.stage {
            Some(s) => format!(
                "stage={}[{}]{{{}}} parts={}",
                s.key,
                s.row,
                self.source_stage.tag(),
                s.parts.len()
            ),
            None => format!("stage=none{{{}}} parts=0", self.source_stage.tag()),
        };
        let movie = if self.movie_camera_main.is_empty() {
            String::new()
        } else {
            format!(
                " movie-cameras=main:{} non:{}",
                self.movie_camera_main.len(),
                self.movie_camera_non.len()
            )
        };
        format!(
            "{} dancers=[{}] clips=[{}] wear=[{}] cameras=main:{} non:{}{} arcs={} seed=0x{:X}{}",
            stage,
            dancers.join(", "),
            clips.join(" | "),
            parts.join(" | "),
            self.camera_main.len(),
            self.camera_non.len(),
            movie,
            self.arcs().len(),
            self.seed,
            if self.pinned { " (PINNED)" } else { "" }
        )
    }
}

/// Random pick for `n` dancers. `camera_rows` = the row-parallel
/// `stage_camera_resources.rlist` (missing row ⇒ no camera set, Step 9
/// falls back to the fixed camera). `None` when there is no stage or no
/// dancer candidate.
pub fn make_pick(
    rng: &mut Rng,
    stages: &[StageCandidate],
    camera_rows: &[(String, Vec<String>)],
    dancers: &[DancerCandidate],
    n: usize,
    arc_exists: impl Fn(&str) -> bool,
) -> Option<Pick> {
    let stage = pick_stage(rng, stages)?.clone();
    let picked = pick_dancers(rng, dancers, n.max(1));
    if picked.is_empty() {
        return None;
    }
    Some(assemble_pick(
        rng,
        stage,
        camera_rows,
        picked,
        false,
        arc_exists,
    ))
}

/// Finish a gameplay pick whose stage/dancers are already chosen (random or
/// pinned): camera row + playlists + the part arcs that exist (`arc_exists`
/// takes an arc file name like `pl_emi00_face01.arc`).
pub fn assemble_pick(
    rng: &mut Rng,
    stage: StageCandidate,
    camera_rows: &[(String, Vec<String>)],
    dancers: Vec<DancerCandidate>,
    pinned: bool,
    arc_exists: impl Fn(&str) -> bool,
) -> Pick {
    assemble_pick_opt(rng, Some(stage), camera_rows, dancers, pinned, arc_exists)
}

/// The general form of [`assemble_pick`]: an optional stage (dancer-only
/// scenes) and a possibly empty dancer list (stage-only scenes). The rng
/// draw order — one playlist shuffle per dancer, then the two camera-list
/// shuffles — is the gameplay one, so a `Some(stage)` call reproduces
/// [`assemble_pick`] exactly.
pub fn assemble_pick_opt(
    rng: &mut Rng,
    stage: Option<StageCandidate>,
    camera_rows: &[(String, Vec<String>)],
    dancers: Vec<DancerCandidate>,
    pinned: bool,
    arc_exists: impl Fn(&str) -> bool,
) -> Pick {
    let camera_row = stage
        .as_ref()
        .and_then(|s| camera_rows.get(s.row))
        .map(|(_, f)| f.clone())
        .unwrap_or_default();
    let playlists = dancers.iter().map(|d| playlist(rng, d.sex)).collect();
    let parts = dancers
        .iter()
        .map(|d| d.parts_present(&arc_exists))
        .collect();
    let (camera_main, camera_non) = camera_lists(rng, &camera_row);
    let n = dancers.len();
    Pick {
        seed: 0,
        stage,
        camera_row,
        camera_main,
        camera_non,
        movie_camera_main: Vec::new(),
        movie_camera_non: Vec::new(),
        dancers,
        playlists,
        parts,
        pinned,
        source_stage: PickSource::Random,
        source_dancers: vec![PickSource::Random; n],
    }
}

/// The BACKGROUND STAGE preview's pick (design §4.6): a uniform row of
/// `key` (the chosen-stage rule of `selection::resolve_choice`), no dancers,
/// the row's camera lists shuffled. `None` when the catalog key has no row.
pub fn stage_only(
    rng: &mut Rng,
    stages: &[StageCandidate],
    camera_rows: &[(String, Vec<String>)],
    key: &str,
) -> Option<Pick> {
    let rows: Vec<&StageCandidate> = stages.iter().filter(|s| s.key == key).collect();
    if rows.is_empty() {
        return None;
    }
    let row = (*rows.get(rng.below(rows.len() as u32) as usize)?).clone();
    Some(
        assemble_pick_opt(rng, Some(row), camera_rows, vec![], false, |_| false)
            .with_sources(PickSource::Option, vec![]),
    )
}

/// The BACKGROUND DANCER preview's pick: that dancer alone, no stage (no
/// camera set — the caller's fixed camera), its playlist shuffled from its
/// sex pool, the accessory parts whose arc exists. `None` for an unknown key.
pub fn dancer_only(
    rng: &mut Rng,
    dancers: &[DancerCandidate],
    key: &str,
    arc_exists: impl Fn(&str) -> bool,
) -> Option<Pick> {
    let d = dancers.iter().find(|d| d.key == key)?.clone();
    Some(
        assemble_pick_opt(rng, None, &[], vec![d], false, arc_exists)
            .with_sources(PickSource::Random, vec![PickSource::Option]),
    )
}

#[cfg(test)]
mod tests {
    use super::super::selection::fixtures::{real_chara_rows, real_map_rows, rows};
    use super::super::selection::{dancer_candidates, stage_candidates, Rng};
    use super::*;

    const SEED: u64 = 0x5EED_CAFE;

    fn tables() -> (
        Vec<StageCandidate>,
        Vec<(String, Vec<String>)>,
        Vec<DancerCandidate>,
    ) {
        let stages = stage_candidates(&real_map_rows(), |_| true);
        let dancers = dancer_candidates(&real_chara_rows(), |_| true);
        // Camera rows for both boom00 rows (0 and 32): two main + one `_non`;
        // every other row empty (no camera set).
        let mut camera_rows: Vec<(String, Vec<String>)> = vec![(String::new(), Vec::new()); 40];
        for row in [0usize, 32] {
            camera_rows[row] = rows(&[("boom00", &["st001_st00", "st001_st01", "st001_non00"])])
                .pop()
                .unwrap();
        }
        (stages, camera_rows, dancers)
    }

    fn stage(stages: &[StageCandidate], key: &str) -> StageCandidate {
        stages.iter().find(|s| s.key == key).unwrap().clone()
    }

    fn dancer(dancers: &[DancerCandidate], key: &str) -> DancerCandidate {
        dancers.iter().find(|d| d.key == key).unwrap().clone()
    }

    #[test]
    fn parse_options_consts() {
        assert!(ParseOptions::GAMEPLAY.shadow);
        assert!(!ParseOptions::PREVIEW.shadow);
    }

    #[test]
    fn gameplay_pick_is_unchanged() {
        let (stages, camera_rows, dancers) = tables();
        let boom = stage(&stages, "boom00");
        let picked = vec![dancer(&dancers, "emi01"), dancer(&dancers, "rage00")];

        let mut rng = Rng::new(SEED);
        let pick = assemble_pick(
            &mut rng,
            boom.clone(),
            &camera_rows,
            picked.clone(),
            false,
            |_| true,
        );
        assert_eq!(pick.stage.as_ref(), Some(&boom));
        let arcs = pick.arcs();
        assert_eq!(arcs[0], "data/arc/mapset_boom00.arc");
        assert_eq!(arcs.last().unwrap(), "data/arc/pl_shadow00.arc");
        assert_eq!(pick.arcs_for(&ParseOptions::GAMEPLAY), arcs);
        assert_eq!(pick.camera_main.len(), 2);
        assert_eq!(pick.camera_non.len(), 1);
        assert_eq!(pick.playlists.len(), 2);
        assert_eq!(pick.parts.len(), 2);
        assert!(
            pick.parts.iter().all(|p| p.len() == 5),
            "every part arc exists"
        );
        let summary = pick.summary();
        assert!(
            summary.starts_with("stage=boom00[0]{random} parts=5 dancers=["),
            "{summary}"
        );
        assert!(summary.contains("emi01(F) x=-0.8{random}"), "{summary}");
        assert!(summary.contains("rage00(M) x=+0.8{random}"), "{summary}");
        assert!(summary.contains("cameras=main:2 non:1"), "{summary}");
        assert!(
            summary.contains(&format!("arcs={}", arcs.len())),
            "{summary}"
        );

        // The general form with `Some(stage)` reproduces it field for field.
        let mut rng2 = Rng::new(SEED);
        let opt = assemble_pick_opt(&mut rng2, Some(boom), &camera_rows, picked, false, |_| true);
        assert_eq!(opt.camera_row, pick.camera_row);
        assert_eq!(opt.camera_main, pick.camera_main);
        assert_eq!(opt.camera_non, pick.camera_non);
        assert_eq!(opt.playlists, pick.playlists);
        assert_eq!(opt.parts, pick.parts);
        assert_eq!(opt.summary(), pick.summary());
    }

    #[test]
    fn stage_only_pick() {
        let (stages, camera_rows, _) = tables();
        let boom = stage(&stages, "boom00");
        let mut rng = Rng::new(SEED);
        let pick = assemble_pick_opt(&mut rng, Some(boom), &camera_rows, vec![], false, |_| true);
        assert_eq!(pick.arcs(), vec!["data/arc/mapset_boom00.arc".to_string()]);
        assert_eq!(pick.arcs_for(&ParseOptions::PREVIEW), pick.arcs());
        assert!(pick.playlists.is_empty());
        assert!(pick.parts.is_empty());
        assert!(pick.source_dancers.is_empty());
        assert_eq!(pick.camera_main.len(), 2);
        let summary = pick.summary();
        assert!(summary.contains("dancers=[] clips=[] wear=[]"), "{summary}");
        assert!(
            summary.starts_with("stage=boom00[0]{random} parts=5"),
            "{summary}"
        );
    }

    #[test]
    fn dancer_only_pick() {
        let (_, camera_rows, dancers) = tables();
        let emi = dancer(&dancers, "emi01");
        let mut rng = Rng::new(SEED);
        let pick = assemble_pick_opt(&mut rng, None, &camera_rows, vec![emi], false, |_| true);
        let arcs = pick.arcs();
        assert!(arcs.iter().all(|a| !a.contains("mapset_")), "{arcs:?}");
        assert_eq!(arcs[0], "data/arc/pl_emi01.arc");
        assert_eq!(arcs.last().unwrap(), "data/arc/pl_shadow00.arc");
        let preview = pick.arcs_for(&ParseOptions::PREVIEW);
        assert!(!preview.iter().any(|a| a.ends_with("pl_shadow00.arc")));
        assert_eq!(preview.len(), arcs.len() - 1);
        assert!(pick.camera_row.is_empty());
        assert!(pick.camera_main.is_empty() && pick.camera_non.is_empty());
        let summary = pick.summary();
        assert!(
            summary.starts_with("stage=none{random} parts=0 dancers=[emi01(F) x=+0.0{random}]"),
            "{summary}"
        );
        assert!(summary.contains("cameras=main:0 non:0"), "{summary}");
    }

    #[test]
    fn with_sources_pads_and_stamps() {
        let (stages, camera_rows, dancers) = tables();
        let mut rng = Rng::new(SEED);
        let pick = assemble_pick(
            &mut rng,
            stage(&stages, "boom00"),
            &camera_rows,
            vec![dancer(&dancers, "emi01"), dancer(&dancers, "rage00")],
            true,
            |_| true,
        )
        .with_sources(PickSource::Option, vec![PickSource::Pin]);
        assert_eq!(pick.source_stage, PickSource::Option);
        assert_eq!(
            pick.source_dancers,
            vec![PickSource::Pin, PickSource::Random]
        );
        let summary = pick.summary();
        assert!(summary.starts_with("stage=boom00[0]{option}"), "{summary}");
        assert!(summary.ends_with(" (PINNED)"), "{summary}");
    }

    #[test]
    fn make_pick_needs_both_tables() {
        let (stages, camera_rows, dancers) = tables();
        let mut rng = Rng::new(SEED);
        assert!(make_pick(&mut rng, &[], &camera_rows, &dancers, 1, |_| true).is_none());
        assert!(make_pick(&mut rng, &stages, &camera_rows, &[], 1, |_| true).is_none());
        let pick = make_pick(&mut rng, &stages, &camera_rows, &dancers, 2, |_| true).unwrap();
        assert!(pick.stage.is_some());
        assert_eq!(pick.dancers.len(), 2);
    }

    #[test]
    fn stage_only_builder() {
        let (stages, camera_rows, _) = tables();
        let mut rng = Rng::new(SEED);
        let pick = stage_only(&mut rng, &stages, &camera_rows, "boom00").expect("boom00 exists");
        assert_eq!(pick.stage.as_ref().map(|s| s.key.as_str()), Some("boom00"));
        assert!(pick.dancers.is_empty() && pick.playlists.is_empty() && pick.parts.is_empty());
        assert_eq!(pick.camera_main.len(), 2);
        assert_eq!(pick.camera_non.len(), 1);
        assert_eq!(pick.source_stage, PickSource::Option);
        assert!(pick.source_dancers.is_empty());
        assert_eq!(
            pick.arcs_for(&ParseOptions::PREVIEW),
            vec!["data/arc/mapset_boom00.arc".to_string()]
        );
        assert!(
            pick.summary().starts_with("stage=boom00["),
            "{}",
            pick.summary()
        );
        assert!(
            pick.summary().contains("]{option} parts="),
            "{}",
            pick.summary()
        );
        assert!(
            pick.summary().contains(" dancers=[] clips=[] wear=[]"),
            "{}",
            pick.summary()
        );
        assert!(stage_only(&mut rng, &stages, &camera_rows, "nosuchstage").is_none());
        // Any other catalog key resolves to one of its rows.
        let p2 = stage_only(&mut rng, &stages, &camera_rows, "monitor01").unwrap();
        assert_eq!(p2.stage.as_ref().unwrap().key, "monitor01");
    }

    #[test]
    fn dancer_only_builder() {
        let (_, _, dancers) = tables();
        let mut rng = Rng::new(SEED);
        let pick = dancer_only(&mut rng, &dancers, "emi01", |_| true).expect("emi01 exists");
        assert!(pick.stage.is_none());
        assert_eq!(pick.dancers.len(), 1);
        assert_eq!(pick.dancers[0].key, "emi01");
        assert_eq!(pick.playlists.len(), 1);
        assert_eq!(pick.parts[0].len(), 5);
        assert_eq!(pick.source_stage, PickSource::Random);
        assert_eq!(pick.source_dancers, vec![PickSource::Option]);
        let arcs = pick.arcs_for(&ParseOptions::PREVIEW);
        assert!(!arcs
            .iter()
            .any(|a| a.contains("mapset_") || a.contains("pl_shadow00")));
        assert_eq!(arcs[0], "data/arc/pl_emi01.arc");
        assert!(pick.summary().contains("dancers=[emi01(F) x=+0.0{option}]"));
        assert!(dancer_only(&mut rng, &dancers, "nobody00", |_| true).is_none());
    }
}
