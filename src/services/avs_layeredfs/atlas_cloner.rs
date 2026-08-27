//! Atlas cloning — build ARGB8888REV + AVSLZ cloned atlases at donor
//! positions, emit a `texturelist.merged.xml` that declares the new
//! atlases and their `<image>` entries.
//!
//! Used by mods that need to inject new texture names into an existing
//! IFS while keeping the stock atlas untouched. Each cloned atlas is a
//! blank canvas the same size as the donor's parent atlas, with custom
//! PNGs composited at the donor images' pixel rects. Layout and texture
//! coordinates are preserved verbatim so geo/AFP files that reference
//! the new texture names can read them with the same UV math the donor
//! textures use.
//!
//! Output:
//!   * Atlas data written to `<cache_root>/<ifs_mod_path>/<md5(atlas_name)>`,
//!     in the AVSLZ-prefixed form LayeredFS expects when the parent IFS
//!     advertises AVSLZ compression.
//!   * `texturelist.merged.xml` written to
//!     `<mod_root>/<ifs_mod_path>/tex/texturelist.merged.xml`, appended
//!     to by LayeredFS's XML merger at IFS mount time.
//!
//! Typical call order for a mod:
//!   1. Extract the stock IFS's `texturelist.xml` (via the game's ARC +
//!      IFS extractors, or by reading a pre-extracted copy).
//!   2. Build a `Vec<NewTextureSpec>` pairing each new texture name
//!      with a donor texture and a PNG path.
//!   3. Call [`generate_cloned_atlases`].
//!   4. LayeredFS picks up the merged xml + cache files at scene load.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::core::{arc, ifs};
use crate::services::avs_layeredfs::cache_hasher::CacheHasher;
use crate::{log_info, log_warn};

/// Latched true when [`generate_cloned_atlases_cached`] actually REBUILT an
/// atlas batch this boot (cold `_cache`, changed inputs, or a deleted merged
/// texturelist) instead of serving it from cache. A rebuild during boot means
/// the game's already-mounted texture lists missed the freshly generated
/// assets — the textures render blank until the next launch — so the splash
/// screen (lib.rs) shows a "reboot at least once" warning when this is set.
static ATLASES_REBUILT_THIS_BOOT: AtomicBool = AtomicBool::new(false);

/// Whether any cache-guarded atlas batch was (re)generated this boot.
/// See [`ATLASES_REBUILT_THIS_BOOT`].
pub fn atlases_rebuilt_this_boot() -> bool {
    ATLASES_REBUILT_THIS_BOOT.load(Ordering::Relaxed)
}

/// One net-new texture to inject by cloning a donor atlas position.
///
/// The donor's parent atlas determines the clone's dimensions; the
/// donor's imgrect/uvrect determine where the new PNG is composited
/// **unless** a previously-emitted spec in the same atlas already
/// claimed that position — in which case the clone allocator packs this
/// spec into a fresh unoccupied rect (see [`allocate_packed_rect`]).
/// Multiple specs can share a donor parent atlas, in which case they
/// all go into a single cloned atlas.
pub struct NewTextureSpec<'a> {
    pub new_name: &'a str,
    pub donor_name: &'a str,
    pub png_path: &'a str,
}

/// Atlas dimensions are constrained to powers of two; when the stock
/// atlas is already full we grow up to this maximum before giving up.
/// Matches the hard limit in [`crate::services::avs_layeredfs::texture_packer`].
const MAX_ATLAS_SIDE: u32 = 4096;

/// Fixed width for a fresh (non-donor-cloned) atlas. Wide enough for several
/// columns of typical injected textures (preview images ~134px, ribbons
/// ~132px → ~15 columns) so the shelf packer fills rows rather than stacking
/// one-per-row and ballooning the height. The shelf packer grows only the
/// height from here as shelves accumulate.
const FRESH_ATLAS_WIDTH: u32 = 2048;

/// Starting height for a fresh atlas; grows by powers of two as shelves are
/// added. Small so a few textures produce a short atlas.
const FRESH_ATLAS_START_HEIGHT: u32 = 256;

/// Minimum 1-pixel gap between packed textures (matches the inset the
/// stock imgrect/uvrect encoding expects — see `emit_image_xml`).
const PACK_PADDING: u32 = 2;

/// A claimed pixel rectangle inside an atlas. Half-open on the max side:
/// the rect occupies `x in [x_min, x_max)` and `y in [y_min, y_max)`.
#[derive(Clone, Copy, Debug)]
struct PixelRect {
    x_min: u32,
    y_min: u32,
    x_max: u32,
    y_max: u32,
}

impl PixelRect {
    fn intersects(&self, other: &PixelRect) -> bool {
        self.x_min < other.x_max
            && other.x_min < self.x_max
            && self.y_min < other.y_max
            && other.y_min < self.y_max
    }
    fn width(&self) -> u32 {
        self.x_max - self.x_min
    }
    fn height(&self) -> u32 {
        self.y_max - self.y_min
    }
}

