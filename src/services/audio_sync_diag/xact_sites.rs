//! Supported XACT code identity and passive-observer address resolution.

use super::xact_model::Owners;
use crate::core::scanner;

pub const FACTORY: &str = "40 53 56 57 48 83 EC 40 48 8D 44 24 30 48 8B F9 48 8D 15 ?? ?? ?? ?? 41 B9 19 00 02 00 45 33 C0 48 C7 C1 02 00 00 80";
pub const MANAGER: &str =
    "83 FA FF 74 71 53 48 83 EC 20 8B D2 48 8B D9 48 8D 42 05 48 C1 E0 05 48 03 C1";

// Fingerprints cover the consumed instructions, not just entry prologues. None of
// these code spans has a base relocation; ASLR and IAT binding leave them intact.
const SPECS: &[(&str, &str, usize, u64)] = &[
    ("schedule", "48 83 EC 28 48 8B 02 48 89 5C 24 40 48 8B DA 48 89 7C 24 48 48 8B F9 BA 01 00 00 00 48 8B CB FF 50 28", 0x8f, 0x3cc183d69c7d1cb4),
    ("submit", "48 83 EC 28 48 89 5C 24 30 48 89 74 24 40 48 8B F1 48 8B 89 90 00 00 00 48 89 7C 24 48 8B DA 48 85 C9 49 8B F8", 0xe0, 0x2feb37131b26c316),
    ("sound_stop", "48 83 EC 28 0F B6 41 60 48 89 6C 24 38 48 89 74 24 40 48 89 7C 24 48 33 FF A8 04 8B EA 48 8B F1", 0xd0, 0x61887571791fd2a0),
    ("cue_destroy", "48 8B C4 48 81 EC C8 00 00 00 48 89 68 10 48 89 70 18 48 89 78 20 4C 89 60 F8 4C 89 68 F0 48 8D 05 ?? ?? ?? ?? 48 89 01", 0x380, 0x57e8cf7b3440e3cd),
    ("cursor", "48 53 48 83 EC 20 48 8B D9 48 8B 89 90 00 00 00 4C 8D 44 24 38 48 8B 01 48 8D 54 24 30 FF 50 20 85 C0", 0xef, 0x6b1f6e0354e2b37f),
    ("cue_play", "48 83 EC 28 48 89 5C 24 40 48 8B D9 48 89 7C 24 48 48 8B 79 60 48 8D 8F 58 01 00 00", 0x60, 0x4e2d3175f2fd0b3b),
    ("event_start", "48 53 48 83 EC 30 48 8B D9 E8 ?? ?? ?? ?? 4C 8B 1B 48 8B CB 41 FF 93 80 00 00 00 48 85 C0 74 28", 0x8d, 0x63cec4eb2946840a),
    ("stream_play", "83 B9 80 00 00 00 02 75 05 E9 ?? ?? ?? ?? 80 89 48 01 00 00 08 33 C0 C3", 0x18, 0x9333cebf533f88f3),
    ("event_getter", "48 8B 41 60 C3", 5, 0x72b8501ff969e4ba),
    ("ownership", "48 8B 47 48 89 4F 30 48 8B D7 48 8B 48 20 48 8B 49 28 48 8B 7C 24 48 48 83 C4 28 E9 ?? ?? ?? ??", 0x20, 0xd87862741f209fc0),
    ("event_ctor", "48 8D 05 ?? ?? ?? ?? 48 89 49 08 48 89 49 10 48 89 01 48 8B 44 24 28 48 89 51 18 48 89 41 38 33 C0 4C 89 49 20", 0x48, 0x856bc5e66453d0ca),
    ("wave_ctor", "48 83 EC 48 48 89 5C 24 40 48 89 6C 24 38 48 89 74 24 30 48 89 49 08 48 89 49 10", 0x130, 0x9113ac159ec571d3),
    ("voice_set", "48 83 EC 28 48 85 D2 48 89 7C 24 48 4C 8B CA 48 8B F9 48 89 91 88 00 00 00", 0xb2, 0x7509bb56a1833b10),
    ("cue_ctor", "48 8B C4 48 83 EC 68 48 89 58 F8 48 89 68 F0 48 89 70 E8 48 89 78 E0 4C 89 60 D8 4C 89 68 D0 4C 89 70 C8", 0x2a0, 0xde83d4340e68e47d),
    ("sound_ctor", "48 83 EC 48 48 89 5C 24 40 48 89 6C 24 38 48 89 74 24 30 48 8D 05 ?? ?? ?? ?? 48 89 7C 24 28 4C 89 64 24 20 4C 8B E1", 0x1c0, 0xe7b9ac430f6ba685),
    ("track_state", "48 83 EC 28 83 79 48 03 48 89 5C 24 40 48 8B D9", 0xcb, 0x96b59f6cad85c2e6),
    ("cursor_units", "48 53 48 83 EC 20 48 8B D9 48 83 C1 F8 E8 ?? ?? ?? ?? 83 F8 FF 44 8B D8", 0x35, 0x0a9bcd2c1e3e328a),
    // ── Deterministic audio clock (services/audio_clock) ──
    // Source-node per-pass produce (0x43CAC0): pulls exactly the source bytes the
    // decoder needs for one pass, submits to the bus, then `ADD [node+0x5F8], R15`
    // (cumulative source bytes consumed since Start — the 0 → >0 edge IS sample 0).
    // Hashed through its RET so the +0x5F8 accumulate at +0x32A is attested.
    ("produce", "48 81 EC 88 00 00 00 48 8B 81 70 03 00 00 48 89 9C 24 90 00 00 00 48 8B D9 48 85 C0 74 05 48 8B 00 EB 02 33 C0 48 8B 88 F0 01 00 00", 0x34e, 0xde8769025f87a588),
    // In-memory wave submission (0x419DB0): the sibling of the streaming
    // submission `submit` for non-streaming waves (the assist-tick bank). Same
    // shape: `V = wave+0x88`, `call [V.vt+0x20]` (wrapper Start). Distinct
    // stack frame (0x38) keeps it unique against 0x425ED0.
    ("memory_submit", "48 83 EC 38 48 89 5C 24 30 48 89 74 24 28 48 8B F1 48 8B 89 90 00 00 00 48 89 7C 24 20 8B DA 48 85 C9 49 8B F8 74 38", 0xd0, 0x7cabc223ca386f3d),
    // Voice-wrapper Start (0x41E1F0): `rcx = [V+0x28]` (the source-voice
    // interface), `call [vt+0x38]` — attests the V+0x28 link the node walk uses.
    ("wrapper_start", "48 53 48 83 EC 20 F7 DA 48 8B D9 48 8B 49 28 48 8B 01 1B D2 81 E2 00 10 00 00 FF 50 38", 0x31, 0x75a28c922e273bd9),
    // Start applied on the render thread (0x43B250): `node+0x640 = 1;
    // node+0x5F8 = 0` — attests the consumed-bytes field the produce hook reads.
    ("node_start_apply", "C7 81 40 06 00 00 01 00 00 00 48 C7 81 F8 05 00 00 00 00 00 00 C3", 0x16, 0x0d7adbf523e04aea),
];

