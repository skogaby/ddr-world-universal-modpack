//! Custom-option row ordering + menu-placement overrides.
//!
//! Owns the operator's `custom_options.option_menu_settings` (from
//! `mod-config.json`) and the pure logic that turns it into (a) a display
//! permutation over the registered options and (b) per-id menu-placement
//! overrides for the in-game and overlay menus.
//!
//! Row display order is otherwise implicit registration order: the builder
//! hook iterates the registry in registration order and injects rows in that
//! order (and `rows::ROWS` / the scroll driver follow it). This module lets an
//! operator override that order without touching the registry — the builder
//! hook applies [`display_order_for`] to its per-open snapshot, leaving
//! `registry::STATE.options` and every [`super::api::OptionHandle`] index
//! stable. The overlay menu consumes the same permutation through the
//! overlay snapshot.
//!
//! Each configured entry is `{ "id": "...", "overlay": bool?, "in_game":
//! bool? }` — array order = display order in BOTH menus; the optional flags
//! override the option's registered [`super::api::MenuPlacement`] (config
//! wins; omitted flags inherit the registration default; `false`/`false` =
//! hidden everywhere). Placement ENFORCEMENT lives with the consumers
//! (`builder_hook` for in-game, the overlay snapshot for the overlay) via
//! [`placement_override_for`].
//!
//! Ordering rules (see the overlay-menu rewrite design §4.4):
//!   - Listed ids render first, in the listed order.
//!   - Any registered NON-HEADER option NOT listed falls to the end, keeping
//!     its current registration order.
//!   - A registered HEADER option NOT listed is EXCLUDED from the result
//!     entirely (R10: decorative headers render only when the operator placed
//!     them — an unlisted header must not orphan itself at the end).
//!   - A listed id matching no registered option is logged once and ignored
//!     (never fatal) — it may be a typo, or a disabled mod / asset absent this
//!     boot.
//!   - Ids are matched case-insensitively (ASCII); duplicate entries place
//!     the row once and resolve placement once (first occurrence wins).
//!   - Absent or empty `option_menu_settings` ⇒ identity for normal rows
//!     (current registration order); headers are all unlisted, hence all
//!     excluded; no placement overrides.
//!
//! The legacy `custom_options.row_order` key is GONE (design D17): it is no
//! longer read anywhere, and a leftover key in operator JSON is silently
//! ignored by serde.

use once_cell::sync::OnceCell;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::log_warn;

/// One operator-configured entry: display position (by array order) plus
/// optional per-menu placement overrides. Plain data — the serde twin lives
/// in `crate::mods::config` and is converted at read time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OptionMenuSetting {
    /// Option id (stored ASCII-lowercased; matched case-insensitively).
    pub id: String,
    /// Override for the overlay menu (`None` = inherit registration default).
    pub overlay: Option<bool>,
    /// Override for the in-game menu (`None` = inherit registration default).
    pub in_game: Option<bool>,
}

/// The operator's configured settings, ids ASCII-lowercased at store time so
/// matching is a simple `eq_ignore_ascii_case`. Empty (or unset) is treated
/// as identity/no-overrides.
static CONFIGURED: OnceCell<Vec<OptionMenuSetting>> = OnceCell::new();

/// Warn-once latch for unknown ids. The builder hook fires on every menu open,
/// so unlatched logging would spam the same warning repeatedly.
static UNKNOWN_WARNED: AtomicBool = AtomicBool::new(false);

/// Store the operator's configured settings. Called once from
/// [`super::init`] with `custom_options.option_menu_settings` (or an empty
/// vec when the key is absent). Ids are lowercased here; an empty vec is
/// stored as-is and later treated as identity.
pub(crate) fn set_configured_settings(settings: Vec<OptionMenuSetting>) {
    let lowered: Vec<OptionMenuSetting> = settings
        .into_iter()
        .map(|mut s| {
            s.id = s.id.to_ascii_lowercase();
            s
        })
        .collect();
    // Ignore a double-set: init is one-shot, but be defensive.
    let _ = CONFIGURED.set(lowered);
}

