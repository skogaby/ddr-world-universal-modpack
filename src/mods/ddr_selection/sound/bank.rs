//! The `dsel` era-sound bank: build (background thread), registration (game
//! thread), lock-free lookup (any thread).
//!
//! Sources (LayeredFS-aware — a mod-folder copy wins, e.g. the A3 importer's
//! `data_mods/ddr_selection_a3/` on an install that lacks them):
//! `data/arc/soundbanks_n.arc` (`voice_n.xsb`, `se_normal_n.xsb`),
//! `data/arc/se_normal_n.arc` (`se_normal_n.xwb`, in-memory),
//! `data/sound/win/voice_n.xwb` (streaming — only the needed waves are read).
//! World's LOADED `data/arc/soundbanks.arc` is read too, only for its cue
//! names: a `dsel` cue World also carries (`vo_ingame_ready`,
//! `vo_stage_clear`, `vo_stage_*`) is marked "shared" (diagnostics; the AFP
//! route only ever acts while a legacy skin is armed).
//!
//! Registration claims manager slot 4 (`game_audio::register_bank`), which no
//! stock bank ever maps to. It is refused unless slots 0..=3 hold the stock
//! banks and slot 4 is empty — never slot 5 (the per-song bank slot).
//!
//! Fail-open: any failure ⇒ one WARN, no bank, the legacy clips' embedded
//! sounds stay silent (World's stock behaviour); everything else in DDR
//! SELECTION is unaffected.

use std::collections::HashMap;
use std::ffi::CString;
use std::io::{Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};

use crate::core::arc;
use crate::services::game_audio::{self, BankRequest};
use crate::services::scene3d::arc_set;
use crate::{log_info, log_warn};

use super::bank_build::{self, Built, Source, WaveBytes, WaveSource};
use super::cues;

pub const BANK_NAME: &str = "dsel";
/// The manager slot the bank must land in (see the module docs).
const TARGET_SLOT: i32 = 4;

const ST_IDLE: u8 = 0;
const ST_BUILDING: u8 = 1;
const ST_BUILT: u8 = 2;
const ST_REGISTERED: u8 = 3;
const ST_FAILED: u8 = 4;

static STATE: AtomicU8 = AtomicU8::new(ST_IDLE);
/// Registered slot (−1 = none) — read lock-free by the AFP route.
static SLOT: AtomicI32 = AtomicI32::new(-1);
/// The built pair waiting for the game thread.
static PENDING: Mutex<Option<Built>> = Mutex::new(None);
/// `cue name → (index, shared with World's loaded banks)`. Immutable once set.
static NAMES: OnceLock<HashMap<Box<[u8]>, (usize, bool)>> = OnceLock::new();
/// The cue names by index (diagnostics).
static NAME_LIST: OnceLock<Vec<String>> = OnceLock::new();
/// Upper bound on the cue count (per-cue play counters).
pub const MAX_CUES: usize = 128;
static REGISTER_WARNED: AtomicBool = AtomicBool::new(false);

