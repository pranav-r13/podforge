use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager, State};

use crate::convert::ffmpeg::ConvertOptions;
use crate::convert::job_queue::JobQueue;
use crate::db::models::{Album, Job, Track};
use crate::db::{queries, DbState};
use crate::import::folder_scan;
use crate::metadata::cover_art;
use crate::metadata::ipod_fix::{self, FixReport};
use crate::metadata::musicbrainz::{self, MbCandidate};
use crate::metadata::tags::{self, TagPatch};

#[tauri::command]
pub fn scan_folder(state: State<DbState>, path: String) -> Result<Vec<Album>, String> {
    let scanned = folder_scan::scan_folder(Path::new(&path)).map_err(|e| e.to_string())?;

    let mut conn = state.0.lock().map_err(|e| e.to_string())?;
    let mut inserted_ids = Vec::new();

    for album in &scanned {
        if queries::album_exists_for_path(&conn, &album.source_path).map_err(|e| e.to_string())? {
            continue; // already imported, skip to avoid duplicates on rescan
        }
        let id = queries::insert_scanned_album(&mut conn, album).map_err(|e| e.to_string())?;
        inserted_ids.push(id);
    }

    inserted_ids
        .into_iter()
        .map(|id| queries::get_album(&conn, id).map_err(|e| e.to_string()))
        .collect()
}

#[tauri::command]
pub fn get_library(state: State<DbState>) -> Result<Vec<Album>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    queries::get_library(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_album(state: State<DbState>, album_id: i64) -> Result<Album, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    queries::get_album(&conn, album_id).map_err(|e| e.to_string())
}

/// Writes `patch` to a track's file tags and mirrors the track-level fields
/// (title/artist/track#/disc#) into the DB.
#[tauri::command]
pub fn update_track_tags(state: State<DbState>, track_id: i64, patch: TagPatch) -> Result<Track, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    apply_patch(&conn, track_id, &patch)?;
    queries::get_track(&conn, track_id).map_err(|e| e.to_string())
}

/// Applies the same shared-field patch (album/album_artist/year/genre, plus
/// any track-level fields given) to every track in `track_ids`.
#[tauri::command]
pub fn bulk_update_tags(state: State<DbState>, track_ids: Vec<i64>, patch: TagPatch) -> Result<Vec<Track>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    for track_id in &track_ids {
        apply_patch(&conn, *track_id, &patch)?;
    }
    track_ids
        .into_iter()
        .map(|id| queries::get_track(&conn, id).map_err(|e| e.to_string()))
        .collect()
}

