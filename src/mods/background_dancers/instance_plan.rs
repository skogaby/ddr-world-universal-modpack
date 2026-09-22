//! The instance table of a scene — PURE (std + `super::selection` only, so
//! the host harness mounts it): which render item + scene node a
//! [`Session`](super::session::Session) builds for each parsed model, in
//! what order, with which pass mask, sort key and frame-board slot.
//!
//! Build order (design §4.3.5): stage parts, dancers, each dancer's parts,
//! each dancer's shadow, then — when the outline plan is non-empty — one
//! inverted-hull twin per restyle-eligible body PER LAYER, sharing the
//! body's slot. Slots are `slot_base + owner_index` (design §5.3: gameplay
//! 0, P1 previews 0, P2 previews 16); owners beyond the budget are skipped
//! with [`NO_SLOT`]. An `item_pass_mask` override stamps every instance
//! (previews: the side's private node-mask bit, so only that side's pass
//! clones draw the scene); `None` keeps the stock stage / lowprio / dancer
//! masks the gameplay passes filter on.

use std::time::Instant;

use super::selection::SHADOW_MODEL;

/// `SceneNode.instance` value meaning "no board slot" — pinned equal to
/// `scene3d::frame_board::NO_SLOT` by a `const` assertion in `session.rs`.
pub const NO_SLOT: u32 = u32::MAX;

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
    /// Inverted-hull OUTLINE twin of instance `of` (a `Dancer`, `Part` or
    /// `StagePart`): the same model/resource, every draw record carrying the
    /// bit-31 program selector so the synthesized variant container's
    /// outline pair (program 0) draws it. Shares `of`'s frame-board slot —
    /// never published itself (RE §4.6). `layer` indexes the session's
    /// `HullPlan`: INK has one layer; LAYERED stacks one hull per palette
    /// colour, each a band wider (`outline.rs`).
    Hull { of: usize, layer: usize },
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
    /// Frame-board slot (`slot_base` + the owner index; hull twins carry
    /// their body's).
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

impl Instance {
    fn pending(
        kind: InstanceKind,
        model_name: String,
        pass_mask: u32,
        sort_key: i32,
        slot: u32,
        mirror: bool,
        bone_count: usize,
    ) -> Instance {
        Instance {
            kind,
            model_name,
            pass_mask,
            sort_key,
            slot,
            mirror,
            status: InstanceStatus::Pending,
            node: 0,
            item: 0,
            bone_count,
            material_count: 0,
            textures_pending: 0,
            attached_at: None,
            node_shown: false,
            queued: false,
            freed: false,
        }
    }
}

/// The stock node-mask bits the gameplay passes filter on
/// (`scene3d::render_item_layout::PASS_MASK_*`, passed in because this file
/// mounts outside the crate).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PassMasks {
    pub stage: u32,
    pub lowprio: u32,
    pub dancer: u32,
}

/// One parsed stage part (`gm_<stage>_<part>`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagePartSpec {
    pub model_name: String,
    /// The `:N` low-priority rank (⇒ the lowprio pass, sort key = N).
    pub priority: Option<i32>,
    pub bone_count: usize,
}

/// One parsed accessory part of a dancer (`pl_<key>_<part>`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartSpec {
    pub model_name: String,
    /// The right-forearm point inversion (log tag only here).
    pub mirror: bool,
    pub bone_count: usize,
}

/// One parsed dancer body with its parts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DancerSpec {
    pub model_name: String,
    pub bone_count: usize,
    pub parts: Vec<PartSpec>,
    /// Ground-contact bones resolved ⇒ a shadow quad is planned (when the
    /// shadow skeleton parsed).
    pub has_ground: bool,
}

/// Everything `plan_instances` needs from a `Parsed` bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanInput {
    pub stage_parts: Vec<StagePartSpec>,
    pub dancers: Vec<DancerSpec>,
    /// `pl_shadow00`'s bone count; `None` ⇒ no shadows at all.
    pub shadow_bone_count: Option<usize>,
    /// The shadow quad's model name (`pl_shadow00`).
    pub shadow_model: String,
}

/// The planned table.
pub struct Plan {
    pub instances: Vec<Instance>,
    /// Per dancer: the instance indices of its parts and shadow.
    pub children: Vec<Vec<usize>>,
    /// The largest bone count over every planned model (≥ 1) — sizes the
    /// session's evaluation scratch.
    pub max_bones: usize,
    /// Owners skipped because they exceeded `slot_budget`.
    pub truncated: usize,
}

