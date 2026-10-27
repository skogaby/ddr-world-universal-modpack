//! Boot-time validation of `index.bin` records — pure (`std` only).
//!
//! The DLL never converts; it trusts a record the prep tool wrote only while
//! the record still describes the disk. The impure half (`index.rs`) stats
//! the files on background workers and asks [`verdict`]; the per-cause
//! tallies become one WARN that tells the operator to run the tool again,
//! and the failed songs are hidden in the music DB.

use std::fmt;

/// Why a record was dropped this boot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropCause {
    /// An `ssq/` / `xsb/` / `tn/` artefact the record promises is gone.
    ArtefactMissing,
    /// The song folder, its simfile or a stamped source file is gone.
    SourceMissing,
    /// A stamped source file changed since the tool ran.
    SourceChanged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Keep,
    Dropped(DropCause),
}

/// The facts the impure side gathers for one record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Probe {
    /// `records::artefacts_present`.
    pub artefacts_present: bool,
    /// `convert::find_simfile(song_dir)` found a simfile.
    pub simfile_found: bool,
    /// `records::warm_fingerprint` (None when a stamped file is missing).
    pub fingerprint_now: Option<u64>,
}

/// Decide one record. Artefacts first (cache-local), then the source tree.
#[must_use]
pub fn verdict(probe: &Probe, recorded_fingerprint: u64) -> Verdict {
    if !probe.artefacts_present {
        return Verdict::Dropped(DropCause::ArtefactMissing);
    }
    if !probe.simfile_found {
        return Verdict::Dropped(DropCause::SourceMissing);
    }
    match probe.fingerprint_now {
        None => Verdict::Dropped(DropCause::SourceMissing),
        Some(fp) if fp != recorded_fingerprint => Verdict::Dropped(DropCause::SourceChanged),
        Some(_) => Verdict::Keep,
    }
}

/// Per-cause counts for the one-line WARN.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DropTally {
    pub artefacts: usize,
    pub sources: usize,
    pub changed: usize,
}

impl DropTally {
    pub fn record(&mut self, cause: DropCause) {
        match cause {
            DropCause::ArtefactMissing => self.artefacts += 1,
            DropCause::SourceMissing => self.sources += 1,
            DropCause::SourceChanged => self.changed += 1,
        }
    }

    #[must_use]
    pub fn total(&self) -> usize {
        self.artefacts + self.sources + self.changed
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.total() == 0
    }
}

impl fmt::Display for DropTally {
    /// `N song(s) hidden -- changed or missing since the last prep run (a
    /// artefact(s) missing, s source(s) missing, c changed) -- run
    /// ddr_world_hook_prep.exe`
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} song(s) hidden -- changed or missing since the last prep run ({} artefact(s) missing, {} source(s) missing, {} changed) -- run ddr_world_hook_prep.exe",
            self.total(),
            self.artefacts,
            self.sources,
            self.changed
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe(a: bool, s: bool, fp: Option<u64>) -> Probe {
        Probe {
            artefacts_present: a,
            simfile_found: s,
            fingerprint_now: fp,
        }
    }

    #[test]
    fn verdicts() {
        assert_eq!(verdict(&probe(true, true, Some(7)), 7), Verdict::Keep);
        assert_eq!(
            verdict(&probe(false, true, Some(7)), 7),
            Verdict::Dropped(DropCause::ArtefactMissing)
        );
        assert_eq!(
            verdict(&probe(true, false, Some(7)), 7),
            Verdict::Dropped(DropCause::SourceMissing)
        );
        assert_eq!(
            verdict(&probe(true, true, None), 7),
            Verdict::Dropped(DropCause::SourceMissing)
        );
        assert_eq!(
            verdict(&probe(true, true, Some(8)), 7),
            Verdict::Dropped(DropCause::SourceChanged)
        );
        // Artefacts are checked first.
        assert_eq!(
            verdict(&probe(false, false, None), 7),
            Verdict::Dropped(DropCause::ArtefactMissing)
        );
    }

    #[test]
    fn tally_text() {
        let mut t = DropTally::default();
        assert!(t.is_empty());
        t.record(DropCause::SourceChanged);
        t.record(DropCause::SourceChanged);
        t.record(DropCause::ArtefactMissing);
        assert_eq!(
            t.to_string(),
            "3 song(s) hidden -- changed or missing since the last prep run (1 artefact(s) missing, 0 source(s) missing, 2 changed) -- run ddr_world_hook_prep.exe"
        );
    }
}
