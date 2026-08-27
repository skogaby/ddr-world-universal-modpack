//! Timing Offsets — exposes the game's global timing-offset values as
//! operator-configurable settings.
//!
//! Ports/expands the 32-bit `patches.js` sound-offset hex hack to the four
//! integer timing offsets the engine publishes into its runtime config map:
//! `SOUND_OFFSET`, `INPUT_OFFSET`, `RENDER_OFFSET`, `BOMB_FRAME_OFFSET`. See
//! `docs/hex_edit_porting.md` (Hack 4) and
//! `.agents/planning/20260626-timing-offsets/` (research r1–r5).
//!
//! These values are **global / cabinet-wide** (the game publishes them into one
//! process-wide config map), so they are configured only via `mod-config.json`
//! and the DLL overlay menu (`mod_menu`) — never the game's per-player options.
//!
//! Mechanism (one detour, derived in `core::signatures`): hook the config-map
//! **int setter** that the timing-init publisher calls. The setter has only
//! timing-related call sites, so filtering on the four offset keys and
//! substituting our configured value makes our values win at every publish;
//! the game then latches them into the `GamePlayActor` at the next gameplay
//! entry (so changes take effect on the **next song**, not mid-song).
//!
//! Two-tier graceful degradation:
//!   - the **setter** resolution is load-bearing — if it can't resolve, the
//!     whole mod self-disables (no hook, no effect);
//!   - the **overlay UI** is non-fatal — if the overlay rows can't register,
//!     the mod still applies config-file values at boot.
//!
//! Sub-feature: **auto-calibration** ([`calibration`]) — the "Calibrate next
//! song?" overlay row measures one song's per-step timing errors and folds
//! the mean into SOUND_OFFSET through this mod's `set_offset` path. Pure
//! decision layers live in [`compute`] (host-tested via
//! `scripts/validate_auto_calibration.sh`).

mod calibration;
pub mod compute;

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

use once_cell::sync::Lazy;
use retour::GenericDetour;

use crate::mods::mod_trait::{Mod, ModContext};
use crate::{log_info, log_warn};

/// The four offsets this mod controls, in a fixed index order used everywhere
/// (state arrays, config, overlay rows): `[SOUND, INPUT, RENDER, BOMB_FRAME]`.
const FIELD_COUNT: usize = 4;

/// Inclusive clamp applied to every configured / written value (Q4).
const VALUE_MIN: i32 = -1000;
const VALUE_MAX: i32 = 1000;

fn clamp_value(v: i32) -> i32 {
    v.clamp(VALUE_MIN, VALUE_MAX)
}

/// All per-field data for one timing offset, co-located in a single struct so
/// adding/editing a field is one struct literal instead of edits spread across
/// many parallel arrays — a mis-ordered value is then visible at a glance rather
/// than a silent, compiler-invisible cross-array mismatch. Indexed in the
/// canonical `[SOUND, INPUT, RENDER, BOMB_FRAME]` order used everywhere.
struct FieldDef {
    /// Engine config-map key — the exact ASCII string the game FNV-1a-hashes.
    engine_key: &'static str,
    /// NUL-terminated copy of `engine_key`, for passing to the original setter
    /// (which takes a C-string). Kept adjacent so the two can't drift apart.
    engine_key_cstr: &'static str,
    /// `mod-config.json` key for this field.
    json_key: &'static str,
    /// Stock default (record 0 / common preset), re-confirmed from the binary
    /// in R1. The fall-back "stock" value if a live write was never observed.
    default: i32,
    /// Human-readable overlay row label.
    label: &'static str,
    /// One-line overlay hint (R3: all four semantics binary-confirmed).
    hint: &'static str,
    /// Overlay row key (distinct from `engine_key` and `json_key`).
    row_key: &'static str,
}

