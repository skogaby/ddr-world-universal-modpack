//! Per-song assets and instances (design §4.3.5): the [`Pick`] (what this
//! song shows), the [`Parsed`] bundle a background thread produces from the
//! pick's arcs (`.anm` clips, `.model` bone tables), the [`Session`] that
//! owns them for the window, and the game-thread instance builder that
//! turns each resident model into a render item + scene node
//! (generalised from the Step 3/4 spike's `build_slot`).
//!
//! Step 8: stage parts, dancer bodies, the dancers' accessory PARTS (rigid
//! models hung off one body bone each — `head00`/`face01` → `Head`, `hips00`
//! → `Hips`, `chest00` → `Spine2`, `forearm00` → `LeftForeArmRoll` plus a
//! mirrored second instance on `RightForeArmRoll`) and one `pl_shadow00`
//! floor quad per dancer. Step 9: the stage's `.camanm` sets (main + `_non`
//! lists, shuffled per song) sequenced by [`CameraSchedule`] drive camera
//! slot 0 every visible frame.
//!
//! Threading: `parse_pick` runs on ONE std thread per song and touches no
//! engine state (it reads the `.arc` files from disk — `arc_set::read_bytes`
//! returns the whole archive, `core::arc` extracts + LZ77-decompresses the
//! members); everything else here is game thread only.

use std::sync::Arc;
use std::time::Instant;

use crate::core::anm::pose::{seed_local_trs, Skeleton, Trs};
use crate::core::anm::{anm as anmfile, b2it, ktmdl, Anm, Mat4};
use crate::core::arc as arcfile;
use crate::services::avs_layeredfs::shader_layout::{self, SceneStyle};
use crate::services::scene3d::frame_board::{self, NO_SLOT};
use crate::services::scene3d::render_item_layout::{
    scale_translation, IDENTITY, PASS_MASK_DANCER, PASS_MASK_LOWPRIO, PASS_MASK_STAGE,
};
use crate::services::scene3d::{
    arc_set, model_registry, node, pure, render_item, scene_graph, texture,
};
use crate::{log_info, log_warn};

use super::director_math::{
    body_world, part_world, shadow_target, shadow_world, transform_point, BLACK, SHADOW_FLOOR_Y,
    WHITE,
};
use super::instance_plan::{
    plan_instances, DancerSpec, PartSpec, PassMasks, PlanInput, StagePartSpec,
};
use super::outline::{self, HullPlan};
use super::schedule::{CameraSchedule, CameraState, ClipRef, DanceSchedule};
use super::selection::{
    camanm_member_path, clip_member_path, part_attach_bone, Sex, GROUND_BONES, HIPS_BONE,
    MIRROR_ATTACH_BONE, MIRROR_PART, SHADOW_ARC, SHADOW_MODEL,
};
use super::tempo::{TempoOptions, BEAT_TAU};

/// The stage camera sets (read by OUR parser only — the engine needs nothing
/// from this arc, so it is never `FileManager::Load`ed).
pub const STAGE_CAMERA_ARC: &str = "data/arc/camera/stage_camera.arc";

/// Build a resident model even if its textures are still unregistered after
/// this long (the per-frame retry then finishes the job) — Step 3 finding.
pub const TEXTURE_WAIT_TIMEOUT_MS: u64 = 10_000;

// ---------------------------------------------------------------------------
// Pick (pure — `pick.rs`; re-exported so every consumer keeps its path)
// ---------------------------------------------------------------------------

pub use super::pick::{assemble_pick, make_pick, ParseOptions, Pick};

// ---------------------------------------------------------------------------
// Parsed bundle (background thread)
// ---------------------------------------------------------------------------

/// One parsed ANM clip with the bytes it samples from.
pub struct Clip {
    pub name: String,
    pub bytes: Arc<Vec<u8>>,
    pub anm: Anm,
}

impl Clip {
    pub fn clip_ref(&self) -> ClipRef {
        ClipRef::new(self.name.clone(), self.anm.duration_s(), self.anm.loops)
    }
}

pub struct ParsedStagePart {
    pub part: String,
    pub priority: Option<i32>,
    /// `gm_<stage>_<part>` — the ResourceManager key.
    pub model_name: String,
    pub skeleton: Skeleton,
    /// A3 bind seed (partial loops keep the authored pose — RE doc §3.2).
    pub seed: Vec<Trs>,
    pub loop_clip: Option<Clip>,
}

/// One rigid accessory part hung off a body bone.
pub struct ParsedPart {
    /// `head00`, …, or `forearm00` twice (the second mirrored).
    pub part: String,
    /// `pl_<key>_<part>` — the ResourceManager key (shared by both forearms).
    pub model_name: String,
    /// Body bone index the part follows.
    pub attach: usize,
    /// The right-forearm point inversion (`director_math::MIRROR`).
    pub mirror: bool,
    pub skeleton: Skeleton,
}

