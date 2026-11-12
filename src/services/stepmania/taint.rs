//! Primary score taint for injected songs (design §4.13, this step's slice).
//!
//! The gameplay-load signal is the FULL-variant bank request: the game opens
//! `sound/win/dance/<b>.xwb` outside the song wheel only when it is about to
//! play `<b>` (`DancePlaySequence::onSetup`), so the resolver reports every
//! full-variant request here — injected AND stock basenames. The injected
//! flag in `score_guard` is **level-written** from that stream: an injected
//! basename sets it, a stock basename clears it. That is deliberate (and the
//! one place this deviates from the design's "cleared by `reset_song_taint`"
//! wording): `reset_song_taint` fires on the scene-28 entry callback — which
//! runs BEFORE `createNextSequence` builds the DancePlaySequence that opens
//! the bank, so ordering happens to work there — but it ALSO fires on an
//! in-place quick restart, which never re-opens the bank; a per-song clear
//! would launder the taint on a restart of an injected song. A level mirror
//! has no such window; `reset_session` (card-in) clears it like the other
//! mirrors.
//!
//! Alongside the per-song flag, the entered non-bot sides get
//! `mark_session_tainted` so the card-out logout save is sanitised (Step 7
//! narrows the sanitiser to injected records). With `stage_records`
//! unavailable both sides are marked — fail closed.
//!
//! The same per-change edge drives the display-only scroll-gimmick tables
//! (`super::on_gameplay_load` → `services::scroll_remap`): an injected
//! basename publishes its tables, a stock one clears them.

use std::sync::Mutex;

use crate::mods::multiplayer_bot;
use crate::services::{score_guard, stage_records};
use crate::{log_info, log_warn};

use super::cache::BankVariant;

/// The basename whose full bank was requested last (for one INFO per
/// change, not per lstat/open pair).
static LAST_FULL: Mutex<Option<String>> = Mutex::new(None);

/// Called by the resolver for every `sound/win/dance/<b>.xwb` request it
/// classifies. Only full-variant requests matter.
pub fn note_bank_request(basename: &str, injected: bool, variant: BankVariant) {
    if variant != BankVariant::Full {
        return;
    }
    let changed = match LAST_FULL.lock() {
        Ok(mut last) => {
            let changed = last.as_deref() != Some(basename);
            if changed {
                *last = Some(basename.to_string());
            }
            changed
        }
        Err(_) => true,
    };
    // Level-triggered (like the taint flag below): a replay of the same
    // song after a scene-exit clear must re-publish; `publish` dedupes.
    super::on_gameplay_load(basename, injected);
    if !injected {
        score_guard::set_injected_song_taint(false);
        return;
    }
    score_guard::set_injected_song_taint(true);
    let sides = sides_to_mark();
    for &side in &sides {
        score_guard::mark_session_tainted(side);
    }
    if changed {
        log_info!(
            "StepMania: injected song '{}' loaded for gameplay -- stage save suppressed, session tainted for side(s) {:?}",
            basename,
            sides
        );
        if !score_guard::is_available() {
            log_warn!(
                "StepMania: score_guard save hook is not installed -- the taint cannot suppress anything this boot"
            );
        }
    }
}

/// Entered, non-bot sides; both sides when the player-work chain cannot be
/// read (fail closed on score integrity).
fn sides_to_mark() -> Vec<usize> {
    if !stage_records::is_available() {
        return vec![0, 1];
    }
    let entered = [
        stage_records::side_entered(0),
        stage_records::side_entered(1),
    ];
    let any = entered.iter().any(|e| *e == Some(true));
    let mut out = Vec::with_capacity(2);
    for (side, e) in entered.iter().enumerate() {
        let take = if any { *e == Some(true) } else { true };
        if take && !multiplayer_bot::is_bot_side(side) {
            out.push(side);
        }
    }
    if out.is_empty() {
        // Every candidate was the bot side (or none decoded): still taint
        // both — a bot-only pairing cannot have a human whose save this
        // would wrongly suppress.
        out = vec![0, 1];
    }
    out
}
