/// Fetches the front cover image for a MusicBrainz release from the Cover
/// Art Archive. Returns `(bytes, mime_type)`. Many releases have no art at
/// all (404) -- that's an expected, non-fatal outcome the caller should
/// treat as "no cover available", not an error to surface loudly.
pub fn fetch_front_cover(user_agent: &str, release_id: &str) -> Result<(Vec<u8>, String), String> {
    let url = format!("https://coverartarchive.org/release/{release_id}/front");

    let client = reqwest::blocking::Client::new();
    let response = client
        .get(&url)
        .header("User-Agent", user_agent)
        .send()
        .map_err(|e| format!("Cover Art Archive request failed: {e}"))?;

    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err("no cover art available for this release".to_string());
    }
    if !response.status().is_success() {
        return Err(format!("Cover Art Archive returned status {}", response.status()));
    }

    let mime = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("image/jpeg")
        .to_string();

    let bytes = response
        .bytes()
        .map_err(|e| format!("could not read cover art response: {e}"))?
        .to_vec();

    Ok((bytes, mime))
}
