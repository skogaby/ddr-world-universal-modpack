//! IFS Texture Replacement — parse texturelist.xml, convert PNGs to game format, cache results.

use once_cell::sync::Lazy;
use std::collections::{BTreeMap, HashSet};
use std::sync::Mutex;

use crate::{log_info, log_warn};

use super::afplist_ext::extend_afplist_geo;
use super::cache_hasher::CACHE_FOLDER;
use super::mod_paths;
use super::xml_merger;

// ── Types ────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ImgFormat {
    Argb8888Rev,
    Dxt5,
    Unsupported,
}

#[derive(Clone, Copy, PartialEq)]
pub enum CompressType {
    None,
    Avslz,
    Unsupported,
}

#[derive(Clone)]
pub struct ImageInfo {
    pub name: String,
    pub name_md5: String,
    pub format: ImgFormat,
    pub compression: CompressType,
    pub ifs_mod_path: String,
    pub width: u32,
    pub height: u32,
}

impl ImageInfo {
    fn cache_folder(&self) -> String {
        format!("{}/{}", CACHE_FOLDER, self.ifs_mod_path)
    }
    fn cache_file(&self) -> String {
        format!("{}/{}", self.cache_folder(), self.name_md5)
    }
}

#[derive(Clone)]
pub struct AfpInfo {
    pub mod_path: String,
}

static TEXTURE_MAP: Lazy<Mutex<BTreeMap<String, ImageInfo>>> =
    Lazy::new(|| Mutex::new(BTreeMap::new()));
static AFP_MAP: Lazy<Mutex<BTreeMap<String, AfpInfo>>> = Lazy::new(|| Mutex::new(BTreeMap::new()));

/// Registered afplist `<geo>` id-list extensions, keyed by normalized IFS
/// path. Each entry appends shape ids to an EXISTING `<afp name=...>`
/// entry's geo list — the AFP runtime loads geos strictly from this list
/// at IFS mount (cabinet-observed: `afp-mip: can not find geo id` for any
/// unlisted geo, no on-demand fallback), and the append-only XML merger
/// can't edit an existing entry (a duplicate `<afp>` node risks
/// double-registering the whole template). Mods register at enable time
/// (before the IFS mounts); the afplist open then serves a rewritten copy.
static AFPLIST_EXTENSIONS: Lazy<Mutex<BTreeMap<String, Vec<(String, Vec<u16>)>>>> =
    Lazy::new(|| Mutex::new(BTreeMap::new()));

/// In-memory index of files present under `CACHE_FOLDER` (`./data_mods/_cache`),
/// stored as their full relative paths (e.g.
/// `./data_mods/_cache/select_music_option_lang_eng_v3_ifs/<md5>`).
///
/// Built once at init from a directory walk, then consulted instead of a
/// per-open `Path::exists()` syscall on the texture hot path. Scene-21 (CAUTION)
/// preloads the whole song-select asset set — thousands of texture opens — and a
/// filesystem stat per open made load time scale with OS file-cache warmth (the
/// observed 20–30s loads, nondeterministic across reboots). An in-memory set
/// lookup removes that cost. Kept live: [`cache_texture`] inserts the new path
/// after writing a cache file, so a cold-cache first run still resolves on the
/// next open without a stat.
static CACHE_INDEX: Lazy<Mutex<HashSet<String>>> = Lazy::new(|| Mutex::new(HashSet::new()));

/// Walk `CACHE_FOLDER` and populate [`CACHE_INDEX`]. Called once at LayeredFS
/// init. Recurses one level per cached IFS subfolder (the cache layout is
/// `_cache/<ifs_mod_path>/<file>`, with some nested arc/ifs subtrees), indexing
/// every regular file by its full relative path.
pub fn build_cache_index() {
    let mut index = CACHE_INDEX.lock().unwrap();
    index.clear();
    let mut count = 0usize;
    index_dir_recursive(std::path::Path::new(CACHE_FOLDER), &mut index, &mut count);
    log_info!(
        "LayeredFS: cache index built — {} file(s) under {}",
        count,
        CACHE_FOLDER
    );
}

