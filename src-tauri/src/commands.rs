use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager, State};

use crate::convert::ffmpeg::ConvertOptions;
use crate::convert::job_queue::{self, JobQueue};
use crate::db::models::{Album, Job, PlannedTrack, Settings, Track};
use crate::db::{queries, DbState};
use crate::import::cd_detect::{self, CdInfo};
use crate::import::disc_id;
use crate::import::folder_scan;
use crate::metadata::cover_art;
use crate::metadata::ipod_fix::{self, FixReport};
use crate::metadata::musicbrainz::{self, MbCandidate};
use crate::metadata::tags::{self, TagPatch};

/// Reads Settings out of the `settings` key/value table, falling back to
/// hardcoded defaults for any key never written (fresh install, or a key
/// added in a later version). Never fails -- a missing/corrupt setting just
/// means "use the default" rather than blocking the whole app.
pub(crate) fn load_settings(conn: &rusqlite::Connection) -> Settings {
    let get = |key: &str, default: &str| {
        queries::get_setting(conn, key)
            .ok()
            .flatten()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| default.to_string())
    };
    let default_concurrency = job_queue::default_concurrency();
    Settings {
        output_dir: get("output_dir", ""),
        default_format: get("default_format", "mp3"),
        default_quality: get("default_quality", "0"),
        mb_user_agent: get("mb_user_agent", musicbrainz::DEFAULT_USER_AGENT),
        concurrency: get("concurrency", &default_concurrency.to_string())
            .parse()
            .unwrap_or(default_concurrency as i64),
    }
}

#[tauri::command]
pub fn get_settings(state: State<DbState>) -> Result<Settings, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    Ok(load_settings(&conn))
}

/// Persists every field of `patch`. The frontend always sends the full
/// Settings object (loaded via `get_settings` first), so there's no partial
/// update case to reconcile.
#[tauri::command]
pub fn update_settings(state: State<DbState>, patch: Settings) -> Result<Settings, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    queries::set_setting(&conn, "output_dir", &patch.output_dir).map_err(|e| e.to_string())?;
    queries::set_setting(&conn, "default_format", &patch.default_format).map_err(|e| e.to_string())?;
    queries::set_setting(&conn, "default_quality", &patch.default_quality).map_err(|e| e.to_string())?;
    queries::set_setting(&conn, "mb_user_agent", &patch.mb_user_agent).map_err(|e| e.to_string())?;
    queries::set_setting(&conn, "concurrency", &patch.concurrency.max(1).to_string()).map_err(|e| e.to_string())?;
    Ok(load_settings(&conn))
}

fn require_mb_user_agent(conn: &rusqlite::Connection) -> Result<String, String> {
    let settings = load_settings(conn);
    if settings.mb_user_agent.trim().is_empty() {
        return Err("Set a MusicBrainz User-Agent in Settings before searching MusicBrainz.".to_string());
    }
    Ok(settings.mb_user_agent)
}

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

/// Polls for an inserted audio CD and reads its TOC. Returns `None` when no
/// audio CD is present -- not an error, just "nothing to import yet".
#[tauri::command]
pub fn detect_cd() -> Option<CdInfo> {
    let device = cd_detect::detect_cd()?;
    let (mb_disc_id, track_count) = match disc_id::read_disc(&device) {
        Ok(info) => (Some(info.disc_id), info.tracks.len() as i32),
        Err(_) => (None, 0),
    };
    Some(CdInfo {
        device,
        disc_id: mb_disc_id,
        track_count,
    })
}

/// Disc-ID-based MusicBrainz lookup for a detected CD -- MB's most reliable
/// match, when the exact pressing is catalogued. Always returns a list
/// (possibly empty) for manual disambiguation, same contract as
/// `lookup_musicbrainz`.
#[tauri::command]
pub fn lookup_cd_release(state: State<DbState>, disc_id: String) -> Result<Vec<MbCandidate>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let user_agent = require_mb_user_agent(&conn)?;
    musicbrainz::lookup_by_discid(&user_agent, &disc_id)
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CdImportResult {
    pub album_id: i64,
    pub job_ids: Vec<i64>,
}

