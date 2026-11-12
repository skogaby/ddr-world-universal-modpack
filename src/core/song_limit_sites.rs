//! The byte sites `mods::song_limit_expansion` patches, as data plus a
//! finder — one definition shared by the mod (which scans before the
//! signature store exists: `ea3_boot`, the `startup.arc` open) and by
//! `core::signatures::SignatureStore::derive_song_limit_sites` (which
//! publishes them so `scripts/validate_signatures.sh` proves every site on
//! every supported build). Pure: `core::scanner` + raw unaligned reads;
//! mounted by the signature harness.
//!
//! Each [`SiteSpec`] is a unique-or-counted AOB plus the immediates inside
//! it that carry the limit; [`find`] accepts a hit set only when the count
//! matches AND every immediate still holds its stock value — the
//! "unexpected byte ⇒ touch nothing" rule every patch here follows. The
//! `property_mem_read` mode load is not one AOB (the bytes between it and
//! the READ site differ between builds) and is derived by
//! [`find_mode_site`] from each READ hit instead.

use crate::core::scanner::scan_pattern_all;

/// An immediate inside a site: its offset from the pattern hit and the
/// value the game ships with.
#[derive(Clone, Copy, Debug)]
pub struct Imm {
    pub offset: usize,
    pub stock: u32,
}

/// A patch site family.
#[derive(Clone, Copy, Debug)]
pub struct SiteSpec {
    /// Store/log name (one hit) or name prefix (several hits: `_0`, `_1`…).
    pub name: &'static str,
    pub pattern: &'static str,
    /// Exact number of hits the pattern must produce.
    pub hits: usize,
    pub imms: &'static [Imm],
}

/// Stock values, named once so the mod and the ceiling agree.
pub const STOCK_ARENA_BYTES: u32 = 0x0010_0000;
pub const STOCK_PROPERTY_MODE: u32 = 0x11;
pub const STOCK_RECORD_POOL: u32 = 0x1000;
pub const STOCK_ARC_CHUNK_BYTES: u32 = 0x8_0000;
pub const ARC_CHUNK_SLOTS: u32 = 4;
pub const STOCK_TEXTURE_SLOTS: u32 = 0x1000;

/// `XOR R8D,R8D; MOV EDX,0x100000; CALL …` — the property arena allocation
/// in each of the three XML parsers (license, musicdb, coursedb).
pub const ARENA_ALLOC: SiteSpec = SiteSpec {
    name: "song_limit_arena_alloc",
    pattern: "45 33 C0 BA 00 00 10 00 E8",
    hits: 3,
    imms: &[Imm {
        offset: 4,
        stock: STOCK_ARENA_BYTES,
    }],
};

/// `MOV dword ptr [RSP+0x20],0x100000` — the arena size handed to
/// `property_mem_read` in the same three parsers.
pub const ARENA_READ: SiteSpec = SiteSpec {
    name: "song_limit_arena_read",
    pattern: "C7 44 24 20 00 00 10 00",
    hits: 3,
    imms: &[Imm {
        offset: 4,
        stock: STOCK_ARENA_BYTES,
    }],
};

/// `MOV dword ptr [RBP+0x38],0x7E; MOV dword ptr [RBP+0x3C],-1;
/// MOV dword ptr [RBP+0x30],0x1000` — the step-data manager parameter
/// block in `Application::onBoot`; the pool imm32 is at offset 17.
pub const RECORD_POOL: SiteSpec = SiteSpec {
    name: "song_limit_record_pool",
    pattern: "C7 45 38 7E 00 00 00 C7 45 3C FF FF FF FF C7 45 30 00 10 00 00",
    hits: 1,
    imms: &[Imm {
        offset: 17,
        stock: STOCK_RECORD_POOL,
    }],
};

/// `MOV dword ptr [chunk_size],0x80000; MOV dword ptr [slots],4;
/// MOV EDX,0x200000; MOV R8D,0x20` — `agcs::ArchiveFileCallback`'s ctor.
/// Chunk size at offset 6, pool allocation (`slots × chunk`) at 21.
pub const ARC_CHUNK: SiteSpec = SiteSpec {
    name: "song_limit_arc_chunk",
    pattern:
        "C7 05 ?? ?? ?? ?? 00 00 08 00 C7 05 ?? ?? ?? ?? 04 00 00 00 BA 00 00 20 00 41 B8 20 00 00 00",
    hits: 1,
    imms: &[
        Imm {
            offset: 6,
            stock: STOCK_ARC_CHUNK_BYTES,
        },
        Imm {
            offset: 21,
            stock: STOCK_ARC_CHUNK_BYTES * ARC_CHUNK_SLOTS,
        },
    ],
};

