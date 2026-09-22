//! Host tests for the [`PersistMode`] persistence matrix and the
//! [`PersistMode::Session`] card-in reset (Training Mode v1, Step 3 —
//! design §4.1 "session persistence": Session rows are registered normally,
//! excluded from BOTH network directions and the JSON cache, and reset to
//! the row's `default_value` at the card-in profile-load lifecycle point).
//!
//! The three persistence choke points in the service facade
//! (`snapshot_for_save` / `snapshot_for_json_cache`, `resolve_from_load`)
//! consume the matrix METHODS pinned here — so "Session serializes nothing"
//! is enforced structurally by these tests, not re-derived per call site.
//! The two snapshots are exercised through the REAL `FrameworkState`
//! methods the facade calls, not re-implemented filters.
//!
//! [`PersistMode::Local`] (Multiplayer Bot Target Score, 2026-09-14): the
//! cabinet-only mode — JSON cache in both directions, never on the wire. The
//! load gate is split by [`LoadSource`] so a `Local` row accepts its own
//! JSON prime but ignores a network response.

use super::api::{LoadSource, PersistMode, RegisterSpec, ScalarFormat};
use super::registry::FrameworkState;

/// One scalar spec per persist mode, non-trivial default so resets are
/// observable (default 100, range 0..=599 like the training bound rows).
fn spec(id: &'static str, mode: PersistMode) -> RegisterSpec {
    RegisterSpec::scalar(id, 0, 599, 5, ScalarFormat::Integer)
        .step_coarse(30)
        .default_value(100)
        .persist_mode(mode)
}

const ALL_MODES: [PersistMode; 5] = [
    PersistMode::Full,
    PersistMode::SaveOnly,
    PersistMode::Local,
    PersistMode::None,
    PersistMode::Session,
];

#[test]
fn persistence_matrix_is_exact() {
    // (mode, saved_to_network, loaded_from_network, json_cached, session_scoped)
    let expected = [
        (PersistMode::Full, true, true, true, false),
        (PersistMode::SaveOnly, true, false, false, false),
        (PersistMode::Local, false, false, true, false),
        (PersistMode::None, false, false, false, false),
        (PersistMode::Session, false, false, false, true),
    ];
    assert_eq!(expected.len(), ALL_MODES.len(), "every mode has a row");
    for (mode, save, load, json, session) in expected {
        assert_eq!(mode.saved_to_network(), save, "{mode:?} network save");
        assert_eq!(mode.loaded_from_network(), load, "{mode:?} network load");
        assert_eq!(mode.json_cached(), json, "{mode:?} JSON cache");
        assert_eq!(mode.session_scoped(), session, "{mode:?} session scope");
        // The per-source load gate is exactly the two columns above.
        assert_eq!(
            mode.accepts_load(LoadSource::Network),
            load,
            "{mode:?} gate/network"
        );
        assert_eq!(
            mode.accepts_load(LoadSource::JsonPrime),
            json,
            "{mode:?} gate/json"
        );
    }
}

/// The network save snapshot (`snapshot_for_save` in the facade →
/// [`FrameworkState::network_save_snapshot`]) includes exactly the modes with
/// `saved_to_network()` — a Session / Local row with a non-default value
/// contributes NO wire field.
#[test]
fn save_snapshot_filter_excludes_session_and_none() {
    let mut state = FrameworkState::default();
    for (id, mode) in [
        ("mx_full", PersistMode::Full),
        ("mx_saveonly", PersistMode::SaveOnly),
        ("mx_local", PersistMode::Local),
        ("mx_none", PersistMode::None),
        ("mx_session", PersistMode::Session),
    ] {
        state.try_register(spec(id, mode)).unwrap();
        let _ = state.set_value(id, 0, 500); // non-default everywhere
    }
    let emitted: Vec<String> = state
        .network_save_snapshot()
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    assert_eq!(emitted, vec!["mx_full", "mx_saveonly"]);
}

