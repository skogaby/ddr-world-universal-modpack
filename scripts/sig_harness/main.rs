//! Offline signature harness entry point — generated crate `main.rs`, mounted
//! by `scripts/validate_signatures.sh` (never compiled into the DLL).
//!
//! Maps a PE file the way the Windows loader would (sections at their
//! virtual addresses, DIR64 relocations applied against the host allocation)
//! and runs the DLL's REAL `SignatureStore::resolve_all` +
//! `resolve_derived` over it. Every `[+]`/`[-]` log line the game would
//! print at boot is printed to stdout, so the report side can parse the
//! exact strings operators see in `log.txt`.
//!
//! Usage: sig_harness <build-name> <path-to-gamemdx.dll>

use std::process::exit;

use std::collections::HashMap;

use sig_harness::core::module_resolver::GameModule;
use sig_harness::core::signatures::{SignatureStore, HOST_LIBAFP_EXPORTS};

fn rd_u16(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
fn rd_u32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
fn rd_u64(b: &[u8], o: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[o..o + 8]);
    u64::from_le_bytes(a)
}

fn cstr_at(b: &[u8], off: usize) -> String {
    let mut end = off;
    while end < b.len() && b[end] != 0 {
        end += 1;
    }
    String::from_utf8_lossy(&b[off.min(b.len())..end]).into_owned()
}

struct MappedImage {
    /// Backing allocation; `Vec<u64>` so the base is 8-aligned.
    _backing: Vec<u64>,
    base: *const u8,
    size: usize,
    preferred_base: u64,
}

/// Map `file` as a loaded image. Returns None (with a message) on a
/// malformed PE. Only PE32+ (x64) is supported — every DDR World module
/// this DLL hooks is 64-bit.
/// Parse a PE export table into (ordinal -> name). Names only; unnamed
/// ordinals are skipped (nothing in signatures.rs resolves those).
fn parse_exports(file: &[u8]) -> Result<HashMap<u32, String>, String> {
    if file.len() < 0x40 || &file[0..2] != b"MZ" {
        return Err("not an MZ file".into());
    }
    let pe = rd_u32(file, 0x3C) as usize;
    let coff = pe + 4;
    let num_sections = rd_u16(file, coff + 2) as usize;
    let opt_size = rd_u16(file, coff + 16) as usize;
    let opt = coff + 20;
    let num_dirs = rd_u32(file, opt + 108) as usize;
    if num_dirs < 1 {
        return Err("no export directory".into());
    }
    let exp_rva = rd_u32(file, opt + 112) as usize;
    // RVA -> file offset via the section table.
    let sec_table = opt + opt_size;
    let to_off = |rva: usize| -> Option<usize> {
        for i in 0..num_sections {
            let s = sec_table + i * 40;
            let va = rd_u32(file, s + 12) as usize;
            let raw_size = rd_u32(file, s + 16) as usize;
            let raw_ptr = rd_u32(file, s + 20) as usize;
            if rva >= va && rva < va + raw_size {
                return Some(raw_ptr + (rva - va));
            }
        }
        None
    };
    let e = to_off(exp_rva).ok_or("export dir outside sections")?;
    let base_ord = rd_u32(file, e + 16);
    let num_names = rd_u32(file, e + 24) as usize;
    let names = to_off(rd_u32(file, e + 32) as usize).ok_or("names rva")?;
    let ords = to_off(rd_u32(file, e + 36) as usize).ok_or("ordinals rva")?;
    let mut out = HashMap::new();
    for i in 0..num_names {
        let name_rva = rd_u32(file, names + i * 4) as usize;
        let name_off = to_off(name_rva).ok_or("name rva")?;
        let ord = rd_u16(file, ords + i * 2) as u32 + base_ord;
        out.insert(ord, cstr_at(file, name_off));
    }
    Ok(out)
}

