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
use crate::services::scene3d::{arc_set, model_registry, node, render_item, scene_graph, texture};
use crate::{log_info, log_warn};

use super::director_math::{
    body_world, part_world, shadow_target, shadow_world, transform_point, BLACK, SHADOW_FLOOR_Y,
    WHITE,
};
use super::schedule::{CameraSchedule, CameraState, ClipRef, DanceSchedule};
use super::selection::{
    camanm_member_path, camera_lists, clip_member_path, dancer_x, part_attach_bone, pick_dancers,
    pick_stage, playlist, DancerCandidate, Rng, Sex, StageCandidate, GROUND_BONES, HIPS_BONE,
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
// Pick
// ---------------------------------------------------------------------------

/// What one song window shows. Immutable once made.
#[derive(Debug, Clone)]
pub struct Pick {
    pub seed: u64,
    pub stage: StageCandidate,
    /// `stage_camera_resources.rlist` fields of the stage's ROW.
    pub camera_row: Vec<String>,
    /// The row's camera names split + shuffled (A3 stage mode): the main
    /// cycle and the `_non` cut-aways.
    pub camera_main: Vec<String>,
    pub camera_non: Vec<String>,
    /// Side order: index 0 = the left dancer.
    pub dancers: Vec<DancerCandidate>,
    /// Per dancer: shuffled clip names (`mc_<sex>_<name>_exec`).
    pub playlists: Vec<Vec<String>>,
    /// Per dancer: the accessory parts whose arc exists (`head00`, …).
    pub parts: Vec<Vec<String>>,
    /// Selection came from `DDR_DANCERS_PIN`.
    pub pinned: bool,
}

impl Pick {
    /// Game-relative arc paths, deduplicated, in load order (stage first).
    pub fn arcs(&self) -> Vec<String> {
        let mut out: Vec<String> = vec![format!("data/arc/{}", self.stage.arc_name())];
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
        if !self.dancers.is_empty() {
            let a = format!("data/arc/{SHADOW_ARC}");
            if !out.contains(&a) {
                out.push(a);
            }
        }
        out
    }

    /// The one-line per-song INFO.
    pub fn summary(&self) -> String {
        let n = self.dancers.len();
        let dancers: Vec<String> = self
            .dancers
            .iter()
            .enumerate()
            .map(|(i, d)| {
                format!(
                    "{}({}) x={:+.1}",
                    d.key,
                    match d.sex {
                        Sex::Male => "M",
                        Sex::Female => "F",
                    },
                    dancer_x(i, n)
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
        format!(
            "stage={}[{}] parts={} dancers=[{}] clips=[{}] wear=[{}] cameras=main:{} non:{} arcs={} seed=0x{:X}{}",
            self.stage.key,
            self.stage.row,
            self.stage.parts.len(),
            dancers.join(", "),
            clips.join(" | "),
            parts.join(" | "),
            self.camera_main.len(),
            self.camera_non.len(),
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

/// Finish a pick whose stage/dancers are already chosen (random or pinned):
/// camera row + playlists + the part arcs that exist (`arc_exists` takes an
/// arc file name like `pl_emi00_face01.arc`).
pub fn assemble_pick(
    rng: &mut Rng,
    stage: StageCandidate,
    camera_rows: &[(String, Vec<String>)],
    dancers: Vec<DancerCandidate>,
    pinned: bool,
    arc_exists: impl Fn(&str) -> bool,
) -> Pick {
    let camera_row = camera_rows
        .get(stage.row)
        .map(|(_, f)| f.clone())
        .unwrap_or_default();
    let playlists = dancers.iter().map(|d| playlist(rng, d.sex)).collect();
    let parts = dancers
        .iter()
        .map(|d| d.parts_present(&arc_exists))
        .collect();
    let (camera_main, camera_non) = camera_lists(rng, &camera_row);
    Pick {
        seed: 0,
        stage,
        camera_row,
        camera_main,
        camera_non,
        dancers,
        playlists,
        parts,
        pinned,
    }
}

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
pub fn parse_pick(pick: &Pick) -> Parsed {
    let started = Instant::now();
    let mut warnings = Vec::new();
    let mut stage_parts = Vec::new();
    let mut dancers = Vec::new();

    // Stage parts.
    let stage_arc = format!("data/arc/{}", pick.stage.arc_name());
    match ArcReader::open(&stage_arc) {
        None => warnings.push(format!("{stage_arc}: unreadable -- no stage this song")),
        Some(reader) => {
            for (part, priority) in &pick.stage.parts {
                let model_name = format!("gm_{}_{}", pick.stage.key, part);
                let dir = format!("data/map/{model_name}");
                let Some(skeleton) =
                    parse_skeleton(&reader, &format!("{dir}/{model_name}.model"), &mut warnings)
                else {
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

    // The shared floor-shadow quad (rigid, one bone).
    let shadow = if dancers.iter().any(|d| !d.ground.is_empty()) {
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

    // Stage camera sets.
    let cameras = if pick.camera_main.is_empty() {
        warnings.push(format!(
            "stage {} row {} has no camera set -- fixed camera",
            pick.stage.key, pick.stage.row
        ));
        None
    } else {
        match ArcReader::open(STAGE_CAMERA_ARC) {
            None => {
                warnings.push(format!("{STAGE_CAMERA_ARC}: unreadable -- fixed camera"));
                None
            }
            Some(reader) => {
                let mut load = |names: &[String]| -> Vec<Clip> {
                    names
                        .iter()
                        .filter_map(|n| {
                            parse_clip(&reader, &camanm_member_path(n), n, &mut warnings)
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
        }
    };

    Parsed {
        stage_parts,
        dancers,
        shadow,
        cameras,
        warnings,
        elapsed_ms: started.elapsed().as_millis() as u64,
    }
}

/// The camera schedule over the parsed camera clips (`None` without one).
pub fn camera_schedule(parsed: &Parsed, seed: u64) -> Option<CameraSchedule> {
    let c = parsed.cameras.as_ref()?;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstanceKind {
    /// Index into `Parsed::stage_parts`.
    StagePart(usize),
    /// Index into `Parsed::dancers` (= side order).
    Dancer(usize),
    /// `Parsed::dancers[dancer].parts[part]` — a rigid part following one
    /// body bone.
    Part { dancer: usize, part: usize },
    /// The `pl_shadow00` quad under dancer `dancer`.
    Shadow(usize),
    /// Inverted-hull OUTLINE twin of instance `of` (a `Dancer` or `Part`):
    /// the same model/resource, every draw record carrying the bit-31
    /// program selector so the synthesized `mdl_*_lambert` container's
    /// outline pair (program 0) draws it. Shares `of`'s frame-board slot —
    /// never published itself (RE §4.6).
    Hull { of: usize },
}

impl InstanceKind {
    /// Short tag for the built/skipped log lines.
    pub fn tag(&self) -> &'static str {
        match self {
            InstanceKind::StagePart(_) => "stage",
            InstanceKind::Dancer(_) => "dancer",
            InstanceKind::Part { .. } => "part",
            InstanceKind::Shadow(_) => "shadow",
            InstanceKind::Hull { .. } => "hull",
        }
    }

    /// Hull twins read their body's board slot; everything else owns one.
    pub fn owns_slot(&self) -> bool {
        !matches!(self, InstanceKind::Hull { .. })
    }
}

/// Whether an instance takes the scene style at all (RE §4.7): never the
/// floor shadow (a black `mdl_bg_constant` quad — lighting it is nonsense)
/// and never a stage part whose model is the `_bg` skydome/backdrop (an
/// inward-facing dome under a directional light gets a gradient across the
/// sky). Hull twins inherit their body's verdict. Per-material blend-group
/// exclusions are applied on top by `render_item::restyle_materials`.
pub fn restyle_allowed(kind: &InstanceKind, model_name: &str) -> bool {
    match kind {
        InstanceKind::Shadow(_) => false,
        InstanceKind::StagePart(_) => !model_name.ends_with("_bg"),
        InstanceKind::Dancer(_) | InstanceKind::Part { .. } => true,
        // The twin's model_name is the body's; a Hull of a Hull never exists.
        InstanceKind::Hull { .. } => !model_name.ends_with("_bg") && model_name != SHADOW_MODEL,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstanceStatus {
    /// Waiting for residency / textures.
    Pending,
    /// Item + node attached (hidden until the director shows it).
    Built,
    /// Gave up on this instance for the song (one WARN was logged).
    Skipped,
}

pub struct Instance {
    pub kind: InstanceKind,
    pub model_name: String,
    pub pass_mask: u32,
    pub sort_key: i32,
    /// Frame-board slot (== index in `Session::instances`).
    pub slot: u32,
    /// Log tag: the mirrored right-forearm copy.
    pub mirror: bool,
    pub status: InstanceStatus,
    /// `*mut SceneNode` as usize (Send).
    pub node: usize,
    /// The item pointer the node owns (list scan / retry / diagnostics).
    pub item: usize,
    pub bone_count: usize,
    pub material_count: usize,
    /// Texture-table entries still on the default texture (retry while > 0).
    pub textures_pending: usize,
    pub attached_at: Option<Instant>,
    /// The node-level "force hidden" it was attached with has been dropped
    /// (after the instance's first frame-board publish — the board's own
    /// hidden bit is the visibility control from then on).
    pub node_shown: bool,
    /// Teardown bookkeeping (the spike's `Slot` fields).
    pub queued: bool,
    pub freed: bool,
}

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
    /// Per-frame evaluation scratch (sized once).
    pub scratch: Vec<Trs>,
    pub bones: Vec<Mat4>,
    pub requested_at: Instant,
    pub built_at: Option<Instant>,
}

impl Session {
    /// `style`: the scene style applied at item build (materials re-pointed
    /// at the `<name>_<style>` variant containers, RE §4.7). `hulls`: build an
    /// inverted-hull outline twin for every restyle-eligible instance (SCENE
    /// OUTLINES on AND a non-stock style AND the synthesized containers carry
    /// the outline pair — `style::effective()` decides).
    pub fn new(
        pick: Pick,
        parsed: Parsed,
        requested_at: Instant,
        tempo_opts: TempoOptions,
        style: SceneStyle,
        hulls: bool,
    ) -> Session {
        let schedule = dance_schedule(&parsed, tempo_opts);
        let camera = camera_schedule(&parsed, pick.seed);
        let mut instances = Vec::new();
        let mut max_bones = 1usize;
        for (i, p) in parsed.stage_parts.iter().enumerate() {
            let (pass_mask, sort_key) = match p.priority {
                Some(prio) => (PASS_MASK_LOWPRIO, prio),
                None => (PASS_MASK_STAGE, 0),
            };
            max_bones = max_bones.max(p.skeleton.bone_count());
            instances.push(Instance {
                kind: InstanceKind::StagePart(i),
                model_name: p.model_name.clone(),
                pass_mask,
                sort_key,
                slot: instances.len() as u32,
                mirror: false,
                status: InstanceStatus::Pending,
                node: 0,
                item: 0,
                bone_count: p.skeleton.bone_count(),
                material_count: 0,
                textures_pending: 0,
                attached_at: None,
                node_shown: false,
                queued: false,
                freed: false,
            });
        }
        for (i, d) in parsed.dancers.iter().enumerate() {
            max_bones = max_bones.max(d.skeleton.bone_count());
            instances.push(Instance {
                kind: InstanceKind::Dancer(i),
                model_name: d.model_name.clone(),
                pass_mask: PASS_MASK_DANCER,
                sort_key: 0,
                slot: instances.len() as u32,
                mirror: false,
                status: InstanceStatus::Pending,
                node: 0,
                item: 0,
                bone_count: d.skeleton.bone_count(),
                material_count: 0,
                textures_pending: 0,
                attached_at: None,
                node_shown: false,
                queued: false,
                freed: false,
            });
        }
        // Build order (design §4.3.5): stage parts, dancers, parts, shadows.
        let mut children: Vec<Vec<usize>> = vec![Vec::new(); parsed.dancers.len()];
        for (i, d) in parsed.dancers.iter().enumerate() {
            for (k, p) in d.parts.iter().enumerate() {
                children[i].push(instances.len());
                instances.push(Instance {
                    kind: InstanceKind::Part { dancer: i, part: k },
                    model_name: p.model_name.clone(),
                    pass_mask: PASS_MASK_DANCER,
                    sort_key: 0,
                    slot: instances.len() as u32,
                    mirror: p.mirror,
                    status: InstanceStatus::Pending,
                    node: 0,
                    item: 0,
                    bone_count: p.skeleton.bone_count(),
                    material_count: 0,
                    textures_pending: 0,
                    attached_at: None,
                    node_shown: false,
                    queued: false,
                    freed: false,
                });
            }
        }
        if let Some(shadow) = parsed.shadow.as_ref() {
            for (i, d) in parsed.dancers.iter().enumerate() {
                if d.ground.is_empty() {
                    continue;
                }
                children[i].push(instances.len());
                instances.push(Instance {
                    kind: InstanceKind::Shadow(i),
                    model_name: SHADOW_MODEL.to_string(),
                    pass_mask: PASS_MASK_DANCER,
                    sort_key: 0,
                    slot: instances.len() as u32,
                    mirror: false,
                    status: InstanceStatus::Pending,
                    node: 0,
                    item: 0,
                    bone_count: shadow.bone_count(),
                    material_count: 0,
                    textures_pending: 0,
                    attached_at: None,
                    node_shown: false,
                    queued: false,
                    freed: false,
                });
            }
        }
        let shadow_size = parsed.dancers.iter().map(|d| d.shadow_scale).collect();
        // Slot budget: every instance so far OWNS a board slot (its index).
        let slot_owners = instances.len();
        if slot_owners > frame_board::MAX_INSTANCES {
            log_warn!(
                "BackgroundDancers: {} instances exceed the frame board ({}) -- the tail is skipped",
                slot_owners,
                frame_board::MAX_INSTANCES
            );
            for inst in instances.iter_mut().skip(frame_board::MAX_INSTANCES) {
                inst.status = InstanceStatus::Skipped;
                inst.slot = NO_SLOT;
            }
        }
        // Inverted-hull twins (scene outlines, RE §4.6/§4.7): one per
        // restyle-eligible instance (dancer bodies, parts, stage props),
        // reading the body's slot. Never for the shadow or the skydome part
        // (their materials stay stock, so program 0 of their container is the
        // body again) — `restyle_allowed` is the same rule the restyle uses.
        // (Deploy #4 shipped a Dancer|Part-only filter here — a `cargo fmt`
        // reflow had defeated the edit — so stage props never got twins.)
        if hulls {
            let n = instances.len();
            for of in 0..n {
                let body = &instances[of];
                if !restyle_allowed(&body.kind, &body.model_name)
                    || body.status == InstanceStatus::Skipped
                {
                    continue;
                }
                let twin = Instance {
                    kind: InstanceKind::Hull { of },
                    model_name: body.model_name.clone(),
                    pass_mask: body.pass_mask,
                    sort_key: body.sort_key,
                    slot: body.slot,
                    mirror: body.mirror,
                    status: InstanceStatus::Pending,
                    node: 0,
                    item: 0,
                    bone_count: body.bone_count,
                    material_count: 0,
                    textures_pending: 0,
                    attached_at: None,
                    node_shown: false,
                    queued: false,
                    freed: false,
                };
                instances.push(twin);
            }
        }
        Session {
            pick,
            parsed,
            style,
            schedule,
            instances,
            children,
            shadow_size,
            camera,
            camera_state: None,
            scratch: vec![Trs::IDENTITY; max_bones],
            bones: vec![IDENTITY; max_bones],
            requested_at,
            built_at: None,
        }
    }

    pub fn dancer_count(&self) -> usize {
        self.parsed.dancers.len()
    }

    pub fn has_camera(&self) -> bool {
        self.camera.is_some()
    }

    /// Song (re)start: the camera event loop re-simulates from 0.
    pub fn reset_camera(&mut self) {
        self.camera_state = None;
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
            InstanceKind::Hull { of } => match self.instances.get(of) {
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
            ) {
                true => progress.built_now += 1,
                false => progress.skipped_now += 1,
            }
        }
        progress
    }
}

/// Build one instance's item + node and attach it hidden. `false` = skipped
/// (one WARN logged, status set).
fn build_one(
    inst: &mut Instance,
    view: &model_registry::ResourceView,
    world: &Mat4,
    since_request_ms: u64,
    style: SceneStyle,
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
        let st = unsafe { item.restyle_materials(true, &variant_for) };
        let is_hull = matches!(inst.kind, InstanceKind::Hull { .. });
        if is_hull {
            // Per-kind rim width (the hull VS reads ModelParameters.w): the
            // twin's model_name is the body's, so a `gm_` prefix = stage prop.
            let (px_dancer, px_stage) = super::style::outline_widths();
            let px = if inst.model_name.starts_with("gm_") {
                px_stage
            } else {
                px_dancer
            };
            // SAFETY: as above.
            let (marked, hidden) = unsafe {
                item.set_outline_width(px);
                item.mark_hull_records(&st.record_restyled)
            };
            log_info!(
                "BackgroundDancers: {} [hull] {} record(s) marked bit-31 (program 0 = outline pair), {} hidden (blended / stock material), rim {} px; materials restyled={} kept: blend={} no-variant={}",
                inst.model_name,
                marked,
                hidden,
                px,
                st.restyled,
                st.kept_blend,
                st.kept_no_variant
            );
        } else {
            log_info!(
                "BackgroundDancers: {} [{}] style {} -- materials restyled={} kept: blend={} no-variant={}",
                inst.model_name,
                inst.kind.tag(),
                style.key(),
                st.restyled,
                st.kept_blend,
                st.kept_no_variant
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
