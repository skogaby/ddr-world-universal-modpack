//! Probe: HTTPS GET against the GitHub releases API with rustls (bundled
//! webpki roots — no OS trust store), parse JSON preserving key order, hash
//! the body, and prove the zip crate links. Exit 0 on success.
use sha2::{Digest, Sha256};
use std::io::Read;

fn main() {
    let url = "https://api.github.com/repos/skogaby/ddr-world-universal-modpack/releases/latest";
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(10))
        .timeout_read(std::time::Duration::from_secs(30))
        .user_agent("ddr-world-universal-modpack-updater-probe")
        .build();
    match agent.get(url).call() {
        Ok(resp) => {
            let mut body = String::new();
            resp.into_reader().take(4 << 20).read_to_string(&mut body).ok();
            let digest = Sha256::digest(body.as_bytes());
            match serde_json::from_str::<serde_json::Value>(&body) {
                Ok(v) => {
                    println!("tag_name = {}", v["tag_name"]);
                    for a in v["assets"].as_array().into_iter().flatten() {
                        println!("asset {} size={} digest={}", a["name"], a["size"], a["digest"]);
                    }
                }
                Err(e) => println!("json parse error: {e}"),
            }
            println!("body sha256 = {:x}", digest);
        }
        Err(e) => {
            println!("request failed: {e}");
            std::process::exit(2);
        }
    }
    // Prove the zip crate links (no real archive needed).
    let cursor = std::io::Cursor::new(Vec::<u8>::new());
    let _ = zip::ZipArchive::new(cursor).err();
    println!("probe ok");
}