const FIELDS: [FieldDef; FIELD_COUNT] = [
    FieldDef {
        engine_key: "SOUND_OFFSET",
        engine_key_cstr: "SOUND_OFFSET\0",
        json_key: "sound_offset",
        default: 87,
        label: "Sound Offset",
        hint: "Global audio offset (ms). Higher = audio plays later.",
        row_key: "timing_sound_offset",
    },
    FieldDef {
        engine_key: "INPUT_OFFSET",
        engine_key_cstr: "INPUT_OFFSET\0",
        json_key: "input_offset",
        default: 28,
        label: "Input Offset",
        hint: "Input/judge timing offset (ms).",
        row_key: "timing_input_offset",
    },
    FieldDef {
        engine_key: "RENDER_OFFSET",
        engine_key_cstr: "RENDER_OFFSET\0",
        json_key: "render_offset",
        default: 17,
        label: "Render Offset",
        hint: "Display latency offset (ms). Higher = arrows drawn later.",
        row_key: "timing_render_offset",
    },
    FieldDef {
        engine_key: "BOMB_FRAME_OFFSET",
        engine_key_cstr: "BOMB_FRAME_OFFSET\0",
        json_key: "bomb_frame_offset",
        default: 0,
        label: "Bomb Frame Offset",
        hint: "Shock-arrow effect timing (frames, 60fps).",
        row_key: "timing_bomb_frame_offset",
    },
];

/// Materialize the per-field stock defaults as an array (for whole-array
/// initializers / fallbacks). Per-field code reads `FIELDS[idx].default`.
fn default_values() -> [i32; FIELD_COUNT] {
    std::array::from_fn(|i| FIELDS[i].default)
}

// ── Config-map int setter hook ──────────────────────────────────────
//
// Signature: `i64 set(const char* key /*RCX*/, i32 value /*EDX*/)`. The game
// FNV-1a-hashes `key` and updates the matching node's value. We match the four
// timing keys by hashing the incoming key the same way (R1: seed 0x811c9dc5,
// prime 0x1000193) and comparing to the precomputed key hashes.

type SetIntFn = unsafe extern "C" fn(*const u8, i32) -> i64;

static mut SETTER_HOOK: Option<GenericDetour<SetIntFn>> = None;

/// FNV-1a hash of each field's `engine_key`, computed once in `init`. Indexed in
/// the canonical `[SOUND, INPUT, RENDER, BOMB]` order. Used to identify which
/// timing key an incoming setter call is for, without per-call string compares.
static mut KEY_HASHES: [u32; FIELD_COUNT] = [0; FIELD_COUNT];

/// One-shot diagnostic latch per field: log the first substitution of each key
/// (the boot publish), then stay quiet. Hand-sized to `FIELD_COUNT`: `AtomicBool`
/// isn't `Copy`, so the `[expr; N]` shorthand won't compile — adding a field
/// means adding an entry here too (a length mismatch is a compile error, not a
/// silent bug).
static DIAG_LOGGED: [AtomicBool; FIELD_COUNT] = [
    AtomicBool::new(false),
    AtomicBool::new(false),
    AtomicBool::new(false),
    AtomicBool::new(false),
];

/// Latched true the first time our setter hook observes a write. The game only
/// calls the config-map setter once the map global is initialized (the boot
/// publisher itself guards on `map != null`), so a hook firing is proof the map
/// exists. Live pushes (`push_to_map`/`push_all_stock`) gate on this: at DLL-init
/// time the map isn't created yet, so calling the setter then would dereference a
/// null map and crash (see learnings: "don't call native game functions from the
/// init thread"). Boot seeding doesn't need the push anyway — the boot publish
/// flows through the hook and is substituted. Once true it stays true (the map
/// lives for the process lifetime); not reset on disable.
///
/// The hook hit is the primary readiness signal, but it only fires on a publish.
/// `map_is_live()` *also* consults `MAP_GLOBAL` directly so a push can proceed
/// once the map exists even if our hook never saw a write (e.g. if the boot
/// publisher somehow ran before our detour installed). See `map_is_live`.
static MAP_READY: AtomicBool = AtomicBool::new(false);

