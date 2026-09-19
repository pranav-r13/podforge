use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// MusicBrainz ToS requires a descriptive User-Agent identifying the app.
/// TODO(Phase 6): move to Settings (`mb_user_agent`) once the settings
/// screen exists, so users can put their own contact info in it.
const USER_AGENT: &str = "Podforge/0.1.0 ( https://github.com/pranav-r13/podforge )";

const SEARCH_URL: &str = "https://musicbrainz.org/ws/2/release/";
const DISCID_URL: &str = "https://musicbrainz.org/ws/2/discid/";
const RELEASE_URL: &str = "https://musicbrainz.org/ws/2/release/";

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

    Ok(parsed.releases.into_iter().map(release_json_to_candidate).collect())
}

fn join_artist_credit(credits: Vec<ArtistCreditJson>) -> String {
    credits.into_iter().map(|a| a.name).collect::<Vec<_>>().join(", ")
}

fn release_json_to_candidate(r: ReleaseJson) -> MbCandidate {
    MbCandidate {
        release_id: r.id,
        title: r.title,
        artist: join_artist_credit(r.artist_credit),
        date: r.date,
        country: r.country,
        disambiguation: r.disambiguation,
    }
}

#[derive(Debug, Deserialize)]
struct DiscIdLookupResponse {
    #[serde(default)]
    releases: Vec<ReleaseJson>,
}

/// Looks up releases whose table of contents matches `disc_id` exactly --
/// MusicBrainz's most reliable match, when it exists. A disc pressed
/// slightly differently than what's catalogued (common for reissues and
/// promos) returns no releases here, which is a normal outcome, not an
/// error -- the caller falls back to `search_release` for manual
/// disambiguation.
pub fn lookup_by_discid(disc_id: &str) -> Result<Vec<MbCandidate>, String> {
    rate_limit();
    let client = reqwest::blocking::Client::new();
    let response = client
        .get(format!("{DISCID_URL}{disc_id}"))
        .query(&[("fmt", "json"), ("inc", "artist-credits")])
        .header("User-Agent", USER_AGENT)
        .send()
        .map_err(|e| format!("MusicBrainz request failed: {e}"))?;

    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(Vec::new());
    }
    if !response.status().is_success() {
        return Err(format!("MusicBrainz returned status {}", response.status()));
    }

    let parsed: DiscIdLookupResponse = response
        .json()
        .map_err(|e| format!("could not parse MusicBrainz response: {e}"))?;

    Ok(parsed.releases.into_iter().map(release_json_to_candidate).collect())
}

/// One track's title/artist as listed on a specific MusicBrainz release.
#[derive(Debug, Clone, Serialize)]
pub struct MbReleaseTrack {
    pub number: i64,
    pub title: String,
    pub artist: Option<String>,
}

pub struct ReleaseDetail {
    pub title: String,
    pub artist: String,
    pub tracks: Vec<MbReleaseTrack>,
}

#[derive(Debug, Deserialize)]
struct ReleaseDetailJson {
    title: String,
    #[serde(rename = "artist-credit", default)]
    artist_credit: Vec<ArtistCreditJson>,
    #[serde(default)]
    media: Vec<MediumJson>,
}

#[derive(Debug, Deserialize)]
struct MediumJson {
    #[serde(default)]
    tracks: Vec<TrackJson>,
}

#[derive(Debug, Deserialize)]
struct TrackJson {
    position: i64,
    title: String,
    #[serde(rename = "artist-credit", default)]
    artist_credit: Vec<ArtistCreditJson>,
}

/// Fetches the full track listing plus title/artist for a specific release,
/// used once a disc-ID or text-search match has been chosen so ripped CD
/// tracks can be tagged immediately instead of left as "Track N"
/// placeholders. Only the first medium is used -- multi-disc box sets
/// aren't disambiguated by disc number yet.
pub fn fetch_release_detail(release_id: &str) -> Result<ReleaseDetail, String> {
    rate_limit();
    let client = reqwest::blocking::Client::new();
    let response = client
        .get(format!("{RELEASE_URL}{release_id}"))
        .query(&[("fmt", "json"), ("inc", "recordings+artist-credits")])
        .header("User-Agent", USER_AGENT)
        .send()
        .map_err(|e| format!("MusicBrainz request failed: {e}"))?;

    if !response.status().is_success() {
        return Err(format!("MusicBrainz returned status {}", response.status()));
    }

    let parsed: ReleaseDetailJson = response
        .json()
        .map_err(|e| format!("could not parse MusicBrainz response: {e}"))?;

    let tracks = parsed
        .media
        .into_iter()
        .next()
        .map(|m| m.tracks)
        .unwrap_or_default()
        .into_iter()
        .map(|t| MbReleaseTrack {
            number: t.position,
            title: t.title,
            artist: if t.artist_credit.is_empty() {
                None
            } else {
                Some(join_artist_credit(t.artist_credit))
            },
        })
        .collect();

    Ok(ReleaseDetail {
        title: parsed.title,
        artist: join_artist_credit(parsed.artist_credit),
        tracks,
    })
}
