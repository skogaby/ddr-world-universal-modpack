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
use crate::core::anm::{anm as anmfile, b2it, ktmdl, sanm, Anm, Mat4};
use crate::core::arc as arcfile;
use crate::services::avs_layeredfs::shader_layout::{self, SceneStyle};
use crate::services::scene3d::frame_board::{self, MatParam, NO_SLOT};
use crate::services::scene3d::render_item_layout::{
    scale_translation, IDENTITY, PASS_MASK_DANCER, PASS_MASK_LOWPRIO, PASS_MASK_STAGE,
};
use crate::services::scene3d::{
    arc_set, model_registry, node, pure, render_item, scene_graph, texture,
};
use crate::{log_info, log_warn};

use super::director_math::{
    body_world, part_world, shadow_target, shadow_world, subtree_of, transform_point, BLACK,
    SHADOW_FLOOR_Y, WHITE,
};
use super::flight_fx::{self, DancerFx, FxLayout, StageFx, Teb};
use super::instance_plan::{
    plan_instances, DancerSpec, FxSpec, PartSpec, PassMasks, PlanInput, StagePartSpec,
};
use super::outline::{self, HullPlan};
use super::schedule::{CameraSchedule, CameraState, ClipRef, DanceSchedule};
use super::selection::{
    camanm_member_path, part_attach_bone, Sex, GROUND_BONES, HEAD_BONE, HIPS_BONE,
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

/// A part's `_play_loop.sanm` (material parameters: UV scroll, colour /
/// glow pulses — `core::anm::sanm`) with its slots bound to the part's
/// material indices.
pub struct MaterialClip {
    pub bytes: Arc<Vec<u8>>,
    pub sanm: sanm::Sanm,
    /// `.sanm` slot → material index in the `.model` (= the item's copies).
    pub binding: Vec<Option<u16>>,
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
    pub material_clip: Option<MaterialClip>,
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
    /// The `Head` bone and its descendants (root first) — what Big Head
    /// scales (`director_math::scale_subtree_about_root`); empty without a
    /// `.b2it` / a `Head` entry, which leaves the dancer at normal size.
    pub head_subtree: Vec<usize>,
    /// The flight effects' attach joints (`flight_fx::JOINT_NAMES`: Hips,
    /// hands, feet); `None` when the `.b2it` lacks one — no effects then.
    pub fx_joints: Option<[usize; 5]>,
}

/// A flight stage's effect bank + pool layout (`data/map/flight_fx/`,
/// `port_flight_fx.py`): what the flying dancers' effects run from.
pub struct FlightFxAssets {
    pub teb: Teb,
    pub layout: FxLayout,
    /// Per layout pool: its model's bone count (from the `.model` file).
    pub pool_bones: Vec<Option<usize>>,
    /// The intro's stage effect bank (the sky burst; the layout's
    /// `stage_effect` line says where and when), when the stage ships one.
    pub stage_teb: Option<Teb>,
    /// The intro sound, ready to register (`game_audio::register_one_shot_bank`).
    pub stage_sound: Option<StageSound>,
}

/// The flight intro's sound (SE_DDR_BOSS, from intro frame 180): a one-cue
/// XACT pair built on the parse thread from the stage's mono 44.1 kHz PCM.
pub struct StageSound {
    /// Bank / wave bank / cue name (`fxb` + a hash of the samples: two
    /// stages shipping the same sound share one registered bank).
    pub name: String,
    pub start_s: f32,
    pub xwb: Vec<u8>,
    pub xsb: Vec<u8>,
}

/// Build the one-cue bank pair for `pcm` (mono i16, 44.1 kHz).
fn stage_sound(pcm: &[i16], start_s: f32) -> Option<StageSound> {
    use crate::services::se_bank_synth::{adpcm, xsb, xwb};
    if pcm.is_empty() {
        return None;
    }
    let mut h: u32 = 0x811C_9DC5;
    for v in pcm {
        for b in v.to_le_bytes() {
            h = (h ^ b as u32).wrapping_mul(0x0100_0193);
        }
    }
    let name = format!("fxb{h:08x}");
    let encoded = adpcm::encode_mono(pcm);
    let blocks = encoded.len() / adpcm::BLOCK_ALIGN;
    let mut wave = xwb::build(
        &name,
        (blocks * adpcm::SAMPLES_PER_BLOCK) as u32,
        blocks * adpcm::BLOCK_ALIGN,
    );
    wave.bytes
        .get_mut(wave.sample_seg_offset..wave.sample_seg_offset + wave.sample_seg_len)?
        .copy_from_slice(encoded.get(..wave.sample_seg_len)?);
    Some(StageSound {
        xsb: xsb::build_se(&name),
        name,
        start_s,
        xwb: wave.bytes,
    })
}

/// The effect bank / layout members inside a flight stage's arc.
pub const FLIGHT_FX_TEB: &str = "data/map/flight_fx/flight_fx.teb";
pub const FLIGHT_FX_LAYOUT: &str = "data/map/flight_fx/flight_fx.txt";
pub const FLIGHT_FX_STAGE_TEB: &str = "data/map/flight_fx/stage_fx.teb";

/// The stage's camera sets that parsed (in the pick's shuffled order).
pub struct ParsedCameras {
    pub main: Vec<Clip>,
    pub non: Vec<Clip>,
    /// A FLIGHT stage's take-off shots, played in order during the take-off
    /// (`Pick::camera_intro`; empty elsewhere).
    pub intro: Vec<Clip>,
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
    /// The flight effects (a flight pick whose stage ships them).
    pub flight_fx: Option<Arc<FlightFxAssets>>,
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

/// A part's `<dir>/<model>_play_loop.sanm`, when the arc carries one: parsed
/// and bound to the `.model`'s material identities. A clip whose targets
/// are all foreign to the model is dropped (WARN) — it would animate
/// nothing.
fn parse_material_clip(
    reader: &ArcReader,
    member: &str,
    model_member: &str,
    warnings: &mut Vec<String>,
) -> Option<MaterialClip> {
    let bytes = reader.get(member)?;
    let parsed = match sanm::parse(&bytes) {
        Ok(s) => s,
        Err(e) => {
            warnings.push(format!("{member}: {e}"));
            return None;
        }
    };
    let identities = match reader
        .get(model_member)
        .map(|m| ktmdl::material_identities(&m))
    {
        Some(Ok(ids)) => ids,
        Some(Err(e)) => {
            warnings.push(format!("{model_member}: material table: {e}"));
            return None;
        }
        None => return None,
    };
    let binding = sanm::bind_targets(&parsed, &identities);
    let bound = binding.iter().filter(|b| b.is_some()).count();
    if bound == 0 || parsed.tracks.is_empty() {
        warnings.push(format!(
            "{member}: {} track(s), {} of {} material target(s) found in the model -- ignored",
            parsed.tracks.len(),
            bound,
            binding.len()
        ));
        return None;
    }
    // One INFO line per process (previews re-parse on every option scroll).
    static ANNOUNCED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if !ANNOUNCED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        log_info!(
            "BackgroundDancers: {member}: material animation -- {} track(s) on {}/{} material(s), {} frames @ {} fps{} (first .sanm this session; the director samples it into the part's material copies)",
            parsed.tracks.len(),
            bound,
            binding.len(),
            parsed.frame_count,
            parsed.fps,
            if parsed.loops { ", loop" } else { "" }
        );
    }
    Some(MaterialClip {
        bytes: Arc::new(bytes),
        sanm: parsed,
        binding,
    })
}

/// Highest bone index any track of `anm` drives (0 for a trackless clip).
fn max_track_target(anm: &Anm) -> usize {
    anm.bone_tracks
        .iter()
        .map(|t| t.target as usize)
        .max()
        .unwrap_or(0)
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
                    let model_member = format!("{dir}/{model_name}.model");
                    let Some(skeleton) = parse_skeleton(&reader, &model_member, &mut warnings)
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
                    let sanm_member = format!("{dir}/{model_name}_play_loop.sanm");
                    let material_clip = if reader.entries.iter().any(|e| e.path == sanm_member) {
                        parse_material_clip(&reader, &sanm_member, &model_member, &mut warnings)
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
                        material_clip,
                    });
                }
                stage_reader = Some(reader);
            }
        }
    }
    let flight_fx = match (pick.flight, stage_reader.as_ref()) {
        (true, Some(reader)) => parse_flight_fx(reader, &mut warnings),
        _ => None,
    };

    // Dancers: body skeleton + playlist clips — from the motion arc of each
    // dancer's sex, or its OWN clips inside its body arc (design 2026-09-28
    // D1: a dancer ported with its original rig brings its choreography).
    let mut motion: Vec<(String, Option<ArcReader>)> = Vec::new();
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
        let mut clips = Vec::new();
        let mut load_clips = |reader: &ArcReader, warnings: &mut Vec<String>| {
            for clip in pick.playlists.get(i).into_iter().flatten() {
                if let Some(c) = parse_clip(reader, &d.clip_member(clip), clip, warnings) {
                    clips.push(c);
                }
            }
        };
        if d.has_own_motion() {
            load_clips(&body, &mut warnings);
        } else {
            let arc = format!("data/arc/{}", d.motion_arc_name());
            if motion.iter().all(|(a, _)| *a != arc) {
                let r = ArcReader::open(&arc);
                if r.is_none() {
                    warnings.push(format!("{arc}: unreadable -- no choreography for that sex"));
                }
                motion.push((arc.clone(), r));
            }
            if let Some((_, Some(reader))) = motion.iter().find(|(a, _)| *a == arc) {
                load_clips(reader, &mut warnings);
            }
        }
        if clips.is_empty() {
            warnings.push(format!("dancer {} has no playable clip -- dropped", d.key));
            continue;
        }
        if let Some(c) = clips
            .iter()
            .find(|c| max_track_target(&c.anm) >= skeleton.bone_count())
        {
            // Stock clips address the 33-bone HumanIK rig by index; a foreign
            // rig must bring its own clips (tracks past its bone count are
            // skipped by the evaluator, so this only warns).
            warnings.push(format!(
                "dancer {}: clip {} targets bone {} but {model_name} has {} bones -- pose will be partial",
                d.key,
                c.name,
                max_track_target(&c.anm),
                skeleton.bone_count()
            ));
        }
        let seed = seed_local_trs(&skeleton);

        // Parts, shadow bones and the Big Head subtree need the body's `.b2it`
        // (bone name → index). Missing/unparseable ⇒ the body dances alone,
        // at normal size (one warning).
        let names = match body.get(&format!("data/chara/{model_name}/{model_name}.b2it")) {
            Some(bytes) => match b2it::parse(&bytes) {
                Ok(t) => Some(t),
                Err(e) => {
                    warnings.push(format!(
                        "{model_name}.b2it: {e} -- no parts/shadow/big head"
                    ));
                    None
                }
            },
            None => {
                warnings.push(format!(
                    "{model_name}.b2it: member missing -- no parts/shadow/big head"
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
        let head_subtree = bone_index(HEAD_BONE)
            .map(|h| subtree_of(&skeleton.parents, h))
            .unwrap_or_default();
        if names.is_some() && head_subtree.is_empty() {
            warnings.push(format!(
                "{model_name}.b2it has no {HEAD_BONE} -- big head skipped for this dancer"
            ));
        }

        let fx_joints = {
            let mut j = [0usize; 5];
            let mut ok = names.is_some();
            for (k, name) in flight_fx::JOINT_NAMES.iter().enumerate() {
                match bone_index(name) {
                    Some(b) => j[k] = b,
                    None => ok = false,
                }
            }
            if pick.flight && flight_fx.is_some() && !ok {
                warnings.push(format!(
                    "{model_name}.b2it lacks one of {:?} -- no flight effects for this dancer",
                    flight_fx::JOINT_NAMES
                ));
            }
            ok.then_some(j)
        };
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
            head_subtree,
            fx_joints,
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
            let intro = load(&pick.camera_intro);
            if main.is_empty() {
                warnings.push("no main camera clip parsed -- fixed camera".to_string());
                None
            } else {
                Some(ParsedCameras { main, non, intro })
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
            Some(ParsedCameras {
                main,
                non,
                intro: Vec::new(),
            })
        }
    };

    Parsed {
        stage_parts,
        dancers,
        shadow,
        cameras,
        movie_cameras,
        flight_fx,
        warnings,
        elapsed_ms: started.elapsed().as_millis() as u64,
    }
}

/// The stage arc's flight effects: the TEB bank, the pool layout and every
/// pool model's bone count. `None` (with a warning when the stage ships
/// half of it) otherwise — the dancers then fly without effects.
fn parse_flight_fx(reader: &ArcReader, warnings: &mut Vec<String>) -> Option<Arc<FlightFxAssets>> {
    let (teb, text) = match (reader.get(FLIGHT_FX_TEB), reader.get(FLIGHT_FX_LAYOUT)) {
        (None, None) => {
            warnings.push(format!(
                "flight stage without {FLIGHT_FX_TEB} -- the dancers fly without effects (re-run port_flight_fx.py)"
            ));
            return None;
        }
        (Some(t), Some(l)) => (t, l),
        _ => {
            warnings.push(format!(
                "{FLIGHT_FX_TEB} / {FLIGHT_FX_LAYOUT}: only one present -- no flight effects"
            ));
            return None;
        }
    };
    let Some(teb) = flight_fx::parse_teb(&teb) else {
        warnings.push(format!(
            "{FLIGHT_FX_TEB}: not a readable TEB -- no flight effects"
        ));
        return None;
    };
    let layout = match FxLayout::parse(&String::from_utf8_lossy(&text)) {
        Ok(l) => l,
        Err(e) => {
            warnings.push(format!("{FLIGHT_FX_LAYOUT}: {e} -- no flight effects"));
            return None;
        }
    };
    let pool_bones = layout
        .pools
        .iter()
        .map(|p| {
            let member = format!("data/map/{0}/{0}.model", p.model);
            let n = parse_skeleton(reader, &member, warnings)?.bone_count();
            if n != p.bones() {
                warnings.push(format!(
                    "{member}: {n} bones but the layout says {} -- pool skipped",
                    p.bones()
                ));
                return None;
            }
            Some(n)
        })
        .collect();
    let stage_teb = match (layout.stage_effect, reader.get(FLIGHT_FX_STAGE_TEB)) {
        (None, _) => None,
        (Some(_), None) => {
            warnings.push(format!(
                "{FLIGHT_FX_LAYOUT} names a stage effect but {FLIGHT_FX_STAGE_TEB} is missing -- no sky burst"
            ));
            None
        }
        (Some(_), Some(b)) => {
            let t = flight_fx::parse_teb(&b);
            if t.is_none() {
                warnings.push(format!(
                    "{FLIGHT_FX_STAGE_TEB}: not a readable TEB -- no sky burst"
                ));
            }
            t
        }
    };
    let stage_sound = layout.stage_sound.as_ref().and_then(|(frame, member)| {
        let path = format!("data/map/flight_fx/{member}");
        let Some(bytes) = reader.get(&path) else {
            warnings.push(format!("{path}: member missing -- no intro sound"));
            return None;
        };
        let pcm: Vec<i16> = bytes
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect();
        let s = stage_sound(&pcm, frame / 60.0);
        if s.is_none() {
            warnings.push(format!("{path}: no usable samples -- no intro sound"));
        }
        s
    });
    Some(Arc::new(FlightFxAssets {
        teb,
        layout,
        pool_bones,
        stage_teb,
        stage_sound,
    }))
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
pub fn dance_schedule(parsed: &Parsed, opts: TempoOptions, flight: bool) -> Option<DanceSchedule> {
    let d = DanceSchedule::new(
        parsed
            .dancers
            .iter()
            .map(|d| d.clips.iter().map(Clip::clip_ref).collect())
            .collect(),
    )?;
    let d = if opts.bpm_sync {
        d.with_quantum(BEAT_TAU)
    } else {
        d
    };
    // FLIGHT: every dancer's first clip is its take-off (a missing take-off
    // member drops it from the parsed playlist — then no intro, the
    // platform stays and the flight clips cycle on it).
    let takeoffs = parsed.dancers.iter().all(|d| {
        d.clips
            .first()
            .is_some_and(|c| super::selection::is_takeoff_clip(&c.name))
    });
    Some(if flight && takeoffs {
        d.with_intro()
    } else {
        d
    })
}

/// The switch time of a flight stage (see [`Session::flight_switch`]).
pub fn flight_switch(pick: &Pick, schedule: Option<&DanceSchedule>) -> Option<f32> {
    if !pick.flight {
        return None;
    }
    if pick.dancers.is_empty() {
        return Some(super::director_math::DEFAULT_TAKEOFF_S);
    }
    schedule.and_then(DanceSchedule::intro_end)
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
    /// no hulls; the DSU ink = one black hull — `outline.rs`). Frozen per
    /// song.
    pub hulls: HullPlan,
    pub schedule: Option<DanceSchedule>,
    /// The stage is a FLIGHT stage (`StageCandidate::flight`): its `pre_` /
    /// `fly_` parts follow [`flight_switch`](Self::flight_switch).
    pub flight_stage: bool,
    /// When the flight starts (the take-off segment's end; the stage-only
    /// preview: `DEFAULT_TAKEOFF_S`). `None` on a flight stage = no flight
    /// (nobody can fly, or a take-off failed to parse): the platform stays.
    pub flight_switch: Option<f32>,
    pub instances: Vec<Instance>,
    /// Per dancer: the instance indices of its parts, shadow and effect
    /// pools (the director derives them from the dancer's bones without
    /// re-scanning).
    pub children: Vec<Vec<usize>>,
    /// Per dancer: its flight effects (`None` off a flight / without the
    /// stage's effect assets / the attach joints).
    pub fx: Vec<Option<DancerFx>>,
    /// Per dancer: the leap's dance time (take-off frame 544 of 600).
    pub fx_leap: Vec<f32>,
    /// The camera this frame is filmed with (set by the camera director
    /// before the publish): what the effects' billboards face.
    pub fx_camera: Option<scene_graph::CamSample>,
    /// The flight intro's stage effect (the sky burst), when shipped.
    pub stage_fx: Option<StageFx>,
    /// The intro sound's state: played this pass of the intro (re-armed
    /// when the clock goes back before its start).
    pub stage_sound_played: bool,
    /// Its registered bank (process lifetime; first play registers it).
    pub stage_sound_bank: Option<crate::services::game_audio::OneShotBankHandle>,
    /// The dance time at real time `flight_switch` (the lifecycle keeps it
    /// current from the tempo map; `None` = dance time is real time): the
    /// take-off runs on the real clock (`flight_fx::flight_schedule_time`).
    pub dance_at_switch: Option<f32>,
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
    /// Per-frame `.sanm` samples of the stage part being published.
    pub mat_params: Vec<MatParam>,
    pub requested_at: Instant,
    pub built_at: Option<Instant>,
}

impl Session {
    /// `style`: the scene style applied at item build (materials re-pointed
    /// at the `<name>_<style>` variant containers, RE §4.7). `hulls`: the
    /// inverted-hull outline layers to build for every restyle-eligible
    /// instance — empty when SCENE OUTLINES is off, the style is stock or the
    /// synthesized containers lack the outline pair (`style::hull_plan`
    /// decides; the DSU ink is one black hull).
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
        let schedule = dance_schedule(&parsed, tempo_opts, pick.flight);
        let flight_stage = pick.stage.as_ref().is_some_and(|s| s.flight);
        let flight_switch = flight_switch(&pick, schedule.as_ref());
        // Flight effects: every flying dancer with its joints gets player
        // (index mod 4)'s pools — only when the take-off really leads into
        // a flight.
        let fx_assets = parsed
            .flight_fx
            .clone()
            .filter(|_| flight_switch.is_some() && !parsed.dancers.is_empty());
        let fx_pool_specs = |i: usize| -> Vec<(usize, FxSpec)> {
            let Some(a) = fx_assets.as_ref() else {
                return Vec::new();
            };
            if parsed.dancers.get(i).and_then(|d| d.fx_joints).is_none() {
                return Vec::new();
            }
            a.layout
                .pools_of(i % flight_fx::PLAYERS)
                .into_iter()
                .take(flight_fx::MAX_POOLS)
                .filter_map(|k| {
                    let bones = (*a.pool_bones.get(k)?)?;
                    Some((
                        k,
                        FxSpec {
                            model_name: a.layout.pools.get(k)?.model.clone(),
                            bone_count: bones,
                        },
                    ))
                })
                .collect()
        };
        if flight_stage {
            match flight_switch {
                Some(s) => log_info!(
                    "BackgroundDancers: flight stage -- take-off for {:.1} s, then the tunnel (pre_/fly_ parts switch there)",
                    s
                ),
                None => log_warn!(
                    "BackgroundDancers: flight stage without a flight (no flight-capable dancer, or a take-off clip missing) -- the platform stays"
                ),
            }
        }
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
                    fx_pools: Vec::new(),
                })
                .collect(),
            shadow_bone_count: parsed.shadow.as_ref().map(|s| s.bone_count()),
            shadow_model: SHADOW_MODEL.to_string(),
            stage_fx: Vec::new(),
        };
        let mut input = input;
        // The intro's stage effect (the sky burst): its pools, when the
        // take-off leads into a flight. A dancer-less (stage-only preview)
        // flight plays it too.
        let stage_fx = parsed
            .flight_fx
            .as_ref()
            .filter(|_| flight_switch.is_some())
            .and_then(|a| {
                let teb = a.stage_teb.as_ref()?;
                let sfx = StageFx::new(teb, &a.layout, pick.seed as u32)?;
                let specs: Vec<FxSpec> = sfx
                    .pools
                    .iter()
                    .filter_map(|&k| {
                        Some(FxSpec {
                            model_name: a.layout.pools.get(k)?.model.clone(),
                            bone_count: (*a.pool_bones.get(k)?)?,
                        })
                    })
                    .collect();
                (specs.len() == sfx.pools.len()).then_some((sfx, specs))
            });
        if let Some((sfx, specs)) = stage_fx.as_ref() {
            input.stage_fx = specs.clone();
            log_info!(
                "BackgroundDancers: flight intro stage effect -- effect {} at intro frame {} at ({:.0}, {:.0}, {:.0}), {} pool(s)",
                sfx.spec.effect,
                sfx.spec.frame,
                sfx.spec.at[0],
                sfx.spec.at[1],
                sfx.spec.at[2],
                specs.len()
            );
        }
        let stage_fx = stage_fx.map(|(sfx, _)| sfx);
        let mut fx: Vec<Option<DancerFx>> = Vec::new();
        for (i, d) in input.dancers.iter_mut().enumerate() {
            let specs = fx_pool_specs(i);
            let built = match (fx_assets.as_ref(), specs.is_empty()) {
                (Some(a), false) => {
                    let mut dfx = DancerFx::new(&a.teb, &a.layout, i, pick.seed as u32);
                    // keep only the pools whose model parsed, in plan order
                    let keep: Vec<usize> = specs.iter().map(|(k, _)| *k).collect();
                    let frames = std::mem::take(&mut dfx.frames);
                    let (pools, frames): (Vec<usize>, Vec<_>) = dfx
                        .pools
                        .iter()
                        .copied()
                        .zip(frames)
                        .filter(|(k, _)| keep.contains(k))
                        .unzip();
                    dfx.pools = pools;
                    dfx.frames = frames;
                    d.fx_pools = specs.into_iter().map(|(_, s)| s).collect();
                    Some(dfx)
                }
                _ => None,
            };
            fx.push(built);
        }
        let fx_leap: Vec<f32> = parsed
            .dancers
            .iter()
            .map(|d| {
                let takeoff = d
                    .clips
                    .first()
                    .filter(|c| super::selection::is_takeoff_clip(&c.name))
                    .map_or(director_math_takeoff_s(), |c| c.anm.duration_s());
                takeoff * flight_fx::LEAP_FRAME / flight_fx::TAKEOFF_FRAMES
            })
            .collect();
        if fx.iter().any(Option::is_some) {
            log_info!(
                "BackgroundDancers: flight effects -- {} of {} dancer(s), {} pool instance(s), leap at {:.2} s (boss_ddr3.TEB, {} effects)",
                fx.iter().filter(|f| f.is_some()).count(),
                fx.len(),
                input.dancers.iter().map(|d| d.fx_pools.len()).sum::<usize>(),
                fx_leap.first().copied().unwrap_or(0.0),
                fx_assets.as_ref().map_or(0, |a| a.teb.effects.len())
            );
        }
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
            flight_stage,
            flight_switch,
            instances: plan.instances,
            children: plan.children,
            fx,
            fx_leap,
            fx_camera: None,
            stage_fx,
            stage_sound_played: false,
            stage_sound_bank: None,
            dance_at_switch: None,
            shadow_size,
            camera,
            camera_state: None,
            movie_camera,
            movie_camera_state: None,
            scratch: vec![Trs::IDENTITY; plan.max_bones],
            bones: vec![IDENTITY; plan.max_bones],
            mat_params: Vec::with_capacity(frame_board::MAX_MAT_PARAMS),
            requested_at,
            built_at: None,
        }
    }

    /// The schedule time of a frame at dance time `t` / real time `real_s`
    /// (`flight_fx::flight_schedule_time`: a flight's take-off on the real
    /// clock; everything else unchanged).
    pub fn schedule_time(&self, t: f32, real_s: f32) -> f32 {
        let switch = self.flight_switch.filter(|_| self.pick.flight);
        let at = self.dance_at_switch.or(switch).unwrap_or(0.0);
        flight_fx::flight_schedule_time(switch, t, real_s, at)
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
    /// `(stage parts, dancers, parts, shadows, hulls, effect pools — the
    /// stage effect's included)`.
    pub fn built_counts(&self) -> (usize, usize, usize, usize, usize, usize) {
        let mut c = (0, 0, 0, 0, 0, 0);
        for i in self.built() {
            match i.kind {
                InstanceKind::StagePart(_) => c.0 += 1,
                InstanceKind::Dancer(_) => c.1 += 1,
                InstanceKind::Part { .. } => c.2 += 1,
                InstanceKind::Shadow(_) => c.3 += 1,
                InstanceKind::Hull { .. } => c.4 += 1,
                InstanceKind::Fx { .. } | InstanceKind::StageFx(_) => c.5 += 1,
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
            // Effect pools: bones carry world positions (identity bind).
            InstanceKind::StagePart(_) | InstanceKind::Fx { .. } | InstanceKind::StageFx(_) => {
                IDENTITY
            }
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
            // The layer's colour rides the records (collector → c23 → the
            // outline PS emits it verbatim); `ModelParameters.w` is the
            // multiplier on DSU's depth-scaled push (the hull VS,
            // `outline.rs`). A layer index outside the plan can only come
            // from a plan/instances mismatch — draw nothing.
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
            // SAFETY: as above.
            let (marked, hidden) = unsafe {
                item.set_outline_push_scale(spec.push_scale);
                item.set_record_colors(spec.rgba);
                item.mark_hull_records(&st.record_restyled)
            };
            log_info!(
                "BackgroundDancers: {} [hull L{} {}] {} record(s) marked bit-31 (program 0 = outline pair), {} hidden (blended / stock material), push x{:.2}; materials restyled={} kept: blend={} no-variant={} screen={}",
                inst.model_name,
                layer,
                outline::hex(spec.rgba),
                marked,
                hidden,
                spec.push_scale,
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
    if matches!(inst.kind, InstanceKind::Fx { .. }) {
        // The effect pools address draw record i as quad / strip mesh i
        // (`flight_fx.txt`): confirm the record → material order the item
        // carries (a sprite pool must read 0, 1, 2, …).
        // SAFETY: our own fresh block, not yet attached.
        let map = unsafe { item.record_materials() };
        let identity = map.iter().enumerate().all(|(i, &m)| i == m);
        let head: Vec<String> = map.iter().take(12).map(|m| m.to_string()).collect();
        log_info!(
            "BackgroundDancers: {} [fx] {} record(s) -> materials [{}{}]{}",
            inst.model_name,
            map.len(),
            head.join(","),
            if map.len() > 12 { ",..." } else { "" },
            if identity { " (identity)" } else { "" }
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

/// The take-off's length when its clip did not parse (the leap then lands
/// at the same fraction of the default).
fn director_math_takeoff_s() -> f32 {
    super::director_math::DEFAULT_TAKEOFF_S
}

/// Used by `initial_world` callers that only have the scale (tests, Step 8).
pub fn scaled_at(scale: f32, x: f32) -> Mat4 {
    scale_translation(scale, x, 0.0, 0.0)
}