/// Address of the game's config-map root pointer (`DAT_1806ebcf0`/`DAT_1806f1d70`
/// analog), derived in `core::signatures` as `timing_config_map_global`. The
/// setter dereferences this and the boot publisher null-guards on it, so
/// `*MAP_GLOBAL != 0` means the map is built and the setter is safe to call.
/// Stored as a `usize` (0 = unresolved) for a lock-free read from any thread.
/// Resolved once in `init`; the pointed-at global is valid for the process
/// lifetime. Used as the independent boot-seed fallback signal behind
/// `map_is_live()` (see F4 in the aspect review).
static MAP_GLOBAL: AtomicUsize = AtomicUsize::new(0);

/// Whether it's safe to call the game's config-map setter: true once the map
/// root has been built. Returns true if either the hook has fired at least once
/// (`MAP_READY` — proof the game itself called the setter) OR we can observe the
/// derived map-root global holding a non-null pointer (`MAP_GLOBAL`). The latter
/// is the boot-seed fallback: it lets a push succeed even if our hook never
/// observed the boot write, without ever dereferencing a not-yet-built map.
fn map_is_live() -> bool {
    if MAP_READY.load(Ordering::Acquire) {
        return true;
    }
    let g = MAP_GLOBAL.load(Ordering::Acquire);
    if g == 0 {
        return false; // global unresolved → fall back to hook-only gating
    }
    // SAFETY: `g` is the address of a process-global pointer slot inside the
    // game image (resolved once in `init`, valid for the process lifetime). A
    // volatile read observes the game's latest store; a null value means the
    // map isn't built yet (the same condition the boot publisher checks).
    let live = unsafe { std::ptr::read_volatile(g as *const usize) != 0 };
    if live {
        // Cache the positive result so subsequent calls skip the volatile read.
        MAP_READY.store(true, Ordering::Release);
    }
    live
}

/// Shared mutable state for the four offsets. Read on the game thread (the
/// setter hook) and written on the input/render thread (overlay callbacks /
/// enable-disable). A `Mutex` is fine: the hook fires only on publish + our own
/// pushes (a handful of times), never per-frame, so lock cost is negligible.
struct TimingState {
    /// Desired value per field (clamped). Substituted into the setter when ON.
    configured: [i32; FIELD_COUNT],
    /// The genuine value the game first tried to publish per field (captured on
    /// the first observed write). Used to revert on master-OFF. `None` until
    /// observed; falls back to each field's `default`.
    stock: [Option<i32>; FIELD_COUNT],
    /// Whether the mod is active (master ON). When false the hook forwards
    /// writes unchanged.
    master_on: bool,
}

static STATE: Lazy<Mutex<TimingState>> = Lazy::new(|| {
    Mutex::new(TimingState {
        configured: default_values(),
        stock: [None; FIELD_COUNT],
        master_on: false,
    })
});

/// FNV-1a (32-bit) over a NUL-terminated C string, matching the game's config-
/// map hash (R1). Returns `None` if `key` is null. Reads until the terminator
/// with a generous cap so a missing NUL can't run away.
unsafe fn fnv1a_cstr(key: *const u8) -> Option<u32> {
    if key.is_null() {
        return None;
    }
    let mut hash: u32 = 0x811c9dc5;
    let mut p = key;
    for _ in 0..256 {
        let b = *p;
        if b == 0 {
            return Some(hash);
        }
        hash = hash.wrapping_mul(0x0100_0193) ^ (b as u32);
        p = p.add(1);
    }
    None
}

/// If `key` is one of the four timing keys, return its canonical index.
unsafe fn match_key(key: *const u8) -> Option<usize> {
    let h = fnv1a_cstr(key)?;
    let hashes = *std::ptr::addr_of!(KEY_HASHES);
    hashes.iter().position(|&kh| kh == h)
}