fn apply_patch(conn: &rusqlite::Connection, track_id: i64, patch: &TagPatch) -> Result<(), String> {
    let track = queries::get_track(conn, track_id).map_err(|e| e.to_string())?;
    let path = Path::new(&track.source_path);
    tags::write_tags(path, patch).map_err(|e| e.to_string())?;
    queries::update_track_fields(conn, track_id, patch).map_err(|e| e.to_string())?;
    if patch.album.is_some() || patch.album_artist.is_some() || patch.year.is_some() || patch.genre.is_some() {
        queries::update_album_fields(conn, track.album_id, patch).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn apply_ipod_compat_fix(state: State<DbState>, album_id: i64) -> Result<FixReport, String> {
    let mut conn = state.0.lock().map_err(|e| e.to_string())?;
    ipod_fix::apply_ipod_compat_fix(&mut conn, album_id).map_err(|e| e.to_string())
}

/// Text-searches MusicBrainz for release candidates matching the album's
/// current artist/title tags. Always returns a list for manual
/// disambiguation -- never auto-picks, since text search on common
/// artist/album names routinely returns several plausible releases.
#[tauri::command]
pub fn lookup_musicbrainz(state: State<DbState>, album_id: i64) -> Result<Vec<MbCandidate>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let album = queries::get_album(&conn, album_id).map_err(|e| e.to_string())?;
    let artist = album.album_artist.clone().unwrap_or_default();
    musicbrainz::search_release(&artist, &album.title)
}

/// Records the chosen MusicBrainz release on the album, then best-effort
/// fetches and embeds its front cover from the Cover Art Archive. A release
/// with no cover art is a normal outcome, not a failure of the match itself.
#[tauri::command]
pub fn apply_musicbrainz_match(app: AppHandle, state: State<DbState>, album_id: i64, release_id: String) -> Result<Album, String> {
    {
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        queries::set_album_musicbrainz_release(&conn, album_id, &release_id).map_err(|e| e.to_string())?;
    }

    if let Ok((bytes, mime)) = cover_art::fetch_front_cover(&release_id) {
        apply_cover_bytes(&app, &state, album_id, bytes, &mime)?;
    }

    let conn = state.0.lock().map_err(|e| e.to_string())?;
    queries::get_album(&conn, album_id).map_err(|e| e.to_string())
}

/// Manual cover art fallback for when MusicBrainz has no match or no art:
/// embeds a user-picked local image file into every track of the album.
#[tauri::command]
pub fn set_cover_art(app: AppHandle, state: State<DbState>, album_id: i64, image_path: String) -> Result<Album, String> {
    let bytes = std::fs::read(&image_path).map_err(|e| e.to_string())?;
    let mime = mime_from_extension(Path::new(&image_path));
    apply_cover_bytes(&app, &state, album_id, bytes, mime)?;

    let conn = state.0.lock().map_err(|e| e.to_string())?;
    queries::get_album(&conn, album_id).map_err(|e| e.to_string())
}

fn apply_cover_bytes(app: &AppHandle, state: &State<DbState>, album_id: i64, bytes: Vec<u8>, mime: &str) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let album = queries::get_album(&conn, album_id).map_err(|e| e.to_string())?;

    let ext = if mime == "image/png" { "png" } else { "jpg" };
    let dir = cover_dir(app)?;
    let file_path = dir.join(format!("{album_id}.{ext}"));
    std::fs::write(&file_path, &bytes).map_err(|e| e.to_string())?;

    for track in &album.tracks {
        tags::embed_cover_art(Path::new(&track.source_path), bytes.clone(), mime)
            .map_err(|e| e.to_string())?;
    }

    queries::set_album_cover_art_path(&conn, album_id, &file_path.to_string_lossy())
        .map_err(|e| e.to_string())
}

/// Enqueues one conversion job per track. Each job runs on the shared
/// `JobQueue` worker pool once a permit is free; progress is persisted to
/// `jobs` and pushed to the frontend via `job-progress` events rather than
/// polled.
#[tauri::command]
pub fn enqueue_conversion(
    app: AppHandle,
    state: State<DbState>,
    job_queue: State<JobQueue>,
    track_ids: Vec<i64>,
    format: String,
    quality: String,
    output_dir: String,
) -> Result<Vec<i64>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let mut job_ids = Vec::new();

    for track_id in track_ids {
        let track = queries::get_track(&conn, track_id).map_err(|e| e.to_string())?;
        let album = queries::get_album(&conn, track.album_id).map_err(|e| e.to_string())?;
        let job_id = queries::insert_job(&conn, "convert", Some(track_id), Some(track.album_id))
            .map_err(|e| e.to_string())?;
        job_ids.push(job_id);

        let options = ConvertOptions {
            format: format.clone(),
            quality: quality.clone(),
            output_dir: output_dir.clone(),
        };
        job_queue.spawn_conversion(app.clone(), job_id, track, album, options);
    }

    Ok(job_ids)
}

#[tauri::command]
pub fn get_job_status(state: State<DbState>, job_id: i64) -> Result<Job, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    queries::get_job(&conn, job_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_jobs(state: State<DbState>, status: Option<String>) -> Result<Vec<Job>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    queries::list_jobs(&conn, status.as_deref()).map_err(|e| e.to_string())
}

/// Kills the job's ffmpeg process if it's already running, and marks it
/// cancelled in the DB either way (a job still queued behind the semaphore
/// has no process yet, so this is what stops it from ever starting).
#[tauri::command]
pub fn cancel_job(state: State<DbState>, job_queue: State<JobQueue>, job_id: i64) -> Result<(), String> {
    job_queue.cancel(job_id);
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    queries::update_job_progress(&conn, job_id, "cancelled", 0.0, None).map_err(|e| e.to_string())
}

fn cover_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("covers");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn mime_from_extension(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        _ => "image/jpeg",
    }
}
