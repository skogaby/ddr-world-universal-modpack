//! The series thumbnail arc — `thumbnail/all.arc` + `all.digest` — built
//! from the ceiling-kept songs' `tn/<b>.dds`. Pure (`std`, `super::{cache,
//! model}`, `crate::core::art`): the prep tool builds it after indexing (the
//! normal path), and the DLL runs the same function at registration as a
//! fallback, which finds the digest up to date and does nothing.
//!
//! The arc's CONTENTS do not depend on the series value the game registers
//! at boot (members are `data/jacket/thumbnail/<b>_tn.dds`); only the name
//! the game asks for does (`jacket_thumbnails_<r>_<S>.arc`), and the
//! resolver maps that to this one file. Members for songs the game did not
//! inject (validation drops, a narrower DLL-side selection) are harmless —
//! never requested.

use std::time::Instant;

use crate::core::art;
use crate::log_info;

use super::cache;
use super::model::{self, SongEntry};

/// What `ensure` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Arc and digest already matched these entries.
    UpToDate,
    /// Rebuilt: members, bytes, placeholders regenerated, elapsed ms.
    Built {
        members: usize,
        bytes: usize,
        regenerated: usize,
        ms: u128,
    },
    /// Could not write the arc.
    Failed,
}

/// Digest sidecar text for `entries` (order-independent).
#[must_use]
pub fn digest_of(entries: &[SongEntry]) -> String {
    format!("{:016x}", model::index_digest(entries))
}

/// Build (or confirm) the arc for exactly `entries`. Stale `<N>.arc` files
/// from the per-series layout are removed.
pub fn ensure(entries: &[SongEntry]) -> Outcome {
    let digest = digest_of(entries);
    let arc_path = cache::thumbnail_arc_path();
    let digest_path = cache::thumbnail_digest_path();
    if arc_path.is_file()
        && std::fs::read_to_string(&digest_path)
            .map(|d| d.trim() == digest)
            .unwrap_or(false)
    {
        return Outcome::UpToDate;
    }
    let started = Instant::now();
    let mut members = Vec::with_capacity(entries.len());
    let mut regenerated = 0usize;
    for e in entries {
        let path = cache::tn_path(&e.basename);
        let dds = match std::fs::read(&path) {
            Ok(d) if art::thumbnail::is_thumbnail_dds(&d) => d,
            _ => {
                regenerated += 1;
                let dds = art::thumbnail_dds(&art::placeholder_jacket());
                let _ = cache::write_atomic(&path, &dds);
                dds
            }
        };
        members.push((e.basename.clone(), dds));
    }
    let bytes = art::thumbnail_arc(members);
    if let Ok(rd) = std::fs::read_dir(cache::root().join("thumbnail")) {
        for entry in rd.flatten() {
            let p = entry.path();
            if p != arc_path && p != digest_path {
                let _ = std::fs::remove_file(&p);
            }
        }
    }
    if !cache::write_atomic(&arc_path, &bytes) {
        return Outcome::Failed;
    }
    let _ = std::fs::write(&digest_path, &digest);
    Outcome::Built {
        members: entries.len(),
        bytes: bytes.len(),
        regenerated,
        ms: started.elapsed().as_millis(),
    }
}

/// `ensure` plus the one INFO line both sides log.
pub fn ensure_logged(entries: &[SongEntry], who: &str) -> Outcome {
    let outcome = ensure(entries);
    if let Outcome::Built {
        members,
        bytes,
        regenerated,
        ms,
    } = outcome
    {
        log_info!(
            "{}: thumbnail arc -- {} member(s), {} KiB, {} ms{}",
            who,
            members,
            bytes / 1024,
            ms,
            if regenerated > 0 {
                format!(" ({regenerated} placeholder(s) regenerated)")
            } else {
                String::new()
            }
        );
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::super::ceiling::{thumbnail_header_cost, THUMBNAIL_MEMBER_OVERHEAD};
    use super::*;

    #[test]
    fn ceiling_header_cost_matches_the_arc_writer() {
        // One cue (16 bytes) plus the NUL-terminated member path.
        for b in ["zab12c", "x", "a_longer_basename"] {
            let e = SongEntry {
                basename: b.to_string(),
                ..SongEntry::default()
            };
            assert_eq!(
                thumbnail_header_cost(&e),
                16 + art::arcpack::thumbnail_member(b).len() + 1
            );
        }
        assert_eq!(
            THUMBNAIL_MEMBER_OVERHEAD,
            16 + "data/jacket/thumbnail/_tn.dds".len() + 1
        );
    }
}
