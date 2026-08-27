//! Impure chrome pipeline for the mod-menu modal: background PNG synthesis
//! (via the pure `chrome` layer) → hash-sidecar cache under
//! `data_mods/_cache/mod_menu/` → `asset_loader` loose-PNG load → texture
//! resolution, published to `render.rs` through atomics.
//!
//! Kicked once from the mod's `enable()` so the textures are resident before
//! the first open (asset resolution takes ~0.7 s). Flow:
//!
//! 1. `kick()` reads the full `overlay_menu` section (theme id — unknown ⇒
//!    the default theme + one WARN — animate flag, clamped opacity), then spawns ONE
//!    background thread (panic-contained) that cache-checks / synthesizes /
//!    writes both PNGs and deposits `(kind, path, stem, generation)` into
//!    the PENDING mailbox. THEME tab edits later call [`resynthesize`],
//!    which regenerates the PANEL only under the new (theme, opacity)
//!    stems — generation-tokened latest-wins, and the old panel keeps
//!    rendering until the replacement resolves.
//! 2. A render-thread pump (strip_hud's pattern) drains the mailbox, issues
//!    `asset_loader::load`, and polls `resolve_hash` until each texture
//!    binds — every asset_loader call stays on the game thread. The pump
//!    self-requeues only while work remains.
//! 3. Resolved texture ids land in atomics; `render.rs` reads them via
//!    [`status`] at every repaint (re-binding sprite fields is a cheap
//!    memory write). Any piece transition schedules one extra repaint so
//!    chrome appears even while the menu sits open and idle.
//!
//! Failure ladder (design §6, one latched WARN per class, everything
//! fail-open): synthesis/encode/write failure or load refusal ⇒ the piece is
//! FAILED; panel failed but strip resolved ⇒ render.rs stretches the tinted
//! strip as a solid panel; both failed ⇒ text-only (today's look). Textures
//! are never released — they live for the process (the menu is permanent and
//! widget nodes are, too).
//!
//! Dev fault injection: `DDR_MOD_MENU_CHROME_FAULT` = `panel` | `strip`
//! (synthesis failure) | `load` (loads refused) exercises the ladder rungs.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicI64, AtomicU32, AtomicUsize, Ordering};
use std::sync::Mutex;

use crate::services::asset_loader::{self, AssetHandle};
use crate::services::avs_layeredfs::cache_hasher::CacheHasher;
use crate::services::widget_renderer;
use crate::{log_info, log_warn};

use super::chrome;
use super::theme;
use super::MOD_MENU_STATE;

/// Cache directory for the synthesized chrome PNGs (+ `.hash` sidecars).
const CACHE_DIR: &str = "./data_mods/_cache/mod_menu";

// ── Published state (read by render.rs at every repaint) ─────────────

/// Resolved panel texture id, or −1 while unresolved.
static PANEL_TEX: AtomicI64 = AtomicI64::new(-1);
/// Resolved strip texture id, or −1 while unresolved.
static STRIP_TEX: AtomicI64 = AtomicI64::new(-1);
/// Panel piece hit a ladder rung (synthesis/load failure).
static PANEL_FAILED: AtomicBool = AtomicBool::new(false);
/// Strip piece hit a ladder rung.
static STRIP_FAILED: AtomicBool = AtomicBool::new(false);
/// Effective (clamped+snapped) opacity percent, latched at kick and
/// updated live by the THEME tab's MENU OPACITY row.
static EFFECTIVE_OPACITY: AtomicI32 = AtomicI32::new(80);
/// Active theme index into [`theme::THEMES`], latched at kick and updated
/// live by the THEME tab's THEME row.
static ACTIVE_THEME: AtomicUsize = AtomicUsize::new(theme::DEFAULT_THEME_INDEX);
/// ANIMATED BACKGROUND toggle (inert until Step 8's shader backgrounds;
/// carried here so the whole appearance state lives in one place).
static ANIMATE: AtomicBool = AtomicBool::new(true);
/// Panel synthesis generation. Every [`resynthesize`] bump makes older
/// in-flight panel work stale: stale resolves/failures are DISCARDED so
/// rapid theme/opacity cycling is latest-wins (stale textures still load
/// harmlessly — chrome textures are never released anyway).
static SYNTH_GENERATION: AtomicU32 = AtomicU32::new(0);
/// `kick()` once-latch.
static KICKED: AtomicBool = AtomicBool::new(false);

/// Active theme index (render.rs palette lookups, tabs.rs row builder).
pub(super) fn active_theme_index() -> usize {
    ACTIVE_THEME.load(Ordering::Relaxed)
}