pub struct ParsedDancer {
    pub key: String,
    pub sex: Sex,
    /// `pl_<key>`.
    pub model_name: String,
    pub model_scale: f32,
    pub shadow_scale: f32,
    pub skeleton: Skeleton,
    pub seed: Vec<Trs>,
    /// In playlist order (missing members dropped — the schedule adapts).
    pub clips: Vec<Clip>,
    /// Accessory parts that parsed (empty without a `.b2it`).
    pub parts: Vec<ParsedPart>,
    /// Ground-contact bone indices (shadow centre/spread); empty ⇒ no shadow.
    pub ground: Vec<usize>,
    /// `Hips` bone index (shadow height rule).
    pub hips: Option<usize>,
}

/// The stage's camera sets that parsed (in the pick's shuffled order).
pub struct ParsedCameras {
    pub main: Vec<Clip>,
    pub non: Vec<Clip>,
}

pub struct Parsed {
    pub stage_parts: Vec<ParsedStagePart>,
    pub dancers: Vec<ParsedDancer>,
    /// `pl_shadow00`'s bone table (one rigid bone); `None` ⇒ no shadows.
    pub shadow: Option<Skeleton>,
    /// `None` ⇒ no usable camera set (the fixed fallback camera is used).
    pub cameras: Option<ParsedCameras>,
    /// The MOVIE camera set (loose `.camanm` files, `movie_camera.rs`) —
    /// `None` unless the pick carried one and a main clip parsed.
    pub movie_cameras: Option<ParsedCameras>,
    /// One line per skipped member/instance — logged once by the lifecycle.
    pub warnings: Vec<String>,
    pub elapsed_ms: u64,
}

/// A whole `.arc` in memory with its member table.
struct ArcReader {
    data: Vec<u8>,
    entries: Vec<arcfile::ArcEntry>,
}

impl ArcReader {
    fn open(game_rel: &str) -> Option<ArcReader> {
        let data = arc_set::read_bytes(game_rel)?;
        let entries = arcfile::parse(&data)?;
        Some(ArcReader { data, entries })
    }
    fn get(&self, member: &str) -> Option<Vec<u8>> {
        let e = self.entries.iter().find(|e| e.path == member)?;
        arcfile::extract(&self.data, e)
    }
}

fn parse_clip(
    reader: &ArcReader,
    member: &str,
    name: &str,
    warnings: &mut Vec<String>,
) -> Option<Clip> {
    let Some(bytes) = reader.get(member) else {
        warnings.push(format!("{member}: member missing"));
        return None;
    };
    match anmfile::parse(&bytes) {
        Ok(anm) => Some(Clip {
            name: name.to_string(),
            bytes: Arc::new(bytes),
            anm,
        }),
        Err(e) => {
            warnings.push(format!("{member}: {e}"));
            None
        }
    }
}

fn parse_skeleton(
    reader: &ArcReader,
    member: &str,
    warnings: &mut Vec<String>,
) -> Option<Skeleton> {
    let Some(bytes) = reader.get(member) else {
        warnings.push(format!("{member}: member missing"));
        return None;
    };
    match ktmdl::bone_table(&bytes) {
        Ok(sk) if sk.bone_count() > 0 => Some(sk),
        Ok(_) => {
            warnings.push(format!("{member}: no bones"));
            None
        }
        Err(e) => {
            warnings.push(format!("{member}: {e}"));
            None
        }
    }
}

