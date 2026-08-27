use super::memory_patch::{
    allocate_rel32_block, apply_checked_patch, rel32_displacement, PatchBackend, PatchError,
    PatchStep,
};

const BASE: usize = 0x1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Protection {
    ReadExecute,
    ReadWriteExecute,
}

struct FakeMemory {
    bytes: Vec<u8>,
    protection: Protection,
    fail_once: Option<PatchStep>,
    allocation: Option<usize>,
}

impl FakeMemory {
    fn new(bytes: &[u8]) -> Self {
        Self {
            bytes: bytes.to_vec(),
            protection: Protection::ReadExecute,
            fail_once: None,
            allocation: Some(BASE + 0x1000),
        }
    }

    fn failing(bytes: &[u8], step: PatchStep) -> Self {
        let mut memory = Self::new(bytes);
        memory.fail_once = Some(step);
        memory
    }

    fn fail(&mut self, step: PatchStep) -> bool {
        if self.fail_once == Some(step) {
            self.fail_once = None;
            true
        } else {
            false
        }
    }
}

impl PatchBackend for FakeMemory {
    type Protection = Protection;

    fn read(&mut self, address: usize, length: usize, step: PatchStep) -> Result<Vec<u8>, ()> {
        if self.fail(step) {
            return Err(());
        }
        let start = address.checked_sub(BASE).ok_or(())?;
        let end = start.checked_add(length).ok_or(())?;
        self.bytes.get(start..end).map(ToOwned::to_owned).ok_or(())
    }

    fn make_writable(&mut self, _address: usize, _length: usize) -> Result<Self::Protection, ()> {
        if self.fail(PatchStep::MakeWritable) {
            return Err(());
        }
        let old = self.protection;
        self.protection = Protection::ReadWriteExecute;
        Ok(old)
    }

    fn write(&mut self, address: usize, bytes: &[u8], step: PatchStep) -> Result<(), ()> {
        let start = address.checked_sub(BASE).ok_or(())?;
        let end = start.checked_add(bytes.len()).ok_or(())?;
        let fail = self.fail(step);
        let destination = self.bytes.get_mut(start..end).ok_or(())?;
        if fail {
            destination[0] = bytes[0];
            return Err(());
        }
        destination.copy_from_slice(bytes);
        Ok(())
    }

    fn flush(&mut self, _address: usize, _length: usize, step: PatchStep) -> Result<(), ()> {
        if self.fail(step) {
            Err(())
        } else {
            Ok(())
        }
    }

    fn restore_protection(
        &mut self,
        _address: usize,
        _length: usize,
        protection: Self::Protection,
        step: PatchStep,
    ) -> Result<(), ()> {
        if self.fail(step) {
            return Err(());
        }
        self.protection = protection;
        Ok(())
    }

    fn allocate_near(&mut self, _near: usize, _size: usize) -> Option<usize> {
        self.allocation
    }
}

#[test]
fn checked_patch_writes_flushes_reads_back_and_restores_protection() {
    let original = [0x44, 0x8d, 0x34, 0x18, 0x4c, 0x8d, 0x67, 0x58];
    let replacement = [0xe9, 1, 2, 3, 4, 0x90, 0x90, 0x90];
    let mut memory = FakeMemory::new(&original);

    apply_checked_patch(&mut memory, BASE, &original, &replacement).unwrap();

    assert_eq!(memory.bytes, replacement);
    assert_eq!(memory.protection, Protection::ReadExecute);
}

#[test]
fn checked_patch_rejects_bad_lengths_and_expected_bytes_without_writing() {
    let original = [1, 2, 3, 4];
    let mut memory = FakeMemory::new(&original);
    assert_eq!(
        apply_checked_patch(&mut memory, BASE, &original, &[9, 9, 9]),
        Err(PatchError::LengthMismatch)
    );
    assert_eq!(
        apply_checked_patch(&mut memory, BASE, &[0, 2, 3, 4], &[9, 9, 9, 9]),
        Err(PatchError::ExpectedBytesMismatch)
    );
    assert_eq!(memory.bytes, original);
    assert_eq!(memory.protection, Protection::ReadExecute);
}