/// ANIMATED BACKGROUND row state (inert until Step 8).
pub(super) fn animate_background() -> bool {
    ANIMATE.load(Ordering::Relaxed)
}

/// Effective (clamped) opacity percent (tabs.rs row builder).
pub(super) fn effective_opacity() -> i32 {
    EFFECTIVE_OPACITY.load(Ordering::Relaxed)
}

/// THEME row edit: latch the new theme index (clamped to the table).
pub(super) fn set_active_theme(index: usize) {
    ACTIVE_THEME.store(index.min(theme::THEMES.len() - 1), Ordering::Relaxed);
}

/// ANIMATED BACKGROUND row edit.
pub(super) fn set_animate(on: bool) {
    ANIMATE.store(on, Ordering::Relaxed);
}

/// MENU OPACITY row edit (caller clamps via `chrome::clamp_opacity`).
pub(super) fn set_effective_opacity(percent: i32) {
    EFFECTIVE_OPACITY.store(percent, Ordering::Release);
}

/// Persist the whole `overlay_menu` section from the live appearance
/// state (`save_json_key` replaces the top-level key, so every field is
/// serialized on every change — the quick_restart persist pattern). Runs
/// on the input/repeat thread; file write only, no game calls.
pub(super) fn persist_overlay_menu() {
    let theme_id = theme::theme(active_theme_index()).id;
    crate::mods::config::save_json_key(
        "overlay_menu",
        serde_json::json!({
            "theme": theme_id,
            "animate_background": animate_background(),
            "opacity": effective_opacity(),
        }),
    );
}

/// Snapshot of the chrome texture state for the renderer.
#[derive(Clone, Copy, Debug)]
pub(super) struct ChromeStatus {
    pub panel_tex: Option<i32>,
    pub strip_tex: Option<i32>,
    pub panel_failed: bool,
    /// Effective opacity percent (for the solid-fallback tint).
    pub opacity: i32,
}

pub(super) fn status() -> ChromeStatus {
    let tex = |a: &AtomicI64| -> Option<i32> {
        let v = a.load(Ordering::Acquire);
        (v >= 0).then_some(v as i32)
    };
    ChromeStatus {
        panel_tex: tex(&PANEL_TEX),
        strip_tex: tex(&STRIP_TEX),
        panel_failed: PANEL_FAILED.load(Ordering::Acquire),
        opacity: EFFECTIVE_OPACITY.load(Ordering::Acquire),
    }
}

// ── Latched WARNs (one per failure class) ────────────────────────────

static WARNED_SYNTH: AtomicBool = AtomicBool::new(false);
static WARNED_LOAD: AtomicBool = AtomicBool::new(false);
static WARNED_PANIC: AtomicBool = AtomicBool::new(false);

fn warn_once(latch: &AtomicBool, msg: &str) {
    if !latch.swap(true, Ordering::Relaxed) {
        log_warn!("ModMenu chrome: {}", msg);
    }
}

// ── Pending mailbox (synthesis thread → game-thread pump) ────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PieceKind {
    Panel,
    Strip,
}

struct PendingFile {
    kind: PieceKind,
    path: String,
    stem: String,
    /// Panel synthesis generation this file belongs to (strip work is
    /// kick-only and ignores generations).
    generation: u32,
}

/// In-flight load state per piece (game thread only). The AssetHandle is
/// retained so the load/release pairing stays intact — we intentionally
/// never release (process-lifetime textures).
struct Loading {
    kind: PieceKind,
    name_hash: u32,
    generation: u32,
    _handle: AssetHandle,
}

static PENDING: Mutex<Vec<PendingFile>> = Mutex::new(Vec::new());
static LOADING: Mutex<Vec<Loading>> = Mutex::new(Vec::new());

/// True while `generation` is the newest panel synthesis — stale panel
/// work must neither publish nor mark failure (a stale failure would
/// knock out a healthy newer panel).
fn generation_current(generation: u32) -> bool {
    generation == SYNTH_GENERATION.load(Ordering::Acquire)
}

fn mark_failed(kind: PieceKind, generation: u32) {
    match kind {
        PieceKind::Panel => {
            if generation_current(generation) {
                PANEL_FAILED.store(true, Ordering::Release);
            }
        }
        PieceKind::Strip => STRIP_FAILED.store(true, Ordering::Release),
    }
}

fn fault(mode: &str) -> bool {
    std::env::var("DDR_MOD_MENU_CHROME_FAULT").as_deref() == Ok(mode)
}

// ── Kick (mod enable) ────────────────────────────────────────────────