/// Plan the instance table (see the module docs for the order). Pure.
///
/// - `slot_base`: the first frame-board slot; owner `i` gets `slot_base + i`.
/// - `slot_budget`: how many owners may hold a slot; the tail is `Skipped`
///   with [`NO_SLOT`] (the caller logs the WARN from `Plan::truncated`).
/// - `hull_layers`: outline layers per restyle-eligible body (0 = none).
/// - `item_pass_mask`: `Some(m)` stamps every instance with `m` (hull twins
///   included); `None` = the stock `masks`.
pub fn plan_instances(
    input: &PlanInput,
    masks: PassMasks,
    slot_base: u32,
    slot_budget: usize,
    hull_layers: usize,
    item_pass_mask: Option<u32>,
) -> Plan {
    let mask_of = |stock: u32| item_pass_mask.unwrap_or(stock);
    let mut instances: Vec<Instance> = Vec::new();
    let mut max_bones = 1usize;
    let slot_for = |index: usize| slot_base.saturating_add(index as u32);

    for (i, p) in input.stage_parts.iter().enumerate() {
        let (pass_mask, sort_key) = match p.priority {
            Some(prio) => (mask_of(masks.lowprio), prio),
            None => (mask_of(masks.stage), 0),
        };
        max_bones = max_bones.max(p.bone_count);
        instances.push(Instance::pending(
            InstanceKind::StagePart(i),
            p.model_name.clone(),
            pass_mask,
            sort_key,
            slot_for(instances.len()),
            false,
            p.bone_count,
        ));
    }
    for (i, d) in input.dancers.iter().enumerate() {
        max_bones = max_bones.max(d.bone_count);
        instances.push(Instance::pending(
            InstanceKind::Dancer(i),
            d.model_name.clone(),
            mask_of(masks.dancer),
            0,
            slot_for(instances.len()),
            false,
            d.bone_count,
        ));
    }
    // Build order (design §4.3.5): stage parts, dancers, parts, shadows.
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); input.dancers.len()];
    for (i, d) in input.dancers.iter().enumerate() {
        for (k, p) in d.parts.iter().enumerate() {
            children[i].push(instances.len());
            instances.push(Instance::pending(
                InstanceKind::Part { dancer: i, part: k },
                p.model_name.clone(),
                mask_of(masks.dancer),
                0,
                slot_for(instances.len()),
                p.mirror,
                p.bone_count,
            ));
        }
    }
    if let Some(shadow_bones) = input.shadow_bone_count {
        for (i, d) in input.dancers.iter().enumerate() {
            if !d.has_ground {
                continue;
            }
            children[i].push(instances.len());
            instances.push(Instance::pending(
                InstanceKind::Shadow(i),
                input.shadow_model.clone(),
                mask_of(masks.dancer),
                0,
                slot_for(instances.len()),
                false,
                shadow_bones,
            ));
        }
    }
    // Slot budget: every instance so far OWNS a board slot.
    let slot_owners = instances.len();
    let truncated = slot_owners.saturating_sub(slot_budget);
    if truncated > 0 {
        for inst in instances.iter_mut().skip(slot_budget) {
            inst.status = InstanceStatus::Skipped;
            inst.slot = NO_SLOT;
        }
    }
    // Inverted-hull twins (scene outlines, RE §4.6/§4.7): one per
    // restyle-eligible instance (dancer bodies, parts, stage props) PER
    // LAYER of the plan, each reading the body's slot. Never for the
    // shadow or the skydome part (their materials stay stock, so program
    // 0 of their container is the body again) — `restyle_allowed` is the
    // same rule the restyle uses. Layer order within a body does not
    // matter for the look (the z-test stacks them); layers are grouped
    // per body so the build/teardown log reads body-by-body.
    // (Deploy #4 shipped a Dancer|Part-only filter here — a `cargo fmt`
    // reflow had defeated the edit — so stage props never got twins.)
    if hull_layers > 0 {
        let n = instances.len();
        for of in 0..n {
            let body = &instances[of];
            if !restyle_allowed(&body.kind, &body.model_name)
                || body.status == InstanceStatus::Skipped
            {
                continue;
            }
            let (model_name, pass_mask, sort_key, slot, mirror, bone_count) = (
                body.model_name.clone(),
                body.pass_mask,
                body.sort_key,
                body.slot,
                body.mirror,
                body.bone_count,
            );
            for layer in 0..hull_layers {
                instances.push(Instance::pending(
                    InstanceKind::Hull { of, layer },
                    model_name.clone(),
                    pass_mask,
                    sort_key,
                    slot,
                    mirror,
                    bone_count,
                ));
            }
        }
    }
    Plan {
        instances,
        children,
        max_bones,
        truncated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MASKS: PassMasks = PassMasks {
        stage: 4,
        lowprio: 0x10,
        dancer: 2,
    };

    fn fixture() -> PlanInput {
        PlanInput {
            stage_parts: vec![
                StagePartSpec {
                    model_name: "gm_boom00_bg".into(),
                    priority: Some(-2),
                    bone_count: 3,
                },
                StagePartSpec {
                    model_name: "gm_boom00_stage".into(),
                    priority: None,
                    bone_count: 2,
                },
            ],
            dancers: vec![
                DancerSpec {
                    model_name: "pl_emi01".into(),
                    bone_count: 40,
                    parts: vec![
                        PartSpec {
                            model_name: "pl_emi01_head00".into(),
                            mirror: false,
                            bone_count: 1,
                        },
                        PartSpec {
                            model_name: "pl_emi01_forearm00".into(),
                            mirror: true,
                            bone_count: 1,
                        },
                    ],
                    has_ground: true,
                },
                DancerSpec {
                    model_name: "pl_rage00".into(),
                    bone_count: 38,
                    parts: vec![PartSpec {
                        model_name: "pl_rage00_hips00".into(),
                        mirror: false,
                        bone_count: 1,
                    }],
                    has_ground: true,
                },
            ],
            shadow_bone_count: Some(1),
            shadow_model: SHADOW_MODEL.to_string(),
        }
    }

    fn kinds(plan: &Plan) -> Vec<InstanceKind> {
        plan.instances.iter().map(|i| i.kind).collect()
    }

    fn owners(plan: &Plan) -> Vec<&Instance> {
        plan.instances
            .iter()
            .filter(|i| i.kind.owns_slot())
            .collect()
    }

    #[test]
    fn gameplay_plan_order_and_slots() {
        let plan = plan_instances(&fixture(), MASKS, 0, 32, 0, None);
        use InstanceKind::*;
        assert_eq!(
            kinds(&plan),
            vec![
                StagePart(0),
                StagePart(1),
                Dancer(0),
                Dancer(1),
                Part { dancer: 0, part: 0 },
                Part { dancer: 0, part: 1 },
                Part { dancer: 1, part: 0 },
                Shadow(0),
                Shadow(1),
            ]
        );
        let slots: Vec<u32> = plan.instances.iter().map(|i| i.slot).collect();
        assert_eq!(slots, (0..9).collect::<Vec<u32>>());
        let masks: Vec<u32> = plan.instances.iter().map(|i| i.pass_mask).collect();
        assert_eq!(masks, vec![0x10, 4, 2, 2, 2, 2, 2, 2, 2]);
        let sort: Vec<i32> = plan.instances.iter().map(|i| i.sort_key).collect();
        assert_eq!(sort, vec![-2, 0, 0, 0, 0, 0, 0, 0, 0]);
        let mirror: Vec<bool> = plan.instances.iter().map(|i| i.mirror).collect();
        assert_eq!(
            mirror,
            vec![false, false, false, false, false, true, false, false, false]
        );
        assert_eq!(plan.children, vec![vec![4, 5, 7], vec![6, 8]]);
        assert_eq!(plan.max_bones, 40);
        assert_eq!(plan.truncated, 0);
        assert!(plan
            .instances
            .iter()
            .all(|i| i.status == InstanceStatus::Pending));
        assert_eq!(plan.instances[7].model_name, SHADOW_MODEL);
        assert_eq!(plan.instances[7].bone_count, 1);
    }

    #[test]
    fn slot_base_shifts_owner_slots() {
        let plan = plan_instances(&fixture(), MASKS, 16, 16, 1, None);
        let slots: Vec<u32> = owners(&plan).iter().map(|i| i.slot).collect();
        assert_eq!(slots, (16..25).collect::<Vec<u32>>());
        for h in plan.instances.iter() {
            if let InstanceKind::Hull { of, .. } = h.kind {
                assert_eq!(
                    h.slot, plan.instances[of].slot,
                    "hull shares its body's slot"
                );
                assert_eq!(h.model_name, plan.instances[of].model_name);
            }
        }
        // Frame board P2 base + 9 owners fits the 16-slot side budget.
        assert_eq!(plan.truncated, 0);
    }

    #[test]
    fn mask_override_covers_every_instance() {
        let plan = plan_instances(&fixture(), MASKS, 0, 32, 2, Some(0x20));
        assert!(plan.instances.iter().all(|i| i.pass_mask == 0x20));
        let hulls: Vec<&Instance> = plan
            .instances
            .iter()
            .filter(|i| matches!(i.kind, InstanceKind::Hull { .. }))
            .collect();
        // Eligible bodies: the `_stage` part, 2 dancers, 3 parts = 6 × 2 layers.
        assert_eq!(hulls.len(), 12);
        for h in &hulls {
            let InstanceKind::Hull { of, .. } = h.kind else {
                unreachable!()
            };
            let body = &plan.instances[of];
            assert!(!matches!(body.kind, InstanceKind::Shadow(_)));
            assert!(!body.model_name.ends_with("_bg"));
        }
        // Grouped per body, in body order, layer 0 then 1.
        let pairs: Vec<(usize, usize)> = hulls
            .iter()
            .map(|h| match h.kind {
                InstanceKind::Hull { of, layer } => (of, layer),
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(
            pairs,
            vec![
                (1, 0),
                (1, 1),
                (2, 0),
                (2, 1),
                (3, 0),
                (3, 1),
                (4, 0),
                (4, 1),
                (5, 0),
                (5, 1),
                (6, 0),
                (6, 1)
            ]
        );
        // Sort keys / mirror flags ride along unchanged.
        assert_eq!(plan.instances[0].sort_key, -2);
    }

    #[test]
    fn budget_truncates_the_tail() {
        let plan = plan_instances(&fixture(), MASKS, 16, 4, 1, None);
        let owners = owners(&plan);
        assert_eq!(owners.len(), 9);
        for (i, inst) in owners.iter().enumerate() {
            if i < 4 {
                assert_eq!(inst.slot, 16 + i as u32);
                assert_eq!(inst.status, InstanceStatus::Pending);
            } else {
                assert_eq!(inst.slot, NO_SLOT);
                assert_eq!(inst.status, InstanceStatus::Skipped);
            }
        }
        assert_eq!(plan.truncated, 5);
        // No hull for a skipped body; the kept bodies are the `_stage` part
        // (index 1) and both dancers (2, 3).
        let hull_of: Vec<usize> = plan
            .instances
            .iter()
            .filter_map(|i| match i.kind {
                InstanceKind::Hull { of, .. } => Some(of),
                _ => None,
            })
            .collect();
        assert_eq!(hull_of, vec![1, 2, 3]);
    }

    #[test]
    fn restyle_allowed_cases() {
        use InstanceKind::*;
        assert!(!restyle_allowed(&Shadow(0), SHADOW_MODEL));
        assert!(!restyle_allowed(&StagePart(0), "gm_boom00_bg"));
        assert!(restyle_allowed(&StagePart(1), "gm_boom00_stage"));
        assert!(restyle_allowed(&Dancer(0), "pl_emi01"));
        assert!(restyle_allowed(
            &Part { dancer: 0, part: 0 },
            "pl_emi01_head00"
        ));
        assert!(!restyle_allowed(&Hull { of: 0, layer: 0 }, "gm_boom00_bg"));
        assert!(!restyle_allowed(&Hull { of: 7, layer: 0 }, SHADOW_MODEL));
        assert!(restyle_allowed(&Hull { of: 2, layer: 1 }, "pl_emi01"));
        assert!(Hull { of: 0, layer: 0 }.tag() == "hull" && !Hull { of: 0, layer: 0 }.owns_slot());
        assert!(Shadow(0).owns_slot());
    }

    #[test]
    fn no_shadow_when_skeleton_missing() {
        let mut input = fixture();
        input.shadow_bone_count = None;
        let plan = plan_instances(&input, MASKS, 0, 32, 0, None);
        assert!(!plan
            .instances
            .iter()
            .any(|i| matches!(i.kind, InstanceKind::Shadow(_))));
        assert_eq!(plan.children, vec![vec![4, 5], vec![6]]);
        assert_eq!(plan.instances.len(), 7);
    }

    #[test]
    fn stage_only_and_dancer_only_shapes() {
        let mut stage_only = fixture();
        stage_only.dancers.clear();
        let plan = plan_instances(&stage_only, MASKS, 0, 16, 1, Some(0x08));
        assert_eq!(
            kinds(&plan),
            vec![
                InstanceKind::StagePart(0),
                InstanceKind::StagePart(1),
                InstanceKind::Hull { of: 1, layer: 0 }
            ]
        );
        assert!(plan.children.is_empty());
        assert_eq!(plan.max_bones, 3);

        let mut dancer_only = fixture();
        dancer_only.stage_parts.clear();
        dancer_only.dancers.truncate(1);
        dancer_only.shadow_bone_count = None; // previews parse no shadow
        let plan = plan_instances(&dancer_only, MASKS, 16, 16, 0, Some(0x20));
        assert_eq!(
            kinds(&plan),
            vec![
                InstanceKind::Dancer(0),
                InstanceKind::Part { dancer: 0, part: 0 },
                InstanceKind::Part { dancer: 0, part: 1 },
            ]
        );
        assert_eq!(
            plan.instances.iter().map(|i| i.slot).collect::<Vec<_>>(),
            vec![16, 17, 18]
        );
        assert!(plan.instances.iter().all(|i| i.pass_mask == 0x20));
        assert_eq!(plan.children, vec![vec![1, 2]]);
    }
}
