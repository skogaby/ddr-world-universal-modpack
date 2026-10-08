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
//!   - An unlisted NON-HEADER option that belongs to a `ShowWhen` FAMILY stays
//!     next to the family's listed member (2026-09-30): an unlisted parent is
//!     placed immediately before its first listed child; an unlisted child is
//!     placed immediately after the last already-placed member of its family
//!     (the parent or a sibling). A family with no listed member falls to the
//!     end in registration order as before. Operators' configs list only the
//!     rows they knew about, so rows discovered at runtime (the Background
//!     Dancers per-source model rows and their source row) follow their
//!     listed relative instead of stranding at the bottom.
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

/// Warn-once latch for unknown ids ([`warn_unknown_configured_ids_once`]).
/// Both menus fire on every open, so unlatched logging would spam the same
/// warning repeatedly.
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

/// Map each option's `ShowWhen` parent id onto its position in the same
/// snapshot: `parent_ids[i]` names `ids[i]`'s parent (or `None`); the result
/// holds that parent's index in `ids`, or `None` when it has no parent or the
/// parent is not in the snapshot (filtered out by availability / placement).
/// Exact match — option ids are unique.
pub(crate) fn parent_positions(ids: &[&str], parent_ids: &[Option<&str>]) -> Vec<Option<usize>> {
    ids.iter()
        .enumerate()
        .map(|(i, _)| {
            parent_ids
                .get(i)
                .copied()
                .flatten()
                .and_then(|p| ids.iter().position(|id| *id == p))
        })
        .collect()
}