/// Detour body. For a timing key: capture the genuine stock value on the first
/// observed write, then (if master ON) substitute our configured value before
/// forwarding. Non-timing keys and the master-OFF case forward unchanged.
/// Computes the value to forward under the lock, then releases it before
/// calling the original (never hold the lock across the FFI call).
unsafe extern "C" fn set_int_hook(key: *const u8, value: i32) -> i64 {
    let mut forward = value;
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let Some(idx) = match_key(key) else { return };
        // A hook firing proves the config map exists → live pushes are now safe.
        MAP_READY.store(true, Ordering::Release);
        if let Ok(mut st) = STATE.lock() {
            // Capture the genuine stock value the game tried to publish, once.
            if st.stock[idx].is_none() {
                st.stock[idx] = Some(value);
            }
            if st.master_on {
                forward = clamp_value(st.configured[idx]);
            }
        }
        // One-shot per key at the boot publish: record the game's stock value
        // and the value we apply. Operational provenance (4 lines at boot), not
        // per-frame spam.
        if !DIAG_LOGGED[idx].swap(true, Ordering::AcqRel) {
            log_info!(
                "TimingOffsets: {} stock={} applied={}",
                FIELDS[idx].engine_key,
                value,
                forward
            );
        }
    }));
    call_original(key, forward)
}

/// Forward to the original setter. Returns 0 if the hook isn't installed
/// (should never happen while the detour is active, but keeps the path total).
unsafe fn call_original(key: *const u8, value: i32) -> i64 {
    if let Some(ref hook) = *std::ptr::addr_of!(SETTER_HOOK) {
        hook.call(key, value)
    } else {
        0
    }
}

/// Install the setter detour at `setter_addr`. Returns false (logged) on
/// failure; caller treats that as the load-bearing failure and self-disables.
fn install_hook(setter_addr: *const u8) -> bool {
    unsafe {
        let target: SetIntFn = std::mem::transmute(setter_addr);
        match crate::core::hooks::install_enabled(
            std::ptr::addr_of_mut!(SETTER_HOOK),
            target,
            set_int_hook,
        ) {
            Ok(()) => true,
            Err(e) => {
                log_warn!("TimingOffsets: setter hook install failed: {:?}", e);
                false
            }
        }
    }
}

/// Tear down the setter detour (on disable). Note (F7): this `take()`+`disable()`
/// runs on the disable thread (operator-driven, via the mod-menu toggle) while
/// `set_int_hook`/`call_original` may read `SETTER_HOOK` on the game thread, and
/// `retour::disable()` does not drain in-flight callers. The window is tiny and
/// benign in practice — disable is a rare manual action and the setter fires
/// only on a config publish (not per-frame), so the two racing on `SETTER_HOOK`
/// is improbable; and `call_original` already tolerates a `None` (returns 0).
/// We accept this rather than add quiescence/refcount machinery: the offsets
/// feature can wait for the next publish, and a fuller fix isn't worth the
/// complexity for an operator-paced teardown. (The five input-manager detours
/// share this shape but are install-once / never removed, so they're
/// effectively immutable post-init and don't hit this window.)
fn remove_hook() {
    unsafe {
        if let Some(d) = (*std::ptr::addr_of_mut!(SETTER_HOOK)).take() {
            let _ = d.disable();
        }
    }
    for f in &DIAG_LOGGED {
        f.store(false, Ordering::Release);
    }
}

/// Push `value` into the live config map for field `idx` by calling the
/// **original** setter with the key C-string. Update-only: the key already
/// exists post-boot, so this updates it in place. The new value is latched
/// into the next `GamePlayActor` (next song). No-op if the hook isn't
/// installed. Note: this re-enters our detour (we call the *original* via
/// `call_original`, so no recursion), and stock is already captured, so the
/// detour just re-substitutes — benign.
fn push_to_map(idx: usize, value: i32) {
    if idx >= FIELD_COUNT {
        return;
    }
    // Gate on the map being live. At DLL-init time the game hasn't created the
    // config map yet, so calling the setter would dereference a null map and
    // crash; `map_is_live()` returns true once the hook has fired OR the derived
    // map-root global is observed non-null (the boot-seed fallback).
    if !map_is_live() {
        return;
    }
    let key = FIELDS[idx].engine_key_cstr.as_ptr();
    unsafe {
        // Call the original directly so we don't double-substitute through the
        // hook entry. (The map is update-only; the key exists after boot.)
        call_original(key, clamp_value(value));
    }
}

