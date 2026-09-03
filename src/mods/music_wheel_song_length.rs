//! Music Wheel Song Length — shows the highlighted song's length in the
//! song-select header card, styled like the stock BPM readout.
//!
//! The header card renders its BPM digits through the game's own
//! `sequence::SpriteLayer` widget class — a "row of bitmaps by texture
//! name" anchored to a named child MC (`bpm_usr`) of the card's
//! `music_info` AFP clip, laid out fresh every frame by the card's tick
//! (position, scale, and the anchor's live ALPHA — so the digits fade with
//! the card and vanish whenever the anchor is gone). This mod constructs
//! ONE additional SpriteLayer of its own, anchored to the same `bpm_usr`
//! but shifted right via the class's built-in x/y pixel-offset fields
//! (+0xC0/+0xC8), and drives it with `LENGTH`-time glyphs:
//!
//! * the green "LENGTH" caption — a net-new `muca_card_len_t` texture
//!   (the original mod author's label art, used with permission) rendered
//!   as the FIRST glyph of the row, so caption + digits move as one unit;
//! * digits — the STOCK `muca_card_bpm_0..9` textures by name
//!   (pixel-identical to the BPM readout, zero art duplication);
//! * colon — a net-new `muca_card_len_c` texture.
//!
//! Both net-new textures are injected into the `select_music_card_v3` IFS
//! by the atlas cloner at enable (FRESH atlas mode — the 80×24 label
//! doesn't fit a stock donor cell; donor `muca_card_bpm_question` supplies
//! only the encoding conventions).
//!
//! ## Data + staleness policy
//!
//! Primary length source: the song's SSQ chart data, read from disk and
//! parsed on a background worker the FRAME the selection changes (the
//! song code is an inline field of the selection's `music::Info`, readable
//! immediately). Length = the last step's time across all charts, via the
//! tempo chunk (`note_types_expansion::timing::TempoConverter` — the same
//! bit-exact math the gameplay engine uses). This gives ~single-frame
//! latency, matching the original hex-edit mod's chart-derived semantics.
//!
//! Fallback: when the SSQ is missing/unparseable, the audio length from
//! `song_rate::selected_song()` (the wavebank publication emitted when the
//! wheel settles and the preview loads — slower, ~0.5 s) is used, gated on
//! an exact `code_digest` match against the current selection. The display
//! BLANKS the instant the highlighted-song pointer at
//! `selectmusic_model+0x1B0` changes (polled per frame — the same global
//! the game's own card tick polls); a stale result is never shown.
//!
//! ## Lifecycle & threading
//!
//! Everything runs on the game thread from an `input_manager::on_frame`
//! callback (panic-contained by the dispatcher). The SpriteLayer instance
//! is created lazily and lives for the process lifetime; leaving scene 25
//! (or the `music_info` clip dying) blanks it — an empty names list
//! releases every CBitmap back to the game's pool, and the per-frame
//! layout call stops until the card returns. The parent-wrapper pointer is
//! re-validated (active slot + name) every frame before any use; a stale
//! pointer at worst reads static pool memory (the CMovieClip pool is a
//! static array) and resolves no anchor, which hides the glyphs.
//!
//! RE notes: `.agents/planning/2026-08-16-music-wheel-song-length/research.md`.

use std::sync::Mutex;

use crate::core::memory;
use crate::core::msvc::{MsvcString, MsvcVec};
use crate::mods::mod_trait::{Mod, ModContext};
use crate::services::chart_length;
use crate::services::song_rate::binding::song_code_digest;
use crate::services::song_rate::selected_song;
use crate::services::{bm2d_api, input_manager, scene_manager};
use crate::types::scenes::scene;
use crate::{log_info, log_warn};

/// `sequence::SpriteLayer::ctor` — pure field init on a 0xF8 struct.
type SpriteLayerCtorFn = unsafe extern "C" fn(this: *mut u8) -> *mut u8;
/// `sequence::SpriteLayer::SetBitmaps(this, names)` — COPY-assigns the
/// names vector (source stays caller-owned), rebuilds the CBitmap row,
/// ends with a virtual layout call.
type SpriteLayerSetNamesFn =
    unsafe extern "C" fn(this: *mut u8, names: *const MsvcVec<MsvcString>) -> *mut u8;
