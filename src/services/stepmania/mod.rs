//! StepMania pack loader — the game-facing service (design §3, §4.7).
//!
//! Operators drop StepMania packs into `./stepmania/Songs/<Pack>/<Song>/`
//! and run the prep tool (`ddr_world_hook_prep.exe`, crate `prep/`) before
//! the game; it converts charts and audio into the game's own formats under
//! `data_mods/_cache/stepmania/`. At boot this service READS that cache,
//! injects the songs into the music database so they appear on the wheel in
//! a STEPMANIA folder / VERSION series and serves them like stock songs —
//! scores are never submitted (`taint.rs`). The DLL never converts: the only
//! way a song reaches the game is through a cache the tool produced.
//!
//! ## Boot timeline (why `early_init` sits where it does in `lib.rs`)
//!
//! ```text
//! init thread                               game thread (gamemdx onBoot)
//! 0   config::init
//! 0'  stepmania::early_init                 …
//!     ├ root + README, settings
//!     ├ register resolver (mod_paths)
//!     ├ register startup.arc musicdb transform + fingerprint (arc_handler)
//!     ├ cache layout + LRU, index::load (index.bin → lookup maps, ms)
//!     └ index::validate_async ──────────────► read-only, below-normal workers
//!                                              (done → hide what changed)
//! 0b  avs_layeredfs::init (hooks live)
//! 1-2 gamemdx wait, resolve_all …           avs_fs_open arc/startup.arc
//!                                             pre-repack cb: arena patch
//!                                             fingerprint cb: injectable (ceiling) -- no wait
//!                                                             injectable (ceiling)
//!                                                             ensure_registration (folder bit + series)
//!                                                             thumbnail arc
//!                                             transform cb:   musicdb::transform (inject)
//! 7   mods register (StepmaniaPacksMod::init → ensure_registration — the
//!     slow-cabinet direction; listed BEFORE series/folder expansion —
//!     and init_runtime → scheduler triggers: wheel_cards, highlight, scenes)
//! 8   mods enable (series/folder expansion read the runtime registries;
//!     StepmaniaPacksMod::enable → auto-sync ledger when auto_sync is on)
//! ```
//!
//! Audio delivery after boot (design §3.3): wheel-card binds and the
//! highlight feed `scheduler.rs`, which builds preview banks and jackets on
//! a small worker pool before the wheel settles; the gameplay bank is a
//! progressive file (`progressive.rs`) whose reads the LayeredFS read hook
//! holds at the writer's watermark, so a stage load never waits on an
//! encode. `providers.rs` is the resolver that ties the two together. The
//! same full-bank request is the gameplay-load signal for the score taint
//! (`taint.rs`) and for publishing the song's `#SCROLLS`/`#SPEEDS` tables
//! to `services::scroll_remap` (`on_gameplay_load`; cleared on leaving
//! scene 28 — Step 9).
//!
//! Auto-sync: the tool measures each song against its audio and records ONE
//! delta per song in `sync.bin`; `sync_apply.rs` loads the ledger and applies
//! the delta at play time through the game's JUDGEMENT OFFSET by
//! `per_song_judgement_offsets` (`applied = (player's per-song entry, else
//! stock) + delta`, ±100), frozen per song for the session. A song without a
//! record plays as authored. Off with `auto_sync: false`.
//!
//! The two consumers of the folder bit and series value — the musicdb
//! entries (written inside the `startup.arc` open) and the
//! `folder_expansion` / `series_expansion` mods (their `init` at step 7) —
//! can come in either order depending on the cabinet (CrossOver reaches the
//! `startup.arc` open seconds AFTER `resolve_all`; the Win7 tester before).
//! `ensure_registration` is therefore idempotent and called from both: the
//! first caller picks the lowest free bit / series through task-01's APIs
//! and builds the thumbnail arc; the second finds it done. Registration
//! happens only when the index holds ≥ 1 song.
//!
//! ## Cache ownership
//!
//! Tool writes `index.bin`, `ssq/`, `xsb/`, `tn/`, `sync.bin`. DLL writes
//! `xwb/`, `jacket/`, `lru.bin`, `thumbnail/`. Neither touches the other's
//! files, and the tool refuses to run while the game is up — there is no
//! second writer to race.
//!
//! ## The injection ceiling
//!
//! The index is unbounded; what reaches the game is not. `ceiling.rs`
//! selects the subset one boot may inject — a fixed count ceiling over
//! everything downstream that scales with the injected count (thumbnail
//! arc, boot pass, wheel) and nothing at all when the XML property arena is
//! stock-sized (`song_limit_expansion` off or its patch failed: the stock
//! document already fills most of it, and an overflowing parse loads NO
//! songs and stalls the boot) — and `musicdb::transform` enforces the arena
//! text budget on the final document as the last safety net. Every path
//! that publishes "the songs this boot" (registration + thumbnail arc, the
//! `startup.arc` fingerprint, the transform, `injected_entries`) goes
//! through [`injectable`], so they agree. One WARN per boot names what was
//! left out and why; the operator's lever is the folder contents (the first
//! packs alphabetically are kept).
//!
//! ## Release / update safety
//!
//! The release ships `stepmania/Songs/README.txt` and the prep tool. The
//! updater's plan (`updater/src/plan.rs`) writes release-owned files, prunes
//! a path only when the previous manifest listed it AND its on-disk hash is
//! unchanged, and never touches anything in neither set — so operator packs
//! under `stepmania/Songs/` and the tool-built `data_mods/_cache/stepmania/`
//! survive every update.

