//! Init-time profiling. Permanent observability for the DLL load
//! sequence, gated behind a runtime flag from `mod-config.json`:
//!
//! ```json
//! { "diagnostics": { "profiling": true } }
//! ```
//!
//! When the flag is absent or false, every public entry point short-
//! circuits on a single `AtomicBool::load` — zero log noise, zero
//! mutex traffic.
//!
//! When the flag is true, ticks emit one `[init-prof]` line per phase
//! boundary plus an aggregate `scan_pattern` / `scan_pattern_all` /
//! `scan_batch` summary at end of init.
//!
//! Usage from `lib.rs::init`:
//!
//!   profiling::start();
//!   ... module load + signature scan ...
//!   profiling::tick("module_load");
//!   ... resolve_all ...
//!   profiling::tick("resolve_all");
//!   mods::config::init();
//!   profiling::set_enabled(<flag from config>);  // flushes any buffered ticks
//!   ... rest of init ...
//!   profiling::tick("init_complete");
//!   profiling::dump_scan_stats();
//!
//! `start` and the first few ticks run before `set_enabled` resolves
//! the gate. Those ticks are buffered; at `set_enabled` time they
//! either flush (if on) or are silently dropped (if off).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::log_info;

static ENABLED: AtomicBool = AtomicBool::new(false);

#[inline]
fn enabled() -> bool {
    ENABLED.load(Ordering::Acquire)
}

struct BufferedTick {
    label: String,
    delta: Duration,
    elapsed: Duration,
}

struct ProfileState {
    start: Option<Instant>,
    last: Option<Instant>,
    /// True once `set_enabled` has been called; ticks before this point
    /// land in `buffered` and get flushed (or dropped) at decision time.
    gate_decided: bool,
    buffered: Vec<BufferedTick>,
    scan_pattern_calls: u64,
    scan_pattern_total: Duration,
    scan_pattern_slowest: Duration,
    scan_pattern_slowest_label: String,
    scan_pattern_all_calls: u64,
    scan_pattern_all_total: Duration,
    scan_pattern_all_slowest: Duration,
    scan_pattern_all_slowest_label: String,
    scan_batch_calls: u64,
    scan_batch_patterns_total: u64,
    scan_batch_hits_total: u64,
    scan_batch_total: Duration,
}

static STATE: Mutex<ProfileState> = Mutex::new(ProfileState {
    start: None,
    last: None,
    gate_decided: false,
    buffered: Vec::new(),
    scan_pattern_calls: 0,
    scan_pattern_total: Duration::ZERO,
    scan_pattern_slowest: Duration::ZERO,
    scan_pattern_slowest_label: String::new(),
    scan_pattern_all_calls: 0,
    scan_pattern_all_total: Duration::ZERO,
    scan_pattern_all_slowest: Duration::ZERO,
    scan_pattern_all_slowest_label: String::new(),
    scan_batch_calls: 0,
    scan_batch_patterns_total: 0,
    scan_batch_hits_total: 0,
    scan_batch_total: Duration::ZERO,
});

/// Record the init-start instant. Always runs (so ticks have a meaningful
/// elapsed-since-start), but only emits a log line if the gate was already
/// open when called — practically `set_enabled` runs after this, so the
/// first emitted line is always a buffered-flush.
pub fn start() {
    let now = Instant::now();
    let mut st = STATE.lock().unwrap();
    st.start = Some(now);
    st.last = Some(now);
    drop(st);
    if enabled() {
        log_info!("[init-prof] start");
    }
}

/// Mark a phase boundary. Records the delta since the previous tick and
/// the elapsed since `start`. Buffered if called before `set_enabled`;
/// dropped (if disabled) or emitted (if enabled) at flush time.
pub fn tick(label: &str) {
    let now = Instant::now();
    let mut st = STATE.lock().unwrap();
    let last = st.last.unwrap_or(now);
    let start = st.start.unwrap_or(now);
    let delta = now - last;
    let elapsed = now - start;
    st.last = Some(now);

    if !st.gate_decided {
        st.buffered.push(BufferedTick {
            label: label.to_string(),
            delta,
            elapsed,
        });
        return;
    }
    drop(st);

    if !enabled() {
        return;
    }

    log_info!(
        "[init-prof] {:<32} +{:>8.3}ms  (elapsed {:>8.3}ms)",
        label,
        delta.as_secs_f64() * 1000.0,
        elapsed.as_secs_f64() * 1000.0
    );
}

