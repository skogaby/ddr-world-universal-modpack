//! Song Limit Expansion Mod — Increases the XML read buffer from 1MB to 8MB.
//!
//! The game allocates a fixed 1MB buffer to read musicdb.xml (and coursedb.xml,
//! license.xml). At ~463 bytes per song in XML, 1MB caps out at ~2262 songs.
//! This mod patches the buffer size to 8MB, supporting ~17,000 songs.
//!
//! Each parser has two patch sites:
//!   1. MOV EDX, 0x100000       (allocation size)
//!   2. MOV [RSP+0x20], 0x100000 (read size parameter)
//!
//! The patch is a single byte change at each site: 0x10 → 0x80.

use crate::core::memory;
use crate::core::scanner::{scan_pattern_all, ScanResult};
use crate::mods::mod_trait::{EarlyContext, Mod, ModContext};
use crate::{log_info, log_warn};

/// Original buffer size byte (0x10 in the imm32 = 0x100000 = 1MB)
const ORIGINAL_SIZE_BYTE: u8 = 0x10;
/// New buffer size byte (0x80 in the imm32 = 0x800000 = 8MB)
const NEW_SIZE_BYTE: u8 = 0x80;

/// XOR R8D,R8D; MOV EDX,0x100000; CALL ...
/// The 0x10 byte is at pattern offset 6.
const ALLOC_PATTERN: &str = "45 33 C0 BA 00 00 10 00 E8";
const ALLOC_PATCH_OFFSET: usize = 6;

/// MOV dword ptr [RSP+0x20], 0x100000
/// The 0x10 byte is at pattern offset 6.
const READ_PATTERN: &str = "C7 44 24 20 00 00 10 00";
const READ_PATCH_OFFSET: usize = 6;

const EXPECTED_HITS: usize = 3; // license, musicdb, coursedb

struct PatchSite {
    addr: *mut u8,
    original: u8,
}

unsafe impl Send for PatchSite {}

pub struct SongLimitExpansionMod {
    sites: Vec<PatchSite>,
    /// Set true when `early_apply` has scanned + verified + written the
    /// patches. Subsequent `init` and `enable` see this and no-op so the
    /// race-critical work isn't duplicated. `disable` ignores the flag —
    /// it always rolls back, so the mod-menu's runtime toggle still works.
    early_applied: bool,
}

unsafe impl Send for SongLimitExpansionMod {}

impl SongLimitExpansionMod {
    pub fn new() -> Self {
        Self {
            sites: Vec::new(),
            early_applied: false,
        }
    }

    /// Scan-and-verify the 6 patch sites. Populates `self.sites`. Returns
    /// `false` on any failure (wrong hit count, unexpected byte). Shared
    /// by `early_apply` and the `init` fallback path so the AOB +
    /// validation logic lives in one place.
    fn scan_and_verify(&mut self, base: *const u8, size: usize) -> bool {
        let alloc_hits = scan_pattern_all(base, size, ALLOC_PATTERN);
        if alloc_hits.len() != EXPECTED_HITS {
            log_warn!(
                "SongLimitExpansion: expected {} alloc sites, found {} — aborting",
                EXPECTED_HITS,
                alloc_hits.len()
            );
            return false;
        }

        let read_hits = scan_pattern_all(base, size, READ_PATTERN);
        if read_hits.len() != EXPECTED_HITS {
            log_warn!(
                "SongLimitExpansion: expected {} read sites, found {} — aborting",
                EXPECTED_HITS,
                read_hits.len()
            );
            return false;
        }

        if !self.record_sites(&alloc_hits, ALLOC_PATCH_OFFSET) {
            return false;
        }
        if !self.record_sites(&read_hits, READ_PATCH_OFFSET) {
            return false;
        }

        log_info!(
            "SongLimitExpansion: found {} patch sites (1MB → 8MB)",
            self.sites.len()
        );
        true
    }

