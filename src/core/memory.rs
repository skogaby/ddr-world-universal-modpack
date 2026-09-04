//! Memory Utilities — Safe read/write/allocate helpers using Win32 API.

use windows::Win32::System::Diagnostics::Debug::FlushInstructionCache;
use windows::Win32::System::Memory::{
    VirtualAlloc, VirtualProtect, VirtualQuery, MEMORY_BASIC_INFORMATION, MEM_COMMIT, MEM_RESERVE,
    PAGE_EXECUTE_READWRITE, PAGE_GUARD, PAGE_NOACCESS, PAGE_PROTECTION_FLAGS,
};
use windows::Win32::System::Threading::GetCurrentProcess;

use super::memory_patch::{self, PatchBackend, PatchError, PatchStep};

/// Whether `len` bytes at `addr` are committed, readable, non-guard memory
/// in this process — a cheap "is this really a pointer" probe for values
/// read out of game objects whose layout is only PROBABLY what we think.
///
/// Rejects null and non-canonical (garbage) addresses up front, then asks
/// `VirtualQuery` about the containing region. A range that spans a region
/// boundary is walked region by region. `false` on any query failure.
///
/// Intended for walks like the song-select preview loader chain, where a
/// build-dependent struct offset can hand us an arbitrary qword: an
/// identity gate (`*view == vftable`) can only protect the caller if the
/// dereference itself is safe — probe first, then compare.
pub fn is_readable(addr: *const u8, len: usize) -> bool {
    if addr.is_null() || len == 0 {
        return false;
    }
    let start = addr as usize;
    // User-mode canonical range on x64 Windows (48-bit, sign-extended
    // kernel half excluded). Anything above is not a pointer we can hold.
    const USER_MAX: usize = 0x0000_7FFF_FFFF_FFFF;
    let Some(end) = start.checked_add(len) else {
        return false;
    };
    if end > USER_MAX {
        return false;
    }
    let mut cursor = start;
    while cursor < end {
        let mut mbi = MEMORY_BASIC_INFORMATION::default();
        // SAFETY: VirtualQuery only inspects page tables; it never touches
        // the target bytes, so any address value is safe to pass.
        let got = unsafe {
            VirtualQuery(
                Some(cursor as *const _),
                &mut mbi,
                std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
            )
        };
        if got == 0 {
            return false;
        }
        if mbi.State != MEM_COMMIT {
            return false;
        }
        let prot = mbi.Protect;
        if prot == PAGE_NOACCESS || (prot & PAGE_GUARD) == PAGE_GUARD || prot.0 == 0 {
            return false;
        }
        let region_end = mbi.BaseAddress as usize + mbi.RegionSize;
        if region_end <= cursor {
            return false;
        }
        cursor = region_end;
    }
    true
}

/// Allocate a zero-filled RWX memory block.
pub unsafe fn alloc_zeroed(size: usize) -> *mut u8 {
    VirtualAlloc(None, size, MEM_COMMIT | MEM_RESERVE, PAGE_EXECUTE_READWRITE) as *mut u8
}

/// Allocate a zero-filled RWX memory block within ±2GB of `near_addr`.
/// Required for RIP-relative addressing patches (disp32 range).
pub unsafe fn alloc_near(near_addr: *const u8, size: usize) -> *mut u8 {
    // Search in 64KB increments within ±2GB
    let base = near_addr as usize;
    let step: usize = 0x10000; // 64KB allocation granularity
    for offset in (step..0x7FFF0000).step_by(step) {
        // Try above
        let try_addr = base.wrapping_add(offset) & !(step - 1);
        let result = VirtualAlloc(
            Some(try_addr as *const _),
            size,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_EXECUTE_READWRITE,
        ) as *mut u8;
        if !result.is_null() {
            return result;
        }
        // Try below
        if base > offset {
            let try_addr = (base - offset) & !(step - 1);
            let result = VirtualAlloc(
                Some(try_addr as *const _),
                size,
                MEM_COMMIT | MEM_RESERVE,
                PAGE_EXECUTE_READWRITE,
            ) as *mut u8;
            if !result.is_null() {
                return result;
            }
        }
    }
    std::ptr::null_mut()
}

/// Make a memory region writable (RWX). Returns the old protection flags.
pub unsafe fn make_writable(addr: *const u8, size: usize) -> PAGE_PROTECTION_FLAGS {
    let mut old = PAGE_PROTECTION_FLAGS(0);
    let _ = VirtualProtect(addr as *const _, size, PAGE_EXECUTE_READWRITE, &mut old);
    old
}

