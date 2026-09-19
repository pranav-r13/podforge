use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use lofty::file::{AudioFile, TaggedFileExt};
use lofty::probe::Probe;
use lofty::tag::{ItemKey, Tag};
use walkdir::WalkDir;

use crate::db::models::{ScannedAlbum, ScannedTrack};

const AUDIO_EXTENSIONS: &[&str] = &["flac", "mp3", "m4a", "mp4", "alac", "wav", "aiff", "aif", "ogg"];

fn is_audio_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| AUDIO_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// macOS writes AppleDouble sidecar files (e.g. `._track.flac`) alongside real
/// files on non-native filesystems; they aren't audio and must be skipped.
fn is_apple_double(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.starts_with("._"))
        .unwrap_or(false)
}

fn get_str(tag: Option<&Tag>, key: ItemKey) -> Option<String> {
    tag?.get_string(&key).map(|s| s.to_string())
}

fn get_num(tag: Option<&Tag>, key: ItemKey) -> Option<i64> {
    tag?.get_string(&key)
        .and_then(|s| s.split('/').next())
        .and_then(|s| s.trim().parse::<i64>().ok())
}

#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("path does not exist or is not a directory: {0}")]
    InvalidPath(String),
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
}

struct AlbumMeta {
    title: Option<String>,
    album_artist: Option<String>,
    year: Option<i64>,
    genre: Option<String>,
}

/// Walks `root` recursively, reads tags from every audio file found, and
/// groups tracks by their containing directory (one directory == one album).
pub fn scan_folder(root: &Path) -> Result<Vec<ScannedAlbum>, ScanError> {
    if !root.is_dir() {
        return Err(ScanError::InvalidPath(root.display().to_string()));
    }

    let mut tracks_by_dir: BTreeMap<PathBuf, Vec<ScannedTrack>> = BTreeMap::new();
    let mut meta_by_dir: BTreeMap<PathBuf, AlbumMeta> = BTreeMap::new();

    for entry in WalkDir::new(root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let path = entry.path();
        if !is_audio_file(path) || is_apple_double(path) {
            continue;
        }

        let tagged_file = match Probe::open(path).and_then(|p| p.read()) {
            Ok(f) => f,
            Err(_) => continue, // corrupt/unreadable file, skip rather than fail the whole scan
        };
        let tag = tagged_file.primary_tag().or_else(|| tagged_file.first_tag());

        let fallback_title = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Unknown Title")
            .to_string();

        let track = ScannedTrack {
            disc_number: get_num(tag, ItemKey::DiscNumber),
            track_number: get_num(tag, ItemKey::TrackNumber),
            title: get_str(tag, ItemKey::TrackTitle).unwrap_or(fallback_title),
            artist: get_str(tag, ItemKey::TrackArtist),
            duration_ms: Some(tagged_file.properties().duration().as_millis() as i64),
            source_path: path.to_string_lossy().to_string(),
            codec: Some(format!("{:?}", tagged_file.file_type())),
            container: path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_lowercase()),
        };

        let dir = path.parent().unwrap_or(root).to_path_buf();
        meta_by_dir.entry(dir.clone()).or_insert_with(|| AlbumMeta {
            title: get_str(tag, ItemKey::AlbumTitle),
            album_artist: get_str(tag, ItemKey::AlbumArtist),
            year: get_num(tag, ItemKey::Year),
            genre: get_str(tag, ItemKey::Genre),
        });
        tracks_by_dir.entry(dir).or_default().push(track);
    }

    let mut albums: Vec<ScannedAlbum> = Vec::new();
    for (dir, mut tracks) in tracks_by_dir {
        tracks.sort_by(|a, b| {
            (a.disc_number, a.track_number, &a.title).cmp(&(b.disc_number, b.track_number, &b.title))
        });

        let meta = meta_by_dir.remove(&dir).unwrap_or(AlbumMeta {
            title: None,
            album_artist: None,
            year: None,
            genre: None,
        });

        let title = meta.title.unwrap_or_else(|| {
            dir.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("Unknown Album")
                .to_string()
        });

        albums.push(ScannedAlbum {
            title,
            album_artist: meta.album_artist,
            year: meta.year,
            genre: meta.genre,
            source_path: dir.to_string_lossy().to_string(),
            tracks,
        });
    }

    Ok(albums)
}