/// Source-voice interface Start (0x434FC0). Its prologue is shared by seven
/// sibling op posters, so it is selected by the CALL target at +0x4C (must be
/// `node_start_apply`) and then fingerprinted; `[rsi+8]` at +0x48 is the
/// `iface+8 → node` link the node walk depends on.
const IFACE_START_PATTERN: &str = "48 83 EC 28 48 89 6C 24 38 48 89 74 24 40 48 8B F1 48 8B 49 E0 48 89 7C 24 48 8B EA 48 8B 01 4C 89 64 24 20 FF 50 10";
const IFACE_START_CALL: usize = 0x4c;
const IFACE_START_LEN: usize = 0x56;
const IFACE_START_HASH: u64 = 0x372cbc8ad1d6c47e;

#[derive(Clone, Copy, Debug)]
pub struct Sites {
    pub schedule: usize,
    pub submit: usize,
    pub sound_stop: usize,
    pub cue_destroy: usize,
    pub cursor: usize,
    pub cue_play: usize,
    pub event_start: usize,
    pub stream_play: usize,
    pub event_getter: usize,
    /// Audio-clock seams (see the SPECS comments).
    pub produce: usize,
    pub memory_submit: usize,
    pub wrapper_start: usize,
    pub node_start_apply: usize,
    pub iface_start: usize,
}

