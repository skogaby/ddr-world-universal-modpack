//! AP2 document editing primitives — the mutation surface the S-Marvelous
//! feature's AFP patches drive (design §4.1): frame-label writes,
//! labeled-segment cloning with character-id remapping (verbatim and
//! placements-only variants), sprite-definition cloning under a fresh
//! character id, shape-definition injection, named-instance placement, and
//! translate adjustment of existing placements.
//!
//! Contract shared by every primitive (task requirement 2/3): returns
//! `Option`/count, never panics, and any failure leaves the document
//! UNCHANGED — all fallible validation runs before the first mutation, the
//! string-table intern (which leaves the table untouched on its own failure)
//! is the last fallible step, and everything after it is infallible. Packed-
//! field-width overflows an edit may produce (frame start > 20 bits, tag
//! count > u32, …) are caught by `Ap2Doc::serialize` returning `None` with
//! the document intact (task requirement 6).
//!
//! PlaceObject writes never go through a full re-encode of an existing
//! payload: `PlaceObject::build` only models a subset of the flags, so a
//! rebuild would silently drop unmodeled fields (label ids, blend modes, the
//! whole color/event/filter tail real segment animations carry). Instead the
//! two fields this surface touches — `source_tag_id` (clone remap) and
//! `translate` (placement adjustment) — are spliced at their byte offsets,
//! which the flag word pins deterministically (`place_field_offsets` mirrors
//! the `PlaceObject::view` walk). Decision record:
//! `.agents/scratchpad/2026-08-29-s-marvelous-judgement/editing-primitives/plan.md`.

use std::collections::HashMap;

use super::align4;
use super::model::{
    read_u16, read_u32, Ap2Doc, FrameSpan, Label, PlaceObject, PlaceObjectParams, PlaceObjectView,
    Shape, SpritePath, Tag, TagSection, TAG_DEFINE_EDIT_TEXT, TAG_DEFINE_FONT,
    TAG_DEFINE_MORPH_SHAPE, TAG_DEFINE_SPRITE, TAG_DEFINE_TEXT, TAG_IMAGE, TAG_SHAPE,
};

/// Character-id remap applied to the tags of a cloned segment (old id → new
/// id). Ids absent from the map stay shared with the source segment —
/// intentional (shared masks / helper sprites keep one definition).
pub type TagRemap = HashMap<u16, u16>;

/// The ids resolved/allocated by
/// [`Ap2Doc::clone_word_segment_with_new_shape`]. All dynamic — callers
/// bind the new geo as `{exported_name}_shape{new_shape_id}` and must never
/// assume specific values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WordSegmentClone {
    /// The source segment's word sprite (resolved by structure walk).
    pub word_sprite_id: u16,
    /// The freshly added `AP2_SHAPE`'s character id.
    pub new_shape_id: u16,
    /// The cloned word sprite's character id.
    pub new_sprite_id: u16,
}

/// Inputs for [`Ap2Doc::add_place_object_named`]: a create-mode placement
/// (source character + named instance + optional position), inserted at the
/// end of `frame`'s tag span.
pub struct NamedPlacement<'a> {
    /// Target frame index in the addressed section.
    pub frame: usize,
    /// Placement depth — must be unique among the PlaceObjects the target
    /// frame executes (docs/afp_system.md §9: duplicate depths silently
    /// overwrite the previous object).
    pub depth: u16,
    pub object_id: u16,
    /// Character to place (shape/sprite id, NOT a tag index).
    pub source_tag_id: u16,
    /// Instance (movie) name; interned into the string table.
    pub instance_name: &'a str,
    /// Optional position, raw fixed-point (/20) integers.
    pub translate: Option<(i32, i32)>,
}

impl Ap2Doc {
    /// Add a frame label to the section at `path`. Rejects duplicate names
    /// within that section (labels in OTHER sections may share the name —
    /// label maps are per-section) and frames past the section's current
    /// frame count. Interns the name; string-table appends never move
    /// existing offsets.
    pub fn add_label(&mut self, path: &SpritePath, name: &str, frame: u16) -> Option<()> {
        {
            let sec = self.section(path)?;
            if (frame as usize) >= sec.frames.len() {
                return None;
            }
            if sec.labels.iter().any(|l| l.name == name) {
                return None;
            }
        }
        // Last fallible step: intern leaves the table untouched on failure.
        let name_offset = self.strings.intern(name)?;
        let sec = self.section_mut(path)?; // validated above; cannot fail
        sec.labels.push(Label {
            frame,
            name_offset,
            name: name.to_string(),
        });
        Some(())
    }

    /// Add a new `AP2_SHAPE` definition with character id
    /// `max_character_id() + 1`, inserted at the END of `frame`'s tag span in
    /// the section at `path`. Returns the allocated id; the geo binding
    /// `{exported_name}_shape{id}` is the caller's concern.
    ///
    /// The definition goes INSIDE an executed frame span, before any tag a
    /// later insert into the same span would add — definitions must precede
    /// referencing PlaceObjects in the tag list (docs/afp_system.md §9; same
    /// placement rule as the shipped `core/afp.rs::patch_inject_children`).
    ///
    /// NOTE for callers: inserting into a section shifts the tag indices
    /// at/after the insertion point — recompute any held [`SpritePath`]
    /// through that section afterwards (`find_sprite_by_label`).
    pub fn add_shape(&mut self, path: &SpritePath, frame: usize, unknown: u16) -> Option<u16> {
        let id = self.max_character_id().checked_add(1)?;
        let sec = self.section_mut(path)?;
        insert_tags_at_frame_end(sec, frame, vec![Tag::Shape(Shape { unknown, id })])?;
        Some(id)
    }

