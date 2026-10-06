//! WRAP addressing for mod-shipped textures — a one-imm32 boot patch.
//!
//! World fixes a texture's sampler state (address modes, filters) when the
//! texture is CREATED, from a `usage` word the DDS loader derives from
//! `data/data/texture.db` (a member of `startup.arc`) by the texture's
//! registry key. A key that is not in the db gets the default attr `0x55` =
//! CLAMP / LINEAR / mip LINEAR / aniso 1 — and no custom-model texture is in
//! the db (stock ones are, as `0x315` = WRAP), so every texture a mod ships
//! clamps: a mesh whose UVs (+ `m_vTexAnime`) leave [0, 1] smears its edge
//! texels, and a scrolling one snaps back once per texture period (the HP3
//! flight-tunnel jitter). RE: `docs/3d_model_format_research.md` §3.8.
//!
//! The fix rewrites the default's imm32 in `Application::onBoot`
//! (`texture_db_default_attr_imm32`, match+12) from `0x55` to `0x15` — attr
//! bits 7:6 cleared = WRAP, everything else unchanged — in the `early_apply`
//! phase, before `onBoot` runs. It changes only textures ABSENT from the db:
//! mod content plus a handful of stock `2d_font_*` / license sheets whose UVs
//! stay inside [0, 1]. Nothing is re-read after boot, so there is no revert
//! path; the mod simply skips the patch when it is disabled in config
//! (`lib.rs` runs `early_apply` only for config-enabled mods). The stock
//! imm32 is verified before the write — unknown bytes are never patched.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::core::memory;
use crate::core::signatures::SignatureStore;
use crate::{log_info, log_warn};

/// Offset of the `0x55` imm32 inside the signature match.
const IMM_OFFSET: usize = 12;
/// texture.db attr: `[1:0]` filter LINEAR, `[5:4]` mip LINEAR, `[7:6]` CLAMP.
const STOCK_ATTR: u32 = 0x55;
/// The same with `[7:6]` cleared = WRAP.
const WRAP_ATTR: u32 = 0x15;

static APPLIED: AtomicBool = AtomicBool::new(false);

/// True once the default was rewritten this boot.
pub fn applied() -> bool {
    APPLIED.load(Ordering::Relaxed)
}

/// Rewrite the texture.db default attr to WRAP. Idempotent; fail-open (a
/// miss or an unexpected stock value logs one WARN and leaves the game
/// clamping mod textures).
pub fn apply(signatures: &SignatureStore) -> bool {
    if APPLIED.load(Ordering::Relaxed) {
        return true;
    }
    let Some(site) = signatures.get_address("texture_db_default_attr_imm32") else {
        log_warn!(
            "BackgroundDancers: texture_db_default_attr_imm32 unresolved -- mod textures keep CLAMP addressing (repeating / scrolling UVs will smear)"
        );
        return false;
    };
    // SAFETY: the AOB pins the `MOV dword [RSP+d],imm32` whose imm32 sits
    // at match+12 inside the game image, mapped for the process lifetime.
    let imm = unsafe { site.add(IMM_OFFSET) as *mut u8 };
    let stock = unsafe { memory::read_u32(imm as *const u8) };
    if stock != STOCK_ATTR {
        log_warn!(
            "BackgroundDancers: texture.db default attr imm32 is 0x{stock:X} (expected 0x{STOCK_ATTR:X}) -- not patching"
        );
        return false;
    }
    unsafe {
        let old = memory::make_writable(imm as *const u8, 4);
        memory::write_u32(imm, WRAP_ATTR);
        memory::restore_protection(imm as *const u8, 4, old);
    }
    APPLIED.store(true, Ordering::Relaxed);
    log_info!(
        "BackgroundDancers: texture.db default sampler attr 0x{STOCK_ATTR:X} -> 0x{WRAP_ATTR:X} (mod textures WRAP instead of CLAMP) at {:p}",
        imm
    );
    true
}