/// Which wave class a live wave object belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaveKind {
    /// Streaming wave (vtable slot 1 = `stream_play`; submitted through 0x25ED0).
    Streaming,
    /// In-memory wave (vtable slot 1 = `memory_submit`).
    Memory,
}

#[derive(Clone, Copy)]
pub struct Wave {
    pub owners: Owners,
    pub event: usize,
    pub voice: usize,
    pub target: usize,
    pub kind: WaveKind,
}

pub fn event_owners(
    event: usize,
    base: usize,
    sites: &Sites,
    read: &impl Fn(usize) -> Option<usize>,
) -> Option<Owners> {
    let at = |p: usize, n: usize| {
        if p == 0 {
            None
        } else {
            read(p.checked_add(n)?)
        }
    };
    let vt = at(event, 0)?;
    if at(vt, 8)? != base.checked_add(sites.event_start)?
        || at(vt, 0x80)? != base.checked_add(sites.event_getter)?
    {
        return None;
    }
    let state = at(event, 0x48)?;
    let track = at(state, 0x20)?;
    if at(track, 0x30)? != state {
        return None;
    }
    let sound = at(track, 0x28)?;
    sound_owners(sound, base, sites, read)
}

pub fn sound_owners(
    sound: usize,
    base: usize,
    sites: &Sites,
    read: &impl Fn(usize) -> Option<usize>,
) -> Option<Owners> {
    let at = |p: usize, n: usize| {
        if p == 0 {
            None
        } else {
            read(p.checked_add(n)?)
        }
    };
    let cue = at(sound, 0x58)?;
    if at(cue, 0x58)? != sound || at(at(cue, 0)?, 0)? != base.checked_add(sites.cue_play)? {
        return None;
    }
    let owners = Owners {
        sound,
        cue,
        bank: at(cue, 0x240)?,
    };
    owners.valid().then_some(owners)
}

pub fn walk_wave(
    wave: usize,
    base: usize,
    sites: &Sites,
    read: impl Fn(usize) -> Option<usize>,
) -> Option<Wave> {
    let at = |p: usize, n: usize| {
        if p == 0 {
            None
        } else {
            read(p.checked_add(n)?)
        }
    };
    let slot1 = at(at(wave, 0)?, 8)?;
    let kind = if slot1 == base.checked_add(sites.stream_play)? {
        WaveKind::Streaming
    } else if sites.memory_submit != 0 && slot1 == base.checked_add(sites.memory_submit)? {
        WaveKind::Memory
    } else {
        return None;
    };
    let event = at(wave, 0x90)?;
    if at(event, 0x60)? != wave {
        return None;
    }
    let owners = event_owners(event, base, sites, &read)?;
    let voice = at(wave, 0x88)?;
    let target = if voice == 0 {
        0
    } else {
        at(at(voice, 0)?, 0x20)?
    };
    Some(Wave {
        owners,
        event,
        voice,
        target,
        kind,
    })
}

/// The engine source node behind a voice wrapper: `node = *(*(V+0x28) + 8)`
/// (attested by `wrapper_start` reading `V+0x28` and `iface_start` reading
/// `iface+8`). Both pointers are probed by `read`; the wrapper's Start target
/// (`V.vt+0x20`) must be the attested `wrapper_start`.
pub fn voice_node(
    voice: usize,
    base: usize,
    sites: &Sites,
    read: impl Fn(usize) -> Option<usize>,
) -> Option<usize> {
    let at = |p: usize, n: usize| {
        if p == 0 {
            None
        } else {
            read(p.checked_add(n)?)
        }
    };
    if sites.wrapper_start == 0
        || at(at(voice, 0)?, 0x20)? != base.checked_add(sites.wrapper_start)?
    {
        return None;
    }
    let iface = at(voice, 0x28)?;
    let node = at(iface, 8)?;
    (node != 0).then_some(node)
}

/// -1 unknown; 0 suppressed branch; 1 Start called successfully; 2 Start failed.
pub fn submission_branch(flags: Option<u16>, result: i32) -> i64 {
    match flags {
        None => -1,
        Some(flags) if flags & 4 != 0 => 0,
        Some(_) if result < 0 => 2,
        Some(_) => 1,
    }
}