    /// Clone the labeled segment `src_label` as `new_label` in the section at
    /// `path`. Segment rule: see [`segment_end`]. The covered tags are copied
    /// to the END of the tag list with character ids remapped through
    /// `remap` (see [`remap_tag`] for the id scope), the segment's frames are
    /// copied to the END of the frame list with spans re-pointed at the
    /// copies, and the new label points at the first appended frame.
    ///
    /// This is the VERBATIM clone — every covered tag duplicates, including
    /// definition tags. Right for segments that carry no definitions (and for
    /// tests asserting whole-segment duplication); for real templates whose
    /// segment frames carry the dictionary (the dance_judge template defines
    /// every character in frame 0 — research display-side-re.md §10), use
    /// [`Ap2Doc::clone_labeled_segment_placements_only`] instead: remapping
    /// inside a duplicated definition would corrupt the dictionary.
    ///
    /// Rejects: unknown path/label, a `new_label` already present in the
    /// section, segments whose spans point outside the tag list, a first
    /// appended frame index that no u16 label could address, and (with a
    /// non-empty remap) segments containing an undecodable PlaceObject.
    /// Packed-field limits are re-validated at serialize.
    pub fn clone_labeled_segment(
        &mut self,
        path: &SpritePath,
        src_label: &str,
        new_label: &str,
        remap: &TagRemap,
    ) -> Option<()> {
        // Phase 1 — read-only validation + clone construction.
        let (new_tags, new_frames, label_frame) = {
            let sec = self.section(path)?;
            let bounds = segment_clone_bounds(sec, src_label, new_label)?;
            let (start, end, label_frame) = (bounds.start, bounds.end, bounds.label_frame);

            // Covered tag range [lo, hi) across the segment's non-empty spans.
            let mut lo = usize::MAX;
            let mut hi = 0usize;
            for f in &sec.frames[start..end] {
                if f.tag_count > 0 {
                    lo = lo.min(f.start_tag as usize);
                    hi = hi.max((f.start_tag as usize).checked_add(f.tag_count as usize)?);
                }
            }
            let base = sec.tags.len();
            let base_u32 = u32::try_from(base).ok()?;
            let new_tags = if lo == usize::MAX {
                Vec::new() // all-empty segment: no tags to copy
            } else {
                if hi > sec.tags.len() {
                    return None; // span points past the tag list
                }
                let mut cloned = Vec::with_capacity(hi - lo);
                for tag in &sec.tags[lo..hi] {
                    cloned.push(remap_tag(tag, remap)?);
                }
                cloned
            };
            let mut new_frames = Vec::with_capacity(end - start);
            for f in &sec.frames[start..end] {
                new_frames.push(if f.tag_count == 0 {
                    // Span start is semantics-free at count 0; keep it inside
                    // the grown tag list.
                    FrameSpan {
                        start_tag: base_u32,
                        tag_count: 0,
                    }
                } else {
                    // lo is the min over non-empty spans, so start >= lo here.
                    let ns = f.start_tag as usize - lo + base;
                    FrameSpan {
                        start_tag: u32::try_from(ns).ok()?,
                        tag_count: f.tag_count,
                    }
                });
            }
            (new_tags, new_frames, label_frame)
        };

        // Phase 2 — last fallible step (table untouched on failure).
        let name_offset = self.strings.intern(new_label)?;

        // Phase 3 — infallible appends (appends never move existing
        // tag indices or frame numbers, so nothing else needs fixups).
        let sec = self.section_mut(path)?; // validated above; cannot fail
        sec.tags.extend(new_tags);
        sec.frames.extend(new_frames);
        sec.labels.push(Label {
            frame: label_frame,
            name_offset,
            name: new_label.to_string(),
        });
        Some(())
    }

    /// Clone the `DefineSprite` definition with character id `src_id` in the
    /// section at `path` under a fresh id (`max_character_id() + 1`, the
    /// returned value), with `remap` applied to the COPY's internals at every
    /// nesting level (per-tag id scope identical to [`remap_tag`]: Shape ids,
    /// nested DefineSprite ids, PlaceObject source ids via the surgical
    /// splice). The copy's OWN id field is always the fresh allocation —
    /// never a `remap` lookup. The original definition and every other tag
    /// are untouched; ids absent from the map stay shared with the original
    /// (shared sub-definitions keep one dictionary entry).
    ///
    /// The copy is inserted DIRECTLY AFTER the original definition, inside
    /// the same frame span (dictionary ordering: a definition precedes its
    /// first use — docs/afp_system.md §9), with the standard later-span
    /// fixups ([`insert_tags_in_frame`]). Inserting shifts tag indices
    /// at/after the insertion point — recompute held [`SpritePath`]s through
    /// this section afterwards.
    ///
    /// Rejects (doc unchanged): unknown path, no `DefineSprite` with `src_id`
    /// in the section (a `Shape` with that id does not count), a definition
    /// covered by no frame span (there is no "same span" to insert into),
    /// character-id space exhausted, and — with a non-empty remap — an
    /// undecodable PlaceObject anywhere in the copied tree (an uninspectable
    /// definition cannot be remapped safely; an empty remap copies verbatim).
    pub fn clone_sprite_definition(
        &mut self,
        path: &SpritePath,
        src_id: u16,
        remap: &TagRemap,
    ) -> Option<u16> {
        let new_id = self.max_character_id().checked_add(1)?;

        // Phase 1 — read-only validation + copy construction. The deep copy
        // is a private tree; a remap failure drops it with the doc untouched.
        let (copy, frame, insert_index) = {
            let sec = self.section(path)?;
            let ti = sec
                .tags
                .iter()
                .position(|t| matches!(t, Tag::DefineSprite(s) if s.id == src_id))?;
            // The first frame whose span covers the definition owns the
            // insert; a definition outside every executed span fails closed.
            let frame = sec.frames.iter().position(|f| {
                let s = f.start_tag as usize;
                s <= ti && ti - s < f.tag_count as usize
            })?;
            let Tag::DefineSprite(src) = &sec.tags[ti] else {
                return None; // unreachable: position matched DefineSprite
            };
            let mut copy = src.clone();
            remap_section_recursive(&mut copy.section, remap)?;
            copy.id = new_id;
            let insert_index = ti.checked_add(1)?;
            validate_insert_in_frame(sec, frame, insert_index, 1)?;
            (copy, frame, insert_index)
        };

        // Phase 2 — pre-validated above; cannot fail.
        let sec = self.section_mut(path)?;
        insert_tags_in_frame(sec, frame, insert_index, vec![Tag::DefineSprite(copy)])?;
        Some(new_id)
    }