fn index_dir_recursive(dir: &std::path::Path, index: &mut HashSet<String>, count: &mut usize) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return, // missing _cache dir (cold start) is fine — built lazily on write
    };
    for entry in entries.flatten() {
        let path = entry.path();
        match entry.file_type() {
            Ok(ft) if ft.is_dir() => index_dir_recursive(&path, index, count),
            Ok(ft) if ft.is_file() => {
                if let Some(s) = normalize_cache_path(&path) {
                    index.insert(s);
                    *count += 1;
                }
            }
            _ => {}
        }
    }
}

/// Normalize a cache path to the same `./data_mods/_cache/...` form that the
/// lookups (`cache_file()`, the merged-texturelist path) produce, so index hits
/// match. Uses forward slashes and the `CACHE_FOLDER` prefix.
fn normalize_cache_path(path: &std::path::Path) -> Option<String> {
    let s = path.to_str()?.replace('\\', "/");
    // read_dir yields paths prefixed with CACHE_FOLDER ("./data_mods/_cache");
    // keep that prefix so it matches ImageInfo::cache_file() output verbatim.
    Some(s)
}

/// True if `cache_path` is present in the in-memory cache index. Replaces a
/// per-open `Path::exists()` syscall.
pub fn cache_has(cache_path: &str) -> bool {
    CACHE_INDEX.lock().unwrap().contains(cache_path)
}

/// Record a newly-written cache file in the index so subsequent opens resolve it
/// without a filesystem stat (keeps the index correct across cold-cache writes).
pub fn cache_index_insert(cache_path: &str) {
    CACHE_INDEX.lock().unwrap().insert(cache_path.to_string());
}

// ── Public API ───────────────────────────────────────────────────────

/// Parse texturelist.xml and register MD5→texture mappings.
/// Called from file_hooks when path ends in "texturelist.xml".
pub fn parse_texturelist(norm_path: &str, path_to_open: &str) {
    // Derive IFS path: strip /tex/texturelist.xml
    let ifs_path = match norm_path
        .strip_suffix("/tex/texturelist.xml")
        .or_else(|| norm_path.strip_suffix("\\tex\\texturelist.xml"))
    {
        Some(p) => p.to_string(),
        None => return,
    };
    let ifs_mod_path = ifs_path.replace(".ifs", "_ifs");

    if mod_paths::find_first_modfolder(&ifs_mod_path).is_none() {
        log_info!(
            "LayeredFS: parse_texturelist: no mod folder for '{}'",
            ifs_mod_path
        );
        return;
    }

    log_info!(
        "LayeredFS: parse_texturelist: loading XML from '{}'",
        path_to_open
    );

    // Load and parse the XML
    let xml = match xml_merger::load_xml_from_avs_path(path_to_open) {
        Some(x) => x,
        None => {
            log_warn!(
                "LayeredFS: parse_texturelist: failed to load XML from '{}'",
                path_to_open
            );
            return;
        }
    };

    // Simple XML parsing — extract texture entries
    let mut compress = CompressType::None;

    // Check for compress attribute on texturelist node
    if let Some(pos) = xml.find("<texturelist") {
        let tag_end = xml[pos..].find('>').unwrap_or(0) + pos;
        let tag = &xml[pos..tag_end];
        if let Some(c) = extract_attr(tag, "compress") {
            compress = match c.to_lowercase().as_str() {
                "avslz" => CompressType::Avslz,
                _ => CompressType::Unsupported,
            };
        }
    }

    let mut map = TEXTURE_MAP.lock().unwrap();

    // Parse each <texture> and its <image> children
    let mut search_pos = 0;
    while let Some(tex_start) = xml[search_pos..].find("<texture") {
        let tex_start = search_pos + tex_start;
        let tex_end = match xml[tex_start..].find("</texture>") {
            Some(e) => tex_start + e + "</texture>".len(),
            None => break,
        };
        let tex_block = &xml[tex_start..tex_end];

        // Get format
        let format = extract_attr(tex_block, "format")
            .map(|f| match f.to_lowercase().as_str() {
                "argb8888rev" => ImgFormat::Argb8888Rev,
                "dxt5" => ImgFormat::Dxt5,
                _ => ImgFormat::Unsupported,
            })
            .unwrap_or(ImgFormat::Unsupported);

        // Parse each <image> in this texture
        let mut img_pos = 0;
        while let Some(img_start) = tex_block[img_pos..].find("<image") {
            let img_start = img_pos + img_start;
            let img_end = match tex_block[img_start..]
                .find("/>")
                .or_else(|| tex_block[img_start..].find("</image>"))
            {
                Some(e) => img_start + e,
                None => break,
            };
            let img_tag = &tex_block[img_start..img_end];

            if let Some(name) = extract_attr(img_tag, "name") {
                // Parse imgrect for dimensions
                let (width, height) = parse_imgrect(tex_block, img_start);

                let name_md5 = format!("{:x}", md5::compute(name.as_bytes()));
                let md5_path = format!("{}/tex/{}", ifs_path, name_md5);

                map.insert(
                    md5_path,
                    ImageInfo {
                        name: name.to_string(),
                        name_md5,
                        format,
                        compression: compress,
                        ifs_mod_path: ifs_mod_path.clone(),
                        width,
                        height,
                    },
                );
            }

            img_pos = img_end + 1;
        }

        search_pos = tex_end;
    }

    // New texture injection: find PNGs in mod folders that don't match existing textures
    // and pack them into atlas canvases (Task 7)
    let known_names: std::collections::HashSet<String> = map
        .values()
        .filter(|info| info.ifs_mod_path == ifs_mod_path)
        .map(|info| info.name.to_lowercase())
        .collect();

    let extra_pngs = list_extra_pngs(&ifs_mod_path, &known_names);
    if !extra_pngs.is_empty() {
        inject_new_textures(
            &extra_pngs,
            &ifs_path,
            &ifs_mod_path,
            compress,
            &mut map,
            &xml,
            norm_path,
        );
    }
}

