//! Legacy song info — skin 2's (MAX-EXTREME) band and skins 3–5's A3 panel.
//!
//! World's `SongInfoActor` creates `dance_song_info_single` / `_double` (the
//! jacket + title card) from the record's package, then a `SongInfoChild`
//! that writes the title / artist / source into the card's `music_usr` /
//! `artist_usr` / `source_usr`. A3:
//!
//! * skin 1 — no song info (World's own `GameWork+0xA8 == 1` gate);
//! * skin 2 — `dance_song_info` from `dance_song_info0002`: a plain band at
//!   priority 9 with no text children (World's child then finds nothing to
//!   write, exactly like A3's);
//! * skins 3–5 — no package of their own: A3's own skin-0 panel
//!   `dance_song_info0000_v2` (World ships it byte-identical) at priority 5,
//!   title / artist centred in `music_name_usr` / `artist_name_usr` in white
//!   `2d_font_songtitle_m`.
//!
//! Mechanism (the Step 7 / 8 pattern): checked code patches, live exactly
//! while the current `LayoutActor`'s `dance_song_info` record is legacy —
//! applied by the package helper right before it registers the package (a
//! patch failure keeps the package stock: World's init NULL-derefs on a
//! package without its export), restored whenever `dance_song_info` goes
//! through the helper stock, at disarm and disable. The patch lists are the
//! pure [`song_info_logic::plan`]. No detour (center_arrows_single already
//! owns the init's; its dark-card style flip only selects between the two
//! card names — both patched — and the text colour — white either way here).
//!
//! Game thread only. RE:
//! `.agents/planning/2026-09-22-ddr-selection/research/legacy-score.md` §6 / §7.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};

use crate::core::memory;
use crate::core::signatures::SignatureStore;
use crate::{log_info, log_warn};

use super::song_info_logic::{self as logic, Buffers, CardSites, Mode, PanelSites, Patch};

struct Sites {
    card: CardSites,
    panel: Option<PanelSites>,
    buffers: Buffers,
}

static SITES: OnceLock<Sites> = OnceLock::new();
/// 0 = stock, else the applied [`Mode`] (1 band, 2 panel).
static APPLIED: AtomicU8 = AtomicU8::new(0);
static BROKEN: AtomicBool = AtomicBool::new(false);
static PANEL_BROKEN: AtomicBool = AtomicBool::new(false);
static LOCK: Mutex<()> = Mutex::new(());

fn mode_code(m: Option<Mode>) -> u8 {
    match m {
        None => 0,
        Some(Mode::Band) => 1,
        Some(Mode::Panel) => 2,
    }
}

fn read<const N: usize>(at: usize) -> [u8; N] {
    let mut b = [0u8; N];
    unsafe { std::ptr::copy_nonoverlapping(at as *const u8, b.as_mut_ptr(), N) };
    b
}

/// Resolve the sites and build the near buffer (mod init).
pub fn init(signatures: &SignatureStore) -> bool {
    let Some(s) = signatures.ddr_sel_song_info_sites() else {
        log_warn!("DDR SELECTION: legacy song-info site unresolved -- the song info stays World's");
        return false;
    };
    let site = s.site as usize;
    if read::<1>(site + logic::CARD_PRIORITY_IMM)[0] != logic::WORLD_PRIORITY {
        log_warn!("DDR SELECTION: song-info priority is not stock -- the song info stays World's");
        return false;
    }
    let buffer = unsafe { memory::alloc_near(s.site, 0x1000) } as usize;
    if buffer == 0 {
        log_warn!(
            "DDR SELECTION: no near buffer for the song-info export -- the song info stays World's"
        );
        return false;
    }
    let buffers = Buffers {
        export: buffer,
        music: buffer + 0x20,
        artist: buffer + 0x40,
    };
    unsafe {
        for (at, s) in [
            (buffers.export, logic::EXPORT),
            (buffers.music, logic::MUSIC_NAME),
            (buffers.artist, logic::ARTIST_NAME),
        ] {
            std::ptr::copy_nonoverlapping(s.as_ptr(), at as *mut u8, s.len());
        }
    }
    let card = CardSites {
        site,
        lea_stock: [
            read::<4>(site + logic::CARD_LEAS[0].0),
            read::<4>(site + logic::CARD_LEAS[1].0),
        ],
    };
    if logic::plan(Mode::Band, &card, None, &buffers).is_none() {
        log_warn!(
            "DDR SELECTION: song-info export out of rel32 reach -- the song info stays World's"
        );
        return false;
    }
    let panel = s.panel.and_then(|p| {
        let leas = p.name_leas.map(|l| l as usize);
        let panel = PanelSites {
            font_imm: p.font_imm as usize,
            font_stock: read::<1>(p.font_imm as usize)[0],
            color_jcc: p.color_jcc as usize,
            color_stock: read::<2>(p.color_jcc as usize),
            name_leas: leas,
            name_stock: leas.map(|l| read::<4>(l + 3)),
            align_store: p.align_store as usize,
            align_stock: read::<7>(p.align_store as usize),
            x_sub: p.x_offset_sub as usize,
            x_sub_stock: read::<2>(p.x_offset_sub as usize),
        };
        if logic::plan(Mode::Panel, &card, Some(&panel), &buffers).is_none() {
            log_warn!(
                "DDR SELECTION: song-info panel sites not in their stock shape (font {}, jcc {:02X}, align {:02X?}, sub {:02X?}) -- skins 3-5 keep World's card",
                panel.font_stock,
                panel.color_stock[0],
                panel.align_stock,
                panel.x_sub_stock
            );
            return None;
        }
        Some(panel)
    });
    if panel.is_none() && s.panel.is_none() {
        log_warn!("DDR SELECTION: song-info panel sites unresolved -- skins 3-5 keep World's card");
    }
    let _ = SITES.set(Sites {
        card,
        panel,
        buffers,
    });
    true
}