    /// Clone the labeled segment `src_label` as `new_label`, copying ONLY the
    /// non-definition tags — the placements-only variant of
    /// [`Ap2Doc::clone_labeled_segment`]. Definition tags (classified by TAG
    /// ID, Opaque carriage included — see [`is_definition_tag_id`]) stay
    /// where they are: the AP2 dictionary persists across frames, so the
    /// cloned segment references the ORIGINAL definitions, re-pointed through
    /// `remap` where the caller substituted characters (the id scope of
    /// [`remap_tag`] applies to the cloned tags). Cloned frame spans shrink
    /// to the copied tags; a frame whose span held only definitions clones
    /// to a zero-tag frame (span count 0 with a semantics-free start index
    /// kept within the grown tag list — the serializer accepts count-0
    /// spans).
    ///
    /// This is the right variant for real templates whose segment frames
    /// carry the dictionary (dance_judge defines every character in frame 0);
    /// the verbatim variant remains for definition-free segments.
    ///
    /// Rejects (doc unchanged): the same set as the verbatim clone — unknown
    /// path/label, duplicate `new_label`, spans past the tag list, an
    /// unaddressable first appended frame, and (with a non-empty remap) an
    /// undecodable PlaceObject in the segment. Packed-field limits are
    /// re-validated at serialize.
    pub fn clone_labeled_segment_placements_only(
        &mut self,
        path: &SpritePath,
        src_label: &str,
        new_label: &str,
        remap: &TagRemap,
    ) -> Option<()> {
        // Phase 1 — read-only validation + clone construction. Unlike the
        // verbatim clone's contiguous [lo, hi) copy, the filter forces a
        // per-frame walk; under the module contract (consecutive disjoint
        // spans) the two traversals cover identical tags.
        let (new_tags, new_frames, label_frame) = {
            let sec = self.section(path)?;
            let bounds = segment_clone_bounds(sec, src_label, new_label)?;
            let base = u32::try_from(sec.tags.len()).ok()?;
            let mut new_tags: Vec<Tag> = Vec::new();
            let mut new_frames = Vec::with_capacity(bounds.end - bounds.start);
            for f in &sec.frames[bounds.start..bounds.end] {
                let s = f.start_tag as usize;
                let e = s.checked_add(f.tag_count as usize)?;
                if e > sec.tags.len() {
                    return None; // span points past the tag list
                }
                let span_start = base.checked_add(u32::try_from(new_tags.len()).ok()?)?;
                let mut copied = 0usize;
                for tag in &sec.tags[s..e] {
                    if is_definition_tag_id(tag.tag_id()) {
                        continue; // the dictionary is not duplicated
                    }
                    new_tags.push(remap_tag(tag, remap)?);
                    copied += 1;
                }
                new_frames.push(FrameSpan {
                    start_tag: span_start,
                    tag_count: u32::try_from(copied).ok()?,
                });
            }

            // OBJECT-ID REBASE — engine-mandated (live-RE, s-marvelous
            // deploys #10–#11 + two Cheat Engine sessions): in this engine
            // an object id doubles as the object's DEATH FRAME. The frame
            // executor's catch-up (`FUN_1800d4520`) only creates a
            // PlaceObject when `object_id > target_frame` (`CMP objid,
            // [RSP+0x50]; JLE skip` — [RSP+0x50] spills the executor's
            // target-frame argument at +0x45F8): objects that would die
            // before the seek target are skipped. Stock data obeys it
            // everywhere (word 32 dies ≈ frame 32; in_perfect's 70/76
            // follow its label 38; in_ng's 295/301 follow 263). Cloned
            // placements must therefore shift ids by the same distance the
            // FRAMES moved — new id = old + (new label frame − old label
            // frame) — preserving the id↔death-frame relationship for any
            // seek into or past the segment. Same shift for every record so
            // create/update pairs stay linked; object id 0 (depth-keyed
            // RemoveObject sentinels) stays 0.
            let frame_shift = u16::try_from(sec.frames.len())
                .ok()?
                .checked_sub(u16::try_from(bounds.start).ok()?)?;
            for tag in &mut new_tags {
                let Tag::PlaceObject(p) = tag else { continue };
                let old = read_u16(&p.data, 6)?;
                if old == 0 {
                    continue;
                }
                let new_id = old.checked_add(frame_shift)?;
                p.data.get_mut(6..8)?.copy_from_slice(&new_id.to_le_bytes());
            }

            (new_tags, new_frames, bounds.label_frame)
        };

        // Phase 2 — last fallible step (table untouched on failure).
        let name_offset = self.strings.intern(new_label)?;

        // Phase 3 — infallible appends (appends never move existing
        // tag indices or frame numbers, so nothing else needs fixups).
        let sec = self.section_mut(path)?; // validated above; cannot fail
        sec.tags.extend(new_tags);
        sec.frames.extend(new_frames);
        sec.labels.push(Label {
            frame: label_frame,
            name_offset,
            name: new_label.to_string(),
        });
        Some(())
    }