/// Creates the album + track rows for a CD import and kicks off one rip job
/// per track on the shared `JobQueue`. If `release_id` is given (from
/// `lookup_cd_release` or the existing text-search `lookup_musicbrainz`),
/// track titles/artist and album title/artist are pulled from that release
/// instead of being left as "Track N" placeholders.
#[tauri::command]
pub fn rip_and_import_cd(
    app: AppHandle,
    state: State<DbState>,
    job_queue: State<JobQueue>,
    device: String,
    release_id: Option<String>,
) -> Result<CdImportResult, String> {
    let disc = disc_id::read_disc(&device)?;
    let user_agent = {
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        load_settings(&conn).mb_user_agent
    };
    let release_detail = release_id
        .as_ref()
        .and_then(|id| musicbrainz::fetch_release_detail(&user_agent, id).ok());

    let album_title = release_detail
        .as_ref()
        .map(|r| r.title.clone())
        .unwrap_or_else(|| "Unknown Album (CD import)".to_string());
    let album_artist = release_detail.as_ref().map(|r| r.artist.clone());

    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let album_id = queries::insert_cd_album(&conn, &album_title, album_artist.as_deref(), &device, release_id.as_deref())
        .map_err(|e| e.to_string())?;
    let rip_dir = cd_rip_dir(&app, album_id)?;

    let mut job_ids = Vec::new();
    for disc_track in &disc.tracks {
        let release_track = release_detail
            .as_ref()
            .and_then(|r| r.tracks.iter().find(|t| t.number == i64::from(disc_track.number)));

        let planned = PlannedTrack {
            track_number: i64::from(disc_track.number),
            title: release_track
                .map(|t| t.title.clone())
                .unwrap_or_else(|| format!("Track {}", disc_track.number)),
            artist: release_track.and_then(|t| t.artist.clone()),
            duration_ms: disc_track.duration_ms,
            source_path: rip_dir
                .join(format!("{:02}.wav", disc_track.number))
                .to_string_lossy()
                .to_string(),
        };
        let track_id = queries::insert_planned_track(&conn, album_id, &planned).map_err(|e| e.to_string())?;
        let track = queries::get_track(&conn, track_id).map_err(|e| e.to_string())?;
        let job_id = queries::insert_job(&conn, "rip", Some(track_id), Some(album_id)).map_err(|e| e.to_string())?;
        job_ids.push(job_id);

        let output_path = PathBuf::from(&track.source_path);
        job_queue.spawn_rip(
            app.clone(),
            job_id,
            track,
            album_title.clone(),
            album_artist.clone(),
            device.clone(),
            disc_track.number,
            disc_track.sectors,
            output_path,
        );
    }

    Ok(CdImportResult { album_id, job_ids })
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
    let user_agent = require_mb_user_agent(&conn)?;
    let album = queries::get_album(&conn, album_id).map_err(|e| e.to_string())?;
    let artist = album.album_artist.clone().unwrap_or_default();
    musicbrainz::search_release(&user_agent, &artist, &album.title)
}

/// Records the chosen MusicBrainz release on the album, then best-effort
/// fetches and embeds its front cover from the Cover Art Archive. A release
/// with no cover art is a normal outcome, not a failure of the match itself.
#[tauri::command]
pub fn apply_musicbrainz_match(app: AppHandle, state: State<DbState>, album_id: i64, release_id: String) -> Result<Album, String> {
    let user_agent = {
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        queries::set_album_musicbrainz_release(&conn, album_id, &release_id).map_err(|e| e.to_string())?;
        load_settings(&conn).mb_user_agent
    };

    if let Ok((bytes, mime)) = cover_art::fetch_front_cover(&user_agent, &release_id) {
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
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub fn enqueue_conversion(
    app: AppHandle,
    state: State<DbState>,
    job_queue: State<JobQueue>,
    track_ids: Vec<i64>,
    format: String,
    quality: String,
    output_dir: String,
    output_folder_name: Option<String>,
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
            output_folder_name: output_folder_name.clone(),
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

/// Deletes done/error/cancelled jobs from the `jobs` table so the Jobs
/// drawer's "Clear" button can empty out finished history. Queued/running
/// jobs are left untouched -- cancel them first if they need to go too.
#[tauri::command]
pub fn clear_finished_jobs(state: State<DbState>) -> Result<Vec<Job>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    queries::clear_finished_jobs(&conn).map_err(|e| e.to_string())?;
    queries::list_jobs(&conn, None).map_err(|e| e.to_string())
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

fn cd_rip_dir(app: &AppHandle, album_id: i64) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("cd_rips")
        .join(album_id.to_string());
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