/// Read + parse everything the pick needs. Blocking; no engine calls.
/// `opts.shadow == false` (previews) never opens `pl_shadow00.arc`; a
/// `stage: None` pick parses no stage and no camera set (no warning either).
pub fn parse_pick(pick: &Pick, opts: &ParseOptions) -> Parsed {
    let started = Instant::now();
    let mut warnings = Vec::new();
    let mut stage_parts = Vec::new();
    let mut dancers = Vec::new();

    // Stage parts. The reader stays open: a custom stage may carry its own
    // `.camanm` clips (design 2026-09-22 D9), looked up there first.
    let mut stage_reader: Option<ArcReader> = None;
    if let Some(stage) = pick.stage.as_ref() {
        let stage_arc = format!("data/arc/{}", stage.arc_name());
        match ArcReader::open(&stage_arc) {
            None => warnings.push(format!("{stage_arc}: unreadable -- no stage this song")),
            Some(reader) => {
                for (part, priority) in &stage.parts {
                    let model_name = format!("gm_{}_{}", stage.key, part);
                    let dir = format!("data/map/{model_name}");
                    let Some(skeleton) = parse_skeleton(
                        &reader,
                        &format!("{dir}/{model_name}.model"),
                        &mut warnings,
                    ) else {
                        continue;
                    };
                    let loop_member = format!("{dir}/{model_name}_play_loop.anm");
                    let loop_clip = if reader.entries.iter().any(|e| e.path == loop_member) {
                        parse_clip(
                            &reader,
                            &loop_member,
                            &format!("{model_name}_play_loop"),
                            &mut warnings,
                        )
                    } else {
                        None
                    };
                    let seed = seed_local_trs(&skeleton);
                    stage_parts.push(ParsedStagePart {
                        part: part.clone(),
                        priority: *priority,
                        model_name,
                        skeleton,
                        seed,
                        loop_clip,
                    });
                }
                stage_reader = Some(reader);
            }
        }
    }

    // Dancers: body skeleton + playlist clips (one motion arc per sex).
    let mut motion: Vec<(Sex, Option<ArcReader>)> = Vec::new();
    for (i, d) in pick.dancers.iter().enumerate() {
        let body_arc = format!("data/arc/{}", d.body_arc_name());
        let Some(body) = ArcReader::open(&body_arc) else {
            warnings.push(format!(
                "{body_arc}: unreadable -- dancer {} dropped",
                d.key
            ));
            continue;
        };
        let model_name = d.body_model_name();
        let Some(skeleton) = parse_skeleton(
            &body,
            &format!("data/chara/{model_name}/{model_name}.model"),
            &mut warnings,
        ) else {
            continue;
        };
        if motion.iter().all(|(s, _)| *s != d.sex) {
            let arc = format!("data/arc/{}.arc", d.sex.arc_stem());
            let r = ArcReader::open(&arc);
            if r.is_none() {
                warnings.push(format!("{arc}: unreadable -- no choreography for that sex"));
            }
            motion.push((d.sex, r));
        }
        let mut clips = Vec::new();
        if let Some((_, Some(reader))) = motion.iter().find(|(s, _)| *s == d.sex) {
            for clip in pick.playlists.get(i).into_iter().flatten() {
                if let Some(c) =
                    parse_clip(reader, &clip_member_path(d.sex, clip), clip, &mut warnings)
                {
                    clips.push(c);
                }
            }
        }
        if clips.is_empty() {
            warnings.push(format!("dancer {} has no playable clip -- dropped", d.key));
            continue;
        }
        let seed = seed_local_trs(&skeleton);

        // Parts + shadow bones need the body's `.b2it` (bone name → index).
        // Missing/unparseable ⇒ the body dances alone (one warning).
        let names = match body.get(&format!("data/chara/{model_name}/{model_name}.b2it")) {
            Some(bytes) => match b2it::parse(&bytes) {
                Ok(t) => Some(t),
                Err(e) => {
                    warnings.push(format!("{model_name}.b2it: {e} -- no parts/shadow"));
                    None
                }
            },
            None => {
                warnings.push(format!(
                    "{model_name}.b2it: member missing -- no parts/shadow"
                ));
                None
            }
        };
        let bone_index = |name: &str| -> Option<usize> {
            let t = names.as_ref()?;
            let idx = b2it::index_of(t, name)? as usize;
            (idx < skeleton.bone_count()).then_some(idx)
        };
        let mut parts = Vec::new();
        if names.is_some() {
            for part in pick.parts.get(i).into_iter().flatten() {
                let Some(bone_name) = part_attach_bone(part) else {
                    continue;
                };
                let arc = format!("data/arc/{}", d.part_arc_name(part));
                let Some(reader) = ArcReader::open(&arc) else {
                    warnings.push(format!("{arc}: unreadable -- part skipped"));
                    continue;
                };
                let part_model = d.part_model_name(part);
                let Some(part_sk) = parse_skeleton(
                    &reader,
                    &format!("data/chara/{part_model}/{part_model}.model"),
                    &mut warnings,
                ) else {
                    continue;
                };
                let Some(attach) = bone_index(bone_name) else {
                    warnings.push(format!(
                        "{model_name}.b2it has no {bone_name} -- {part} skipped"
                    ));
                    continue;
                };
                parts.push(ParsedPart {
                    part: part.clone(),
                    model_name: part_model.clone(),
                    attach,
                    mirror: false,
                    skeleton: part_sk.clone(),
                });
                if part == MIRROR_PART {
                    match bone_index(MIRROR_ATTACH_BONE) {
                        Some(attach) => parts.push(ParsedPart {
                            part: part.clone(),
                            model_name: part_model,
                            attach,
                            mirror: true,
                            skeleton: part_sk,
                        }),
                        None => warnings.push(format!(
                            "{model_name}.b2it has no {MIRROR_ATTACH_BONE} -- right forearm skipped"
                        )),
                    }
                }
            }
        }
        let ground: Vec<usize> = GROUND_BONES.iter().filter_map(|n| bone_index(n)).collect();
        let hips = bone_index(HIPS_BONE);

        dancers.push(ParsedDancer {
            key: d.key.clone(),
            sex: d.sex,
            model_name,
            model_scale: d.model_scale,
            shadow_scale: d.shadow_scale,
            skeleton,
            seed,
            clips,
            parts,
            ground,
            hips,
        });
    }

    // The shared floor-shadow quad (rigid, one bone) — gameplay only.
    let shadow = if opts.shadow && dancers.iter().any(|d| !d.ground.is_empty()) {
        let arc = format!("data/arc/{SHADOW_ARC}");
        match ArcReader::open(&arc) {
            Some(reader) => parse_skeleton(
                &reader,
                &format!("data/chara/{SHADOW_MODEL}/{SHADOW_MODEL}.model"),
                &mut warnings,
            ),
            None => {
                warnings.push(format!("{arc}: unreadable -- no shadows"));
                None
            }
        }
    } else {
        None
    };

    // Stage camera sets (none without a stage — a dancer-only scene keeps
    // its caller's fixed camera silently). Each name is looked up in the
    // stage's OWN arc first (any `<name>.camanm` member — custom stages ship
    // their cameras that way), then in the stock `camera/stage_camera.arc`
    // under the `long/<name[..5]>/` layout.
    let cameras = match pick.stage.as_ref() {
        None => None,
        Some(stage) if pick.camera_main.is_empty() => {
            warnings.push(format!(
                "stage {} row {} has no camera set -- fixed camera",
                stage.key, stage.row
            ));
            None
        }
        Some(_) => {
            let own_member = |name: &str| -> Option<String> {
                let reader = stage_reader.as_ref()?;
                let file = format!("{name}.camanm");
                reader
                    .entries
                    .iter()
                    .find(|e| e.path.rsplit('/').next() == Some(file.as_str()))
                    .map(|e| e.path.clone())
            };
            let needs_stock = pick
                .camera_main
                .iter()
                .chain(pick.camera_non.iter())
                .any(|n| own_member(n).is_none());
            let stock_reader = if needs_stock {
                let r = ArcReader::open(STAGE_CAMERA_ARC);
                if r.is_none() {
                    warnings.push(format!(
                        "{STAGE_CAMERA_ARC}: unreadable -- stock camera names unavailable"
                    ));
                }
                r
            } else {
                None
            };
            let mut load = |names: &[String]| -> Vec<Clip> {
                names
                    .iter()
                    .filter_map(|n| match own_member(n) {
                        Some(member) => {
                            parse_clip(stage_reader.as_ref()?, &member, n, &mut warnings)
                        }
                        None => parse_clip(
                            stock_reader.as_ref()?,
                            &camanm_member_path(n),
                            n,
                            &mut warnings,
                        ),
                    })
                    .collect()
            };
            let main = load(&pick.camera_main);
            let non = load(&pick.camera_non);
            if main.is_empty() {
                warnings.push("no main camera clip parsed -- fixed camera".to_string());
                None
            } else {
                Some(ParsedCameras { main, non })
            }
        }
    };

    // The movie camera set: loose files read straight from disk (never
    // FileManager-loaded — only this parser reads camera clips).
    let movie_cameras = if pick.movie_camera_main.is_empty() {
        None
    } else {
        let mut load = |names: &[String]| -> Vec<Clip> {
            names
                .iter()
                .filter_map(|n| {
                    let path = super::movie_camera::clip_path(n);
                    let bytes = match std::fs::read(&path) {
                        Ok(b) => b,
                        Err(e) => {
                            warnings.push(format!("movie camera {}: {e}", path.display()));
                            return None;
                        }
                    };
                    match anmfile::parse(&bytes) {
                        Ok(anm) if anm.camera_slots.iter().any(Option::is_some) => Some(Clip {
                            name: n.clone(),
                            bytes: Arc::new(bytes),
                            anm,
                        }),
                        Ok(_) => {
                            warnings.push(format!("movie camera {n}: no camera chunk"));
                            None
                        }
                        Err(e) => {
                            warnings.push(format!("movie camera {n}: {e}"));
                            None
                        }
                    }
                })
                .collect()
        };
        let main = load(&pick.movie_camera_main);
        let non = load(&pick.movie_camera_non);
        if main.is_empty() {
            warnings.push(
                "no movie camera main clip parsed -- stage cameras behind the movie".to_string(),
            );
            None
        } else {
            Some(ParsedCameras { main, non })
        }
    };

    Parsed {
        stage_parts,
        dancers,
        shadow,
        cameras,
        movie_cameras,
        warnings,
        elapsed_ms: started.elapsed().as_millis() as u64,
    }
}