/// SpriteLayer vtable slot 0 — per-frame layout.
type SpriteLayerLayoutFn = unsafe extern "C" fn(this: *mut u8);
/// Music object vt+0x08 — song code (basename) getter, returns a C string.
type MusicCodeGetterFn = unsafe extern "C" fn(this: *mut u8) -> *const u8;

/// Max glyphs: label + blank spacer + "MMM:SS" = 8; 10 leaves headroom.
const MAX_GLYPHS: usize = 10;

/// SpriteLayer field offsets (research.md §3).
const SL_SIZE: usize = 0xF8;
const SL_PARENT: usize = 0x60;
const SL_ANCHOR_NAME: usize = 0x68; // SSO string: buf +0x68, len +0x78, cap +0x80
const SL_PRIORITY: usize = 0x94;
const SL_OFFSET_X: usize = 0xC0;
const SL_OFFSET_Y: usize = 0xC8;
const SL_FIT_TO_ANCHOR: usize = 0xD8;
const SL_FIXED_SCALE: usize = 0xE0;
const SL_SPACING: usize = 0xE8;

/// The anchor child MC of the `music_info` clip the stock BPM digits use.
const ANCHOR_NAME: &str = "bpm_usr";
/// Named child MCs that identify the header card's `music_info` clip in
/// the BM2D pool (clip NAMES are not stored in pool slots — discovery goes
/// by content; this set is unique to the card layer, which owns the
/// title/artist/source TextLayer anchors alongside `bpm_usr`).
const CLIP_CHILDREN: &[&str] = &["bpm_usr", "music_name_usr", "gimmick_top_usr"];
/// Net-new colon glyph texture (≤15 chars — must stay SSO).
const COLON_TEXTURE: &str = "muca_card_len_c";
/// Net-new "LENGTH" caption texture (92×24 — the 80px label art plus a
/// baked-in 12px right gap so no spacer glyph is needed; the stock blank
/// glyph's name is 19 chars and can't ride the SSO names vector).
const LABEL_TEXTURE: &str = "muca_card_len_t";

/// Placement defaults (config-overridable for cabinet calibration).
/// offset_x positions the LABEL's left edge relative to the bpm_usr anchor
/// (the digits follow the label + a blank spacer within the same row).
const DEFAULT_OFFSET_X: f64 = 280.0;
const DEFAULT_OFFSET_Y: f64 = 0.0;
const DEFAULT_SPACING: f64 = -10.0; // the stock BPM row's value
const DEFAULT_SCALE: f64 = 1.0;

struct Runtime {
    ctor: SpriteLayerCtorFn,
    set_names: SpriteLayerSetNamesFn,
    model: *const u8,
    /// Offset of the highlighted-song shared_ptr inside the model object
    /// (obj at +0, ctrl at +8) — 0x1B0 on 20260324+, 0x190 before; derived.
    highlight_slot: usize,
    /// gamemdx module bounds — used to sanity-check vtable/getter pointers
    /// before the indirect call in [`read_song_code`].
    module_base: usize,
    module_size: usize,
    /// Lazily-created SpriteLayer instance (process lifetime).
    sprite: *mut u8,
    /// Cached layout vfunc, captured from the constructed object's vtable.
    layout: Option<SpriteLayerLayoutFn>,
    /// Cached `music_info` wrapper — re-validated every frame.
    wrapper: *mut u8,
    /// Raw highlighted-song object pointer last seen at model+0x1B0.
    last_selection: usize,
    /// Waiting for a length answer for `last_selection`.
    waiting: bool,
    /// The current selection's song code (read at selection change;
    /// empty = not yet readable).
    pending_code: String,
    /// Non-empty names currently applied.
    displayed: bool,
    /// Backing storage for the names vector handed to the setter.
    names: [MsvcString; MAX_GLYPHS],
    /// Diagnostic: last logged stage marker (latched — logs on change only).
    diag_stage: &'static str,
    // Placement (from config).
    offset_x: f64,
    offset_y: f64,
    spacing: f64,
    scale: f64,
}