/// Public entry the overlay's scalar `on_change` calls: clamp + store the new
/// configured value, push it live into the map, and persist to mod-config.json.
/// `idx` is the canonical field index `[SOUND, INPUT, RENDER, BOMB]`.
pub fn set_offset(idx: usize, value: i32) {
    if idx >= FIELD_COUNT {
        return;
    }
    let v = clamp_value(value);
    let on = {
        match STATE.lock() {
            Ok(mut st) => {
                st.configured[idx] = v;
                st.master_on
            }
            Err(_) => return,
        }
    };
    // Only push live if we're active; if master is off the value is stored for
    // when the mod is next enabled.
    if on {
        push_to_map(idx, v);
    }
    persist_all();
}

/// Read the current configured value for a field (for the overlay to display).
pub fn get_offset(idx: usize) -> i32 {
    if idx >= FIELD_COUNT {
        return 0;
    }
    STATE
        .lock()
        .map(|st| st.configured[idx])
        .unwrap_or(FIELDS[idx].default)
}

/// Write all four configured values to `mod-config.json` under `timing_offsets`
/// (read-modify-write, preserving other keys).
fn persist_all() {
    let cfg = match STATE.lock() {
        Ok(st) => st.configured,
        Err(_) => return,
    };
    let mut obj = serde_json::Map::new();
    for (i, field) in FIELDS.iter().enumerate() {
        obj.insert(field.json_key.to_string(), serde_json::Value::from(cfg[i]));
    }
    crate::mods::config::save_json_key("timing_offsets", serde_json::Value::Object(obj));
}

/// Push all configured values live (used on enable so a mid-session enable
/// applies without waiting for a republish).
fn push_all_configured() {
    let cfg = match STATE.lock() {
        Ok(st) => st.configured,
        Err(_) => return,
    };
    for i in 0..FIELD_COUNT {
        push_to_map(i, cfg[i]);
    }
}

/// Revert the live map to stock for all four fields (used on disable). Uses the
/// captured genuine stock value, or the known default if never observed.
fn push_all_stock() {
    // Same map-liveness gate as push_to_map: never call the setter before the
    // game has created the config map.
    if !map_is_live() {
        return;
    }
    let stock = match STATE.lock() {
        Ok(st) => {
            let mut out = [0i32; FIELD_COUNT];
            for i in 0..FIELD_COUNT {
                out[i] = st.stock[i].unwrap_or(FIELDS[i].default);
            }
            out
        }
        Err(_) => return,
    };
    for (i, field) in FIELDS.iter().enumerate() {
        // Push stock directly via the original (master is already off, so the
        // hook wouldn't substitute anyway, but this is explicit).
        let key = field.engine_key_cstr.as_ptr();
        unsafe {
            call_original(key, stock[i]);
        }
    }
}

// ── Overlay scalar rows ─────────────────────────────────────────────

/// Registry mod id (the master toggle row the scalar rows nest under).
const MOD_ID: &str = "timing-offsets";

/// Overlay scalar-row step sizes (Q4): fine (Left/Right) and coarse (Start-held).
const STEP_FINE: i32 = 1;
const STEP_COARSE: i32 = 20;

/// Register the four scalar child rows in the overlay (best-effort). Each nests
/// under the `timing-offsets` master toggle and is seeded from the current
/// configured value.
fn register_overlay_rows() {
    let configured = STATE
        .lock()
        .map(|st| st.configured)
        .unwrap_or_else(|_| default_values());
    for i in 0..FIELD_COUNT {
        register_overlay_row(i, configured[i]);
    }
    log_info!("TimingOffsets: registered 4 overlay scalar rows");
}

