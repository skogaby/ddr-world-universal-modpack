//! Runtime shader-container synthesis — builds the extended
//! `gs_screencommand_*.gsp` GSPW containers from the game's OWN stock
//! shader.arc blobs plus the committed mod blobs at
//! `data_mods/shader_fixes/blobs/*.d3dbc`.
//!
//! Why synthesis instead of committed `.gsp` files: program 0 of every
//! touched container uses the game's own stock bytecode (no Konami bytecode
//! in the repo, automatic tracking of stock shader updates), and the
//! ANTI-ALIASING toggle becomes a synthesis parameter instead of a committed
//! variant explosion.
//!
//! ## Container set (the minimal-overlay rule)
//!
//! | container | program 0 | program 1 | theme programs (LAST) | overlaid when |
//! |---|---|---|---|---|
//! | arrow   | stock VS + (AA ? AA PS : stock PS) | persp VS + AA PS | — | AA or persp |
//! | judge   | stock VS + (AA ? AA judge PS : stock judge PS) | arrow persp VS + same PS | — | AA or persp |
//! | default | stock VS + stock PS (bit-identical)| persp VS + stock PS | theme VS + per-theme PS | persp or themes |
//!
//! The program-table layout (persp EXACTLY at index 1; the mod-menu theme
//! programs appended last in every configuration) is the pure, host-tested
//! `shader_layout` module — this file assembles blobs to match. Theme
//! program indices are published to `overlay_draw` on every success path
//! (fresh build AND cache hit); unset ⇒ the menu's static degrade.
//!
//! The judge container's perspective program reuses the ARROW persp VS
//! blob: the stock judge VS is byte-identical to the stock arrow VS and
//! the judge PS reads the same v0/v1 contract (bytecode decode,
//! `.agents/planning/20260719-shader-injection/research/judge-and-toolchain.md`).
//! It exists for `screen::JudgeEffectRenderer` (tap hit-burst + freeze-hold
//! glow at the receptor row), whose pass player_perspective rewrites.
//!
//! With AA, perspective, themes all off (or the `shader-fixes` mod disabled
//! in the `mods` map, or the `shader_fixes` mod folder blocklisted), nothing
//! is overlaid at all — the game runs literal stock bytecode. A missing
//! THEME blob degrades only the theme programs (one WARN), never AA/persp.
//!
//! ## Where it runs
//!
//! Lazily inside `arc_handler::handle_arc` when the game opens
//! `data/arc/shader.arc`. That open happens ONCE per session, inside
//! `Application::onBoot` within a few hundred ms of gamemdx loading — so
//! the LayeredFS hooks must already be installed: `lib.rs` initializes
//! LayeredFS at step 0b, BEFORE the gamemdx wait and the signature scan
//! (a Win7 cabinet lost this race on 2026-09-03 when LayeredFS still
//! installed after the scan; `overlay_draw` now WARNs when it sees a live
//! default container while [`status`] is still `NotSeen`). Synthesized
//! containers are written to
//! `data_mods/_cache/shader_synthesis/*.gsp` behind a fingerprint sidecar
//! ({stock arc, blob files, AA, persp}); warm boots reuse the files, and the
//! outer arc cache hashes them like any other overlay input.
//!
//! ## GSPW packing
//!
//! Byte-compatible with `scripts/gsp_pack.py pack` (the offline dev tool):
//! header + program/VS/PS tables at computed offsets, program entries
//! `{flags@+0, vs_idx@+4, ps_idx@+5}`, blobs 16-aligned in table order. The
//! FNV-1 name hash is copied from the stock container (never recomputed).
//! Validate a synthesized file offline with
//! `python3 scripts/gsp_pack.py inspect <file> --expect-name <name>`.

use std::path::Path;
use std::sync::atomic::{AtomicU8, Ordering};

use crate::core::arc;
use crate::{log_info, log_warn};

use super::cache_hasher::CACHE_FOLDER;
use super::mod_paths;
use super::shader_layout;