pub mod cache;
pub mod ceiling;
pub mod convert;
pub mod hide;
pub mod identity;
pub mod index;
pub mod model;
pub mod musicdb;
pub mod progressive;
pub mod progressive_logic;
pub mod providers;
pub mod records;
pub mod scheduler;
pub mod scheduler_logic;
pub mod ssq_split;
pub mod sync_apply;
pub mod sync_state;
pub mod taint;
pub mod thumbnail_arc;
pub mod validation;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use once_cell::sync::OnceCell;

use crate::core::sm::scroll::ScrollGimmicks;
use crate::mods::config::{self, StepmaniaPacksConfig};
use crate::mods::folder_expansion::{self, CustomFolderEntry};
use crate::mods::mod_trait::mod_enabled_in_config;
use crate::mods::series_expansion::{self, CustomSeriesEntry};
use crate::mods::song_limit_expansion;
use crate::services::avs_layeredfs::{arc_handler, mod_paths};
use crate::services::{scene_manager, scroll_remap};
use crate::types::scenes::scene;
use crate::{log_info, log_warn};

use index::IndexSnapshot;

/// The mod id this service is gated on.
pub const MOD_ID: &str = "stepmania-packs";
/// Songs root, relative to the game directory.
pub const SONGS_ROOT: &str = "./stepmania/Songs";
/// Enhanced-layout label art for the STEPMANIA series.
pub const SERIES_LABEL_DIR: &str = "./data_mods/stepmania_packs/series_labels";
pub const SERIES_LABEL: &str = "STEPMANIA";
pub const SERIES_TEXTURE: &str = "stepmania";
pub const FOLDER_KEY: &str = "stepmania";
/// Operator note written into the songs root on first boot.
const SONGS_README: &str = include_str!("songs_readme.txt");
/// The tool that writes the cache this service reads.
pub const PREP_TOOL: &str = "ddr_world_hook_prep.exe";
/// Songs the background validation dropped, waiting for a music DB to hide
/// them in; `guards_ran` once the visibility guards have applied (the DB
/// exists from then on, so later drops are hidden immediately).
static VALIDATION_DROPS: Mutex<(Vec<u32>, bool)> = Mutex::new((Vec::new(), false));