/// Resolve the runtime gate. Called once after `mod-config.json` is loaded.
/// Flushes any buffered ticks (emitting them if `on`, dropping them
/// silently if not).
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Release);
    let buffered = {
        let mut st = STATE.lock().unwrap();
        st.gate_decided = true;
        std::mem::take(&mut st.buffered)
    };
    if on {
        for b in buffered {
            log_info!(
                "[init-prof] {:<32} +{:>8.3}ms  (elapsed {:>8.3}ms)",
                b.label,
                b.delta.as_secs_f64() * 1000.0,
                b.elapsed.as_secs_f64() * 1000.0
            );
        }
    }
}

// The `record_*` functions below are unconditional. Scans that fire
// before `set_enabled` resolves (most notably `resolve_all` running
// inside `scan_patterns_batch`) would otherwise be silently dropped.
// Cost when profiling is off: one Mutex lock + a few integer adds per
// call, called ~50 times at init — negligible. Emission is still gated
// at `dump_scan_stats`.

pub fn record_scan_pattern(pattern: &str, dur: Duration) {
    let mut st = STATE.lock().unwrap();
    st.scan_pattern_calls += 1;
    st.scan_pattern_total += dur;
    if dur > st.scan_pattern_slowest {
        st.scan_pattern_slowest = dur;
        st.scan_pattern_slowest_label = pattern.to_string();
    }
}

pub fn record_scan_pattern_all(pattern: &str, dur: Duration) {
    let mut st = STATE.lock().unwrap();
    st.scan_pattern_all_calls += 1;
    st.scan_pattern_all_total += dur;
    if dur > st.scan_pattern_all_slowest {
        st.scan_pattern_all_slowest = dur;
        st.scan_pattern_all_slowest_label = pattern.to_string();
    }
}

/// Record a single `scan_patterns_batch` call. `n_patterns` is how many
/// patterns participated; `n_hits` is how many resolved successfully.
pub fn record_scan_batch(n_patterns: usize, n_hits: usize, dur: Duration) {
    let mut st = STATE.lock().unwrap();
    st.scan_batch_calls += 1;
    st.scan_batch_patterns_total += n_patterns as u64;
    st.scan_batch_hits_total += n_hits as u64;
    st.scan_batch_total += dur;
}

/// Emit aggregate stats for all scan flavors. Call once near the end of
/// init so the summary follows all the per-phase lines.
pub fn dump_scan_stats() {
    if !enabled() {
        return;
    }
    let st = STATE.lock().unwrap();
    log_info!(
        "[init-prof] scan_pattern: {} calls, total {:.3}ms, slowest {:.3}ms ({})",
        st.scan_pattern_calls,
        st.scan_pattern_total.as_secs_f64() * 1000.0,
        st.scan_pattern_slowest.as_secs_f64() * 1000.0,
        if st.scan_pattern_slowest_label.is_empty() {
            "<none>"
        } else {
            &st.scan_pattern_slowest_label
        }
    );
    log_info!(
        "[init-prof] scan_pattern_all: {} calls, total {:.3}ms, slowest {:.3}ms ({})",
        st.scan_pattern_all_calls,
        st.scan_pattern_all_total.as_secs_f64() * 1000.0,
        st.scan_pattern_all_slowest.as_secs_f64() * 1000.0,
        if st.scan_pattern_all_slowest_label.is_empty() {
            "<none>"
        } else {
            &st.scan_pattern_all_slowest_label
        }
    );
    log_info!(
        "[init-prof] scan_batch: {} calls, {} patterns total, {} hits total, {:.3}ms total",
        st.scan_batch_calls,
        st.scan_batch_patterns_total,
        st.scan_batch_hits_total,
        st.scan_batch_total.as_secs_f64() * 1000.0
    );
}
