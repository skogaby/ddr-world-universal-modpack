//! Host tests for the song-select preview policy: the pure qualification
//! matrix (preview design §Components 4/5 — the create-detour branch's
//! decision logic), the feature gate's semantics, the restart half's
//! pure loader field-sanity predicate (design §Components 5 step 1), the
//! refresh debounce cell (design §Data Models), and the restart
//! sequence's ordering/abort semantics over a recording seam.

use super::preview::{
    cue_is_preview, feature_active, loader_sane, qualify, row_state_loaded, run_restart_sequence,
    set_feature_active, watchdog_cover, LoaderSnapshot, PreviewBindRequest, QualifyInputs,
    RefreshCell, RefreshPoll, RestartIo, RestartOutcome, INITIAL_PACKET_BYTES, LOADER_MODE_ONESHOT,
    PREVIEW_SLOT, REFRESH_DEBOUNCE_NANOS,
};
use crate::types::scenes::scene;

/// The qualifying baseline every matrix test perturbs: feature on, song
/// select, dance bank, P1 entered alone desiring 75% pitch-preserved.
fn qualifying() -> QualifyInputs<'static> {
    QualifyInputs {
        feature_active: true,
        scene: scene::SONG_SELECT,
        song_code: Some("abdt"),
        entered: [Some(true), Some(false)],
        desired: [75, 100],
        preserve: [true, false],
    }
}

#[test]
fn the_baseline_qualifies_with_the_controlling_sides_values() {
    assert_eq!(
        qualify(&qualifying()),
        Some(PreviewBindRequest {
            side: 0,
            percent: 75,
            preserve_pitch: true,
        })
    );

    // P2 as the single entered side takes P2's values.
    let mut inputs = qualifying();
    inputs.entered = [Some(false), Some(true)];
    inputs.desired = [100, 150];
    inputs.preserve = [true, false];
    assert_eq!(
        qualify(&inputs),
        Some(PreviewBindRequest {
            side: 1,
            percent: 150,
            preserve_pitch: false,
        })
    );
}

#[test]
fn feature_scene_and_path_gates_decline() {
    let mut inputs = qualifying();
    inputs.feature_active = false;
    assert_eq!(qualify(&inputs), None, "feature off");

    let mut inputs = qualifying();
    inputs.scene = scene::GAMEPLAY;
    assert_eq!(qualify(&inputs), None, "not song select");

    let mut inputs = qualifying();
    inputs.song_code = None;
    assert_eq!(qualify(&inputs), None, "non-dance path (custom_bgm etc.)");
}

#[test]
fn entered_side_policy_mirrors_gameplay_eligibility() {
    // Local versus qualifies with P1 governing (gameplay-classifier
    // parity — the SONG SPEED mod mirrors the rows, P1 as the seed; the
    // qualifier independently takes P1's values so a torn mirror can
    // never split the preview from the gameplay rate). P2's values are
    // distinct to make P1 governance observable.
    let mut inputs = qualifying();
    inputs.entered = [Some(true), Some(true)];
    inputs.desired = [75, 150];
    inputs.preserve = [true, false];
    assert_eq!(
        qualify(&inputs),
        Some(PreviewBindRequest {
            side: 0,
            percent: 75,
            preserve_pitch: true,
        }),
        "versus qualifies with P1's values"
    );

    // Versus with P1 at identity declines even while P2 desires a rate
    // (P1 governs).
    let mut inputs = qualifying();
    inputs.entered = [Some(true), Some(true)];
    inputs.desired = [100, 150];
    assert_eq!(qualify(&inputs), None, "versus, P1 at identity");

    // No side entered: nothing controls.
    let mut inputs = qualifying();
    inputs.entered = [Some(false), Some(false)];
    assert_eq!(qualify(&inputs), None, "no side");

    // ANY unreadable flag fails closed — even when the other side reads
    // as entered (the gameplay classifier's UnknownSession analogue).
    for unreadable in [
        [None, Some(false)],
        [Some(true), None],
        [None, None],
        [None, Some(true)],
    ] {
        let mut inputs = qualifying();
        inputs.entered = unreadable;
        assert_eq!(qualify(&inputs), None, "unreadable {unreadable:?}");
    }
}

#[test]
fn rate_gates_decline_identity_and_unsupported_values() {
    // Identity keeps zero footprint (R2/R4): the controlling side at 100
    // declines even while the OTHER (non-entered) side desires a rate.
    let mut inputs = qualifying();
    inputs.desired = [100, 75];
    assert_eq!(qualify(&inputs), None, "controlling side at identity");

    // Out-of-domain / unsnapped values fail closed (the option row snaps,
    // but the qualification must not trust that).
    for bad in [24, 73, 180, 0, -5] {
        let mut inputs = qualifying();
        inputs.desired = [bad, 100];
        assert_eq!(qualify(&inputs), None, "unsupported {bad}");
    }
}

#[test]
fn feature_gate_toggles() {
    set_feature_active(true);
    assert!(feature_active());
    set_feature_active(false);
    assert!(!feature_active());
}

