// Many public API items are only used at runtime by hook callbacks or
// are reserved for future mod use. Suppress dead_code for the whole crate.
#![allow(dead_code)]

//! DDR World Hook DLL — Entry point and initialization.
//!
//! Loaded via spice2x's -k flag. Hooks game functions to provide
//! modding capabilities using the game's own native systems.

mod core;
mod mods;
mod services;
mod types;
mod widgets;

use std::sync::Arc;
use windows::Win32::Foundation::*;
use windows::Win32::System::SystemServices::*;

use crate::core::module_resolver;
use crate::core::profiling;
use crate::core::signatures::SignatureStore;
use crate::mods::mod_menu::MOD_MENU_STATE;
use crate::mods::mod_trait::{EarlyContext, Mod, ModContext, ModRegistry};
use crate::services::{
    afp_patcher, asset_loader, avs_layeredfs, bm2d_api, bm2d_package, cull_window, custom_options,
    custom_options_persistence, game_audio, input_manager, judge_hook, movie_policy, movie_sync,
    options_scroll, overlay_draw, render_notes_hook, scene_manager, se_bank_synth,
    series_filter_scroll, song_rate, song_reset, stage_records, texture_resolver, widget_renderer,
};

#[no_mangle]
extern "system" fn DllMain(_dll: HINSTANCE, reason: u32, _reserved: *mut ()) -> BOOL {
    match reason {
        DLL_PROCESS_ATTACH => {
            // Spawn init on a separate thread to avoid loader lock deadlock
            std::thread::spawn(init);
        }
        DLL_PROCESS_DETACH => {
            shutdown();
        }
        _ => {}
    }
    TRUE
}

