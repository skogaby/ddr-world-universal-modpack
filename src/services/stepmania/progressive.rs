//! Progressive full-bank registry — the handles the writer publishes and
//! the LayeredFS open/read hooks consult (design §4.8 item 3, §5.6).
//!
//! A [`ProgressiveFile`] is created the moment a full bank is first wanted
//! (`xwb/<b>.xwb.part`, pre-sized to the plan's total length, header
//! written) and lives here, findable by BOTH its `.part` path and its final
//! path, until the encode finishes and the file is renamed into place. The
//! writer (`providers::encode_full_bank`, on a scheduler worker) publishes
//! `watermark` / `tail_ready` / `done`; `file_hooks` records `fd →
//! handle` on open and holds reads past the watermark
//! (`progressive_logic::should_wait`).
//!
//! Lifetimes: a handle is *active* (in the registry) while the file is at
//! its `.part` path. Completion renames it; when the rename is refused
//! (Windows: the game still holds the file open without share-delete) the
//! handle stays active with `done` set, the resolver keeps serving the
//! `.part` path, and the rename is retried at the next resolve or job
//! completion. A boot-time sweep removes every stale `.part`.

use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::core::audio::preview::PreviewWindow;
use crate::core::audio::AudioInfo;
use crate::core::xact::song_bank::SongBankPlan;
use crate::{log_info, log_warn};

use super::cache;
use super::progressive_logic::Progress;

/// Inputs the encoder needs, staged by whoever created the file so the
/// worker does not read / probe the audio a second time.
pub struct Staged {
    pub audio: Vec<u8>,
    pub info: AudioInfo,
    pub window: PreviewWindow,
    pub file: File,
}

pub struct ProgressiveFile {
    pub basename: String,
    pub part_path: PathBuf,
    pub final_path: PathBuf,
    pub plan: SongBankPlan,
    /// Final bytes from offset 0.
    pub watermark: AtomicU64,
    /// The preview entry (file tail) has been written.
    pub tail_ready: AtomicBool,
    pub done: AtomicBool,
    /// Encode failed or was cancelled; the file is gone or silent-padded.
    pub failed: AtomicBool,
    /// The resolver handed the `.part` path to the game at least once —
    /// a cancel is no longer honoured.
    pub served: AtomicBool,
    /// Cooperative cancel request (speculative job, highlight moved on).
    pub cancel: AtomicBool,
    /// Done but still at the `.part` path (rename refused).
    pub rename_pending: AtomicBool,
    pub created: Instant,
    pub staged: Mutex<Option<Staged>>,
    /// Game-path forms of both paths (what the open hook compares against).
    part_game_path: String,
    final_game_path: String,
}

impl ProgressiveFile {
    /// Snapshot for the pure wait decision.
    #[must_use]
    pub fn progress(&self) -> Progress {
        Progress {
            planned_len: self.plan.total_len(),
            watermark: self.watermark.load(Ordering::Acquire),
            tail_start: self
                .plan
                .entry_data_offset(crate::core::xact::song_bank::PREVIEW_ENTRY),
            tail_ready: self.tail_ready.load(Ordering::Acquire),
            done: self.done.load(Ordering::Acquire),
        }
    }

    #[must_use]
    pub fn is_done(&self) -> bool {
        self.done.load(Ordering::Acquire)
    }

    /// The path the resolver should serve right now.
    #[must_use]
    pub fn serve_path(&self) -> &str {
        if self.rename_pending.load(Ordering::Acquire) || !self.is_done() {
            &self.part_game_path
        } else {
            &self.final_game_path
        }
    }

    /// Whether `game_path` (as the resolver / open hook sees it) is this
    /// file, at either of its two names.
    #[must_use]
    pub fn matches(&self, game_path: &str) -> bool {
        game_path == self.part_game_path || game_path == self.final_game_path
    }

    /// Cancel requested and nobody has the file open: stop encoding.
    #[must_use]
    pub fn should_abort(&self) -> bool {
        self.cancel.load(Ordering::Acquire) && !self.served.load(Ordering::Acquire)
    }
}

static ACTIVE: Mutex<Option<HashMap<String, Arc<ProgressiveFile>>>> = Mutex::new(None);
/// `ACTIVE.len()` mirror so the open hook pays one atomic load when idle.
static ACTIVE_COUNT: AtomicUsize = AtomicUsize::new(0);

/// `<final>.part`.
#[must_use]
pub fn part_path(final_path: &Path) -> PathBuf {
    let mut s = final_path.as_os_str().to_owned();
    s.push(".part");
    PathBuf::from(s)
}

fn with_active<R>(f: impl FnOnce(&mut HashMap<String, Arc<ProgressiveFile>>) -> R) -> R {
    let mut guard = ACTIVE.lock().unwrap_or_else(|p| p.into_inner());
    let map = guard.get_or_insert_with(HashMap::new);
    let r = f(map);
    ACTIVE_COUNT.store(map.len(), Ordering::Release);
    r
}

