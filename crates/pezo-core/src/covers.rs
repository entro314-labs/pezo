use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::time::Duration;

use crate::config::LibraryPaths;
use crate::error::{Result, io_err};

/// Largest cover we'll download (guards against a bad CDN response).
const MAX_COVER_BYTES: u64 = 4 * 1024 * 1024;

/// Existing cached cover for a slug, if previously fetched.
pub fn cover_path(paths: &LibraryPaths, slug: &str) -> Option<PathBuf> {
    let path = paths.covers().join(format!("{slug}.jpg"));
    path.is_file().then_some(path)
}

/// Fetch portrait box art for a game, best effort: a known Steam appid is
/// used directly, otherwise the Steam storefront is searched by title.
/// Network problems and misses return Ok(None) — never an install-breaking
/// error. The image lands in `<library>/.covers/<slug>.jpg`.
pub fn fetch_cover(
    paths: &LibraryPaths,
    slug: &str,
    title: &str,
    steam_appid: Option<&str>,
) -> Result<Option<PathBuf>> {
    if let Some(existing) = cover_path(paths, slug) {
        return Ok(Some(existing));
    }

    // Generous timeout: hosts with broken IPv6 can spend several seconds per
    // connection before falling back to IPv4.
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(20))
        .build();

    let appid = match steam_appid {
        Some(id) => Some(id.to_string()),
        None => search_appid(&agent, title),
    };
    let Some(appid) = appid else {
        tracing::debug!(title, "no steam match for cover");
        return Ok(None);
    };

    // Portrait library asset first (matches the tile aspect), header as backup.
    let urls = [
        format!("https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/{appid}/library_600x900.jpg"),
        format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{appid}/library_600x900.jpg"),
        format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{appid}/header.jpg"),
    ];
    for url in &urls {
        let Ok(response) = agent.get(url).call() else {
            continue;
        };
        let mut bytes = Vec::new();
        if response
            .into_reader()
            .take(MAX_COVER_BYTES)
            .read_to_end(&mut bytes)
            .is_err()
            || bytes.is_empty()
        {
            continue;
        }
        let dir = paths.covers();
        fs::create_dir_all(&dir).map_err(io_err(&dir))?;
        let dest = dir.join(format!("{slug}.jpg"));
        fs::write(&dest, &bytes).map_err(io_err(&dest))?;
        tracing::info!(slug, url, "cover fetched");
        return Ok(Some(dest));
    }
    Ok(None)
}

/// Best-match Steam appid for a title via the storefront search API.
fn search_appid(agent: &ureq::Agent, title: &str) -> Option<String> {
    let url = format!(
        "https://store.steampowered.com/api/storesearch/?term={}&l=english&cc=US",
        urlencode(title)
    );
    let body: serde_json::Value = agent.get(&url).call().ok()?.into_json().ok()?;
    let id = body.get("items")?.get(0)?.get("id")?;
    Some(id.as_i64()?.to_string())
}

fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Delete a cached cover (used when a game is removed).
pub fn remove_cover(paths: &LibraryPaths, slug: &str) {
    if let Some(path) = cover_path(paths, slug) {
        let _ = fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "hits the live Steam storefront"]
    fn fetches_a_real_cover() {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths = LibraryPaths::new(dir.path().to_path_buf());
        let fetched = fetch_cover(&paths, "hades", "Hades", None).expect("fetch");
        let path = fetched.expect("cover found");
        let bytes = fs::read(&path).expect("read cover");
        assert!(bytes.len() > 10_000, "cover too small: {} bytes", bytes.len());
        assert_eq!(cover_path(&paths, "hades"), Some(path));
    }
}