/// Restore memory protection flags.
pub unsafe fn restore_protection(addr: *const u8, size: usize, old: PAGE_PROTECTION_FLAGS) {
    let mut dummy = PAGE_PROTECTION_FLAGS(0);
    let _ = VirtualProtect(addr as *const _, size, old, &mut dummy);
}

pub unsafe fn read_ptr(addr: *const u8) -> *const u8 {
    (addr as *const *const u8).read_unaligned()
}

pub unsafe fn read_u64(addr: *const u8) -> u64 {
    (addr as *const u64).read_unaligned()
}

pub unsafe fn read_u32(addr: *const u8) -> u32 {
    (addr as *const u32).read_unaligned()
}

pub unsafe fn read_i32(addr: *const u8) -> i32 {
    (addr as *const i32).read_unaligned()
}

pub unsafe fn read_u8(addr: *const u8) -> u8 {
    *addr
}

pub unsafe fn read_f32(addr: *const u8) -> f32 {
    *(addr as *const f32)
}

pub unsafe fn write_ptr(addr: *mut u8, value: *const u8) {
    *(addr as *mut *const u8) = value;
}

pub unsafe fn write_u64(addr: *mut u8, value: u64) {
    *(addr as *mut u64) = value;
}

pub unsafe fn write_u32(addr: *mut u8, value: u32) {
    *(addr as *mut u32) = value;
}

pub unsafe fn write_i32(addr: *mut u8, value: i32) {
    *(addr as *mut i32) = value;
}

pub unsafe fn write_u8(addr: *mut u8, value: u8) {
    *addr = value;
}

pub unsafe fn write_f32(addr: *mut u8, value: f32) {
    *(addr as *mut f32) = value;
}

pub struct ProcessPatchBackend;

impl PatchBackend for ProcessPatchBackend {
    type Protection = PAGE_PROTECTION_FLAGS;

    fn read(&mut self, address: usize, length: usize, _step: PatchStep) -> Result<Vec<u8>, ()> {
        if address == 0 || length == 0 {
            return Err(());
        }
        Ok(unsafe { std::slice::from_raw_parts(address as *const u8, length) }.to_vec())
    }

    fn make_writable(&mut self, address: usize, length: usize) -> Result<Self::Protection, ()> {
        let mut old = PAGE_PROTECTION_FLAGS(0);
        unsafe {
            VirtualProtect(
                address as *const _,
                length,
                PAGE_EXECUTE_READWRITE,
                &mut old,
            )
        }
        .map_err(|_| ())?;
        Ok(old)
    }

    fn write(&mut self, address: usize, bytes: &[u8], _step: PatchStep) -> Result<(), ()> {
        if address == 0 || bytes.is_empty() {
            return Err(());
        }
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), address as *mut u8, bytes.len());
        }
        Ok(())
    }

    fn flush(&mut self, address: usize, length: usize, _step: PatchStep) -> Result<(), ()> {
        unsafe { FlushInstructionCache(GetCurrentProcess(), Some(address as *const _), length) }
            .map_err(|_| ())
    }

    fn restore_protection(
        &mut self,
        address: usize,
        length: usize,
        protection: Self::Protection,
        _step: PatchStep,
    ) -> Result<(), ()> {
        let mut ignored = PAGE_PROTECTION_FLAGS(0);
        unsafe { VirtualProtect(address as *const _, length, protection, &mut ignored) }
            .map_err(|_| ())
    }

    fn allocate_near(&mut self, near: usize, size: usize) -> Option<usize> {
        let address = unsafe { alloc_near(near as *const u8, size) } as usize;
        (address != 0).then_some(address)
    }
}

/// Apply a checked patch to the current process and restore the original bytes
/// on any failure after memory protection changes.
pub unsafe fn apply_checked_patch(
    address: *mut u8,
    expected: &[u8],
    replacement: &[u8],
) -> Result<(), PatchError> {
    memory_patch::apply_checked_patch(
        &mut ProcessPatchBackend,
        address as usize,
        expected,
        replacement,
    )
}

/// Allocate an executable block reachable by a five-byte `JMP rel32`.
pub unsafe fn alloc_near_rel32(
    jump_instruction: *const u8,
    size: usize,
) -> Result<*mut u8, PatchError> {
    memory_patch::allocate_rel32_block(&mut ProcessPatchBackend, jump_instruction as usize, size)
        .map(|address| address as *mut u8)
}