/// `index::validator_done` → hide the dropped songs now if the music DB is
/// up (the guards ran), else park them for `take_validation_drops`.
pub(crate) fn on_validation_complete(dropped: &[u32]) {
    if dropped.is_empty() {
        return;
    }
    let hide_now = match VALIDATION_DROPS.lock() {
        Ok(mut d) => {
            if d.1 {
                true
            } else {
                d.0.extend_from_slice(dropped);
                false
            }
        }
        Err(_) => false,
    };
    if hide_now {
        hide::hide_unservable(dropped, "changed or missing since the last prep run");
    }
}

/// The visibility guards are running (music DB built): hand over whatever
/// validation dropped so far and mark the DB available.
pub(crate) fn take_validation_drops() -> Vec<u32> {
    match VALIDATION_DROPS.lock() {
        Ok(mut d) => {
            d.1 = true;
            std::mem::take(&mut d.0)
        }
        Err(_) => Vec::new(),
    }
}

struct Settings {
    root: PathBuf,
    cfg: StepmaniaPacksConfig,
}

static SETTINGS: OnceCell<Settings> = OnceCell::new();
/// Set once the index holds at least one entry (the resolver's cheap gate).
static SERVING: AtomicBool = AtomicBool::new(false);
static CEILING_WARNED: AtomicBool = AtomicBool::new(false);
/// The scroll-gimmick scene-exit clear is subscribed once.
static SCROLL_SCENE_ARMED: AtomicBool = AtomicBool::new(false);

/// What `ensure_registration` decided.
#[derive(Debug, Clone, Copy)]
pub struct Registration {
    pub series: Option<u8>,
    pub folder_bit: Option<u32>,
}

static REGISTRATION: OnceCell<Registration> = OnceCell::new();
static REGISTRATION_LOCK: Mutex<()> = Mutex::new(());
/// The snapshot the `startup.arc` open decided on (final for this boot).
static INJECTED: OnceCell<Arc<Vec<Arc<model::SongEntry>>>> = OnceCell::new();
/// The snapshot the fingerprint callback hashed, handed to the transform of
/// the same open so the cache key and the injected set agree even when the
/// index completes between the two calls.
static STARTUP_SNAPSHOT: Mutex<Option<IndexSnapshot>> = Mutex::new(None);
/// The `musicdb.xml` transform ran this boot (the `startup.arc` cache was
/// stale). When it did not but the fingerprint callback did, the cached arc
/// was built from the identical index — see [`injected_entries`].
static TRANSFORM_RAN: AtomicBool = AtomicBool::new(false);