/// Start the chrome pipeline: read the full `overlay_menu` section,
/// synthesize/cache on a background thread, then hand off to the
/// game-thread load pump. Idempotent.
pub(super) fn kick() {
    if KICKED.swap(true, Ordering::Relaxed) {
        return;
    }

    let section = crate::mods::config::get().and_then(|c| c.overlay_menu.as_ref());

    let configured_theme = section.and_then(|o| o.theme.as_deref());
    let (theme_index, known) = theme::resolve_theme_index(configured_theme);
    if !known {
        log_warn!(
            "ModMenu chrome: unknown overlay_menu.theme {:?} — using {:?}",
            configured_theme.unwrap_or(""),
            theme::theme(theme_index).id
        );
    }
    ACTIVE_THEME.store(theme_index, Ordering::Relaxed);

    ANIMATE.store(
        section.and_then(|o| o.animate_background).unwrap_or(true),
        Ordering::Relaxed,
    );

    let raw = section.and_then(|o| o.opacity).unwrap_or(80);
    let opacity = chrome::clamp_opacity(raw);
    if opacity != raw {
        log_info!(
            "ModMenu chrome: overlay_menu.opacity {} normalized to {}",
            raw,
            opacity
        );
    }
    EFFECTIVE_OPACITY.store(opacity, Ordering::Release);

    let generation = SYNTH_GENERATION.load(Ordering::Acquire);
    spawn_synthesis(generation, theme_index, opacity, true);
}

/// Regenerate the panel for the CURRENT appearance state (theme/opacity
/// row edits). Latest-wins: bumps the generation so any in-flight older
/// panel work goes stale, and clears the panel-failure latch so a
/// recovered synthesis un-runs the solid-fallback rung. The old panel
/// texture keeps rendering until the replacement resolves (publish
/// happens only on resolve — nothing here clears `PANEL_TEX`).
pub(super) fn resynthesize() {
    let generation = SYNTH_GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
    PANEL_FAILED.store(false, Ordering::Release);
    let theme_index = active_theme_index();
    let opacity = effective_opacity();
    spawn_synthesis(generation, theme_index, opacity, false);
}

/// Spawn one panic-contained synthesis thread for the given appearance
/// state. `include_strip` only on the boot kick (the strip is
/// theme/opacity-invariant).
fn spawn_synthesis(generation: u32, theme_index: usize, opacity: i32, include_strip: bool) {
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(|| {
            synthesis_thread(generation, theme_index, opacity, include_strip)
        });
        if result.is_err() {
            warn_once(
                &WARNED_PANIC,
                "synthesis thread panicked — chrome unavailable (text-only menu)",
            );
            mark_failed(PieceKind::Panel, generation);
            if include_strip {
                mark_failed(PieceKind::Strip, generation);
            }
        }
    });
}

// ── Synthesis + cache (background thread) ────────────────────────────

fn synthesis_thread(generation: u32, theme_index: usize, opacity: i32, include_strip: bool) {
    if std::fs::create_dir_all(CACHE_DIR).is_err() {
        warn_once(
            &WARNED_SYNTH,
            "cache dir creation failed — chrome unavailable (text-only menu)",
        );
        mark_failed(PieceKind::Panel, generation);
        if include_strip {
            mark_failed(PieceKind::Strip, generation);
        }
        return;
    }

    let theme = theme::theme(theme_index);
    let mut pieces: Vec<(PieceKind, String, String)> = vec![(
        PieceKind::Panel,
        chrome::panel_file_stem(theme.id, opacity),
        chrome::cache_key_material(theme.id, opacity),
    )];
    if include_strip {
        pieces.push((
            PieceKind::Strip,
            chrome::strip_file_stem(),
            // The strip is theme/opacity-invariant; its key only carries the
            // layout version (cache_key_material is the panel's key shape).
            format!("chrome-strip:v{}", chrome::LAYOUT_VERSION),
        ));
    }

    for (kind, stem, key) in pieces {
        let path = format!("{CACHE_DIR}/{stem}.png");
        let ready = ensure_piece_file(kind, &path, &key, &theme.palette.gradient(), opacity);
        if ready {
            if let Ok(mut pending) = PENDING.lock() {
                pending.push(PendingFile {
                    kind,
                    path,
                    stem,
                    generation,
                });
            }
        } else {
            mark_failed(kind, generation);
        }
    }

    // Hand off to the game thread: issue loads + poll resolution.
    widget_renderer::run_on_render_thread(pump);
}