/// Pure permutation logic — the full display-order policy, unconfigured fast
/// path included (so the whole thing is host-testable). `registered` is the
/// option ids in display-candidate order (the builder hook's per-open
/// snapshot); `is_header` is the parallel header mask (`is_header[i]`
/// describes `registered[i]`; indices past its end are treated as normal
/// rows); `configured` is the operator's settings, ids already
/// ASCII-lowercased, or `None` when nothing is configured (an empty list is
/// equivalent).
///
/// Returns the ordered subset of `0..registered.len()` in display order, plus
/// the configured ids that matched no registered option. Normal rows keep the
/// shipped policy byte-identically (listed first, unlisted appended in input
/// order, identity when unconfigured); HEADERS appear only where listed —
/// an unlisted header is dropped from the result (R10).
///
/// Side-effect-free so the ordering rules live in one reviewable place.
fn compute_order(
    registered: &[&str],
    is_header: &[bool],
    configured: Option<&[OptionMenuSetting]>,
) -> (Vec<usize>, Vec<String>) {
    let n = registered.len();
    let header_at = |idx: usize| is_header.get(idx).copied().unwrap_or(false);

    // Unconfigured (or empty) fast path: identity for normal rows — the
    // pre-header behavior, byte-identical when no header is registered —
    // with every (necessarily unlisted) header excluded.
    let configured = match configured {
        Some(c) if !c.is_empty() => c,
        _ => return ((0..n).filter(|&idx| !header_at(idx)).collect(), Vec::new()),
    };

    let mut placed = vec![false; n];
    let mut order: Vec<usize> = Vec::with_capacity(n);
    let mut unknown: Vec<String> = Vec::new();

    // 1. Listed ids first, in listed order (headers included — a listed
    //    header takes its listed position like any row).
    for setting in configured {
        // Option ids are unique in the registry, so at most one match. The
        // setting id is already lowercased; registered ids are snake_case
        // ASCII.
        match registered
            .iter()
            .position(|id| id.eq_ignore_ascii_case(&setting.id))
        {
            Some(idx) if !placed[idx] => {
                placed[idx] = true;
                order.push(idx);
            }
            // Duplicate id in the configured list — first occurrence already
            // placed it; ignore the repeat.
            Some(_) => {}
            // No such registered option — collect for the warn-once, ignore.
            None => unknown.push(setting.id.clone()),
        }
    }

    // 2. Unlisted registered options, appended in registration order —
    //    EXCEPT headers, which are excluded when unlisted (R10).
    for (idx, done) in placed.iter().enumerate() {
        if !done && !header_at(idx) {
            order.push(idx);
        }
    }

    (order, unknown)
}

/// Pure placement-override lookup: the configured `(in_game, overlay)`
/// overrides for `id`, or `(None, None)` when nothing is configured, the id
/// is unlisted, or the entry carries no flags. Case-insensitive; duplicate
/// entries resolve to the FIRST occurrence (matching the ordering rule).
fn placement_override(
    configured: Option<&[OptionMenuSetting]>,
    id: &str,
) -> (Option<bool>, Option<bool>) {
    let Some(configured) = configured else {
        return (None, None);
    };
    configured
        .iter()
        .find(|s| s.id.eq_ignore_ascii_case(id))
        .map(|s| (s.in_game, s.overlay))
        .unwrap_or((None, None))
}

/// Runtime placement-override query for consumers (`builder_hook` filters
/// `!in_game` rows; the overlay snapshot filters `!overlay`). Returns
/// `(in_game, overlay)` overrides; `None` legs inherit the option's
/// registered `MenuPlacement`. Config wins over registration.
pub(crate) fn placement_override_for(id: &str) -> (Option<bool>, Option<bool>) {
    placement_override(CONFIGURED.get().map(|c| c.as_slice()), id)
}