/// Step 0': config-gated. Creates the songs root, spawns the index, registers
/// the resolver and the `startup.arc` transform. Must run BEFORE
/// `avs_layeredfs::init` (the transform must exist when the hooks go live)
/// and after `config::init`.
pub fn early_init() {
    let Some(cfg_file) = config::get() else {
        log_warn!("StepMania: config store unavailable -- loader disabled");
        return;
    };
    if !mod_enabled_in_config(&cfg_file.mods, MOD_ID) {
        log_info!(
            "StepMania: '{}' disabled in config -- loader idle this boot",
            MOD_ID
        );
        return;
    }
    let cfg = cfg_file.stepmania_packs.clone().unwrap_or_default();
    if cfg.gimmick_policy != "hide_chart" {
        log_warn!(
            "StepMania: gimmick_policy {:?} is not supported yet -- using hide_chart",
            cfg.gimmick_policy
        );
    }
    let root = PathBuf::from(SONGS_ROOT);
    if let Err(e) = std::fs::create_dir_all(&root) {
        log_warn!(
            "StepMania: cannot create {}: {} -- loader disabled",
            SONGS_ROOT,
            e
        );
        return;
    }
    let readme = root.join("README.txt");
    if !readme.exists() {
        if let Err(e) = std::fs::write(&readme, SONGS_README) {
            log_warn!("StepMania: cannot write {}/README.txt: {}", SONGS_ROOT, e);
        }
    }
    let settings = Settings {
        root: root.clone(),
        cfg: cfg.clone(),
    };
    if SETTINGS.set(settings).is_err() {
        return;
    }

    mod_paths::register_resolver(providers::resolve);
    arc_handler::register_arc_fingerprint("arc/startup.arc", startup_fingerprint);
    arc_handler::register_member_transform(
        "arc/startup.arc",
        "data/gamedata/musicdb.xml",
        musicdb_transform,
    );

    // The cache the tool wrote. Layout + LRU are the DLL's (its own writes:
    // banks, jackets, the thumbnail arc); `index.bin` and the per-song
    // artefacts are read-only here.
    if !cache::ensure_layout() {
        log_warn!("StepMania: cache layout unavailable -- no songs this boot");
        return;
    }
    let swept = cache::sweep_tmp_files();
    if swept > 0 {
        log_info!("StepMania: swept {} stale .tmp file(s)", swept);
    }
    cache::init_lru(cfg.audio_cache_mb);
    let report = index::load();
    match report.source {
        index::LoadSource::Missing => {
            log_warn!(
                "StepMania: no {} -- run {} before the game to index the packs in {}; nothing injected this boot",
                cache::to_game_path(&cache::index_path()),
                PREP_TOOL,
                SONGS_ROOT
            );
        }
        index::LoadSource::Corrupt => {
            log_warn!(
                "StepMania: {} is unreadable -- run {} again; nothing injected this boot",
                cache::to_game_path(&cache::index_path()),
                PREP_TOOL
            );
        }
        index::LoadSource::Loaded => {
            if report.songs > 0 {
                index::validate_async(&root);
            }
            log_info!(
                "StepMania: index.bin -- {} song(s), {} cached rejection(s){} (audio cache {} MiB, exclude_from_all_music={}, auto_sync={})",
                report.songs,
                report.rejects,
                if report.songs > 0 { "; validating" } else { "" },
                cfg.audio_cache_mb,
                cfg.exclude_from_all_music,
                cfg.auto_sync
            );
        }
    }
}

/// Whether `early_init` armed the loader this boot.
#[must_use]
pub fn is_armed() -> bool {
    SETTINGS.get().is_some()
}

/// Step 7 (mod init): arm the prefetch scheduler's triggers — wheel-card
/// events, the per-frame highlight poll, scene changes — the scroll-gimmick
/// clear on leaving gameplay, and the `ssq_path` resolver for injected
/// basenames (`<b>.ssq` / `<b>_N.ssq`). Needs the services (`wheel_cards`,
/// `input_manager`, `scene_manager`, `ssq_path`) initialised, hence not part
/// of `early_init`. Idempotent; no-op when not armed.
pub fn init_runtime() {
    if !is_armed() {
        return;
    }
    scheduler::init_triggers();
    if !ssq_split::register() {
        log_warn!(
            "StepMania: ssq_path unavailable -- songs with per-chart timing (<b>_N.ssq) cannot be served"
        );
    }
    if scroll_gimmicks_enabled()
        && scroll_remap::is_available()
        && scene_manager::is_available()
        && !SCROLL_SCENE_ARMED.swap(true, Ordering::AcqRel)
    {
        scene_manager::on_scene_change(Box::new(|prev, next| {
            if prev == scene::GAMEPLAY && next != scene::GAMEPLAY {
                scroll_remap::clear();
            }
        }));
    }
}

/// `./stepmania/Songs` once armed.
#[must_use]
pub fn songs_root() -> Option<PathBuf> {
    SETTINGS.get().map(|s| s.root.clone())
}

/// `stepmania_packs.scroll_gimmicks` (false when the loader is not armed).
#[must_use]
pub fn scroll_gimmicks_enabled() -> bool {
    SETTINGS.get().is_some_and(|s| s.cfg.scroll_gimmicks)
}

/// `stepmania_packs.auto_sync` (false when the loader is not armed).
#[must_use]
pub fn auto_sync_enabled() -> bool {
    SETTINGS.get().is_some_and(|s| s.cfg.auto_sync)
}

/// Mod enable: arm auto-sync when armed and `auto_sync` is on — load the
/// ledger the prep tool wrote and register the judgement-offset provider
/// (`sync_apply.rs`). `true` when the provider is live. Idempotent.
pub fn init_sync() -> bool {
    is_armed() && sync_apply::init()
}