/// Start the background build (once per process).
pub fn start_build() {
    if STATE
        .compare_exchange(ST_IDLE, ST_BUILDING, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("ddr-sel-bank".into())
        .spawn(|| {
            let t0 = std::time::Instant::now();
            let outcome = std::panic::catch_unwind(build_now).unwrap_or_else(|_| {
                Err("builder panicked".to_string())
            });
            match outcome {
                Ok((built, shared)) => {
                    log_info!(
                        "DDR SELECTION: era bank built in {} ms -- {} cues, {} waves, {:.1} MB ({} shared with World's banks{}{})",
                        t0.elapsed().as_millis(),
                        built.cues.len(),
                        built.wave_count,
                        built.wave_bytes as f64 / 1_048_576.0,
                        shared.values().filter(|s| s.1).count(),
                        if built.missing.is_empty() {
                            String::new()
                        } else {
                            format!("; missing {:?}", built.missing)
                        },
                        if built.skipped.is_empty() {
                            String::new()
                        } else {
                            format!("; skipped {:?}", built.skipped)
                        },
                    );
                    let _ = NAME_LIST.set(built.cues.clone());
                    let _ = NAMES.set(shared);
                    if let Ok(mut g) = PENDING.lock() {
                        *g = Some(built);
                    }
                    STATE.store(ST_BUILT, Ordering::Release);
                }
                Err(e) => {
                    log_warn!(
                        "DDR SELECTION: era bank not built ({e}) -- legacy clips' own sounds stay silent"
                    );
                    STATE.store(ST_FAILED, Ordering::Release);
                }
            }
        });
    if spawned.is_err() {
        log_warn!("DDR SELECTION: could not start the era-bank build thread");
        STATE.store(ST_FAILED, Ordering::Release);
    }
}

/// Register the built pair. **GAME THREAD ONLY** (scene callbacks). No-op
/// until the build finished; idempotent after success; one WARN on failure.
pub fn try_register() {
    if STATE.load(Ordering::Acquire) != ST_BUILT || !game_audio::is_available() {
        return;
    }
    // Stock banks in 0..=3, slot 4 free — anything else and register_bank's
    // lowest-free-slot pick could claim slot 5 (the per-song bank's slot).
    // (Early boot: the stock banks are not loaded yet — retry silently.)
    if !(0..TARGET_SLOT).all(|s| game_audio::sound_bank_in_slot(s).is_some()) {
        return;
    }
    if game_audio::sound_bank_in_slot(TARGET_SLOT).is_some() {
        if !REGISTER_WARNED.swap(true, Ordering::Relaxed) {
            log_warn!(
                "DDR SELECTION: manager slot {} is already taken -- era bank not registered (retrying on scene changes)",
                TARGET_SLOT
            );
        }
        return;
    }
    let Some(built) = PENDING.lock().ok().and_then(|mut g| g.take()) else {
        return;
    };
    let names = built.cues.clone();
    let Some(handle) = game_audio::register_bank(BankRequest {
        name: BANK_NAME,
        xwb: built.xwb,
        xsb: built.xsb,
    }) else {
        log_warn!(
            "DDR SELECTION: the engine refused the era bank -- legacy clips' own sounds stay silent"
        );
        STATE.store(ST_FAILED, Ordering::Release);
        return;
    };
    if handle.slot() != TARGET_SLOT {
        log_warn!(
            "DDR SELECTION: era bank landed in slot {} (expected {}) -- routing disabled",
            handle.slot(),
            TARGET_SLOT
        );
        STATE.store(ST_FAILED, Ordering::Release);
        return;
    }
    // Self-check: every cue resolves through the engine's own lookup.
    let unresolved: Vec<&str> = names
        .iter()
        .filter(|n| {
            CString::new(n.as_str())
                .ok()
                .and_then(|c| game_audio::cue_index(handle, &c))
                .is_none()
        })
        .map(String::as_str)
        .collect();
    if !unresolved.is_empty() {
        log_warn!(
            "DDR SELECTION: {} era cue(s) do not resolve in the registered bank: {:?}",
            unresolved.len(),
            unresolved
        );
    }
    SLOT.store(handle.slot(), Ordering::Release);
    STATE.store(ST_REGISTERED, Ordering::Release);
    log_info!(
        "DDR SELECTION: era bank '{}' registered in slot {} ({}/{} cues resolve)",
        BANK_NAME,
        handle.slot(),
        names.len() - unresolved.len(),
        names.len()
    );
}

/// The registered slot, or `None`. Lock-free.
pub fn slot() -> Option<i32> {
    let s = SLOT.load(Ordering::Acquire);
    (s >= 0).then_some(s)
}

/// `Some((index, shared))` when `name` is a `dsel` cue (`shared` = World's
/// loaded banks carry it too). Lock-free, allocation-free.
pub fn lookup(name: &[u8]) -> Option<(usize, bool)> {
    NAMES.get()?.get(name).copied()
}

/// The cue name at `index` (diagnostics).
pub fn cue_name(index: usize) -> Option<&'static str> {
    NAME_LIST.get()?.get(index).map(String::as_str)
}

// ── Build (background thread) ────────────────────────────────────────

type Shared = HashMap<Box<[u8]>, (usize, bool)>;