/// Pure permutation logic — the full display-order policy, unconfigured fast
/// path included (so the whole thing is host-testable). `registered` is the
/// option ids in display-candidate order (the builder hook's per-open
/// snapshot); `is_header` is the parallel header mask (`is_header[i]`
/// describes `registered[i]`; indices past its end are treated as normal
/// rows); `parent` is the parallel `ShowWhen`-parent mask (`parent[i]` is the
/// snapshot index of `registered[i]`'s parent, `None` when it has none or the
/// parent is absent — see [`parent_positions`]; indices past its end are
/// parentless); `configured` is the operator's settings, ids already
/// ASCII-lowercased, or `None` when nothing is configured (an empty list is
/// equivalent).
///
/// Returns the ordered subset of `0..registered.len()` in display order, plus
/// the configured ids that matched no registered option. Listed rows come
/// first in listed order; an unlisted normal row joins its `ShowWhen` family
/// when a member of it is already placed (an unlisted parent right before its
/// first placed child, an unlisted child right after the family's last
/// placed member), else falls to the end in input order — so the
/// family-free policy is byte-identical to the shipped one (identity when
/// unconfigured); HEADERS appear only where listed — an unlisted header is
/// dropped from the result (R10).
///
/// Side-effect-free so the ordering rules live in one reviewable place.
fn compute_order(
    registered: &[&str],
    is_header: &[bool],
    parent: &[Option<usize>],
    configured: Option<&[OptionMenuSetting]>,
) -> (Vec<usize>, Vec<String>) {
    let n = registered.len();
    let header_at = |idx: usize| is_header.get(idx).copied().unwrap_or(false);
    let parent_of = |idx: usize| parent.get(idx).copied().flatten();

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

    // 2. Unlisted registered options in registration order — EXCEPT headers,
    //    which are excluded when unlisted (R10). A row whose ShowWhen family
    //    already has a placed member joins it; anything else is appended.
    let position_of = |order: &[usize], idx: usize| order.iter().position(|&o| o == idx);
    for idx in 0..n {
        if placed[idx] || header_at(idx) {
            continue;
        }
        let at = match parent_of(idx).filter(|&p| placed[p]) {
            // (1) The parent is placed: after the LAST placed family member
            //     (the parent or any placed sibling).
            Some(p) => {
                let last = (0..n)
                    .filter(|&j| j == p || (parent_of(j) == Some(p) && placed[j]))
                    .filter_map(|j| position_of(&order, j))
                    .max();
                last.map(|pos| pos + 1)
            }
            None => {
                // (2) A child of this row is placed: right before the FIRST
                //     placed child. (3) Otherwise: append.
                (0..n)
                    .filter(|&j| parent_of(j) == Some(idx) && placed[j])
                    .filter_map(|j| position_of(&order, j))
                    .min()
            }
        };
        match at {
            Some(pos) if pos <= order.len() => order.insert(pos, idx),
            _ => order.push(idx),
        }
        placed[idx] = true;
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
/// per-open snapshot order), with `is_header` the parallel header mask and
/// `parent` the parallel `ShowWhen`-parent positions ([`parent_positions`]).
/// Returns the ordered subset of `0..ids.len()` — for normal rows a full
/// permutation; header indices appear only where their id is listed in the
/// operator's `option_menu_settings` (an unlisted header is excluded — R10).
///
/// Identity fast-path (minus headers) when nothing is configured or the list
/// is empty, so the unconfigured header-free case is byte-for-byte the
/// shipped behavior. Does NOT warn about configured ids missing from `ids`:
/// every caller passes a menu-filtered subset (in-game-only rows are absent
/// from the overlay's snapshot and vice versa), so "not in this list" is not
/// "not registered". The one-shot unknown-id WARN is
/// [`warn_unknown_configured_ids_once`], fed the FULL registry.
pub(crate) fn display_order_for(
    ids: &[&str],
    is_header: &[bool],
    parent: &[Option<usize>],
) -> Vec<usize> {
    let configured = CONFIGURED.get().map(|c| c.as_slice());
    compute_order(ids, is_header, parent, configured).0
}

/// Configured `option_menu_settings` ids (lowercased at store time) that match
/// none of `registered` — the ids the ordering silently ignores. Sorted,
/// deduplicated; empty when nothing is configured.
pub(crate) fn unknown_configured_ids(
    configured: Option<&[OptionMenuSetting]>,
    registered: &[&str],
) -> Vec<String> {
    let Some(configured) = configured else {
        return Vec::new();
    };
    let mut unknown: Vec<String> = configured
        .iter()
        .filter(|s| !registered.iter().any(|id| id.eq_ignore_ascii_case(&s.id)))
        .map(|s| s.id.clone())
        .collect();
    unknown.sort();
    unknown.dedup();
    unknown
}

/// Emit the single WARN listing configured ids that matched no registered
/// option. `registered` must be EVERY registered option id (not a menu's
/// placement/availability-filtered subset — an in-game-only row that the
/// overlay never shows is still registered). Latched: the menus open many
/// times per session, and only the first call after all mods have registered
/// is informative, so callers fire it from a menu open (well after boot).
pub(crate) fn warn_unknown_configured_ids_once(registered: &[&str]) {
    if UNKNOWN_WARNED.load(Ordering::Acquire) {
        return;
    }
    let unknown = unknown_configured_ids(CONFIGURED.get().map(|c| c.as_slice()), registered);
    if unknown.is_empty() {
        return;
    }
    if UNKNOWN_WARNED.swap(true, Ordering::AcqRel) {
        return;
    }
    log_warn!(
        "custom_options/option_menu_settings: ignoring {} id(s) with no registered option: {:?} \
         (a typo, or a disabled mod / asset not present this boot)",
        unknown.len(),
        unknown
    );
}

#[cfg(test)]
mod tests {
    use super::{
        compute_order, parent_positions, placement_override, unknown_configured_ids,
        OptionMenuSetting,
    };

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
    const NO_PARENTS: &[Option<usize>] = &[None; 16];

    // ── Order semantics (carried forward from the row_order era) ─────

    #[test]
    fn identity_fast_path_without_headers_is_byte_identical() {
        // Unconfigured ⇒ registration order, untouched (the shipped behavior).
        let (order, unknown) = compute_order(&["a", "b", "c"], NO_HEADERS, NO_PARENTS, None);
        assert_eq!(order, vec![0, 1, 2]);
        assert!(unknown.is_empty());
    }

    #[test]
    fn empty_configured_behaves_as_unconfigured() {
        let empty: Vec<OptionMenuSetting> = Vec::new();
        let (order, unknown) = compute_order(&["a", "b"], NO_HEADERS, NO_PARENTS, Some(&empty));
        assert_eq!(order, vec![0, 1]);
        assert!(unknown.is_empty());

        // ... including the header-exclusion leg.
        let (order, _) = compute_order(&["a", "hdr"], &[false, true], NO_PARENTS, Some(&empty));
        assert_eq!(order, vec![0]);
    }

    #[test]
    fn unconfigured_header_is_excluded_not_appended() {
        // R10: with no settings every header is unlisted ⇒ absent entirely;
        // normal rows keep pure registration order.
        let (order, unknown) =
            compute_order(&["a", "hdr", "b"], &[false, true, false], NO_PARENTS, None);
        assert_eq!(order, vec![0, 2]);
        assert!(unknown.is_empty());
    }

    #[test]
    fn listed_header_takes_its_listed_position() {
        let configured = cfg(&["hdr", "a"]);
        let (order, unknown) = compute_order(
            &["a", "b", "hdr"],
            &[false, false, true],
            NO_PARENTS,
            Some(&configured),
        );
        // hdr first (listed), a second (listed), unlisted normal b appended.
        assert_eq!(order, vec![2, 0, 1]);
        assert!(unknown.is_empty());
    }

    #[test]
    fn unlisted_header_is_excluded_from_a_configured_order() {
        let configured = cfg(&["b", "a"]);
        let (order, unknown) = compute_order(
            &["a", "hdr", "b"],
            &[false, true, false],
            NO_PARENTS,
            Some(&configured),
        );
        // b, a (listed); hdr does NOT fall to the end (unlike a normal row).
        assert_eq!(order, vec![2, 0]);
        assert!(unknown.is_empty());
    }

    #[test]
    fn normal_rows_keep_listed_first_unlisted_appended() {
        let configured = cfg(&["c", "a"]);
        let (order, unknown) =
            compute_order(&["a", "b", "c"], NO_HEADERS, NO_PARENTS, Some(&configured));
        assert_eq!(order, vec![2, 0, 1]);
        assert!(unknown.is_empty());
    }

    #[test]
    fn unknown_ids_are_collected_and_ignored() {
        let configured = cfg(&["ghost", "a"]);
        let (order, unknown) =
            compute_order(&["a", "b"], NO_HEADERS, NO_PARENTS, Some(&configured));
        assert_eq!(order, vec![0, 1]);
        assert_eq!(unknown, vec!["ghost".to_string()]);
    }

    /// The unknown-id WARN is computed against the FULL registry, not a menu's
    /// filtered snapshot: an id registered but absent from one menu's list
    /// (an in-game-only row seen from the overlay) is not "unknown".
    /// Regression: the WARN used to ride `display_order_for`, so the first
    /// mod-menu open listed every in-game-only `customize_*` row as a typo.
    #[test]
    fn unknown_configured_ids_match_against_full_registry() {
        let configured = cfg(&["customize_lane_single", "ghost", "ghost", "a"]);
        // Overlay snapshot sees only "a"; the registry also holds the
        // in-game-only "customize_lane_single".
        let all_registered = ["a", "customize_lane_single", "b"];
        assert_eq!(
            unknown_configured_ids(Some(&configured), &all_registered),
            vec!["ghost".to_string()]
        );
        assert!(unknown_configured_ids(None, &all_registered).is_empty());
        assert!(unknown_configured_ids(Some(&[]), &all_registered).is_empty());
    }

    #[test]
    fn duplicate_listed_id_places_once() {
        let configured = cfg(&["a", "a", "b"]);
        let (order, unknown) =
            compute_order(&["a", "b"], NO_HEADERS, NO_PARENTS, Some(&configured));
        assert_eq!(order, vec![0, 1]);
        assert!(unknown.is_empty());
    }

    #[test]
    fn header_match_is_case_insensitive() {
        // Registered ids are matched case-insensitively against the (already
        // lowercased) configured list — headers included.
        let configured = cfg(&["HDR_Training"]);
        let (order, unknown) =
            compute_order(&["Hdr_Training"], &[true], NO_PARENTS, Some(&configured));
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
        let (order, unknown) =
            compute_order(&["a", "b"], NO_HEADERS, NO_PARENTS, Some(&configured));
        assert_eq!(order, vec![1, 0]);
        assert!(unknown.is_empty());
    }

    // ── ShowWhen families (2026-09-30) ───────────────────────────────

    /// The shipped `mod-config.json` shape: the old two Background Dancers ids
    /// are listed, the new source rows (parents) and per-source model rows
    /// (children) are not. Every family member must land next to its listed
    /// anchor — the parent right before its listed child, the unlisted
    /// children right after the last placed member — instead of at the end.
    #[test]
    fn family_shipped_config_scenario() {
        let registered = [
            "hdr",
            "premium_free",
            "ddr_selection",
            "background_dancer_source",
            "background_dancer",
            "background_dancer_a",
            "background_dancer_b",
            "background_stage_source",
            "background_stage",
            "background_stage_a",
            "header_training",
            "training_x",
        ];
        let is_header = [
            true, false, false, false, false, false, false, false, false, false, true, false,
        ];
        let parent = [
            None,
            None,
            None,
            None,
            Some(3),
            Some(3),
            Some(3),
            None,
            Some(7),
            Some(7),
            None,
            None,
        ];
        let configured = cfg(&[
            "hdr",
            "premium_free",
            "ddr_selection",
            "background_dancer",
            "background_stage",
            "header_training",
            "training_x",
        ]);
        let (order, unknown) = compute_order(&registered, &is_header, &parent, Some(&configured));
        assert_eq!(order, vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]);
        assert!(unknown.is_empty());
    }

    #[test]
    fn family_unconfigured_is_identity() {
        let parent = [None, None, Some(0), Some(0)];
        let (order, _) = compute_order(
            &["p", "hdr", "c1", "c2"],
            &[false, true, false, false],
            &parent,
            None,
        );
        assert_eq!(order, vec![0, 2, 3]);
        let empty: Vec<OptionMenuSetting> = Vec::new();
        let (order, _) = compute_order(
            &["p", "hdr", "c1", "c2"],
            &[false, true, false, false],
            &parent,
            Some(&empty),
        );
        assert_eq!(order, vec![0, 2, 3]);
    }

    #[test]
    fn family_listed_parent_gathers_unlisted_children() {
        // Registered p, q, c1, c2 (children of p); listed p, q ⇒ p, c1, c2, q.
        let parent = [None, None, Some(0), Some(0)];
        let configured = cfg(&["p", "q"]);
        let (order, _) = compute_order(
            &["p", "q", "c1", "c2"],
            NO_HEADERS,
            &parent,
            Some(&configured),
        );
        assert_eq!(order, vec![0, 2, 3, 1]);
    }

    #[test]
    fn family_parent_outside_snapshot_appends() {
        // c's parent is not in the snapshot (filtered out) ⇒ today's append.
        let parent = [None, None];
        let configured = cfg(&["a"]);
        let (order, _) = compute_order(&["c", "a"], NO_HEADERS, &parent, Some(&configured));
        assert_eq!(order, vec![1, 0]);
    }

    #[test]
    fn family_rule_leaves_headers_excluded() {
        // hdr unlisted ⇒ excluded (R10); p unlisted parent of the listed c ⇒
        // placed right before c; q listed after c stays after.
        let parent = [None, None, Some(1), None];
        let configured = cfg(&["c", "q"]);
        let (order, _) = compute_order(
            &["hdr", "p", "c", "q"],
            &[true, false, false, false],
            &parent,
            Some(&configured),
        );
        assert_eq!(order, vec![1, 2, 3]);
    }

    #[test]
    fn family_all_unlisted_appends_in_registration_order() {
        // Nothing of the family is listed ⇒ the family lands at the end in
        // registration order (parent first — branch 3, then branch 1).
        let parent = [None, None, Some(1), Some(1)];
        let configured = cfg(&["z"]);
        let (order, _) = compute_order(
            &["z", "p", "c1", "c2"],
            NO_HEADERS,
            &parent,
            Some(&configured),
        );
        assert_eq!(order, vec![0, 1, 2, 3]);
    }

    #[test]
    fn family_children_follow_last_placed_sibling() {
        // Listed: p, c2, q (c2 listed out of registration order). Unlisted c1
        // goes after the LAST placed family member (c2), not right after p.
        let parent = [None, Some(0), Some(0), None];
        let configured = cfg(&["p", "c2", "q"]);
        let (order, _) = compute_order(
            &["p", "c1", "c2", "q"],
            NO_HEADERS,
            &parent,
            Some(&configured),
        );
        assert_eq!(order, vec![0, 2, 1, 3]);
    }

    #[test]
    fn parent_positions_maps_ids() {
        let got = parent_positions(&["a", "b", "c"], &[None, Some("a"), Some("zz")]);
        assert_eq!(got, vec![None, Some(0), None]);
        // Exact match (ids are unique, case-sensitive here).
        let got = parent_positions(&["A"], &[Some("a")]);
        assert_eq!(got, vec![None]);
    }
}