/// Which camera set a frame is filmed with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraSet {
    /// The stage row's `.camanm` set.
    Stage,
    /// The movie camera set (Background Movies = FULLSCREEN, movie live).
    Movie,
}

impl CameraSet {
    pub fn tag(self) -> &'static str {
        match self {
            CameraSet::Stage => "stage",
            CameraSet::Movie => "movie",
        }
    }
}

/// Seed salt of the movie camera schedule (its `_non` hold jitter must not
/// mirror the stage schedule's).
const MOVIE_CAMERA_SEED_SALT: u64 = 0x6D6F_7669_6563_616D; // "moviecam"

/// The camera schedule over the parsed camera clips (`None` without one).
pub fn camera_schedule(parsed: &Parsed, seed: u64) -> Option<CameraSchedule> {
    schedule_over(parsed.cameras.as_ref()?, seed)
}

/// The movie camera schedule (`None` without a movie camera set).
pub fn movie_camera_schedule(parsed: &Parsed, seed: u64) -> Option<CameraSchedule> {
    schedule_over(
        parsed.movie_cameras.as_ref()?,
        seed ^ MOVIE_CAMERA_SEED_SALT,
    )
}

fn schedule_over(c: &ParsedCameras, seed: u64) -> Option<CameraSchedule> {
    CameraSchedule::new(
        c.main.iter().map(Clip::clip_ref).collect(),
        c.non.iter().map(Clip::clip_ref).collect(),
        seed,
    )
}

/// The dance schedule over the parsed dancers (playlist = the clips that
/// parsed, in order). `None` without a dancer. In BPM-sync mode the
/// segment lengths snap to whole beats of dance time so every cut lands on
/// a chart beat.
pub fn dance_schedule(parsed: &Parsed, opts: TempoOptions) -> Option<DanceSchedule> {
    let d = DanceSchedule::new(
        parsed
            .dancers
            .iter()
            .map(|d| d.clips.iter().map(Clip::clip_ref).collect())
            .collect(),
    )?;
    Some(if opts.bpm_sync {
        d.with_quantum(BEAT_TAU)
    } else {
        d
    })
}