/// Whether skin 2's band can be used on this boot (the `SongInfo` adapter).
pub fn capable() -> bool {
    SITES.get().is_some() && !BROKEN.load(Ordering::Acquire)
}

/// Whether skins 3–5's A3 panel can be used on this boot (the
/// `SongInfoPanel` adapter).
pub fn panel_capable() -> bool {
    capable()
        && !PANEL_BROKEN.load(Ordering::Acquire)
        && SITES.get().is_some_and(|s| s.panel.is_some())
}

/// Apply `patches` (stock → legacy when `forward`), all or nothing.
fn apply_all(patches: &[Patch], forward: bool) -> bool {
    let mut done: Vec<&Patch> = Vec::new();
    for p in patches {
        let (old, new) = if forward {
            (&p.stock, &p.legacy)
        } else {
            (&p.legacy, &p.stock)
        };
        if let Err(e) = unsafe { memory::apply_checked_patch(p.at as *mut u8, old, new) } {
            log_warn!(
                "DDR SELECTION: song-info patch at {:#x} failed ({:?}) -- the song info stays World's",
                p.at,
                e
            );
            for d in done.into_iter().rev() {
                let (o, n) = if forward {
                    (&d.stock, &d.legacy)
                } else {
                    (&d.legacy, &d.stock)
                };
                let _ = unsafe { memory::apply_checked_patch(d.at as *mut u8, n, o) };
            }
            return false;
        }
        done.push(p);
    }
    true
}

fn plan_for(st: &Sites, mode: Mode) -> Option<Vec<Patch>> {
    logic::plan(mode, &st.card, st.panel.as_ref(), &st.buffers)
}

/// Switch the patched state to `target` (`None` = World's).
fn switch(target: Option<Mode>) -> bool {
    let Some(st) = SITES.get() else {
        return target.is_none();
    };
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let current = APPLIED.load(Ordering::Acquire);
    if current == mode_code(target) {
        return true;
    }
    let current_mode = match current {
        1 => Some(Mode::Band),
        2 => Some(Mode::Panel),
        _ => None,
    };
    if let Some(m) = current_mode {
        let ok = plan_for(st, m).is_some_and(|p| apply_all(&p, false));
        if !ok {
            BROKEN.store(true, Ordering::Release);
            return false;
        }
        APPLIED.store(0, Ordering::Release);
    }
    let Some(m) = target else {
        return true;
    };
    let ok = plan_for(st, m).is_some_and(|p| apply_all(&p, true));
    if !ok {
        match m {
            Mode::Band => BROKEN.store(true, Ordering::Release),
            Mode::Panel => PANEL_BROKEN.store(true, Ordering::Release),
        }
        return false;
    }
    APPLIED.store(mode_code(Some(m)), Ordering::Release);
    match m {
        Mode::Band => {
            log_info!("DDR SELECTION: song-info actor -> A3 export dance_song_info (priority 9)")
        }
        Mode::Panel => log_info!(
            "DDR SELECTION: song-info actor -> A3 panel (export dance_song_info, priority 5; child: music_name_usr / artist_name_usr, font 3, centred, white)"
        ),
    }
    true
}

/// Patch in A3's song info for `skin` (package helper, before it registers
/// the legacy package). `false` ⇒ keep `dance_song_info` stock.
pub fn apply(skin: u8) -> bool {
    match logic::mode_for_skin(skin) {
        Some(Mode::Band) => capable() && switch(Some(Mode::Band)),
        Some(Mode::Panel) => panel_capable() && switch(Some(Mode::Panel)),
        None => false,
    }
}

/// World's names back (a stock `dance_song_info`, disarm, disable).
pub fn restore() {
    if APPLIED.load(Ordering::Acquire) != 0 && !switch(None) {
        log_warn!("DDR SELECTION: could not restore World's song-info names");
    }
}