#[test]
fn every_post_protection_failure_rolls_back_original_bytes() {
    let original = [1, 2, 3, 4];
    let replacement = [9, 8, 7, 6];
    for step in [
        PatchStep::Write,
        PatchStep::Flush,
        PatchStep::Readback,
        PatchStep::RestoreProtection,
    ] {
        let mut memory = FakeMemory::failing(&original, step);
        assert_eq!(
            apply_checked_patch(&mut memory, BASE, &original, &replacement),
            Err(PatchError::Operation { step })
        );
        assert_eq!(memory.bytes, original, "failed at {step:?}");
        assert_eq!(memory.protection, Protection::ReadExecute);
    }

    let mut memory = FakeMemory::failing(&original, PatchStep::MakeWritable);
    assert_eq!(
        apply_checked_patch(&mut memory, BASE, &original, &replacement),
        Err(PatchError::Operation {
            step: PatchStep::MakeWritable
        })
    );
    assert_eq!(memory.bytes, original);
    assert_eq!(memory.protection, Protection::ReadExecute);

    let mut memory = FakeMemory::failing(&original, PatchStep::VerifyExpected);
    assert_eq!(
        apply_checked_patch(&mut memory, BASE, &original, &replacement),
        Err(PatchError::Operation {
            step: PatchStep::VerifyExpected
        })
    );
    assert_eq!(memory.bytes, original);
    assert_eq!(memory.protection, Protection::ReadExecute);
}

#[test]
fn rollback_failure_reports_primary_and_rollback_steps() {
    let original = [1, 2, 3, 4];
    let replacement = [9, 8, 7, 6];
    // The fake can inject only one step at a time, so make the primary write
    // fail after mutation and fail rollback by switching from inside write.
    struct RollbackFailure(FakeMemory);
    impl PatchBackend for RollbackFailure {
        type Protection = Protection;

        fn read(&mut self, a: usize, l: usize, s: PatchStep) -> Result<Vec<u8>, ()> {
            self.0.read(a, l, s)
        }
        fn make_writable(&mut self, a: usize, l: usize) -> Result<Self::Protection, ()> {
            self.0.make_writable(a, l)
        }
        fn write(&mut self, a: usize, b: &[u8], s: PatchStep) -> Result<(), ()> {
            if s == PatchStep::Write {
                self.0.write(a, b, s)?;
                self.0.fail_once = Some(PatchStep::RollbackWrite);
            }
            self.0.write(a, b, s)
        }
        fn flush(&mut self, a: usize, l: usize, s: PatchStep) -> Result<(), ()> {
            if s == PatchStep::Flush {
                return Err(());
            }
            self.0.flush(a, l, s)
        }
        fn restore_protection(
            &mut self,
            a: usize,
            l: usize,
            p: Self::Protection,
            s: PatchStep,
        ) -> Result<(), ()> {
            self.0.restore_protection(a, l, p, s)
        }
        fn allocate_near(&mut self, near: usize, size: usize) -> Option<usize> {
            self.0.allocate_near(near, size)
        }
    }

    let mut memory = RollbackFailure(FakeMemory::new(&original));
    assert_eq!(
        apply_checked_patch(&mut memory, BASE, &original, &replacement),
        Err(PatchError::Rollback {
            primary: PatchStep::Flush,
            rollback: PatchStep::RollbackWrite,
        })
    );
    assert_eq!(memory.0.protection, Protection::ReadExecute);
}

#[test]
fn rel32_and_near_allocation_reject_out_of_range_targets() {
    assert_eq!(rel32_displacement(0x1005, 0x2000).unwrap(), 0xffb);
    assert_eq!(rel32_displacement(0x2005, 0x1000).unwrap(), -0x1005);
    assert_eq!(
        rel32_displacement(0, usize::MAX),
        Err(PatchError::Rel32OutOfRange)
    );

    let mut memory = FakeMemory::new(&[0; 8]);
    assert_eq!(
        allocate_rel32_block(&mut memory, BASE, 64).unwrap(),
        BASE + 0x1000
    );
    memory.allocation = None;
    assert_eq!(
        allocate_rel32_block(&mut memory, BASE, 64),
        Err(PatchError::AllocationFailed)
    );
    memory.allocation = Some(usize::MAX);
    assert_eq!(
        allocate_rel32_block(&mut memory, BASE, 64),
        Err(PatchError::Rel32OutOfRange)
    );
}