/// The sane baseline every rejection test perturbs: a freshly ctor'd
/// preview loader mid-load (handle −1, not failed — `loader_sane`
/// deliberately ignores both; they are the executor's/watchdog's).
fn sane_snapshot() -> LoaderSnapshot {
    LoaderSnapshot {
        handle: -1,
        failed: false,
        mode: LOADER_MODE_ONESHOT,
        slot: PREVIEW_SLOT,
        xwb_id: 132,
        xsb_id: 133,
    }
}

#[test]
fn loader_sanity_accepts_the_preview_shape() {
    assert!(loader_sane(&sane_snapshot()));
    // Handle and failed states are NOT sanity inputs: a played loader
    // (handle ≥ 0) and a failed-latched one (the watchdog's re-arm
    // target) are both structurally the preview loader.
    let played = LoaderSnapshot {
        handle: 7,
        ..sane_snapshot()
    };
    assert!(loader_sane(&played));
    let failed = LoaderSnapshot {
        failed: true,
        ..sane_snapshot()
    };
    assert!(loader_sane(&failed));
}

#[test]
fn loader_sanity_rejects_every_off_shape_field() {
    // Wrong slot: any SE slot but 5 means the chain walked to some other
    // AudioLoader use (the BGM slots, a foreign sequence's loader).
    for slot in [0, 1, 4, 6, -1] {
        let snapshot = LoaderSnapshot {
            slot,
            ..sane_snapshot()
        };
        assert!(!loader_sane(&snapshot), "slot {slot}");
    }
    // Wrong mode: 0 is the BGM/loop play path, not the one-shot preview.
    for mode in [0u8, 2, 0xFF] {
        let snapshot = LoaderSnapshot {
            mode,
            ..sane_snapshot()
        };
        assert!(!loader_sane(&snapshot), "mode {mode}");
    }
    // Unresolved file ids: the ctor's −1 initializers mean the FileManager
    // acquire failed — nothing to stop/unregister/re-create.
    let no_xwb = LoaderSnapshot {
        xwb_id: -1,
        ..sane_snapshot()
    };
    assert!(!loader_sane(&no_xwb), "xwb -1");
    let no_xsb = LoaderSnapshot {
        xsb_id: -1,
        ..sane_snapshot()
    };
    assert!(!loader_sane(&no_xsb), "xsb -1");
}

// ── RefreshCell (design §Data Models; matrix rows C2/C8/C9) ────────────

const SELECT: i32 = scene::SONG_SELECT;
/// Any non-select scene for the scene-gate rows.
const NOT_SELECT: i32 = scene::SONG_SELECT + 1;

#[test]
fn refresh_cell_debounces_and_fires_once() {
    let cell = RefreshCell::new();
    assert_eq!(cell.poll_at(0, SELECT, 4), RefreshPoll::Idle);

    cell.stamp_at(1_000, 4);
    // Inside the quiet window: pending, request retained.
    assert_eq!(
        cell.poll_at(1_000 + REFRESH_DEBOUNCE_NANOS - 1, SELECT, 4),
        RefreshPoll::Pending
    );
    // Window elapsed: exactly one Fire, then Idle (C8's "exactly one
    // restart after the last tick").
    assert_eq!(
        cell.poll_at(1_000 + REFRESH_DEBOUNCE_NANOS, SELECT, 4),
        RefreshPoll::Fire
    );
    assert_eq!(
        cell.poll_at(1_000 + 2 * REFRESH_DEBOUNCE_NANOS, SELECT, 4),
        RefreshPoll::Idle
    );
}

#[test]
fn refresh_cell_coalesces_rapid_ticks_from_the_last_stamp() {
    let cell = RefreshCell::new();
    cell.stamp_at(0, 4);
    cell.stamp_at(100_000_000, 4); // a second tick 100 ms later
                                   // 150 ms after the FIRST tick but only 60 ms after the last: pending.
    assert_eq!(cell.poll_at(160_000_000, SELECT, 4), RefreshPoll::Pending);
    // 150 ms after the LAST tick: fire.
    assert_eq!(
        cell.poll_at(100_000_000 + REFRESH_DEBOUNCE_NANOS, SELECT, 4),
        RefreshPoll::Fire
    );
}

#[test]
fn refresh_cell_scene_gate_clears_the_request() {
    // C9 (fast-confirm race): the scene leaves SONG_SELECT before the
    // window elapses — the request clears and never fires afterwards.
    let cell = RefreshCell::new();
    cell.stamp_at(0, 4);
    assert_eq!(cell.poll_at(1, NOT_SELECT, 4), RefreshPoll::SceneCleared);
    assert_eq!(
        cell.poll_at(REFRESH_DEBOUNCE_NANOS + 1, SELECT, 4),
        RefreshPoll::Idle
    );
}

#[test]
fn refresh_cell_supersession_clears_the_request() {
    // Design Flow 2 step 0: a wheel settle re-published the selected song
    // after the stamp — the fresh create already qualified with the
    // newest values.
    let cell = RefreshCell::new();
    cell.stamp_at(0, 4);
    assert_eq!(
        cell.poll_at(REFRESH_DEBOUNCE_NANOS, SELECT, 6),
        RefreshPoll::Superseded
    );
    assert_eq!(
        cell.poll_at(2 * REFRESH_DEBOUNCE_NANOS, SELECT, 6),
        RefreshPoll::Idle
    );
}