// Raw game pointers are valid for the process lifetime; all access happens
// on the game thread (frame callback).
unsafe impl Send for Runtime {}

static RUNTIME: Mutex<Option<Runtime>> = Mutex::new(None);

/// Diagnostic stage tracker — one INFO per stage TRANSITION (latched, so
/// the per-frame paths stay silent while nothing changes).
fn diag(rt: &mut Runtime, stage: &'static str) {
    if rt.diag_stage != stage {
        rt.diag_stage = stage;
        log_info!("MusicWheelSongLength[diag]: {}", stage);
    }
}

pub struct MusicWheelSongLengthMod {
    frame_cb: Option<usize>,
}

impl MusicWheelSongLengthMod {
    pub fn new() -> Self {
        MusicWheelSongLengthMod { frame_cb: None }
    }
}

impl Mod for MusicWheelSongLengthMod {
    fn id(&self) -> &str {
        "music-wheel-song-length"
    }
    fn name(&self) -> &str {
        "Music Wheel Song Length"
    }
    fn description(&self) -> &str {
        "Shows the selected song's length next to BPM on the song wheel"
    }
    fn required_signatures(&self) -> &[&str] {
        &[
            "spritelayer_ctor",
            "spritelayer_set_names",
            "selectmusic_model",
        ]
    }

    fn init(&mut self, ctx: &ModContext) -> bool {
        let ctor = match ctx.signatures.get_address("spritelayer_ctor") {
            Some(a) => unsafe { std::mem::transmute::<*const u8, SpriteLayerCtorFn>(a) },
            None => return false,
        };
        let set_names = match ctx.signatures.get_address("spritelayer_set_names") {
            Some(a) => unsafe { std::mem::transmute::<*const u8, SpriteLayerSetNamesFn>(a) },
            None => return false,
        };
        let model = match ctx.signatures.get_address("selectmusic_model") {
            Some(a) => a,
            None => return false,
        };
        let highlight_slot = match ctx.signatures.selectmusic_highlight_slot() {
            Some(s) => s,
            None => return false,
        };

        let cfg = crate::mods::config::get().and_then(|c| c.music_wheel_song_length.clone());
        let cfg = cfg.unwrap_or_default();

        *RUNTIME.lock().unwrap() = Some(Runtime {
            ctor,
            set_names,
            model,
            highlight_slot,
            module_base: ctx.game_module.base as usize,
            module_size: ctx.game_module.size,
            sprite: std::ptr::null_mut(),
            layout: None,
            wrapper: std::ptr::null_mut(),
            last_selection: 0,
            waiting: false,
            pending_code: String::new(),
            displayed: false,
            names: [const { MsvcString::empty() }; MAX_GLYPHS],
            diag_stage: "",
            offset_x: cfg.offset_x.unwrap_or(DEFAULT_OFFSET_X),
            offset_y: cfg.offset_y.unwrap_or(DEFAULT_OFFSET_Y),
            spacing: cfg.spacing.unwrap_or(DEFAULT_SPACING),
            scale: cfg.scale.unwrap_or(DEFAULT_SCALE),
        });
        true
    }

    fn enable(&mut self) {
        generate_glyph_atlas();
        let id = input_manager::on_frame(std::sync::Arc::new(|| {
            on_frame();
        }));
        self.frame_cb = Some(id);
        log_info!("MusicWheelSongLength: enabled (frame callback {})", id);
    }

    fn disable(&mut self) {
        if let Some(id) = self.frame_cb.take() {
            input_manager::remove_frame_callback(id);
        }
        // Blank if we were showing (releases the CBitmaps back to the pool).
        let mut guard = RUNTIME.lock().unwrap();
        if let Some(rt) = guard.as_mut() {
            if rt.displayed && !rt.sprite.is_null() {
                unsafe { apply_names(rt, &[]) };
                rt.displayed = false;
            }
            rt.waiting = false;
        }
    }
}