    /// The definition-aware "clone a labeled word segment onto fresh art"
    /// recipe (research display-side-re.md §10's patch shape, generalized):
    /// given the character id of the WORD SHAPE the segment's art chain
    /// bottoms out in (the shape whose geo references the word's texture
    /// region — resolved by the caller, since geo content lives outside the
    /// AP2 document), this
    ///
    /// 1. resolves the WORD SPRITE dynamically — the unique character the
    ///    `src_label` segment places whose `DefineSprite` (in the same
    ///    section) directly places `word_shape_id`;
    /// 2. adds a fresh `AP2_SHAPE` (donor `unknown` copied from the word
    ///    shape) into the frame span that carries the word shape's
    ///    definition — the new geo binding is `{exported}_shape{returned
    ///    new_shape_id}`, the caller's concern;
    /// 3. clones the word sprite's DEFINITION with the internal remap
    ///    `{word_shape_id → new_shape_id}`;
    /// 4. placements-only-clones the segment as `new_label` with the remap
    ///    `{word_sprite_id → new_sprite_id}`.
    ///
    /// Paths are re-resolved between steps (inserts shift tag indices).
    /// All ids come from the primitives' return values — nothing is
    /// hardcoded, so skin/build variants with different ids resolve
    /// automatically as long as the structure matches.
    ///
    /// Rejects (`None`): unknown `src_label`; `new_label` already present in
    /// the segment's section (pre-checked before any mutation); no `Shape`
    /// tag with `word_shape_id` in that section; the shape's definition
    /// covered by no frame span; a segment span pointing past the tag list;
    /// no — or more than one — candidate word sprite (ambiguity fails
    /// closed); or any underlying primitive failure.
    ///
    /// FAILURE CONTRACT: unlike the single primitives, the recipe is a
    /// composition and is NOT atomic — on `None` the document may be
    /// partially edited (e.g. the shape added but the clone refused).
    /// Callers must discard the document on `None` and fall back to the
    /// original bytes.
    pub fn clone_word_segment_with_new_shape(
        &mut self,
        src_label: &str,
        new_label: &str,
        word_shape_id: u16,
    ) -> Option<WordSegmentClone> {
        let path = self.find_sprite_by_label(src_label)?;

        // Read-only resolution of everything the mutations need.
        let (word_sprite_id, chain, donor_unknown, def_frame) = {
            let sec = self.section(&path)?;
            if sec.labels.iter().any(|l| l.name == new_label) {
                return None; // would fail at step 4 — refuse before mutating
            }

            // Donor shape: supplies the `unknown` field and the dictionary
            // frame (the frame whose span covers its definition).
            let shape_ti = sec
                .tags
                .iter()
                .position(|t| matches!(t, Tag::Shape(s) if s.id == word_shape_id))?;
            let Tag::Shape(donor) = &sec.tags[shape_ti] else {
                return None; // unreachable: position matched Shape
            };
            let donor_unknown = donor.unknown;
            let def_frame = sec.frames.iter().position(|f| {
                let s = f.start_tag as usize;
                s <= shape_ti && shape_ti - s < f.tag_count as usize
            })?;

            // Characters the segment places (decodable PlaceObjects only —
            // an undecodable one that matters fails the clone in step 4).
            let start = sec.label_frame(src_label)? as usize;
            if start >= sec.frames.len() {
                return None; // dangling source label
            }
            let end = segment_end(sec, start);
            let mut candidates: Vec<u16> = Vec::new();
            for f in &sec.frames[start..end] {
                let s = f.start_tag as usize;
                let e = s.checked_add(f.tag_count as usize)?;
                if e > sec.tags.len() {
                    return None; // span points past the tag list
                }
                for tag in &sec.tags[s..e] {
                    if let Tag::PlaceObject(po) = tag {
                        if let Some(id) = po.view().and_then(|v| v.source_tag_id) {
                            if !candidates.contains(&id) {
                                candidates.push(id);
                            }
                        }
                    }
                }
            }

            // The word sprite: the unique candidate whose DefineSprite
            // reaches the word shape through a placement chain of
            // same-section sprites. The live templates differ in depth —
            // dance_judge0000_v0: one level (sprite 35 → shape 32);
            // dance_judge_v3 (the package the cabinet actually loads,
            // deploy #2 finding): three levels (46 → 43 → 42 → shape 41).
            let mut word: Option<(u16, Vec<u16>)> = None;
            for cid in candidates {
                if let Some(chain) = word_chain(sec, cid, word_shape_id, 8) {
                    if word.is_some() {
                        return None; // ambiguous word chain — fail closed
                    }
                    word = Some((cid, chain));
                }
            }
            let (word_sprite_id, chain) = word?;
            (word_sprite_id, chain, donor_unknown, def_frame)
        };

        // The §10 patch sequence (task-01 API notes): inserts shift tag
        // indices at/after the insertion point — re-resolve the path after
        // every mutating step; use returned ids, never constants.
        //
        // The chain clones bottom-up (deepest sprite first) with cascading
        // remaps: the deepest clone remaps the word shape to the new shape;
        // each level above remaps its child sprite to that child's clone;
        // the segment clone finally remaps the top sprite. Mid-chain
        // failure leaves earlier clones behind (the recipe's documented
        // non-atomic-on-None contract — the caller discards the scratch doc
        // and streams stock bytes).
        let new_shape_id = self.add_shape(&path, def_frame, donor_unknown)?;
        let mut mapped_old = word_shape_id;
        let mut mapped_new = new_shape_id;
        for &sprite_id in chain.iter().rev() {
            let path = self.find_sprite_by_label(src_label)?;
            let remap = TagRemap::from([(mapped_old, mapped_new)]);
            mapped_new = self.clone_sprite_definition(&path, sprite_id, &remap)?;
            mapped_old = sprite_id;
        }
        let path = self.find_sprite_by_label(src_label)?;
        let segment_remap = TagRemap::from([(mapped_old, mapped_new)]);
        self.clone_labeled_segment_placements_only(&path, src_label, new_label, &segment_remap)?;

        // The template carries the labeled segment in MORE THAN ONE section:
        // the live dance_judge clip's VISIBLE timeline is an `aep_dummy`
        // child playing the inner sprite's section (own label table, own
        // frame numbering — deploys #6–#8: a root-only clone left the label
        // resolving to root numbering while the visible timeline had
        // neither the label nor the frames ⇒ blank word). Clone the segment
        // into EVERY other section carrying `src_label` with the same remap
        // — definitions are root-global, so the cloned chain serves all of
        // them; each section's new label gets ITS OWN local frame number.
        let mut extra_paths: Vec<SpritePath> = Vec::new();
        collect_sections_with_label(&self.root, src_label, &mut Vec::new(), &mut extra_paths);
        for p in extra_paths {
            if p == path {
                continue; // primary clone already done
            }
            self.clone_labeled_segment_placements_only(&p, src_label, new_label, &segment_remap)?;
        }

        Some(WordSegmentClone {
            word_sprite_id,
            new_shape_id,
            new_sprite_id: mapped_new,
        })
    }