fn init() {
    // Route panics through our log channel (spice2x doesn't capture stderr, so
    // panics were previously invisible — and a panic across an extern "C" hook
    // boundary aborts the process). Install first so it covers all of init.
    crate::core::logger::install_panic_hook();
    // Also install the SEH crash handler: catches HARD FAULTS (access
    // violations etc.) the panic hook can't see, and writes to a durable,
    // flushed ./ddr_hook_crash.log that survives an immediate abort/fault
    // (spice2x's log.txt is buffered and loses the tail on a hard crash).
    crate::core::crash_handler::install();

    profiling::start();
    log_info!("DDR World Hook DLL starting...");

    // 0. Centralized config store. Must be loaded BEFORE the LayeredFS init
    // (its config section + the shader-synthesis plan) and the early_apply
    // phase so race-critical mods can be config-gated, and before
    // resolve_derived/services/mod-init for the same reason. The store has
    // no dependency on signatures or services — it just reads JSON.
    mods::config::init();
    // Resolve the profiling gate from the parsed config. Until this point,
    // ticks are buffered; after this they emit (or flush if the gate is on)
    // / are dropped (if off).
    let profiling_on = mods::config::get()
        .and_then(|c| c.diagnostics.as_ref())
        .map(|d| d.profiling)
        .unwrap_or(false);
    profiling::set_enabled(profiling_on);
    profiling::tick("config_store");

    // 0b. AVS LayeredFS — file replacement service. RACE-CRITICAL: this
    // must run BEFORE the gamemdx wait + signature scan. The game's
    // `Application::onBoot` synchronously drains `data/arc/startup.arc`,
    // `data/arc/shader.arc` (the ONE shader read of the session — the
    // shader-fixes / mod-menu theme synthesis rides that open) and then
    // `musicdb.xml` (custom series/folder merges) within a few hundred ms
    // of gamemdx loading, on the game's own thread, concurrently with this
    // init thread. With LayeredFS installed after the ~127-signature AOB
    // scan + early_apply + resolve_derived, a fast cabinet (Win7 tester,
    // 2026-09-03: real p4io, LAN server) beat the hook to shader.arc and
    // every animated menu background silently degraded to stock. LayeredFS
    // depends only on libavs exports (loaded long before gamemdx — it
    // waits for it) and `./data_mods`; its hook bodies are safe before any
    // signature resolves.
    let layeredfs_ok = avs_layeredfs::init();
    if layeredfs_ok {
        log_info!("AVS LayeredFS started");
    } else {
        log_warn!("AVS LayeredFS unavailable -- file replacement disabled");
    }
    profiling::tick("avs_layeredfs");

    // 1. Wait for gamemdx.dll
    let game_module = module_resolver::wait_for_game_module();
    profiling::tick("module_load");
    log_info!(
        "Game module found: {} @ {:p} ({} bytes)",
        game_module.name,
        game_module.base,
        game_module.size
    );

    // 2. Signature scan
    let mut signatures = SignatureStore::new(&game_module);
    let result = signatures.resolve_all();
    profiling::tick("resolve_all");
    log_info!(
        "Signature scan complete: {}/{} functions located",
        result.found,
        result.total
    );
    if !result.missing.is_empty() {
        log_warn!("Missing signatures: {}", result.missing.join(", "));
    }

    // 2c. Construct mod instances and run early_apply on each (config-gated).
    //
    // This is the race-critical phase. SongLimitExpansion's early_apply
    // patches the musicdb XML buffer size *before* resolve_derived and
    // service init run, so the patches land before the game's
    // master_loader reaches musicdb_parser at ~750ms. Mods constructed
    // here are moved into the registry later via `reg.register`; their
    // normal `init`/`enable` paths see the early_applied flag and skip
    // duplicated work.
    let mut mods_to_register: Vec<Box<dyn Mod>> = vec![
        Box::new(mods::song_limit_expansion::SongLimitExpansionMod::new()),
        Box::new(mods::fps_unlock::FpsUnlockMod::new()),
        Box::new(mods::fast_bootup::FastBootupMod::new()),
        Box::new(mods::skip_intros::SkipIntrosMod::new()),
        Box::new(mods::timer_freeze::TimerFreezeMod::new()),
        Box::new(mods::premium_free::PremiumFreeMod::new()),
        Box::new(mods::quick_restart_or_fail::QuickRestartOrFailMod::new()),
        Box::new(mods::quick_logout::QuickLogoutMod::new()),
        Box::new(mods::classic_difficulty::ClassicDifficultyMod::new()),
        Box::new(mods::autoplay::AutoplayMod::new()),
        Box::new(mods::announcer_mute::AnnouncerMuteMod::new()),
        Box::new(mods::anytime_speedmod::AnytimeSpeedmodMod::new()),
        Box::new(mods::split_ssq_auto_discovery::SplitSsqAutoDiscoveryMod::new()),
        Box::new(mods::series_expansion::SeriesExpansionMod::new()),
        Box::new(mods::folder_expansion::FolderExpansionMod::new()),
        Box::new(mods::note_types_expansion::NoteTypesExpansionMod::new()),
        Box::new(mods::real_speed_fix::RealSpeedFixMod::new()),
        Box::new(mods::power_user_statistics::PowerUserStatisticsMod::new()),
        Box::new(mods::webui_options::WebUiOptionsMod::new()),
        Box::new(mods::movie_size_customization::MovieSizeCustomizationMod::new()),
        Box::new(mods::center_arrows_single::CenterArrowsSingleMod::new()),
        Box::new(mods::timing_offsets::TimingOffsetsMod::new()),
        Box::new(mods::overlay_element_styling::OverlayElementStylingMod::new()),
        Box::new(mods::playfield_styling::PlayfieldStylingMod::new()),
        Box::new(mods::shader_fixes::ShaderFixesMod::new()),
        Box::new(mods::player_perspective::PlayerPerspectiveMod::new()),
        Box::new(mods::non_native_os_support::NonNativeOsSupportMod::new()),
        Box::new(mods::assist_tick::AssistTickMod::new()),
        Box::new(mods::song_playback_speed::SongPlaybackSpeedMod::new()),
        Box::new(mods::training_mode::TrainingModeMod::new()),
        Box::new(mods::decorative_option_headers::DecorativeOptionHeadersMod::new()),
        Box::new(mods::music_wheel_song_length::MusicWheelSongLengthMod::new()),
        Box::new(mods::per_song_judgement_offsets::PerSongJudgementOffsetsMod::new()),
        Box::new(mods::s_marvelous::SMarvelousMod::new()),
        Box::new(mods::smx_hardware::SmxHardwareMod::new()),
    ];
    let mod_config = mods::config::get()
        .map(|c| c.mods.clone())
        .unwrap_or_default();
    {
        let early_ctx = EarlyContext {
            game_module: &game_module,
            signatures: &signatures,
        };
        for m in &mut mods_to_register {
            let id = m.id().to_string();
            let should_run = mod_config.get(&id).copied().unwrap_or(true);
            if !should_run {
                log_info!(
                    "Mod '{}' early_apply skipped (disabled in config)",
                    m.name()
                );
                continue;
            }
            if !m.early_apply(&early_ctx) {
                log_warn!("Mod '{}' early_apply returned false", m.name());
            }
        }
    }
    profiling::tick("early_apply");

    // 3. Derived addresses
    log_info!("Resolving derived addresses...");
    signatures.resolve_derived();
    profiling::tick("resolve_derived");

    let movie_policy_ok = movie_policy::init(&signatures);
    if !movie_policy_ok {
        log_warn!("Shared movie policy unavailable");
    }
    profiling::tick("movie_policy");

    // 3b. Permanent song-rate clock patch, installed at exact identity;
    // non-100% factors publish only through the wave-bank commit.
    if song_rate::clock_patch::init(&signatures) {
        log_info!("Song-rate identity clock installed");
    } else {
        log_warn!("Song-rate identity clock unavailable -- song rate remains disabled");
    }
    profiling::tick("song_rate_clock");

    // 3c. (AVS LayeredFS now installs at step 0b, ahead of the signature
    // scan — see the race note there.)

    let wave_hooks_ok = song_rate::wavebank_hook::init(&signatures);
    // The streaming IO-callback detour pair must install BEFORE the
    // readiness conjunction is computed (its binding leg reports the
    // installed state — design req 40). Fail-open: unresolved signatures
    // leave it uninstalled, readiness false, and the SONG SPEED row never
    // registers.
    let io_hooks_ok = song_rate::io_callback_hook::init(&signatures);
    let song_rate_identity_ready = song_rate::wavebank_hook::readiness(movie_policy_ok);
    if wave_hooks_ok && io_hooks_ok && song_rate_identity_ready.is_ready() {
        log_info!("Song-rate streaming integration ready");
    } else {
        log_warn!("Song-rate integration incomplete -- non-100% remains unavailable");
    }
    profiling::tick("song_rate_identity");

    // 3d. Debug-gated container dump for the assist-tick synthesis module
    // (offline validation hook — no-op outside layeredfs.developer_mode;
    // spawns its own background thread). After the LayeredFS mod-path scan so
    // the clap asset resolves.
    se_bank_synth::debug_dump_if_dev();

    // 4. Widget renderer
    let renderer_ok = widget_renderer::init(&game_module, &signatures);
    if renderer_ok {
        log_info!("WidgetRenderer started");
    } else {
        log_warn!("WidgetRenderer failed to initialize -- widget mods will not render");
    }
    profiling::tick("widget_renderer");

    // 4b. Texture resolver
    let texture_ok = texture_resolver::init();
    if !texture_ok {
        log_warn!("TextureResolver unavailable -- texture-based image widgets disabled");
    }
    profiling::tick("texture_resolver");

    // 4d. AFP patcher — hook afp_stream_do_create for runtime AFP modification
    let afp_patcher_ok = afp_patcher::init();
    if afp_patcher_ok {
        log_info!("AfpPatcher started");
    } else {
        log_warn!("AfpPatcher unavailable -- AFP template patching disabled");
    }
    profiling::tick("afp_patcher");

    // 4e. BM2D API — resolve libafp named exports for MovieClip manipulation
    let bm2d_ok = bm2d_api::init(&signatures);
    if bm2d_ok {
        log_info!("BM2D_API started");
    } else {
        log_warn!("BM2D_API unavailable -- scroll/clip features disabled");
    }
    profiling::tick("bm2d_api");

    // 4e1. BM2D package service — on-demand load/lookup/release of BM2D data
    // packages via the game's own data manager. Graceful: if a signature is
    // missing the service reports unavailable and background previews stay
    // chrome-only.
    let bm2d_pkg_ok = bm2d_package::init(&signatures);
    if bm2d_pkg_ok {
        log_info!("BM2D_PKG started");
    } else {
        log_warn!("BM2D_PKG unavailable -- on-demand BM2D package loading disabled");
    }
    profiling::tick("bm2d_package");

    // 4e2. Asset loader — FileManager/ResourceManager wrapper for on-demand
    // customizer texture loading (WebUI Options preview overlay). Reuses the
    // file_manager/resource_manager signatures note_types_expansion relies on,
    // plus the file_manager_free release counterpart. Graceful: if a signature
    // is missing the service reports unavailable and the overlay path no-ops.
    let asset_loader_ok = asset_loader::init(&signatures);
    if asset_loader_ok {
        log_info!("AssetLoader started");
    } else {
        log_warn!("AssetLoader unavailable -- on-demand preview overlays disabled");
    }
    profiling::tick("asset_loader");

    // 4f. Series filter scroll — hook panel builder for scroll activation
    if bm2d_ok {
        let scroll_ok = series_filter_scroll::init(&signatures);
        if scroll_ok {
            log_info!("SeriesFilterScroll started");
        } else {
            log_warn!("SeriesFilterScroll unavailable -- filter scroll disabled");
        }
    }
    profiling::tick("series_filter_scroll");

    // 4g. Custom player options framework — registration + UI injection.
    // Initialized after afp_patcher so the AFP patches (landing in a later
    // task) can register. options_scroll follows because its cross-service
    // reads target custom_options; custom_options_persistence follows because
    // its save/load bridges read from custom_options' per-player cache.
    let custom_options_ok = custom_options::init(&signatures);
    if custom_options_ok {
        log_info!("CustomOptions started");
    } else {
        log_warn!("CustomOptions unavailable -- custom option registration disabled");
    }
    profiling::tick("custom_options");

    // 4h. Options scroll — per-(side, page) scroll driver for overflowed tabs.
    let options_scroll_ok = options_scroll::init(&signatures);
    if options_scroll_ok {
        log_info!("OptionsScroll started");
    } else {
        log_warn!("OptionsScroll unavailable -- options menu will not auto-scroll");
    }
    profiling::tick("options_scroll");

    // 4h2. Stage records — shared fail-closed decode of the per-stage
    // play-record layout. Must precede custom_options_persistence (whose
    // logout-save sanitiser consumes it); also consumed by premium_free and
    // quick_logout at mod init/enable time (step 7+, always after this).
    let stage_records_ok = stage_records::init(&signatures, &game_module);
    if !stage_records_ok {
        log_warn!(
            "StageRecords unavailable -- premium_free fails closed, tainted logout saves will be suppressed"
        );
    } else {
        // Full-sanitization readiness latch (Song Playback Speed).
        crate::services::score_guard::mark_stage_records_ready();
    }
    profiling::tick("stage_records");

    // 4i. Custom options persistence — ess.dll save/load bridge.
    let cop_ok = custom_options_persistence::init(&signatures);
    if cop_ok {
        log_info!("CustomOptionsPersistence started");
    } else {
        log_warn!(
            "CustomOptionsPersistence unavailable -- custom option values will not round-trip"
        );
    }
    profiling::tick("custom_options_persistence");

    // 5. Scene manager
    let scene_ok = scene_manager::init(&signatures);
    if !scene_ok {
        log_warn!("SceneManager unavailable -- mods will not receive scene change events");
    } else {
        // Full-sanitization readiness latch (Song Playback Speed).
        crate::services::score_guard::mark_scene_manager_ready();
    }
    profiling::tick("scene_manager");

    // 5a. Background-movie sync engine: captures the live DShowPlayer from
    // the shared BuildGraph hook (drops it on every scene change) and, in
    // later steps, drives IMediaSeeking seek/rate. Fail-open: without the
    // movie hook or scene manager it simply never initializes.
    if !movie_sync::init(movie_policy_ok, scene_ok) {
        log_warn!("Movie sync engine unavailable");
    }
    profiling::tick("movie_sync");

    // 5b. Song-rate lifecycle runtime: the permanent scene callback and the
    // boot-only fault selector. Registration is unconditional — lifecycle
    // observation is permanent by design — but arming additionally requires
    // the boot-latched identity readiness and the live full-sanitization
    // conjunction.
    {
        let dev_mode = mods::config::get()
            .and_then(|c| c.layeredfs.as_ref())
            .map(|l| l.developer_mode)
            .unwrap_or(false);
        // The Step 4 pre-generated diagnostic block is retired: rates are
        // selected through the SONG SPEED option. Presence is reported once
        // (parse-but-ignore), never validated or used.
        let retired_diagnostic_present = mods::config::get()
            .and_then(|c| c.song_playback_speed.as_ref())
            .map(|c| c.diagnostic.is_some())
            .unwrap_or(false);
        // Boot-only fault selector (design Error Handling): developer mode
        // only; unknown values warn and select nothing.
        let fault = if dev_mode {
            std::env::var("DDR_SONG_RATE_FAULT").ok().and_then(|value| {
                let parsed = song_rate::transaction::FaultSelector::parse(&value);
                if parsed.is_none() {
                    log_warn!("song_rate: unknown DDR_SONG_RATE_FAULT value '{}'", value);
                }
                parsed
            })
        } else {
            None
        };
        song_rate::runtime::init(
            song_rate_identity_ready.is_ready(),
            retired_diagnostic_present,
            fault,
        );
    }
    profiling::tick("song_rate_runtime");

    // 6. Input manager
    let input_ok = input_manager::init();
    if !input_ok {
        log_warn!("InputManager unavailable -- mods will not receive input events");
    }
    profiling::tick("input_manager");

    // 6b. Judge hook — shared dispatcher for GamePlayActor::judgeNotes.
    // Must be installed before any mod that needs to intercept the judge
    // (Autoplay, and future mods like NoteTypesExpansion) is enabled.
    let judge_hook_ok = judge_hook::init(&signatures);
    if judge_hook_ok {
        log_info!("JudgeHook started");
    } else {
        log_warn!("JudgeHook unavailable -- mods that depend on judge dispatch will no-op");
    }
    profiling::tick("judge_hook");

    // 6a1. Analyze dispatcher — shared detour on IStepReader::Analyze. Owns
    // the single detour that NoteTypesExpansion (mine injection) and the
    // ultrafast-boot capture both subscribe to. Must init before those mods
    // enable so their register_post calls have a live dispatcher.
    if crate::services::analyze_hook::init(&signatures) {
        log_info!("AnalyzeHook started");
    } else {
        log_warn!(
            "AnalyzeHook unavailable -- Analyze subscribers (NTX inject / boot capture) inert"
        );
    }
    profiling::tick("analyze_hook");

    // 6b1. Game audio — mod-owned XACT bank registration + cue playback through
    // the game's own audio engine. Resolves addresses only (it must not call a
    // game function this early), so its only real dependency is step 3's
    // derived addresses. It sits *after* judge_hook because Step 2's temporary
    // demo trigger subscribes to that dispatcher; once Step 3 removes the
    // scaffolding the ordering stops mattering.
    let game_audio_ok = game_audio::init(&signatures);
    if game_audio_ok {
        log_info!("GameAudio started");
    } else {
        log_warn!("GameAudio unavailable -- mods that play their own sounds will no-op");
    }
    profiling::tick("game_audio");

    // 6b2. Song reset — in-place rewind of the live gameplay run (the
    // quick-restart instant path / Training Mode foundation). Resolves
    // addresses and registers its per-song gauge-snapshot scene callback,
    // so it must follow scene_manager (step 5). Fail-open: unavailable
    // just means quick restart keeps its fresh-DPS fast path.
    let song_reset_ok = song_reset::init(&signatures);
    if song_reset_ok {
        log_info!("SongReset started");
    } else {
        log_warn!("SongReset unavailable -- quick restart falls back to the scene-jump path");
    }
    profiling::tick("song_reset");

    // 6c. Render-notes hook — shared dispatcher for ArrowRenderer::render_notes.
    // Must be installed before mod registration: note_types_expansion
    // registers its mine-pass callback from init(), and player_perspective
    // subscribes at enable.
    let render_notes_ok = render_notes_hook::init(&signatures);
    if render_notes_ok {
        log_info!("RenderNotesHook started");
    } else {
        log_warn!(
            "RenderNotesHook unavailable -- mods that depend on the note-render dispatch will no-op"
        );
    }
    profiling::tick("render_notes_hook");

    // 6c2. Overlay-draw — command-list drawing for the mod-menu overlay
    // (per-scene diagnostics + the dev-gated POC emission; the themed
    // animated backgrounds build on this). Resolves the default-shader
    // global; fail-open.
    overlay_draw::init(&signatures);
    profiling::tick("overlay_draw");

    // 6d. Cull-window service — stashes the two derived cull patch sites +
    // module bounds. No code is patched here; the first contributor mod
    // (playfield_styling or player_perspective) to enable installs lazily.
    cull_window::init(&signatures, game_module.base, game_module.size);
    profiling::tick("cull_window");

    // 7. Register mods. The instances were constructed in step 2c above
    // (so early_apply could run); here we move them into the registry
    // which calls their normal `init` (most mods do their full setup
    // here; SongLimitExpansion's init no-ops because early_apply already
    // ran).
    let ctx = ModContext {
        game_module: &game_module,
        signatures: &signatures,
    };

    let registry = Arc::new(std::sync::Mutex::new(ModRegistry::new()));

    {
        let mut reg = registry.lock().unwrap();
        for m in mods_to_register {
            reg.register(m, &ctx);
        }
    }
    profiling::tick("register_all");

    // 8. Enable mods per the config loaded in step 2b.
    {
        let mut reg = registry.lock().unwrap();
        reg.enable_with_config(&mod_config);
    }
    profiling::tick("enable_with_config");

    // 9. Register mod menu (needs registry callbacks)
    {
        let mut reg = registry.lock().unwrap();
        reg.register(Box::new(mods::mod_menu::ModMenuMod::new()), &ctx);
        reg.enable("mod-menu");
    }
    profiling::tick("mod_menu_register_enable");

    // 10. Flush the custom-options label atlas exactly once now that every
    // mod has finished registering options. `register_label_for` is
    // append-only; this single rebuild captures every label registration
    // from every caller (autoplay, webui-options, future mods). Earlier
    // versions rebuilt on every registration which made webui-options'
    // `enable()` take ~2.5s on the cabinet.
    custom_options::flush_label_atlas();
    profiling::tick("flush_label_atlas");

    // Wire up mod menu callbacks to the registry
    {
        let reg_toggle = registry.clone();
        let reg_entries = registry.clone();

        let mut menu_state = MOD_MENU_STATE.lock().unwrap();
        menu_state.toggle_callback = Some(Arc::new(move |id: &str, enable: bool| {
            let mut reg = reg_toggle.lock().unwrap();
            if enable {
                reg.enable(id);
            } else {
                reg.disable(id);
            }
        }));
        menu_state.entries_callback =
            Some(Arc::new(move || reg_entries.lock().unwrap().get_entries()));
    }

    let enabled_count = registry.lock().unwrap().enabled_count();
    log_info!("DDR World Hook DLL ready. {} mod(s) active.", enabled_count);
    profiling::tick("init_complete");
    profiling::dump_scan_stats();

    // 10. Splash screen + debug overlay (deferred until renderer captures font pointer)
    std::thread::spawn(move || {
        // Wait for widget renderer to be ready
        for _ in 0..300 {
            if widget_renderer::is_available() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        if !widget_renderer::is_available() {
            return;
        }

        // Create all widgets on the game thread
        widget_renderer::run_on_render_thread(move || {
            // Splash screen display
            if let Some(mut title) = widget_renderer::create_text_widget() {
                title.set_text("DDR World Universal Modpack v1.1");
                title.set_position(10.0, 10.0);
                title.set_scale(1.1, 1.1);
                title.set_color(1.0, 1.0, 1.0, 1.0);
                title.show();

                if let Some(mut author) = widget_renderer::create_text_widget() {
                    author.set_text("(by skogaby)");
                    author.set_position(10.0, 55.0);
                    author.set_scale(0.75, 0.75);
                    author.set_color(1.0, 1.0, 1.0, 1.0);
                    author.show();

                    if let Some(mut instructions) = widget_renderer::create_text_widget() {
                        instructions
                            .set_text("Press 0 on either pinpad three times to open the mod menu!");
                        instructions.set_position(10.0, 90.0);
                        instructions.set_scale(0.75, 0.75);
                        instructions.set_color(0.0, 1.0, 0.1, 1.0);
                        instructions.show();

                        // First-boot reboot warning: when any cache-guarded
                        // atlas batch was actually (re)generated this boot
                        // (cold `_cache` on a fresh install, or changed
                        // inputs after an update), the game's already-loaded
                        // texture lists missed the new assets and the option
                        // textures render blank until the next launch. Show
                        // a large centered red warning so users who skip the
                        // README don't mistake it for broken textures.
                        let mut reboot_warning = None;
                        if services::avs_layeredfs::atlas_cloner::atlases_rebuilt_this_boot() {
                            if let Some(warning) = widget_renderer::create_text_widget() {
                                warning.set_text(
                                    "REBOOT THE GAME AT LEAST ONCE,\nOTHERWISE TEXTURES WILL BE MISSING",
                                );
                                // Center of the 1280x720 screen; Center
                                // alignment centers each line about x, so
                                // the 2-line block is fully centered.
                                warning.set_position(640.0, 320.0);
                                warning.set_alignment(widgets::text_widget::TextAlignment::Center);
                                warning.set_scale(1.5, 1.5);
                                warning.set_color(1.0, 0.1, 0.1, 1.0);
                                warning.set_outline(0.0, 0.0, 0.0, 1.0, 2);
                                warning.show();
                                reboot_warning = Some(warning);
                            }
                        }

                        // Dismiss after 10 seconds (from a background thread)
                        std::thread::spawn(move || {
                            std::thread::sleep(std::time::Duration::from_secs(10));
                            title.destroy();
                            author.destroy();
                            instructions.destroy();
                            if let Some(mut warning) = reboot_warning {
                                warning.destroy();
                            }
                            log_info!("Splash screen dismissed");
                        });
                    }
                }
            }
        });
    });
}

fn shutdown() {
    log_info!("DDR World Hook DLL shutting down");
}
