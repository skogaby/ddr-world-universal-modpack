//! Cache hash invalidation — MD5 over (path, mtime) pairs persisted to a sidecar file.
//!
//! Used by the LayeredFS handlers (XML merge, ARC repack, etc.) to skip rebuilding
//! cached output when none of the inputs have changed since the last successful build.

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
        let existing_hash = std::fs::read(hash_file)
            .ok()
            .and_then(|data| {
                if data.len() == 16 {
                    let mut arr = [0u8; 16];
                    arr.copy_from_slice(&data);
                    Some(arr)
                } else {
                    None
                }
            })
            .unwrap_or([0u8; 16]);

        Self {
            hash_file: hash_file.to_string(),
            digest: md5::Context::new(),
            existing_hash,
            new_hash: [0u8; 16],
        }
    }

    pub fn add(&mut self, path: &str) {
        self.digest.consume(path.as_bytes());
        if let Ok(meta) = std::fs::metadata(path) {
            if let Ok(modified) = meta.modified() {
                let ts = modified
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                self.digest.consume(ts.to_le_bytes());
            }
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
