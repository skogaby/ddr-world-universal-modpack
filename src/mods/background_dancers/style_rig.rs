//! Runtime look switching for one built instance — PURE data + helpers
//! (std only, harness-mounted by `scripts/validate_background_dancers.sh`).
//!
//! Since the DDR Universe styles (2026-11-02) a dancer's look can change
//! while it is attached: LIGHTING STYLE edits apply at once (gameplay and
//! the song-select previews) and RANDOM (EVERY BAR) changes it every chart
//! bar. Rebuilding items for that would cost a model build per instance per
//! bar, so at build the session captures a [`StyleRig`]: for every material
//! copy the whole-scene restyle may touch, its stock shader object and the
//! synthesized container of EVERY style family; and every draw record's
//! flags as built. Switching is then plain stores into the item's private
//! copies from the game thread (mid-frame, the workers drained — the same
//! contract as the per-frame texture retry): shader-object pointers, record
//! flags, the style texture in material slot 1, the node's item pass mask /
//! sort key, and a hull's node-level hidden flag.
//!
//! The session (`session::Session::style_tick`) turns a style into a
//! [`Look`] per instance; [`StyleRig::applied`] makes re-applying the same
//! look free.

/// Container slots per material: slot `k` = family `k` of
/// `shader_layout::StyleFamily::all()` (lit, cel, the ten Universe ones).
pub const FAMILY_SLOTS: usize = 12;

/// One restyle-eligible material copy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RigMaterial {
    /// Index into the item's material copies.
    pub index: usize,
    /// Its shader object as built (`gs::Shader*` as usize — the stock look).
    pub stock: usize,
    /// Per family slot: the synthesized container object (0 = none: that
    /// family leaves this material stock).
    pub family: [usize; FAMILY_SLOTS],
}

/// How a look rewrites the styled records' flags (from their build flags).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordMode {
    /// As built.
    Base,
    /// Additive glow (CANDY / BUBBLE / CRYSTAL / X-RAY).
    Additive,
    /// OUTLINE ONLY's body: additive with rgb 0, depth written. `trans` =
    /// collected by a TRANS-type pass (gameplay: LOWPRIO_TRANS, so it lands
    /// after the opaque stage and before the hulls' TRANS pass).
    InvisibleDepth { trans: bool },
    /// A hull drawn by the TRANS pass (after OUTLINE ONLY's body).
    Trans,
}

/// What one instance should look like now.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    /// Family slot of the shader containers (`None` = stock). Ignored for a
    /// hull (its materials point at an outline-pair container for life).
    pub family: Option<usize>,
    pub records: RecordMode,
    /// gs hash of the style texture bound into material slot 1.
    pub texture: Option<u32>,
    /// The item pass mask the node stamps (`visit(4)`).
    pub pass_mask: u32,
    /// The node sort key (the LOWPRIO pass draws in this order).
    pub sort_key: i32,
    /// Node-level force-hidden (a hull whose dancer's look has no outline).
    pub hidden: bool,
}

/// The capture + what is applied.
#[derive(Clone, Debug, PartialEq)]
pub struct StyleRig {
    pub hull: bool,
    /// Takes the STAGE look (a stage prop or its hull) — set by the session
    /// after the build.
    pub stage: bool,
    pub materials: Vec<RigMaterial>,
    pub material_count: usize,
    /// Every draw record's flags as built (a hull: after the hull marking).
    pub record_base: Vec<u32>,
    /// Per record: its material is one of [`materials`](Self::materials).
    pub record_styled: Vec<bool>,
    /// The look currently in the item (`None` = nothing applied yet).
    pub applied: Option<Look>,
    /// The style texture currently bound (slot 1).
    pub texture_bound: Option<u32>,
    /// Frames the wanted texture was not registered yet.
    pub texture_misses: u32,
    /// The node-hidden state written (`None` = left as attached).
    pub hidden_applied: Option<bool>,
}

