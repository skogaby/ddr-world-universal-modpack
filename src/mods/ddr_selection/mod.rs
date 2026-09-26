//! DDR SELECTION (`ddr-selection`, default ON; inert until a player's row picks an era) — revive DDR A3's
//! legacy gameplay skins per song: skins 1..=5 = 1st-5th, MAX-EXTREME, SuperNOVA, X, 2013-A.
//! World kept most of the plumbing and the legacy `…000N` packages; it removed the `%04d`
//! package-name append and everything that wrote the skin id. This mod puts them back and
//! re-hosts A3's behaviour on World's own actors.
//!
//! ## Decision
//!
//! The per-player option row ([`options`]: OFF / AUTO / five eras, `PersistMode::Local`,
//! `versus_mirror`ed, effective next song) feeds the pure [`trigger`]: the entered side's row
//! governs (P1 in versus; a multiplayer-bot side never governs); AUTO maps the song's raw
//! musicdb `<series>` to A3's DDR SELECTION folder buckets. A refused mode (course, event
//! chain) stays stock. At the song-select → play edge (25 → 26..=28, before the play
//! sequence's `LayoutActor` exists) `arm` writes the skin to `GameWork+0xA8`, which World's
//! own sequence hands to the `LayoutActor` and whose three surviving `CMP [GameWork+0xA8],1`
//! gates hide the song info and option icons on 1st-5th; the first scene outside {26..=30}
//! disarms. The pure [`policy`] table then decides, per
//! World package, whether it turns legacy: only when its World consumer's [`policy::Adapter`]
//! resolved on this boot (World's HUD actors NULL-deref on a missing export), and a probe
//! miss falls back to the unsuffixed World base, never `<base>0000`.
//!
//! ## Surfaces
//!
//! - [`package_helper`] — full replacement of World's `LayoutActor` per-package helper:
//!   stock packages get the original with skin 0, legacy ones A3's `<arc_base>000N` append.
//!   The whole-package swaps (judge, FAST/SLOW, full combo, game over, danger 1–2,
//!   pacemaker) need nothing else.
//! - [`panel`] + [`panel_logic`] — A3's stage panel with era cut-in and stage call, hosted
//!   in World's ShutterActor stage kind from the song-select confirm (resolved there from the
//!   wheel highlight); [`banner`] + [`banner_logic`] — CLEARED / FAILED / PRAY FOR ALL in the
//!   banner kinds, on the same `ShutterActor::onUpdate` detour.
//! - [`intro`] + [`intro_logic`] — READY! / HERE WE GO!! from `dance_message000N`, World's
//!   READY? panel dismissed.
//! - [`markers`] + [`marker_keys`] — A3's element positions via `services::hud_layout_hooks`;
//!   [`stage_frame`] — the stage frame names.
//! - [`gauge`] + [`gauge_math`], [`combo`] + [`combo_math`] (over `services::combo_hooks`),
//!   [`score`] + [`score_math`], [`song_info`] + [`song_info_logic`] — life gauge, combo,
//!   score / difficulty, skin 2's band and skins 3–5's A3 panel.
//! - [`option_icons`] + [`option_icons_logic`] — A3's option-icon sprites (skins 2–5);
//!   [`options_force`] + [`options_force_logic`] — 1st-5th's forced classic options.
//! - [`movie_sel`] + [`sel_movie_logic`] — A3's `_sel` background movies.
//! - [`sound`] — the mod-owned `dsel` era bank, the AFP-clip sound route, `code_se` flips
//!   silencing World's doubled code sounds, and A3's announcer / crowd over
//!   `services::call_voice_hooks`. [`settings`] — the Era Cut-In row.
//!
//! ## Invariants
//!
//! - **Scoped patches.** Every code / data patch is live only as long as it applies: one call
//!   (combo / score init, the `SceneManageActor::onInitialize` movie byte), one update
//!   (ShutterActor kind rows), while the current `LayoutActor`'s record is legacy (stage
//!   frame, gauge export, song info — applied by the helper before it registers the package,
//!   which stays stock if a patch fails), or while a `code_se` site's per-song gate holds.
//!   Longer-lived patches are restored on the next stock request, at disarm and at disable.
//! - **Layer before package.** Mod-created layers on a `LayoutActor` package are destroyed
//!   in the GAMEPLAY-exit scene callback, before the sequence tears the actor down; mod-held
//!   package tickets are released only after World's layer on them is gone.
//! - **Threads.** Everything engine-facing runs on the game thread, except the AFP sound
//!   route (inside libafp's display pass: lock-, allocation- and log-free), the bank build
//!   (background thread, owned buffers) and [`leaked_forced_options`] (atomics only).
//! - **Cross-mod seams.** [`legacy_package`] + [`armed_skin`] tell S-Marvelous which package
//!   (World's or skin N's) a template stream or re-drive belongs to — it dresses the legacy
//!   skins it has art for and stands down on the rest; `enable` calls
//!   `s_marvelous::on_ddr_selection_enabled` so it stages that art, and the A3 combo write
//!   asks `s_marvelous::legacy_combo_smarv` for the skins 4–5 S-Marvelous sheet;
//!   [`leaked_forced_options`] lets the save
//!   trampoline (`custom_options_persistence`) rewrite the `/data/option` nodes if a save is
//!   ever built while 1st-5th's options are forced. No surface taints through `score_guard`.
//!
//! ## Degradation
//!
//! Only the core group is in `required_signatures` — the package helper and skin-table read
//! plus the `derive_ddr_selection` sites (`ddr_selection_sites`); without it the mod is
//! unavailable. Every other derivation is optional and all-or-nothing per surface (intro,
//! panel, banners, movie, stage frame, gauge, combo, score, song info / panel, option icons,
//! option forcing, `code_se`, call voice, the AFP sound callback, the AUTO series lookup): a
//! missing one leaves that surface World's with one WARN — where the surface is an adapter,
//! the packages it guards stay stock. A disarmed or stock package always runs World's code.
//!
//! ## Assets, config, developer force
//!
//! A3-only data is never committed: `scripts/ddr_selection/import_a3_assets.{sh,bat}` copies
//! it from an operator's A3 install into `data_mods/ddr_selection_a3/` per
//! `a3_assets.manifest` (`always` entries such as World's blanked `dance_combo0005_v0.arc`,
//! `missing` entries only when the World install lacks them). Config section `ddr_selection`
//! (`era_cutin`, default ON) is written whole by [`settings`]. `DDR_SELECTION_FORCE=<1..5>` with
//! `layeredfs.developer_mode` forces that skin on every song, overriding the rows (mode
//! refusals still apply).
//!
//! RE: `docs/ddr_selection_research.md` and the research notes under
//! `.agents/planning/2026-09-22-ddr-selection/`. Host tests (every pure `*_logic` / `*_math`
//! module, [`policy`], [`trigger`], the sound manifest, bank builder and rules):
//! `scripts/validate_ddr_selection.sh`.