/// Outcome of the session's single `shader.arc` open, as seen by this
/// module. Read by the shader-fixes mod's enable line (honest status
/// instead of an assertion) and by the overlay-draw boot-order race
/// detector: a live default shader container while this is still
/// `NotSeen` means the game read shader.arc BEFORE the LayeredFS hooks
/// were installed (the 2026-09-03 Win7 report).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SynthStatus {
    /// `handle_arc("arc/shader.arc")` has not run (yet).
    NotSeen,
    /// The open was intercepted but nothing was overlaid (mod disabled,
    /// features off, blobs missing, or a build error — the log says which).
    Stock,
    /// Synthesized containers were served.
    Synthesized,
}

static STATUS: AtomicU8 = AtomicU8::new(0);

fn set_status(s: SynthStatus) {
    let v = match s {
        SynthStatus::NotSeen => 0,
        SynthStatus::Stock => 1,
        SynthStatus::Synthesized => 2,
    };
    STATUS.store(v, Ordering::Release);
}

/// Current synthesis outcome (see [`SynthStatus`]).
pub fn status() -> SynthStatus {
    match STATUS.load(Ordering::Acquire) {
        1 => SynthStatus::Stock,
        2 => SynthStatus::Synthesized,
        _ => SynthStatus::NotSeen,
    }
}

/// Cache directory for synthesized containers.
fn synth_cache_dir() -> String {
    format!("{}/shader_synthesis", CACHE_FOLDER)
}

/// The three container names we may synthesize.
const ARROW: &str = "gs_screencommand_arrow";
const JUDGE: &str = "gs_screencommand_judge";
const DEFAULT: &str = "gs_screencommand_default";

/// Committed mod blob file names (inside `<mod>/blobs/`).
const BLOB_ARROW_AA_PS: &str = "gs_screencommand_arrow.ps.d3dbc";
const BLOB_ARROW_PERSP_VS: &str = "gs_screencommand_arrow.vs_persp.d3dbc";
const BLOB_JUDGE_AA_PS: &str = "gs_screencommand_judge.ps.d3dbc";
const BLOB_DEFAULT_PERSP_VS: &str = "gs_screencommand_default.vs_persp.d3dbc";
/// Mod-menu animated-background blobs (overlay-menu rewrite Step 8;
/// Shadertoy theme pack 2026-08-25): one shared passthrough VS + one PS
/// per shader-backed theme, appended to the DEFAULT container per
/// `shader_layout` (theme programs LAST, always). ORDER MATTERS: the PS
/// entries after the VS must match `ThemeProgram::slot()` order
/// (`mod_menu::theme`) — slot 0 is the SECOND array entry.
const BLOB_THEME_VS: &str = "theme_passthrough.vs.d3dbc";
const THEME_BLOBS: [&str; 1 + shader_layout::THEME_PROGRAM_COUNT as usize] = [
    BLOB_THEME_VS,
    "theme_bubbles.ps.d3dbc",
    "theme_terminal.ps.d3dbc",
    "theme_waveform.ps.d3dbc",
    "theme_spectrum.ps.d3dbc",
    "theme_tunnel.ps.d3dbc",
    "theme_xmb.ps.d3dbc",
    "theme_squares.ps.d3dbc",
    "theme_card_swirl.ps.d3dbc",
    "theme_blobs.ps.d3dbc",
    "theme_ps2.ps.d3dbc",
    "theme_prime_cube.ps.d3dbc",
];

/// A synthesized (or planned) overlay entry: arc entry name → cache file.
pub(super) struct SynthEntry {
    /// Arc-relative entry name, e.g. `data/shader/gs_screencommand_arrow.gsp`.
    pub entry_name: String,
    /// Absolute-ish filesystem path of the synthesized container.
    pub file_path: String,
}