/// Inject the mod's net-new textures into the `select_music_card_v3` IFS:
/// the LENGTH caption (`muca_card_len_t`, 80×24) and the colon glyph
/// (`muca_card_len_c`, 24×24). FRESH atlas mode — the label doesn't fit a
/// stock donor cell, so both pack into a new minimal atlas; the donor
/// (`muca_card_bpm_question`) supplies only the imgrect/uvrect encoding
/// conventions. Best-effort: a missing PNG or stock arc logs one WARN and
/// the affected glyph simply renders blank.
fn generate_glyph_atlas() {
    use crate::services::avs_layeredfs::atlas_cloner::{
        generate_cloned_atlases_xml_fresh, load_stock_texturelist, write_merged_texturelist,
        NewTextureSpec,
    };

    const MOD_ROOT: &str = "./data_mods/music_wheel_song_length";
    const CACHE_ROOT: &str = "./data_mods/_cache";
    const IFS_MOD_PATH: &str = "select_music_card_v3_ifs";

    let tex = |name: &str| format!("{}/{}/tex/{}.png", MOD_ROOT, IFS_MOD_PATH, name);
    let colon_png = tex(COLON_TEXTURE);
    let label_png = tex(LABEL_TEXTURE);
    for (what, path) in [("colon", &colon_png), ("label", &label_png)] {
        if !std::path::Path::new(path).exists() {
            log_warn!(
                "MusicWheelSongLength: {} PNG missing at {} — glyph renders blank",
                what,
                path
            );
        }
    }

    let xml = match load_stock_texturelist(
        "data/arc/bm2d/select_music_card_v3.arc",
        "select_music_card_v3.ifs",
    ) {
        Some(x) => x,
        None => {
            log_warn!("MusicWheelSongLength: stock select_music_card texturelist unavailable — glyphs not injected");
            return;
        }
    };

    let specs = [
        NewTextureSpec {
            new_name: LABEL_TEXTURE,
            donor_name: "muca_card_bpm_question",
            png_path: &label_png,
        },
        NewTextureSpec {
            new_name: COLON_TEXTURE,
            donor_name: "muca_card_bpm_question",
            png_path: &colon_png,
        },
    ];
    match generate_cloned_atlases_xml_fresh(&xml, IFS_MOD_PATH, CACHE_ROOT, "mwsl", &specs) {
        Some(fragment) => {
            if write_merged_texturelist(IFS_MOD_PATH, MOD_ROOT, &fragment) {
                log_info!("MusicWheelSongLength: glyph atlas generated (label + colon)");
                // The mod-paths file cache was scanned at layeredfs init —
                // BEFORE this enable ran. If the merged texturelist wasn't
                // on disk at scan time (first boot after deploy), rescan
                // once so the xml merger sees it this boot.
                use crate::services::avs_layeredfs::mod_paths;
                let merged_rel = format!("{}/tex/texturelist.merged.xml", IFS_MOD_PATH);
                if mod_paths::find_first_modfile(&merged_rel).is_none() {
                    log_info!(
                        "MusicWheelSongLength: merged texturelist not in mod-path cache — rescanning"
                    );
                    mod_paths::init_mod_paths();
                }
            }
        }
        None => {
            log_warn!("MusicWheelSongLength: glyph atlas generation failed");
        }
    }
}

