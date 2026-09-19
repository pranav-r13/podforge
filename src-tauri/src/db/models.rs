use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub id: i64,
    pub album_id: i64,
    pub disc_number: Option<i64>,
    pub track_number: Option<i64>,
    pub title: String,
    pub artist: Option<String>,
    pub duration_ms: Option<i64>,
    pub source_path: String,
    pub output_path: Option<String>,
    pub status: String,
    pub musicbrainz_recording_id: Option<String>,
    pub codec: Option<String>,
    pub container: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Album {
    pub id: i64,
    pub title: String,
    pub album_artist: Option<String>,
    pub year: Option<i64>,
    pub genre: Option<String>,
    pub musicbrainz_release_id: Option<String>,
    pub cover_art_path: Option<String>,
    pub source_type: String,
    pub source_path: Option<String>,
    pub created_at: String,
    pub tracks: Vec<Track>,
}

/// In-memory result of scanning a folder, before it's inserted into the DB.
#[derive(Debug, Clone)]
pub struct ScannedAlbum {
    pub title: String,
    pub album_artist: Option<String>,
    pub year: Option<i64>,
    pub genre: Option<String>,
    pub source_path: String,
    pub tracks: Vec<ScannedTrack>,
}

#[derive(Debug, Clone)]
pub struct ScannedTrack {
    pub disc_number: Option<i64>,
    pub track_number: Option<i64>,
    pub title: String,
    pub artist: Option<String>,
    pub duration_ms: Option<i64>,
    pub source_path: String,
    pub codec: Option<String>,
    pub container: Option<String>,
}