/// The committed `#SCROLLS`/`#SPEEDS` tables of an injected basename and
/// the music count of its beat 0 (the as-converted `tempo_data[0]`);
/// `None` for unknown basenames and for songs without gimmicks.
#[must_use]
pub fn scroll_tables_for(basename: &str) -> Option<(Arc<ScrollGimmicks>, i32)> {
    let entry = index::entry_by_basename(basename)?;
    let g = entry.scroll.as_ref()?;
    Some((Arc::new(g.clone()), entry.beat0_ms))
}

/// Gameplay-load signal from `taint::note_bank_request` (the full-bank
/// serve): publish the song's tables, or clear for a stock / plain song.
pub(crate) fn on_gameplay_load(basename: &str, injected: bool) {
    if !scroll_remap::is_available() {
        return;
    }
    if !injected || !scroll_gimmicks_enabled() {
        scroll_remap::clear();
        return;
    }
    match scroll_tables_for(basename) {
        Some((tables, beat0_ms)) => {
            let entry = index::entry_by_basename(basename);
            let label = entry
                .map(|e| format!("{}/{} ({})", e.pack, e.folder, basename))
                .unwrap_or_else(|| basename.to_string());
            scroll_remap::publish(&label, tables, beat0_ms);
        }
        None => scroll_remap::clear(),
    }
}

/// Whether the resolver may answer (≥ 1 song indexed).
#[must_use]
pub fn serving() -> bool {
    SERVING.load(Ordering::Acquire)
}

/// The STEPMANIA series value, once registered.
#[must_use]
pub fn registered_series() -> Option<u8> {
    REGISTRATION.get().and_then(|r| r.series)
}

/// The STEPMANIA folder bit, once registered.
#[must_use]
pub fn registered_folder_bit() -> Option<u32> {
    REGISTRATION.get().and_then(|r| r.folder_bit)
}

/// Is this mcode an injected song's? (Step 7's sanitiser predicate.)
#[must_use]
pub fn is_injected(mcode: u32) -> bool {
    index::has_mcode(mcode)
}

/// Is this basename an injected song's?
#[must_use]
pub fn is_injected_basename(basename: &str) -> bool {
    index::has_basename(basename)
}

/// The index entry behind an injected mcode (declared levels, basename) —
/// `fast_bootup`'s boot pre-flight input.
#[must_use]
pub fn injected_song(mcode: u32) -> Option<Arc<model::SongEntry>> {
    index::entry_by_mcode(mcode)
}

/// Index summary for the mod's `init` log / a future splash line.
#[must_use]
pub fn status() -> Option<IndexSnapshot> {
    if !is_armed() {
        return None;
    }
    Some(index::snapshot())
}

/// The loaded index (validation runs in the background and hides what it
/// drops afterwards — nothing here waits).
fn wait_index() -> IndexSnapshot {
    let snap = index::snapshot();
    if !snap.songs.is_empty() {
        SERVING.store(true, Ordering::Release);
    }
    snap
}

/// This boot's injection budget: the arena the game's parsers will use
/// (accurate from `early_apply` onwards and inside the `startup.arc` open,
/// after its pre-repack hook), whether their property trees are extended,
/// the live step-data record pool and the fixed count ceiling.
fn budget() -> ceiling::Budget {
    ceiling::Budget {
        arena_bytes: song_limit_expansion::arena_bytes(),
        max_songs: ceiling::MAX_INJECTED_SONGS,
        record_pool: song_limit_expansion::record_pool(),
        property_extended: song_limit_expansion::property_extended(),
        arc_chunk_bytes: song_limit_expansion::arc_chunk_bytes(),
        texture_slots: song_limit_expansion::texture_slots(),
    }
}