/// The active handle for `basename`, if any.
#[must_use]
pub fn active(basename: &str) -> Option<Arc<ProgressiveFile>> {
    if ACTIVE_COUNT.load(Ordering::Acquire) == 0 {
        return None;
    }
    with_active(|m| m.get(basename).cloned())
}

/// The active handle whose `.part` or final path is `game_path`. Called by
/// the open hook for every redirected open; one atomic load when nothing is
/// in flight.
#[must_use]
pub fn lookup_path(game_path: &str) -> Option<Arc<ProgressiveFile>> {
    if ACTIVE_COUNT.load(Ordering::Acquire) == 0 {
        return None;
    }
    with_active(|m| m.values().find(|p| p.matches(game_path)).cloned())
}

/// Register a freshly created `.part` (header written). Returns the
/// existing handle instead when one is already active for the basename.
pub fn register(
    basename: &str,
    final_path: &Path,
    plan: SongBankPlan,
    header_len: u64,
    staged: Staged,
) -> Arc<ProgressiveFile> {
    let part = part_path(final_path);
    let handle = Arc::new(ProgressiveFile {
        basename: basename.to_string(),
        part_game_path: cache::to_game_path(&part),
        final_game_path: cache::to_game_path(final_path),
        part_path: part,
        final_path: final_path.to_path_buf(),
        plan,
        watermark: AtomicU64::new(header_len),
        tail_ready: AtomicBool::new(false),
        done: AtomicBool::new(false),
        failed: AtomicBool::new(false),
        served: AtomicBool::new(false),
        cancel: AtomicBool::new(false),
        rename_pending: AtomicBool::new(false),
        created: Instant::now(),
        staged: Mutex::new(Some(staged)),
    });
    with_active(|m| m.entry(basename.to_string()).or_insert(handle).clone())
}

/// Drop the handle (file renamed into place, or removed after a failure).
pub fn unregister(basename: &str) {
    with_active(|m| {
        m.remove(basename);
    });
}

/// Mark complete and move `.part` → final. On a refused rename the handle
/// stays registered with `rename_pending` so the `.part` keeps being served
/// (it is complete) and the rename is retried later.
pub fn complete(handle: &ProgressiveFile) {
    handle.done.store(true, Ordering::Release);
    try_rename(handle);
}

/// Retry a pending rename. `true` when the file now sits at its final path.
pub fn try_rename(handle: &ProgressiveFile) -> bool {
    if !handle.is_done() {
        return false;
    }
    if handle.final_path.is_file() && !handle.part_path.exists() {
        handle.rename_pending.store(false, Ordering::Release);
        unregister(&handle.basename);
        return true;
    }
    if handle.final_path.exists() {
        let _ = std::fs::remove_file(&handle.final_path);
    }
    match std::fs::rename(&handle.part_path, &handle.final_path) {
        Ok(()) => {
            handle.rename_pending.store(false, Ordering::Release);
            unregister(&handle.basename);
            true
        }
        Err(e) => {
            if !handle.rename_pending.swap(true, Ordering::AcqRel) {
                log_info!(
                    "StepMania: full bank '{}' complete but still in use -- rename deferred ({})",
                    handle.basename,
                    e
                );
            }
            false
        }
    }
}

/// Retry every deferred rename (cheap; called when a job completes or a
/// full bank is resolved).
pub fn retry_pending_renames() {
    if ACTIVE_COUNT.load(Ordering::Acquire) == 0 {
        return;
    }
    let pending: Vec<Arc<ProgressiveFile>> = with_active(|m| {
        m.values()
            .filter(|p| p.rename_pending.load(Ordering::Acquire))
            .cloned()
            .collect()
    });
    for p in pending {
        if try_rename(&p) {
            log_info!("StepMania: full bank '{}' moved into place", p.basename);
        }
    }
}

/// The encode failed or was cancelled before anyone opened the file:
/// remove the `.part` and forget the handle.
pub fn abandon(handle: &ProgressiveFile, why: &str) {
    handle.failed.store(true, Ordering::Release);
    handle.done.store(true, Ordering::Release);
    if let Ok(mut s) = handle.staged.lock() {
        *s = None;
    }
    if let Err(e) = std::fs::remove_file(&handle.part_path) {
        if e.kind() != std::io::ErrorKind::NotFound {
            log_warn!(
                "StepMania: cannot remove abandoned {}: {}",
                cache::to_game_path(&handle.part_path),
                e
            );
        }
    }
    unregister(&handle.basename);
    log_info!(
        "StepMania: full bank '{}' abandoned -- {}",
        handle.basename,
        why
    );
}

/// How many handles are active (status lines / tests).
#[must_use]
pub fn active_count() -> usize {
    ACTIVE_COUNT.load(Ordering::Acquire)
}
