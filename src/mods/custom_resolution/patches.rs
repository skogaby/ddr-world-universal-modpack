//! Immediate patch groups (design §4.6). Each group resolves its sites from
//! the linear AOB hits, reads the bytes back, and writes through
//! `memory::apply_checked_patch` with the stock bytes as the expected value —
//! a stock mismatch on any site skips the group (fail-open). Groups are
//! bundled into two SETS that apply atomically: the OUTPUT set (back-buffer +
//! AA config) and — from plan Step 6 — the RENDER set (surfaces, list
//! viewports, letterbox src). A set that fails half-way is rolled back so
//! the game never boots with, e.g., a bigger back-buffer and stock surfaces.

use crate::core::memory;
use crate::core::signatures::{CustomResolutionAnchors, SignatureStore};
use crate::{log_info, log_warn};

use super::plan::Plan;
use super::sites;

/// One applied imm32 write, kept so a later failure in the same set can undo it.
struct Applied {
    addr: *mut u8,
    stock: [u8; 4],
    new: [u8; 4],
    what: &'static str,
}

/// A group of imm32 writes that was applied (or rolled back) as a unit.
pub struct PatchSet {
    name: &'static str,
    applied: Vec<Applied>,
}

// Raw pointers into the game image — valid for the process lifetime.
unsafe impl Send for PatchSet {}

impl PatchSet {
    pub fn name(&self) -> &'static str {
        self.name
    }
    pub fn len(&self) -> usize {
        self.applied.len()
    }
    pub fn is_empty(&self) -> bool {
        self.applied.is_empty()
    }

    /// Restore every site to its stock bytes (best-effort, logs each miss).
    pub fn rollback(&mut self) {
        for a in self.applied.drain(..).rev() {
            let r = unsafe { memory::apply_checked_patch(a.addr, &a.new, &a.stock) };
            if let Err(e) = r {
                log_warn!(
                    "CustomResolution: rollback of {} ({}) failed: {:?}",
                    self.name,
                    a.what,
                    e
                );
            }
        }
    }
}

/// A pending imm32 write.
struct Pending {
    addr: *mut u8,
    stock: u32,
    new: u32,
    what: &'static str,
}

fn apply_all(name: &'static str, pending: Vec<Pending>) -> Result<PatchSet, String> {
    let mut set = PatchSet {
        name,
        applied: Vec::with_capacity(pending.len()),
    };
    for p in pending {
        if p.stock == p.new {
            continue; // nothing to change (e.g. a stock dimension)
        }
        let stock = p.stock.to_le_bytes();
        let new = p.new.to_le_bytes();
        match unsafe { memory::apply_checked_patch(p.addr, &stock, &new) } {
            Ok(()) => set.applied.push(Applied {
                addr: p.addr,
                stock,
                new,
                what: p.what,
            }),
            Err(e) => {
                set.rollback();
                return Err(format!("{} ({}) failed: {e:?}", name, p.what));
            }
        }
    }
    Ok(set)
}

/// Read `len` bytes after a resolved match as a slice (the match is inside
/// the mapped image; the window is bounded by the caller's knowledge of the
/// function body).
unsafe fn window(addr: *const u8, len: usize) -> Option<&'static [u8]> {
    if !memory::is_readable(addr, len) {
        return None;
    }
    Some(std::slice::from_raw_parts(addr, len))
}

/// OUTPUT set — group 1 (back-buffer selector: all four imms → output dims,
/// so the HD/SD machine-type branch no longer matters) + group 2 (AA config
/// imm → 0 when the plan says so). Returns the applied set or the reason it
/// could not be applied (nothing is left written on `Err`).
pub fn apply_output_set(
    sigs: &SignatureStore,
    anchors: &CustomResolutionAnchors,
    plan: &Plan,
) -> Result<PatchSet, String> {
    let mut pending = Vec::new();

    if !plan.output_is_stock() {
        let m = sigs
            .get_address("display_backbuffer_dims")
            .ok_or("display_backbuffer_dims unresolved")?;
        let b = unsafe { window(m, 51) }.ok_or("display_backbuffer_dims window unreadable")?;
        let s = sites::backbuffer_sites(b)
            .ok_or("display_backbuffer_dims: stock immediates not where expected")?;
        let vals = [plan.output.w, plan.output.h, plan.output.w, plan.output.h];
        let whats = ["hd_w", "hd_h", "sd_w", "sd_h"];
        for i in 0..4 {
            pending.push(Pending {
                addr: unsafe { m.add(s[i].off) as *mut u8 },
                stock: s[i].stock,
                new: vals[i],
                what: whats[i],
            });
        }
    }

    if !plan.output_is_stock() {
        // Window client size (spice2x `-w` keeps whatever the game asked for).
        let m = sigs
            .get_address("window_client_size")
            .ok_or("window_client_size unresolved")?;
        let b = unsafe { window(m, 32) }.ok_or("window_client_size window unreadable")?;
        let (w, h) = sites::window_client_sites(b)
            .ok_or("window_client_size: stock immediates not where expected")?;
        pending.push(Pending {
            addr: unsafe { m.add(w.off) as *mut u8 },
            stock: w.stock,
            new: plan.output.w,
            what: "window_client_w",
        });
        pending.push(Pending {
            addr: unsafe { m.add(h.off) as *mut u8 },
            stock: h.stock,
            new: plan.output.h,
            what: "window_client_h",
        });
    }

    if plan.force_aa_zero {
        let imm = anchors.aa_config_imm.ok_or("aa_config_imm unresolved")?;
        let cur = unsafe { memory::read_u32(imm) };
        if cur != 3 {
            return Err(format!("aa_config_imm reads {cur}, expected 3"));
        }
        pending.push(Pending {
            addr: imm as *mut u8,
            stock: 3,
            new: 0,
            what: "aa_config",
        });
    }

    let set = apply_all("output", pending)?;
    log_info!(
        "CustomResolution: OUTPUT set applied ({} write(s)) -> back-buffer + window client {}x{}{}",
        set.len(),
        plan.output.w,
        plan.output.h,
        if plan.force_aa_zero {
            ", AA config 0"
        } else {
            ""
        }
    );
    Ok(set)
}