/// Compute the display order for `ids` (option ids in the builder hook's
/// per-open snapshot order), with `is_header` the parallel header mask.
/// Returns the ordered subset of `0..ids.len()` — for normal rows a full
/// permutation; header indices appear only where their id is listed in the
/// operator's `option_menu_settings` (an unlisted header is excluded — R10).
///
/// Identity fast-path (minus headers) when nothing is configured or the list
/// is empty, so the unconfigured header-free case is byte-for-byte the
/// shipped behavior. Emits a single WARN listing any configured ids that
/// matched no registered option.
pub(crate) fn display_order_for(ids: &[&str], is_header: &[bool]) -> Vec<usize> {
    let configured = CONFIGURED.get().map(|c| c.as_slice());
    let (order, mut unknown) = compute_order(ids, is_header, configured);

    if !unknown.is_empty() && !UNKNOWN_WARNED.swap(true, Ordering::AcqRel) {
        unknown.sort();
        unknown.dedup();
        log_warn!(
            "custom_options/option_menu_settings: ignoring {} id(s) with no registered option: {:?} \
             (a typo, or a disabled mod / asset not present this boot)",
            unknown.len(),
            unknown
        );
    }

    order
}

#[cfg(test)]
mod tests {
    use super::{compute_order, placement_override, OptionMenuSetting};

    /// Order-only settings (no placement flags) from a list of ids — the
    /// direct analog of the legacy `row_order` array. Ids arrive
    /// ASCII-lowercased (set_configured_settings does it at store time);
    /// mirror that here.
    fn cfg(ids: &[&str]) -> Vec<OptionMenuSetting> {
        ids.iter()
            .map(|s| OptionMenuSetting {
                id: s.to_ascii_lowercase(),
                overlay: None,
                in_game: None,
            })
            .collect()
    }

    /// A full settings entry (id lowercased like the store path).
    fn entry(id: &str, in_game: Option<bool>, overlay: Option<bool>) -> OptionMenuSetting {
        OptionMenuSetting {
            id: id.to_ascii_lowercase(),
            overlay,
            in_game,
        }
    }

    const NO_HEADERS: &[bool] = &[false; 8];

    // ── Order semantics (carried forward from the row_order era) ─────

    #[test]
    fn identity_fast_path_without_headers_is_byte_identical() {
        // Unconfigured ⇒ registration order, untouched (the shipped behavior).
        let (order, unknown) = compute_order(&["a", "b", "c"], NO_HEADERS, None);
        assert_eq!(order, vec![0, 1, 2]);
        assert!(unknown.is_empty());
    }

    #[test]
    fn empty_configured_behaves_as_unconfigured() {
        let empty: Vec<OptionMenuSetting> = Vec::new();
        let (order, unknown) = compute_order(&["a", "b"], NO_HEADERS, Some(&empty));
        assert_eq!(order, vec![0, 1]);
        assert!(unknown.is_empty());

        // ... including the header-exclusion leg.
        let (order, _) = compute_order(&["a", "hdr"], &[false, true], Some(&empty));
        assert_eq!(order, vec![0]);
    }

    #[test]
    fn unconfigured_header_is_excluded_not_appended() {
        // R10: with no settings every header is unlisted ⇒ absent entirely;
        // normal rows keep pure registration order.
        let (order, unknown) = compute_order(&["a", "hdr", "b"], &[false, true, false], None);
        assert_eq!(order, vec![0, 2]);
        assert!(unknown.is_empty());
    }

    #[test]
    fn listed_header_takes_its_listed_position() {
        let configured = cfg(&["hdr", "a"]);
        let (order, unknown) =
            compute_order(&["a", "b", "hdr"], &[false, false, true], Some(&configured));
        // hdr first (listed), a second (listed), unlisted normal b appended.
        assert_eq!(order, vec![2, 0, 1]);
        assert!(unknown.is_empty());
    }

