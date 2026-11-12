//! Streamed download with verification and resume (design §4.4; optional
//! content packs design §4.7).
//!
//! A file is written to `<name>.part` through a running SHA-256 and renamed to
//! `<name>` only once the byte count equals the declared size and, when a
//! digest is known, the hash matches it. Without `resume` any failure leaves
//! nothing behind. With `resume` a transfer that was cut short keeps its part
//! file, and the next call continues it with a `Range` request; a part file is
//! still discarded whenever the result fails verification, so a resumed
//! download can never be assembled from bytes of two different files.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::github::{parse_digest, Asset, NetError};
use crate::manifest::hex;

/// What to download: a release asset, or any file a catalog names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// File name inside the destination folder.
    pub name: String,
    pub url: String,
    /// Expected size in bytes; 0 = unknown (not checked).
    pub size: u64,
    /// Expected SHA-256 (hex); `None` = only the size is verified.
    pub sha256: Option<String>,
}

impl Target {
    pub fn from_asset(asset: &Asset) -> Self {
        Target {
            name: asset.name.clone(),
            url: asset.browser_download_url.clone(),
            size: asset.size,
            sha256: asset.digest.as_deref().and_then(parse_digest),
        }
    }
}

pub struct Downloaded {
    pub path: PathBuf,
    pub sha256_hex: String,
    pub bytes: u64,
    /// True when no digest was known and only the size was checked.
    pub size_only: bool,
    /// Bytes taken from a part file an earlier run left (0 = downloaded whole).
    /// Always 0 for the base release, which is the only caller so far.
    #[allow(dead_code)]
    pub resumed_from: u64,
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
    /// No digest was known; only the size matched.
    SizeOnly,
}

/// Pure check of a finished transfer against its target.
pub fn verify(
    bytes_written: u64,
    sha256_hex: &str,
    target: &Target,
) -> Result<Verified, VerifyError> {
    if target.size != 0 && bytes_written != target.size {
        return Err(VerifyError::SizeMismatch {
            expected: target.size,
            actual: bytes_written,
        });
    }
    match &target.sha256 {
        Some(expected) => {
            if expected.eq_ignore_ascii_case(sha256_hex) {
                Ok(Verified::Full)
            } else {
                Err(VerifyError::DigestMismatch {
                    expected: expected.clone(),
                    actual: sha256_hex.to_string(),
                })
            }
        }
        None => Ok(Verified::SizeOnly),
    }
}