// ---------------------------------------------------------------------------
// Instances (game thread)
// ---------------------------------------------------------------------------

pub use super::instance_plan::{restyle_allowed, Instance, InstanceKind, InstanceStatus};

// The pure planner's "no slot" sentinel must be the frame board's.
const _: () = assert!(super::instance_plan::NO_SLOT == NO_SLOT);

pub struct BuildProgress {
    pub built_now: usize,
    pub pending: usize,
    pub skipped_now: usize,
}

/// Everything one song window owns.
pub struct Session {
    pub pick: Pick,
    pub parsed: Parsed,
    /// The scene style every eligible material is re-pointed at (per song).
    pub style: SceneStyle,
    /// The outline layers built for every restyle-eligible instance (empty =
    /// no hulls; INK = one grey layer; LAYERED = one hull per palette
    /// colour, one band wider each — `outline.rs`). Frozen per song.
    pub hulls: HullPlan,
    pub schedule: Option<DanceSchedule>,
    pub instances: Vec<Instance>,
    /// Per dancer: the instance indices of its parts and shadow (the
    /// director derives them from the dancer's bones without re-scanning).
    pub children: Vec<Vec<usize>>,
    /// Per dancer: the shadow's low-passed size (A3 `prev += 0.1·(target −
    /// prev)`), reset at every song (re)start.
    pub shadow_size: Vec<f32>,
    /// The stage camera sequencing (None ⇒ the fixed fallback camera).
    pub camera: Option<CameraSchedule>,
    /// The camera event-loop state + the song time it was advanced to;
    /// `None` ⇒ re-simulate from 0 at the next frame (song (re)start).
    pub camera_state: Option<(CameraState, f32)>,
    /// The movie camera sequencing (None ⇒ the stage cameras film the
    /// movie backdrop too) and its own event-loop state.
    pub movie_camera: Option<CameraSchedule>,
    pub movie_camera_state: Option<(CameraState, f32)>,
    /// Per-frame evaluation scratch (sized once).
    pub scratch: Vec<Trs>,
    pub bones: Vec<Mat4>,
    pub requested_at: Instant,
    pub built_at: Option<Instant>,
}

impl Session {
    /// `style`: the scene style applied at item build (materials re-pointed
    /// at the `<name>_<style>` variant containers, RE §4.7). `hulls`: the
    /// inverted-hull outline layers to build for every restyle-eligible
    /// instance — empty when SCENE OUTLINES is off, the style is stock or the
    /// synthesized containers lack the outline pair (`style::hull_plan`
    /// decides); INK = one layer, LAYERED = one hull per palette colour.
    /// `slot_base`: the first frame-board slot (gameplay 0; P1 previews 0,
    /// P2 previews 16 — design §5.3); the owner budget is what remains of
    /// the board above it. `item_pass_mask`: `Some(bit)` stamps every
    /// instance with that node-mask bit (a preview's private pass clones),
    /// `None` keeps the stock stage / lowprio / dancer masks.
    pub fn new(
        pick: Pick,
        parsed: Parsed,
        requested_at: Instant,
        tempo_opts: TempoOptions,
        style: SceneStyle,
        hulls: HullPlan,
        slot_base: u32,
        item_pass_mask: Option<u32>,
    ) -> Session {
        let schedule = dance_schedule(&parsed, tempo_opts);
        let camera = camera_schedule(&parsed, pick.seed);
        let movie_camera = movie_camera_schedule(&parsed, pick.seed);
        let input = PlanInput {
            stage_parts: parsed
                .stage_parts
                .iter()
                .map(|p| StagePartSpec {
                    model_name: p.model_name.clone(),
                    priority: p.priority,
                    bone_count: p.skeleton.bone_count(),
                })
                .collect(),
            dancers: parsed
                .dancers
                .iter()
                .map(|d| DancerSpec {
                    model_name: d.model_name.clone(),
                    bone_count: d.skeleton.bone_count(),
                    parts: d
                        .parts
                        .iter()
                        .map(|p| PartSpec {
                            model_name: p.model_name.clone(),
                            mirror: p.mirror,
                            bone_count: p.skeleton.bone_count(),
                        })
                        .collect(),
                    has_ground: !d.ground.is_empty(),
                })
                .collect(),
            shadow_bone_count: parsed.shadow.as_ref().map(|s| s.bone_count()),
            shadow_model: SHADOW_MODEL.to_string(),
        };
        let slot_budget = frame_board::MAX_INSTANCES.saturating_sub(slot_base as usize);
        let plan = plan_instances(
            &input,
            PassMasks {
                stage: PASS_MASK_STAGE,
                lowprio: PASS_MASK_LOWPRIO,
                dancer: PASS_MASK_DANCER,
            },
            slot_base,
            slot_budget,
            hulls.layers.len(),
            item_pass_mask,
        );
        if plan.truncated > 0 {
            log_warn!(
                "BackgroundDancers: {} instances exceed the frame board ({}) -- the tail is skipped",
                plan.instances.iter().filter(|i| i.kind.owns_slot()).count(),
                slot_budget
            );
        }
        let shadow_size = parsed.dancers.iter().map(|d| d.shadow_scale).collect();
        Session {
            pick,
            parsed,
            style,
            hulls,
            schedule,
            instances: plan.instances,
            children: plan.children,
            shadow_size,
            camera,
            camera_state: None,
            movie_camera,
            movie_camera_state: None,
            scratch: vec![Trs::IDENTITY; plan.max_bones],
            bones: vec![IDENTITY; plan.max_bones],
            requested_at,
            built_at: None,
        }
    }

