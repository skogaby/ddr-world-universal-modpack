//! Immediate patch groups (design §4.6). Each group resolves its sites from
//! the linear AOB hits, reads the bytes back, and writes through
//! `memory::apply_checked_patch` with the stock bytes as the expected value —
//! a stock mismatch on any site skips the group (fail-open). Groups are
//! bundled into two SETS that apply atomically: the OUTPUT set (back-buffer +
//! AA config + window client) and the RENDER set (surfaces, list viewports,
//! letterbox src). A set that fails half-way is rolled back so the game
//! never boots with, e.g., a bigger back-buffer and stock surfaces.

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

    if plan.force_aa_zero() {
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
        if plan.force_aa_zero() {
            ", AA config 0"
        } else {
            ""
        }
    );
    Ok(set)
}

/// Bytes read after the `render_surface_hoist` match: the ctor body carrying
/// the six RT-struct dimension stores ends well inside this (all four
/// builds; `shape_diff.py` identical through 0x1200).
const HOIST_SCAN_LEN: usize = 0x1200;
/// Bytes read after the `list_viewport_table` match (the eight-entry table
/// is ~0x120 long on every build).
const VIEWPORT_SCAN_LEN: usize = 0x140;

/// RENDER set — group 3 (render-surface ctor: the hoisted `R15D`/`ESI`
/// registers every 1280×720 surface-create reads + the six RT-struct dim
/// stores), group 4 (the eight-entry list-viewport table: 5 wide pairs + the
/// square OFFSCREEN1) and group 5 (letterbox source rect x1/y1 — x1 doubles
/// as the `screen_w == render_w` comparand, so the engine's 1:1 POINT branch
/// fires exactly when render == output). Every site is content-verified
/// against its stock value before the first write; any miss rolls the set
/// back and returns `Err` (nothing left written). An empty set when the plan's
/// render is stock.
pub fn apply_render_set(sigs: &SignatureStore, plan: &Plan) -> Result<PatchSet, String> {
    let mut pending = Vec::new();
    if plan.render_is_stock() {
        return apply_all("render", pending);
    }
    let (rw, rh) = (plan.render.w, plan.render.h);
    let packed_wide = sites::pack_dims(rw, rh).ok_or("render dims exceed u16")?;
    let packed_square = sites::pack_dims(rw, rw).ok_or("render dims exceed u16")?;

    // Group 3 — surfaces.
    {
        let m = sigs
            .get_address("render_surface_hoist")
            .ok_or("render_surface_hoist unresolved")?;
        let b =
            unsafe { window(m, HOIST_SCAN_LEN) }.ok_or("render_surface_hoist window unreadable")?;
        let (w, h) = sites::hoist_sites(b)
            .ok_or("render_surface_hoist: hoisted R15D/ESI immediates not where expected")?;
        pending.push(Pending {
            addr: unsafe { m.add(w.off) as *mut u8 },
            stock: w.stock,
            new: rw,
            what: "surface_hoist_w",
        });
        pending.push(Pending {
            addr: unsafe { m.add(h.off) as *mut u8 },
            stock: h.stock,
            new: rh,
            what: "surface_hoist_h",
        });
        let rt = sites::rt_dim_sites(b);
        if !rt.is_complete() {
            return Err(format!(
                "render_surface_hoist: RT dim stores {}/{}/{} (expected {}/{}/{})",
                rt.packed_wide.len(),
                rt.packed_square.len(),
                rt.height_only.len(),
                sites::RT_PACKED_WIDE_COUNT,
                sites::RT_PACKED_SQUARE_COUNT,
                sites::RT_HEIGHT_ONLY_COUNT
            ));
        }
        for s in &rt.packed_wide {
            pending.push(Pending {
                addr: unsafe { m.add(s.off) as *mut u8 },
                stock: s.stock,
                new: packed_wide,
                what: "rt_dims_wide",
            });
        }
        for s in &rt.packed_square {
            pending.push(Pending {
                addr: unsafe { m.add(s.off) as *mut u8 },
                stock: s.stock,
                new: packed_square,
                what: "rt_dims_square",
            });
        }
        for s in &rt.height_only {
            pending.push(Pending {
                addr: unsafe { m.add(s.off) as *mut u8 },
                stock: s.stock,
                new: rh,
                what: "rt_dims_h",
            });
        }
    }

    // Group 4 — list viewports.
    {
        let m = sigs
            .get_address("list_viewport_table")
            .ok_or("list_viewport_table unresolved")?;
        let b = unsafe { window(m, VIEWPORT_SCAN_LEN) }
            .ok_or("list_viewport_table window unreadable")?;
        let pairs = sites::viewport_pairs(b);
        if !sites::viewport_pairs_complete(&pairs) {
            return Err(format!(
                "list_viewport_table: {} pair(s) found (expected {} wide + {} square)",
                pairs.len(),
                sites::VIEWPORT_WIDE_COUNT,
                sites::VIEWPORT_SQUARE_COUNT
            ));
        }
        for p in &pairs {
            let (new_h, what_w, what_h) = match p.kind {
                sites::ViewportKind::Wide => (rh, "viewport_w", "viewport_h"),
                sites::ViewportKind::Square => (rw, "viewport_sq_w", "viewport_sq_h"),
            };
            pending.push(Pending {
                addr: unsafe { m.add(p.w.off) as *mut u8 },
                stock: p.w.stock,
                new: rw,
                what: what_w,
            });
            pending.push(Pending {
                addr: unsafe { m.add(p.h.off) as *mut u8 },
                stock: p.h.stock,
                new: new_h,
                what: what_h,
            });
        }
    }

    // Group 5 — letterbox source rect.
    {
        let m = sigs
            .get_address("letterbox_rect_fn")
            .ok_or("letterbox_rect_fn unresolved")?;
        let b = unsafe { window(m, sites::LETTERBOX_SCAN_LEN) }
            .ok_or("letterbox_rect_fn window unreadable")?;
        let (x1, y1) = sites::letterbox_sites(b)
            .ok_or("letterbox_rect_fn: source-rect immediates not where expected")?;
        pending.push(Pending {
            addr: unsafe { m.add(x1.off) as *mut u8 },
            stock: x1.stock,
            new: rw,
            what: "letterbox_src_x1",
        });
        pending.push(Pending {
            addr: unsafe { m.add(y1.off) as *mut u8 },
            stock: y1.stock,
            new: rh,
            what: "letterbox_src_y1",
        });
    }

    let set = apply_all("render", pending)?;
    log_info!(
        "CustomResolution: RENDER set applied ({} write(s)) -> surfaces + list viewports + letterbox src {}x{} (OFFSCREEN1 {}x{})",
        set.len(),
        rw,
        rh,
        rw,
        rw
    );
    Ok(set)
}