fn map_pe(file: &[u8], libafp: Option<&HashMap<u32, String>>) -> Result<MappedImage, String> {
    if file.len() < 0x40 || &file[0..2] != b"MZ" {
        return Err("not an MZ file".into());
    }
    let pe = rd_u32(file, 0x3C) as usize;
    if pe + 24 > file.len() || &file[pe..pe + 4] != b"PE\0\0" {
        return Err("bad PE signature".into());
    }
    let coff = pe + 4;
    let num_sections = rd_u16(file, coff + 2) as usize;
    let opt_size = rd_u16(file, coff + 16) as usize;
    let opt = coff + 20;
    if rd_u16(file, opt) != 0x20B {
        return Err("not PE32+ (x64)".into());
    }
    let image_base = rd_u64(file, opt + 24);
    let size_of_image = rd_u32(file, opt + 56) as usize;
    let size_of_headers = rd_u32(file, opt + 60) as usize;
    let num_dirs = rd_u32(file, opt + 108) as usize;
    let dirs = opt + 112;
    let (import_rva, import_size) = if num_dirs > 1 {
        (
            rd_u32(file, dirs + 8) as usize,
            rd_u32(file, dirs + 12) as usize,
        )
    } else {
        (0, 0)
    };
    let (reloc_rva, reloc_size) = if num_dirs > 5 {
        (
            rd_u32(file, dirs + 5 * 8) as usize,
            rd_u32(file, dirs + 5 * 8 + 4) as usize,
        )
    } else {
        (0, 0)
    };

    let words = (size_of_image + 7) / 8;
    let mut backing = vec![0u64; words];
    let base = backing.as_mut_ptr() as *mut u8;
    let image = unsafe { std::slice::from_raw_parts_mut(base, size_of_image) };

    // Headers.
    let hdr = size_of_headers.min(file.len()).min(size_of_image);
    image[..hdr].copy_from_slice(&file[..hdr]);

    // Sections.
    let sec_table = opt + opt_size;
    for i in 0..num_sections {
        let s = sec_table + i * 40;
        if s + 40 > file.len() {
            return Err("section table truncated".into());
        }
        let vsize = rd_u32(file, s + 8) as usize;
        let va = rd_u32(file, s + 12) as usize;
        let raw_size = rd_u32(file, s + 16) as usize;
        let raw_ptr = rd_u32(file, s + 20) as usize;
        let n = raw_size.min(vsize.max(raw_size));
        if raw_ptr + n > file.len() || va + n > size_of_image {
            return Err(format!("section {} out of bounds", i));
        }
        image[va..va + n].copy_from_slice(&file[raw_ptr..raw_ptr + n]);
    }

    // Relocations (DIR64 only — x64 images use nothing else).
    let delta = (base as u64).wrapping_sub(image_base);
    let mut applied = 0usize;
    if reloc_rva != 0 && reloc_size != 0 && delta != 0 {
        let mut off = reloc_rva;
        let end = (reloc_rva + reloc_size).min(size_of_image);
        while off + 8 <= end {
            let page = rd_u32(image, off) as usize;
            let block = rd_u32(image, off + 4) as usize;
            if block < 8 {
                break;
            }
            let entries = (block - 8) / 2;
            for e in 0..entries {
                let ent = rd_u16(image, off + 8 + e * 2);
                let kind = ent >> 12;
                let rva = page + (ent & 0xFFF) as usize;
                if kind == 10 && rva + 8 <= size_of_image {
                    let v = rd_u64(image, rva).wrapping_add(delta);
                    image[rva..rva + 8].copy_from_slice(&v.to_le_bytes());
                    applied += 1;
                }
            }
            off += block;
        }
    }

    eprintln!("[map] relocs applied: {} (dir rva=0x{:X} size=0x{:X})", applied, reloc_rva, reloc_size);

    // Emulate the loader's IAT patching for the sibling DLL(s) whose exports
    // signatures.rs compares against (libafp). Each imported-by-name function
    // gets a distinct synthetic pointer; the same table is handed to
    // `resolve_libafp_export` so IAT-target comparisons behave as on the
    // cabinet. Real libafp code is never needed — only pointer identity.
    let mut host_exports: HashMap<String, usize> = HashMap::new();
    if import_rva != 0 && import_size != 0 {
        let mut d = import_rva;
        let mut synth: usize = 0x7F00_0000_0000;
        while d + 20 <= size_of_image {
            let oft = rd_u32(image, d) as usize;
            let name_rva = rd_u32(image, d + 12) as usize;
            let ft = rd_u32(image, d + 16) as usize;
            if name_rva == 0 && ft == 0 {
                break;
            }
            let dll = cstr_at(image, name_rva).to_ascii_lowercase();
            if dll.starts_with("libafp-win64") {
                let mut i = 0usize;
                loop {
                    let thunk = rd_u64(image, oft + i * 8);
                    if thunk == 0 {
                        break;
                    }
                    let name = if thunk & (1 << 63) == 0 {
                        Some(cstr_at(image, thunk as usize + 2))
                    } else {
                        libafp.and_then(|t| t.get(&((thunk & 0xFFFF) as u32)).cloned())
                    };
                    if let Some(name) = name {
                        synth += 0x10;
                        let slot = ft + i * 8;
                        image[slot..slot + 8].copy_from_slice(&(synth as u64).to_le_bytes());
                        host_exports.insert(name, synth);
                    }
                    i += 1;
                }
            }
            d += 20;
        }
    }
    eprintln!("[map] libafp imports emulated: {} (import dir rva=0x{:X} size=0x{:X})", host_exports.len(), import_rva, import_size);
    if let Ok(mut g) = HOST_LIBAFP_EXPORTS.lock() {
        *g = Some(host_exports);
    }

    Ok(MappedImage {
        _backing: backing,
        base,
        size: size_of_image,
        preferred_base: image_base,
    })
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 && args.len() != 4 {
        eprintln!("usage: sig_harness <build-name> <gamemdx.dll> [libafp-win64.dll]");
        exit(2);
    }
    let libafp_exports = match args.get(3) {
        Some(p) => match std::fs::read(p).map_err(|e| e.to_string()).and_then(|f| parse_exports(&f)) {
            Ok(t) => {
                eprintln!("[map] libafp export table: {} named exports from {}", t.len(), p);
                Some(t)
            }
            Err(e) => {
                eprintln!("[map] libafp export table unavailable ({}): {}", p, e);
                None
            }
        },
        None => None,
    };
    let build = &args[1];
    let path = &args[2];
    let file = match std::fs::read(path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("cannot read {}: {}", path, e);
            exit(2);
        }
    };
    let img = match map_pe(&file, libafp_exports.as_ref()) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("{}: {}", path, e);
            exit(2);
        }
    };
    println!(
        "### build={} file={} size_of_image=0x{:X} preferred_base=0x{:X}",
        build, path, img.size, img.preferred_base
    );

    let module = GameModule {
        name: "gamemdx.dll".to_string(),
        base: img.base,
        size: img.size,
    };
    let mut store = SignatureStore::new(&module);
    println!("### phase=resolve_all");
    let r = store.resolve_all();
    println!("### phase=resolve_derived");
    store.resolve_derived();
    println!(
        "### summary found={} total={} missing={}",
        r.found,
        r.total,
        r.missing.join(",")
    );
}