/// Register (or idempotently re-register — `register_scalar_row` replaces by
/// key) field `idx`'s overlay row seeded with `initial`.
fn register_overlay_row(idx: usize, initial: i32) {
    use crate::mods::mod_menu::{self, RowChangeCallback, ScalarRowSpec};
    let field = &FIELDS[idx];
    // The callback captures its field index; `set_offset` is the single
    // authoritative apply path (no per-field shim functions needed).
    let cb: RowChangeCallback = std::sync::Arc::new(move |v| set_offset(idx, v));
    mod_menu::register_scalar_row(ScalarRowSpec {
        key: field.row_key.to_string(),
        label: field.label.to_string(),
        hint: field.hint.to_string(),
        parent_row_key: Some(MOD_ID.to_string()),
        min: VALUE_MIN,
        max: VALUE_MAX,
        step_fine: STEP_FINE,
        step_coarse: STEP_COARSE,
        initial,
        on_change: cb,
    });
}

/// Refresh field `idx`'s overlay row DISPLAY to the current configured value
/// after a programmatic `set_offset` (auto-calibration apply). The row store
/// only updates through menu edits, so without this the row keeps showing
/// the pre-calibration value — and a later edit would step from the stale
/// value and clobber the calibration (cabinet-observed, first deploy
/// 2026-08-26). Idempotent re-registration is the same mechanism the
/// calibrate row's flip-OFF uses.
pub(crate) fn refresh_overlay_row(idx: usize) {
    if idx >= FIELD_COUNT {
        return;
    }
    register_overlay_row(idx, get_offset(idx));
}

/// Remove the four scalar rows from the overlay (on disable).
fn remove_overlay_rows() {
    let row_keys: [&str; FIELD_COUNT] = std::array::from_fn(|i| FIELDS[i].row_key);
    crate::mods::mod_menu::remove_rows_for(&row_keys);
}

pub struct TimingOffsetsMod {
    /// Resolved address of the config-map int setter (load-bearing). `None`
    /// until `init` resolves it; if still `None` at `enable`, the mod
    /// self-disables.
    setter_addr: Option<*const u8>,
}

// Raw pointers into game address space are valid for the process lifetime and
// only touched on controlled threads (matches the project's other mods).
unsafe impl Send for TimingOffsetsMod {}

impl TimingOffsetsMod {
    pub fn new() -> Self {
        Self { setter_addr: None }
    }
}

impl Mod for TimingOffsetsMod {
    fn id(&self) -> &str {
        "timing-offsets"
    }

    fn name(&self) -> &str {
        "Timing Offsets"
    }

    fn description(&self) -> &str {
        "Adjust the game's global timing offsets (sound/input/render/bomb)"
    }

    fn required_signatures(&self) -> &[&str] {
        // Graceful degradation: resolved best-effort in `init`, the mod
        // self-disables in `enable` if the setter is missing (rather than
        // failing registration). Matches `center_arrows_single`.
        &[]
    }

    fn init(&mut self, ctx: &ModContext) -> bool {
        // The int setter is derived in `resolve_derived` from the publisher's
        // config-set landmark (see `derive_timing_config_setter`). Resolved by
        // its role, not a (non-unique) prologue AOB.
        self.setter_addr = ctx.signatures.get_address("timing_config_set_int");
        if self.setter_addr.is_none() {
            log_warn!(
                "TimingOffsets: timing_config_set_int unresolved -- mod will self-disable in enable()"
            );
        }

        // Record the config-map root global (derived alongside the setter) so
        // `map_is_live()` can observe map readiness independently of the hook
        // (the boot-seed fallback, F4). Non-fatal if absent — gating then falls
        // back to the hook-fired latch alone (the original behavior).
        match ctx.signatures.get_address("timing_config_map_global") {
            Some(g) => MAP_GLOBAL.store(g as usize, Ordering::Release),
            None => log_warn!(
                "TimingOffsets: timing_config_map_global unresolved -- map-liveness falls back to hook-only gating"
            ),
        }

        // Precompute the FNV-1a hash of each key so the per-call detour does a
        // cheap integer compare instead of a string compare.
        unsafe {
            let hashes = &mut *std::ptr::addr_of_mut!(KEY_HASHES);
            for (i, field) in FIELDS.iter().enumerate() {
                let mut h: u32 = 0x811c9dc5;
                for &b in field.engine_key.as_bytes() {
                    h = h.wrapping_mul(0x0100_0193) ^ (b as u32);
                }
                hashes[i] = h;
            }
        }

        // Auto-calibration data source: the judge_submit per-step feed
        // (idempotent — power_user_statistics installs the same detour when
        // it inits first). Non-fatal: without it `calibration::enable()`
        // refuses and only the calibrate row is absent.
        if !crate::mods::power_user_statistics::data_feed::install(ctx.signatures) {
            log_warn!(
                "TimingOffsets: judge_submit feed unavailable -- auto-calibration will be absent"
            );
        }
        true
    }

