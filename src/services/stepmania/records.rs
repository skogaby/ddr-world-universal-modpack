//! On-disk record checks shared by the prep tool's indexer and the DLL's
//! boot-time validation — pure (`std`, `super::{cache, convert, model}`).
//!
//! A `SongEntry` is trusted only while (a) its three artefacts exist in the
//! cache and (b) the stamps (`path`, size, mtime) of its simfile, audio and
//! art still hash to `entry.fingerprint`. Rejections are cached against a
//! stamp of every file directly in the song folder.

use std::path::{Path, PathBuf};

use super::cache;
use super::convert;
use super::model::{self, FileStamp, SongEntry};

fn stamp_of<'a>(path_rel: &'a str, path: &Path) -> Option<FileStamp<'a>> {
    let (size, mtime_secs) = cache::stat(path)?;
    Some(FileStamp {
        path: path_rel,
        size,
        mtime_secs,
    })
}

/// Relative (to the songs root, forward slashes) rendering of a path.
#[must_use]
pub fn rel_to_root(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Fingerprint of simfile + audio + art stamps.
#[must_use]
pub fn song_fingerprint(
    root: &Path,
    simfile: &Path,
    audio: &Path,
    art: Option<&Path>,
) -> Option<u64> {
    let sim_rel = rel_to_root(root, simfile);
    let audio_rel = rel_to_root(root, audio);
    let art_rel = art.map(|p| rel_to_root(root, p));
    let mut stamps = vec![stamp_of(&sim_rel, simfile)?, stamp_of(&audio_rel, audio)?];
    if let (Some(rel), Some(path)) = (art_rel.as_deref(), art) {
        stamps.push(stamp_of(rel, path)?);
    }
    Some(model::fingerprint(&stamps))
}

/// A cached entry's fingerprint recomputed from today's files. `simfile` is
/// the one discovery picks today (an `.ssc` added next to the `.sm` changes
/// it); `song_dir` is `<root>/<pack>/<folder>`.
#[must_use]
pub fn warm_fingerprint(
    root: &Path,
    song_dir: &Path,
    simfile: &Path,
    entry: &SongEntry,
) -> Option<u64> {
    let audio = song_dir.join(&entry.audio.rel_path);
    let art = entry.art.source.as_ref().map(|s| root.join(s));
    song_fingerprint(root, simfile, &audio, art.as_deref())
}

/// The song folder of a record.
#[must_use]
pub fn song_dir_of(root: &Path, entry: &SongEntry) -> PathBuf {
    root.join(&entry.pack).join(&entry.folder)
}

/// Every cache artefact the record promises exists.
#[must_use]
pub fn artefacts_present(entry: &SongEntry) -> bool {
    let ssq_ok = if entry.has_split_files() {
        entry
            .split_levels
            .iter()
            .enumerate()
            .filter(|(_, m)| **m != 0)
            .all(|(i, _)| cache::ssq_split_path(&entry.basename, i as u8 + 1).is_file())
    } else {
        cache::ssq_path(&entry.basename).is_file()
    };
    ssq_ok
        && cache::xsb_path(&entry.basename).is_file()
        && cache::tn_path(&entry.basename).is_file()
}

/// Stamp of every file directly in a song folder (names, sizes, mtimes),
/// salted with `FORMAT_VERSION` so a pipeline change re-tries rejections.
#[must_use]
pub fn folder_stamp(dir: &Path) -> u64 {
    let mut h = model::fnv1a64(&model::FORMAT_VERSION.to_le_bytes());
    for p in convert::list_files(dir) {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let (size, mtime) = cache::stat(&p).unwrap_or((0, 0));
        h = model::fnv1a64_continue(h, name.as_bytes());
        h = model::fnv1a64_continue(h, &size.to_le_bytes());
        h = model::fnv1a64_continue(h, &mtime.to_le_bytes());
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rel_to_root_uses_forward_slashes_and_tolerates_foreign_paths() {
        let root = Path::new("/g/stepmania/Songs");
        assert_eq!(
            rel_to_root(root, Path::new("/g/stepmania/Songs/P/S/a.ssc")),
            "P/S/a.ssc"
        );
        assert_eq!(rel_to_root(root, Path::new("/elsewhere/x")), "/elsewhere/x");
    }

    #[test]
    fn fingerprint_is_none_when_a_stamped_file_is_missing() {
        let root = Path::new("/definitely/not/here");
        assert!(song_fingerprint(root, &root.join("a.ssc"), &root.join("a.ogg"), None).is_none());
    }

    #[test]
    fn folder_stamp_tracks_contents() {
        let dir = std::env::temp_dir().join(format!("ddr_records_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let a = folder_stamp(&dir);
        std::fs::write(dir.join("x.sm"), b"#TITLE:x;").unwrap();
        let b = folder_stamp(&dir);
        assert_ne!(a, b);
        assert_eq!(b, folder_stamp(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