    /// Resolve the WORD SHAPE of a labeled segment's section by GEO
    /// content: the unique `AP2_SHAPE` whose geo (name
    /// `{exported_name}_shape{id}`, looked up by the caller-supplied
    /// closure) carries a region label ending `_{suffix}`. Returns
    /// `(shape_id, region_label)`.
    ///
    /// Geo-first on purpose (cabinet deploy #3 root cause): identifying the
    /// word by walking sprite placements breaks across template
    /// generations — the live `dance_judge_v3` nests the shape three
    /// sprites deep where the v0-skin template used one. The geo → region
    /// binding is the invariant. Ambiguity (two matching geos) fails
    /// closed. The closure returns the geo's LABELS (region names) or
    /// `None` when the geo doesn't exist/parse; this module stays
    /// geo-format-agnostic (callers feed `core::geo::labels`).
    pub fn find_word_shape_by_geo(
        &self,
        src_label: &str,
        suffix: &str,
        mut geo_labels: impl FnMut(&str) -> Option<Vec<String>>,
    ) -> Option<(u16, String)> {
        let path = self.find_sprite_by_label(src_label)?;
        let sec = self.section(&path)?;
        let want = format!("_{suffix}");
        let mut found: Option<(u16, String)> = None;
        for tag in &sec.tags {
            let Tag::Shape(shape) = tag else { continue };
            let geo_name = format!("{}_shape{}", self.exported_name(), shape.id);
            let Some(labels) = geo_labels(&geo_name) else {
                continue;
            };
            let Some(region) = labels.iter().find(|l| l.ends_with(&want)) else {
                continue;
            };
            if found.is_some() {
                return None; // two word-art geos — ambiguous, fail closed
            }
            found = Some((shape.id, region.clone()));
        }
        found
    }

    /// Build a create-mode PlaceObject (source character + instance name +
    /// optional translate) via `PlaceObject::build` and insert it at the END
    /// of `placement.frame`'s tag span in the section at `path`, shifting
    /// later frame spans (see [`insert_tags_at_frame_end`]).
    ///
    /// The depth must be unique among the PlaceObjects the target frame
    /// already executes; every PlaceObject in that frame must decode for the
    /// check to be provable, else `None`.
    pub fn add_place_object_named(
        &mut self,
        path: &SpritePath,
        placement: &NamedPlacement<'_>,
    ) -> Option<()> {
        let mut params = PlaceObjectParams {
            depth: placement.depth,
            object_id: placement.object_id,
            source_tag_id: Some(placement.source_tag_id),
            movie_name_offset: Some(0), // placeholder until interned below
            translate: placement.translate,
            ..PlaceObjectParams::default()
        };

        // Phase 1 — read-only validation, including a pre-flight encode and
        // the insert-point arithmetic, so nothing after the intern can fail.
        {
            let sec = self.section(path)?;
            let span = *sec.frames.get(placement.frame)?;
            let s = span.start_tag as usize;
            let e = s.checked_add(span.tag_count as usize)?;
            if e > sec.tags.len() {
                return None;
            }
            for tag in &sec.tags[s..e] {
                if let Tag::PlaceObject(p) = tag {
                    // Fail closed on undecodable placements: depth
                    // uniqueness would be unprovable.
                    if p.view()?.depth == placement.depth {
                        return None;
                    }
                }
            }
            frame_end_insert_point(sec, placement.frame, 1)?;
            PlaceObject::build(&params)?;
        }

        // Phase 2 — last fallible step.
        params.movie_name_offset = Some(self.strings.intern(placement.instance_name)?);

        // Phase 3 — pre-flighted above; cannot fail.
        let po = PlaceObject::build(&params)?;
        let sec = self.section_mut(path)?;
        insert_tags_at_frame_end(sec, placement.frame, vec![Tag::PlaceObject(po)])
    }

