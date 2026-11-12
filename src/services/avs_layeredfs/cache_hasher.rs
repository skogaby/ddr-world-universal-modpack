//! Cache hash invalidation — MD5 over (path, mtime) pairs persisted to a sidecar file.
//!
//! Used by the LayeredFS handlers (XML merge, ARC repack, etc.) to skip rebuilding
//! cached output when none of the inputs have changed since the last successful build.
//!
//! [`CacheHasher::add`] stats the input itself (`fs::metadata`). Under CrossOver
//! that is ~0.4 ms per call (open + query + close through wineserver), so a
//! caller fingerprinting thousands of files should take the mtime from the
//! directory listing it already did (`DirEntry::metadata` is free on Windows —
//! it comes out of the FindNextFile record) and use [`CacheHasher::add_stamped`],
//! which folds exactly the same bytes. [`CacheHasher::deferred`] likewise skips
//! reading the sidecar until [`CacheHasher::load_existing`] is asked for it.

use std::time::SystemTime;

use super::mod_paths;

pub const CACHE_FOLDER: &str = "./data_mods/_cache";

/// Hash-based cache invalidation. Hashes file paths and timestamps.
pub struct CacheHasher {
    hash_file: String,
    digest: md5::Context,
    existing_hash: [u8; 16],
    new_hash: [u8; 16],
}

impl CacheHasher {
    pub fn new(hash_file: &str) -> Self {
        let mut hasher = Self::deferred(hash_file);
        hasher.load_existing();
        hasher
    }

    /// A hasher that has NOT read its sidecar yet ([`Self::matches`] is false
    /// until [`Self::load_existing`]) — for callers that usually know the
    /// previous hash from an index of their own.
    pub fn deferred(hash_file: &str) -> Self {
        Self {
            hash_file: hash_file.to_string(),
            digest: md5::Context::new(),
            existing_hash: [0u8; 16],
            new_hash: [0u8; 16],
        }
    }

    /// Read the persisted hash from the sidecar (missing / malformed ⇒ none).
    pub fn load_existing(&mut self) {
        self.existing_hash = std::fs::read(&self.hash_file)
            .ok()
            .and_then(|data| <[u8; 16]>::try_from(data.as_slice()).ok())
            .unwrap_or([0u8; 16]);
    }

    pub fn add(&mut self, path: &str) {
        let modified = std::fs::metadata(path).ok().and_then(|m| m.modified().ok());
        self.add_stamped(path, modified);
    }

    /// [`Self::add`] with the modification time already known (`None` = the
    /// file could not be stat'ed: only the path is folded, as `add` does).
    pub fn add_stamped(&mut self, path: &str, modified: Option<SystemTime>) {
        self.digest.consume(path.as_bytes());
        if let Some(modified) = modified {
            let ts = modified
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            self.digest.consume(ts.to_le_bytes());
        }
    }

    /// Fold an arbitrary string into the hash (no filesystem lookup). Use for
    /// inputs that aren't files but still affect the output — e.g. atlas
    /// prefixes, texture names, or donor names — so adding/renaming a spec
    /// invalidates the cache even when no PNG mtime changed.
    pub fn add_str(&mut self, s: &str) {
        self.digest.consume(s.as_bytes());
    }

    pub fn finish(&mut self) {
        let result = self.digest.clone().compute();
        self.new_hash = result.into();
    }

    /// The hash computed by [`Self::finish`].
    pub fn new_hash(&self) -> [u8; 16] {
        self.new_hash
    }

    pub fn matches(&self) -> bool {
        self.existing_hash == self.new_hash && self.existing_hash != [0u8; 16]
    }

    pub fn commit(&self) {
        let folder = self
            .hash_file
            .rsplit_once('/')
            .map(|(f, _)| f)
            .unwrap_or(".");
        mod_paths::mkdir_p(folder);
        let _ = std::fs::write(&self.hash_file, self.new_hash);
    }
}