/// Synthesis inputs resolved from config + mod folders.
struct Plan {
    aa: bool,
    persp: bool,
    /// Mod-menu theme background programs ride the DEFAULT container.
    themes: bool,
    /// Blob name → found mod file path.
    blob_paths: Vec<(&'static str, String)>,
}

/// Read the synthesis plan from config + the mod-folder scan. Returns `None`
/// when nothing should be synthesized (mod disabled, every feature off, or
/// required blobs unavailable e.g. blocklisted) — the game then runs stock
/// shaders. Theme blobs degrade softly: a missing theme blob drops ONLY the
/// theme programs (one WARN), never the AA/perspective synthesis.
fn plan() -> Option<Plan> {
    let cfg = crate::mods::config::get()?;
    let mod_enabled = |id: &str| cfg.mods.get(id).copied().unwrap_or(true);

    if !mod_enabled("shader-fixes") {
        log_info!("shader_synthesis: shader-fixes mod disabled — stock shaders");
        return None;
    }
    let aa = cfg
        .shader_fixes
        .as_ref()
        .map(|s| s.anti_aliasing)
        .unwrap_or(true);
    let persp = mod_enabled("player-perspective");
    let mut themes = mod_enabled("mod-menu");
    if !aa && !persp && !themes {
        log_info!("shader_synthesis: AA off + perspective off + menu off — stock shaders");
        return None;
    }

    // Which blobs does this configuration need?
    let mut needed: Vec<&'static str> = Vec::new();
    if aa || persp {
        // The arrow container always carries the AA PS when overlaid at all
        // (perspective programs bake it in even with AA off).
        needed.push(BLOB_ARROW_AA_PS);
    }
    if aa {
        needed.push(BLOB_JUDGE_AA_PS);
    }
    if persp {
        needed.push(BLOB_ARROW_PERSP_VS);
        needed.push(BLOB_DEFAULT_PERSP_VS);
    }

    let mut blob_paths = Vec::new();
    for name in needed {
        match mod_paths::find_first_modfile(&format!("blobs/{}", name)) {
            Some(p) => blob_paths.push((name, p)),
            None => {
                log_warn!(
                    "shader_synthesis: blob '{}' not found (shader_fixes missing/blocklisted?) — stock shaders",
                    name
                );
                return None;
            }
        }
    }

    // Theme blobs: soft-degrade resolution (themes off, plan survives).
    if themes {
        let mut theme_paths = Vec::new();
        for name in THEME_BLOBS {
            match mod_paths::find_first_modfile(&format!("blobs/{}", name)) {
                Some(p) => theme_paths.push((name, p)),
                None => {
                    log_warn!(
                        "shader_synthesis: theme blob '{}' not found — menu backgrounds degrade to static",
                        name
                    );
                    themes = false;
                    break;
                }
            }
        }
        if themes {
            blob_paths.extend(theme_paths);
        }
    }

    if !aa && !persp && !themes {
        log_info!("shader_synthesis: nothing left to overlay — stock shaders");
        return None;
    }
    Some(Plan {
        aa,
        persp,
        themes,
        blob_paths,
    })
}

/// Entry point, called from `arc_handler::handle_arc` for
/// `data/arc/shader.arc`. Returns the synthesized overlay entries (empty ⇒
/// serve stock). `original_path` is the AVS path of the stock arc.
pub(super) fn synthesize(original_path: &str) -> Vec<SynthEntry> {
    let entries = synthesize_inner(original_path);
    set_status(if entries.is_empty() {
        SynthStatus::Stock
    } else {
        SynthStatus::Synthesized
    });
    entries
}

fn synthesize_inner(original_path: &str) -> Vec<SynthEntry> {
    let plan = match plan() {
        Some(p) => p,
        None => return Vec::new(),
    };

    let dir = synth_cache_dir();
    if !mod_paths::mkdir_p(&dir) {
        log_warn!("shader_synthesis: couldn't create cache dir '{}'", dir);
        return Vec::new();
    }

    // Fingerprint: config bits + blob contents + the stock arc bytes. The
    // stock arc participates so a game update to the stock shaders
    // regenerates our containers automatically.
    let stock_bytes = match super::xml_merger::load_bytes_from_avs_path(original_path) {
        Some(b) => b,
        None => {
            log_warn!(
                "shader_synthesis: couldn't read stock arc '{}' — stock shaders",
                original_path
            );
            return Vec::new();
        }
    };
    // "v4": the theme program set changed (Shadertoy theme pack — arrows/
    // wavefield retired, bubbles + 5 new ports; 3 -> 6 theme programs) —
    // the version prefix is what invalidates pre-existing caches when the
    // packing recipe changes for identical config+blob inputs.
    let mut fp = format!(
        "v4 aa={} persp={} themes={} arc={}",
        plan.aa,
        plan.persp,
        plan.themes,
        {
            let mut h = 0xcbf2_9ce4_8422_2325u64; // FNV-1a 64 over the arc bytes
            for &b in &stock_bytes {
                h ^= b as u64;
                h = h.wrapping_mul(0x1000_0000_01b3);
            }
            h
        }
    );
    for (name, path) in &plan.blob_paths {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        if let Ok(bytes) = std::fs::read(path) {
            for &b in &bytes {
                h ^= b as u64;
                h = h.wrapping_mul(0x1000_0000_01b3);
            }
        }
        fp.push_str(&format!(" {}={:016x}", name, h));
    }

    let names = planned_names(&plan);
    let sidecar = format!("{}/fingerprint.txt", dir);
    let cached_ok = std::fs::read_to_string(&sidecar)
        .map(|s| s == fp)
        .unwrap_or(false)
        && names
            .iter()
            .all(|n| Path::new(&format!("{}/{}.gsp", dir, n)).is_file());
    if cached_ok {
        log_info!(
            "shader_synthesis: cache up to date ({} containers)",
            names.len()
        );
        publish_theme_indices(&plan);
        return entries_for(&names, &dir);
    }

    log_info!(
        "shader_synthesis: synthesizing (aa={}, persp={}, themes={})",
        plan.aa,
        plan.persp,
        plan.themes
    );
    match build_all(&plan, &stock_bytes, &dir) {
        Ok(built) => {
            let _ = std::fs::write(&sidecar, fp);
            publish_theme_indices(&plan);
            entries_for(&built, &dir)
        }
        Err(e) => {
            log_warn!("shader_synthesis: {} — stock shaders", e);
            // Poison the sidecar so the next boot retries.
            let _ = std::fs::remove_file(&sidecar);
            Vec::new()
        }
    }
}

/// Publish the theme programs' DEFAULT-container indices to the
/// overlay-draw emitter — the layout is a pure function of the plan, so
/// this runs identically on the fresh-build and cache-hit success paths.
/// Never called when themes were degraded/disabled (the export stays
/// unset ⇒ the menu's static degrade).
fn publish_theme_indices(plan: &Plan) {
    if let Some(idx) = shader_layout::default_theme_indices(plan.persp, plan.themes) {
        crate::services::overlay_draw::publish_theme_programs(idx.map(|i| i as u32));
    }
}

fn planned_names(plan: &Plan) -> Vec<&'static str> {
    let planned = shader_layout::planned(plan.aa, plan.persp, plan.themes);
    let mut v = Vec::new();
    if planned.arrow {
        v.push(ARROW);
    }
    if planned.judge {
        v.push(JUDGE);
    }
    if planned.default {
        v.push(DEFAULT);
    }
    v
}

