//! Dev-only fault injection (`DDR_UPDATER_FAULT`), following the hook DLL's
//! `DDR_*_FAULT` precedent. Lets the integration tests exercise the rollback
//! paths that no ordinary run reaches.
//!
//! Values: `apply-after:<n>` — fail after `n` actions have been performed;
//! `rollback` — additionally make the first restore fail, so the run ends in
//! `RollbackFailed`; `crash-after:<n>` — exit the process after `n` actions
//! WITHOUT rolling back (leaves the journal for crash recovery). Anything else
//! (or unset) means no fault.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    None,
    /// Fail once `n` actions have completed.
    ApplyAfter(usize),
    /// Fail after the first action that has a backup AND make its restore fail.
    Rollback,
    /// Abort after `n` actions WITHOUT rolling back (simulates a crash/power
    /// loss mid-apply; the journal stays for the next run's recovery).
    CrashAfter(usize),
}

pub const ENV_VAR: &str = "DDR_UPDATER_FAULT";

pub fn from_env() -> Fault {
    parse(std::env::var(ENV_VAR).ok().as_deref())
}

pub fn parse(value: Option<&str>) -> Fault {
    match value.map(str::trim) {
        Some("rollback") => Fault::Rollback,
        Some(v) if v.starts_with("apply-after:") => v["apply-after:".len()..]
            .parse()
            .map(Fault::ApplyAfter)
            .unwrap_or(Fault::None),
        Some(v) if v.starts_with("crash-after:") => v["crash-after:".len()..]
            .parse()
            .map(Fault::CrashAfter)
            .unwrap_or(Fault::None),
        _ => Fault::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_table() {
        assert_eq!(parse(None), Fault::None);
        assert_eq!(parse(Some("")), Fault::None);
        assert_eq!(parse(Some("bogus")), Fault::None);
        assert_eq!(parse(Some("rollback")), Fault::Rollback);
        assert_eq!(parse(Some(" apply-after:2 ")), Fault::ApplyAfter(2));
        assert_eq!(parse(Some("apply-after:x")), Fault::None);
        assert_eq!(parse(Some("crash-after:1")), Fault::CrashAfter(1));
    }
}