mod banner;
pub mod banner_logic;
mod combo;
pub mod combo_math;
mod gauge;
pub mod gauge_math;
mod intro;
pub mod intro_logic;
pub mod marker_keys;
mod markers;
mod movie_sel;
mod option_icons;
mod option_icons_logic;
mod options;
mod options_force;
mod options_force_logic;
mod package_helper;
mod panel;
pub mod panel_logic;
pub mod policy;
mod score;
pub mod score_math;
pub mod sel_movie_logic;
mod settings;
mod song_info;
mod song_info_logic;
mod sound;
mod stage_frame;
pub mod trigger;

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU8, Ordering};
use std::sync::OnceLock;

use crate::core::memory;
use crate::core::signatures::DdrSelectionSites;
use crate::mods::mod_trait::{Mod, ModContext};
use crate::services::{scene_manager, selectmusic_highlight, stage_records};
use crate::types::scenes::scene;
use crate::{log_info, log_warn};

use policy::AdapterSet;

/// Skin armed for the current song window (0 = disarmed).
static ARMED_SKIN: AtomicU8 = AtomicU8::new(0);
/// The governing side's committed mcode of the armed song (-1 = none).
static ARMED_MCODE: AtomicI32 = AtomicI32::new(-1);
/// Bases registered legacy during the current window (`policy::package_index`
/// bits). Read by other mods (S-Marvelous) through [`legacy_package`].
static LEGACY_MASK: AtomicU32 = AtomicU32::new(0);
/// Operator enable (mod toggle).
static ENABLED: AtomicBool = AtomicBool::new(false);
/// Helper detour installed.
static CAPABLE: AtomicBool = AtomicBool::new(false);
/// Developer-knob skin (0 = unset).
static DEV_SKIN: AtomicU8 = AtomicU8::new(0);