#[test]
fn refresh_cell_clear_discards_the_request() {
    let cell = RefreshCell::new();
    cell.stamp_at(0, 4);
    cell.clear();
    assert_eq!(
        cell.poll_at(REFRESH_DEBOUNCE_NANOS, SELECT, 4),
        RefreshPoll::Idle
    );
}

// ── Restart sequence (design Flow 2 steps 2–5) ─────────────────────────

#[derive(Debug, PartialEq, Eq, Clone)]
enum Op {
    Stop(i32),
    Unregister(i32),
    Create(i32),
    Rearm,
}

struct RecordingIo {
    ops: Vec<Op>,
    fail_create: Option<i32>,
}

impl RecordingIo {
    fn new() -> Self {
        Self {
            ops: Vec::new(),
            fail_create: None,
        }
    }
}

impl RestartIo for RecordingIo {
    fn stop_cue(&mut self, handle: i32) {
        self.ops.push(Op::Stop(handle));
    }
    fn unregister(&mut self, file_id: i32) {
        self.ops.push(Op::Unregister(file_id));
    }
    fn create(&mut self, file_id: i32) -> bool {
        self.ops.push(Op::Create(file_id));
        self.fail_create != Some(file_id)
    }
    fn rearm_loader(&mut self) {
        self.ops.push(Op::Rearm);
    }
}

#[test]
fn restart_sequence_runs_the_stock_order() {
    // Stop (handle ≠ −1) → unregister XSB then XWB (the 2026-08-05
    // timeline's stock order) → create XWB then XSB → re-arm.
    let snapshot = LoaderSnapshot {
        handle: 7,
        ..sane_snapshot()
    };
    let mut io = RecordingIo::new();
    assert_eq!(
        run_restart_sequence(&snapshot, &mut io),
        RestartOutcome::Restarted
    );
    assert_eq!(
        io.ops,
        vec![
            Op::Stop(7),
            Op::Unregister(133),
            Op::Unregister(132),
            Op::Create(132),
            Op::Create(133),
            Op::Rearm,
        ]
    );
}

#[test]
fn restart_sequence_skips_the_stop_for_an_unplayed_loader() {
    // handle == −1: nothing to stop (the tick never fired, or was
    // re-armed) — the rest of the sequence runs unchanged.
    let mut io = RecordingIo::new();
    assert_eq!(
        run_restart_sequence(&sane_snapshot(), &mut io),
        RestartOutcome::Restarted
    );
    assert_eq!(io.ops.first(), Some(&Op::Unregister(133)));
    assert_eq!(io.ops.last(), Some(&Op::Rearm));
}

#[test]
fn restart_sequence_aborts_without_rearm_on_create_failure() {
    // First create (XWB) fails: abort immediately — no XSB create, no
    // re-arm (the loader keeps its stopped state; silent fail-open).
    let mut io = RecordingIo::new();
    io.fail_create = Some(132);
    assert_eq!(
        run_restart_sequence(&sane_snapshot(), &mut io),
        RestartOutcome::CreateFailed { file_id: 132 }
    );
    assert_eq!(
        io.ops,
        vec![Op::Unregister(133), Op::Unregister(132), Op::Create(132),]
    );

    // Second create (XSB) fails: the XWB create happened; still no re-arm.
    let mut io = RecordingIo::new();
    io.fail_create = Some(133);
    assert_eq!(
        run_restart_sequence(&sane_snapshot(), &mut io),
        RestartOutcome::CreateFailed { file_id: 133 }
    );
    assert!(!io.ops.contains(&Op::Rearm));
    assert_eq!(io.ops.last(), Some(&Op::Create(133)));
}

// ── Executor precondition + watchdog helpers ───────────────────────────

#[test]
fn row_state_gate_matches_the_loader_ticks_set() {
    // RE §1.3: the AudioLoader tick's own loaded-state set {0, 5, 6, 8}.
    for loaded in [0u32, 5, 6, 8] {
        assert!(row_state_loaded(loaded), "state {loaded}");
    }
    for pending in [1u32, 2, 3, 4, 7, 9, 0xFF] {
        assert!(!row_state_loaded(pending), "state {pending}");
    }
}

#[test]
fn cue_shape_gate_requires_the_preview_suffix() {
    assert!(cue_is_preview(b"abdt_s"));
    assert!(cue_is_preview(b"_s"));
    assert!(!cue_is_preview(b"abdt"));
    assert!(!cue_is_preview(b"abdt_S"));
    assert!(!cue_is_preview(b""));
}

#[test]
fn watchdog_cover_is_the_initial_packet_clamped_to_the_entry() {
    // Normal entry: data start + the engine's fixed 64 KiB first read.
    assert_eq!(
        watchdog_cover(2048, 30_000_000),
        2048 + INITIAL_PACKET_BYTES
    );
    // Short entry: the engine never requests past the declared length —
    // production covering the whole entry is enough to prepare.
    assert_eq!(watchdog_cover(2048, 40_000), 40_000);
}