/// Five consecutive `MOV dword ptr [RBP+d],imm32` stores of the gs init
/// config in `Renderer::initGs`: `0x800`, `0x100`, `0x100`, **`0x1000`**
/// (the texture count), `0x40`. The texture imm32 is at offset 24.
pub const TEXTURE_SLOTS: SiteSpec = SiteSpec {
    name: "song_limit_texture_slots",
    pattern: "C7 45 ?? 00 08 00 00 C7 45 ?? 00 01 00 00 C7 45 ?? 00 01 00 00 C7 45 ?? 00 10 00 00 C7 45 ?? 40 00 00 00",
    hits: 1,
    imms: &[Imm {
        offset: 24,
        stock: STOCK_TEXTURE_SLOTS,
    }],
};

/// Every family, for the sweep.
pub const ALL: &[&SiteSpec] = &[
    &ARENA_ALLOC,
    &ARENA_READ,
    &RECORD_POOL,
    &ARC_CHUNK,
    &TEXTURE_SLOTS,
];

/// `MOV R8D, imm32` prefix; the mode load before each READ site.
const MODE_PREFIX: [u8; 2] = [0x41, 0xB8];
/// Offset of the imm32 inside that instruction.
pub const MODE_IMM_OFFSET: usize = 2;
/// How far back from a READ site the mode load may sit.
pub const MODE_SCAN_BACK: usize = 32;

/// One verified hit: the pattern address plus the address of each
/// immediate, in `imms` order.
#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub address: *const u8,
    pub offset: usize,
}

/// Scan `spec` and verify it: exactly `spec.hits` hits (in address order),
/// each holding every immediate's stock value. Any deviation is an error
/// naming the reason — callers touch nothing then.
pub fn find(base: *const u8, size: usize, spec: &SiteSpec) -> Result<Vec<Hit>, String> {
    let hits = scan_pattern_all(base, size, spec.pattern);
    if hits.len() != spec.hits {
        return Err(format!(
            "{}: expected {} site(s), found {}",
            spec.name,
            spec.hits,
            hits.len()
        ));
    }
    let mut out = Vec::with_capacity(hits.len());
    for h in hits {
        for imm in spec.imms {
            let val = unsafe { read_u32(h.address.add(imm.offset)) };
            if val != imm.stock {
                return Err(format!(
                    "{}: imm32 at +0x{:X}+{} is 0x{:08X}, expected 0x{:08X}",
                    spec.name, h.offset, imm.offset, val, imm.stock
                ));
            }
        }
        out.push(Hit {
            address: h.address,
            offset: h.offset,
        });
    }
    Ok(out)
}

/// The imm32 address of the `MOV R8D, 0x11` that loads the parser mode
/// before the READ site `read_hit`: exactly one such instruction within
/// [`MODE_SCAN_BACK`] bytes before it, else `None`.
pub fn find_mode_site(base: *const u8, read_hit: &Hit) -> Option<*const u8> {
    let start = read_hit.offset.checked_sub(MODE_SCAN_BACK)?;
    let window: &[u8] = unsafe { std::slice::from_raw_parts(base.add(start), MODE_SCAN_BACK) };
    let mut found: Option<usize> = None;
    for i in 0..=MODE_SCAN_BACK - 6 {
        if window[i..i + 2] == MODE_PREFIX
            && u32::from_le_bytes([window[i + 2], window[i + 3], window[i + 4], window[i + 5]])
                == STOCK_PROPERTY_MODE
        {
            if found.is_some() {
                return None;
            }
            found = Some(i);
        }
    }
    let i = found?;
    Some(unsafe { base.add(start + i + MODE_IMM_OFFSET) })
}

/// Unaligned little-endian u32 read (code immediates are unaligned).
unsafe fn read_u32(p: *const u8) -> u32 {
    (p as *const u32).read_unaligned()
}