static SITES: OnceLock<DdrSelectionSitesSync> = OnceLock::new();
/// `find_music_by_mcode` + the entry's raw-series vslot (AUTO; optional).
static SERIES_LOOKUP: OnceLock<SeriesLookup> = OnceLock::new();

#[derive(Clone, Copy)]
struct SeriesLookup {
    find_music_by_mcode: unsafe extern "C" fn(i32) -> *mut u8,
    vslot: usize,
    module_lo: usize,
    module_hi: usize,
}
unsafe impl Send for SeriesLookup {}
unsafe impl Sync for SeriesLookup {}

/// `PlayerWork` offset of the committed mcode (written by the song-select
/// commit; the wheel highlight lives elsewhere).
const PW_COMMITTED_MCODE: usize = 0x54;

struct DdrSelectionSitesSync(DdrSelectionSites);
unsafe impl Send for DdrSelectionSitesSync {}
unsafe impl Sync for DdrSelectionSitesSync {}

/// `bm2d::SoundCallback::play` (verified at derivation), or null.
static AFP_SOUND_PLAY: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Whether the mod is enabled (operator toggle and the helper detour live).
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Acquire) && CAPABLE.load(Ordering::Acquire)
}

/// The skin armed for the current song window (0 = World UI).
pub fn armed_skin() -> u8 {
    if ENABLED.load(Ordering::Acquire) {
        ARMED_SKIN.load(Ordering::Acquire)
    } else {
        0
    }
}

/// The armed song's mcode (-1 when disarmed) — A3's PRAY FOR ALL rule.
pub fn armed_mcode() -> i32 {
    if armed_skin() == 0 {
        -1
    } else {
        ARMED_MCODE.load(Ordering::Acquire)
    }
}

/// Whether World's `base` package (`"dance_judge"`, `"dance_fullcombo"`, …)
/// is the legacy skin package for the current song. Other mods that edit a
/// World template (S-Marvelous) stand down when this is true.
pub fn legacy_package(base: &str) -> bool {
    if armed_skin() == 0 {
        return false;
    }
    match policy::package_index(base) {
        Some(i) => LEGACY_MASK.load(Ordering::Acquire) & (1 << i) != 0,
        None => false,
    }
}

/// `LayoutActor` offset of the per-side record / marker parent (`+ side ·
/// 0x48`) — HUD actors hold a pointer to theirs.
fn records_side_off() -> Option<usize> {
    Some(SITES.get()?.0.records_side_off)
}

/// Adapters available on this boot.
fn adapters() -> AdapterSet {
    let mut set = AdapterSet::none();
    if intro::capable() {
        set = set.with(policy::Adapter::ReadyGo);
    }
    if markers::capable() {
        set = set.with(policy::Adapter::Markers);
    }
    // The legacy stage frame must also land at A3's `stage_frame_usr`
    // marker — World's root places World's frame elsewhere.
    if stage_frame::capable() && markers::capable() {
        set = set.with(policy::Adapter::StageFrame);
    }
    if gauge::capable() {
        set = set.with(policy::Adapter::Gauge);
    }
    if combo::capable() {
        set = set.with(policy::Adapter::Combo);
    }
    if score::capable() {
        set = set.with(policy::Adapter::Score);
    }
    // The band / panel must land at A3's `song_info_usr` marker too.
    if song_info::capable() && markers::capable() {
        set = set.with(policy::Adapter::SongInfo);
    }
    if song_info::panel_capable() && markers::capable() {
        set = set.with(policy::Adapter::SongInfoPanel);
    }
    // The icon row sits at A3's `option` marker, written by the post-pass.
    if option_icons::capable() && markers::capable() {
        set = set.with(policy::Adapter::OptionIcons);
    }
    set
}