/// One injected texture's resolved position within an atlas, ready to
/// composite. Produced by the packing pass, consumed by
/// [`finalize_cloned_atlas`].
struct Placement {
    new_name: String,
    png_path: String,
    rect: PixelRect,
}

/// A fully-packed atlas: the placements it holds plus its final dimensions
/// (after any growth during packing). One group produces a single build in
/// donor-clone mode, or several in fresh mode when previews spill across
/// multiple atlases.
struct AtlasBuild {
    placements: Vec<Placement>,
    atlas_w: u32,
    atlas_h: u32,
}

/// Generate cloned atlases + a `texturelist.merged.xml` for a set of
/// new textures keyed off donor positions in an existing IFS's
/// texturelist.
///
/// `texlist_xml`: plaintext stock texturelist (already kbin-decoded).
/// `ifs_mod_path`: mod-folder-relative IFS directory name (e.g.
/// `"select_music_option_v3_ifs"`). `cache_root` and `mod_root` are
/// absolute or relative filesystem paths.
///
/// `custom_atlas_prefix` is used to name the cloned atlases
/// (`"<prefix>_<donor_atlas_index>"`). Keep it short and unique per
/// caller to avoid collisions across mods that inject into the same
/// target IFS.
///
/// Graceful degradation: unreadable PNGs, unknown donor names, and
/// file-write failures log at WARN and skip that entry. The function
/// returns `true` if at least one cloned atlas was produced, `false`
/// otherwise (all failures or empty spec list).
pub fn generate_cloned_atlases(
    texlist_xml: &str,
    ifs_mod_path: &str,
    cache_root: &str,
    mod_root: &str,
    custom_atlas_prefix: &str,
    specs: &[NewTextureSpec<'_>],
) -> bool {
    match generate_cloned_atlases_xml(
        texlist_xml,
        ifs_mod_path,
        cache_root,
        custom_atlas_prefix,
        specs,
    ) {
        Some(xml_fragment) => write_merged_texturelist(ifs_mod_path, mod_root, &xml_fragment),
        None => false,
    }
}

/// Like [`generate_cloned_atlases`] but returns the `<texture>` XML
/// fragments instead of writing `texturelist.merged.xml`. Cache files
/// are still written. Callers that need to aggregate multiple
/// independent atlas sets into one merged XML (e.g. one atlas per
/// custom folder) should call this per-set and then pass the
/// concatenated fragments to [`write_merged_texturelist`].
pub fn generate_cloned_atlases_xml(
    texlist_xml: &str,
    ifs_mod_path: &str,
    cache_root: &str,
    custom_atlas_prefix: &str,
    specs: &[NewTextureSpec<'_>],
) -> Option<String> {
    // Default mode: donor-slot-preserving (clone the donor's full atlas
    // footprint, first spec reuses the donor's own slot). Used by
    // folder/series expansion, where a cloned texture must sit exactly where
    // its donor was.
    generate_cloned_atlases_xml_impl(
        texlist_xml,
        ifs_mod_path,
        cache_root,
        custom_atlas_prefix,
        specs,
        false,
    )
}

/// `fresh_atlas = true`: pack all specs into a NEW, minimally-sized atlas
/// instead of cloning the donor atlas's full footprint. The donor entry is
/// still used for the imgrect/uvrect encoding conventions, but its sibling
/// textures are NOT treated as blockers and no spec reuses the donor's own
/// slot — every spec is packed from a small starting canvas that grows only
/// as needed.
///
/// This is the right mode for BULK injection of net-new, self-contained
/// textures (e.g. dozens/hundreds of option preview images): the cloned
/// `<image>` entries carry their own UVs into the cloned atlas, so they don't
/// need the donor atlas's other contents present. Cloning a crowded stock
/// atlas (e.g. 159 textures in a 2048² atlas) and threading new textures
/// around them forces growth to 4096² and a ~67 MB near-empty buffer to
/// compress — 20+ seconds. A fresh tight atlas is a fraction of that.
pub fn generate_cloned_atlases_xml_fresh(
    texlist_xml: &str,
    ifs_mod_path: &str,
    cache_root: &str,
    custom_atlas_prefix: &str,
    specs: &[NewTextureSpec<'_>],
) -> Option<String> {
    generate_cloned_atlases_xml_impl(
        texlist_xml,
        ifs_mod_path,
        cache_root,
        custom_atlas_prefix,
        specs,
        true,
    )
}

fn generate_cloned_atlases_xml_impl(
    texlist_xml: &str,
    ifs_mod_path: &str,
    cache_root: &str,
    custom_atlas_prefix: &str,
    specs: &[NewTextureSpec<'_>],
    fresh_atlas: bool,
) -> Option<String> {
    if specs.is_empty() {
        return None;
    }

    let atlas_entries = parse_texturelist(texlist_xml);

    let mut grouped: BTreeMap<String, Vec<(&NewTextureSpec<'_>, &TexEntry)>> = BTreeMap::new();
    for spec in specs {
        let mut donor_found = None;
        for (atlas_name, entries) in &atlas_entries {
            if let Some(e) = entries.iter().find(|e| e.image_name == spec.donor_name) {
                donor_found = Some((atlas_name.clone(), e));
                break;
            }
        }
        match donor_found {
            Some((atlas_name, entry)) => {
                grouped.entry(atlas_name).or_default().push((spec, entry));
            }
            None => {
                log_warn!(
                    "atlas_cloner[{}]: donor '{}' not in source texturelist — skipping '{}'",
                    ifs_mod_path,
                    spec.donor_name,
                    spec.new_name
                );
            }
        }
    }
    if grouped.is_empty() {
        return None;
    }

    let cache_dir = format!("{}/{}", cache_root, ifs_mod_path);
    if let Err(e) = std::fs::create_dir_all(&cache_dir) {
        log_warn!("atlas_cloner[{}]: mkdir {}: {}", ifs_mod_path, cache_dir, e);
        return None;
    }

    let mut xml_fragment = String::new();
    let mut any_written = 0usize;

    // Running index across every atlas this call emits. Donor-clone groups
    // produce exactly one atlas each; a fresh group can spill into several, so
    // names can't be derived from the group index — this gapless counter keeps
    // them unique and deterministic (`<prefix>_000`, `_001`, …).
    let mut atlas_seq = 0usize;

    for (donor_atlas_name, pairs) in grouped.iter() {
        let donor_atlas_entries: &[TexEntry] = atlas_entries
            .iter()
            .find(|(n, _)| n == donor_atlas_name)
            .map(|(_, e)| e.as_slice())
            .unwrap_or(&[]);
        // Donor-slot-preserving mode treats the donor atlas's existing
        // textures as blockers (so cloned textures land in the gaps and the
        // first spec can reuse the donor's exact slot). Fresh-atlas mode packs
        // into an empty canvas — no blockers, no donor-slot reuse — because
        // the new textures are self-contained (own UVs) and don't need the
        // donor atlas's other contents present.
        let blockers: Vec<PixelRect> = if fresh_atlas {
            Vec::new()
        } else {
            donor_atlas_entries
                .iter()
                .filter_map(|e| {
                    if e.pixel_w == 0 || e.pixel_h == 0 {
                        None
                    } else {
                        Some(PixelRect {
                            x_min: e.pixel_x,
                            y_min: e.pixel_y,
                            x_max: e.pixel_x + e.pixel_w,
                            y_max: e.pixel_y + e.pixel_h,
                        })
                    }
                })
                .collect()
        };

        // Pack this group into one atlas (donor mode) or as many as it takes
        // (fresh mode spills when an atlas fills), then composite + write each.
        for build in pack_group(ifs_mod_path, pairs, &blockers, fresh_atlas) {
            let custom_atlas_name = format!("{}_{:03}", custom_atlas_prefix, atlas_seq);
            if let Some(frag) = finalize_cloned_atlas(
                ifs_mod_path,
                &cache_dir,
                donor_atlas_name,
                &custom_atlas_name,
                &build,
            ) {
                xml_fragment.push_str(&frag);
                any_written += 1;
                atlas_seq += 1; // only advance on a real emit → gapless names
            }
        }
    }

    if any_written == 0 {
        return None;
    }
    Some(xml_fragment)
}

/// Pack one donor group's specs into one or more [`AtlasBuild`]s.
///
/// * **Donor-clone mode** (`fresh_atlas = false`): a single atlas sized to the
///   donor atlas. The first spec per donor reuses the donor's exact slot; the
///   rest pack into the gaps, with the atlas widening toward
///   [`MAX_ATLAS_SIDE`] if needed. A spec that still won't fit is skipped.
/// * **Fresh mode** (`fresh_atlas = true`): a fixed-width atlas that grows in
///   height only. When it fills (the packer reports no room), the placements so
///   far are sealed into one `AtlasBuild` and the remaining specs spill into a
///   new atlas — repeated until everything is placed. This is what lets the
///   caller hand over an unbounded number of preview images without guessing a
///   chunk size; the packer decides when to roll to the next atlas. A single
///   texture larger than a max-size atlas is skipped (logged).
fn pack_group(
    ifs_mod_path: &str,
    pairs: &[(&NewTextureSpec<'_>, &TexEntry)],
    blockers: &[PixelRect],
    fresh_atlas: bool,
) -> Vec<AtlasBuild> {
    let new_packer = || {
        let (start_w, start_h) = if fresh_atlas {
            (FRESH_ATLAS_WIDTH, FRESH_ATLAS_START_HEIGHT)
        } else {
            // Donor-slot mode must start at the donor atlas's real size (the
            // preserved slot is addressed in those coordinates). `pairs` is
            // non-empty here (every group has at least one spec).
            (pairs[0].1.atlas_w, pairs[0].1.atlas_h)
        };
        ShelfPacker::new(start_w, start_h, blockers.to_vec(), !fresh_atlas)
    };

    let mut builds: Vec<AtlasBuild> = Vec::new();
    let mut packer = new_packer();
    let mut placements: Vec<Placement> = Vec::new();
    let mut claimed_donors: Vec<u32> = Vec::new();

    for (spec, donor) in pairs {
        // Donor-slot reuse: only in donor mode, only the first spec per donor.
        let donor_key = (donor.pixel_x << 16) | donor.pixel_y;
        if !fresh_atlas && !claimed_donors.contains(&donor_key) {
            claimed_donors.push(donor_key);
            placements.push(Placement {
                new_name: spec.new_name.to_string(),
                png_path: spec.png_path.to_string(),
                rect: PixelRect {
                    x_min: donor.pixel_x,
                    y_min: donor.pixel_y,
                    x_max: donor.pixel_x + donor.pixel_w,
                    y_max: donor.pixel_y + donor.pixel_h,
                },
            });
            continue;
        }

        let (w, h) = image::image_dimensions(spec.png_path)
            .ok()
            .unwrap_or((donor.pixel_w, donor.pixel_h));

        let rect = match packer.place(w, h) {
            Some(r) => r,
            None if fresh_atlas => {
                // Fresh atlas is full. Seal what we have and spill into a new
                // one. If nothing's placed yet, this single texture exceeds a
                // max-size atlas — skip it rather than loop forever.
                if placements.is_empty() {
                    log_warn!(
                        "atlas_cloner[{}]: '{}' ({}x{}) too large for a {}x{} atlas — skipping",
                        ifs_mod_path,
                        spec.new_name,
                        w,
                        h,
                        FRESH_ATLAS_WIDTH,
                        MAX_ATLAS_SIDE
                    );
                    continue;
                }
                builds.push(AtlasBuild {
                    placements: std::mem::take(&mut placements),
                    atlas_w: packer.atlas_w,
                    atlas_h: packer.atlas_h,
                });
                packer = new_packer();
                match packer.place(w, h) {
                    Some(r) => r,
                    None => {
                        log_warn!(
                            "atlas_cloner[{}]: '{}' ({}x{}) too large for a fresh atlas — skipping",
                            ifs_mod_path,
                            spec.new_name,
                            w,
                            h
                        );
                        continue;
                    }
                }
            }
            None => {
                log_warn!(
                    "atlas_cloner[{}]: no room to pack '{}' ({}x{}) into {}x{} atlas even after growth — skipping",
                    ifs_mod_path, spec.new_name, w, h, packer.atlas_w, packer.atlas_h
                );
                continue;
            }
        };

        placements.push(Placement {
            new_name: spec.new_name.to_string(),
            png_path: spec.png_path.to_string(),
            rect,
        });
    }

    if !placements.is_empty() {
        builds.push(AtlasBuild {
            placements,
            atlas_w: packer.atlas_w,
            atlas_h: packer.atlas_h,
        });
    }
    builds
}

/// Composite one packed [`AtlasBuild`] into ARGB8888REV, AVSLZ-compress it,
/// write the cache blob keyed by `md5(custom_atlas_name)`, and return the
/// `<texture>` XML fragment declaring it. Returns `None` if nothing could be
/// composited (every PNG failed to load) or the cache write failed — in which
/// case the caller leaves `atlas_seq` unadvanced so the name is reused.
fn finalize_cloned_atlas(
    ifs_mod_path: &str,
    cache_dir: &str,
    donor_atlas_name: &str,
    custom_atlas_name: &str,
    build: &AtlasBuild,
) -> Option<String> {
    let (atlas_w, atlas_h) = (build.atlas_w, build.atlas_h);
    let mut rgba = vec![0u8; (atlas_w * atlas_h * 4) as usize];
    let mut composited = 0;
    let mut xml_images = String::new();

    for placement in &build.placements {
        let img = match image::open(&placement.png_path) {
            Ok(i) => i.into_rgba8(),
            Err(e) => {
                log_warn!(
                    "atlas_cloner[{}]: can't load PNG {}: {}",
                    ifs_mod_path,
                    placement.png_path,
                    e
                );
                continue;
            }
        };
        let rect = placement.rect;
        let src_w = img.width().min(rect.width());
        let src_h = img.height().min(rect.height());
        for y in 0..src_h {
            for x in 0..src_w {
                let px = img.get_pixel(x, y).0;
                let dst = (((rect.y_min + y) * atlas_w + (rect.x_min + x)) * 4) as usize;
                if dst + 3 < rgba.len() {
                    rgba[dst] = px[0];
                    rgba[dst + 1] = px[1];
                    rgba[dst + 2] = px[2];
                    rgba[dst + 3] = px[3];
                }
            }
        }
        composited += 1;
        xml_images.push_str(&emit_image_xml(&placement.new_name, &rect));
    }
    if composited == 0 {
        return None;
    }

    for pixel in rgba.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }

    let compressed = super::avslz::compress(&rgba);
    let mut final_data = Vec::with_capacity(8 + compressed.len());
    final_data.extend_from_slice(&(rgba.len() as u32).to_be_bytes());
    final_data.extend_from_slice(&(compressed.len() as u32).to_be_bytes());
    final_data.extend_from_slice(&compressed);

    let atlas_md5 = format!("{:x}", md5::compute(custom_atlas_name.as_bytes()));
    let cache_file = format!("{}/{}", cache_dir, atlas_md5);
    if let Err(e) = std::fs::write(&cache_file, &final_data) {
        log_warn!(
            "atlas_cloner[{}]: can't write atlas cache {}: {}",
            ifs_mod_path,
            cache_file,
            e
        );
        return None;
    }
    // Keep the in-memory cache index live. Cloned atlases written here at mod
    // enable() land after the init-time index scan, so a cold-cache run would
    // otherwise miss them on the texture hot path (handle_texture → cache_has).
    super::ifs_textures::cache_index_insert(&cache_file);

    log_info!(
        "atlas_cloner[{}]: cloned atlas {} -> {} ({}x{}, {} new texture(s))",
        ifs_mod_path,
        donor_atlas_name,
        custom_atlas_name,
        atlas_w,
        atlas_h,
        composited
    );

    Some(format!(
        "<texture format=\"argb8888rev\" mag_filter=\"linear\" min_filter=\"linear\" \
         name=\"{}\" wrap_s=\"clamp\" wrap_t=\"clamp\">\
         <size __type=\"2u16\">{} {}</size>{}</texture>\n",
        custom_atlas_name, atlas_w, atlas_h, xml_images
    ))
}

/// Write a `texturelist.merged.xml` from one or more XML fragments
/// produced by [`generate_cloned_atlases_xml`].
pub fn write_merged_texturelist(ifs_mod_path: &str, mod_root: &str, xml_fragment: &str) -> bool {
    let tex_dir = format!("{}/{}/tex", mod_root, ifs_mod_path);
    if let Err(e) = std::fs::create_dir_all(&tex_dir) {
        log_warn!("atlas_cloner[{}]: mkdir {}: {}", ifs_mod_path, tex_dir, e);
        return false;
    }
    let merged_path = format!("{}/texturelist.merged.xml", tex_dir);
    let merged_xml = format!("<texturelist>\n{}</texturelist>\n", xml_fragment);
    match std::fs::write(&merged_path, &merged_xml) {
        Ok(()) => {
            log_info!(
                "atlas_cloner[{}]: wrote merged texturelist to {}",
                ifs_mod_path,
                merged_path
            );
            true
        }
        Err(e) => {
            log_warn!(
                "atlas_cloner[{}]: can't write {}: {}",
                ifs_mod_path,
                merged_path,
                e
            );
            false
        }
    }
}

/// Owned counterpart of [`NewTextureSpec`] for the batch/cached API, so
/// callers can build specs from runtime `String`s without lifetime gymnastics.
pub struct OwnedTextureSpec {
    pub new_name: String,
    pub donor_name: String,
    pub png_path: String,
}

/// One cloned-atlas set within a batch: a distinct atlas prefix plus the
/// textures to inject under it. Multiple sets let a caller spread large or
/// numerous textures across several atlases (each capped at
/// [`MAX_ATLAS_SIDE`]) — e.g. one set for small labels/ribbons and several
/// chunked sets for large preview images. Every set in a batch merges into
/// the same IFS's `texturelist.merged.xml`.
pub struct AtlasSet {
    pub atlas_prefix: String,
    pub specs: Vec<OwnedTextureSpec>,
    /// When true, pack into a fresh minimally-sized atlas instead of cloning
    /// the donor atlas's full footprint (see
    /// [`generate_cloned_atlases_xml_fresh`]). Use for bulk net-new textures
    /// (e.g. preview images); leave false for small donor-slot-preserving
    /// sets (labels/ribbons cloned to a specific stock slot).
    pub fresh: bool,
}

/// Outcome of [`generate_cloned_atlases_cached`].
#[derive(Debug, PartialEq, Eq)]
pub enum BatchResult {
    /// Inputs unchanged since the last successful build AND the merged
    /// texturelist still exists — nothing was rebuilt.
    Cached,
    /// At least one atlas was rebuilt and the merged texturelist rewritten.
    Rebuilt,
    /// No atlas was produced (empty batch, or every spec failed).
    Nothing,
}

/// Bulk, cache-guarded atlas injection — the high-level entry point for any
/// mod that clones textures into an IFS. Builds every [`AtlasSet`] in `batch`,
/// then writes ONE merged `texturelist.merged.xml`, but **skips the entire
/// rebuild** when none of the inputs changed since the last successful run.
///
/// This is what keeps boot fast: decoding/packing/converting hundreds of
/// PNGs every launch is the expensive part, and it's pure waste when nothing
/// changed. The guard hashes (via [`CacheHasher`]):
/// - each spec's `png_path` + its mtime (a changed/added/removed PNG busts it),
/// - each spec's `new_name` + `donor_name` and the set's `atlas_prefix`
///   (a renamed/re-donored/re-prefixed spec busts it, even with equal mtimes),
/// - the stock `texlist_xml` content (a game/IFS update busts it).
///
/// On a hash match it additionally requires the merged XML to still exist —
/// so deleting `_cache` (or the merged file) forces a rebuild even if the
/// sidecar hash survived. The hash sidecar lives at
/// `<cache_root>/<ifs_mod_path>/<merged-cache-key>.atlasbatch.md5`.
///
/// Returns [`BatchResult`] so callers can log/branch; all failure modes are
/// graceful (a bad PNG is skipped per the underlying cloner).
pub fn generate_cloned_atlases_cached(
    texlist_xml: &str,
    ifs_mod_path: &str,
    cache_root: &str,
    mod_root: &str,
    batch: &[AtlasSet],
) -> BatchResult {
    let merged_path = format!("{}/{}/tex/texturelist.merged.xml", mod_root, ifs_mod_path);
    let hash_file = format!("{}/{}/atlasbatch.md5", cache_root, ifs_mod_path);

    // Hash all inputs that affect the output.
    let mut hasher = CacheHasher::new(&hash_file);
    hasher.add_str(texlist_xml);
    for set in batch {
        hasher.add_str(&set.atlas_prefix);
        hasher.add_str(if set.fresh { "fresh" } else { "donor" });
        for spec in &set.specs {
            hasher.add_str(&spec.new_name);
            hasher.add_str(&spec.donor_name);
            hasher.add(&spec.png_path); // path + mtime
        }
    }
    hasher.finish();

    // Skip the rebuild only if inputs match AND the output is still present.
    if hasher.matches() && std::path::Path::new(&merged_path).is_file() {
        log_info!(
            "atlas_cloner[{}]: inputs unchanged — skipping rebuild ({} set(s))",
            ifs_mod_path,
            batch.len()
        );
        return BatchResult::Cached;
    }

    // Rebuild: clone every set, aggregate the XML fragments, write once.
    let mut fragments = String::new();
    for set in batch {
        let specs: Vec<NewTextureSpec<'_>> = set
            .specs
            .iter()
            .map(|s| NewTextureSpec {
                new_name: &s.new_name,
                donor_name: &s.donor_name,
                png_path: &s.png_path,
            })
            .collect();
        let frag = if set.fresh {
            generate_cloned_atlases_xml_fresh(
                texlist_xml,
                ifs_mod_path,
                cache_root,
                &set.atlas_prefix,
                &specs,
            )
        } else {
            generate_cloned_atlases_xml(
                texlist_xml,
                ifs_mod_path,
                cache_root,
                &set.atlas_prefix,
                &specs,
            )
        };
        if let Some(frag) = frag {
            fragments.push_str(&frag);
        }
    }

    if fragments.is_empty() {
        return BatchResult::Nothing;
    }
    if !write_merged_texturelist(ifs_mod_path, mod_root, &fragments) {
        return BatchResult::Nothing;
    }

    // Commit the input hash so the next boot can skip.
    hasher.commit();
    ATLASES_REBUILT_THIS_BOOT.store(true, Ordering::Relaxed);
    BatchResult::Rebuilt
}

/// Shelf (row) packer for injected textures. Keeps a left-to-right cursor on
/// the current shelf and starts a new shelf below when the next texture won't
/// fit the remaining width — so each placement is amortized O(donor-rect
/// count) instead of the previous brute-force re-scan-from-origin
/// (`O(atlas_area · N)` per placement, `O(... · N²)` overall, which made a
/// 66-texture preview atlas take ~24s to pack).
///
/// `blockers` are the donor textures already present in the cloned atlas
/// (typically just the one donor slot) that the cursor must route around;
/// the shelf cursor itself guarantees no overlap among newly-packed rects, so
/// only the (small, fixed) blocker set is ever intersection-tested. Grows the
/// atlas height (then width) up to [`MAX_ATLAS_SIDE`] when a shelf won't fit.
struct ShelfPacker {
    atlas_w: u32,
    atlas_h: u32,
    /// Left edge of the next slot on the current shelf.
    cursor_x: u32,
    /// Top edge of the current shelf.
    shelf_y: u32,
    /// Height of the tallest rect placed on the current shelf.
    shelf_h: u32,
    blockers: Vec<PixelRect>,
    /// Whether the atlas may grow in WIDTH when it runs out of height.
    /// Donor-clone atlases (`true`) widen toward [`MAX_ATLAS_SIDE`] to fit
    /// extras in a single atlas. Fresh atlases (`false`) keep a fixed width
    /// and grow height only — when height tops out, `place` returns `None` so
    /// the caller spills the rest into a new atlas instead of ballooning into
    /// a huge, mostly-empty buffer.
    allow_widen: bool,
}

impl ShelfPacker {
    fn new(atlas_w: u32, atlas_h: u32, blockers: Vec<PixelRect>, allow_widen: bool) -> Self {
        Self {
            atlas_w,
            atlas_h,
            cursor_x: 0,
            shelf_y: 0,
            shelf_h: 0,
            blockers,
            allow_widen,
        }
    }

    /// Does a padded `rect` clear every blocker?
    fn clears_blockers(&self, rect: &PixelRect) -> bool {
        let probe = PixelRect {
            x_min: rect.x_min.saturating_sub(PACK_PADDING),
            y_min: rect.y_min.saturating_sub(PACK_PADDING),
            x_max: rect.x_max + PACK_PADDING,
            y_max: rect.y_max + PACK_PADDING,
        };
        !self.blockers.iter().any(|b| b.intersects(&probe))
    }

    /// Place a `w × h` rect, advancing the cursor. Returns the placed rect or
    /// `None` if it can't fit even after growing to [`MAX_ATLAS_SIDE`].
    fn place(&mut self, w: u32, h: u32) -> Option<PixelRect> {
        let step = PACK_PADDING.max(1);
        loop {
            // New shelf if this rect won't fit the remaining row width.
            if self.cursor_x + w + PACK_PADDING > self.atlas_w {
                self.shelf_y += self.shelf_h + step;
                self.cursor_x = 0;
                self.shelf_h = 0;
            }

            if self.shelf_y + h + PACK_PADDING <= self.atlas_h
                && self.cursor_x + w + PACK_PADDING <= self.atlas_w
            {
                let rect = PixelRect {
                    x_min: self.cursor_x,
                    y_min: self.shelf_y,
                    x_max: self.cursor_x + w,
                    y_max: self.shelf_y + h,
                };
                if self.clears_blockers(&rect) {
                    self.cursor_x += w + step;
                    self.shelf_h = self.shelf_h.max(h);
                    return Some(rect);
                }
                // Blocker in the way (rare — only near a donor slot): nudge
                // the cursor past it and retry on this shelf.
                self.cursor_x += step;
                continue;
            }

            // Shelf ran off the bottom — grow the atlas and retry. Donor
            // atlases grow height first then width (staying roughly square up
            // to the cap). Fresh atlases keep their fixed width and grow height
            // only; when height tops out, return `None` so the caller starts a
            // new atlas rather than widening into a huge near-empty buffer.
            if self.atlas_h < MAX_ATLAS_SIDE && (!self.allow_widen || self.atlas_h <= self.atlas_w)
            {
                self.atlas_h = (self.atlas_h * 2).min(MAX_ATLAS_SIDE);
            } else if self.allow_widen && self.atlas_w < MAX_ATLAS_SIDE {
                self.atlas_w = (self.atlas_w * 2).min(MAX_ATLAS_SIDE);
            } else {
                return None;
            }
        }
    }
}

/// Emit the `<image>...</image>` xml entry for a packed rect. Matches
/// the encoding used by stock texturelists: `imgrect` and `uvrect` are
/// stored as `(x_min*2, x_max*2, y_min*2, y_max*2)`, with the uvrect
/// inset from the imgrect by 2 encoding-units (i.e. 1 pixel) on each
/// side to leave a sampling margin.
fn emit_image_xml(new_name: &str, rect: &PixelRect) -> String {
    let (imin, imax, jmin, jmax) = (rect.x_min, rect.x_max, rect.y_min, rect.y_max);
    format!(
        "<image name=\"{}\"><imgrect __type=\"4u16\">{} {} {} {}</imgrect>\
         <uvrect __type=\"4u16\">{} {} {} {}</uvrect></image>",
        new_name,
        imin * 2,
        imax * 2,
        jmin * 2,
        jmax * 2,
        imin * 2 + 2,
        imax * 2 - 2,
        jmin * 2 + 2,
        jmax * 2 - 2,
    )
}

/// One image entry inside an atlas, with the pixel rect (derived from
/// imgrect) needed to composite into a cloned atlas.
#[derive(Clone)]
struct TexEntry {
    image_name: String,
    atlas_w: u32,
    atlas_h: u32,
    imgrect: String,
    uvrect: String,
    pixel_x: u32,
    pixel_y: u32,
    pixel_w: u32,
    pixel_h: u32,
}

/// Parse a plaintext texturelist.xml into (atlas_name, entries) pairs.
/// Entries within each atlas preserve document order.
fn parse_texturelist(xml: &str) -> Vec<(String, Vec<TexEntry>)> {
    let mut out: Vec<(String, Vec<TexEntry>)> = Vec::new();
    let mut search_pos = 0;
    while let Some(tex_start) = xml[search_pos..].find("<texture ") {
        let tex_start = search_pos + tex_start;
        let tex_end = match xml[tex_start..].find("</texture>") {
            Some(e) => tex_start + e + "</texture>".len(),
            None => break,
        };
        let tex_block = &xml[tex_start..tex_end];
        let atlas_name = extract_xml_attr(tex_block, "name").unwrap_or_default();
        let size_text = extract_xml_content(tex_block, "size").unwrap_or_default();
        let size_parts: Vec<u32> = size_text
            .split_whitespace()
            .filter_map(|s| s.parse().ok())
            .collect();
        let (atlas_w, atlas_h) = if size_parts.len() >= 2 {
            (size_parts[0], size_parts[1])
        } else {
            (0, 0)
        };

        let mut entries = Vec::new();
        let mut img_pos = 0;
        while let Some(img_start) = tex_block[img_pos..].find("<image ") {
            let img_start = img_pos + img_start;
            let img_end = match tex_block[img_start..].find("</image>") {
                Some(e) => img_start + e + "</image>".len(),
                None => break,
            };
            let img_block = &tex_block[img_start..img_end];
            let img_name = extract_xml_attr(img_block, "name").unwrap_or_default();
            let imgrect = extract_xml_content(img_block, "imgrect").unwrap_or_default();
            let uvrect = extract_xml_content(img_block, "uvrect").unwrap_or_default();
            let ir: Vec<u32> = imgrect
                .split_whitespace()
                .filter_map(|s| s.parse().ok())
                .collect();
            let (pixel_x, pixel_y, pixel_w, pixel_h) = if ir.len() >= 4 {
                // imgrect encodes (x_min*2, x_max*2, y_min*2, y_max*2)
                // in source pixels×2. Convert back to pixels.
                (
                    ir[0] / 2,
                    ir[2] / 2,
                    (ir[1] - ir[0]) / 2,
                    (ir[3] - ir[2]) / 2,
                )
            } else {
                (0, 0, 0, 0)
            };

            entries.push(TexEntry {
                image_name: img_name.to_string(),
                atlas_w,
                atlas_h,
                imgrect: imgrect.to_string(),
                uvrect: uvrect.to_string(),
                pixel_x,
                pixel_y,
                pixel_w,
                pixel_h,
            });
            img_pos = img_end;
        }
        out.push((atlas_name.to_string(), entries));
        search_pos = tex_end;
    }
    out
}

/// Extract and decode a stock IFS's `texturelist.xml` from a game ARC.
/// Returns the plaintext XML on success. Handles kbin-binary files
/// transparently via [`kbin::reader::decode_to_string`]. All error paths
/// log at WARN and return `None` so callers can graceful-degrade.
pub fn load_stock_texturelist(arc_path: &str, ifs_name: &str) -> Option<String> {
    let arc_data = match std::fs::read(arc_path) {
        Ok(d) => d,
        Err(e) => {
            log_warn!("atlas_cloner: can't read {}: {}", arc_path, e);
            return None;
        }
    };
    let entries = arc::parse(&arc_data).or_else(|| {
        log_warn!("atlas_cloner: failed to parse {}", arc_path);
        None
    })?;
    let ifs_entry = entries
        .iter()
        .find(|e| e.path.ends_with(ifs_name))
        .or_else(|| {
            log_warn!("atlas_cloner: {} not found inside {}", ifs_name, arc_path);
            None
        })?;
    let ifs_data = arc::extract(&arc_data, ifs_entry).or_else(|| {
        log_warn!(
            "atlas_cloner: failed to extract {} from {}",
            ifs_name,
            arc_path
        );
        None
    })?;
    let raw = ifs::extract_file_by_name(&ifs_data, "tex", "texturelist.xml").or_else(|| {
        log_warn!("atlas_cloner: no texturelist.xml in {}", ifs_name);
        None
    })?;

    if raw.first() == Some(&0xA0) {
        super::kbin::reader::decode_to_string(&raw)
            .map_err(|e| {
                log_warn!(
                    "atlas_cloner: kbin decode of {}/texturelist.xml failed: {}",
                    ifs_name,
                    e
                );
            })
            .ok()
    } else {
        Some(String::from_utf8_lossy(&raw).into_owned())
    }
}

fn extract_xml_attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let search = format!("{}=\"", name);
    let pos = tag.find(&search)?;
    let start = pos + search.len();
    let end = tag[start..].find('"')? + start;
    Some(&tag[start..end])
}

fn extract_xml_content<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{}", tag);
    let pos = xml.find(&open)?;
    let start = xml[pos..].find('>')? + pos + 1;
    let close = format!("</{}>", tag);
    let end = xml[start..].find(&close)? + start;
    Some(&xml[start..end])
}