/// Per-frame driver (game thread; dispatcher panic-contains us).
fn on_frame() {
    let mut guard = match RUNTIME.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    let rt = match guard.as_mut() {
        Some(r) => r,
        None => return,
    };

    if scene_manager::current_scene() != scene::SONG_SELECT {
        // Leaving the wheel: release bitmaps once, drop cached pointers.
        if rt.displayed && !rt.sprite.is_null() {
            unsafe { apply_names(rt, &[]) };
            rt.displayed = false;
        }
        rt.waiting = false;
        rt.wrapper = std::ptr::null_mut();
        rt.last_selection = 0;
        return;
    }

    unsafe {
        // Lazy SpriteLayer construction (needs bm2d for the pool lookup —
        // without it there's no parent to bind anyway).
        if rt.sprite.is_null() {
            if !bm2d_api::is_available() {
                return;
            }
            let mem = memory::alloc_zeroed(SL_SIZE);
            if mem.is_null() {
                return;
            }
            (rt.ctor)(mem);
            configure_sprite(rt, mem);
            let vtable = memory::read_ptr(mem) as *const usize;
            if vtable.is_null() {
                return; // ctor didn't run as expected — leave sprite null
            }
            let layout_fn = *vtable;
            rt.layout = Some(std::mem::transmute::<usize, SpriteLayerLayoutFn>(layout_fn));
            rt.sprite = mem;
            log_info!("MusicWheelSongLength: SpriteLayer constructed");
        }

        // Parent clip: re-validate the cached wrapper; rescan on miss.
        if !bm2d_api::wrapper_has_children(rt.wrapper, CLIP_CHILDREN) {
            let found = bm2d_api::find_wrapper_by_children(CLIP_CHILDREN);
            match found {
                Some(w) => {
                    rt.wrapper = w;
                    memory::write_ptr(rt.sprite.add(SL_PARENT), w);
                    diag(rt, "music_info wrapper bound");
                }
                None => {
                    // Card layer not up (scene still loading / tearing
                    // down). Blank FIRST (the trailing layout still sees
                    // the old — static, safe — parent), then clear BOTH
                    // the parent field and the cached wrapper. Keeping
                    // them in lockstep is load-bearing: a stale cached
                    // wrapper can re-validate when the pool slot is
                    // recycled, which would skip the parent rewrite and
                    // leave layout running on a NULL parent (the
                    // 2026-08-16 fast-scroll crash).
                    if rt.displayed {
                        apply_names(rt, &[]);
                        rt.displayed = false;
                        rt.waiting = false;
                    }
                    memory::write_ptr(rt.sprite.add(SL_PARENT), std::ptr::null());
                    rt.wrapper = std::ptr::null_mut();
                    diag(rt, "music_info wrapper NOT found");
                    return;
                }
            }
        }

        // Selection poll: the global holds a POINTER to the select-music
        // model (the card tick does `MOV R11,[global]` then `[R11+slot]`);
        // the highlighted-song weak_ptr lives at model+slot (obj) /
        // +slot+8 (ctrl), slot derived per build. All mutation happens on
        // this same thread (the game's card tick polls the identical
        // global), so plain reads without refcount traffic are safe.
        let model_obj = memory::read_ptr(rt.model);
        if model_obj.is_null() {
            diag(rt, "select-music model not constructed");
            return;
        }
        let obj = memory::read_ptr(model_obj.add(rt.highlight_slot)) as *mut u8;
        let ctrl = memory::read_ptr(model_obj.add(rt.highlight_slot + 8));
        let strong = if ctrl.is_null() {
            0
        } else {
            memory::read_u32(ctrl.add(0x08))
        };
        let selection = if obj.is_null() || strong == 0 {
            0usize
        } else {
            obj as usize
        };

        if selection != rt.last_selection {
            rt.last_selection = selection;
            if rt.displayed {
                apply_names(rt, &[]);
                rt.displayed = false;
            }
            rt.waiting = selection != 0;
            rt.pending_code.clear();
            if selection == 0 {
                diag(rt, "selection: none (folder/blank)");
            } else {
                // Read the code NOW (inline field of music::Info) and kick
                // the chart-length service — no waiting for the preview
                // pipeline.
                match read_song_code(rt, selection as *mut u8) {
                    Ok(code) => {
                        chart_length::request(&code);
                        rt.pending_code = code;
                        diag(rt, "selection: song — chart length requested");
                    }
                    Err(why) => diag(rt, why),
                }
            }
        }

        if rt.waiting {
            // Late code read (selection-change frame couldn't read it).
            if rt.pending_code.is_empty() {
                if let Ok(code) = read_song_code(rt, rt.last_selection as *mut u8) {
                    chart_length::request(&code);
                    rt.pending_code = code;
                    diag(rt, "selection: song — chart length requested (late)");
                }
            }

            if !rt.pending_code.is_empty() {
                match chart_length::get(&rt.pending_code) {
                    chart_length::State::Ready(secs) => {
                        let glyphs = build_time_glyphs_secs(secs);
                        log_info!(
                            "MusicWheelSongLength[diag]: chart length code='{}' len={}s glyphs={:?}",
                            rt.pending_code,
                            secs,
                            glyphs
                        );
                        apply_names(rt, &glyphs);
                        rt.displayed = true;
                        rt.waiting = false;
                        rt.diag_stage = "displayed (chart)";
                    }
                    chart_length::State::Failed => {
                        // Fallback: audio length from the wavebank
                        // publication, digest-matched against the current
                        // selection's code.
                        if let Some(info) = selected_song::selected_song() {
                            if song_code_digest(&rt.pending_code) == info.code_digest {
                                let secs = (u64::from(info.audio_len_ms) + 500) / 1000;
                                let glyphs = build_time_glyphs_secs(secs as u32);
                                log_info!(
                                    "MusicWheelSongLength[diag]: publication fallback code='{}' len={}ms glyphs={:?}",
                                    rt.pending_code,
                                    info.audio_len_ms,
                                    glyphs
                                );
                                apply_names(rt, &glyphs);
                                rt.displayed = true;
                                rt.waiting = false;
                                rt.diag_stage = "displayed (publication)";
                            }
                        }
                    }
                    chart_length::State::Unknown => {
                        // Superseded while queued (fast scrolling) —
                        // re-dispatch.
                        chart_length::request(&rt.pending_code);
                    }
                    chart_length::State::Pending => {}
                }
            }
        }

        // Per-frame layout while the card is live — mirrors the game's own
        // card tick so alpha fades and anchor-gone hiding behave natively.
        if rt.displayed {
            if let Some(layout) = rt.layout {
                layout(rt.sprite);
            }
        }
    }
}