/// Parse afplist.xml and register MD5→AFP/geo mappings.
pub fn parse_afplist(norm_path: &str, path_to_open: &str) {
    let ifs_path = match norm_path
        .strip_suffix("/afp/afplist.xml")
        .or_else(|| norm_path.strip_suffix("\\afp\\afplist.xml"))
    {
        Some(p) => p.to_string(),
        None => return,
    };
    let ifs_mod_path = ifs_path.replace(".ifs", "_ifs");

    if mod_paths::find_first_modfolder(&ifs_mod_path).is_none() {
        return;
    }

    let xml = match xml_merger::load_xml_from_avs_path(path_to_open) {
        Some(x) => x,
        None => return,
    };

    let mut map = AFP_MAP.lock().unwrap();
    let mut mapped = 0;

    // Parse each <afp> entry
    let mut search_pos = 0;
    while let Some(afp_start) = xml[search_pos..].find("<afp") {
        let afp_start = search_pos + afp_start;
        let afp_end = match xml[afp_start..].find("</afp>") {
            Some(e) => afp_start + e + "</afp>".len(),
            None => break,
        };
        let afp_block = &xml[afp_start..afp_end];

        if let Some(name) = extract_attr(afp_block, "name") {
            // Map AFP file
            let afp_md5 = format!("{:x}", md5::compute(name.as_bytes()));
            map.insert(
                format!("{}/afp/{}", ifs_path, afp_md5),
                AfpInfo {
                    mod_path: format!("{}/afp/{}", ifs_mod_path, name),
                },
            );
            // Map BSI file
            map.insert(
                format!("{}/afp/bsi/{}", ifs_path, afp_md5),
                AfpInfo {
                    mod_path: format!("{}/afp/bsi/{}", ifs_mod_path, name),
                },
            );
            mapped += 2;

            // Map geo shapes
            if let Some(geo_text) = extract_node_text(afp_block, "geo") {
                for index in geo_text.split_whitespace() {
                    let geo_name = format!("{}_shape{}", name, index);
                    let geo_md5 = format!("{:x}", md5::compute(geo_name.as_bytes()));
                    map.insert(
                        format!("{}/geo/{}", ifs_path, geo_md5),
                        AfpInfo {
                            mod_path: format!("{}/geo/{}", ifs_mod_path, geo_name),
                        },
                    );
                    mapped += 1;
                }
            }
        }

        search_pos = afp_end;
    }

    if mapped > 0 {
        log_info!(
            "LayeredFS: mapped {} AFP/geo filenames from {}",
            mapped,
            norm_path
        );
    }
}