    /// Install `fallback` as the dance schedule when the parse produced none
    /// (a stage-only scene has no dancers): the camera event loop needs cut
    /// times — `schedule::synthetic_schedule` supplies them.
    pub fn with_schedule(mut self, fallback: DanceSchedule) -> Session {
        if self.schedule.is_none() {
            self.schedule = Some(fallback);
        }
        self
    }

    pub fn dancer_count(&self) -> usize {
        self.parsed.dancers.len()
    }

    pub fn has_camera(&self) -> bool {
        self.camera.is_some()
    }

    pub fn has_movie_camera(&self) -> bool {
        self.movie_camera.is_some()
    }

    /// Whether `set` can film this session.
    pub fn has_camera_set(&self, set: CameraSet) -> bool {
        match set {
            CameraSet::Stage => self.has_camera(),
            CameraSet::Movie => self.has_movie_camera(),
        }
    }

    /// Song (re)start: both camera event loops re-simulate from 0.
    pub fn reset_camera(&mut self) {
        self.camera_state = None;
        self.movie_camera_state = None;
    }

    /// A3 resets the shadow low-pass at song start: back to the rlist scale.
    pub fn reset_shadow(&mut self) {
        for (i, d) in self.parsed.dancers.iter().enumerate() {
            if let Some(s) = self.shadow_size.get_mut(i) {
                *s = d.shadow_scale;
            }
        }
    }

    /// Instance counts by kind (built ones) for the `built` INFO:
    /// `(stage parts, dancers, parts, shadows, hulls)`.
    pub fn built_counts(&self) -> (usize, usize, usize, usize, usize) {
        let mut c = (0, 0, 0, 0, 0);
        for i in self.built() {
            match i.kind {
                InstanceKind::StagePart(_) => c.0 += 1,
                InstanceKind::Dancer(_) => c.1 += 1,
                InstanceKind::Part { .. } => c.2 += 1,
                InstanceKind::Shadow(_) => c.3 += 1,
                InstanceKind::Hull { .. } => c.4 += 1,
            }
        }
        c
    }

    /// Instances that reached `Built` (their nodes are live until torn down).
    pub fn built(&self) -> impl Iterator<Item = &Instance> {
        self.instances
            .iter()
            .filter(|i| i.status == InstanceStatus::Built)
    }

    pub fn built_mut(&mut self) -> impl Iterator<Item = &mut Instance> {
        self.instances
            .iter_mut()
            .filter(|i| i.status == InstanceStatus::Built)
    }

    pub fn all_settled(&self) -> bool {
        self.instances
            .iter()
            .all(|i| i.status != InstanceStatus::Pending)
    }

    /// The world matrix an instance is built with (also what the director
    /// republishes every frame).
    pub fn initial_world(&self, inst: &Instance) -> Mat4 {
        match inst.kind {
            InstanceKind::Hull { of, .. } => match self.instances.get(of) {
                Some(body) if !matches!(body.kind, InstanceKind::Hull { .. }) => {
                    self.initial_world(body)
                }
                _ => IDENTITY,
            },
            InstanceKind::StagePart(_) => IDENTITY,
            InstanceKind::Dancer(i) => self.dancer_body_world(i),
            InstanceKind::Part { dancer, part } => {
                // Rest pose: the part on its bind bone.
                let body = self.dancer_body_world(dancer);
                match self
                    .parsed
                    .dancers
                    .get(dancer)
                    .and_then(|d| d.parts.get(part).map(|p| (d, p)))
                {
                    Some((d, p)) => {
                        let bind = d.skeleton.bind_world.get(p.attach).unwrap_or(&IDENTITY);
                        part_world(p.mirror, bind, &body)
                    }
                    None => body,
                }
            }
            InstanceKind::Shadow(i) => {
                let body = self.dancer_body_world(i);
                let Some(d) = self.parsed.dancers.get(i) else {
                    return body;
                };
                let ground: Vec<[f32; 3]> = d
                    .ground
                    .iter()
                    .filter_map(|&g| d.skeleton.bind_world.get(g))
                    .map(|m| [m[12], m[13], m[14]])
                    .collect();
                let centre = shadow_target(&ground, 0.0, 0.0, d.shadow_scale)
                    .map(|(c, _)| c)
                    .unwrap_or([0.0, SHADOW_FLOOR_Y, 0.0]);
                shadow_world(d.shadow_scale, transform_point(&body, centre))
            }
        }
    }

    /// `diag(s) · T(x, 0, 0)` for dancer `i` (A3 placement).
    pub fn dancer_body_world(&self, i: usize) -> Mat4 {
        let n = self.parsed.dancers.len();
        let s = self
            .parsed
            .dancers
            .get(i)
            .map(|d| d.model_scale)
            .unwrap_or(1.0);
        body_world(s, i, n)
    }