/// Ensure the piece's PNG exists and matches the cache key: sidecar hit ⇒
/// reuse; miss ⇒ synthesize, encode, write, commit. Returns readiness.
fn ensure_piece_file(
    kind: PieceKind,
    path: &str,
    key: &str,
    gradient: &chrome::PanelGradient,
    opacity: i32,
) -> bool {
    let injected_fault = match kind {
        PieceKind::Panel => fault("panel"),
        PieceKind::Strip => fault("strip"),
    };
    if injected_fault {
        warn_once(
            &WARNED_SYNTH,
            "DDR_MOD_MENU_CHROME_FAULT — simulated synthesis failure",
        );
        return false;
    }

    let hash_path = format!("{path}.hash");
    let mut hasher = CacheHasher::new(&hash_path);
    hasher.add_str(key);
    hasher.finish();
    if hasher.matches() && std::path::Path::new(path).is_file() {
        log_info!("ModMenu chrome: cache hit for {}", path);
        return true;
    }

    let img = match kind {
        PieceKind::Panel => chrome::synthesize_panel(gradient, opacity),
        PieceKind::Strip => chrome::synthesize_strip(),
    };
    let bytes = match chrome::encode_png(&img) {
        Ok(b) => b,
        Err(e) => {
            warn_once(&WARNED_SYNTH, e.describe());
            return false;
        }
    };
    if std::fs::write(path, &bytes).is_err() {
        warn_once(&WARNED_SYNTH, "cache file write failed");
        return false;
    }
    hasher.commit();
    log_info!(
        "ModMenu chrome: synthesized {} ({} bytes, opacity {}%)",
        path,
        bytes.len(),
        opacity
    );
    true
}

// ── Load + resolve pump (game thread) ────────────────────────────────

/// Drain pending loads, poll in-flight resolutions, publish texture ids.
/// Self-requeues while any work remains; schedules one repaint per
/// transition so an idle-open menu picks late chrome up immediately.
fn pump() {
    let mut transitioned = false;

    // Issue loads for freshly synthesized/cached files.
    let pending: Vec<PendingFile> = match PENDING.lock() {
        Ok(mut p) => p.drain(..).collect(),
        Err(_) => Vec::new(),
    };
    for file in pending {
        if fault("load") {
            warn_once(
                &WARNED_LOAD,
                "DDR_MOD_MENU_CHROME_FAULT — simulated load failure",
            );
            mark_failed(file.kind, file.generation);
            transitioned = true;
            continue;
        }
        if !asset_loader::is_available() {
            warn_once(
                &WARNED_LOAD,
                "asset_loader unavailable — chrome unavailable (text-only menu)",
            );
            mark_failed(file.kind, file.generation);
            transitioned = true;
            continue;
        }
        match asset_loader::load(&file.path, &file.stem) {
            Some(handle) => {
                let name_hash = handle.name_hash;
                if let Ok(mut loading) = LOADING.lock() {
                    loading.push(Loading {
                        kind: file.kind,
                        name_hash,
                        generation: file.generation,
                        _handle: handle,
                    });
                }
            }
            None => {
                warn_once(&WARNED_LOAD, "texture load request refused");
                mark_failed(file.kind, file.generation);
                transitioned = true;
            }
        }
    }

    // Poll in-flight resolutions.
    let mut still_loading = false;
    if let Ok(mut loading) = LOADING.lock() {
        loading.retain(|entry| match asset_loader::resolve_hash(entry.name_hash) {
            Some(texture) => {
                let tex = texture.handle as i64;
                match entry.kind {
                    // Stale panel resolves are discarded (latest-wins;
                    // the texture stays resident harmlessly — chrome
                    // textures are never released).
                    PieceKind::Panel => {
                        if generation_current(entry.generation) {
                            PANEL_TEX.store(tex, Ordering::Release);
                            transitioned = true;
                        } else {
                            log_info!(
                                "ModMenu chrome: stale Panel resolve (gen {}) discarded",
                                entry.generation
                            );
                        }
                    }
                    PieceKind::Strip => {
                        STRIP_TEX.store(tex, Ordering::Release);
                        transitioned = true;
                    }
                }
                log_info!(
                    "ModMenu chrome: {:?} texture resolved (id={})",
                    entry.kind,
                    texture.handle
                );
                false
            }
            None => {
                still_loading = true;
                true
            }
        });
    }

    // An open, idle menu repaints only on input — push one refresh so chrome
    // that resolved after opening appears without a nav event.
    if transitioned {
        widget_renderer::run_on_render_thread(|| {
            let Ok(state) = MOD_MENU_STATE.lock() else {
                return;
            };
            super::render::refresh_all(&state);
        });
    }

    let pending_remains = PENDING.lock().map(|p| !p.is_empty()).unwrap_or(false);
    if still_loading || pending_remains {
        widget_renderer::run_on_render_thread(pump);
    }
}