/// A package turned legacy (package helper, game thread): record it and
/// silence any World code sound its legacy clip replaces.
fn note_legacy(bit: u32) {
    let before = LEGACY_MASK.fetch_or(bit, Ordering::AcqRel);
    if before & bit == 0 {
        sound::code_se::sync();
    }
}

/// `GameWork` (double hop through the decoded global), null-checked.
fn game_work() -> Option<*mut u8> {
    let sites = &SITES.get()?.0;
    unsafe {
        let p1 = memory::read_ptr(sites.game_work_global);
        if p1.is_null() {
            return None;
        }
        let gw = memory::read_ptr(p1);
        if gw.is_null() {
            None
        } else {
            Some(gw as *mut u8)
        }
    }
}

fn write_skin(skin: u8) -> bool {
    let (Some(sites), Some(gw)) = (SITES.get(), game_work()) else {
        return false;
    };
    let field = unsafe { gw.add(sites.0.gamework_skin_off) };
    if !memory::is_readable(field, 4) {
        return false;
    }
    unsafe { memory::write_i32(field, skin as i32) };
    true
}

fn in_play_entry(scene_id: i32) -> bool {
    (scene::SONG_TO_STAGE_INTERSTITIAL..=scene::GAMEPLAY).contains(&scene_id)
}

fn in_window(scene_id: i32) -> bool {
    (scene::SONG_TO_STAGE_INTERSTITIAL..=scene::RESULTS_DETAIL).contains(&scene_id)
}

/// Why a song stays stock regardless of the trigger (design R5).
fn mode_refusal() -> Option<&'static str> {
    let course_off = stage_records::course_field_offset();
    if course_off == 0 {
        return Some("course field unavailable");
    }
    let Some(gw) = stage_records::game_work().filter(|gw| memory::is_readable(*gw, course_off + 8))
    else {
        return Some("GameWork unavailable");
    };
    if unsafe { memory::read_u64(gw.add(course_off)) } != 0 {
        return Some("course");
    }
    match stage_records::event_mode() {
        Some(1) | Some(2) => Some("event chain"),
        _ => None,
    }
}

/// Raw musicdb `<series>` of `mcode` through the game's own DB (game
/// thread). `None` on any miss.
fn series_of(mcode: i32) -> Option<u8> {
    let l = SERIES_LOOKUP.get()?;
    if mcode <= 0 {
        return None;
    }
    unsafe {
        let entry = (l.find_music_by_mcode)(mcode);
        if entry.is_null() || !memory::is_readable(entry, 8) {
            return None;
        }
        let vtable = memory::read_ptr(entry);
        if !memory::is_readable(vtable.add(l.vslot), 8) {
            return None;
        }
        let f = memory::read_ptr(vtable.add(l.vslot)) as usize;
        if !(l.module_lo..l.module_hi).contains(&f) {
            return None;
        }
        let getter: unsafe extern "C" fn(*mut u8) -> u8 = std::mem::transmute(f);
        Some(getter(entry))
    }
}

fn committed_mcode(side: u8) -> Option<i32> {
    let pw = stage_records::player_work(side as usize)?;
    if !memory::is_readable(pw, PW_COMMITTED_MCODE + 4) {
        return None;
    }
    Some(unsafe { memory::read_i32(pw.add(PW_COMMITTED_MCODE)) })
}

/// One song's DDR SELECTION decision (no side effects).
struct Song {
    r: trigger::Resolution,
    mcode: i32,
    /// A legacy skin the mode policy refuses (course, event chain, …).
    refusal: Option<&'static str>,
}