    #[test]
    fn unlisted_header_is_excluded_from_a_configured_order() {
        let configured = cfg(&["b", "a"]);
        let (order, unknown) =
            compute_order(&["a", "hdr", "b"], &[false, true, false], Some(&configured));
        // b, a (listed); hdr does NOT fall to the end (unlike a normal row).
        assert_eq!(order, vec![2, 0]);
        assert!(unknown.is_empty());
    }

    #[test]
    fn normal_rows_keep_listed_first_unlisted_appended() {
        let configured = cfg(&["c", "a"]);
        let (order, unknown) = compute_order(&["a", "b", "c"], NO_HEADERS, Some(&configured));
        assert_eq!(order, vec![2, 0, 1]);
        assert!(unknown.is_empty());
    }

    #[test]
    fn unknown_ids_are_collected_and_ignored() {
        let configured = cfg(&["ghost", "a"]);
        let (order, unknown) = compute_order(&["a", "b"], NO_HEADERS, Some(&configured));
        assert_eq!(order, vec![0, 1]);
        assert_eq!(unknown, vec!["ghost".to_string()]);
    }

    #[test]
    fn duplicate_listed_id_places_once() {
        let configured = cfg(&["a", "a", "b"]);
        let (order, unknown) = compute_order(&["a", "b"], NO_HEADERS, Some(&configured));
        assert_eq!(order, vec![0, 1]);
        assert!(unknown.is_empty());
    }

    #[test]
    fn header_match_is_case_insensitive() {
        // Registered ids are matched case-insensitively against the (already
        // lowercased) configured list — headers included.
        let configured = cfg(&["HDR_Training"]);
        let (order, unknown) = compute_order(&["Hdr_Training"], &[true], Some(&configured));
        assert_eq!(order, vec![0]);
        assert!(unknown.is_empty());
    }

    // ── Placement overrides ──────────────────────────────────────────

    #[test]
    fn placement_unconfigured_and_unlisted_yield_none() {
        assert_eq!(placement_override(None, "a"), (None, None));
        let configured = vec![entry("a", Some(true), Some(false))];
        assert_eq!(placement_override(Some(&configured), "b"), (None, None));
    }

    #[test]
    fn placement_listed_without_flags_inherits() {
        // A pure-ordering entry carries no overrides.
        let configured = cfg(&["a"]);
        assert_eq!(placement_override(Some(&configured), "a"), (None, None));
    }

    #[test]
    fn placement_explicit_flags_reported_verbatim() {
        let configured = vec![
            entry("a", Some(false), None), // in-game hidden, overlay inherited
            entry("b", None, Some(true)),  // overlay forced on
            entry("c", Some(false), Some(false)), // the "neither" case — hidden everywhere
        ];
        assert_eq!(
            placement_override(Some(&configured), "a"),
            (Some(false), None)
        );
        assert_eq!(
            placement_override(Some(&configured), "b"),
            (None, Some(true))
        );
        assert_eq!(
            placement_override(Some(&configured), "c"),
            (Some(false), Some(false))
        );
    }

    #[test]
    fn placement_match_is_case_insensitive() {
        let configured = vec![entry("Song_Speed", Some(false), None)];
        assert_eq!(
            placement_override(Some(&configured), "SONG_SPEED"),
            (Some(false), None)
        );
    }

    #[test]
    fn placement_duplicate_entries_first_wins() {
        // Consistent with the ordering rule: the first occurrence governs.
        let configured = vec![entry("a", Some(false), None), entry("a", Some(true), None)];
        assert_eq!(
            placement_override(Some(&configured), "a"),
            (Some(false), None)
        );
    }

    #[test]
    fn placement_only_entry_still_takes_order_position() {
        // An entry present for placement participates in ordering identically.
        let configured = vec![entry("b", Some(false), None)];
        let (order, unknown) = compute_order(&["a", "b"], NO_HEADERS, Some(&configured));
        assert_eq!(order, vec![1, 0]);
        assert!(unknown.is_empty());
    }
}