    /// Verify each hit holds the expected original byte at `patch_offset`,
    /// and record a `PatchSite` so `disable` can roll back. Returns false on
    /// the first unexpected byte (mod aborts to avoid corrupting unknown
    /// memory).
    fn record_sites(&mut self, hits: &[ScanResult], patch_offset: usize) -> bool {
        for hit in hits {
            let addr = unsafe { hit.address.add(patch_offset) as *mut u8 };
            let val = unsafe { *addr };
            if val != ORIGINAL_SIZE_BYTE {
                log_warn!(
                    "SongLimitExpansion: unexpected byte 0x{:02X} at site +0x{:X}",
                    val,
                    hit.offset
                );
                return false;
            }
            self.sites.push(PatchSite {
                addr,
                original: val,
            });
        }
        true
    }

    /// Write 0x80 at every site's address. Used by both `early_apply` and
    /// the `enable` fallback path.
    fn write_patches(&self) {
        for site in &self.sites {
            unsafe {
                let old = memory::make_writable(site.addr as *const u8, 1);
                memory::write_u8(site.addr, NEW_SIZE_BYTE);
                memory::restore_protection(site.addr as *const u8, 1, old);
            }
        }
    }
}

impl Mod for SongLimitExpansionMod {
    fn id(&self) -> &str {
        "song-limit-expansion"
    }
    fn name(&self) -> &str {
        "Song Limit Expansion"
    }
    fn description(&self) -> &str {
        "Increases the number of songs that can be loaded by ~8x"
    }
    fn required_signatures(&self) -> &[&str] {
        &[]
    }

    fn early_apply(&mut self, ctx: &EarlyContext) -> bool {
        // Race-critical path: scan, verify, AND write all in one pass at
        // the earliest moment we have a stable game module. Patches need to
        // land before the game's master_loader reaches musicdb_parser.
        let base = ctx.game_module.base;
        let size = ctx.game_module.size;

        if !self.scan_and_verify(base, size) {
            // scan_and_verify already logged. Leave self.sites populated up
            // to the failure point; disable() walks self.sites and is
            // tolerant of partial state, but since we never wrote 0x80, the
            // bytes are already 0x10 and disable would no-op safely.
            return false;
        }

        self.write_patches();
        self.early_applied = true;
        log_info!(
            "SongLimitExpansion: early_apply landed — XML buffers expanded to 8MB ({} patches)",
            self.sites.len()
        );
        true
    }

    fn init(&mut self, ctx: &ModContext) -> bool {
        if self.early_applied {
            // early_apply already scanned, verified, and wrote the patches.
            // sites is populated; nothing to do at registration time.
            log_info!("SongLimitExpansion: init() skipped — early_apply already ran");
            return true;
        }

        // Fallback: early_apply was config-gated off, or the user disabled
        // and re-enabled the mod via mod-menu after a fresh boot. Scan +
        // verify so enable() has self.sites to write into. Don't write yet —
        // that's enable()'s job in this path so the user-facing toggle
        // works.
        let base = ctx.game_module.base;
        let size = ctx.game_module.size;
        self.scan_and_verify(base, size)
    }

    fn enable(&mut self) {
        if self.early_applied {
            // Bytes are already 0x80 in memory; no work to do.
            return;
        }

        // Fallback: scan ran in init() but the patches haven't been written
        // yet. This is the mod-menu toggle-on path.
        self.write_patches();
        log_info!(
            "SongLimitExpansion: enabled — XML buffers expanded to 8MB ({} patches)",
            self.sites.len()
        );
    }

    fn disable(&mut self) {
        for site in &self.sites {
            unsafe {
                let old = memory::make_writable(site.addr as *const u8, 1);
                memory::write_u8(site.addr, site.original);
                memory::restore_protection(site.addr as *const u8, 1, old);
            }
        }
        log_info!("SongLimitExpansion: disabled — XML buffers restored to 1MB");
    }
}
