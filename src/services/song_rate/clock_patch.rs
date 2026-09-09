//! Permanent identity-first patch for the authoritative gameplay music count.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, AtomicU8, Ordering};

use crate::core::memory_patch::{
    allocate_rel32_block, apply_checked_patch, rel32_displacement, PatchBackend, PatchError,
};
use crate::core::xact::rate::RateRatio;

#[cfg(windows)]
use crate::core::{memory, signatures::SignatureStore};
#[cfg(windows)]
use crate::{log_info, log_warn};
#[cfg(windows)]
use once_cell::sync::OnceCell;
#[cfg(windows)]
use std::sync::atomic::AtomicPtr;

pub const IDENTITY_Q31: u64 = 1 << 31;
pub const CLOCK_PATCH_BYTES: [u8; 8] = [0x44, 0x8d, 0x34, 0x18, 0x4c, 0x8d, 0x67, 0x58];
/// Room for the identity body (~70 bytes) plus the optional audio-clock
/// call-out prologue (~180 bytes) plus the 8-aligned factor slot.
const STUB_ALLOCATION_SIZE: usize = 320;

/// The audio-clock call-out the stub can be built with: Win64 `extern "C"`
/// `fn(actor: *mut u8 /* rcx = rdi */, rbx: i32 /* edx = ebx */) -> i32`
/// returning the `rbx` the stub continues with (identity ⇒ unchanged).
pub type ClockCallout = extern "C" fn(*mut u8, i32) -> i32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClockStubLayout {
    pub bytes: Vec<u8>,
    pub factor_offset: usize,
    pub return_jump_offset: usize,
    /// Byte offset of the `mov rax, imm64` call-out address inside the stub
    /// (`None` when built without a call-out).
    pub callout_imm_offset: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InstalledClock {
    pub stub_address: usize,
    pub factor_address: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClockInstallError {
    Allocate,
    Build(PatchError),
    Stub(PatchError),
    Site(PatchError),
}

#[must_use]
pub fn scale_music_count_q31(value: i32, factor: u64) -> i32 {
    let product = i128::from(value) * i128::from(factor);
    let negative = product < 0;
    let magnitude = if negative { -product } else { product };
    let rounded = (magnitude + (1i128 << 30)) >> 31;
    let signed = if negative { -rounded } else { rounded };
    signed.clamp(i128::from(i32::MIN), i128::from(i32::MAX)) as i32
}

pub fn build_clock_stub(
    stub_address: usize,
    return_address: usize,
) -> Result<ClockStubLayout, PatchError> {
    build_clock_stub_with_callout(stub_address, return_address, None)
}

/// Build the stub, optionally prefixed by the audio-clock call-out.
///
/// The call-out prologue runs BEFORE the displaced `lea r14d,[rax+rbx]`
/// (rax = the option vcall's `J`, rbx = `T − S − A`, rdi = the GamePlayActor)
/// and replaces `rbx` with the call-out's return. Register/ABI contract at
/// the site (verified on all four supported builds — `rbx` is DEAD after the
/// `lea`, `r12`/`r14` are written by the displaced instructions, flags are
/// already clobbered by the identity body's `cmp`s):
///
/// ```text
/// mov r12, rsp ; and rsp,-16 ; sub rsp,0xC0      ; r12 = saved rsp (dead reg), 16-aligned frame:
///                                                 ;   [0x00..0x20) shadow, [0x20..0x80) xmm0-5,
///                                                 ;   [0x80..0xB8) rax rcx rdx r8 r9 r10 r11
/// movups [rsp+0x20..0x70], xmm0..xmm5 ; mov [rsp+0x80..0xB0], rax rcx rdx r8 r9 r10 r11
/// mov rcx, rdi ; mov edx, ebx ; mov rax, imm64 ; call rax ; mov ebx, eax
/// restore r11..r8 rdx rcx rax ; restore xmm5..xmm0 ; mov rsp, r12
/// ```
///
/// `mov ebx,eax` zero-extends exactly like the original 32-bit `sub ebx`s did.
/// The call-out is a plain Rust `extern "C"` fn — it preserves every
/// callee-saved register (rbx rbp rsi rdi r12-r15, xmm6-15) itself.
pub fn build_clock_stub_with_callout(
    stub_address: usize,
    return_address: usize,
    callout: Option<usize>,
) -> Result<ClockStubLayout, PatchError> {
    let mut bytes = Vec::with_capacity(STUB_ALLOCATION_SIZE);
    let mut callout_imm_offset = None;
    if let Some(target) = callout {
        bytes.extend_from_slice(&[0x49, 0x89, 0xe4]); // mov r12,rsp
        bytes.extend_from_slice(&[0x48, 0x83, 0xe4, 0xf0]); // and rsp,-16
        bytes.extend_from_slice(&[0x48, 0x81, 0xec, 0xc0, 0x00, 0x00, 0x00]); // sub rsp,0xC0
                                                                              // movups [rsp+disp8], xmm0..xmm5
        for (index, disp) in [0x20u8, 0x30, 0x40, 0x50, 0x60, 0x70]
            .into_iter()
            .enumerate()
        {
            bytes.extend_from_slice(&[0x0f, 0x11, 0x44 | ((index as u8) << 3), 0x24, disp]);
        }
        // mov [rsp+disp32], rax/rcx/rdx (REX.W) then r8..r11 (REX.WR)
        for (rex, reg, disp) in [
            (0x48u8, 0u8, 0x80u32),
            (0x48, 1, 0x88),
            (0x48, 2, 0x90),
            (0x4c, 0, 0x98),
            (0x4c, 1, 0xa0),
            (0x4c, 2, 0xa8),
            (0x4c, 3, 0xb0),
        ] {
            bytes.extend_from_slice(&[rex, 0x89, 0x84 | (reg << 3), 0x24]);
            bytes.extend_from_slice(&disp.to_le_bytes());
        }
        bytes.extend_from_slice(&[0x48, 0x89, 0xf9]); // mov rcx,rdi
        bytes.extend_from_slice(&[0x89, 0xda]); // mov edx,ebx
        bytes.extend_from_slice(&[0x48, 0xb8]); // mov rax,imm64
        callout_imm_offset = Some(bytes.len());
        bytes.extend_from_slice(&(target as u64).to_le_bytes());
        bytes.extend_from_slice(&[0xff, 0xd0]); // call rax
        bytes.extend_from_slice(&[0x89, 0xc3]); // mov ebx,eax
                                                // restore r11..r8, rdx, rcx, rax
        for (rex, reg, disp) in [
            (0x4cu8, 3u8, 0xb0u32),
            (0x4c, 2, 0xa8),
            (0x4c, 1, 0xa0),
            (0x4c, 0, 0x98),
            (0x48, 2, 0x90),
            (0x48, 1, 0x88),
            (0x48, 0, 0x80),
        ] {
            bytes.extend_from_slice(&[rex, 0x8b, 0x84 | (reg << 3), 0x24]);
            bytes.extend_from_slice(&disp.to_le_bytes());
        }
        // movups xmm5..xmm0, [rsp+disp8]
        for (index, disp) in [
            (5u8, 0x70u8),
            (4, 0x60),
            (3, 0x50),
            (2, 0x40),
            (1, 0x30),
            (0, 0x20),
        ] {
            bytes.extend_from_slice(&[0x0f, 0x10, 0x44 | (index << 3), 0x24, disp]);
        }
        bytes.extend_from_slice(&[0x4c, 0x89, 0xe4]); // mov rsp,r12
    }
    bytes.extend_from_slice(&[0x44, 0x8d, 0x34, 0x18]); // lea r14d,[rax+rbx]
    bytes.extend_from_slice(&[0x50, 0x51, 0x52]); // preserve rax, rcx, rdx
    bytes.extend_from_slice(&[0x49, 0x63, 0xc6]); // movsxd rax,r14d

    let imul_offset = bytes.len();
    bytes.extend_from_slice(&[0x48, 0xf7, 0x2d, 0, 0, 0, 0]); // imul qword [rip+factor]
    bytes.extend_from_slice(&[0x48, 0x89, 0xc1]); // mov rcx,rax
    bytes.extend_from_slice(&[0x48, 0xc1, 0xf9, 0x3f]); // sar rcx,63
    bytes.extend_from_slice(&[0x48, 0x31, 0xc8]); // xor rax,rcx
    bytes.extend_from_slice(&[0x48, 0x29, 0xc8]); // sub rax,rcx (abs)
    bytes.extend_from_slice(&[0x48, 0x05, 0x00, 0x00, 0x00, 0x40]); // add rax,2^30
    bytes.extend_from_slice(&[0x48, 0xc1, 0xe8, 0x1f]); // shr rax,31
    bytes.extend_from_slice(&[0x48, 0x31, 0xc8, 0x48, 0x29, 0xc8]); // restore sign

    bytes.extend_from_slice(&[0x48, 0x3d, 0xff, 0xff, 0xff, 0x7f]); // cmp rax,i32::MAX
    let jle_min = push_rel8_branch(&mut bytes, 0x7e);
    bytes.extend_from_slice(&[0xb8, 0xff, 0xff, 0xff, 0x7f]); // mov eax,i32::MAX
    let jmp_assign = push_rel8_branch(&mut bytes, 0xeb);
    let check_min = bytes.len();
    bytes.extend_from_slice(&[0x48, 0x3d, 0x00, 0x00, 0x00, 0x80]); // cmp rax,i32::MIN
    let jge_assign = push_rel8_branch(&mut bytes, 0x7d);
    bytes.extend_from_slice(&[0xb8, 0x00, 0x00, 0x00, 0x80]); // mov eax,i32::MIN
    let assign = bytes.len();
    bytes.extend_from_slice(&[0x41, 0x89, 0xc6]); // mov r14d,eax
    bytes.extend_from_slice(&[0x5a, 0x59, 0x58]); // restore rdx, rcx, rax
    bytes.extend_from_slice(&[0x4c, 0x8d, 0x67, 0x58]); // lea r12,[rdi+0x58]
    let return_jump_offset = bytes.len();
    bytes.extend_from_slice(&[0xe9, 0, 0, 0, 0]);

    patch_rel8(&mut bytes, jle_min, check_min)?;
    patch_rel8(&mut bytes, jmp_assign, assign)?;
    patch_rel8(&mut bytes, jge_assign, assign)?;

    while (stub_address + bytes.len()) % 8 != 0 {
        bytes.push(0);
    }
    let factor_offset = bytes.len();
    bytes.extend_from_slice(&IDENTITY_Q31.to_le_bytes());

    let imul_end = stub_address
        .checked_add(imul_offset + 7)
        .ok_or(PatchError::Rel32OutOfRange)?;
    let factor_address = stub_address
        .checked_add(factor_offset)
        .ok_or(PatchError::Rel32OutOfRange)?;
    let factor_disp = rel32_displacement(imul_end, factor_address)?.to_le_bytes();
    bytes[imul_offset + 3..imul_offset + 7].copy_from_slice(&factor_disp);

    let return_end = stub_address
        .checked_add(return_jump_offset + 5)
        .ok_or(PatchError::Rel32OutOfRange)?;
    let return_disp = rel32_displacement(return_end, return_address)?.to_le_bytes();
    bytes[return_jump_offset + 1..return_jump_offset + 5].copy_from_slice(&return_disp);

    if bytes.len() > STUB_ALLOCATION_SIZE {
        return Err(PatchError::Rel32OutOfRange);
    }
    Ok(ClockStubLayout {
        bytes,
        factor_offset,
        return_jump_offset,
        callout_imm_offset,
    })
}

fn push_rel8_branch(bytes: &mut Vec<u8>, opcode: u8) -> usize {
    bytes.extend_from_slice(&[opcode, 0]);
    bytes.len() - 1
}

fn patch_rel8(
    bytes: &mut [u8],
    displacement_index: usize,
    target: usize,
) -> Result<(), PatchError> {
    let branch_end = displacement_index + 1;
    let displacement = (target as isize) - (branch_end as isize);
    bytes[displacement_index] =
        i8::try_from(displacement).map_err(|_| PatchError::Rel32OutOfRange)? as u8;
    Ok(())
}

pub fn install_clock_with_backend<B: PatchBackend>(
    backend: &mut B,
    patch_address: usize,
    readiness: &AtomicBool,
) -> Result<InstalledClock, ClockInstallError> {
    install_clock_with_backend_and_callout(backend, patch_address, readiness, None)
}

pub fn install_clock_with_backend_and_callout<B: PatchBackend>(
    backend: &mut B,
    patch_address: usize,
    readiness: &AtomicBool,
    callout: Option<usize>,
) -> Result<InstalledClock, ClockInstallError> {
    readiness.store(false, Ordering::Release);
    let stub_address = allocate_rel32_block(backend, patch_address, STUB_ALLOCATION_SIZE)
        .map_err(|_| ClockInstallError::Allocate)?;
    let layout = build_clock_stub_with_callout(
        stub_address,
        patch_address + CLOCK_PATCH_BYTES.len(),
        callout,
    )
    .map_err(ClockInstallError::Build)?;
    let expected_stub = vec![0; layout.bytes.len()];
    apply_checked_patch(backend, stub_address, &expected_stub, &layout.bytes)
        .map_err(ClockInstallError::Stub)?;

    let displacement =
        rel32_displacement(patch_address + 5, stub_address).map_err(ClockInstallError::Build)?;
    let mut replacement = [0x90; CLOCK_PATCH_BYTES.len()];
    replacement[0] = 0xe9;
    replacement[1..5].copy_from_slice(&displacement.to_le_bytes());
    apply_checked_patch(backend, patch_address, &CLOCK_PATCH_BYTES, &replacement)
        .map_err(ClockInstallError::Site)?;

    readiness.store(true, Ordering::Release);
    Ok(InstalledClock {
        stub_address,
        factor_address: stub_address + layout.factor_offset,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RateSnapshot {
    pub generation: u64,
    pub requested_percent: i32,
    pub participant_mask: u8,
    pub effective_rate: RateRatio,
    pub committed: bool,
}

impl RateSnapshot {
    pub const IDENTITY: Self = Self {
        generation: 0,
        requested_percent: 100,
        participant_mask: 0,
        effective_rate: RateRatio::IDENTITY,
        committed: false,
    };

    /// Whether a non-identity generation is COMMITTED — the selector for the
    /// assist-tick content→wall conversion path (design req 30;
    /// `tick_domain`): a committed non-identity snapshot converts tick
    /// positions and restart skips through the exact ratio, everything else
    /// takes the legacy identity arithmetic bit-identically. (Step 4's
    /// interim scaffold gate consumed the same predicate to refuse
    /// synthesis; the conversion replaced the refusal.) The 100% guard is
    /// defensive — identity never arms, so a committed identity snapshot is
    /// unreachable by construction.
    #[must_use]
    pub fn is_non_identity_commit(&self) -> bool {
        self.committed && self.requested_percent != 100
    }

    /// The PUS CSV export's two rate cells (design req 34): the requested
    /// percent and the committed EXACT ratio as a `source/output` fraction
    /// (the fraction — never a rounded decimal — is what "the committed
    /// exact ratio" means; the existing CSV carries only integers, so
    /// neither representation had precedent and exactness won). Identity,
    /// committed-100, and uncommitted snapshots all emit the uniform
    /// identity values so the schema never varies. Consumed by the
    /// power_user_statistics exporter, which latches the snapshot alongside
    /// its per-song identity (the mod is not host-mounted; this method is —
    /// the `is_non_identity_commit` split).
    #[must_use]
    pub fn csv_rate_cells(&self) -> (i32, String) {
        if self.is_non_identity_commit() {
            (
                self.requested_percent,
                format!(
                    "{}/{}",
                    self.effective_rate.source_frames, self.effective_rate.output_frames
                ),
            )
        } else {
            (100, "1/1".to_string())
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResetOutcome {
    Applied,
    Deferred,
}

pub struct RatePublication {
    sequence: AtomicU64,
    generation: AtomicU64,
    requested_percent: AtomicI32,
    participant_mask: AtomicU8,
    source_frames: AtomicU64,
    output_frames: AtomicU64,
    committed: AtomicBool,
    reset_pending: AtomicBool,
    factor: &'static AtomicU64,
}

impl RatePublication {
    pub fn new(factor: &'static AtomicU64) -> Self {
        factor.store(IDENTITY_Q31, Ordering::Release);
        Self {
            sequence: AtomicU64::new(0),
            generation: AtomicU64::new(0),
            requested_percent: AtomicI32::new(100),
            participant_mask: AtomicU8::new(0),
            source_frames: AtomicU64::new(1),
            output_frames: AtomicU64::new(1),
            committed: AtomicBool::new(false),
            reset_pending: AtomicBool::new(false),
            factor,
        }
    }

    #[must_use]
    pub fn read(&self) -> RateSnapshot {
        loop {
            let before = self.sequence.load(Ordering::Acquire);
            if before & 1 != 0 {
                std::hint::spin_loop();
                continue;
            }
            let snapshot = RateSnapshot {
                generation: self.generation.load(Ordering::Relaxed),
                requested_percent: self.requested_percent.load(Ordering::Relaxed),
                participant_mask: self.participant_mask.load(Ordering::Relaxed),
                effective_rate: RateRatio {
                    source_frames: self.source_frames.load(Ordering::Relaxed),
                    output_frames: self.output_frames.load(Ordering::Relaxed),
                },
                committed: self.committed.load(Ordering::Relaxed),
            };
            let after = self.sequence.load(Ordering::Acquire);
            if before == after {
                return snapshot;
            }
        }
    }

    pub fn publish_identity(&self, generation: u64, participant_mask: u8) {
        loop {
            if let Some(guard) = self.try_begin_write() {
                guard.finish(generation, participant_mask);
                return;
            }
            std::hint::spin_loop();
        }
    }

    pub fn reset_identity(&self) -> ResetOutcome {
        self.factor.store(IDENTITY_Q31, Ordering::Release);
        let sequence = self.sequence.load(Ordering::Acquire);
        if sequence & 1 != 0
            || self
                .sequence
                .compare_exchange(sequence, sequence + 1, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            self.reset_pending.store(true, Ordering::Release);
            self.factor.store(IDENTITY_Q31, Ordering::Release);
            return ResetOutcome::Deferred;
        }
        self.write_identity_fields(0, 0);
        self.reset_pending.store(false, Ordering::Release);
        self.sequence.store(sequence + 2, Ordering::Release);
        ResetOutcome::Applied
    }

    /// The ONLY writer that can publish `committed = true` — called
    /// exclusively from the wave-bank transaction's commit step, after score
    /// and movie safety state are already published. Allocation-free and
    /// lock-free (bounded seqlock spin against the tiny writer sections);
    /// writes the full non-identity field set, releases the even sequence,
    /// and only THEN stores the non-identity Q31 factor — the machine-visible
    /// clock scale is always last (design req 37). If a definitive reset
    /// raced this commit (`RESET_PENDING`), the reset wins: safety
    /// publication still happens, then the deferred identity reset is applied
    /// and the factor never leaves identity.
    pub fn publish_committed(
        &self,
        generation: u64,
        requested_percent: i32,
        participant_mask: u8,
        effective_rate: RateRatio,
    ) {
        loop {
            let sequence = self.sequence.load(Ordering::Acquire);
            if sequence & 1 != 0
                || self
                    .sequence
                    .compare_exchange(sequence, sequence + 1, Ordering::AcqRel, Ordering::Acquire)
                    .is_err()
            {
                std::hint::spin_loop();
                continue;
            }
            self.generation.store(generation, Ordering::Relaxed);
            self.requested_percent
                .store(requested_percent, Ordering::Relaxed);
            self.participant_mask
                .store(participant_mask, Ordering::Relaxed);
            self.source_frames
                .store(effective_rate.source_frames, Ordering::Relaxed);
            self.output_frames
                .store(effective_rate.output_frames, Ordering::Relaxed);
            self.committed.store(true, Ordering::Relaxed);
            self.sequence.store(sequence + 2, Ordering::Release);
            if self.reset_pending.swap(false, Ordering::AcqRel) {
                let _ = self.reset_identity();
            } else {
                // A rate whose Q31 cannot be represented is rejected at
                // exposure; the identity fallback here is a can't-happen
                // fail-closed guard (clock integrity over audio sync).
                let factor = effective_rate
                    .q31()
                    .ok()
                    .and_then(|q| u64::try_from(q).ok())
                    .unwrap_or(IDENTITY_Q31);
                self.factor.store(factor, Ordering::Release);
            }
            return;
        }
    }

    fn try_begin_write(&self) -> Option<IdentityWriteGuard<'_>> {
        let sequence = self.sequence.load(Ordering::Acquire);
        if sequence & 1 != 0 {
            return None;
        }
        self.sequence
            .compare_exchange(sequence, sequence + 1, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| IdentityWriteGuard {
                publication: self,
                odd_sequence: sequence + 1,
                finished: false,
            })
    }

    pub(crate) fn begin_identity_write_for_test(&self) -> Option<IdentityWriteGuard<'_>> {
        self.try_begin_write()
    }

    fn write_identity_fields(&self, generation: u64, participant_mask: u8) {
        self.generation.store(generation, Ordering::Relaxed);
        self.requested_percent.store(100, Ordering::Relaxed);
        self.participant_mask
            .store(participant_mask, Ordering::Relaxed);
        self.source_frames.store(1, Ordering::Relaxed);
        self.output_frames.store(1, Ordering::Relaxed);
        self.committed.store(false, Ordering::Relaxed);
    }
}

pub(crate) struct IdentityWriteGuard<'a> {
    publication: &'a RatePublication,
    odd_sequence: u64,
    finished: bool,
}

impl IdentityWriteGuard<'_> {
    pub(crate) fn finish(mut self, generation: u64, participant_mask: u8) {
        self.publication
            .write_identity_fields(generation, participant_mask);
        if self.publication.reset_pending.swap(false, Ordering::AcqRel) {
            self.publication.write_identity_fields(0, 0);
        }
        self.publication
            .sequence
            .store(self.odd_sequence + 1, Ordering::Release);
        self.finished = true;
        if self.publication.reset_pending.swap(false, Ordering::AcqRel) {
            let _ = self.publication.reset_identity();
        }
    }
}

impl Drop for IdentityWriteGuard<'_> {
    fn drop(&mut self) {
        if !self.finished {
            self.publication
                .factor
                .store(IDENTITY_Q31, Ordering::Release);
            self.publication.write_identity_fields(0, 0);
            self.publication
                .sequence
                .store(self.odd_sequence + 1, Ordering::Release);
        }
    }
}

#[cfg(windows)]
static INSTALLED: AtomicBool = AtomicBool::new(false);
#[cfg(windows)]
static STUB_ADDRESS: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
#[cfg(windows)]
static FACTOR_ADDRESS: AtomicPtr<AtomicU64> = AtomicPtr::new(std::ptr::null_mut());
#[cfg(windows)]
static PUBLICATION: OnceCell<RatePublication> = OnceCell::new();
/// The call-out address the installed stub was built with (0 = none) — the
/// diagnostics' stub verification rebuilds the layout with the same value.
#[cfg(windows)]
static CALLOUT_ADDRESS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// The call-out the installed stub carries, if any.
#[cfg(windows)]
#[must_use]
pub fn installed_callout() -> Option<usize> {
    let address = CALLOUT_ADDRESS.load(Ordering::Acquire);
    (address != 0).then_some(address)
}

#[cfg(windows)]
pub fn init(signatures: &SignatureStore) -> bool {
    if INSTALLED.load(Ordering::Acquire) {
        return true;
    }
    let Some(patch) = signatures.get_address("song_rate_clock_patch") else {
        log_warn!("song_rate: clock patch signature unavailable");
        return false;
    };
    // The audio clock's call-out is compiled into the stub ONLY when the
    // gameplay-timing-fixes mod is enabled in config (design §0: no footprint
    // otherwise). The call-out itself passes `rbx` through until armed.
    let callout = crate::services::audio_clock::wants_engine()
        .then(|| crate::services::audio_clock::game::corrected_rbx as ClockCallout as usize);
    let transaction_ready = AtomicBool::new(false);
    let installed = install_clock_with_backend_and_callout(
        &mut memory::ProcessPatchBackend,
        patch as usize,
        &transaction_ready,
        callout,
    );
    let installed = match installed {
        Ok(installed) => installed,
        Err(error) => {
            log_warn!("song_rate: identity clock installation failed: {:?}", error);
            return false;
        }
    };
    STUB_ADDRESS.store(installed.stub_address as *mut u8, Ordering::Release);
    FACTOR_ADDRESS.store(
        installed.factor_address as *mut AtomicU64,
        Ordering::Release,
    );
    let factor = unsafe { &*(installed.factor_address as *const AtomicU64) };
    if factor.load(Ordering::Acquire) != IDENTITY_Q31 {
        log_warn!("song_rate: clock factor did not initialize to identity");
        return false;
    }
    let _ = PUBLICATION.set(RatePublication::new(factor));
    CALLOUT_ADDRESS.store(callout.unwrap_or(0), Ordering::Release);
    INSTALLED.store(true, Ordering::Release);
    log_info!(
        "song_rate: permanent identity clock installed (stub @ {:p}{})",
        installed.stub_address as *const u8,
        if callout.is_some() {
            ", with the audio-clock call-out"
        } else {
            ""
        }
    );
    true
}

#[cfg(windows)]
#[must_use]
pub fn is_installed() -> bool {
    INSTALLED.load(Ordering::Acquire)
}

#[cfg(not(windows))]
#[must_use]
pub fn is_installed() -> bool {
    false
}

#[cfg(windows)]
#[must_use]
pub fn snapshot() -> RateSnapshot {
    PUBLICATION
        .get()
        .map(RatePublication::read)
        .unwrap_or(RateSnapshot::IDENTITY)
}

/// The live publication (writer surface for the lifecycle runtime's
/// production sink). `None` until the identity clock installed.
#[cfg(windows)]
#[must_use]
pub(crate) fn publication() -> Option<&'static RatePublication> {
    PUBLICATION.get()
}