fn build_now() -> Result<(Built, Shared), String> {
    let voice_xsb = arc_member("data/arc/soundbanks_n.arc", "voice_n.xsb")?;
    let se_xsb = arc_member("data/arc/soundbanks_n.arc", "se_normal_n.xsb")?;
    let se_xwb = arc_member("data/arc/se_normal_n.arc", "se_normal_n.xwb")?;
    let voice_path = arc_set::resolve_path("data/sound/win/voice_n.xwb")
        .ok_or("data/sound/win/voice_n.xwb not found")?;
    let mut voice_file = FileBytes::open(&voice_path)?;
    let voice_meta = voice_file.meta()?;

    let se_parsed = bank_build::parse_xsb(&se_xsb).map_err(|e| format!("se_normal_n.xsb: {e}"))?;
    let voice_parsed =
        bank_build::parse_xsb(&voice_xsb).map_err(|e| format!("voice_n.xsb: {e}"))?;
    let se_meta =
        bank_build::parse_xwb_meta(&se_xwb).map_err(|e| format!("se_normal_n.xwb: {e}"))?;
    let mut se_bytes: &[u8] = &se_xwb;
    let mut sources = vec![
        Source {
            xsb: se_parsed,
            waves: vec![WaveSource {
                meta: se_meta,
                bytes: &mut se_bytes,
            }],
        },
        Source {
            xsb: voice_parsed,
            waves: vec![WaveSource {
                meta: voice_meta,
                bytes: &mut voice_file,
            }],
        },
    ];
    let wanted = cues::all();
    let built =
        bank_build::build(&mut sources, &wanted, BANK_NAME).map_err(|e| format!("build: {e}"))?;
    bank_build::validate(&built.xsb, &built.xwb).map_err(|e| format!("self-validation: {e}"))?;

    // World's loaded banks — names only.
    let mut world: Vec<String> = Vec::new();
    for m in [
        "voice.xsb",
        "se_normal.xsb",
        "se_system.xsb",
        "bgm_menu.xsb",
    ] {
        match arc_member("data/arc/soundbanks.arc", m)
            .and_then(|b| bank_build::parse_xsb(&b).map_err(|e| e.to_string()))
        {
            Ok(x) => world.extend(x.cue_names().map(str::to_string)),
            Err(e) => log_warn!(
                "DDR SELECTION: World's {m} unreadable ({e}) -- its cues treated as not shared"
            ),
        }
    }
    if built.cues.len() > MAX_CUES {
        return Err(format!(
            "{} cues exceed the {MAX_CUES} limit",
            built.cues.len()
        ));
    }
    let shared: Shared = built
        .cues
        .iter()
        .enumerate()
        .map(|(i, c)| {
            (
                c.as_bytes().to_vec().into_boxed_slice(),
                (i, world.iter().any(|w| w == c)),
            )
        })
        .collect();
    Ok((built, shared))
}

/// The bytes of the arc member whose path ends with `/<file>`.
fn arc_member(arc_rel: &str, file: &str) -> Result<Vec<u8>, String> {
    let bytes = arc_set::read_bytes(arc_rel).ok_or_else(|| format!("{arc_rel} not found"))?;
    let entries = arc::parse(&bytes).ok_or_else(|| format!("{arc_rel}: not an arc"))?;
    let suffix = format!("/{file}");
    let entry = entries
        .iter()
        .find(|e| e.path.ends_with(&suffix) || e.path == file)
        .ok_or_else(|| format!("{arc_rel} has no {file}"))?;
    arc::extract(&bytes, entry).ok_or_else(|| format!("{arc_rel}: {file} did not extract"))
}

/// A wave bank read on demand from disk (the 45 MB streaming `voice_n.xwb`
/// only contributes ~13 MB of waves).
struct FileBytes {
    file: std::fs::File,
}

impl FileBytes {
    fn open(path: &str) -> Result<Self, String> {
        std::fs::File::open(path)
            .map(|file| FileBytes { file })
            .map_err(|e| format!("{path}: {e}"))
    }

    fn meta(&mut self) -> Result<bank_build::XwbMeta, String> {
        let head = WaveBytes::read(self, 0, 52)?;
        let need = bank_build::xwb_meta_len(&head).map_err(|e| e.to_string())?;
        let meta_bytes = WaveBytes::read(self, 0, need)?;
        bank_build::parse_xwb_meta(&meta_bytes).map_err(|e| format!("voice_n.xwb: {e}"))
    }
}

impl WaveBytes for FileBytes {
    fn read(&mut self, offset: u64, len: usize) -> Result<Vec<u8>, String> {
        self.file
            .seek(SeekFrom::Start(offset))
            .map_err(|e| e.to_string())?;
        let mut buf = vec![0u8; len];
        self.file.read_exact(&mut buf).map_err(|e| e.to_string())?;
        Ok(buf)
    }
}
