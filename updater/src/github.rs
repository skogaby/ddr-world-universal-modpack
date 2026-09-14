//! GitHub Releases feed client (design §4.3).
//!
//! One API request per run: `/releases/latest` (stable only — pre-releases and
//! drafts are invisible there) or, with `--include-prerelease`,
//! `/releases?per_page=10` followed by [`select_release`]. The transport is
//! rustls with bundled Mozilla roots, so neither Windows 7's TLS stack (no
//! TLS 1.2 unless patched) nor its root store is involved. Every failure is a
//! [`NetError`] that the caller turns into one "skipped (...)" line — the game
//! must start regardless.

use std::fmt;
use std::time::Duration;

use serde::Deserialize;

pub const API_BASE: &str = "https://api.github.com";
pub const ASSET_PREFIX: &str = "ddr-world-universal-modpack-";
pub const ASSET_SUFFIX: &str = ".zip";
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
pub const READ_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Release {
    pub tag_name: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub published_at: Option<String>,
    #[serde(default)]
    pub html_url: Option<String>,
    #[serde(default)]
    pub assets: Vec<Asset>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    #[serde(default)]
    pub size: u64,
    /// `"sha256:<64 hex>"` — present on every asset GitHub has served so far.
    #[serde(default)]
    pub digest: Option<String>,
    pub browser_download_url: String,
}

#[derive(Debug)]
pub enum NetError {
    /// DNS, connect, TLS, timeout, or a read error mid-transfer.
    Transport(String),
    /// The server answered with a non-2xx status.
    Status(u16, String),
    /// The response body was not the JSON we expected.
    BadJson(String),
    /// No release at all (empty list).
    NoRelease,
}

impl fmt::Display for NetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NetError::Transport(e) => write!(f, "could not reach GitHub: {e}"),
            NetError::Status(403, _) => write!(f, "GitHub API returned HTTP 403 (rate limited?)"),
            NetError::Status(404, _) => write!(
                f,
                "GitHub API returned HTTP 404 (repository or release not found)"
            ),
            NetError::Status(code, url) => write!(f, "GitHub returned HTTP {code} for {url}"),
            NetError::BadJson(e) => write!(f, "unexpected response from the GitHub API: {e}"),
            NetError::NoRelease => write!(f, "the repository has no releases"),
        }
    }
}

impl From<ureq::Error> for NetError {
    fn from(e: ureq::Error) -> Self {
        match e {
            ureq::Error::Status(code, resp) => NetError::Status(code, resp.get_url().to_string()),
            ureq::Error::Transport(t) => NetError::Transport(t.to_string()),
        }
    }
}