/// The JSON-cache snapshot (`snapshot_for_json_cache` →
/// [`FrameworkState::json_cache_snapshot`]) includes exactly the modes with
/// `json_cached()` — `Full` AND the cache-only `Local`, with both sides'
/// current values. Regression (2026-09-22): the writer used to filter the
/// NETWORK snapshot by `json_cached`, an intersection that is `Full` only, so
/// no `Local` row (Background Dancer/Stage, the Multiplayer Bot rows) was
/// ever written to `mod-config.json`.
#[test]
fn json_cache_snapshot_includes_local_rows() {
    let mut state = FrameworkState::default();
    for (id, mode) in [
        ("jc_full", PersistMode::Full),
        ("jc_saveonly", PersistMode::SaveOnly),
        ("jc_local", PersistMode::Local),
        ("jc_none", PersistMode::None),
        ("jc_session", PersistMode::Session),
    ] {
        state.try_register(spec(id, mode)).unwrap();
        let _ = state.set_value(id, 0, 500);
        let _ = state.set_value(id, 1, 250);
    }
    let cached = state.json_cache_snapshot();
    assert_eq!(
        cached,
        vec![
            ("jc_full".to_string(), [500, 250]),
            ("jc_local".to_string(), [500, 250]),
        ]
    );
    // Local is cache-only: never in the network snapshot.
    assert!(state
        .network_save_snapshot()
        .iter()
        .all(|(id, _)| id != "jc_local"));
}

/// Both snapshots carry the post-`save_transform` wire value — the one the
/// JSON prime's `load_transform` inverts — for `Local` rows too.
#[test]
fn json_cache_snapshot_applies_save_transform() {
    fn plus_one(_id: &str, v: i32) -> i32 {
        v + 1
    }
    fn minus_one(_id: &str, v: i32) -> i32 {
        v - 1
    }
    let mut state = FrameworkState::default();
    state
        .try_register(spec("tx_local", PersistMode::Local).persist_transform(plus_one, minus_one))
        .unwrap();
    let _ = state.set_value("tx_local", 0, 200);
    assert_eq!(
        state.json_cache_snapshot(),
        vec![("tx_local".to_string(), [201, 101])]
    );
}

/// The load-side gate (`resolve_from_load` in the facade) admits only
/// `loaded_from_network()` modes: a simulated network/JSON-prime load
/// application must skip a Session row entirely.
#[test]
fn load_gate_rejects_session_rows() {
    let mut state = FrameworkState::default();
    state
        .try_register(spec("ld_session", PersistMode::Session))
        .unwrap();
    state
        .try_register(spec("ld_full", PersistMode::Full))
        .unwrap();

    // Model the facade's load path: gate on the matrix method, then write.
    for (id, incoming) in [("ld_session", 555), ("ld_full", 555)] {
        let idx = state.index_of(id).unwrap();
        if state.options[idx].persist.loaded_from_network() {
            let _ = state.set_value(id, 0, incoming);
        }
    }
    assert_eq!(
        state.get_value("ld_session", 0),
        Some(100),
        "load must not touch Session"
    );
    assert_eq!(
        state.get_value("ld_full", 0),
        Some(555),
        "Full load unregressed"
    );
}

/// Card-in reset: Session rows return to `default_value` for exactly the
/// carded-in side; other sides, other modes, and already-default rows are
/// untouched (no-op resets dispatch no callbacks).
#[test]
fn card_in_reset_restores_session_defaults_per_side() {
    let mut state = FrameworkState::default();
    for (id, mode) in [
        ("ci_session_a", PersistMode::Session),
        ("ci_session_b", PersistMode::Session),
        ("ci_full", PersistMode::Full),
        ("ci_none", PersistMode::None),
    ] {
        state.try_register(spec(id, mode)).unwrap();
    }
    // Side 0 dirty on one Session row + the Full + None rows; side 1 dirty
    // on the OTHER Session row.
    let _ = state.set_value("ci_session_a", 0, 500);
    let _ = state.set_value("ci_session_b", 1, 300);
    let _ = state.set_value("ci_full", 0, 150);
    let _ = state.set_value("ci_none", 0, 250);

    // Card-in on side 0.
    let callbacks = state.reset_session_values(0);
    assert_eq!(
        callbacks.len(),
        1,
        "exactly the one changed Session row dispatches"
    );
    assert_eq!(
        callbacks[0].0, "ci_session_a",
        "callback carries the option id"
    );
    assert_eq!(callbacks[0].2, 0, "callback side is the carded-in side");
    assert_eq!(callbacks[0].3, 100, "callback carries the default value");

    assert_eq!(
        state.get_value("ci_session_a", 0),
        Some(100),
        "reset to default"
    );
    assert_eq!(
        state.get_value("ci_session_b", 1),
        Some(300),
        "other side untouched"
    );
    assert_eq!(state.get_value("ci_full", 0), Some(150), "Full untouched");
    assert_eq!(
        state.get_value("ci_none", 0),
        Some(250),
        "None untouched (no reset semantics)"
    );

    // Card-in on side 0 again: everything already default — no dispatch.
    let callbacks = state.reset_session_values(0);
    assert!(callbacks.is_empty(), "no-op reset dispatches nothing");

    // Card-in on side 1 resets the other Session row.
    let callbacks = state.reset_session_values(1);
    assert_eq!(callbacks.len(), 1);
    assert_eq!(state.get_value("ci_session_b", 1), Some(100));
}