/// The subset of `snap` this boot may inject (`ceiling::select`), as a
/// snapshot in selection order; WARNs once per boot about what was left
/// out. Every publisher of "the songs this boot" goes through here.
fn injectable(snap: IndexSnapshot) -> IndexSnapshot {
    let b = budget();
    let sel = ceiling::select(&snap.songs, &b);
    if (sel.arena_unpatched && !snap.songs.is_empty()) || sel.left_out() > 0 {
        if !CEILING_WARNED.swap(true, Ordering::AcqRel) {
            if sel.arena_unpatched {
                log_warn!(
                    "StepMania: the game's XML arena is stock-sized ({} MiB; Song Limit Expansion off or its patch failed) -- none of the {} indexed song(s) injected: the stock musicdb already fills it",
                    b.arena_bytes >> 20,
                    snap.songs.len()
                );
            } else if sel.over_pool > 0 {
                log_warn!(
                    "StepMania: the game's step-data record pool holds {} records this boot ({} for injected files) -- the first {} song(s) by pack/folder name are injected, {} left out{}",
                    b.record_pool,
                    b.file_budget(),
                    sel.kept.len(),
                    sel.left_out(),
                    if b.record_pool <= ceiling::STOCK_RECORD_POOL {
                        " (Song Limit Expansion's pool patch did not land before the game built its loader -- it applies from the next boot)"
                    } else {
                        ""
                    }
                );
            } else if sel.over_textures > 0 {
                log_warn!(
                    "StepMania: the game's texture table holds {} textures this boot ({} for injected thumbnails) -- the first {} song(s) by pack/folder name are injected, {} left out{}",
                    b.texture_slots,
                    b.texture_budget(),
                    sel.kept.len(),
                    sel.left_out(),
                    if b.texture_slots <= ceiling::STOCK_TEXTURE_SLOTS {
                        " (Song Limit Expansion's texture-table patch did not land before the game built its renderer -- it applies from the next boot)"
                    } else {
                        ""
                    }
                );
            } else if sel.over_arc > 0 {
                log_warn!(
                    "StepMania: the game's arc loader streams {} KiB chunks this boot and the thumbnail arc's header must fit one -- the first {} song(s) by pack/folder name are injected, {} left out{}",
                    b.arc_chunk_bytes >> 10,
                    sel.kept.len(),
                    sel.left_out(),
                    if b.arc_chunk_bytes <= ceiling::STOCK_ARC_CHUNK_BYTES {
                        " (Song Limit Expansion's arc-chunk patch did not land before the game built its loader -- it applies from the next boot)"
                    } else {
                        ""
                    }
                );
            } else if sel.over_nodes > 0 {
                log_warn!(
                    "StepMania: the game's music-DB property tree is in stock mode this boot (65,535-node cap; Song Limit Expansion could not derive the parser-mode sites) -- the first {} song(s) by pack/folder name are injected, {} left out",
                    sel.kept.len(),
                    sel.left_out()
                );
            } else {
                log_warn!(
                    "StepMania: {} indexed song(s) exceed the {}-song ceiling -- the first {} by pack/folder name are injected, {} left out (remove packs to choose)",
                    snap.songs.len(),
                    b.max_songs,
                    sel.kept.len(),
                    sel.over_count
                );
            }
        }
    }
    let songs: Vec<Arc<model::SongEntry>> = sel
        .kept
        .iter()
        .filter_map(|&i| snap.songs.get(i).cloned())
        .collect();
    IndexSnapshot {
        songs: Arc::new(songs),
        ..snap
    }
}