impl Song {
    /// The skin the song actually plays with (0 = World UI).
    fn skin(&self) -> u8 {
        if self.refusal.is_some() {
            0
        } else {
            self.r.skin
        }
    }

    fn source(&self) -> String {
        match self.r.source {
            trigger::Source::DevKnob => "dev-knob".to_string(),
            trigger::Source::Explicit => "option".to_string(),
            trigger::Source::Auto(series) => format!("AUTO series {}", series),
            trigger::Source::None => "?".to_string(),
        }
    }
}

/// Where [`resolve_song`] reads the song from.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SongSource {
    /// The song-select stage-panel request: `PlayerWork+0x54` still holds the
    /// PREVIOUS song there (World writes it after requesting the panel —
    /// cabinet 2026-09-25: AUTO hosted the last song's era), so the wheel's
    /// highlighted song — the one just confirmed — is read instead.
    SongSelect,
    /// The play edge (25 → 26): the committed mcode is current.
    PlayEdge,
}

/// Resolve the song from the rows / AUTO / dev knob and the mode policy.
/// Game thread; at the song-select stage-panel request and at the play edge.
fn resolve_song(at: SongSource) -> Song {
    let entered = [
        stage_records::side_entered(0),
        stage_records::side_entered(1),
    ];
    let bot_side = [0usize, 1]
        .into_iter()
        .find(|&s| crate::mods::multiplayer_bot::is_bot_side(s))
        .map(|s| s as u8);
    let governing = trigger::governing_side(entered, bot_side);
    let highlighted = match at {
        SongSource::SongSelect => selectmusic_highlight::highlighted_mcode().filter(|m| *m > 0),
        SongSource::PlayEdge => None,
    };
    if at == SongSource::SongSelect && highlighted.is_none() {
        log_warn!(
            "DDR SELECTION: song-select wheel highlight unreadable -- the stage panel uses PlayerWork's mcode (may be the previous song; AUTO can pick the wrong era)"
        );
    }
    let mcode = highlighted
        .or_else(|| governing.and_then(committed_mcode))
        .unwrap_or(-1);
    let row = [options::row_value(0), options::row_value(1)];
    let needs_series = governing.is_some_and(|g| row[g as usize] == trigger::ROW_AUTO);
    let series = if needs_series { series_of(mcode) } else { None };
    let inputs = trigger::Inputs {
        entered,
        bot_side,
        row,
        series,
        dev_skin: DEV_SKIN.load(Ordering::Acquire),
    };
    let r = trigger::resolve(&inputs);
    let refusal = if r.skin != 0 { mode_refusal() } else { None };
    Song { r, mcode, refusal }
}

/// World is about to load the stage panel during song select (the
/// ShutterActor update, pre-original, game thread — `panel.rs`): the skin to
/// host A3's panel with, if the song plays a legacy skin. The play edge
/// ([`arm`]) re-resolves and reconciles.
fn stage_panel_request_skin() -> Option<u8> {
    if !ENABLED.load(Ordering::Acquire) || !CAPABLE.load(Ordering::Acquire) {
        return None;
    }
    let song = resolve_song(SongSource::SongSelect);
    let skin = song.skin();
    if skin != 0 {
        log_info!(
            "DDR SELECTION: stage panel requested at song select -- skin {} ({}) source={} governing=P{} mcode={}",
            skin,
            policy::skin_name(skin).unwrap_or("?"),
            song.source(),
            song.r.governing.map(|g| g + 1).unwrap_or(0),
            song.mcode
        );
    }
    (skin != 0).then_some(skin)
}

/// The play edge resolved stock, but the song-select request hosted A3's
/// panel for `hosted`.
fn reconcile_stock(hosted: u8, why: &str) {
    if panel::drop_if_unadopted() {
        log_warn!(
            "DDR SELECTION: the stage panel was hosted for skin {} at song select, but the song resolves stock at the play edge ({}) -- dropped before World loaded it",
            hosted,
            why
        );
    } else {
        log_warn!(
            "DDR SELECTION: the stage panel was hosted for skin {} at song select, but the song resolves stock at the play edge ({}) -- World already loads A3's panel, it stays for this song",
            hosted,
            why
        );
    }
}