    /// Game thread: try to build every pending instance whose model is
    /// resident (and textured, or past the texture timeout). Attach HIDDEN.
    pub fn build_pending(&mut self, since_request_ms: u64) -> BuildProgress {
        let mut progress = BuildProgress {
            built_now: 0,
            pending: 0,
            skipped_now: 0,
        };
        for idx in 0..self.instances.len() {
            if self.instances[idx].status != InstanceStatus::Pending {
                continue;
            }
            let name = self.instances[idx].model_name.clone();
            let Some(res) = model_registry::model_resource(&name) else {
                progress.pending += 1;
                continue;
            };
            let Some(view) = model_registry::ResourceView::new(res) else {
                log_warn!(
                    "BackgroundDancers: {} resource header unreadable -- skipped this song",
                    name
                );
                self.instances[idx].status = InstanceStatus::Skipped;
                progress.skipped_now += 1;
                continue;
            };
            if view.bone_count() as usize != self.instances[idx].bone_count {
                log_warn!(
                    "BackgroundDancers: {} resident with {} bones but the .model file has {} -- skipped (bone array would not match the upload)",
                    name,
                    view.bone_count(),
                    self.instances[idx].bone_count
                );
                self.instances[idx].status = InstanceStatus::Skipped;
                progress.skipped_now += 1;
                continue;
            }
            let ready = render_item::texture_readiness(&view);
            if ready.still_default > 0 && since_request_ms < TEXTURE_WAIT_TIMEOUT_MS {
                progress.pending += 1;
                continue; // wait for the DDS (Step 3 cold-load finding)
            }
            if ready.still_default > 0 {
                log_warn!(
                    "BackgroundDancers: {} -- {} of {} texture(s) still unregistered after {} ms, building anyway (per-frame retry)",
                    name,
                    ready.still_default,
                    ready.total,
                    since_request_ms
                );
            }
            let world = self.initial_world(&self.instances[idx]);
            match build_one(
                &mut self.instances[idx],
                &view,
                &world,
                since_request_ms,
                self.style,
                &self.hulls,
            ) {
                true => progress.built_now += 1,
                false => progress.skipped_now += 1,
            }
        }
        progress
    }
}

