//! Value-changed observer multicast for the custom options framework
//! (overlay-menu rewrite design §4.3.3).
//!
//! Consumers (the overlay's PLAYER SETTINGS tab) subscribe an
//! `Arc<dyn Fn(&str, u8, i32)>` and receive `(option_id, side, new_value)`
//! after EVERY value mutation: `set_value`, `set_value_silent` (observer
//! only — the option's own `on_change` stays suppressed there),
//! `resolve_from_load`, card-in session resets, `set_scalar_bounds`
//! clamp-writes, and the in-game menu's press path. Registration-time
//! priming does NOT dispatch (boot noise the mirror doesn't want).
//!
//! Contract:
//! - [`dispatch`] is NEVER called while a framework lock is held — callers
//!   follow the registry's deferred-dispatch pattern (mutate under lock,
//!   release, then notify). The subscriber list has its own lock, released
//!   before any callback runs (snapshot-clone), so a subscriber may
//!   subscribe/unsubscribe or re-enter framework reads without deadlock.
//! - Each subscriber runs under `catch_unwind`; a panicking subscriber is
//!   contained (one latched WARN) and never blocks later subscribers.
//! - Tokens are process-unique; unsubscribing an unknown token is a no-op.
//!
//! Dependency-light (std + the logging macro) so the module mounts in
//! `scripts/validate_custom_options.sh`'s host harness.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::log_warn;

/// Subscriber signature: `(option_id, player_side, new_value)`.
pub type ValueChangedFn = dyn Fn(&str, u8, i32) + Send + Sync;

/// Registered subscribers, each with its unsubscribe token.
static SUBSCRIBERS: Mutex<Vec<(usize, Arc<ValueChangedFn>)>> = Mutex::new(Vec::new());

/// Monotonic token source (0 is never issued).
static NEXT_TOKEN: AtomicUsize = AtomicUsize::new(1);

/// One latched WARN for the panicking-subscriber class.
static PANIC_WARNED: AtomicBool = AtomicBool::new(false);

/// Subscribe to value-changed events. Returns the unsubscribe token.
pub fn subscribe_value_changed(cb: Arc<ValueChangedFn>) -> usize {
    let token = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
    match SUBSCRIBERS.lock() {
        Ok(mut subs) => subs.push((token, cb)),
        Err(poisoned) => poisoned.into_inner().push((token, cb)),
    }
    token
}

/// Remove a subscriber by token. Unknown tokens are a no-op.
pub fn unsubscribe_value_changed(token: usize) {
    match SUBSCRIBERS.lock() {
        Ok(mut subs) => subs.retain(|(t, _)| *t != token),
        Err(poisoned) => poisoned.into_inner().retain(|(t, _)| *t != token),
    }
}

/// Notify every subscriber of a value change. Callers MUST NOT hold any
/// framework lock (deferred-dispatch contract). The subscriber list is
/// snapshot-cloned so callbacks run with no lock held here either.
pub(crate) fn dispatch(id: &str, side: u8, value: i32) {
    let snapshot: Vec<Arc<ValueChangedFn>> = {
        let subs = match SUBSCRIBERS.lock() {
            Ok(s) => s,
            Err(poisoned) => poisoned.into_inner(),
        };
        subs.iter().map(|(_, cb)| Arc::clone(cb)).collect()
    };
    for cb in snapshot {
        let contained = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            cb(id, side, value);
        }));
        if contained.is_err() && !PANIC_WARNED.swap(true, Ordering::Relaxed) {
            log_warn!("custom_options: value-changed subscriber panicked — contained");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    /// The subscriber list is process-global — serialize tests so one test's
    /// subscribers never observe another's dispatches. Poison-recovered: the
    /// panic-containment test panics inside a dispatch while holding nothing,
    /// but a failed assertion elsewhere must not cascade.
    static TEST_LOCK: StdMutex<()> = StdMutex::new(());

    fn lock_tests() -> std::sync::MutexGuard<'static, ()> {
        match TEST_LOCK.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    /// A recording subscriber: collects `(id, side, value)` into its own log.
    fn recorder() -> (Arc<StdMutex<Vec<(String, u8, i32)>>>, Arc<ValueChangedFn>) {
        let log = Arc::new(StdMutex::new(Vec::new()));
        let log2 = Arc::clone(&log);
        let cb: Arc<ValueChangedFn> = Arc::new(move |id: &str, side: u8, value: i32| {
            log2.lock().unwrap().push((id.to_string(), side, value));
        });
        (log, cb)
    }

    #[test]
    fn tokens_unique_and_unsubscribe_stops_delivery() {
        let _guard = lock_tests();
        let (log_a, cb_a) = recorder();
        let (log_b, cb_b) = recorder();
        let tok_a = subscribe_value_changed(cb_a);
        let tok_b = subscribe_value_changed(cb_b);
        assert_ne!(tok_a, tok_b);

        dispatch("x", 0, 1);
        unsubscribe_value_changed(tok_a);
        dispatch("x", 1, 2);
        unsubscribe_value_changed(tok_b);
        dispatch("x", 0, 3);

        assert_eq!(
            log_a.lock().unwrap().len(),
            1,
            "A: only the pre-unsub event"
        );
        assert_eq!(log_b.lock().unwrap().len(), 2, "B: both pre-unsub events");
    }

    #[test]
    fn all_subscribers_fire_in_subscription_order() {
        let _guard = lock_tests();
        let order = Arc::new(StdMutex::new(Vec::new()));
        let mut tokens = Vec::new();
        for tag in ["first", "second", "third"] {
            let order2 = Arc::clone(&order);
            tokens.push(subscribe_value_changed(Arc::new(
                move |_: &str, _: u8, _: i32| {
                    order2.lock().unwrap().push(tag);
                },
            )));
        }
        dispatch("y", 0, 7);
        for t in tokens {
            unsubscribe_value_changed(t);
        }
        assert_eq!(*order.lock().unwrap(), vec!["first", "second", "third"]);
    }

    #[test]
    fn panicking_subscriber_is_contained_and_later_ones_fire() {
        let _guard = lock_tests();
        let panicker: Arc<ValueChangedFn> = Arc::new(|_: &str, _: u8, _: i32| {
            panic!("subscriber bug");
        });
        let tok_panic = subscribe_value_changed(panicker);
        let (log, cb) = recorder();
        let tok_ok = subscribe_value_changed(cb);

        dispatch("z", 1, -5);

        unsubscribe_value_changed(tok_panic);
        unsubscribe_value_changed(tok_ok);
        assert_eq!(
            *log.lock().unwrap(),
            vec![("z".to_string(), 1u8, -5i32)],
            "the subscriber after the panicker must still fire"
        );
    }

    #[test]
    fn subscribing_during_dispatch_does_not_deadlock() {
        let _guard = lock_tests();
        // Proves dispatch holds no lock while calling subscribers: the
        // callback re-enters subscribe (which takes the list lock).
        let inner_token = Arc::new(StdMutex::new(None));
        let inner_token2 = Arc::clone(&inner_token);
        let tok = subscribe_value_changed(Arc::new(move |_: &str, _: u8, _: i32| {
            let noop: Arc<ValueChangedFn> = Arc::new(|_, _, _| {});
            let t = subscribe_value_changed(noop);
            *inner_token2.lock().unwrap() = Some(t);
        }));

        dispatch("w", 0, 0); // would deadlock if the list lock were held

        unsubscribe_value_changed(tok);
        let inner = inner_token.lock().unwrap().take();
        assert!(inner.is_some(), "inner subscribe must have completed");
        unsubscribe_value_changed(inner.unwrap());
    }
}