    /// For every PlaceObject in the document (root and nested sprites, file
    /// order) whose decoded view matches `pred`, shift its translate by
    /// `dxy` (raw fixed-point /20 units). Returns the number adjusted.
    ///
    /// The write is a surgical splice of the 8 translate bytes at their
    /// flag-determined offset — every other byte of the payload is preserved
    /// exactly (see the module docs for why a `PlaceObject::build` rebuild is
    /// not used). Documented limitation, log-free by design (the caller
    /// compares the returned count against its expectation and WARNs):
    /// matched tags WITHOUT a translate field (flag 0x400 clear) are skipped
    /// and not counted, as are tags whose payload does not decode or whose
    /// adjusted component would overflow i32.
    pub fn adjust_placements(
        &mut self,
        pred: impl Fn(&PlaceObjectView) -> bool,
        dxy: (i32, i32),
    ) -> usize {
        fn walk(
            sec: &mut TagSection,
            pred: &dyn Fn(&PlaceObjectView) -> bool,
            dxy: (i32, i32),
        ) -> usize {
            let mut adjusted = 0usize;
            for tag in &mut sec.tags {
                match tag {
                    Tag::PlaceObject(p) => {
                        let Some(view) = p.view() else { continue };
                        if !pred(&view) {
                            continue;
                        }
                        let Some(offsets) = place_field_offsets(&p.data) else {
                            continue;
                        };
                        let Some(at) = offsets.translate else {
                            continue;
                        };
                        let Some((tx, ty)) = view.translate else {
                            continue;
                        };
                        let (Some(nx), Some(ny)) = (tx.checked_add(dxy.0), ty.checked_add(dxy.1))
                        else {
                            continue;
                        };
                        // Splice into a fresh copy, then swap — per-tag atomic.
                        let mut data = p.data.clone();
                        let Some(dst) = data.get_mut(at..at + 8) else {
                            continue;
                        };
                        dst[..4].copy_from_slice(&nx.to_le_bytes());
                        dst[4..].copy_from_slice(&ny.to_le_bytes());
                        p.data = data;
                        adjusted += 1;
                    }
                    Tag::DefineSprite(s) => adjusted += walk(&mut s.section, pred, dxy),
                    _ => {}
                }
            }
            adjusted
        }
        walk(&mut self.root, &pred, dxy)
    }
}

// ---------------------------------------------------------------------------
// Shared helpers.
// ---------------------------------------------------------------------------

/// Segment boundary rule (task Background): a labeled segment spans frames
/// `[label_frame, next)` where `next` is the smallest label frame STRICTLY
/// greater than the segment's own, or the section frame count when no later
/// label exists (clamped to the frame count against dangling labels).
/// The unique placement chain `[root, ..., innermost]` from sprite `root`
/// down to the sprite that DIRECTLY places `shape` — following placements
/// into DefineSprites of the SAME section (the dictionary lives beside the
/// segment in the shipped templates; §10 + the v3 amendment). `None` when
/// the shape is unreachable, reachable through more than one child
/// (ambiguous — fail closed), or deeper than `depth_left` (cycle guard).
fn word_chain(sec: &TagSection, root: u16, shape: u16, depth_left: u8) -> Option<Vec<u16>> {
    if depth_left == 0 {
        return None;
    }
    let sprite = sec.tags.iter().find_map(|t| match t {
        Tag::DefineSprite(sp) if sp.id == root => Some(sp),
        _ => None,
    })?;
    // Direct placements of this sprite's own timeline (updates without a
    // source id are ignored).
    let mut placed: Vec<u16> = Vec::new();
    for t in &sprite.section.tags {
        if let Tag::PlaceObject(po) = t {
            if let Some(id) = po.view().and_then(|v| v.source_tag_id) {
                if !placed.contains(&id) {
                    placed.push(id);
                }
            }
        }
    }
    if placed.contains(&shape) {
        return Some(vec![root]);
    }
    let mut found: Option<Vec<u16>> = None;
    for child in placed {
        if child == root {
            continue;
        }
        if let Some(mut chain) = word_chain(sec, child, shape, depth_left - 1) {
            if found.is_some() {
                return None; // two children reach the shape — ambiguous
            }
            chain.insert(0, root);
            found = Some(chain);
        }
    }
    found
}

fn segment_end(sec: &TagSection, start: usize) -> usize {
    sec.labels
        .iter()
        .map(|l| l.frame as usize)
        .filter(|&f| f > start)
        .min()
        .unwrap_or(sec.frames.len())
        .min(sec.frames.len())
}

/// Resolved bounds for a labeled-segment clone (shared by the verbatim and
/// placements-only variants).
struct SegmentCloneBounds {
    /// First frame of the source segment.
    start: usize,
    /// One past the last frame of the source segment ([`segment_end`]).
    end: usize,
    /// The frame index the new label will point at (== current frame count).
    label_frame: u16,
}