/// Out-of-range side is a safe no-op (the facade passes raw u8 sides).
#[test]
fn card_in_reset_ignores_invalid_side() {
    let mut state = FrameworkState::default();
    state
        .try_register(spec("ci_bad_side", PersistMode::Session))
        .unwrap();
    let _ = state.set_value("ci_bad_side", 0, 500);
    assert!(state.reset_session_values(2).is_empty());
    assert_eq!(state.get_value("ci_bad_side", 0), Some(500));
}

/// Every mode keeps a consistent matrix: a mode that loads from the network
/// must also save to it (the framework has no network-load-only concept), a
/// JSON-cached mode is never session-scoped (the cache would resurrect a
/// value the card-in reset just cleared), and session scoping implies full
/// exclusion. Guards future variants against nonsensical combinations.
#[test]
fn matrix_invariants_hold_for_all_modes() {
    for mode in ALL_MODES {
        if mode.loaded_from_network() {
            assert!(
                mode.saved_to_network(),
                "{mode:?}: network load-only is unsupported"
            );
        }
        if mode.json_cached() {
            assert!(
                !mode.session_scoped(),
                "{mode:?}: a JSON-cached mode cannot be session-scoped"
            );
        }
        if mode.session_scoped() {
            assert!(
                !mode.saved_to_network() && !mode.loaded_from_network() && !mode.json_cached(),
                "{mode:?}: session scoping means full persistence exclusion"
            );
        }
    }
}

/// `Local` rows: the load gate admits the cabinet's own JSON prime and
/// rejects a network response carrying the same id (an old server or a
/// stale profile field must never override what the cabinet cached).
#[test]
fn local_rows_accept_json_prime_but_not_network_loads() {
    let mut state = FrameworkState::default();
    state
        .try_register(spec("lo_local", PersistMode::Local))
        .unwrap();
    state
        .try_register(spec("lo_full", PersistMode::Full))
        .unwrap();

    // Model the facade's load path with a Network source.
    for id in ["lo_local", "lo_full"] {
        let idx = state.index_of(id).unwrap();
        if state.options[idx].persist.accepts_load(LoadSource::Network) {
            let _ = state.set_value(id, 0, 555);
        }
    }
    assert_eq!(
        state.get_value("lo_local", 0),
        Some(100),
        "network load ignored"
    );
    assert_eq!(state.get_value("lo_full", 0), Some(555), "Full unregressed");

    // ... and with the JSON prime.
    for id in ["lo_local", "lo_full"] {
        let idx = state.index_of(id).unwrap();
        if state.options[idx]
            .persist
            .accepts_load(LoadSource::JsonPrime)
        {
            let _ = state.set_value(id, 1, 333);
        }
    }
    assert_eq!(
        state.get_value("lo_local", 1),
        Some(333),
        "JSON prime applied"
    );
    assert_eq!(
        state.get_value("lo_full", 1),
        Some(333),
        "Full still primed"
    );

    // The JSON writer's snapshot (`json_cache_snapshot`) includes Local.
    let cached: Vec<String> = state
        .json_cache_snapshot()
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    assert_eq!(cached, vec!["lo_local", "lo_full"]);
}