/// The HTTP agent every request goes through: timeouts per design R21 and the
/// `User-Agent` the GitHub API requires.
pub fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(CONNECT_TIMEOUT)
        .timeout_read(READ_TIMEOUT)
        .user_agent(&format!(
            "ddr_world_hook_updater/{}",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
}

/// The releases endpoint for `repo` (`owner/name`).
pub fn endpoint(repo: &str, include_prerelease: bool) -> String {
    if include_prerelease {
        format!("{API_BASE}/repos/{repo}/releases?per_page=10")
    } else {
        format!("{API_BASE}/repos/{repo}/releases/latest")
    }
}

/// Fetch the release to install.
pub fn fetch_latest(
    agent: &ureq::Agent,
    repo: &str,
    include_prerelease: bool,
) -> Result<Release, NetError> {
    let url = endpoint(repo, include_prerelease);
    let response = agent
        .get(&url)
        .set("Accept", "application/vnd.github+json")
        .call()?;
    if include_prerelease {
        let list: Vec<Release> = response
            .into_json()
            .map_err(|e| NetError::BadJson(e.to_string()))?;
        select_release(&list).cloned().ok_or(NetError::NoRelease)
    } else {
        response
            .into_json()
            .map_err(|e| NetError::BadJson(e.to_string()))
    }
}

/// Newest non-draft release by `published_at` (RFC 3339 strings compare
/// chronologically as text). Pre-releases are eligible here by design: this is
/// only called on the `--include-prerelease` path.
pub fn select_release(list: &[Release]) -> Option<&Release> {
    list.iter()
        .filter(|r| !r.draft)
        .max_by(|a, b| a.published_at.cmp(&b.published_at))
}

/// The one asset that is the release zip. With several candidates the first
/// wins (the caller logs a warning).
pub fn select_asset(release: &Release) -> Option<&Asset> {
    release
        .assets
        .iter()
        .find(|a| a.name.starts_with(ASSET_PREFIX) && a.name.ends_with(ASSET_SUFFIX))
}

/// How many assets look like the release zip (for the "several" warning).
pub fn matching_asset_count(release: &Release) -> usize {
    release
        .assets
        .iter()
        .filter(|a| a.name.starts_with(ASSET_PREFIX) && a.name.ends_with(ASSET_SUFFIX))
        .count()
}

/// Parse GitHub's `sha256:<64 hex>` digest into lowercase hex.
pub fn parse_digest(digest: &str) -> Option<String> {
    let hex = digest.strip_prefix("sha256:")?;
    if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(hex.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trimmed from the live `/releases` response of 2026-09-13.
    const V12: &str = r#"{
      "html_url": "https://github.com/skogaby/ddr-world-universal-modpack/releases/tag/v1.2",
      "id": 382377986,
      "tag_name": "v1.2",
      "name": "v1.2 - Bug-fixes, compatibility improvements, QoL enhancements, and split SSQ auto-loading",
      "draft": false, "prerelease": false,
      "published_at": "2026-09-03T22:58:33Z",
      "assets": [{
        "name": "ddr-world-universal-modpack-20260903_hotfix.zip",
        "content_type": "application/zip", "state": "uploaded", "size": 6396575,
        "digest": "sha256:bf3de838811fcd1da23c406feaecc9e112dae10a3b73c34d4e7931b804eac5df",
        "browser_download_url": "https://github.com/skogaby/ddr-world-universal-modpack/releases/download/v1.2/ddr-world-universal-modpack-20260903_hotfix.zip"
      }],
      "body": "Here's another quick-turnaround release."
    }"#;

    fn v12() -> Release {
        serde_json::from_str(V12).unwrap()
    }

    fn rel(tag: &str, published: &str, draft: bool, prerelease: bool) -> Release {
        Release {
            tag_name: tag.into(),
            name: None,
            body: None,
            draft,
            prerelease,
            published_at: Some(published.into()),
            html_url: None,
            assets: vec![],
        }
    }

    #[test]
    fn parses_live_shape_and_ignores_unknown_fields() {
        let r = v12();
        assert_eq!(r.tag_name, "v1.2");
        assert!(!r.prerelease && !r.draft);
        assert_eq!(r.assets.len(), 1);
        assert_eq!(r.assets[0].size, 6_396_575);
    }

    #[test]
    fn select_asset_picks_the_release_zip() {
        let r = v12();
        let a = select_asset(&r).unwrap();
        assert_eq!(a.name, "ddr-world-universal-modpack-20260903_hotfix.zip");
        assert_eq!(
            parse_digest(a.digest.as_deref().unwrap()).unwrap(),
            "bf3de838811fcd1da23c406feaecc9e112dae10a3b73c34d4e7931b804eac5df"
        );
    }

    #[test]
    fn select_asset_ignores_other_assets_and_prefers_first_match() {
        let mut r = v12();
        let mut other = r.assets[0].clone();
        other.name = "checksums.txt".into();
        let mut second = r.assets[0].clone();
        second.name = "ddr-world-universal-modpack-20260904.zip".into();
        r.assets = vec![other, r.assets[0].clone(), second];
        assert_eq!(
            select_asset(&r).unwrap().name,
            "ddr-world-universal-modpack-20260903_hotfix.zip"
        );
        assert_eq!(matching_asset_count(&r), 2);
        r.assets.clear();
        assert!(select_asset(&r).is_none());
    }

    #[test]
    fn digest_parsing_edge_cases() {
        assert!(parse_digest("sha256:").is_none());
        assert!(parse_digest("md5:abcd").is_none());
        assert!(parse_digest(&format!("sha256:{}", "g".repeat(64))).is_none());
        assert!(parse_digest(&format!("sha256:{}", "a".repeat(63))).is_none());
        assert_eq!(
            parse_digest(&format!("sha256:{}", "AB".repeat(32))).unwrap(),
            "ab".repeat(32)
        );
    }

    #[test]
    fn select_release_newest_non_draft_including_prereleases() {
        let list = vec![
            rel("v1.2", "2026-09-03T22:58:33Z", false, false),
            rel("v1.3-rc1", "2026-09-20T10:00:00Z", false, true),
            rel("v9-draft", "2026-12-01T00:00:00Z", true, false),
            rel("v1.1", "2026-09-02T03:55:05Z", false, false),
        ];
        assert_eq!(select_release(&list).unwrap().tag_name, "v1.3-rc1");
        let stable_only: Vec<Release> = list.iter().filter(|r| !r.prerelease).cloned().collect();
        assert_eq!(select_release(&stable_only).unwrap().tag_name, "v1.2");
        assert!(select_release(&[]).is_none());
        assert!(select_release(&[rel("d", "2026-01-01T00:00:00Z", true, false)]).is_none());
    }

    #[test]
    fn endpoints() {
        assert_eq!(
            endpoint("skogaby/ddr-world-universal-modpack", false),
            "https://api.github.com/repos/skogaby/ddr-world-universal-modpack/releases/latest"
        );
        assert_eq!(
            endpoint("a/b", true),
            "https://api.github.com/repos/a/b/releases?per_page=10"
        );
    }

    #[test]
    fn net_error_messages_are_operator_readable() {
        assert!(NetError::Status(403, "u".into())
            .to_string()
            .contains("rate limited"));
        assert!(NetError::Status(404, "u".into())
            .to_string()
            .contains("not found"));
        assert!(NetError::Transport("dns".into())
            .to_string()
            .contains("could not reach GitHub"));
    }
}