/// Copy only non-writable, non-discardable sections at boot. Relocation pages may
/// already have been discarded by the loader; no observer depends on them.
pub fn snapshot_ranges(headers: &[u8], size: usize) -> Option<Vec<std::ops::Range<usize>>> {
    let (nt, sections, count) = pe(headers)?;
    if size != u32_at(headers, nt + 80)? as usize {
        return None;
    }
    let mut ranges = Vec::new();
    for index in 0..count {
        let s = sections + index * 40;
        let flags = u32_at(headers, s + 36)?;
        if flags & (0x80000000 | 0x02000000) != 0 {
            continue;
        }
        let start = u32_at(headers, s + 12)? as usize;
        let len = u32_at(headers, s + 8)? as usize;
        let end = start.checked_add(len)?;
        if end > size {
            return None;
        }
        if len != 0 {
            ranges.push(start..end);
        }
    }
    Some(ranges)
}

pub fn unique(bytes: &[u8], pattern: &str) -> Option<usize> {
    let matches = scanner::scan_pattern_all(bytes.as_ptr(), bytes.len(), pattern);
    if matches.len() == 1 {
        matches.first().map(|m| m.offset)
    } else {
        None
    }
}

fn u16_at(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?,
    ))
}

fn pe(bytes: &[u8]) -> Option<(usize, usize, usize)> {
    if bytes.get(..2)? != b"MZ" {
        return None;
    }
    let nt = u32_at(bytes, 0x3c)? as usize;
    if bytes.get(nt..nt.checked_add(4)?)? != b"PE\0\0"
        || u16_at(bytes, nt + 4)? != 0x8664
        || u16_at(bytes, nt + 24)? != 0x20b
    {
        return None;
    }
    let count = u16_at(bytes, nt + 6)? as usize;
    let sections = nt.checked_add(24 + u16_at(bytes, nt + 20)? as usize)?;
    if count == 0 || count > 96 {
        return None;
    }
    bytes.get(sections..sections.checked_add(count * 40)?)?;
    Some((nt, sections, count))
}

fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ *b as u64).wrapping_mul(0x100000001b3)
    })
}

fn relative_target(image: &[u8], displacement: usize) -> Option<usize> {
    let next = displacement.checked_add(4)?;
    let disp = u32_at(image, displacement)? as i32;
    // Prove the primitive's pointer arithmetic remains inside this allocation.
    if next.checked_add_signed(disp as isize)? > image.len() {
        return None;
    }
    let target = unsafe { scanner::decode_rip_relative(image.as_ptr().add(displacement)) };
    (target as usize).checked_sub(image.as_ptr() as usize)
}

pub fn resolve(image: &[u8]) -> Result<Sites, &'static str> {
    let (nt, _, _) = pe(image).ok_or("not AMD64 PE32+")?;
    if u32_at(image, nt + 8) != Some(0x471c7720)
        || u32_at(image, nt + 80) != Some(0x69000)
        || image.len() != 0x69000
    {
        return Err("unsupported engine identity");
    }
    let mut resolved = [0usize; SPECS.len()];
    let mut valid = true;
    for (index, (name, pattern, length, expected)) in SPECS.iter().enumerate() {
        let offset = unique(image, pattern).ok_or(*name)?;
        let code = image.get(offset..offset + length).ok_or(*name)?;
        let actual = hash(code);
        #[cfg(test)]
        println!("attest {name}: len={length:x} hash={actual:016x}");
        valid &= actual == *expected;
        resolved[index] = offset;
    }
    if !valid {
        return Err("engine consumed-code fingerprint mismatch");
    }
    // Track::activate forwards its +0x30 state to the attested state activation.
    let dispatches =
        scanner::scan_pattern_all(image.as_ptr(), image.len(), "48 8B 49 30 E9 ?? ?? ?? ??");
    let count = dispatches
        .iter()
        .filter(|m| relative_target(image, m.offset + 5) == Some(resolved[15]))
        .count();
    if count != 1 {
        return Err("track state ownership dispatch unavailable");
    }
    // Interface Start: the sibling whose op call targets node_start_apply.
    let node_start_apply = resolved[20];
    let starts: Vec<usize> =
        scanner::scan_pattern_all(image.as_ptr(), image.len(), IFACE_START_PATTERN)
            .iter()
            .map(|m| m.offset)
            .filter(|offset| {
                relative_target(image, offset + IFACE_START_CALL + 1) == Some(node_start_apply)
            })
            .collect();
    let [iface_start] = starts.as_slice() else {
        return Err("interface Start dispatch unavailable");
    };
    let iface_code = image
        .get(*iface_start..*iface_start + IFACE_START_LEN)
        .ok_or("iface_start")?;
    #[cfg(test)]
    println!(
        "attest iface_start: len={IFACE_START_LEN:x} hash={:016x}",
        hash(iface_code)
    );
    if hash(iface_code) != IFACE_START_HASH {
        return Err("engine consumed-code fingerprint mismatch");
    }
    Ok(Sites {
        schedule: resolved[0],
        submit: resolved[1],
        sound_stop: resolved[2],
        cue_destroy: resolved[3],
        cursor: resolved[4],
        cue_play: resolved[5],
        event_start: resolved[6],
        stream_play: resolved[7],
        event_getter: resolved[8],
        produce: resolved[17],
        memory_submit: resolved[18],
        wrapper_start: resolved[19],
        node_start_apply,
        iface_start: *iface_start,
    })
}