/// Download `target` into `dest_dir`. `progress(done, total)` is called at the
/// start, after every chunk written, and at the end; `done` counts resumed
/// bytes too. Callers decide how often to show it.
///
/// A [`NetError::Verify`] means the bytes were wrong and nothing was kept;
/// any other error under `resume` leaves `<name>.part` for the next call.
pub fn fetch(
    agent: &ureq::Agent,
    target: &Target,
    dest_dir: &Path,
    resume: bool,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<Downloaded, NetError> {
    fs::create_dir_all(dest_dir)
        .map_err(|e| NetError::Transport(format!("cannot create {}: {e}", dest_dir.display())))?;
    let path = dest_dir.join(&target.name);
    let part = dest_dir.join(format!("{}.part", target.name));
    if !resume {
        let _ = fs::remove_file(&part);
    }
    let finished = match transfer(agent, target, &part, progress) {
        Ok(f) => f,
        Err(e) => {
            if !resume || matches!(e, NetError::Verify(_)) {
                let _ = fs::remove_file(&part);
            }
            return Err(e);
        }
    };
    if let Err(e) = fs::rename(&part, &path) {
        let _ = fs::remove_file(&part);
        return Err(NetError::Transport(format!(
            "cannot move the download to {}: {e}",
            path.display()
        )));
    }
    Ok(Downloaded {
        path,
        sha256_hex: finished.sha256_hex,
        bytes: finished.bytes,
        size_only: finished.size_only,
        resumed_from: finished.resumed_from,
    })
}

struct Finished {
    sha256_hex: String,
    bytes: u64,
    size_only: bool,
    resumed_from: u64,
}

/// Why one request did not produce a file.
enum Attempt {
    /// The server would not continue from the part file's length: start over.
    Restart,
    Failed(NetError),
}

impl From<NetError> for Attempt {
    fn from(e: NetError) -> Self {
        Attempt::Failed(e)
    }
}

/// Fill `part`, continuing from whatever it already holds.
fn transfer(
    agent: &ureq::Agent,
    target: &Target,
    part: &Path,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<Finished, NetError> {
    let mut offset = fs::metadata(part).map(|m| m.len()).unwrap_or(0);
    if offset > 0 && target.size != 0 && offset >= target.size {
        // Already everything (an earlier run died before the rename), or junk.
        if offset == target.size {
            let sha256_hex = hash_file(part)?;
            if let Ok(verified) = verify(offset, &sha256_hex, target) {
                progress(offset, target.size);
                return Ok(Finished {
                    sha256_hex,
                    bytes: offset,
                    size_only: verified == Verified::SizeOnly,
                    resumed_from: offset,
                });
            }
        }
        offset = 0;
    }
    match attempt(agent, target, part, offset, progress) {
        Ok(f) => Ok(f),
        Err(Attempt::Failed(e)) => Err(e),
        Err(Attempt::Restart) => match attempt(agent, target, part, 0, progress) {
            Ok(f) => Ok(f),
            Err(Attempt::Failed(e)) => Err(e),
            Err(Attempt::Restart) => Err(NetError::Transport(
                "the server answered a plain request with a partial response".to_string(),
            )),
        },
    }
}

/// One request: from `offset` (with `Range`) when it is non-zero, else whole.
fn attempt(
    agent: &ureq::Agent,
    target: &Target,
    part: &Path,
    offset: u64,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<Finished, Attempt> {
    let mut request = agent.get(&target.url);
    if offset > 0 {
        request = request.set("Range", &format!("bytes={offset}-"));
    }
    let response = match request.call() {
        Ok(r) => r,
        Err(ureq::Error::Status(416, _)) if offset > 0 => return Err(Attempt::Restart),
        Err(e) => return Err(Attempt::Failed(e.into())),
    };

    // Where the body starts, and the whole file's size as the server states it.
    let (start, advertised) = if response.status() == 206 {
        match response
            .header("Content-Range")
            .and_then(parse_content_range)
        {
            Some((from, total)) if from == offset && offset > 0 => (offset, total),
            _ => return Err(Attempt::Restart),
        }
    } else {
        // 200: the whole file, whether or not a range was asked for.
        let length = response
            .header("Content-Length")
            .and_then(|v| v.trim().parse::<u64>().ok());
        (0, length)
    };
    if let Some(actual) = advertised {
        if target.size != 0 && actual != target.size {
            return Err(NetError::Verify(
                VerifyError::SizeMismatch {
                    expected: target.size,
                    actual,
                }
                .to_string(),
            )
            .into());
        }
    }

    let io_err = |what: &str, e: io::Error| {
        NetError::Transport(format!("cannot {what} {}: {e}", part.display()))
    };
    let mut hasher = Sha256::new();
    let mut file = if start > 0 {
        hash_into(part, &mut hasher).map_err(|e| io_err("read", e))?;
        OpenOptions::new()
            .append(true)
            .open(part)
            .map_err(|e| io_err("open", e))?
    } else {
        File::create(part).map_err(|e| io_err("create", e))?
    };

    let mut reader = response.into_reader();
    let mut buf = vec![0u8; 64 * 1024];
    let mut done = start;
    let total = target.size;
    progress(done, total);
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| NetError::Transport(format!("read error during download: {e}")))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| io_err("write", e))?;
        hasher.update(&buf[..n]);
        done += n as u64;
        progress(done, total);
    }
    file.flush().map_err(|e| io_err("flush", e))?;
    progress(done, total);
    if total != 0 && done < total {
        // Cut short, not wrong: what arrived is still a valid prefix.
        return Err(NetError::Transport(format!(
            "the connection closed after {done} of {total} bytes"
        ))
        .into());
    }
    let sha256_hex = hex(&hasher.finalize());
    let size_only = match verify(done, &sha256_hex, target) {
        Ok(Verified::Full) => false,
        Ok(Verified::SizeOnly) => true,
        Err(e) => return Err(NetError::Verify(e.to_string()).into()),
    };
    Ok(Finished {
        sha256_hex,
        bytes: done,
        size_only,
        resumed_from: start,
    })
}