/// Shared validation prelude for the two labeled-segment clones: rejects a
/// `new_label` already present in the section, resolves the source label
/// (rejecting dangling labels), computes the segment bounds, and checks the
/// first appended frame is addressable by a u16 name reference.
fn segment_clone_bounds(
    sec: &TagSection,
    src_label: &str,
    new_label: &str,
) -> Option<SegmentCloneBounds> {
    if sec.labels.iter().any(|l| l.name == new_label) {
        return None;
    }
    let start = sec.label_frame(src_label)? as usize;
    if start >= sec.frames.len() {
        return None; // dangling source label
    }
    let end = segment_end(sec, start);
    // The new label must be able to address the first appended frame.
    let label_frame = u16::try_from(sec.frames.len()).ok()?;
    Some(SegmentCloneBounds {
        start,
        end,
        label_frame,
    })
}

/// Definition-class tag ids — the AP2 dictionary entries a placements-only
/// segment clone must NOT duplicate. Classification is by TAG ID (not by
/// modeled type) so definition-class tags carried as [`Tag::Opaque`] are
/// covered: font (0x78), sprite (0x79), text (0x7D), edit text (0x7E),
/// morph shape (0x82), image (0x83), shape (0x84) — ids from bemaniutils
/// `bemani/format/afp/types/ap2.py`.
fn is_definition_tag_id(id: u16) -> bool {
    matches!(
        id,
        TAG_DEFINE_FONT
            | TAG_DEFINE_SPRITE
            | TAG_DEFINE_TEXT
            | TAG_DEFINE_EDIT_TEXT
            | TAG_DEFINE_MORPH_SHAPE
            | TAG_IMAGE
            | TAG_SHAPE
    )
}

/// Validate an insert of `n` tags at tag index `insert_index`, attributed to
/// `frame`'s span: bounds, span sanity, containment (the index must lie
/// within the frame's span, `start ..= end`), and the fixup arithmetic
/// (hand-built models may carry out-of-range spans; never panic, never
/// wrap). Read-only — usable as a pre-flight check.
fn validate_insert_in_frame(
    sec: &TagSection,
    frame: usize,
    insert_index: usize,
    n: usize,
) -> Option<()> {
    let n32 = u32::try_from(n).ok()?;
    let span = sec.frames.get(frame)?;
    let s = span.start_tag as usize;
    let e = s.checked_add(span.tag_count as usize)?;
    if e > sec.tags.len() {
        return None; // span points past the tag list — refuse to guess
    }
    if insert_index < s || insert_index > e {
        return None; // not inside the target frame's span
    }
    span.tag_count.checked_add(n32)?;
    for (i, f) in sec.frames.iter().enumerate() {
        if i != frame && (f.start_tag as usize) >= insert_index {
            f.start_tag.checked_add(n32)?;
        }
    }
    Some(())
}

/// Tag index at the END of `frame`'s span, validated for an insert of `n`
/// tags there ([`validate_insert_in_frame`]). Read-only pre-flight.
fn frame_end_insert_point(sec: &TagSection, frame: usize, n: usize) -> Option<usize> {
    let span = sec.frames.get(frame)?;
    let insert_index = (span.start_tag as usize).checked_add(span.tag_count as usize)?;
    validate_insert_in_frame(sec, frame, insert_index, n)?;
    Some(insert_index)
}

/// Insert `new_tags` at `insert_index` within `frame`'s span — the ONE
/// FrameSpan-fixup implementation (the shipped
/// `core/afp.rs::patch_inject_children` rule, generalized from end-of-span
/// to any in-span index): the target frame's `tag_count` grows by
/// `new_tags.len()`, and every OTHER frame whose `start_tag` is at/after the
/// insertion index shifts right. Labels reference frame NUMBERS, never tag
/// indices, so they are structurally stable here.
///
/// Contract note: real AP2 files use consecutive disjoint frame spans; a
/// frame whose span STRADDLES the insertion point (possible only with
/// overlapping spans) keeps its count and would absorb the leading inserted
/// tags — same behavior as the shipped injector, out of contract.
///
/// Validates before mutating; `None` leaves the section untouched.
fn insert_tags_in_frame(
    sec: &mut TagSection,
    frame: usize,
    insert_index: usize,
    new_tags: Vec<Tag>,
) -> Option<()> {
    validate_insert_in_frame(sec, frame, insert_index, new_tags.len())?;
    let n32 = new_tags.len() as u32; // fits: validated by the try_from above
    sec.tags.splice(insert_index..insert_index, new_tags);
    for (i, f) in sec.frames.iter_mut().enumerate() {
        if i == frame {
            f.tag_count += n32;
        } else if (f.start_tag as usize) >= insert_index {
            f.start_tag += n32;
        }
    }
    Some(())
}

/// Insert `new_tags` at the END of `frame`'s span (the common case:
/// definitions and placements appended inside an executed frame) — a thin
/// wrapper over [`insert_tags_in_frame`].
fn insert_tags_at_frame_end(sec: &mut TagSection, frame: usize, new_tags: Vec<Tag>) -> Option<()> {
    let span = sec.frames.get(frame)?;
    let insert_index = (span.start_tag as usize).checked_add(span.tag_count as usize)?;
    insert_tags_in_frame(sec, frame, insert_index, new_tags)
}