fn arm() {
    if !ENABLED.load(Ordering::Acquire) || !CAPABLE.load(Ordering::Acquire) {
        return;
    }
    let song = resolve_song(SongSource::PlayEdge);
    let hosted = panel::hosted_skin();
    let r = &song.r;
    if r.skin == 0 {
        if r.stock_reason == Some(trigger::StockReason::SeriesUnavailable) {
            log_warn!(
                "DDR SELECTION: AUTO could not read the series of mcode {} -- stock for this song",
                song.mcode
            );
        } else if r.stock_reason == Some(trigger::StockReason::AutoStock) {
            log_info!(
                "DDR SELECTION: AUTO -- mcode {} is a modern-era song, stock UI",
                song.mcode
            );
        }
        if let Some(h) = hosted {
            reconcile_stock(h, "no legacy skin");
        }
        return;
    }
    if let Some(why) = song.refusal {
        log_info!("DDR SELECTION: stock for this song ({})", why);
        if let Some(h) = hosted {
            reconcile_stock(h, why);
        }
        return;
    }
    if !write_skin(r.skin) {
        log_warn!("DDR SELECTION: GameWork skin field not writable -- song stays stock");
        if let Some(h) = hosted {
            reconcile_stock(h, "GameWork skin field not writable");
        }
        return;
    }
    LEGACY_MASK.store(0, Ordering::Release);
    package_helper::reset_arm_logs();
    combo::reset_logs();
    score::reset_logs();
    ARMED_MCODE.store(song.mcode, Ordering::Release);
    ARMED_SKIN.store(r.skin, Ordering::Release);
    // Usually already hosted by the song-select request; otherwise before
    // `SelectMusicTerminateSequence` requests the stage panel (scene 26).
    panel::arm(r.skin, "the play edge");
    sound::code_se::sync();
    log_info!(
        "DDR SELECTION: armed skin {} ({}) source={} governing=P{} mcode={}{}",
        r.skin,
        policy::skin_name(r.skin).unwrap_or("?"),
        song.source(),
        r.governing.map(|g| g + 1).unwrap_or(0),
        song.mcode,
        match hosted {
            Some(h) if h == r.skin => " (stage panel hosted at the song-select request)",
            _ => "",
        }
    );
}

fn disarm(reason: &str) {
    let skin = ARMED_SKIN.swap(0, Ordering::AcqRel);
    ARMED_MCODE.store(-1, Ordering::Release);
    // (A legacy end banner still on screen is not disarmed: `banner.rs`
    // follows World's ShutterActor until World releases it.)
    panel::disarm();
    // World's stage-frame names back (no-op when nothing is patched) and no
    // pending marker post-pass.
    stage_frame::restore();
    gauge::restore();
    song_info::restore();
    option_icons::release_all("disarm");
    options_force::restore_all("disarm");
    markers::reset();
    // Restore World's code sounds (no-op when nothing was silenced).
    sound::code_se::sync();
    if skin == 0 {
        return;
    }
    if !write_skin(0) {
        log_warn!("DDR SELECTION: could not clear the GameWork skin field");
    }
    let mask = LEGACY_MASK.swap(0, Ordering::AcqRel);
    sound::afp_route::drain_counters();
    log_info!(
        "DDR SELECTION: disarmed skin {} ({}; {} legacy package(s) this window)",
        skin,
        reason,
        mask.count_ones()
    );
}

