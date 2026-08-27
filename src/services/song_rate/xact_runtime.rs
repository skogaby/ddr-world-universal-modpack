//! Fixed transaction primitives for the identity-only XACT hook path.

use std::cell::RefCell;
use std::cell::UnsafeCell;
use std::mem::MaybeUninit;
use std::sync::atomic::{AtomicI32, AtomicU64, AtomicU8, AtomicUsize, Ordering};

use crate::core::xact::rate::RateRatio;

pub const MAX_XACT_SLOTS: usize = 4;
const MAX_TLS_DEPTH: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RedirectToken {
    pub call_nonce: u64,
    pub call_depth: u8,
    pub generation: u64,
    pub requested_percent: i32,
    pub participant_mask: u8,
    pub stage_index: i32,
    pub effective_rate: RateRatio,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum XactSlotPhase {
    Free = 0,
    Entered = 1,
    Exposed = 2,
    Committed = 3,
    ReleasePending = 4,
    Quarantined = 5,
}

impl XactSlotPhase {
    fn from_raw(value: u8) -> Option<Self> {
        Some(match value {
            0 => Self::Free,
            1 => Self::Entered,
            2 => Self::Exposed,
            3 => Self::Committed,
            4 => Self::ReleasePending,
            5 => Self::Quarantined,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotError {
    InvalidIndex,
    IdentityMismatch,
    IllegalPhase,
    NoExactMatch,
    Ambiguous,
}

pub struct XactSlots {
    slots: [XactSlot; MAX_XACT_SLOTS],
}

impl Default for XactSlots {
    fn default() -> Self {
        Self::new()
    }
}

impl XactSlots {
    #[must_use]
    pub fn new() -> Self {
        Self {
            slots: std::array::from_fn(|_| XactSlot::new()),
        }
    }

    pub fn claim(&self, owner_thread: u64, nonce: u64, depth: u8, file_id: i32) -> Option<usize> {
        for (index, slot) in self.slots.iter().enumerate() {
            if slot
                .phase
                .compare_exchange(
                    XactSlotPhase::Free as u8,
                    XactSlotPhase::Entered as u8,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_ok()
            {
                slot.owner_thread.store(owner_thread, Ordering::Relaxed);
                slot.call_nonce.store(nonce, Ordering::Relaxed);
                slot.call_depth.store(depth, Ordering::Relaxed);
                slot.file_id.store(file_id, Ordering::Relaxed);
                std::sync::atomic::fence(Ordering::Release);
                return Some(index);
            }
        }
        None
    }

    pub fn expose(
        &self,
        index: usize,
        owner_thread: u64,
        nonce: u64,
        depth: u8,
        file_id: i32,
        token: RedirectToken,
    ) -> Result<(), SlotError> {
        let slot = self.slots.get(index).ok_or(SlotError::InvalidIndex)?;
        slot.validate_identity(owner_thread, nonce, depth, file_id)?;
        if token.call_nonce != nonce || token.call_depth != depth {
            return Err(SlotError::IdentityMismatch);
        }
        slot.token.store(token);
        slot.phase
            .compare_exchange(
                XactSlotPhase::Entered as u8,
                XactSlotPhase::Exposed as u8,
                Ordering::Release,
                Ordering::Acquire,
            )
            .map(|_| ())
            .map_err(|_| SlotError::IllegalPhase)
    }

    pub fn recover_exposed(
        &self,
        owner_thread: u64,
        nonce: u64,
        file_id: i32,
    ) -> Result<usize, SlotError> {
        let mut match_index = None;
        for (index, slot) in self.slots.iter().enumerate() {
            if slot.phase.load(Ordering::Acquire) == XactSlotPhase::Exposed as u8
                && slot.owner_thread.load(Ordering::Relaxed) == owner_thread
                && slot.call_nonce.load(Ordering::Relaxed) == nonce
                && slot.file_id.load(Ordering::Relaxed) == file_id
            {
                if match_index.replace(index).is_some() {
                    return Err(SlotError::Ambiguous);
                }
            }
        }
        match_index.ok_or(SlotError::NoExactMatch)
    }

    pub fn commit(
        &self,
        index: usize,
        owner_thread: u64,
        nonce: u64,
        file_id: i32,
    ) -> Result<RedirectToken, SlotError> {
        let slot = self.slots.get(index).ok_or(SlotError::InvalidIndex)?;
        if slot.owner_thread.load(Ordering::Relaxed) != owner_thread
            || slot.call_nonce.load(Ordering::Relaxed) != nonce
            || slot.file_id.load(Ordering::Relaxed) != file_id
        {
            return Err(SlotError::IdentityMismatch);
        }
        slot.phase
            .compare_exchange(
                XactSlotPhase::Exposed as u8,
                XactSlotPhase::Committed as u8,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .map_err(|_| SlotError::IllegalPhase)?;
        Ok(slot.token.load())
    }

    pub fn quarantine(&self, index: usize) -> Result<(), SlotError> {
        let slot = self.slots.get(index).ok_or(SlotError::InvalidIndex)?;
        slot.phase
            .compare_exchange(
                XactSlotPhase::Exposed as u8,
                XactSlotPhase::Quarantined as u8,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .map_err(|_| SlotError::IllegalPhase)?;
        Ok(())
    }

    pub fn begin_release(&self, index: usize, file_id: i32) -> Result<(), SlotError> {
        let slot = self.slots.get(index).ok_or(SlotError::InvalidIndex)?;
        if slot.file_id.load(Ordering::Relaxed) != file_id {
            return Err(SlotError::IdentityMismatch);
        }
        slot.phase
            .compare_exchange(
                XactSlotPhase::Committed as u8,
                XactSlotPhase::ReleasePending as u8,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .map_err(|_| SlotError::IllegalPhase)?;
        Ok(())
    }

    pub fn begin_release_by_file(&self, file_id: i32) -> Result<usize, SlotError> {
        let mut found = None;
        for (index, slot) in self.slots.iter().enumerate() {
            if slot.phase.load(Ordering::Acquire) == XactSlotPhase::Committed as u8
                && slot.file_id.load(Ordering::Relaxed) == file_id
            {
                if found.replace(index).is_some() {
                    return Err(SlotError::Ambiguous);
                }
            }
        }
        let index = found.ok_or(SlotError::NoExactMatch)?;
        self.begin_release(index, file_id)?;
        Ok(index)
    }

    pub fn finish_release(&self, index: usize) -> Result<(), SlotError> {
        let slot = self.slots.get(index).ok_or(SlotError::InvalidIndex)?;
        slot.phase
            .compare_exchange(
                XactSlotPhase::ReleasePending as u8,
                XactSlotPhase::Entered as u8,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .map_err(|_| SlotError::IllegalPhase)?;
        slot.clear();
        slot.phase
            .store(XactSlotPhase::Free as u8, Ordering::Release);
        Ok(())
    }

    /// Free a quarantined slot once its binding has been retired (the
    /// maintenance drain's counterpart to [`XactSlots::finish_release`] for
    /// the late-fail leg — without it every rejected create would pin a
    /// slot for the process lifetime).
    pub fn finish_quarantine(&self, index: usize) -> Result<(), SlotError> {
        let slot = self.slots.get(index).ok_or(SlotError::InvalidIndex)?;
        slot.phase
            .compare_exchange(
                XactSlotPhase::Quarantined as u8,
                XactSlotPhase::Entered as u8,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .map_err(|_| SlotError::IllegalPhase)?;
        slot.clear();
        slot.phase
            .store(XactSlotPhase::Free as u8, Ordering::Release);
        Ok(())
    }

    pub fn abandon(&self, index: usize) -> Result<(), SlotError> {
        let slot = self.slots.get(index).ok_or(SlotError::InvalidIndex)?;
        let phase = slot.phase.load(Ordering::Acquire);
        if !matches!(
            XactSlotPhase::from_raw(phase),
            Some(XactSlotPhase::Entered | XactSlotPhase::Exposed)
        ) {
            return Err(SlotError::IllegalPhase);
        }
        slot.phase
            .compare_exchange(
                phase,
                XactSlotPhase::ReleasePending as u8,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .map_err(|_| SlotError::IllegalPhase)?;
        slot.clear();
        slot.phase
            .store(XactSlotPhase::Free as u8, Ordering::Release);
        Ok(())
    }

    #[must_use]
    pub fn phase(&self, index: usize) -> Option<XactSlotPhase> {
        XactSlotPhase::from_raw(self.slots.get(index)?.phase.load(Ordering::Acquire))
    }

    /// The stored token (drain-side and diagnostics; the token is only
    /// meaningful for non-Free phases).
    #[must_use]
    pub fn token(&self, index: usize) -> Option<RedirectToken> {
        Some(self.slots.get(index)?.token.load())
    }
}

struct XactSlot {
    phase: AtomicU8,
    owner_thread: AtomicU64,
    call_nonce: AtomicU64,
    call_depth: AtomicU8,
    file_id: AtomicI32,
    token: AtomicRedirectToken,
}

impl XactSlot {
    fn new() -> Self {
        Self {
            phase: AtomicU8::new(XactSlotPhase::Free as u8),
            owner_thread: AtomicU64::new(0),
            call_nonce: AtomicU64::new(0),
            call_depth: AtomicU8::new(0),
            file_id: AtomicI32::new(-1),
            token: AtomicRedirectToken::new(),
        }
    }

    fn validate_identity(
        &self,
        owner_thread: u64,
        nonce: u64,
        depth: u8,
        file_id: i32,
    ) -> Result<(), SlotError> {
        if self.phase.load(Ordering::Acquire) != XactSlotPhase::Entered as u8
            || self.owner_thread.load(Ordering::Relaxed) != owner_thread
            || self.call_nonce.load(Ordering::Relaxed) != nonce
            || self.call_depth.load(Ordering::Relaxed) != depth
            || self.file_id.load(Ordering::Relaxed) != file_id
        {
            Err(SlotError::IdentityMismatch)
        } else {
            Ok(())
        }
    }

    fn clear(&self) {
        self.owner_thread.store(0, Ordering::Relaxed);
        self.call_nonce.store(0, Ordering::Relaxed);
        self.call_depth.store(0, Ordering::Relaxed);
        self.file_id.store(-1, Ordering::Relaxed);
    }
}

struct AtomicRedirectToken {
    call_nonce: AtomicU64,
    call_depth: AtomicU8,
    generation: AtomicU64,
    requested_percent: AtomicI32,
    participant_mask: AtomicU8,
    stage_index: AtomicI32,
    source_frames: AtomicU64,
    output_frames: AtomicU64,
}

impl AtomicRedirectToken {
    fn new() -> Self {
        Self {
            call_nonce: AtomicU64::new(0),
            call_depth: AtomicU8::new(0),
            generation: AtomicU64::new(0),
            requested_percent: AtomicI32::new(100),
            participant_mask: AtomicU8::new(0),
            stage_index: AtomicI32::new(0),
            source_frames: AtomicU64::new(1),
            output_frames: AtomicU64::new(1),
        }
    }

    fn store(&self, token: RedirectToken) {
        self.call_nonce.store(token.call_nonce, Ordering::Relaxed);
        self.call_depth.store(token.call_depth, Ordering::Relaxed);
        self.generation.store(token.generation, Ordering::Relaxed);
        self.requested_percent
            .store(token.requested_percent, Ordering::Relaxed);
        self.participant_mask
            .store(token.participant_mask, Ordering::Relaxed);
        self.stage_index.store(token.stage_index, Ordering::Relaxed);
        self.source_frames
            .store(token.effective_rate.source_frames, Ordering::Relaxed);
        self.output_frames
            .store(token.effective_rate.output_frames, Ordering::Relaxed);
    }

    fn load(&self) -> RedirectToken {
        RedirectToken {
            call_nonce: self.call_nonce.load(Ordering::Relaxed),
            call_depth: self.call_depth.load(Ordering::Relaxed),
            generation: self.generation.load(Ordering::Relaxed),
            requested_percent: self.requested_percent.load(Ordering::Relaxed),
            participant_mask: self.participant_mask.load(Ordering::Relaxed),
            stage_index: self.stage_index.load(Ordering::Relaxed),
            effective_rate: RateRatio {
                source_frames: self.source_frames.load(Ordering::Relaxed),
                output_frames: self.output_frames.load(Ordering::Relaxed),
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameIdentity {
    pub nonce: u64,
    pub depth: u8,
    pub file_id: i32,
    pub slot_index: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TlsError {
    Overflow,
    NotTopFrame,
}

struct FrameStack {
    frames: [FrameIdentity; MAX_TLS_DEPTH],
    len: usize,
    next_nonce: u64,
}

impl FrameStack {
    const fn new() -> Self {
        Self {
            frames: [FrameIdentity {
                nonce: 0,
                depth: 0,
                file_id: -1,
                slot_index: None,
            }; MAX_TLS_DEPTH],
            len: 0,
            next_nonce: 0,
        }
    }
}

thread_local! {
    static FRAMES: RefCell<FrameStack> = const { RefCell::new(FrameStack::new()) };
}

#[derive(Debug)]
pub struct FrameGuard {
    identity: FrameIdentity,
    active: bool,
}

impl FrameGuard {
    #[must_use]
    pub const fn identity(&self) -> FrameIdentity {
        self.identity
    }

    pub fn attach_slot(&mut self, slot_index: usize) -> Result<(), TlsError> {
        FRAMES.with(|frames| {
            let mut frames = frames.borrow_mut();
            let top = frames.len.checked_sub(1).ok_or(TlsError::NotTopFrame)?;
            if frames.frames[top].nonce != self.identity.nonce {
                return Err(TlsError::NotTopFrame);
            }
            frames.frames[top].slot_index = Some(slot_index);
            self.identity.slot_index = Some(slot_index);
            Ok(())
        })
    }
}

impl Drop for FrameGuard {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        FRAMES.with(|frames| {
            let mut frames = frames.borrow_mut();
            let Some(top) = frames.len.checked_sub(1) else {
                return;
            };
            if frames.frames[top].nonce == self.identity.nonce
                && frames.frames[top].depth == self.identity.depth
            {
                frames.frames[top] = FrameIdentity::default();
                frames.len -= 1;
            } else {
                frames.frames = [FrameIdentity::default(); MAX_TLS_DEPTH];
                frames.len = 0;
            }
        });
        self.active = false;
    }
}

pub fn enter_frame(file_id: i32) -> Result<FrameGuard, TlsError> {
    FRAMES.with(|frames| {
        let mut frames = frames.borrow_mut();
        if frames.len == MAX_TLS_DEPTH {
            return Err(TlsError::Overflow);
        }
        frames.next_nonce = frames.next_nonce.wrapping_add(1).max(1);
        let identity = FrameIdentity {
            nonce: frames.next_nonce,
            depth: frames.len as u8 + 1,
            file_id,
            slot_index: None,
        };
        let index = frames.len;
        frames.frames[index] = identity;
        frames.len += 1;
        Ok(FrameGuard {
            identity,
            active: true,
        })
    })
}

#[must_use]
pub fn current_frame() -> Option<FrameIdentity> {
    FRAMES.with(|frames| {
        let frames = frames.borrow();
        frames.len.checked_sub(1).map(|index| frames.frames[index])
    })
}

/// Attach a slot index to the CURRENT top frame, validated by nonce. Used by
/// the binding path (a different stack frame than the detour's
/// `FrameGuard`, so it cannot call `FrameGuard::attach_slot`); the detour's
/// post-original processing re-reads the top frame via [`current_frame`], so
/// the attachment is visible without touching the guard's stale copy.
pub fn attach_slot_to_current(nonce: u64, slot_index: usize) -> Result<(), TlsError> {
    FRAMES.with(|frames| {
        let mut frames = frames.borrow_mut();
        let top = frames.len.checked_sub(1).ok_or(TlsError::NotTopFrame)?;
        if frames.frames[top].nonce != nonce {
            return Err(TlsError::NotTopFrame);
        }
        frames.frames[top].slot_index = Some(slot_index);
        Ok(())
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum MaintenanceKind {
    /// Reclaim a retired binding's slot/resources on the maintenance drain
    /// (consumed from plan Step 4 onward; until then the record is pushed by
    /// the late-fail leg and simply drains unobserved).
    ReclaimBinding = 1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MaintenanceEvent {
    pub kind: MaintenanceKind,
    pub slot_index: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueFull;

pub struct MaintenanceQueue<const N: usize> {
    enqueue_position: AtomicUsize,
    dequeue_position: AtomicUsize,
    slots: [QueueSlot; N],
}

impl<const N: usize> Default for MaintenanceQueue<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> MaintenanceQueue<N> {
    #[must_use]
    pub fn new() -> Self {
        assert!(N > 0, "maintenance queue capacity must be nonzero");
        Self {
            enqueue_position: AtomicUsize::new(0),
            dequeue_position: AtomicUsize::new(0),
            slots: std::array::from_fn(QueueSlot::new),
        }
    }

    pub fn push(&self, event: MaintenanceEvent) -> Result<(), QueueFull> {
        let mut position = self.enqueue_position.load(Ordering::Relaxed);
        loop {
            let slot = &self.slots[position % N];
            let sequence = slot.sequence.load(Ordering::Acquire);
            let difference = sequence.wrapping_sub(position) as isize;
            if difference == 0 {
                match self.enqueue_position.compare_exchange_weak(
                    position,
                    position.wrapping_add(1),
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => {
                        unsafe { (*slot.event.get()).write(event) };
                        slot.sequence
                            .store(position.wrapping_add(1), Ordering::Release);
                        return Ok(());
                    }
                    Err(actual) => position = actual,
                }
            } else if difference < 0 {
                return Err(QueueFull);
            } else {
                position = self.enqueue_position.load(Ordering::Relaxed);
            }
        }
    }

    pub fn pop(&self) -> Option<MaintenanceEvent> {
        let mut position = self.dequeue_position.load(Ordering::Relaxed);
        loop {
            let slot = &self.slots[position % N];
            let sequence = slot.sequence.load(Ordering::Acquire);
            let difference = sequence.wrapping_sub(position.wrapping_add(1)) as isize;
            if difference == 0 {
                match self.dequeue_position.compare_exchange_weak(
                    position,
                    position.wrapping_add(1),
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => {
                        let event = unsafe { (*slot.event.get()).assume_init_read() };
                        slot.sequence
                            .store(position.wrapping_add(N), Ordering::Release);
                        return Some(event);
                    }
                    Err(actual) => position = actual,
                }
            } else if difference < 0 {
                return None;
            } else {
                position = self.dequeue_position.load(Ordering::Relaxed);
            }
        }
    }
}

struct QueueSlot {
    sequence: AtomicUsize,
    event: UnsafeCell<MaybeUninit<MaintenanceEvent>>,
}

impl QueueSlot {
    fn new(index: usize) -> Self {
        Self {
            sequence: AtomicUsize::new(index),
            event: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }
}

unsafe impl<const N: usize> Send for MaintenanceQueue<N> {}
unsafe impl<const N: usize> Sync for MaintenanceQueue<N> {}

// ── Diagnostic bank-event timeline ───────────────────────────────────
//
// A bounded lock-free record of every `wavebank_create`/`wavebank_unregister`
// the detours observe, drained by the maintenance thread into the log. Pure
// diagnosis: it answers "which bank instances existed when" (the 2026-08-06
// stock-audio investigation — a stale same-named preview bank is suspected of
// winning the engine's by-name cue bind). Recording is a couple of atomics —
// legal inside the allocation-free, log-free detours; the LOGGING happens on
// the drain thread only, and only diagnostic boots drain at all.

/// Timeline capacity (events; a song load produces a handful).
pub const BANK_TIMELINE_CAPACITY: usize = 64;

/// What one timeline entry describes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BankEventKind {
    Create,
    Unregister,
}

/// How the create transaction resolved (compressed [`CreateOutcome`] — the
/// full enum carries payloads the timeline does not need).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum BankCreatePath {
    /// Identity fallback (shared pieces absent) or an unregister entry.
    None = 0,
    /// Full transaction ran; no token was exposed (ordinary stock bank).
    Stock = 1,
    /// Exposed token consumed and committed — THIS create carried our
    /// bound bank.
    Committed = 2,
    /// Exposed token consumed but XACT rejected the bank.
    LateFailed = 3,
    /// Exposure known, exact record unrecoverable.
    RecoveryFailed = 4,
    /// TLS stack exhausted; original ran with no binding attributable.
    TlsOverflow = 5,
}

impl BankCreatePath {
    fn from_bits(bits: u8) -> Self {
        match bits {
            1 => Self::Stock,
            2 => Self::Committed,
            3 => Self::LateFailed,
            4 => Self::RecoveryFailed,
            5 => Self::TlsOverflow,
            _ => Self::None,
        }
    }
}

/// One observed bank event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BankEvent {
    pub kind: BankEventKind,
    pub file_id: i32,
    /// The u8 the game received from `wavebank_create` (0 for unregister).
    pub status: u8,
    pub path: BankCreatePath,
    /// Milliseconds since the timeline was created, wrapping at 2^24
    /// (~4.6 h) — ordering plus rough deltas, not absolute time.
    pub tick_ms: u32,
}

const BANK_EVENT_VALID: u64 = 1 << 63;

fn pack_bank_event(event: BankEvent) -> u64 {
    let kind = match event.kind {
        BankEventKind::Create => 0u64,
        BankEventKind::Unregister => 1u64,
    };
    BANK_EVENT_VALID
        | (kind << 62)
        | ((event.path as u64 & 0x7) << 59)
        | ((u64::from(event.status)) << 51)
        | ((u64::from(event.file_id as u32)) << 19)
        | u64::from(event.tick_ms & 0x7_FFFF)
}

fn unpack_bank_event(packed: u64) -> BankEvent {
    BankEvent {
        kind: if packed >> 62 & 1 == 0 {
            BankEventKind::Create
        } else {
            BankEventKind::Unregister
        },
        path: BankCreatePath::from_bits((packed >> 59 & 0x7) as u8),
        status: (packed >> 51 & 0xFF) as u8,
        file_id: (packed >> 19 & 0xFFFF_FFFF) as u32 as i32,
        tick_ms: (packed & 0x7_FFFF) as u32,
    }
}

/// Bounded multi-producer/single-consumer timeline. Each slot is one
/// `AtomicU64` (0 = empty), handed off writer→reader by the valid bit and
/// back reader→writer by storing 0 — tear-free by construction. Overflow
/// drops the NEW event (counted); nothing blocks, nothing allocates.
pub struct BankTimeline {
    slots: [AtomicU64; BANK_TIMELINE_CAPACITY],
    enqueue_position: AtomicUsize,
    dequeue_position: AtomicUsize,
    dropped: AtomicUsize,
    epoch: std::time::Instant,
}

impl Default for BankTimeline {
    fn default() -> Self {
        Self::new()
    }
}

impl BankTimeline {
    #[must_use]
    pub fn new() -> Self {
        Self {
            slots: std::array::from_fn(|_| AtomicU64::new(0)),
            enqueue_position: AtomicUsize::new(0),
            dequeue_position: AtomicUsize::new(0),
            dropped: AtomicUsize::new(0),
            epoch: std::time::Instant::now(),
        }
    }

    /// Milliseconds since this timeline was created (wrapping into the
    /// packed 19-bit field on record).
    #[must_use]
    pub fn now_ms(&self) -> u32 {
        (self.epoch.elapsed().as_millis() & 0xFFFF_FFFF) as u32
    }

    /// Record one event (any thread; lock-free, allocation-free).
    pub fn record(&self, kind: BankEventKind, file_id: i32, status: u8, path: BankCreatePath) {
        let event = BankEvent {
            kind,
            file_id,
            status,
            path,
            tick_ms: self.now_ms() & 0x7_FFFF,
        };
        let position = self.enqueue_position.fetch_add(1, Ordering::AcqRel);
        let slot = &self.slots[position % BANK_TIMELINE_CAPACITY];
        // The slot must still be empty (the reader freed the previous lap);
        // otherwise the ring is full and the event is dropped, counted.
        if slot
            .compare_exchange(
                0,
                pack_bank_event(event),
                Ordering::AcqRel,
                Ordering::Relaxed,
            )
            .is_err()
        {
            self.dropped.fetch_add(1, Ordering::AcqRel);
        }
    }

    /// Pop the oldest event (single consumer — the maintenance drain).
    pub fn pop(&self) -> Option<BankEvent> {
        let position = self.dequeue_position.load(Ordering::Acquire);
        let slot = &self.slots[position % BANK_TIMELINE_CAPACITY];
        let packed = slot.load(Ordering::Acquire);
        if packed & BANK_EVENT_VALID == 0 {
            return None;
        }
        slot.store(0, Ordering::Release);
        self.dequeue_position.store(position + 1, Ordering::Release);
        Some(unpack_bank_event(packed))
    }

    /// Events dropped on overflow since the last call (resets the counter).
    pub fn take_dropped(&self) -> usize {
        self.dropped.swap(0, Ordering::AcqRel)
    }
}

unsafe impl Send for BankTimeline {}
unsafe impl Sync for BankTimeline {}
