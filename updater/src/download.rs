//! Streamed asset download with verification (design §4.4).
//!
//! The zip is written to `download/<asset name>` through a running SHA-256;
//! afterwards the byte count must equal the asset's declared size and, when
//! GitHub published a digest, the hash must match it. A failed or unverifiable
//! transfer leaves no partial file behind.

use std::fmt;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::github::{parse_digest, Asset, NetError};
use crate::manifest::hex;

pub struct Downloaded {
    pub path: PathBuf,
    pub sha256_hex: String,
    pub bytes: u64,
    /// True when the API carried no digest and only the size was checked.
    pub size_only: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum VerifyError {
    SizeMismatch { expected: u64, actual: u64 },
    DigestMismatch { expected: String, actual: String },
}

impl fmt::Display for VerifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VerifyError::SizeMismatch { expected, actual } => {
                write!(f, "download was {actual} bytes, expected {expected}")
            }
            VerifyError::DigestMismatch { expected, actual } => {
                write!(
                    f,
                    "download digest {actual} does not match the published {expected}"
                )
            }
        }
    }
}

/// Outcome of a successful verification.
#[derive(Debug, PartialEq, Eq)]
pub enum Verified {
    /// Size and digest both matched.
    Full,
    /// The asset carried no parseable digest; only the size matched.
    SizeOnly,
}

/// Pure check of a finished transfer against the asset descriptor.
pub fn verify(
    bytes_written: u64,
    sha256_hex: &str,
    asset: &Asset,
) -> Result<Verified, VerifyError> {
    if asset.size != 0 && bytes_written != asset.size {
        return Err(VerifyError::SizeMismatch {
            expected: asset.size,
            actual: bytes_written,
        });
    }
    match asset.digest.as_deref().and_then(parse_digest) {
        Some(expected) => {
            if expected.eq_ignore_ascii_case(sha256_hex) {
                Ok(Verified::Full)
            } else {
                Err(VerifyError::DigestMismatch {
                    expected,
                    actual: sha256_hex.to_string(),
                })
            }
        }
        None => Ok(Verified::SizeOnly),
    }
}

/// Download `asset` into `dest_dir`. `progress(done, total)` is called at the
/// start, whenever a 10 % boundary is crossed, and at the end.
pub fn fetch_asset(
    agent: &ureq::Agent,
    asset: &Asset,
    dest_dir: &Path,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<Downloaded, NetError> {
    fs::create_dir_all(dest_dir)
        .map_err(|e| NetError::Transport(format!("cannot create {}: {e}", dest_dir.display())))?;
    let path = dest_dir.join(&asset.name);
    let result = stream_to(agent, asset, &path, progress);
    if result.is_err() {
        let _ = fs::remove_file(&path);
    }
    result
}

fn stream_to(
    agent: &ureq::Agent,
    asset: &Asset,
    path: &Path,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<Downloaded, NetError> {
    let response = agent.get(&asset.browser_download_url).call()?;
    let mut reader = response.into_reader();
    let mut file = File::create(path)
        .map_err(|e| NetError::Transport(format!("cannot create {}: {e}", path.display())))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut done: u64 = 0;
    let total = asset.size;
    let mut next_tick = 0u64; // percent
    progress(0, total);
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| NetError::Transport(format!("read error during download: {e}")))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .map_err(|e| NetError::Transport(format!("cannot write {}: {e}", path.display())))?;
        hasher.update(&buf[..n]);
        done += n as u64;
        if total > 0 {
            let pct = done * 100 / total;
            while next_tick <= pct && next_tick <= 100 {
                progress(done, total);
                next_tick += 10;
            }
        }
    }
    file.flush()
        .map_err(|e| NetError::Transport(format!("cannot flush {}: {e}", path.display())))?;
    progress(done, total);
    let sha256_hex = hex(&hasher.finalize());
    let size_only = match verify(done, &sha256_hex, asset) {
        Ok(Verified::Full) => false,
        Ok(Verified::SizeOnly) => true,
        Err(e) => return Err(NetError::Transport(e.to_string())),
    };
    Ok(Downloaded {
        path: path.to_path_buf(),
        sha256_hex,
        bytes: done,
        size_only,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(size: u64, digest: Option<&str>) -> Asset {
        Asset {
            name: "ddr-world-universal-modpack-x.zip".into(),
            size,
            digest: digest.map(str::to_string),
            browser_download_url: "https://example.invalid/x.zip".into(),
        }
    }

    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    #[test]
    fn verify_rules() {
        let full = format!("sha256:{ABC}");
        assert_eq!(verify(3, ABC, &asset(3, Some(&full))), Ok(Verified::Full));
        assert_eq!(
            verify(3, &ABC.to_uppercase(), &asset(3, Some(&full))),
            Ok(Verified::Full)
        );
        assert_eq!(verify(3, ABC, &asset(3, None)), Ok(Verified::SizeOnly));
        assert_eq!(
            verify(3, ABC, &asset(3, Some("garbage"))),
            Ok(Verified::SizeOnly)
        );
        assert_eq!(
            verify(2, ABC, &asset(3, Some(&full))),
            Err(VerifyError::SizeMismatch {
                expected: 3,
                actual: 2
            })
        );
        assert!(matches!(
            verify(3, &"0".repeat(64), &asset(3, Some(&full))),
            Err(VerifyError::DigestMismatch { .. })
        ));
        // Unknown size (0) is not checked.
        assert_eq!(verify(99, ABC, &asset(0, Some(&full))), Ok(Verified::Full));
    }

    #[test]
    fn failed_transfer_leaves_no_partial_file() {
        let dir = std::env::temp_dir().join(format!("ddr_updater_download_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        // An unresolvable host fails fast at connect time.
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(std::time::Duration::from_millis(500))
            .build();
        let a = asset(3, None);
        let mut ticks = 0;
        let err = fetch_asset(&agent, &a, &dir, &mut |_, _| ticks += 1)
            .err()
            .expect("must fail");
        assert!(
            matches!(err, NetError::Transport(_) | NetError::Status(..)),
            "{err}"
        );
        assert!(!dir.join(&a.name).exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