/// Register the STEPMANIA folder bit and series value (lowest free of each)
/// and build the thumbnail arc — once, from whichever path gets there first
/// (the `startup.arc` open or the mod's `init`). No-op until the index has
/// ≥ 1 injectable song. Returns the registration (series may be `None`
/// when every value is taken — then nothing is injected).
pub fn ensure_registration() -> Option<Registration> {
    if !is_armed() {
        return None;
    }
    if let Some(r) = REGISTRATION.get() {
        return Some(*r);
    }
    let _guard = REGISTRATION_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(r) = REGISTRATION.get() {
        return Some(*r);
    }
    let snap = injectable(wait_index());
    if snap.songs.is_empty() {
        return None;
    }
    let exclude = SETTINGS
        .get()
        .map(|s| s.cfg.exclude_from_all_music)
        .unwrap_or(false);

    let series = match series_expansion::lowest_free_series_value() {
        Some(v) => match series_expansion::register_runtime_series(
            CustomSeriesEntry {
                series_value: v,
                label: SERIES_LABEL.to_string(),
                texture_name: SERIES_TEXTURE.to_string(),
            },
            SERIES_LABEL_DIR,
        ) {
            Ok(()) => Some(v),
            Err(e) => {
                log_warn!(
                    "StepMania: series registration failed: {} -- nothing injected",
                    e
                );
                None
            }
        },
        None => {
            log_warn!("StepMania: no free series value (22..=127 all in use) -- nothing injected");
            None
        }
    };
    let folder_bit = match folder_expansion::highest_free_bit() {
        Some(bit) => match folder_expansion::register_runtime_folder(CustomFolderEntry {
            bit_index: bit,
            key: FOLDER_KEY.to_string(),
            voice_key: String::new(),
            exclude_from_all_music: exclude,
        }) {
            Ok(()) => Some(bit),
            Err(e) => {
                log_warn!(
                    "StepMania: folder registration failed: {} -- songs land in ALL MUSIC only",
                    e
                );
                None
            }
        },
        None => {
            log_warn!("StepMania: no free folder bit -- songs land in ALL MUSIC only");
            None
        }
    };
    let reg = Registration { series, folder_bit };
    let _ = REGISTRATION.set(reg);
    log_info!(
        "StepMania: registered series {:?} / folder bit {:?} for {} song(s)",
        series,
        folder_bit,
        snap.songs.len()
    );
    if let Some(s) = series {
        build_thumbnail_arc(s, &snap);
    }
    Some(reg)
}

static THUMBNAIL_LOCK: Mutex<()> = Mutex::new(());

/// Confirm the series thumbnail arc the prep tool built (`thumbnail_arc.rs`);
/// builds it only when the tool did not (digest mismatch), e.g. an older
/// tool. Shared code with the tool, so the digest agrees.
fn build_thumbnail_arc(series: u8, snap: &IndexSnapshot) {
    let _guard = THUMBNAIL_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let entries = snap.entries();
    match thumbnail_arc::ensure_logged(&entries, "StepMania") {
        thumbnail_arc::Outcome::UpToDate => {
            log_info!(
                "StepMania: thumbnail arc for series {} -- {} member(s), built by the prep tool",
                series,
                entries.len()
            );
        }
        thumbnail_arc::Outcome::Built { .. } => {
            log_warn!(
                "StepMania: the prep tool had not built the thumbnail arc for this song set -- built at boot (run {} to avoid this)",
                PREP_TOOL
            );
        }
        thumbnail_arc::Outcome::Failed => {
            log_warn!(
                "StepMania: could not write the thumbnail arc -- wheel thumbnails will be missing"
            );
        }
    }
}

/// `arc/startup.arc` cache-key contribution: the digest of the injectable
/// subset plus the series / bit the entries will carry and the arena size
/// (a toggled Song Limit Expansion changes the ceiling verdict). Runs inside
/// the game's open, BEFORE the transform, so it is where the bounded wait
/// and the registration happen.
fn startup_fingerprint() -> Option<String> {
    let reg = ensure_registration();
    let snap = injectable(index::snapshot());
    let entries = snap.entries();
    let arena = song_limit_expansion::arena_bytes();
    let pool = song_limit_expansion::record_pool();
    let extended = song_limit_expansion::property_extended();
    let chunk = song_limit_expansion::arc_chunk_bytes();
    let textures = song_limit_expansion::texture_slots();
    if let Ok(mut slot) = STARTUP_SNAPSHOT.lock() {
        *slot = Some(snap);
    }
    Some(format!(
        "stepmania:{:016x}:{:?}:{:?}:{}:{}:{}:{}:{}:{}",
        model::index_digest(&entries),
        reg.and_then(|r| r.series),
        reg.and_then(|r| r.folder_bit),
        entries.len(),
        arena,
        pool,
        extended,
        chunk,
        textures
    ))
}