/// Build one instance's item + node and attach it hidden. `false` = skipped
/// (one WARN logged, status set). `hulls` = the session's outline plan (a
/// `Hull { layer }` instance takes its colour + width step from it).
fn build_one(
    inst: &mut Instance,
    view: &model_registry::ResourceView,
    world: &Mat4,
    since_request_ms: u64,
    style: SceneStyle,
    hulls: &HullPlan,
) -> bool {
    let item = match render_item::build(view, inst.pass_mask) {
        Ok(i) => i,
        Err(e) => {
            log_warn!(
                "BackgroundDancers: render_item::build({}) failed: {:?} -- skipped this song",
                inst.model_name,
                e
            );
            inst.status = InstanceStatus::Skipped;
            return false;
        }
    };
    let ts = item.textures;
    let tex = item.bone_textures();
    // Stage screens (Background Movies = STAGE SCREENS, research §8): a
    // material sampling `offscreen1` — the movie render target — keeps its
    // stock unlit shader in every style and gets no outline (the hull twin
    // hides records whose material stayed stock).
    let screen_hash = pure::fnv1_name_hash(super::movie_mode::SCREEN_TEXTURE_STEM);
    if !matches!(inst.kind, InstanceKind::Hull { .. }) {
        // SAFETY: our own fresh block, not yet attached.
        let screens = unsafe { item.materials_sampling(screen_hash) };
        let n = screens.iter().filter(|b| **b).count();
        if n > 0 {
            // SAFETY: as above (the bound TextureData is probed).
            let info = unsafe { item.sampled_texture_info(screen_hash) };
            log_info!(
                "BackgroundDancers: {} [{}] {} screen material(s) sample 'offscreen1' -- kept stock (unlit, no outline); bound texture {}",
                inst.model_name,
                inst.kind.tag(),
                n,
                match info {
                    Some((w, h, hash)) => format!(
                        "{}x{} hash 0x{:08X}{}",
                        w,
                        h,
                        hash,
                        if hash == screen_hash {
                            ""
                        } else {
                            " (NOT the offscreen1 key)"
                        }
                    ),
                    None => "unreadable".to_string(),
                }
            );
        }
    }
    // Whole-scene restyle (RE §4.7): re-point eligible material copies at the
    // `<name>_<style>` variant objects. Hull twins ALWAYS restyle (their
    // program 0 is the outline pair) and then hide every record whose
    // material stayed stock.
    let allows = restyle_allowed(&inst.kind, &inst.model_name);
    if style != SceneStyle::Stock && allows {
        let variant_for = |stock_hash: u32| -> Option<*mut u8> {
            let v = shader_layout::variant_for_stock_hash(stock_hash)?;
            let name = shader_layout::variant_container_name(v, style)?;
            texture::lookup_shader(shader_layout::fnv1_32(&name))
        };
        // SAFETY: our own fresh block, not yet attached.
        let st = unsafe { item.restyle_materials(true, screen_hash, &variant_for) };
        if let InstanceKind::Hull { layer, .. } = inst.kind {
            // Per-kind rim width (the hull VS reads ModelParameters.w): the
            // twin's model_name is the body's, so a `gm_` prefix = stage prop.
            // The layer's colour rides the records (collector → c23 → the
            // outline PS emits it verbatim); its width is the base plus
            // `step` bands (`outline.rs`). A layer index outside the plan
            // can only come from a plan/instances mismatch — draw nothing.
            let (px_dancer, px_stage) = super::style::outline_widths();
            let base = if inst.model_name.starts_with("gm_") {
                px_stage
            } else {
                px_dancer
            };
            let Some(spec) = hulls.layers.get(layer) else {
                log_warn!(
                    "BackgroundDancers: {} [hull L{}] has no layer in the plan ({} layer(s)) -- item freed, skipped this song",
                    inst.model_name,
                    layer,
                    hulls.layers.len()
                );
                render_item::free(item);
                inst.status = InstanceStatus::Skipped;
                return false;
            };
            let px = hulls.width(base, spec.step);
            // SAFETY: as above.
            let (marked, hidden) = unsafe {
                item.set_outline_width(px);
                item.set_record_colors(spec.rgba);
                item.mark_hull_records(&st.record_restyled)
            };
            log_info!(
                "BackgroundDancers: {} [hull L{} {}] {} record(s) marked bit-31 (program 0 = outline pair), {} hidden (blended / stock material), rim {:.2} px; materials restyled={} kept: blend={} no-variant={} screen={}",
                inst.model_name,
                layer,
                outline::hex(spec.rgba),
                marked,
                hidden,
                px,
                st.restyled,
                st.kept_blend,
                st.kept_no_variant,
                st.kept_screen
            );
        } else {
            log_info!(
                "BackgroundDancers: {} [{}] style {} -- materials restyled={} kept: blend={} no-variant={} screen={}",
                inst.model_name,
                inst.kind.tag(),
                style.key(),
                st.restyled,
                st.kept_blend,
                st.kept_no_variant,
                st.kept_screen
            );
        }
    } else if matches!(inst.kind, InstanceKind::Hull { .. }) {
        // A hull whose body cannot be restyled must not draw at all.
        // SAFETY: our own fresh block, not yet attached.
        let (marked, hidden) = unsafe { item.mark_hull_records(&[]) };
        log_info!(
            "BackgroundDancers: {} [hull] {} record(s) hidden (no restyle for this instance)",
            inst.model_name,
            hidden.max(marked)
        );
    }
    log_info!(
        "BackgroundDancers: {} [{}{}] item built at 0x{:X} {} ms after request (mode=0x{:X} bones={} records={} mats={} pals={} skinned={} bone_tex=0x{:X}/0x{:X} textures total={} load={} re={} default={} slot={} pass=0x{:X} sort={})",
        inst.model_name,
        inst.kind.tag(),
        if inst.mirror { " mirrored" } else { "" },
        item.ptr() as usize,
        since_request_ms,
        item.mode(),
        item.counts().bones,
        item.counts().draw_records,
        item.counts().materials,
        item.counts().palettes,
        item.is_skinned(),
        tex[0],
        tex[1],
        ts.total,
        ts.resolved_at_load,
        ts.re_resolved,
        ts.still_default,
        inst.slot,
        inst.pass_mask,
        inst.sort_key
    );
    // SAFETY: our own fresh block.
    unsafe {
        item.set_world(world);
        item.set_tint(if matches!(inst.kind, InstanceKind::Shadow(_)) {
            BLACK
        } else {
            WHITE
        });
        item.set_hidden(true);
    }
    let item_ptr = item.ptr() as usize;
    let material_count = item.counts().materials;
    let n = match node::new_node(item, inst.pass_mask, inst.sort_key, inst.slot) {
        Ok(n) => n,
        Err(item) => {
            render_item::free(item);
            log_warn!(
                "BackgroundDancers: node build failed for {} -- item freed, skipped this song",
                inst.model_name
            );
            inst.status = InstanceStatus::Skipped;
            return false;
        }
    };
    // Hidden until the director shows it (pass 4 honours the node flag).
    // SAFETY: fresh node, not yet attached.
    unsafe { node::set_hidden(n, true) };
    if !scene_graph::attach_under_root(n) {
        // SAFETY: never attached — our own dtor path is the only owner.
        unsafe { node::destroy_unattached(n) };
        log_warn!(
            "BackgroundDancers: attach_under_root refused for {} (manager/graph unreadable or lock unavailable) -- node destroyed, skipped this song",
            inst.model_name
        );
        inst.status = InstanceStatus::Skipped;
        return false;
    }
    inst.node = n as usize;
    inst.item = item_ptr;
    inst.material_count = material_count;
    inst.textures_pending = ts.still_default;
    inst.attached_at = Some(Instant::now());
    inst.status = InstanceStatus::Built;
    true
}

/// Used by `initial_world` callers that only have the scale (tests, Step 8).
pub fn scaled_at(scale: f32, x: f32) -> Mat4 {
    scale_translation(scale, x, 0.0, 0.0)
}