impl StyleRig {
    pub fn new(
        hull: bool,
        materials: Vec<RigMaterial>,
        material_count: usize,
        record_base: Vec<u32>,
        record_materials: &[usize],
    ) -> StyleRig {
        let record_styled = record_materials
            .iter()
            .map(|mi| materials.iter().any(|m| m.index == *mi))
            .collect();
        StyleRig {
            hull,
            stage: false,
            materials,
            material_count,
            record_base,
            record_styled,
            applied: None,
            texture_bound: None,
            texture_misses: 0,
            hidden_applied: None,
        }
    }

    /// `(material index, shader object)` for every captured material under
    /// `family` (a material without that family's container goes stock).
    pub fn material_writes(&self, family: Option<usize>) -> Vec<(usize, usize)> {
        self.materials
            .iter()
            .map(|m| {
                let obj = family
                    .and_then(|f| m.family.get(f).copied())
                    .filter(|&p| p != 0)
                    .unwrap_or(m.stock);
                (m.index, obj)
            })
            .collect()
    }

    /// The materials a family actually re-points (they sample its style
    /// texture).
    pub fn styled_materials(&self, family: usize) -> Vec<usize> {
        self.materials
            .iter()
            .filter(|m| m.family.get(family).copied().unwrap_or(0) != 0)
            .map(|m| m.index)
            .collect()
    }

    /// Every record's flags under `mode`: the styled records through
    /// `transform(build_flags, mode)`, the rest as built.
    pub fn record_flags(
        &self,
        mode: RecordMode,
        transform: impl Fn(u32, RecordMode) -> u32,
    ) -> Vec<u32> {
        self.record_base
            .iter()
            .zip(self.record_styled.iter())
            .map(|(&b, &styled)| if styled { transform(b, mode) } else { b })
            .collect()
    }

    /// Whether any record is styled at all (nothing to switch otherwise).
    pub fn has_styled_records(&self) -> bool {
        self.record_styled.iter().any(|&s| s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mat(index: usize, stock: usize, fams: &[(usize, usize)]) -> RigMaterial {
        let mut family = [0usize; FAMILY_SLOTS];
        for &(k, p) in fams {
            family[k] = p;
        }
        RigMaterial {
            index,
            stock,
            family,
        }
    }

    fn rig() -> StyleRig {
        // Materials 0 and 2 captured (1 is blended / a screen); material 2
        // has no Universe container in slot 5.
        StyleRig::new(
            false,
            vec![
                mat(0, 0x100, &[(0, 0x10), (1, 0x11), (5, 0x15)]),
                mat(2, 0x300, &[(0, 0x30), (1, 0x31)]),
            ],
            3,
            vec![0x0, 0x20, 0x1, 0x0],
            &[0, 1, 2, 0],
        )
    }

    #[test]
    fn records_follow_their_materials() {
        let r = rig();
        assert_eq!(r.record_styled, vec![true, false, true, true]);
        assert!(r.has_styled_records());
        let f = r.record_flags(RecordMode::Additive, |b, _| b | 0x40);
        assert_eq!(f, vec![0x40, 0x20, 0x41, 0x40]);
        // Base mode with the identity transform restores the build flags.
        assert_eq!(r.record_flags(RecordMode::Base, |b, _| b), r.record_base);
        // A record whose material index is out of range is never styled.
        let odd = StyleRig::new(false, vec![], 1, vec![0x7], &[usize::MAX]);
        assert!(!odd.has_styled_records());
    }

    #[test]
    fn material_writes_fall_back_to_stock() {
        let r = rig();
        assert_eq!(r.material_writes(None), vec![(0, 0x100), (2, 0x300)]);
        assert_eq!(r.material_writes(Some(1)), vec![(0, 0x11), (2, 0x31)]);
        assert_eq!(r.material_writes(Some(5)), vec![(0, 0x15), (2, 0x300)]);
        assert_eq!(
            r.material_writes(Some(FAMILY_SLOTS + 3)),
            vec![(0, 0x100), (2, 0x300)]
        );
        assert_eq!(r.styled_materials(5), vec![0]);
        assert_eq!(r.styled_materials(1), vec![0, 2]);
        assert!(r.applied.is_none() && r.texture_bound.is_none());
    }
}
