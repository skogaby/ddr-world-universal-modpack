//! Legacy stage frame ("1st STAGE" …) — A3's `StageFrameActor` names on
//! World's actor.
//!
//! World's `StageFrameActor` is A3's with three names changed: the package
//! (`dance_stage`, which the package helper already maps to
//! `dance_stage_frame000N`), the export (`dance_stage` → A3 `stage_frame`)
//! and the texture prefix (`dast_stage_` → A3 `stage_frame%04d_stage_`).
//! World's stage-suffix logic (`01..05` / `final` / `extra` / …) and the
//! marker (`stage`, moved by [`super::markers`]) are unchanged.
//!
//! Mechanism: two checked code patches, live exactly while the current
//! `LayoutActor`'s `dance_stage` record is legacy — applied by the package
//! helper right before it registers `dance_stage_frame000N` (and the package
//! stays stock if either patch fails: World's init NULL-derefs on a package
//! without its export), restored whenever `dance_stage` goes through the
//! helper stock (every `LayoutActor` requests it before its StageFrameActor
//! initialises), at disarm and at disable:
//!
//! * the export `LEA R8,[rip+"dance_stage"]` (init + 0xC8) → a near-allocated
//!   `"stage_frame"`;
//! * the texture `MOV R8D,0xB; LEA RDX,[rip+"dast_stage_"]` → `22` / a
//!   near-allocated `"stage_frame000N_stage_"` (a theme: A3's skin-0
//!   `"stage_frame0000_stage_"`, [`policy::tex_number`]; one slot per skin).
//!
//! Game thread only (the helper runs in `LayoutActor::onInitialize`).
//! RE: `.agents/planning/2026-09-22-ddr-selection/research/
//! hud-layout-stage-frame.md` §4 / §8.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};

use crate::core::memory;
use crate::core::signatures::SignatureStore;
use crate::{log_info, log_warn};

use super::policy;

/// A3's export in `dance_stage_frame000N`.
const EXPORT: &[u8] = b"stage_frame\0";
/// Stride of the per-skin texture prefixes in the near buffer.
const PREFIX_STRIDE: usize = 0x20;
/// `stage_frame000N_stage_` (A3's `stage_frame%04d_stage_%s` minus the suffix).
const PREFIX_LEN: u32 = 22;
/// The near buffer holds the export at slot 0 and one prefix per skin.
const BUFFER_LEN: usize = 0x1000;
const _: () = assert!(PREFIX_STRIDE * (policy::SKIN_MAX as usize + 1) <= BUFFER_LEN);

struct Sites {
    export_lea: usize,
    texture_site: usize,
    buffer: usize,
    stock_export: [u8; 4],
    stock_texture: [u8; 11],
}

static SITES: OnceLock<Sites> = OnceLock::new();
/// Skin currently patched in (0 = stock bytes).
static APPLIED: AtomicU8 = AtomicU8::new(0);
/// A patch failed once — the stage frame stays World's for the session.
static BROKEN: AtomicBool = AtomicBool::new(false);
static LOCK: Mutex<()> = Mutex::new(());

fn prefix(skin: u8) -> String {
    format!("stage_frame{:04}_stage_", policy::tex_number(skin))
}

fn rel32(from_next: usize, to: usize) -> Option<i32> {
    i32::try_from(to as i64 - from_next as i64).ok()
}