pub fn factory_valid(image: &[u8], offset: usize) -> bool {
    let Some(code) = image.get(offset..offset.saturating_add(0xe8)) else {
        return false;
    };
    if unique(code, FACTORY) != Some(0) {
        return false;
    }
    // RIP-relative string is part of the factory identity, not a guessed RVA.
    let Some(start) = relative_target(image, offset + 0x13) else {
        return false;
    };
    let expected: Vec<u8> = "Software\\Microsoft\\XACT\0"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    let mut shape = code.to_vec();
    for disp in [
        0x13, 0x40, 0x67, 0x75, 0x8b, 0x92, 0x99, 0xa6, 0xb6, 0xc5, 0xcc, 0xdc,
    ] {
        shape[disp..disp + 4].fill(0);
    }
    #[cfg(test)]
    println!("factory normalized hash={:016x}", hash(&shape));
    hash(&shape) == 0x2c1a373b1ed7d8a1
        && image.get(start..start.saturating_add(expected.len())) == Some(expected.as_slice())
        && code
            .get(0xb4..0xba)
            .is_some_and(|b| b.starts_with(&[0xff, 0x15]))
        && code
            .get(0xda..0xe0)
            .is_some_and(|b| b.starts_with(&[0xff, 0x15]))
        && code.get(0xe0..0xe8) == Some(&[0x48, 0x83, 0xc4, 0x40, 0x5f, 0x5e, 0x5b, 0xc3])
}

pub fn manager_valid(image: &[u8], offset: usize) -> bool {
    let Some(code) = image.get(offset..offset.saturating_add(0x77)) else {
        return false;
    };
    unique(code, MANAGER) == Some(0)
        && code.get(0x20..0x28) == Some(&[0x80, 0xbc, 0x0a, 0xb0, 0, 0, 0, 0])
        && code.get(0x48..0x4a) == Some(&[0xff, 0x10])
        && code.get(0x65..0x68) == Some(&[0xff, 0x50, 8])
}