/// One-time field setup after the game ctor ran.
unsafe fn configure_sprite(rt: &Runtime, sprite: *mut u8) {
    // Anchor name: in-place SSO string "bpm_usr" (buf/len/cap layout).
    let name = sprite.add(SL_ANCHOR_NAME);
    let bytes = ANCHOR_NAME.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        *name.add(i) = b;
    }
    for i in bytes.len()..16 {
        *name.add(i) = 0;
    }
    memory::write_u64(sprite.add(SL_ANCHOR_NAME + 0x10), bytes.len() as u64);
    memory::write_u64(sprite.add(SL_ANCHOR_NAME + 0x18), 0xF);

    memory::write_i32(sprite.add(SL_PRIORITY), 4); // the stock BPM row's priority
    memory::write_u8(sprite.add(SL_FIT_TO_ANCHOR), 0); // fixed scale, like the card
    write_f64(sprite.add(SL_FIXED_SCALE), rt.scale);
    write_f64(sprite.add(SL_SPACING), rt.spacing);
    write_f64(sprite.add(SL_OFFSET_X), rt.offset_x);
    write_f64(sprite.add(SL_OFFSET_Y), rt.offset_y);
}

unsafe fn write_f64(addr: *mut u8, value: f64) {
    (addr as *mut f64).write_unaligned(value);
}

