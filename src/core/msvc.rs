//! MSVC ABI struct layouts shared by every mod that passes strings/vectors
//! INTO game code (the game is MSVC; we must match its `std::string` /
//! `std::vector` object layouts exactly).
//!
//! THE 0x28-STRIDE LESSON (cabinet-caught twice — music_wheel 2026-08-16,
//! s-marvelous results deploy #1 2026-08-30): the game's
//! `vector<std::string>` elements are 0x28 bytes — 16-byte SSO buf/heap-ptr
//! union, u64 size, u64 capacity, and **8 bytes of TRAILING PAD**. A
//! 0x20-stride source array reads as ZERO elements when game code walks it
//! at its own stride. Never re-derive this layout — use these structs.
//!
//! All types here are plain data the GAME only READS (const-source
//! copy-assign / by-const-pointer); backing storage stays mod-owned.

/// MSVC `std::string` at the game's `vector<string>` element stride (0x28).
/// Also safe wherever a string is passed by const pointer (the callee
/// reads buf/len/cap; the pad is our storage).
#[repr(C)]
pub struct MsvcString {
    pub buf: [u8; 16],
    pub len: u64,
    pub cap: u64,
    pub _pad: u64,
}

impl MsvcString {
    pub const fn empty() -> Self {
        MsvcString {
            buf: [0; 16],
            len: 0,
            cap: 0xF,
            _pad: 0,
        }
    }

    /// SSO form — `s` must be ≤ 15 bytes. Oversized names clamp to EMPTY
    /// rather than panicking (hook-path rule; the 2026-08-16 cabinet
    /// lesson: a 19-char name panicking here silently killed every set
    /// call). Debug builds assert.
    pub fn sso(s: &str) -> MsvcString {
        let mut out = MsvcString::empty();
        out.set(s);
        out
    }

    /// Overwrite in place with an SSO name (≤ 15 bytes; clamps like
    /// [`MsvcString::sso`]).
    pub fn set(&mut self, s: &str) {
        let bytes = s.as_bytes();
        debug_assert!(bytes.len() <= 15, "SSO string too long: {}", s);
        let n = if bytes.len() <= 15 { bytes.len() } else { 0 };
        self.buf = [0; 16];
        self.buf[..n].copy_from_slice(&bytes[..n]);
        self.len = n as u64;
        self.cap = 0xF;
    }

    /// SSO form from raw bytes (non-UTF-8 payloads like SJIS glyphs — the
    /// graph legend's `■` prefix). Same ≤ 15-byte clamp as [`MsvcString::sso`].
    pub fn sso_bytes(bytes: &[u8]) -> MsvcString {
        debug_assert!(
            bytes.len() <= 15,
            "SSO string too long ({} bytes)",
            bytes.len()
        );
        let n = if bytes.len() <= 15 { bytes.len() } else { 0 };
        let mut buf = [0u8; 16];
        buf[..n].copy_from_slice(&bytes[..n]);
        MsvcString {
            buf,
            len: n as u64,
            cap: 0xF,
            _pad: 0,
        }
    }

    /// Heap form referencing caller-owned storage (names > 15 bytes).
    /// `bytes` must outlive every use (callers keep `'static` data).
    pub fn heap_ref(bytes: &'static [u8]) -> MsvcString {
        let mut buf = [0u8; 16];
        buf[..8].copy_from_slice(&(bytes.as_ptr() as u64).to_le_bytes());
        MsvcString {
            buf,
            len: bytes.len() as u64,
            // Any value > 15 selects the heap-pointer interpretation; 31
            // mirrors MSVC's minimum heap capacity.
            cap: 31,
            _pad: 0,
        }
    }
}

/// MSVC `std::vector<T>` header — {begin, end, cap_end}. Passed by const
/// pointer as a copy-assign SOURCE; backing storage stays mod-owned.
#[repr(C)]
pub struct MsvcVec<T> {
    pub begin: *const T,
    pub end: *const T,
    pub cap_end: *const T,
}

/// MSVC `std::shared_ptr` — {object, control block}.
#[repr(C)]
pub struct SharedPtrPair {
    pub obj: *mut u8,
    pub ctrl: *mut u8,
}