/// Handle a texture file access — look up MD5 path, convert PNG if found.
/// Returns Some(cached_path) if a replacement was created.
pub fn handle_texture(norm_path: &str) -> Option<String> {
    let info = {
        let map = TEXTURE_MAP.lock().unwrap();
        map.get(norm_path)?.clone()
    };

    // Check if a pre-rendered atlas already exists in cache (from
    // inject_new_textures). Consult the in-memory cache index rather than a
    // filesystem stat — this runs for every texture open during the scene-21
    // asset preload, where a per-open `exists()` syscall dominated load time.
    let cache_file = info.cache_file();
    if cache_has(&cache_file) {
        return Some(cache_file);
    }

    // Find the PNG in mod folders
    let png_path =
        mod_paths::find_first_modfile(&format!("{}/{}.png", info.ifs_mod_path, info.name))
            .or_else(|| {
                mod_paths::find_first_modfile(&format!(
                    "{}/tex/{}.png",
                    info.ifs_mod_path, info.name
                ))
            })?;

    if info.format == ImgFormat::Unsupported || info.compression == CompressType::Unsupported {
        log_warn!("LayeredFS: unsupported format/compression for {}", png_path);
        return None;
    }

    if cache_texture(&png_path, &info) {
        Some(info.cache_file())
    } else {
        None
    }
}

/// Purge a STOCK-NAME texture replacement: delete the converted cache file
/// and drop its `CACHE_INDEX` entry so `handle_texture` stops short-circuiting
/// to a file that no longer matches the caller's intent. Callers that stage
/// stock-texture replacements gated on a mod's enable state (the
/// s_marvelous results sheets — the only stock-name replacements a MOD owns;
/// operator mod-folder replacements are unaffected) must call this whenever
/// they remove the staged PNG, otherwise a stale cache entry keeps serving
/// the old art (or, worse, an index entry pointing at a deleted file fails
/// the game's open outright).
pub fn purge_texture_replacement(ifs_mod_path: &str, image_name: &str) {
    let name_md5 = format!("{:x}", md5::compute(image_name.as_bytes()));
    let cache_file = format!("{}/{}/{}", CACHE_FOLDER, ifs_mod_path, name_md5);
    let existed = std::path::Path::new(&cache_file).exists();
    if existed {
        if let Err(e) = std::fs::remove_file(&cache_file) {
            log_warn!(
                "LayeredFS: can't purge cached texture {}: {}",
                cache_file,
                e
            );
            return;
        }
    }
    let dropped = CACHE_INDEX.lock().unwrap().remove(&cache_file);
    if existed || dropped {
        log_info!(
            "LayeredFS: purged texture replacement cache for {} ({})",
            image_name,
            cache_file
        );
    }
}

/// Register an afplist `<geo>` id-list extension: `extra_ids` are appended
/// to the EXISTING `<afp name="{afp_name}">` entry's geo list when the
/// IFS's afplist is opened. Idempotent per (ifs, afp, id). Callers must
/// register at enable time — before the target IFS mounts.
pub fn register_afplist_geo_extension(ifs_path: &str, afp_name: &str, extra_ids: &[u16]) {
    let mut map = AFPLIST_EXTENSIONS.lock().unwrap();
    let entries = map.entry(ifs_path.to_string()).or_default();
    match entries.iter_mut().find(|(name, _)| name == afp_name) {
        Some((_, ids)) => {
            for id in extra_ids {
                if !ids.contains(id) {
                    ids.push(*id);
                }
            }
        }
        None => entries.push((afp_name.to_string(), extra_ids.to_vec())),
    }
}

