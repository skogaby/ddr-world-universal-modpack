//! Pure sequencing for transactional multi-hook installation.

pub fn install_all_or_rollback(
    count: usize,
    mut install: impl FnMut(usize) -> bool,
    mut rollback: impl FnMut(usize),
) -> bool {
    for index in 0..count {
        if !install(index) {
            for installed in (0..index).rev() {
                rollback(installed);
            }
            return false;
        }
    }
    true
}