/// Read the selection's song code. The object at `selectmusic_model+0x1B0`
/// is an outer HOLDER (cabinet minidump 2026-08-16: its first field is NOT
/// a vtable — Ghidra's decompile of the preview-request function hid an
/// extra dereference): fields +0x00/+0x08 are an inner `{music_obj, ctrl}`
/// shared-ptr pair (the pair `FUN_1801a7930` locks), and the code getter is
/// vt+0x08 on the INNER object. Every pointer is null-checked and the
/// vtable + getter are bounds-checked against the game module before the
/// indirect call. `Err` carries a static guard name for latched diag.
unsafe fn read_song_code(rt: &Runtime, holder: *mut u8) -> Result<String, &'static str> {
    if holder.is_null() {
        return Err("code read: holder null");
    }
    let inner = memory::read_ptr(holder) as *mut u8;
    let inner_ctrl = memory::read_ptr(holder.add(0x08));
    if inner.is_null() || inner_ctrl.is_null() {
        return Err("code read: inner pair null");
    }
    // Inner strong count (control block +0x08) — expired ⇒ don't touch.
    if memory::read_u32(inner_ctrl.add(0x08)) == 0 {
        return Err("code read: inner strong count 0");
    }
    let in_module = |p: usize| p >= rt.module_base && p < rt.module_base + rt.module_size;
    let vtable = memory::read_ptr(inner) as *const usize;
    if vtable.is_null() || !in_module(vtable as usize) {
        return Err("code read: inner vtable outside module");
    }
    let getter_addr = *vtable.add(1); // vt+0x08
    if !in_module(getter_addr) {
        return Err("code read: getter outside module");
    }
    let getter = std::mem::transmute::<usize, MusicCodeGetterFn>(getter_addr);
    let cstr = getter(inner);
    if cstr.is_null() {
        return Err("code read: getter returned null");
    }
    let mut out = Vec::with_capacity(16);
    for i in 0..32usize {
        let b = *cstr.add(i);
        if b == 0 {
            break;
        }
        out.push(b);
    }
    if out.is_empty() || out.len() >= 32 {
        return Err("code read: implausible string");
    }
    String::from_utf8(out).map_err(|_| "code read: non-utf8 string")
}

/// Texture-name list for the full row — `LENGTH` caption (gap baked into
/// its art), then `M:SS` (minutes unpadded, matching the reference art).
fn build_time_glyphs_secs(total_s: u32) -> Vec<&'static str> {
    const DIGITS: [&str; 10] = [
        "muca_card_bpm_0",
        "muca_card_bpm_1",
        "muca_card_bpm_2",
        "muca_card_bpm_3",
        "muca_card_bpm_4",
        "muca_card_bpm_5",
        "muca_card_bpm_6",
        "muca_card_bpm_7",
        "muca_card_bpm_8",
        "muca_card_bpm_9",
    ];
    let minutes = (total_s / 60).min(999);
    let seconds = total_s % 60;

    let mut out: Vec<&'static str> = Vec::with_capacity(9);
    out.push(LABEL_TEXTURE);
    let m = minutes as usize;
    if m >= 100 {
        out.push(DIGITS[(m / 100) % 10]);
    }
    if m >= 10 {
        out.push(DIGITS[(m / 10) % 10]);
    }
    out.push(DIGITS[m % 10]);
    out.push(COLON_TEXTURE);
    out.push(DIGITS[(seconds as usize) / 10]);
    out.push(DIGITS[(seconds as usize) % 10]);
    out
}

/// Apply a glyph list through the game setter (copy-assign — the backing
/// storage stays ours). An empty list blanks the display and releases the
/// CBitmaps back to the game's pool.
unsafe fn apply_names(rt: &mut Runtime, glyphs: &[&str]) {
    // The setter ends with an unconditional layout call, and layout
    // dereferences the parent clip with no null check — never invoke it
    // while the parent is unbound. (Blanking is only ever needed when a
    // display happened, which implies a bound parent.)
    if rt.sprite.is_null() || memory::read_ptr(rt.sprite.add(SL_PARENT)).is_null() {
        return;
    }
    let count = glyphs.len().min(MAX_GLYPHS);
    for (slot, name) in rt.names.iter_mut().zip(glyphs.iter().take(count)) {
        slot.set(name);
    }
    let begin = rt.names.as_ptr();
    let vec = MsvcVec::<MsvcString> {
        begin,
        end: begin.add(count),
        cap_end: begin.add(MAX_GLYPHS),
    };
    (rt.set_names)(rt.sprite, &vec);
}