fn entries_for(names: &[&'static str], dir: &str) -> Vec<SynthEntry> {
    names
        .iter()
        .map(|n| SynthEntry {
            entry_name: format!("data/shader/{}.gsp", n),
            file_path: format!("{}/{}.gsp", dir, n),
        })
        .collect()
}

// ── Container building ──────────────────────────────────────────────

fn build_all(plan: &Plan, stock_arc: &[u8], dir: &str) -> Result<Vec<&'static str>, String> {
    let read_blob = |name: &str| -> Result<Vec<u8>, String> {
        let path = plan
            .blob_paths
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, p)| p.clone())
            .ok_or_else(|| format!("blob '{}' not in plan", name))?;
        let bytes = std::fs::read(&path).map_err(|e| format!("read '{}': {}", path, e))?;
        validate_blob(&bytes, name)?;
        Ok(bytes)
    };

    let mut built = Vec::new();

    // Arrow: prog0 = stock VS + (AA ? AA PS : stock PS);
    //        prog1 (persp) = persp VS + AA PS.
    if plan.aa || plan.persp {
        let stock = extract_stock(stock_arc, ARROW)?;
        let aa_ps = read_blob(BLOB_ARROW_AA_PS)?;
        let persp_vs = if plan.persp {
            Some(read_blob(BLOB_ARROW_PERSP_VS)?)
        } else {
            None
        };
        let mut vs_blobs: Vec<&[u8]> = vec![&stock.vs];
        // Program 0's PS is the AA PS when AA is on, else the stock PS; the
        // AA PS is always LAST in the table (the perspective program's PS).
        let ps_blobs: Vec<&[u8]> = if plan.aa {
            vec![&aa_ps]
        } else {
            vec![&stock.ps, &aa_ps]
        };
        let mut programs: Vec<(u8, u8, u8)> = vec![(0, 0, 0)];
        if let Some(ref pv) = persp_vs {
            vs_blobs.push(pv);
            programs.push((0, 1, (ps_blobs.len() - 1) as u8));
        }
        write_container(dir, ARROW, stock.name_hash, &vs_blobs, &ps_blobs, &programs)?;
        built.push(ARROW);
    }

    // Judge: prog0 = stock VS + (AA ? AA judge PS : stock judge PS);
    //        prog1 (persp) = ARROW persp VS + the same PS as prog0
    //        (stock judge VS ≡ stock arrow VS byte-identically, and the
    //        judge PS reads the same v0/v1 contract — see module doc).
    if plan.aa || plan.persp {
        let stock = extract_stock(stock_arc, JUDGE)?;
        let aa_ps = if plan.aa {
            Some(read_blob(BLOB_JUDGE_AA_PS)?)
        } else {
            None
        };
        let persp_vs = if plan.persp {
            Some(read_blob(BLOB_ARROW_PERSP_VS)?)
        } else {
            None
        };
        let mut vs_blobs: Vec<&[u8]> = vec![&stock.vs];
        let ps_blobs: Vec<&[u8]> = match aa_ps {
            Some(ref ps) => vec![ps],
            None => vec![&stock.ps],
        };
        let mut programs: Vec<(u8, u8, u8)> = vec![(0, 0, 0)];
        if let Some(ref pv) = persp_vs {
            vs_blobs.push(pv);
            programs.push((0, 1, 0));
        }
        write_container(dir, JUDGE, stock.name_hash, &vs_blobs, &ps_blobs, &programs)?;
        built.push(JUDGE);
    }

    // Default: prog0 = stock VS + stock PS (bit-identical);
    //          prog1 (persp) = persp VS + stock PS;
    //          theme programs LAST (theme passthrough VS + one PS per
    //          shader-backed menu theme) — layout per `shader_layout`
    //          (host-tested; the persp-at-index-1 contract lives there).
    if plan.persp || plan.themes {
        let stock = extract_stock(stock_arc, DEFAULT)?;
        let persp_vs = if plan.persp {
            Some(read_blob(BLOB_DEFAULT_PERSP_VS)?)
        } else {
            None
        };
        // The shared theme VS followed by one PS per theme, in
        // THEME_BLOBS (== ThemeProgram::slot()) order.
        let theme_blobs: Option<Vec<Vec<u8>>> = if plan.themes {
            let mut v = Vec::with_capacity(THEME_BLOBS.len());
            for name in THEME_BLOBS {
                v.push(read_blob(name)?);
            }
            Some(v)
        } else {
            None
        };

        let mut vs_blobs: Vec<&[u8]> = vec![&stock.vs];
        if let Some(ref pv) = persp_vs {
            vs_blobs.push(pv);
        }
        let mut ps_blobs: Vec<&[u8]> = vec![&stock.ps];
        if let Some(ref tb) = theme_blobs {
            vs_blobs.push(&tb[0]);
            for ps in &tb[1..] {
                ps_blobs.push(ps);
            }
        }

        let programs = shader_layout::default_programs(plan.persp, plan.themes);
        // Defensive re-verification of the positional contracts the pure
        // layout encodes (a wrong container must never ship — pass_rewrite
        // binds program 1 blind and the SetShader handler has no bounds
        // check).
        let (want_vs, want_ps) = shader_layout::default_table_counts(plan.persp, plan.themes);
        if plan.persp
            && programs.get(shader_layout::PERSP_PROGRAM_INDEX as usize) != Some(&(0, 1, 0))
        {
            return Err("default container layout violates the persp-program-1 contract".into());
        }
        if vs_blobs.len() != want_vs as usize || ps_blobs.len() != want_ps as usize {
            return Err("default container blob tables disagree with the layout".into());
        }

        write_container(
            dir,
            DEFAULT,
            stock.name_hash,
            &vs_blobs,
            &ps_blobs,
            &programs,
        )?;
        built.push(DEFAULT);
    }

    Ok(built)
}