/// Apply the character-id remap to ONE tag in place — the shared body of
/// [`remap_tag`] (clone-then-remap for segment clones) and
/// [`remap_section_recursive`] (all-levels remap of a sprite-definition
/// copy's private tree). Id scope: `Shape.id`, `DefineSprite.id`, and the
/// tag's own `PlaceObject` source id — NO recursion here. The PlaceObject id
/// is spliced at the field's deterministic byte offset (see the module
/// docs). With a non-empty remap, a PlaceObject whose payload cannot be
/// walked fails — a tag we cannot fully inspect cannot be remapped safely;
/// with an empty remap every tag passes untouched.
fn remap_tag_in_place(tag: &mut Tag, remap: &TagRemap) -> Option<()> {
    if remap.is_empty() {
        return Some(());
    }
    match tag {
        Tag::Shape(s) => {
            if let Some(&new_id) = remap.get(&s.id) {
                s.id = new_id;
            }
        }
        Tag::DefineSprite(s) => {
            if let Some(&new_id) = remap.get(&s.id) {
                s.id = new_id;
            }
        }
        Tag::PlaceObject(p) => {
            let offsets = place_field_offsets(&p.data)?;
            if let Some(at) = offsets.source_tag_id {
                let current = read_u16(&p.data, at)?;
                if let Some(&new_id) = remap.get(&current) {
                    p.data
                        .get_mut(at..at + 2)?
                        .copy_from_slice(&new_id.to_le_bytes());
                }
            }
        }
        Tag::Opaque(_) => {}
    }
    Some(())
}

/// Clone one tag applying the character-id remap ([`remap_tag_in_place`]'s
/// scope — no recursion into nested sprite sections: a segment clone's
/// nested sections are SHARED definitions the rest of the document still
/// references).
fn remap_tag(tag: &Tag, remap: &TagRemap) -> Option<Tag> {
    let mut cloned = tag.clone();
    remap_tag_in_place(&mut cloned, remap)?;
    Some(cloned)
}

/// Apply the remap to every tag of `sec` AND recursively to nested
/// DefineSprite sections — [`remap_tag_in_place`]'s id scope extended
/// through all nesting levels. Only sound on a PRIVATE tree (a fresh deep
/// copy, [`Ap2Doc::clone_sprite_definition`]): recursing into a shared
/// nested section would remap definitions the original timeline still uses.
fn remap_section_recursive(sec: &mut TagSection, remap: &TagRemap) -> Option<()> {
    if remap.is_empty() {
        return Some(());
    }
    for tag in &mut sec.tags {
        remap_tag_in_place(tag, remap)?;
        if let Tag::DefineSprite(s) = tag {
            remap_section_recursive(&mut s.section, remap)?;
        }
    }
    Some(())
}

/// Byte offsets of the two PlaceObject fields the editing surface splices.
struct PlaceFieldOffsets {
    /// Offset of the u16 source character id (flag 0x2).
    source_tag_id: Option<usize>,
    /// Offset of the i32 tx/ty pair (flag 0x400).
    translate: Option<usize>,
}

/// Walk a PlaceObject payload exactly like `PlaceObject::view` (field ORDER
/// from bemaniutils swf.py ~1281, including the second flag word and the
/// mid-payload realign-to-4) recording the byte offsets of the spliceable
/// fields. `None` when the payload is too short for the fields its flags
/// claim — bounds-checked like the view, never panics.
fn place_field_offsets(d: &[u8]) -> Option<PlaceFieldOffsets> {
    let flags32 = read_u32(d, 0)?;
    read_u16(d, 4)?; // depth — presence check only
    read_u16(d, 6)?; // object id
    let mut p = 8usize;
    let mut flags = flags32 as u64;
    if flags32 & 0x8000_0000 != 0 {
        let more = read_u32(d, p)?;
        p += 4;
        flags |= (more as u64) << 32;
    }
    let mut source_tag_id = None;
    if flags & 0x2 != 0 {
        read_u16(d, p)?;
        source_tag_id = Some(p);
        p += 2;
    }
    for bit in [0x10u64, 0x20, 0x40] {
        if flags & bit != 0 {
            read_u16(d, p)?;
            p += 2;
        }
    }
    if flags & 0x20000 != 0 {
        d.get(p)?;
        p += 1;
    }
    p = align4(p);
    for bit in [0x100u64, 0x200] {
        if flags & bit != 0 {
            read_u32(d, p)?;
            read_u32(d, p + 4)?;
            p += 8;
        }
    }
    let mut translate = None;
    if flags & 0x400 != 0 {
        read_u32(d, p)?;
        read_u32(d, p + 4)?;
        translate = Some(p);
    }
    Some(PlaceFieldOffsets {
        source_tag_id,
        translate,
    })
}

/// Collect the paths of EVERY section (root included, as the empty path)
/// carrying a label named `label`. Recursive over nested DefineSprite
/// sections — the walk mirrors `Ap2Doc::find_sprite_by_label` but returns
/// all hits instead of the first.
fn collect_sections_with_label(
    sec: &TagSection,
    label: &str,
    path: &mut Vec<usize>,
    out: &mut Vec<SpritePath>,
) {
    if sec.labels.iter().any(|l| l.name == label) {
        out.push(SpritePath {
            tag_indices: path.clone(),
        });
    }
    for (i, tag) in sec.tags.iter().enumerate() {
        if let Tag::DefineSprite(s) = tag {
            path.push(i);
            collect_sections_with_label(&s.section, label, path, out);
            path.pop();
        }
    }
}