/// `bytes <first>-<last>/<total>` → `(first, total)`; `total` is `None` for
/// `*`. Anything else (including the `bytes */<total>` of a 416) is `None`.
fn parse_content_range(value: &str) -> Option<(u64, Option<u64>)> {
    let (range, total) = value.trim().strip_prefix("bytes ")?.split_once('/')?;
    let (first, _last) = range.split_once('-')?;
    let first = first.trim().parse().ok()?;
    let total = match total.trim() {
        "*" => None,
        t => Some(t.parse().ok()?),
    };
    Some((first, total))
}

/// Feed a file's bytes into `hasher`.
fn hash_into(path: &Path, hasher: &mut Sha256) -> io::Result<()> {
    let mut file = File::open(path)?;
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            return Ok(());
        }
        hasher.update(&buf[..n]);
    }
}

fn hash_file(path: &Path) -> Result<String, NetError> {
    let mut hasher = Sha256::new();
    hash_into(path, &mut hasher)
        .map_err(|e| NetError::Transport(format!("cannot read {}: {e}", path.display())))?;
    Ok(hex(&hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::net::{SocketAddr, TcpListener, TcpStream};
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread::JoinHandle;
    use std::time::Duration;

    use crate::manifest::sha256_hex;

    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    const NAME: &str = "ddr-world-pack-test.zip";
    const HEAD: usize = 70_000;

    fn target(size: u64, sha256: Option<&str>) -> Target {
        Target {
            name: NAME.into(),
            url: "https://example.invalid/x.zip".into(),
            size,
            sha256: sha256.map(str::to_string),
        }
    }

    fn payload() -> Vec<u8> {
        (0..200_000u32).map(|i| (i * 31 + i / 251) as u8).collect()
    }

    /// A unique scratch folder, removed on drop.
    struct Temp(PathBuf);

    impl Temp {
        fn new() -> Self {
            static N: AtomicU32 = AtomicU32::new(0);
            let dir = std::env::temp_dir().join(format!(
                "ddr_updater_download_{}_{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Temp(dir)
        }

        fn part(&self) -> PathBuf {
            self.0.join(format!("{NAME}.part"))
        }

        fn file(&self) -> PathBuf {
            self.0.join(NAME)
        }

        fn is_empty(&self) -> bool {
            fs::read_dir(&self.0).unwrap().next().is_none()
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// How the test server treats a request for `/file`.
    #[derive(Clone, Copy)]
    enum Mode {
        /// `206` for a satisfiable `Range`, `416` past the end, `200` otherwise.
        Honour,
        /// Always `200` with the whole payload.
        IgnoreRange,
        /// `416` for any ranged request.
        Refuse,
        /// `206` for a ranged request, but from offset 0.
        WrongOffset,
        /// Honest headers, then the connection closes after this many body bytes.
        CutAt(usize),
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Seen {
        path: String,
        range: Option<String>,
    }

    /// A one-thread HTTP/1.1 server: `/file` serves the payload per [`Mode`],
    /// `/redirect` answers `302` to `/file`. Every response closes the
    /// connection.
    struct TestServer {
        addr: SocketAddr,
        seen: Arc<Mutex<Vec<Seen>>>,
        stop: Arc<AtomicBool>,
        thread: Option<JoinHandle<()>>,
    }

    impl TestServer {
        fn start(payload: Vec<u8>, mode: Mode) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = listener.local_addr().unwrap();
            let seen = Arc::new(Mutex::new(Vec::new()));
            let stop = Arc::new(AtomicBool::new(false));
            let (seen_t, stop_t) = (Arc::clone(&seen), Arc::clone(&stop));
            let thread = std::thread::spawn(move || {
                for stream in listener.incoming() {
                    if stop_t.load(Ordering::SeqCst) {
                        break;
                    }
                    if let Ok(stream) = stream {
                        serve(stream, addr, &payload, mode, &seen_t);
                    }
                }
            });
            TestServer {
                addr,
                seen,
                stop,
                thread: Some(thread),
            }
        }

        fn url(&self, path: &str) -> String {
            format!("http://{}{path}", self.addr)
        }

        fn seen(&self) -> Vec<Seen> {
            self.seen.lock().unwrap().clone()
        }

        fn target(&self, path: &str, payload: &[u8]) -> Target {
            Target {
                name: NAME.into(),
                url: self.url(path),
                size: payload.len() as u64,
                sha256: Some(sha256_hex(payload)),
            }
        }
    }

    impl Drop for TestServer {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
            let _ = TcpStream::connect(self.addr); // unblock accept()
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }

    fn serve(
        mut stream: TcpStream,
        addr: SocketAddr,
        payload: &[u8],
        mode: Mode,
        seen: &Mutex<Vec<Seen>>,
    ) {
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut request_line = String::new();
        if reader.read_line(&mut request_line).unwrap_or(0) == 0 {
            return; // the Drop wake-up connection
        }
        let path = request_line
            .split_whitespace()
            .nth(1)
            .unwrap_or("")
            .to_string();
        let mut range = None;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                break;
            }
            if let Some((name, value)) = line.split_once(':') {
                if name.eq_ignore_ascii_case("range") {
                    range = Some(value.trim().to_string());
                }
            }
        }
        seen.lock().unwrap().push(Seen {
            path: path.clone(),
            range: range.clone(),
        });

        let total = payload.len();
        let start = range
            .as_deref()
            .and_then(|r| r.strip_prefix("bytes="))
            .and_then(|r| r.strip_suffix('-'))
            .and_then(|n| n.parse::<usize>().ok());
        let mut send = |status: &str, extra: &str, body: &[u8], cut: Option<usize>| {
            let head = format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body[..cut.unwrap_or(body.len()).min(body.len())]);
            let _ = stream.flush();
        };
        let partial =
            |from: usize| format!("Content-Range: bytes {from}-{}/{total}\r\n", total - 1);
        let unsatisfiable = format!("Content-Range: bytes */{total}\r\n");

        if path == "/redirect" {
            send(
                "302 Found",
                &format!("Location: http://{addr}/file\r\n"),
                b"",
                None,
            );
            return;
        }
        match (mode, start) {
            (Mode::IgnoreRange, _) | (_, None) => {
                let cut = match mode {
                    Mode::CutAt(n) => Some(n),
                    _ => None,
                };
                send("200 OK", "", payload, cut);
            }
            (Mode::Refuse, Some(_)) => send("416 Range Not Satisfiable", &unsatisfiable, b"", None),
            (Mode::WrongOffset, Some(_)) => send("206 Partial Content", &partial(0), payload, None),
            (Mode::Honour | Mode::CutAt(_), Some(from)) if from >= total => {
                send("416 Range Not Satisfiable", &unsatisfiable, b"", None)
            }
            (Mode::Honour, Some(from)) => send(
                "206 Partial Content",
                &partial(from),
                &payload[from..],
                None,
            ),
            (Mode::CutAt(n), Some(from)) => send(
                "206 Partial Content",
                &partial(from),
                &payload[from..],
                Some(n),
            ),
        }
    }

    fn test_agent() -> ureq::Agent {
        ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(2))
            .timeout_read(Duration::from_secs(5))
            .build()
    }

    fn run(target: &Target, dir: &Temp, resume: bool) -> Result<Downloaded, NetError> {
        fetch(&test_agent(), target, &dir.0, resume, &mut |_, _| {})
    }

    #[test]
    fn verify_rules() {
        assert_eq!(verify(3, ABC, &target(3, Some(ABC))), Ok(Verified::Full));
        assert_eq!(
            verify(3, &ABC.to_uppercase(), &target(3, Some(ABC))),
            Ok(Verified::Full)
        );
        assert_eq!(verify(3, ABC, &target(3, None)), Ok(Verified::SizeOnly));
        assert_eq!(
            verify(2, ABC, &target(3, Some(ABC))),
            Err(VerifyError::SizeMismatch {
                expected: 3,
                actual: 2
            })
        );
        assert!(matches!(
            verify(3, &"0".repeat(64), &target(3, Some(ABC))),
            Err(VerifyError::DigestMismatch { .. })
        ));
        // Unknown size (0) is not checked.
        assert_eq!(verify(99, ABC, &target(0, Some(ABC))), Ok(Verified::Full));
    }

    #[test]
    fn target_from_asset_parses_the_published_digest() {
        let mut asset = Asset {
            name: "a.zip".into(),
            size: 3,
            digest: Some(format!("sha256:{ABC}")),
            browser_download_url: "https://example.invalid/a.zip".into(),
        };
        let t = Target::from_asset(&asset);
        assert_eq!(
            (t.name.as_str(), t.url.as_str(), t.size),
            ("a.zip", "https://example.invalid/a.zip", 3)
        );
        assert_eq!(t.sha256.as_deref(), Some(ABC));
        asset.digest = Some("garbage".into());
        assert_eq!(Target::from_asset(&asset).sha256, None);
    }

    #[test]
    fn failed_transfer_leaves_no_partial_file() {
        let dir = Temp::new();
        // An unresolvable host fails fast at connect time.
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_millis(500))
            .build();
        let mut ticks = 0;
        let err = fetch(&agent, &target(3, None), &dir.0, false, &mut |_, _| {
            ticks += 1
        })
        .err()
        .expect("must fail");
        assert!(
            matches!(err, NetError::Transport(_) | NetError::Status(..)),
            "{err}"
        );
        assert!(dir.is_empty());
    }

    #[test]
    fn fresh_download_with_and_without_resume() {
        let data = payload();
        let server = TestServer::start(data.clone(), Mode::Honour);
        for resume in [false, true] {
            let dir = Temp::new();
            let mut last = (0, 0);
            let got = fetch(
                &test_agent(),
                &server.target("/file", &data),
                &dir.0,
                resume,
                &mut |done, total| last = (done, total),
            )
            .unwrap();
            assert_eq!(got.path, dir.file());
            assert_eq!(fs::read(dir.file()).unwrap(), data);
            assert_eq!(got.bytes, data.len() as u64);
            assert_eq!(got.sha256_hex, sha256_hex(&data));
            assert_eq!(got.resumed_from, 0);
            assert!(!got.size_only);
            assert!(!dir.part().exists(), "resume={resume}");
            assert_eq!(last, (data.len() as u64, data.len() as u64));
        }
        assert!(server.seen().iter().all(|s| s.range.is_none()));
    }

    #[test]
    fn resume_appends_to_the_part_file() {
        let data = payload();
        let server = TestServer::start(data.clone(), Mode::Honour);
        let dir = Temp::new();
        fs::write(dir.part(), &data[..HEAD]).unwrap();
        let got = run(&server.target("/file", &data), &dir, true).unwrap();
        assert_eq!(got.resumed_from, HEAD as u64);
        assert_eq!(got.bytes, data.len() as u64);
        assert_eq!(fs::read(dir.file()).unwrap(), data);
        assert!(!dir.part().exists());
        assert_eq!(
            server.seen(),
            vec![Seen {
                path: "/file".into(),
                range: Some(format!("bytes={HEAD}-"))
            }]
        );
    }

    #[test]
    fn range_header_survives_a_redirect() {
        let data = payload();
        let server = TestServer::start(data.clone(), Mode::Honour);
        let dir = Temp::new();
        fs::write(dir.part(), &data[..HEAD]).unwrap();
        let got = run(&server.target("/redirect", &data), &dir, true).unwrap();
        assert_eq!(got.resumed_from, HEAD as u64);
        assert_eq!(fs::read(dir.file()).unwrap(), data);
        let seen = server.seen();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[1].path, "/file");
        assert_eq!(seen[1].range, Some(format!("bytes={HEAD}-")));
    }

    #[test]
    fn a_part_file_is_ignored_without_resume() {
        let data = payload();
        let server = TestServer::start(data.clone(), Mode::Honour);
        let dir = Temp::new();
        fs::write(dir.part(), b"stale bytes from another run").unwrap();
        let got = run(&server.target("/file", &data), &dir, false).unwrap();
        assert_eq!(got.resumed_from, 0);
        assert_eq!(fs::read(dir.file()).unwrap(), data);
        assert_eq!(server.seen()[0].range, None);
    }

    #[test]
    fn unusable_range_answers_restart_from_zero() {
        let data = payload();
        for (mode, requests) in [
            (Mode::IgnoreRange, 1),
            (Mode::Refuse, 2),
            (Mode::WrongOffset, 2),
        ] {
            let server = TestServer::start(data.clone(), mode);
            let dir = Temp::new();
            // Junk of the right length for a prefix: a restart must not keep it.
            fs::write(dir.part(), vec![0xEE; HEAD]).unwrap();
            let got = run(&server.target("/file", &data), &dir, true).unwrap();
            assert_eq!(got.resumed_from, 0);
            assert_eq!(fs::read(dir.file()).unwrap(), data);
            assert!(!dir.part().exists());
            let seen = server.seen();
            assert_eq!(seen.len(), requests);
            assert_eq!(seen[0].range, Some(format!("bytes={HEAD}-")));
            if requests == 2 {
                assert_eq!(seen[1].range, None, "the retry must be a plain request");
            }
        }
    }

    #[test]
    fn interrupted_transfer_keeps_the_part_only_with_resume() {
        let data = payload();
        const CUT: usize = 50_000;

        let dir = Temp::new();
        {
            let broken = TestServer::start(data.clone(), Mode::CutAt(CUT));
            let err = run(&broken.target("/file", &data), &dir, true)
                .err()
                .expect("must fail");
            assert!(matches!(err, NetError::Transport(_)), "{err}");
        }
        let kept = fs::metadata(dir.part()).unwrap().len();
        assert!(kept > 0 && kept <= CUT as u64, "kept {kept}");
        assert!(!dir.file().exists());

        let healthy = TestServer::start(data.clone(), Mode::Honour);
        let got = run(&healthy.target("/file", &data), &dir, true).unwrap();
        assert_eq!(got.resumed_from, kept);
        assert_eq!(fs::read(dir.file()).unwrap(), data);
        assert_eq!(healthy.seen()[0].range, Some(format!("bytes={kept}-")));

        let dir = Temp::new();
        let broken = TestServer::start(data.clone(), Mode::CutAt(CUT));
        let err = run(&broken.target("/file", &data), &dir, false)
            .err()
            .expect("must fail");
        assert!(matches!(err, NetError::Transport(_)), "{err}");
        assert!(dir.is_empty());
    }

    #[test]
    fn verification_failures_leave_nothing_and_are_not_transport_errors() {
        let data = payload();
        let server = TestServer::start(data.clone(), Mode::Honour);
        for resume in [false, true] {
            // Wrong digest.
            let dir = Temp::new();
            let mut t = server.target("/file", &data);
            t.sha256 = Some("0".repeat(64));
            let err = run(&t, &dir, resume).err().expect("must fail");
            assert!(matches!(err, NetError::Verify(_)), "{err}");
            assert!(dir.is_empty(), "resume={resume}");

            // The server advertises another total size than the target names.
            for wrong in [data.len() as u64 + 10, data.len() as u64 - 10] {
                let dir = Temp::new();
                let mut t = server.target("/file", &data);
                t.size = wrong;
                let err = run(&t, &dir, resume).err().expect("must fail");
                assert!(matches!(err, NetError::Verify(_)), "{err}");
                assert!(dir.is_empty(), "resume={resume} wrong={wrong}");
            }
        }
        // Same check on the ranged path: the 206's total disagrees.
        let dir = Temp::new();
        fs::write(dir.part(), &data[..HEAD]).unwrap();
        let mut t = server.target("/file", &data);
        t.size = data.len() as u64 + 10;
        let err = run(&t, &dir, true).err().expect("must fail");
        assert!(matches!(err, NetError::Verify(_)), "{err}");
        assert!(dir.is_empty());
    }

    #[test]
    fn a_complete_part_file_needs_no_request() {
        let data = payload();
        let server = TestServer::start(data.clone(), Mode::Honour);
        let dir = Temp::new();
        fs::write(dir.part(), &data).unwrap();
        let got = run(&server.target("/file", &data), &dir, true).unwrap();
        assert_eq!(got.resumed_from, data.len() as u64);
        assert_eq!(got.sha256_hex, sha256_hex(&data));
        assert_eq!(fs::read(dir.file()).unwrap(), data);
        assert!(server.seen().is_empty());
    }

    #[test]
    fn a_full_length_part_with_wrong_bytes_restarts() {
        let data = payload();
        let server = TestServer::start(data.clone(), Mode::Honour);
        for len in [data.len(), data.len() + 5] {
            let dir = Temp::new();
            fs::write(dir.part(), vec![0xEE; len]).unwrap();
            let got = run(&server.target("/file", &data), &dir, true).unwrap();
            assert_eq!(got.resumed_from, 0);
            assert_eq!(fs::read(dir.file()).unwrap(), data);
        }
        assert!(server.seen().iter().all(|s| s.range.is_none()));
    }

    #[test]
    fn content_range_parsing() {
        assert_eq!(
            parse_content_range("bytes 1000-1999/26626795"),
            Some((1000, Some(26626795)))
        );
        assert_eq!(parse_content_range("bytes 5-9/*"), Some((5, None)));
        assert_eq!(parse_content_range("bytes */100"), None);
        assert_eq!(parse_content_range("items 0-1/2"), None);
        assert_eq!(parse_content_range(""), None);
    }

    /// Design §4.7: GitHub's release download answers with a redirect to a
    /// signed URL. This proves a ranged request survives it, end to end, by
    /// resuming the published v1.4 asset from its first mebibyte and checking
    /// the result against the digest GitHub publishes.
    #[test]
    #[ignore = "network: downloads the published v1.4 release asset (27 MB)"]
    fn live_range_resume_across_github_redirect() {
        const FIRST: u64 = 1 << 20;
        let agent = crate::github::agent();
        let release: crate::github::Release = agent
            .get(&format!(
                "{}/repos/{}/releases/tags/v1.4",
                crate::github::API_BASE,
                crate::cli::REPO_DEFAULT
            ))
            .set("Accept", "application/vnd.github+json")
            .call()
            .expect("GitHub API")
            .into_json()
            .expect("release JSON");
        let asset = crate::github::select_asset(&release).expect("v1.4 release zip");
        let target = Target::from_asset(asset);
        assert!(target.sha256.is_some(), "GitHub publishes a digest");

        let head = agent
            .get(&target.url)
            .set("Range", &format!("bytes=0-{}", FIRST - 1))
            .call()
            .expect("ranged request");
        assert_eq!(
            head.status(),
            206,
            "Range was not honoured across the redirect"
        );
        let mut first = Vec::new();
        head.into_reader().read_to_end(&mut first).unwrap();
        assert_eq!(first.len() as u64, FIRST);

        let dir = Temp::new();
        fs::write(dir.0.join(format!("{}.part", target.name)), &first).unwrap();
        let got = fetch(&agent, &target, &dir.0, true, &mut |_, _| {}).expect("resumed download");
        assert_eq!(
            got.resumed_from, FIRST,
            "the download restarted instead of resuming"
        );
        assert_eq!(got.bytes, target.size);
        assert!(!got.size_only);
        assert_eq!(Some(got.sha256_hex), target.sha256);
    }
}
