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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: i64,
    #[serde(rename = "type")]
    pub job_type: String,
    pub track_id: Option<i64>,
    pub album_id: Option<i64>,
    pub status: String,
    pub progress: f64,
    pub error: Option<String>,
    pub created_at: String,
    pub completed_at: Option<String>,
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

/// A CD track queued for ripping: DB row is inserted before the audio file
/// exists, so `source_path` is the WAV path it *will* be written to.
#[derive(Debug, Clone)]
pub struct PlannedTrack {
    pub track_number: i64,
    pub title: String,
    pub artist: Option<String>,
    pub duration_ms: i64,
    pub source_path: String,
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