/// Cheap pre-check: whether any geo extension is registered for this
/// afplist path (avoids loading/decoding afplists that need no rewrite).
pub fn has_afplist_extensions(norm_path: &str) -> bool {
    let Some(ifs_path) = norm_path
        .strip_suffix("/afp/afplist.xml")
        .or_else(|| norm_path.strip_suffix("\\afp\\afplist.xml"))
    else {
        return false;
    };
    AFPLIST_EXTENSIONS.lock().unwrap().contains_key(ifs_path)
}

/// Serve a rewritten afplist for an IFS with registered geo extensions.
/// `xml` is the (already merged, if applicable) afplist text. Applies every
/// registered extension, writes the result to the cache folder, and returns
/// the cache path to serve. Returns `None` when no extension is registered
/// or every rewrite failed (caller serves the unmodified file — fail-open).
pub fn rewrite_afplist_if_extended(norm_path: &str, xml: &str) -> Option<(String, String)> {
    let ifs_path = norm_path
        .strip_suffix("/afp/afplist.xml")
        .or_else(|| norm_path.strip_suffix("\\afp\\afplist.xml"))?;
    let entries = {
        let map = AFPLIST_EXTENSIONS.lock().unwrap();
        map.get(ifs_path)?.clone()
    };
    let mut out = xml.to_string();
    let mut applied = 0usize;
    for (afp_name, ids) in &entries {
        match extend_afplist_geo(&out, afp_name, ids) {
            Some(rewritten) => {
                out = rewritten;
                applied += 1;
            }
            None => {
                log_warn!(
                    "LayeredFS: afplist geo extension failed for '{}' in {} — entry not found",
                    afp_name,
                    norm_path
                );
            }
        }
    }
    if applied == 0 {
        return None;
    }
    let ifs_mod_path = ifs_path.replace(".ifs", "_ifs");
    let outfolder = format!("{}/{}/afp", CACHE_FOLDER, ifs_mod_path);
    mod_paths::mkdir_p(&outfolder);
    let outfile = format!("{}/afplist.xml", outfolder);
    if let Err(e) = std::fs::write(&outfile, &out) {
        log_warn!(
            "LayeredFS: can't write rewritten afplist {}: {}",
            outfile,
            e
        );
        return None;
    }
    log_info!(
        "LayeredFS: extended {} afplist geo list(s) in {}",
        applied,
        norm_path
    );
    Some((outfile, out))
}

/// Register a custom AFP/geo MD5 mapping so handle_afp can serve mod files.
/// `ifs_path` is the normalized IFS path (e.g. "arc/bm2d/foo.ifs").
/// `geo_name` is the human-readable name (e.g. "folder_dogs_shape41").
/// The mod file is expected at `{ifs_mod_path}/geo/{geo_name}` in data_mods/.
pub fn register_afp_geo_mapping(ifs_path: &str, geo_name: &str) {
    let ifs_mod_path = ifs_path.replace(".ifs", "_ifs");
    let geo_md5 = format!("{:x}", md5::compute(geo_name.as_bytes()));
    let mut map = AFP_MAP.lock().unwrap();
    map.insert(
        format!("{}/geo/{}", ifs_path, geo_md5),
        AfpInfo {
            mod_path: format!("{}/geo/{}", ifs_mod_path, geo_name),
        },
    );
}

/// Register a custom AFP file MD5 mapping (the AFP data itself, not geo).
pub fn register_afp_mapping(ifs_path: &str, afp_name: &str) {
    let ifs_mod_path = ifs_path.replace(".ifs", "_ifs");
    let afp_md5 = format!("{:x}", md5::compute(afp_name.as_bytes()));
    let mut map = AFP_MAP.lock().unwrap();
    map.insert(
        format!("{}/afp/{}", ifs_path, afp_md5),
        AfpInfo {
            mod_path: format!("{}/afp/{}", ifs_mod_path, afp_name),
        },
    );
}