/// The `data/gamedata/musicdb.xml` member transform.
fn musicdb_transform(xml: &[u8]) -> Option<Vec<u8>> {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| transform_inner(xml)));
    match result {
        Ok(r) => r,
        Err(_) => {
            log_warn!("StepMania: musicdb transform panicked -- musicdb left unchanged");
            None
        }
    }
}

fn transform_inner(xml: &[u8]) -> Option<Vec<u8>> {
    TRANSFORM_RAN.store(true, Ordering::Release);
    let reg = ensure_registration()?;
    let Some(series) = reg.series else {
        return None;
    };
    let snap = STARTUP_SNAPSHOT
        .lock()
        .ok()
        .and_then(|mut slot| slot.take())
        .unwrap_or_else(|| injectable(index::snapshot()));
    if snap.songs.is_empty() {
        return None;
    }
    let entries = snap.entries();
    let arena = song_limit_expansion::arena_bytes();
    let report = musicdb::transform_report(
        xml,
        &entries,
        series,
        reg.folder_bit,
        Some(ceiling::text_budget(arena)),
    );
    for (i, reason) in &report.dropped {
        if *reason == musicdb::DropReason::ArenaBudget {
            continue; // summarised below
        }
        if let Some(e) = entries.get(*i) {
            log_warn!(
                "StepMania: '{}/{}' ({} / {}) not injected: {}",
                e.pack,
                e.folder,
                e.basename,
                e.mcode,
                reason
            );
        }
    }
    let over_budget = report.over_budget();
    if over_budget > 0 {
        log_warn!(
            "StepMania: {} song(s) not injected -- musicdb.xml ({} KiB) would exceed the {} MiB property arena's text budget ({} KiB)",
            over_budget,
            xml.len() / 1024,
            arena >> 20,
            ceiling::text_budget(arena) / 1024
        );
    }
    if report.no_close_tag {
        log_warn!("StepMania: musicdb.xml has no </mdb> -- nothing injected");
        return None;
    }
    let injected: Vec<Arc<model::SongEntry>> = report
        .injected
        .iter()
        .filter_map(|&i| snap.songs.get(i).cloned())
        .collect();
    log_info!(
        "StepMania: injecting {} song(s) into musicdb.xml (series {}, property bit {:?}, {} dropped)",
        injected.len(),
        series,
        reg.folder_bit,
        report.dropped.len()
    );
    let _ = INJECTED.set(Arc::new(injected));
    report.xml
}

/// The entries in the game's music DB this boot (empty until the
/// `startup.arc` open).
///
/// When the transform ran, that is exactly what it injected. On a warm boot
/// the LayeredFS arc cache is up to date and the transform does NOT run, but
/// the cache key includes [`startup_fingerprint`] (index digest, series,
/// folder bit, count), so the cached `musicdb.xml` was produced from the same
/// snapshot: the answer is that snapshot's songs — a superset only by songs
/// the transform dropped for a base-musicdb collision or the arena text
/// budget (WARNed when it ran), which have no music-DB entry and are skipped
/// by every consumer that looks them up.
#[must_use]
pub fn injected_entries() -> Vec<Arc<model::SongEntry>> {
    if let Some(v) = INJECTED.get() {
        return v.as_ref().clone();
    }
    if TRANSFORM_RAN.load(Ordering::Acquire) {
        return Vec::new(); // ran and injected nothing (or failed)
    }
    if REGISTRATION.get().and_then(|r| r.series).is_none() {
        return Vec::new();
    }
    STARTUP_SNAPSHOT
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|snap| snap.songs.as_ref().clone()))
        .unwrap_or_default()
}

/// Whether the songs root exists (the mod's `is_active` input).
#[must_use]
pub fn root_exists() -> bool {
    Path::new(SONGS_ROOT).is_dir()
}