    fn enable(&mut self) {
        // The setter is load-bearing: without it we can't apply offsets, so
        // self-disable cleanly (no hook, no effect) rather than half-enable.
        let Some(addr) = self.setter_addr else {
            log_warn!("TimingOffsets: setter unresolved -- mod self-disabled (no effect)");
            return;
        };

        // Load configured values from mod-config.json (clamped); missing
        // section → stock defaults. Set master ON so the hook substitutes.
        let cfg = crate::mods::config::get()
            .and_then(|c| c.timing_offsets.clone())
            .unwrap_or_default();
        if let Ok(mut st) = STATE.lock() {
            st.configured = [
                clamp_value(cfg.sound_offset),
                clamp_value(cfg.input_offset),
                clamp_value(cfg.render_offset),
                clamp_value(cfg.bomb_frame_offset),
            ];
            st.master_on = true;
        }

        if !install_hook(addr) {
            log_warn!("TimingOffsets: setter hook install failed -- mod self-disabled");
            if let Ok(mut st) = STATE.lock() {
                st.master_on = false;
            }
            return;
        }

        // Push configured values live now, so a mid-session enable applies on
        // the next song without waiting for the game to republish. (At boot the
        // publisher hasn't run yet, so these update-only writes may no-op until
        // the keys exist; the boot publish then carries the substituted values.
        // Either way the values are correct by the next gameplay entry.)
        push_all_configured();

        // The hook substitutes our configured values as the game publishes each
        // key (boot publish = the seed). Values latch into the GamePlayActor at
        // the next gameplay entry (changes take effect next song, not mid-song).
        if let Ok(st) = STATE.lock() {
            log_info!(
                "TimingOffsets: enabled -- setter hook @ {:p}; configured={:?}",
                addr,
                st.configured
            );
        }

        // Best-effort: register the overlay rows. Non-fatal — if this fails
        // the mod still applies config-seeded values (config-only mode).
        // Calibration first: its "Calibrate next song?" row renders at the
        // top of the section (contributed rows render in insertion order).
        calibration::enable();
        register_overlay_rows();
    }

    fn disable(&mut self) {
        // Tear down the calibration sub-feature (row, callbacks, any live
        // session) before the offset rows.
        calibration::disable();
        // Remove the overlay rows first.
        remove_overlay_rows();
        // Master OFF: stop substituting and revert the live map to stock.
        if let Ok(mut st) = STATE.lock() {
            st.master_on = false;
        }
        // Push stock BEFORE removing the hook (push uses call_original, which
        // needs the detour present). Effect latches next song.
        push_all_stock();
        remove_hook();
        log_info!("TimingOffsets: disabled (reverted to stock)");
    }

    /// Active iff the load-bearing setter detour is installed. `enable()`
    /// self-disables (returns early without installing) when the setter didn't
    /// resolve, so reporting this keeps the registry/overlay from showing a
    /// false `[ON]` master over an inert mod — which would otherwise reveal the
    /// four child scalar rows and let the operator "set" offsets that never
    /// apply. (See F6 in the aspect review.)
    fn is_active(&self) -> bool {
        unsafe { (*std::ptr::addr_of!(SETTER_HOOK)).is_some() }
    }
}