#[cfg(test)]
fn map_pe(file: &[u8]) -> Option<Vec<u8>> {
    let (nt, sections, count) = pe(file)?;
    let size = u32_at(file, nt + 80)? as usize;
    if size > 128 * 1024 * 1024 {
        return None;
    }
    let headers = u32_at(file, nt + 84)? as usize;
    let mut image = vec![0; size];
    image
        .get_mut(..headers)?
        .copy_from_slice(file.get(..headers)?);
    for index in 0..count {
        let s = sections + index * 40;
        let va = u32_at(file, s + 12)? as usize;
        let len = u32_at(file, s + 16)? as usize;
        let raw = u32_at(file, s + 20)? as usize;
        image
            .get_mut(va..va.checked_add(len)?)?
            .copy_from_slice(file.get(raw..raw.checked_add(len)?)?);
    }
    Some(image)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_images_are_rejected_without_panics() {
        for n in [0, 1, 64, 256, 4096] {
            assert!(resolve(&vec![0; n]).is_err());
        }
    }

    #[test]
    fn relative_decode_rejects_targets_outside_the_borrowed_image() {
        assert_eq!(relative_target(&[0xff; 4], 0), Some(3));
        assert_eq!(relative_target(&i32::MAX.to_le_bytes(), 0), None);
        assert_eq!(relative_target(&i32::MIN.to_le_bytes(), 0), None);
        assert_eq!(relative_target(&[], 0), None);
    }

    #[test]
    fn real_engine_identity_and_consumed_bytes_are_attested() {
        let path = std::env::var("XACT_DIAG_BINARY").expect("harness supplies engine path");
        let file = std::fs::read(path).unwrap();
        let image = map_pe(&file).unwrap();
        let sites = resolve(&image).unwrap();
        let mut boot = vec![0; image.len()];
        boot[..4096].copy_from_slice(&image[..4096]);
        for range in snapshot_ranges(&image[..4096], image.len()).unwrap() {
            boot[range.clone()].copy_from_slice(&image[range]);
        }
        assert!(
            resolve(&boot).is_ok(),
            "runtime snapshot need not read discarded relocation pages"
        );
        // Exercise real DIR64 rebasing and IAT binding, not an assumed ASLR-free image.
        let mut relocated = image.clone();
        let (nt, _, _) = pe(&image).unwrap();
        let reloc_dir = nt + 24 + 112 + 5 * 8;
        let mut block = u32_at(&image, reloc_dir).unwrap() as usize;
        let end = block + u32_at(&image, reloc_dir + 4).unwrap() as usize;
        while block < end {
            let page = u32_at(&image, block).unwrap() as usize;
            let length = u32_at(&image, block + 4).unwrap() as usize;
            assert!(length >= 8);
            for slot in (block + 8..block + length).step_by(2) {
                let entry = u16_at(&image, slot).unwrap();
                if entry >> 12 == 10 {
                    let p = page + (entry & 0xfff) as usize;
                    let value = u64::from_le_bytes(relocated[p..p + 8].try_into().unwrap());
                    relocated[p..p + 8]
                        .copy_from_slice(&value.wrapping_add(0x180000000).to_le_bytes());
                }
            }
            block += length;
        }
        let iat_dir = nt + 24 + 112 + 12 * 8;
        let iat = u32_at(&image, iat_dir).unwrap() as usize;
        let iat_len = u32_at(&image, iat_dir + 4).unwrap() as usize;
        relocated[iat..iat + iat_len].fill(0xaa);
        assert!(
            resolve(&relocated).is_ok(),
            "fingerprints must survive loader fixups"
        );
        assert_ne!(sites.schedule, sites.submit);
        for offset in [
            sites.schedule + 0x26,
            sites.submit + 0x65,
            sites.cursor + 0x54,
            sites.cue_destroy + 0x25,
            // audio clock: produce's +0x5F8 accumulate, memory_submit's
            // V=[wave+0x88] load, wrapper_start's [V+0x28], node_start_apply's
            // +0x5F8 zero, iface_start's [iface+8] node load.
            sites.produce + 0x32d,
            sites.memory_submit + 0x68,
            sites.wrapper_start + 0xe,
            sites.node_start_apply + 0xd,
            sites.iface_start + 0x4b,
        ] {
            let mut changed = image.clone();
            changed[offset] ^= 1;
            assert!(
                resolve(&changed).is_err(),
                "changed consumed code at {offset:x}"
            );
        }
        for (name, pattern, length, _) in SPECS {
            let offset = unique(&image, pattern).unwrap() + length - 1;
            let mut changed = image.clone();
            changed[offset] ^= 1;
            assert!(resolve(&changed).is_err(), "unattested tail of {name}");
        }
        let mut changed = image.clone();
        changed[0xf8] ^= 1;
        assert!(resolve(&changed).is_err());
    }

    #[test]
    fn factory_and_manager_shapes_match_every_supplied_game() {
        let directory = std::env::var("XACT_DIAG_GAME_DIR").expect("harness supplies game corpus");
        let mut count = 0;
        for file in std::fs::read_dir(directory).unwrap().flatten() {
            let name = file.file_name().to_string_lossy().to_string();
            if name.starts_with("gamemdx") && name.ends_with(".dll") {
                let image = map_pe(&std::fs::read(file.path()).unwrap()).unwrap();
                let factory = unique(&image, FACTORY).unwrap();
                assert!(factory_valid(&image, factory), "factory shape: {name}");
                let mut changed = image.clone();
                changed[factory + 0x3c] ^= 1;
                assert!(
                    !factory_valid(&changed, factory),
                    "factory body changed: {name}"
                );
                let manager = unique(&image, MANAGER).unwrap();
                assert!(manager_valid(&image, manager), "manager shape: {name}");
                println!("{name}: factory={factory:x}, manager={manager:x}");
                count += 1;
            }
        }
        assert!(count >= 4, "need the complete four-build corpus");
    }

    #[test]
    fn reciprocal_live_chain_rejects_every_broken_link_and_wrong_event_type() {
        use std::collections::BTreeMap;
        let sites = Sites {
            schedule: 0,
            submit: 0,
            sound_stop: 0,
            cue_destroy: 0,
            cursor: 0,
            cue_play: 1,
            event_start: 2,
            stream_play: 3,
            event_getter: 4,
            produce: 0,
            memory_submit: 5,
            wrapper_start: 6,
            node_start_apply: 0,
            iface_start: 0,
        };
        let memory = BTreeMap::from([
            (0x1000, 0x9000),
            (0x9008, 3),
            (0x1090, 0x2000),
            (0x1088, 0x8000),
            (0x2000, 0xa000),
            (0xa008, 2),
            (0xa080, 4),
            (0x2060, 0x1000),
            (0x2048, 0x3000),
            (0x3020, 0x4000),
            (0x4030, 0x3000),
            (0x4028, 0x5000),
            (0x5058, 0x6000),
            (0x6058, 0x5000),
            (0x6000, 0xb000),
            (0xb000, 1),
            (0x6240, 0x7000),
            (0x8000, 0xc000),
            (0xc020, 0xd000),
        ]);
        let got = walk_wave(0x1000, 0, &sites, |p| memory.get(&p).copied()).unwrap();
        assert_eq!(got.owners.cue, 0x6000);
        assert_eq!(got.voice, 0x8000);
        assert_eq!(got.target, 0xd000);
        assert_eq!(got.kind, WaveKind::Streaming);
        // The in-memory sibling is recognised by ITS vtable slot 1.
        let mut mem = memory.clone();
        mem.insert(0x9008, 5);
        let got = walk_wave(0x1000, 0, &sites, |p| mem.get(&p).copied()).unwrap();
        assert_eq!(got.kind, WaveKind::Memory);
        // Node walk: V.vt+0x20 must be wrapper_start; node = *(*(V+0x28)+8).
        let mut nodes = memory.clone();
        nodes.insert(0xc020, 6);
        nodes.insert(0x8028, 0xe000);
        nodes.insert(0xe008, 0xf000);
        assert_eq!(
            voice_node(0x8000, 0, &sites, |p| nodes.get(&p).copied()),
            Some(0xf000)
        );
        assert_eq!(
            voice_node(0x8000, 0, &sites, |p| memory.get(&p).copied()),
            None
        );
        let mut null_node = nodes.clone();
        null_node.insert(0xe008, 0);
        assert_eq!(
            voice_node(0x8000, 0, &sites, |p| null_node.get(&p).copied()),
            None
        );
        let mut no_iface = nodes.clone();
        no_iface.remove(&0x8028);
        assert_eq!(
            voice_node(0x8000, 0, &sites, |p| no_iface.get(&p).copied()),
            None
        );
        for address in memory.keys() {
            let mut bad = memory.clone();
            bad.remove(address);
            assert!(
                walk_wave(0x1000, 0, &sites, |p| bad.get(&p).copied()).is_none(),
                "{address:x}"
            );
        }
        for address in [0x4030, 0x6058, 0x2060, 0xa008, 0xa080, 0x9008] {
            let mut bad = memory.clone();
            bad.insert(address, 42);
            assert!(walk_wave(0x1000, 0, &sites, |p| bad.get(&p).copied()).is_none());
        }
    }

    #[test]
    fn submission_skip_is_not_successful_voice_submission() {
        assert_eq!(submission_branch(Some(0), 0), 1);
        assert_eq!(submission_branch(Some(4), 0), 0);
        assert_eq!(submission_branch(Some(0), -1), 2);
        assert_eq!(submission_branch(None, 0), -1);
    }
}