fn on_scene_change(prev: i32, next: i32) {
    panel::on_scene_change(next);
    banner::on_scene_change(next);
    // Legacy intro clips go first: leaving GAMEPLAY must destroy them while
    // the owning LayoutActor still lives.
    intro::on_scene_change(next);
    option_icons::on_scene_change(next);
    // Register the era bank as soon as it is built — before the first legacy
    // clip can play (the song-select → stage shutter already fires sounds).
    sound::bank::try_register();
    if ARMED_SKIN.load(Ordering::Acquire) != 0 {
        sound::afp_route::drain_counters();
        // Catches a bank that registered after this window's packages did.
        sound::code_se::sync();
    }
    // Song select can hand off to the interstitial, the stage loader or
    // gameplay directly (the multiplayer bot's FLIP_TARGETS); every one of
    // them precedes the play sequence's `LayoutActor`.
    if prev == scene::SONG_SELECT && in_play_entry(next) {
        arm();
    } else if (ARMED_SKIN.load(Ordering::Acquire) != 0 || panel::hosted()) && !in_window(next) {
        // `panel::hosted()` alone: a song-select request hosted A3's panel
        // but the song never reached the play edge (or resolved stock there).
        disarm("left the song window");
    }
    // After this edge's arm / disarm: 1st-5th's forced options cover
    // {26, 27, 28} and are restored on the first scene outside.
    options_force::sync(next, armed_skin());
}