/// Register a custom BSI file MD5 mapping.
pub fn register_afp_bsi_mapping(ifs_path: &str, afp_name: &str) {
    let ifs_mod_path = ifs_path.replace(".ifs", "_ifs");
    let bsi_md5 = format!("{:x}", md5::compute(afp_name.as_bytes()));
    let mut map = AFP_MAP.lock().unwrap();
    map.insert(
        format!("{}/afp/bsi/{}", ifs_path, bsi_md5),
        AfpInfo {
            mod_path: format!("{}/afp/bsi/{}", ifs_mod_path, afp_name),
        },
    );
}

/// Handle an AFP/geo file access — look up MD5 path, return mod file if found.
pub fn handle_afp(norm_path: &str) -> Option<String> {
    let info = {
        let map = AFP_MAP.lock().unwrap();
        map.get(norm_path)?.clone()
    };
    mod_paths::find_first_modfile(&info.mod_path)
}

// ── Texture conversion ───────────────────────────────────────────────

fn cache_texture(png_path: &str, tex: &ImageInfo) -> bool {
    let cache_folder = tex.cache_folder();
    mod_paths::mkdir_p(&cache_folder);
    let cache_file = tex.cache_file();

    // Check if cache is fresh (simple timestamp comparison)
    if let (Ok(cache_meta), Ok(png_meta)) =
        (std::fs::metadata(&cache_file), std::fs::metadata(png_path))
    {
        if let (Ok(ct), Ok(pt)) = (cache_meta.modified(), png_meta.modified()) {
            if ct >= pt {
                // Cache is up to date on disk but the in-memory index missed it
                // (we only reach here after cache_has returned false). Record it
                // so the next open resolves via the index instead of re-running
                // this metadata revalidation every time.
                cache_index_insert(&cache_file);
                return true; // cache is up to date
            }
        }
    }

    // Load PNG and convert to RGBA
    let img = match image::open(png_path) {
        Ok(i) => i.into_rgba8(),
        Err(e) => {
            log_warn!("LayeredFS: can't load PNG {}: {}", png_path, e);
            return false;
        }
    };

    let width = img.width();
    let height = img.height();

    // If PNG is smaller than atlas (power-of-2 padding), expand with transparent pixels
    let img = if width <= tex.width
        && height <= tex.height
        && (width != tex.width || height != tex.height)
    {
        let mut padded = image::RgbaImage::new(tex.width, tex.height);
        image::imageops::overlay(&mut padded, &img, 0, 0);
        padded
    } else if width != tex.width || height != tex.height {
        log_warn!(
            "LayeredFS: PNG {}x{} doesn't match texturelist {}x{}, skipping {}",
            width,
            height,
            tex.width,
            tex.height,
            png_path
        );
        return false;
    } else {
        img
    };

    let rgba = img.as_raw();

    // Convert to game format
    let image_data = match tex.format {
        ImgFormat::Argb8888Rev => {
            // RGBA → BGRA (swap R and B)
            let mut bgra = rgba.to_vec();
            for pixel in bgra.chunks_exact_mut(4) {
                pixel.swap(0, 2);
            }
            bgra
        }
        ImgFormat::Dxt5 => {
            let dxt5_size = width as usize * height as usize; // DXT5 = 1 byte per pixel
            let mut dxt5 = vec![0u8; dxt5_size];
            texpresso::Format::Bc3.compress(
                rgba,
                width as usize,
                height as usize,
                texpresso::Params {
                    algorithm: texpresso::Algorithm::IterativeClusterFit,
                    ..Default::default()
                },
                &mut dxt5,
            );
            // Word-swap endianness
            for pair in dxt5.chunks_exact_mut(2) {
                pair.swap(0, 1);
            }
            dxt5
        }
        _ => return false,
    };

    let uncompressed_size = image_data.len();

    // Optionally compress with AVSLZ
    let final_data = if tex.compression == CompressType::Avslz {
        match avslz_compress(&image_data) {
            Some(compressed) => {
                // Per-texture line — fires once per injected PNG (hundreds
                // during a heavy load), so gate it behind the LayeredFS verbose
                // flag to keep normal boots quiet and avoid the log overhead.
                if super::config().verbose {
                    log_info!(
                        "LayeredFS: AVSLZ {} -> {} bytes, uncomp_hdr={}, fmt={:?} {}x{} for {}",
                        uncompressed_size,
                        compressed.len(),
                        uncompressed_size,
                        tex.format,
                        tex.width,
                        tex.height,
                        png_path
                    );
                }
                let mut out = Vec::with_capacity(8 + compressed.len());
                out.extend_from_slice(&(uncompressed_size as u32).to_be_bytes());
                out.extend_from_slice(&(compressed.len() as u32).to_be_bytes());
                out.extend_from_slice(&compressed);
                out
            }
            None => {
                log_warn!("LayeredFS: AVSLZ compression failed for {}", png_path);
                return false;
            }
        }
    } else {
        image_data
    };

    if std::fs::write(&cache_file, &final_data).is_ok() {
        // Keep the in-memory cache index live: a cold-cache run writes the file
        // here, and the next open of this texture must resolve via the index
        // (cache_has) without falling back to a filesystem stat.
        cache_index_insert(&cache_file);
        true
    } else {
        false
    }
}