fn write_container(
    dir: &str,
    name: &str,
    name_hash: u32,
    vs_blobs: &[&[u8]],
    ps_blobs: &[&[u8]],
    programs: &[(u8, u8, u8)],
) -> Result<(), String> {
    let bytes = pack_gspw(name_hash, vs_blobs, ps_blobs, programs)?;
    let path = format!("{}/{}.gsp", dir, name);
    std::fs::write(&path, &bytes).map_err(|e| format!("write '{}': {}", path, e))?;
    log_info!(
        "shader_synthesis: {} → {} bytes ({} programs, {} VS, {} PS)",
        name,
        bytes.len(),
        programs.len(),
        vs_blobs.len(),
        ps_blobs.len()
    );
    Ok(())
}

/// A stock container's sliced blobs + identity.
struct StockBlobs {
    name_hash: u32,
    vs: Vec<u8>,
    ps: Vec<u8>,
}

/// Slice VS[0]/PS[0] out of the stock container `name` inside the stock arc.
fn extract_stock(stock_arc: &[u8], name: &str) -> Result<StockBlobs, String> {
    let entry_name = format!("data/shader/{}.gsp", name);
    let entries =
        arc::parse(stock_arc).ok_or_else(|| "stock shader.arc failed to parse".to_string())?;
    let entry = entries
        .iter()
        .find(|e| e.path == entry_name)
        .ok_or_else(|| format!("stock arc has no '{}'", entry_name))?;
    let gsp = arc::extract(stock_arc, entry)
        .ok_or_else(|| format!("couldn't extract '{}'", entry_name))?;

    // GSPW parse (header layout per docs/shader_replacement_research.md §2).
    if gsp.len() < 0x20 || &gsp[0..4] != b"GSPW" {
        return Err(format!("'{}' is not a GSPW container", entry_name));
    }
    let rd32 = |off: usize| -> u32 { u32::from_le_bytes(gsp[off..off + 4].try_into().unwrap()) };
    let name_hash = rd32(0x04);
    let ptr_b = rd32(0x10) as usize;
    let ptr_c = rd32(0x14) as usize;
    let (cnt_b, cnt_c) = (gsp[0x19] as usize, gsp[0x1A] as usize);
    if cnt_b < 1 || cnt_c < 1 {
        return Err(format!("'{}' has no VS/PS entries", entry_name));
    }
    let slice_table = |tab: usize, idx: usize| -> Result<Vec<u8>, String> {
        let e = tab + idx * 8;
        if e + 8 > gsp.len() {
            return Err(format!("'{}' table entry out of bounds", entry_name));
        }
        let off = rd32(e) as usize;
        let size = rd32(e + 4) as usize;
        if off + size > gsp.len() {
            return Err(format!("'{}' blob out of bounds", entry_name));
        }
        Ok(gsp[off..off + size].to_vec())
    };
    let vs = slice_table(ptr_b, 0)?;
    let ps = slice_table(ptr_c, 0)?;
    validate_blob(&vs, &format!("{} stock VS", name))?;
    validate_blob(&ps, &format!("{} stock PS", name))?;
    Ok(StockBlobs { name_hash, vs, ps })
}

