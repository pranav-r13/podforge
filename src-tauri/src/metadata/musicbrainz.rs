use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// MusicBrainz ToS requires a descriptive User-Agent identifying the app.
/// TODO(Phase 6): move to Settings (`mb_user_agent`) once the settings
/// screen exists, so users can put their own contact info in it.
const USER_AGENT: &str = "Podforge/0.1.0 ( https://github.com/pranav-r13/podforge )";

const SEARCH_URL: &str = "https://musicbrainz.org/ws/2/release/";

#[derive(Debug, Clone, Serialize)]
pub struct MbCandidate {
    pub release_id: String,
    pub title: String,
    pub artist: String,
    pub date: Option<String>,
    pub country: Option<String>,
    pub disambiguation: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    #[serde(default)]
    releases: Vec<ReleaseJson>,
}

#[derive(Debug, Deserialize)]
struct ReleaseJson {
    id: String,
    title: String,
    date: Option<String>,
    country: Option<String>,
    disambiguation: Option<String>,
    #[serde(rename = "artist-credit", default)]
    artist_credit: Vec<ArtistCreditJson>,
}

#[derive(Debug, Deserialize)]
struct ArtistCreditJson {
    name: String,
}

/// MusicBrainz asks for at most ~1 request/sec from unauthenticated
/// clients. Every call through this module blocks here first.
fn rate_limit() {
    static LAST_REQUEST: OnceLock<Mutex<Instant>> = OnceLock::new();
    let lock = LAST_REQUEST.get_or_init(|| Mutex::new(Instant::now() - Duration::from_secs(2)));
    let mut last = lock.lock().expect("rate limit mutex poisoned");
    let elapsed = last.elapsed();
    if elapsed < Duration::from_secs(1) {
        std::thread::sleep(Duration::from_secs(1) - elapsed);
    }
    *last = Instant::now();
}

/// Searches MusicBrainz for releases matching `artist`/`album` by text query.
/// There's no disc-ID to key off for folder imports, so this is a best-effort
/// text match -- the caller must show candidates for manual disambiguation,
/// never auto-apply the top hit.
pub fn search_release(artist: &str, album: &str) -> Result<Vec<MbCandidate>, String> {
    let query = format!(
        "release:\"{}\" AND artist:\"{}\"",
        album.replace('"', ""),
        artist.replace('"', "")
    );

    rate_limit();
    let client = reqwest::blocking::Client::new();
    let response = client
        .get(SEARCH_URL)
        .query(&[("query", query.as_str()), ("fmt", "json"), ("limit", "10")])
        .header("User-Agent", USER_AGENT)
        .send()
        .map_err(|e| format!("MusicBrainz request failed: {e}"))?;

    if !response.status().is_success() {
        return Err(format!("MusicBrainz returned status {}", response.status()));
    }

    let parsed: SearchResponse = response
        .json()
        .map_err(|e| format!("could not parse MusicBrainz response: {e}"))?;

    Ok(parsed
        .releases
        .into_iter()
        .map(|r| MbCandidate {
            release_id: r.id,
            title: r.title,
            artist: r
                .artist_credit
                .into_iter()
                .map(|a| a.name)
                .collect::<Vec<_>>()
                .join(", "),
            date: r.date,
            country: r.country,
            disambiguation: r.disambiguation,
        })
        .collect())
}