/// The player's own values of the eleven forced option fields when a save
/// is built while `side` is still forced (unreachable by design; the save
/// trampoline then rewrites these `/data/option` s32 nodes). Any thread.
pub fn leaked_forced_options(
    side: usize,
) -> Option<[(&'static [u8], i32); options_force_logic::COUNT]> {
    let values = options_force::leaked(side)?;
    let mut out = [(&b"\0"[..], 0); options_force_logic::COUNT];
    for ((slot, f), v) in out
        .iter_mut()
        .zip(options_force_logic::FIELDS.iter())
        .zip(values.iter())
    {
        *slot = (f.node, *v);
    }
    Some(out)
}

/// Read the developer knob once (enable time).
fn configure_dev_knob() {
    let dev_mode = crate::mods::config::get()
        .and_then(|c| c.layeredfs.as_ref())
        .map(|l| l.developer_mode)
        .unwrap_or(false);
    let raw = std::env::var("DDR_SELECTION_FORCE").ok();
    let skin = raw
        .as_deref()
        .and_then(|v| v.trim().parse::<u8>().ok())
        .filter(|s| (1..=policy::SKIN_MAX).contains(s))
        .unwrap_or(0);
    let armed = if dev_mode { skin } else { 0 };
    if raw.is_some() && armed == 0 {
        log_warn!(
            "DDR SELECTION: DDR_SELECTION_FORCE={:?} ignored (needs layeredfs.developer_mode and a value 1..5)",
            raw.unwrap_or_default()
        );
    }
    DEV_SKIN.store(armed, Ordering::Release);
    if armed != 0 {
        log_info!(
            "DDR SELECTION: developer knob -- every normal song plays skin {} ({}), overriding the option rows",
            armed,
            policy::skin_name(armed).unwrap_or("?")
        );
    }
}

pub struct DdrSelectionMod {
    scene_cb: Option<usize>,
}

impl DdrSelectionMod {
    pub fn new() -> Self {
        Self { scene_cb: None }
    }
}

impl Mod for DdrSelectionMod {
    fn id(&self) -> &str {
        "ddr-selection"
    }

    fn name(&self) -> &str {
        "DDR SELECTION"
    }

    fn description(&self) -> &str {
        "Play songs with the gameplay UI of the DDR era they came from (A3's DDR SELECTION)"
    }

    fn required_signatures(&self) -> &[&str] {
        &[
            "layout_package_helper",
            "dps_skin_table_read",
            "ddr_sel_pkg_probe",
            "ddr_sel_record_insert",
            "ddr_sel_load_list_push",
            "ddr_sel_bm2d_dir",
            "ddr_sel_game_work_global",
            "ddr_sel_records_shared_off",
            "ddr_sel_records_side_off",
            "ddr_sel_load_list_off",
            "gamework_skin_off",
        ]
    }

    fn init(&mut self, ctx: &ModContext) -> bool {
        let Some(sites) = ctx.signatures.ddr_selection_sites() else {
            log_warn!("DDR SELECTION: derivation group incomplete -- mod unavailable");
            return false;
        };
        let _ = SITES.set(DdrSelectionSitesSync(sites));
        match (
            ctx.signatures.get_address("find_music_by_mcode"),
            ctx.signatures.music_series_vslot(),
        ) {
            (Some(f), Some(vslot)) => {
                let lo = ctx.game_module.base as usize;
                let _ = SERIES_LOOKUP.set(SeriesLookup {
                    find_music_by_mcode: unsafe { std::mem::transmute::<*const u8, _>(f) },
                    vslot,
                    module_lo: lo,
                    module_hi: lo + ctx.game_module.size,
                });
            }
            _ => log_warn!(
                "DDR SELECTION: music-DB series lookup unavailable -- AUTO resolves to stock (explicit eras still work)"
            ),
        }
        sound::code_se::init(ctx.signatures);
        intro::init(ctx.signatures);
        markers::init(sites.records_shared_off, sites.records_side_off);
        stage_frame::init(ctx.signatures);
        gauge::init(ctx.signatures);
        combo::init(ctx.signatures);
        score::init(ctx.signatures);
        song_info::init(ctx.signatures);
        option_icons::init(ctx.signatures);
        // (WARNs itself; 1st-5th songs then keep the player's options.)
        let _ = options_force::init(ctx.signatures);
        panel::init(ctx.signatures);
        movie_sel::init(
            ctx.signatures,
            ctx.game_module.base as usize,
            ctx.game_module.size,
        );
        match ctx.signatures.get_address("afp_sound_callback_play") {
            Some(p) => AFP_SOUND_PLAY.store(p as usize, Ordering::Release),
            None => log_warn!(
                "DDR SELECTION: AFP sound callback unresolved -- legacy clips' own sounds stay silent"
            ),
        }
        true
    }

    fn enable(&mut self) {
        if !CAPABLE.load(Ordering::Acquire) {
            let Some(sites) = SITES.get() else {
                return;
            };
            if !package_helper::install(&sites.0) {
                return;
            }
            CAPABLE.store(true, Ordering::Release);
            log_info!("DDR SELECTION: LayoutActor package-helper detour installed");
        }
        configure_dev_knob();
        settings::load();
        settings::register_rows();
        options::register();
        let play = AFP_SOUND_PLAY.load(Ordering::Acquire);
        if play != 0 && crate::services::game_audio::is_available() {
            if sound::afp_route::install(play as *const u8) {
                sound::bank::start_build();
                // A3's announcer / crowd plays from the era bank.
                sound::call_voice::init();
            }
        } else if play != 0 {
            log_warn!("DDR SELECTION: game audio service unavailable -- no era bank");
        }
        intro::start();
        markers::start();
        gauge::start();
        combo::start();
        score::start();
        option_icons::start();
        panel::start();
        movie_sel::start();
        panel::set_enabled(true);
        panel::on_scene_change(scene_manager::current_scene());
        banner::on_scene_change(scene_manager::current_scene());
        ENABLED.store(true, Ordering::Release);
        // S-Marvelous (enabled before this mod at boot) stages its legacy
        // skins' word / S-MFC splash / combo art now, if it is enabled.
        crate::mods::s_marvelous::on_ddr_selection_enabled();
        if self.scene_cb.is_none() {
            self.scene_cb = Some(scene_manager::on_scene_change(Box::new(|prev, next| {
                on_scene_change(prev, next);
            })));
        }
        log_info!(
            "DDR SELECTION: enabled (era cut-in {})",
            if settings::era_cutin() { "ON" } else { "OFF" }
        );
    }

    fn disable(&mut self) {
        sound::call_voice::shutdown();
        intro::stop();
        movie_sel::stop();
        panel::set_enabled(false);
        settings::remove_rows();
        disarm("mod disabled");
        ENABLED.store(false, Ordering::Release);
        options::set_available(false);
        if let Some(id) = self.scene_cb.take() {
            scene_manager::remove_callback(id);
        }
        log_info!("DDR SELECTION: disabled (package helper passthrough)");
    }

    fn is_active(&self) -> bool {
        CAPABLE.load(Ordering::Acquire)
    }
}