/// Compress data using native AVSLZ implementation.
fn avslz_compress(input: &[u8]) -> Option<Vec<u8>> {
    Some(super::avslz::compress(input))
}

// ── New texture injection ─────────────────────────────────────────────

/// Find PNGs in mod folders that don't correspond to existing textures.
fn list_extra_pngs(
    ifs_mod_path: &str,
    known_names: &std::collections::HashSet<String>,
) -> Vec<String> {
    let mut extras = Vec::new();
    for dir in mod_paths::available_mods() {
        for suffix in &["", "/tex"] {
            let search_dir = format!("{}/{}{}", dir, ifs_mod_path, suffix);
            if let Ok(entries) = std::fs::read_dir(&search_dir) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name.to_lowercase().ends_with(".png") {
                        let stem = &name[..name.len() - 4];
                        if !known_names.contains(&stem.to_lowercase())
                            && !extras.iter().any(|e: &String| e.eq_ignore_ascii_case(stem))
                        {
                            extras.push(stem.to_string());
                        }
                    }
                }
            }
        }
    }
    extras.sort();
    extras
}

/// Pack extra PNGs into atlas canvases and inject into the texturelist XML.
fn inject_new_textures(
    extra_pngs: &[String],
    ifs_path: &str,
    ifs_mod_path: &str,
    compress: CompressType,
    map: &mut BTreeMap<String, ImageInfo>,
    original_xml: &str,
    norm_path: &str,
) {
    // Each image gets its own 1:1 atlas (atlas size = image size).
    // This ensures UV normalization maps correctly regardless of the
    // original atlas layout the geo files were designed for.
    let mut xml_additions = String::new();
    let mut rendered = 0;

    let outfolder = format!("{}/{}", CACHE_FOLDER, ifs_mod_path);
    mod_paths::mkdir_p(&outfolder);

    for (i, name) in extra_pngs.iter().enumerate() {
        let png_path = mod_paths::find_first_modfile(&format!("{}/{}.png", ifs_mod_path, name))
            .or_else(|| {
                mod_paths::find_first_modfile(&format!("{}/tex/{}.png", ifs_mod_path, name))
            });
        let png_path = match png_path {
            Some(p) => p,
            None => continue,
        };

        let img = match image::open(&png_path) {
            Ok(i) => i.into_rgba8(),
            Err(e) => {
                log_warn!("LayeredFS: can't load PNG {}: {}", png_path, e);
                continue;
            }
        };

        let w = img.width();
        let h = img.height();
        let atlas_name = format!("ctex{:03}", i);

        // Generate texturelist entry — one atlas per image, full coverage
        xml_additions.push_str(&format!(
            "<texture format=\"argb8888rev\" mag_filter=\"linear\" min_filter=\"linear\" \
             name=\"{}\" wrap_s=\"clamp\" wrap_t=\"clamp\">\
             <size __type=\"2u16\">{} {}</size>\
             <image name=\"{}\"><imgrect __type=\"4u16\">0 {} 0 {}</imgrect>\
             <uvrect __type=\"4u16\">0 {} 0 {}</uvrect></image></texture>",
            atlas_name,
            w,
            h,
            name,
            w * 2,
            h * 2,
            w * 2,
            h * 2
        ));

        // Convert RGBA → BGRA (argb8888rev)
        let mut bgra = img.into_raw();
        for pixel in bgra.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }

        // Compress with AVSLZ if the IFS uses it
        let final_data = if compress == CompressType::Avslz {
            let compressed = super::avslz::compress(&bgra);
            let mut out = Vec::with_capacity(8 + compressed.len());
            out.extend_from_slice(&(bgra.len() as u32).to_be_bytes());
            out.extend_from_slice(&(compressed.len() as u32).to_be_bytes());
            out.extend_from_slice(&compressed);
            out
        } else {
            bgra
        };

        // Write converted data to cache and register in texture map
        let atlas_md5 = format!("{:x}", md5::compute(atlas_name.as_bytes()));
        let cache_file = format!("{}/{}", outfolder, atlas_md5);
        if std::fs::write(&cache_file, &final_data).is_ok() {
            // Keep the cache index live so handle_texture resolves this injected
            // texture via cache_has (no per-open stat) on its next open.
            cache_index_insert(&cache_file);
        }

        let md5_path = format!("{}/tex/{}", ifs_path, atlas_md5);
        map.insert(
            md5_path,
            ImageInfo {
                name: atlas_name.clone(),
                name_md5: atlas_md5,
                format: ImgFormat::Argb8888Rev,
                compression: compress,
                ifs_mod_path: ifs_mod_path.to_string(),
                width: w,
                height: h,
            },
        );

        rendered += 1;
    }

    if rendered == 0 {
        return;
    }

    // Write modified texturelist.xml to cache
    let outfile = format!("{}/texturelist.xml", outfolder);

    let modified = if let Some(pos) = original_xml.rfind("</texturelist>") {
        format!(
            "{}{}\n{}",
            &original_xml[..pos],
            xml_additions,
            &original_xml[pos..]
        )
    } else {
        original_xml.to_string()
    };

    let _ = std::fs::write(&outfile, &modified);
    log_info!(
        "LayeredFS: injected {} new textures into {}",
        rendered,
        norm_path
    );
}

// ── XML helpers ──────────────────────────────────────────────────────

fn extract_attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let search = format!("{}=\"", name);
    let pos = tag.find(&search)?;
    let start = pos + search.len();
    let end = tag[start..].find('"')? + start;
    Some(&tag[start..end])
}

fn extract_node_text<'a>(xml: &'a str, node_name: &str) -> Option<&'a str> {
    let open = format!("<{}", node_name);
    let pos = xml.find(&open)?;
    let content_start = xml[pos..].find('>')? + pos + 1;
    let close = format!("</{}>", node_name);
    let content_end = xml[content_start..].find(&close)? + content_start;
    Some(&xml[content_start..content_end])
}

fn parse_imgrect(tex_block: &str, img_start: usize) -> (u32, u32) {
    // Find the imgrect node after this image tag
    if let Some(rect_text) = extract_node_text(&tex_block[img_start..], "imgrect") {
        let nums: Vec<u32> = rect_text
            .split_whitespace()
            .filter_map(|s| s.parse().ok())
            .collect();
        if nums.len() >= 4 {
            return ((nums[1] - nums[0]) / 2, (nums[3] - nums[2]) / 2);
        }
    }
    (0, 0)
}
