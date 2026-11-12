//! The judgement-offset composition rule (pure, `std` only — harness-mounted
//! by `scripts/validate_judgement_offsets.sh`).
//!
//! What a side's `Option+0x24` (`timing_music`) is set to for a song:
//!
//! ```text
//! applied = (player's per-song entry for the song, else the stock value)
//!           + song delta (auto-sync, else 0)
//!           clamped to ±TIMING_LIMIT
//! ```
//!
//! The per-song entry is the player's own override (CSV baseline / session
//! map); the song delta is a cabinet-wide measurement supplied by a provider
//! (`services::stepmania::sync_apply`, positive = judged later). The delta is
//! invisible in the per-song rows and never persisted with the player's
//! data. With neither an entry nor a delta there is nothing to apply and the
//! stock value stays — [`compose`] returns `None` so the caller writes
//! nothing.

/// The stock option domain (±100 ms).
pub const TIMING_LIMIT: i32 = 100;

/// See the module docs. `stock` is the value read from the game.
#[must_use]
pub fn compose(entry: Option<i8>, stock: i32, delta: Option<i8>) -> Option<i32> {
    if entry.is_none() && delta.is_none() {
        return None;
    }
    let base = entry.map(i32::from).unwrap_or(stock);
    let value = base + delta.map(i32::from).unwrap_or(0);
    Some(value.clamp(-TIMING_LIMIT, TIMING_LIMIT))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_to_apply_without_entry_or_delta() {
        assert_eq!(compose(None, 0, None), None);
        assert_eq!(compose(None, -37, None), None);
    }

    #[test]
    fn entry_alone_replaces_stock() {
        assert_eq!(compose(Some(12), 0, None), Some(12));
        assert_eq!(compose(Some(12), -50, None), Some(12));
        assert_eq!(
            compose(Some(0), 7, None),
            Some(0),
            "an entry of 0 is still an override"
        );
        assert_eq!(compose(Some(-100), 7, None), Some(-100));
    }

    #[test]
    fn delta_alone_adds_to_stock() {
        assert_eq!(compose(None, 0, Some(9)), Some(9));
        assert_eq!(compose(None, -5, Some(9)), Some(4));
        assert_eq!(compose(None, 20, Some(-60)), Some(-40));
        assert_eq!(
            compose(None, 3, Some(0)),
            Some(3),
            "measured in sync still writes stock"
        );
    }

    #[test]
    fn entry_plus_delta_ignores_stock() {
        assert_eq!(compose(Some(12), -50, Some(9)), Some(21));
        assert_eq!(compose(Some(-3), 100, Some(-4)), Some(-7));
    }

    #[test]
    fn clamped_to_the_option_domain() {
        assert_eq!(compose(Some(95), 0, Some(20)), Some(100));
        assert_eq!(compose(None, -90, Some(-60)), Some(-100));
        assert_eq!(compose(Some(100), 0, Some(127)), Some(100));
        assert_eq!(compose(Some(-100), 0, Some(-128)), Some(-100));
        assert_eq!(compose(None, 100, Some(1)), Some(100));
    }
}
