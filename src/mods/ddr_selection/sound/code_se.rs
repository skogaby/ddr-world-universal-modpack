//! Silence World's CODE-played sounds that a legacy clip already plays.
//!
//! A World actor sometimes plays its own sound from code next to its clip
//! (FullcomboActor: `se_game_fullcombo`; the ShutterActor's stage-panel
//! reveal: `vo_ingame_ready`). A3 played none — the legacy clip carries its
//! own embedded sound (`XAC_full_combo2`; `dance_message000N`'s `00_ready`
//! voice), routed by [`super::afp_route`] — so on a legacy song both would
//! sound (and the legacy stage panel brings its own stage call, replacing
//! World's `vo_stage_*`). Each [`Site`] is a set of one-byte branch flips inside the World
//! function (`JZ` → `JMP`, rel8 kept) onto the path World already takes when
//! the bank is absent (the play is skipped, the AVS lock still released);
//! applied while its gate holds for the current song, restored otherwise.
//!
//! Game thread only: [`sync`] runs from the package helper (where a package
//! turns legacy), the legacy intro (when its clips appear / go), arm /
//! disarm and every armed scene change (scene callbacks); the patched
//! functions run on the same thread, so no instruction is ever rewritten
//! under an executing caller. `memory::apply_checked_patch` verifies the
//! expected byte, so a site is never written twice or over foreign bytes.
//!
//! Fail-open: an unresolved site keeps World's sound (it doubles; cosmetic),
//! one WARN at init.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::core::memory;
use crate::core::signatures::SignatureStore;
use crate::{log_info, log_warn};

/// `JZ rel8` (stock) / `JMP rel8` (suppressed).
const OP_JZ: u8 = 0x74;
const OP_JMP: u8 = 0xEB;
const MAX_ADDRS: usize = 2;

struct Site {
    /// The World cue the site plays (logs).
    cue: &'static str,
    /// Why it is silenced (logs).
    because: &'static str,
    /// Whether World's sound must be silenced for the current song.
    gate: fn() -> bool,
    addrs: [AtomicUsize; MAX_ADDRS],
    applied: AtomicBool,
}

impl Site {
    const fn new(cue: &'static str, because: &'static str, gate: fn() -> bool) -> Self {
        Self {
            cue,
            because,
            gate,
            addrs: [const { AtomicUsize::new(0) }; MAX_ADDRS],
            applied: AtomicBool::new(false),
        }
    }
}

/// FullcomboActor::onMessage: the legacy `dance_fullcombo000N` clip plays
/// `XAC_full_combo2` itself — silence World's only when that cue can play.
fn fullcombo_gate() -> bool {
    super::super::legacy_package("dance_fullcombo") && replacement_playable("XAC_full_combo2")
}

/// ShutterActor update: the legacy READY clip (`dance_message000N`
/// `00_ready`) carries the era READY voice; World's `vo_ingame_ready` is a
/// World-intro element (D21) — silenced while the legacy intro is live.
fn ready_voice_gate() -> bool {
    super::super::intro::legacy_intro_live()
}

/// ShutterActor stage-panel swap: World's stage call (`vo_stage_*`) — A3's
/// legacy panel plays its own era call at the stage clip's `voice` label
/// (`panel.rs`), so World's is silenced while A3's root is live in the stage
/// slot (adopted at state 1 → 2; World's voice plays at the later 2 → 3
/// swap).
fn stage_voice_gate() -> bool {
    super::super::panel::root_live()
}

const FULL_COMBO: usize = 0;
const READY_VOICE: usize = 1;
const STAGE_VOICE: usize = 2;

static SITES: [Site; 3] = [
    Site::new(
        "se_game_fullcombo",
        "the legacy dance_fullcombo clip plays XAC_full_combo2",
        fullcombo_gate,
    ),
    Site::new(
        "vo_ingame_ready",
        "the legacy dance_message READY clip carries the era voice",
        ready_voice_gate,
    ),
    Site::new(
        "stage voice (vo_stage_*)",
        "the legacy stage panel plays the era stage call",
        stage_voice_gate,
    ),
];

/// Whether the legacy clip's own `cue` will actually play.
fn replacement_playable(cue: &str) -> bool {
    super::afp_route::is_installed()
        && super::bank::slot().is_some()
        && super::bank::lookup(cue.as_bytes()).is_some()
}

/// Resolve every site (mod init). Each branch byte is a derived address
/// (`derive_ddr_sel_code_se` / `derive_ddr_sel_vo_ready`), re-checked here
/// for the stock `JZ`.
pub fn init(signatures: &SignatureStore) {
    resolve(&SITES[FULL_COMBO], &["ddr_sel_fullcombo_se_jz"], signatures);
    resolve(
        &SITES[READY_VOICE],
        &["ddr_sel_vo_ready_jz_0", "ddr_sel_vo_ready_jz_1"],
        signatures,
    );
    resolve(&SITES[STAGE_VOICE], &["ddr_sel_stage_voice_jz"], signatures);
}

/// All-or-nothing per site: every named byte must resolve to a stock `JZ`.
fn resolve(s: &Site, names: &[&str], signatures: &SignatureStore) {
    let mut found = [0usize; MAX_ADDRS];
    for (i, name) in names.iter().enumerate().take(MAX_ADDRS) {
        match signatures.get_address(name) {
            Some(p) if unsafe { memory::read_u8(p) } == OP_JZ => found[i] = p as usize,
            Some(_) => {
                log_warn!(
                    "DDR SELECTION: {} is not the stock JZ -- World's {} doubles the legacy clip's sound",
                    name,
                    s.cue
                );
                return;
            }
            None => {
                log_warn!(
                    "DDR SELECTION: {} unresolved -- World's {} doubles the legacy clip's sound",
                    name,
                    s.cue
                );
                return;
            }
        }
    }
    for (slot, addr) in s.addrs.iter().zip(found) {
        slot.store(addr, Ordering::Release);
    }
}

/// Apply / restore every site to match the current song (game thread).
/// Idempotent; cheap when nothing changes.
pub fn sync() {
    for s in &SITES {
        if s.addrs[0].load(Ordering::Acquire) == 0 {
            continue;
        }
        let want = (s.gate)();
        if want == s.applied.load(Ordering::Acquire) {
            continue;
        }
        let (from, to) = if want {
            (OP_JZ, OP_JMP)
        } else {
            (OP_JMP, OP_JZ)
        };
        let mut done = 0usize;
        let mut failed = None;
        for a in &s.addrs {
            let addr = a.load(Ordering::Acquire);
            if addr == 0 {
                continue;
            }
            match unsafe { memory::apply_checked_patch(addr as *mut u8, &[from], &[to]) } {
                Ok(()) => done += 1,
                Err(e) => {
                    failed = Some(e);
                    break;
                }
            }
        }
        if let Some(e) = failed {
            // Put back whatever this pass already flipped, then disable the
            // site for the session (its bytes are not what we expect).
            for a in s.addrs.iter().take(done) {
                let addr = a.load(Ordering::Acquire);
                let _ = unsafe { memory::apply_checked_patch(addr as *mut u8, &[to], &[from]) };
            }
            for a in &s.addrs {
                a.store(0, Ordering::Release);
            }
            log_warn!(
                "DDR SELECTION: {} branch patch failed ({:?}) -- site left alone",
                s.cue,
                e
            );
            continue;
        }
        s.applied.store(want, Ordering::Release);
        if want {
            log_info!(
                "DDR SELECTION: World's {} silenced for this song ({})",
                s.cue,
                s.because
            );
        }
    }
}