/// Resolve the sites and build the near string buffer.
pub fn init(signatures: &SignatureStore) -> bool {
    let Some(s) = signatures.ddr_sel_stage_frame_sites() else {
        log_warn!("DDR SELECTION: stage-frame sites unresolved -- the stage frame stays World's");
        return false;
    };
    let (export_lea, texture_site) = (s.export_lea as usize, s.texture_site as usize);
    let buffer = unsafe { memory::alloc_near(s.export_lea, BUFFER_LEN) } as usize;
    if buffer == 0 {
        log_warn!(
            "DDR SELECTION: no near buffer for the stage-frame names -- stage frame stays World's"
        );
        return false;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(EXPORT.as_ptr(), buffer as *mut u8, EXPORT.len());
        for skin in 1..=policy::SKIN_MAX {
            let p = prefix(skin);
            debug_assert_eq!(p.len() as u32, PREFIX_LEN);
            let dst = (buffer + PREFIX_STRIDE * skin as usize) as *mut u8;
            std::ptr::copy_nonoverlapping(p.as_ptr(), dst, p.len());
            *dst.add(p.len()) = 0;
        }
    }
    let reach = [
        rel32(export_lea + 7, buffer),
        rel32(
            texture_site + 13,
            buffer + PREFIX_STRIDE * policy::SKIN_MAX as usize,
        ),
    ];
    if reach.iter().any(|r| r.is_none()) {
        log_warn!("DDR SELECTION: stage-frame name buffer out of rel32 reach -- stage frame stays World's");
        return false;
    }
    let mut stock_export = [0u8; 4];
    let mut stock_texture = [0u8; 11];
    unsafe {
        std::ptr::copy_nonoverlapping((export_lea + 3) as *const u8, stock_export.as_mut_ptr(), 4);
        std::ptr::copy_nonoverlapping(
            (texture_site + 2) as *const u8,
            stock_texture.as_mut_ptr(),
            11,
        );
    }
    if stock_texture[4..7] != [0x48, 0x8D, 0x15] {
        log_warn!(
            "DDR SELECTION: stage-frame texture site bytes changed -- stage frame stays World's"
        );
        return false;
    }
    let _ = SITES.set(Sites {
        export_lea,
        texture_site,
        buffer,
        stock_export,
        stock_texture,
    });
    true
}

/// Whether the stage frame can turn legacy on this boot.
pub fn capable() -> bool {
    SITES.get().is_some() && !BROKEN.load(Ordering::Acquire)
}

/// The patch bytes for `skin` (0 = stock).
fn bytes_for(s: &Sites, skin: u8) -> Option<([u8; 4], [u8; 11])> {
    if skin == 0 {
        return Some((s.stock_export, s.stock_texture));
    }
    let export = rel32(s.export_lea + 7, s.buffer)?.to_le_bytes();
    let mut tex = [0u8; 11];
    tex[..4].copy_from_slice(&PREFIX_LEN.to_le_bytes());
    tex[4..7].copy_from_slice(&[0x48, 0x8D, 0x15]);
    let target = s.buffer + PREFIX_STRIDE * skin as usize;
    tex[7..].copy_from_slice(&rel32(s.texture_site + 13, target)?.to_le_bytes());
    Some((export, tex))
}

fn switch_to(skin: u8) -> bool {
    let Some(s) = SITES.get() else {
        return skin == 0;
    };
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let from = APPLIED.load(Ordering::Acquire);
    if from == skin {
        return true;
    }
    let (Some(old), Some(new)) = (bytes_for(s, from), bytes_for(s, skin)) else {
        return false;
    };
    unsafe {
        if old.0 != new.0 {
            if let Err(e) =
                memory::apply_checked_patch((s.export_lea + 3) as *mut u8, &old.0, &new.0)
            {
                log_warn!(
                    "DDR SELECTION: stage-frame export patch failed ({:?}) -- stage frame stays World's",
                    e
                );
                BROKEN.store(true, Ordering::Release);
                return false;
            }
        }
        if let Err(e) = memory::apply_checked_patch((s.texture_site + 2) as *mut u8, &old.1, &new.1)
        {
            log_warn!(
                "DDR SELECTION: stage-frame texture patch failed ({:?}) -- stage frame stays World's",
                e
            );
            // Put the export back so the pair is never split.
            if old.0 != new.0 {
                let _ = memory::apply_checked_patch((s.export_lea + 3) as *mut u8, &new.0, &old.0);
            }
            BROKEN.store(true, Ordering::Release);
            return false;
        }
    }
    APPLIED.store(skin, Ordering::Release);
    if skin != 0 {
        log_info!(
            "DDR SELECTION: stage frame -> A3 names (export stage_frame, textures {}*)",
            prefix(skin)
        );
    }
    true
}

/// Patch in A3's names for `skin` (package helper, right before it registers
/// `dance_stage_frame000N` / a theme's `dance_stage_frame0000_vN`). `false` ⇒
/// keep `dance_stage` stock.
pub fn apply(skin: u8) -> bool {
    (1..=policy::SKIN_MAX).contains(&skin) && capable() && switch_to(skin)
}

/// World's names back (a stock `dance_stage`, disarm, disable). No-op when
/// nothing is patched.
pub fn restore() {
    if APPLIED.load(Ordering::Acquire) != 0 && !switch_to(0) {
        log_warn!("DDR SELECTION: could not restore World's stage-frame names");
    }
}
