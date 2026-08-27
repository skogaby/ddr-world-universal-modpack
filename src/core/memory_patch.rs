//! Checked, rollback-capable code-patch transactions.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatchStep {
    VerifyExpected,
    MakeWritable,
    Write,
    Flush,
    Readback,
    RestoreProtection,
    RollbackWrite,
    RollbackFlush,
    RollbackReadback,
    RollbackRestoreProtection,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatchError {
    LengthMismatch,
    ExpectedBytesMismatch,
    AllocationFailed,
    Rel32OutOfRange,
    Operation {
        step: PatchStep,
    },
    Rollback {
        primary: PatchStep,
        rollback: PatchStep,
    },
}

pub trait PatchBackend {
    type Protection: Copy;

    fn read(&mut self, address: usize, length: usize, step: PatchStep) -> Result<Vec<u8>, ()>;
    fn make_writable(&mut self, address: usize, length: usize) -> Result<Self::Protection, ()>;
    fn write(&mut self, address: usize, bytes: &[u8], step: PatchStep) -> Result<(), ()>;
    fn flush(&mut self, address: usize, length: usize, step: PatchStep) -> Result<(), ()>;
    fn restore_protection(
        &mut self,
        address: usize,
        length: usize,
        protection: Self::Protection,
        step: PatchStep,
    ) -> Result<(), ()>;
    fn allocate_near(&mut self, near: usize, size: usize) -> Option<usize>;
}

pub fn apply_checked_patch<B: PatchBackend>(
    backend: &mut B,
    address: usize,
    expected: &[u8],
    replacement: &[u8],
) -> Result<(), PatchError> {
    if expected.is_empty() || expected.len() != replacement.len() {
        return Err(PatchError::LengthMismatch);
    }
    let original = backend
        .read(address, expected.len(), PatchStep::VerifyExpected)
        .map_err(|_| PatchError::Operation {
            step: PatchStep::VerifyExpected,
        })?;
    if original != expected {
        return Err(PatchError::ExpectedBytesMismatch);
    }
    let protection = backend
        .make_writable(address, expected.len())
        .map_err(|_| PatchError::Operation {
            step: PatchStep::MakeWritable,
        })?;

    if backend
        .write(address, replacement, PatchStep::Write)
        .is_err()
    {
        return Err(rollback(
            backend,
            address,
            &original,
            protection,
            PatchStep::Write,
        ));
    }
    if backend
        .flush(address, replacement.len(), PatchStep::Flush)
        .is_err()
    {
        return Err(rollback(
            backend,
            address,
            &original,
            protection,
            PatchStep::Flush,
        ));
    }
    let readback = match backend.read(address, replacement.len(), PatchStep::Readback) {
        Ok(readback) if readback == replacement => readback,
        Ok(_) | Err(()) => {
            return Err(rollback(
                backend,
                address,
                &original,
                protection,
                PatchStep::Readback,
            ));
        }
    };
    debug_assert_eq!(readback, replacement);
    if backend
        .restore_protection(
            address,
            replacement.len(),
            protection,
            PatchStep::RestoreProtection,
        )
        .is_err()
    {
        return Err(rollback(
            backend,
            address,
            &original,
            protection,
            PatchStep::RestoreProtection,
        ));
    }
    Ok(())
}

fn rollback<B: PatchBackend>(
    backend: &mut B,
    address: usize,
    original: &[u8],
    protection: B::Protection,
    primary: PatchStep,
) -> PatchError {
    let mut failed_at = None;
    if backend
        .write(address, original, PatchStep::RollbackWrite)
        .is_err()
    {
        failed_at = Some(PatchStep::RollbackWrite);
    }
    if backend
        .flush(address, original.len(), PatchStep::RollbackFlush)
        .is_err()
    {
        failed_at.get_or_insert(PatchStep::RollbackFlush);
    }
    match backend.read(address, original.len(), PatchStep::RollbackReadback) {
        Ok(readback) if readback == original => {}
        Ok(_) | Err(()) => {
            failed_at.get_or_insert(PatchStep::RollbackReadback);
        }
    }
    if backend
        .restore_protection(
            address,
            original.len(),
            protection,
            PatchStep::RollbackRestoreProtection,
        )
        .is_err()
    {
        failed_at.get_or_insert(PatchStep::RollbackRestoreProtection);
    }
    if let Some(rollback) = failed_at {
        PatchError::Rollback { primary, rollback }
    } else {
        PatchError::Operation { step: primary }
    }
}

pub fn rel32_displacement(
    source_after_instruction: usize,
    target: usize,
) -> Result<i32, PatchError> {
    let displacement = (target as i128) - (source_after_instruction as i128);
    i32::try_from(displacement).map_err(|_| PatchError::Rel32OutOfRange)
}

pub fn allocate_rel32_block<B: PatchBackend>(
    backend: &mut B,
    jump_instruction: usize,
    size: usize,
) -> Result<usize, PatchError> {
    if size == 0 {
        return Err(PatchError::AllocationFailed);
    }
    let target = backend
        .allocate_near(jump_instruction, size)
        .filter(|address| *address != 0)
        .ok_or(PatchError::AllocationFailed)?;
    let jump_end = jump_instruction
        .checked_add(5)
        .ok_or(PatchError::Rel32OutOfRange)?;
    rel32_displacement(jump_end, target)?;
    Ok(target)
}