/// Sanity-check a d3dbc blob's D3D9 version token (vs_/ps_ 3.0 family).
fn validate_blob(blob: &[u8], label: &str) -> Result<(), String> {
    if blob.len() < 8 {
        return Err(format!("blob '{}' too small", label));
    }
    let tok = u32::from_le_bytes(blob[0..4].try_into().unwrap());
    match tok >> 16 {
        0xFFFE | 0xFFFF => Ok(()),
        _ => Err(format!(
            "blob '{}' has a bad version token 0x{:08X}",
            label, tok
        )),
    }
}

/// Pack a GSPW container. Mirrors `scripts/gsp_pack.py pack` byte-for-byte:
/// tables at computed offsets right after the 0x20 header (programs, then
/// VS entries, then PS entries), blobs 16-aligned in table order, zero
/// trailing slack. `programs` entries are `(flags, vs_idx, ps_idx)`.
fn pack_gspw(
    name_hash: u32,
    vs_blobs: &[&[u8]],
    ps_blobs: &[&[u8]],
    programs: &[(u8, u8, u8)],
) -> Result<Vec<u8>, String> {
    if vs_blobs.is_empty() || ps_blobs.is_empty() || programs.is_empty() {
        return Err("pack_gspw: empty inputs".into());
    }
    if vs_blobs.len() > 255 || ps_blobs.len() > 255 || programs.len() > 255 {
        return Err("pack_gspw: counts exceed u8".into());
    }
    for &(_, vsi, psi) in programs {
        if vsi as usize >= vs_blobs.len() || psi as usize >= ps_blobs.len() {
            return Err("pack_gspw: program index out of range".into());
        }
    }

    let align16 = |n: usize| (n + 15) & !15;
    let prog_off = 0x20usize;
    let vs_tab = prog_off + 8 * programs.len();
    let ps_tab = vs_tab + 8 * vs_blobs.len();
    let first_blob = align16(ps_tab + 8 * ps_blobs.len());

    let all_blobs: Vec<&[u8]> = vs_blobs.iter().chain(ps_blobs.iter()).copied().collect();
    let mut offsets = Vec::with_capacity(all_blobs.len());
    let mut pos = first_blob;
    for b in &all_blobs {
        offsets.push(pos);
        pos = align16(pos + b.len());
    }
    let total = offsets.last().unwrap() + all_blobs.last().unwrap().len();

    let mut out = vec![0u8; total];
    out[0..4].copy_from_slice(b"GSPW");
    out[0x04..0x08].copy_from_slice(&name_hash.to_le_bytes());
    out[0x0C..0x10].copy_from_slice(&(prog_off as u32).to_le_bytes());
    out[0x10..0x14].copy_from_slice(&(vs_tab as u32).to_le_bytes());
    out[0x14..0x18].copy_from_slice(&(ps_tab as u32).to_le_bytes());
    out[0x18] = programs.len() as u8;
    out[0x19] = vs_blobs.len() as u8;
    out[0x1A] = ps_blobs.len() as u8;
    for (i, &(flags, vsi, psi)) in programs.iter().enumerate() {
        let e = prog_off + 8 * i;
        out[e] = flags;
        out[e + 4] = vsi;
        out[e + 5] = psi;
    }
    for (i, b) in vs_blobs.iter().enumerate() {
        let e = vs_tab + 8 * i;
        out[e..e + 4].copy_from_slice(&(offsets[i] as u32).to_le_bytes());
        out[e + 4..e + 8].copy_from_slice(&(b.len() as u32).to_le_bytes());
    }
    for (i, b) in ps_blobs.iter().enumerate() {
        let j = vs_blobs.len() + i;
        let e = ps_tab + 8 * i;
        out[e..e + 4].copy_from_slice(&(offsets[j] as u32).to_le_bytes());
        out[e + 4..e + 8].copy_from_slice(&(b.len() as u32).to_le_bytes());
    }
    for (i, b) in all_blobs.iter().enumerate() {
        out[offsets[i]..offsets[i] + b.len()].copy_from_slice(b);
    }
    Ok(out)
}
