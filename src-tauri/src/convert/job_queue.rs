use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::Semaphore;

use crate::convert::ffmpeg::{self, ConvertOptions};
use crate::db::models::{Album, Track};
use crate::db::{queries, DbState};
use crate::import::cd_rip;
use crate::metadata::tags::{self, TagPatch};

/// Bounded worker pool for conversion jobs. Concurrency is capped so a bulk
/// convert doesn't starve the UI or ripping -- see plan's `num_cpus / 2`
/// guidance for CPU-heavy ffmpeg jobs.
pub struct JobQueue {
    semaphore: Arc<Semaphore>,
    running_pids: Arc<Mutex<HashMap<i64, u32>>>,
}

#[derive(Clone, Serialize)]
struct JobProgressEvent {
    job_id: i64,
    track_id: Option<i64>,
    status: String,
    progress: f64,
    error: Option<String>,
}

/// `num_cpus / 2`, rounded down and floored at 1 -- see plan's concurrency
/// guidance for CPU-heavy ffmpeg jobs. Used as the Settings default and
/// whenever the `concurrency` setting is missing or invalid.
pub fn default_concurrency() -> usize {
    let cpus = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    (cpus / 2).max(1)
}

impl JobQueue {
    /// `concurrency` is read once at startup from Settings (falling back to
    /// `default_concurrency()`); changing it later takes effect on next
    /// launch, since a `tokio::sync::Semaphore` can't shrink its permit
    /// count once tasks may already hold one.
    pub fn new(concurrency: usize) -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(concurrency.max(1))),
            running_pids: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Enqueues a conversion job. Returns immediately; the job runs on a
    /// background tokio task once a semaphore permit is free, persisting
    /// progress to `jobs` and emitting `job-progress` events as it goes.
    pub fn spawn_conversion(&self, app: AppHandle, job_id: i64, track: Track, album: Album, options: ConvertOptions) {
        let semaphore = self.semaphore.clone();
        let running_pids = self.running_pids.clone();

        tokio::spawn(async move {
            let _permit = semaphore.acquire().await.expect("semaphore closed");
            run_job(app, job_id, track, album, options, running_pids).await;
        });
    }

    /// Kills the ffmpeg process backing a running job, if any, and marks
    /// the job cancelled. A job that hasn't started yet (still queued
    /// behind the semaphore) has no pid to kill -- `cancel_job` in
    /// commands.rs still flips its DB status so it's dropped before running.
    pub fn cancel(&self, job_id: i64) {
        let pid = self.running_pids.lock().unwrap().get(&job_id).copied();
        if let Some(pid) = pid {
            let _ = std::process::Command::new("kill").arg("-9").arg(pid.to_string()).output();
        }
    }

    /// Enqueues a CD track rip. Shares the same semaphore/pid-map as
    /// conversion jobs -- ripping and converting are both I/O/CPU-bound
    /// background work, so one bounded pool keeps total concurrency (and
    /// `cancel`) consistent across both job types.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_rip(
        &self,
        app: AppHandle,
        job_id: i64,
        track: Track,
        album_title: String,
        album_artist: Option<String>,
        device: String,
        track_number: i32,
        total_sectors: i64,
        output_path: PathBuf,
    ) {
        let semaphore = self.semaphore.clone();
        let running_pids = self.running_pids.clone();

        tokio::spawn(async move {
            let _permit = semaphore.acquire().await.expect("semaphore closed");
            run_rip_job(
                app,
                job_id,
                track,
                album_title,
                album_artist,
                device,
                track_number,
                total_sectors,
                output_path,
                running_pids,
            )
            .await;
        });
    }
}

async fn run_job(
    app: AppHandle,
    job_id: i64,
    track: Track,
    album: Album,
    options: ConvertOptions,
    running_pids: Arc<Mutex<HashMap<i64, u32>>>,
) {
    tracing::info!(job_id, track_id = track.id, format = %options.format, "conversion job started");
    update_job(&app, job_id, "running", 0.0, None);
    emit(&app, job_id, Some(track.id), "running", 0.0, None);

    let output_path = match ffmpeg::build_output_path(&album, &track, &options) {
        Ok(p) => p,
        Err(e) => {
            finish_error(&app, job_id, Some(track.id), &e);
            return;
        }
    };

    let source_path = std::path::Path::new(&track.source_path).to_path_buf();
    let duration_ms = track.duration_ms;
    let progress_app = app.clone();
    let pid_map = running_pids.clone();

    let result = ffmpeg::convert_with_progress(
        &source_path,
        &output_path,
        &options,
        duration_ms,
        move |fraction| {
            update_job(&progress_app, job_id, "running", fraction, None);
            emit(&progress_app, job_id, Some(track.id), "running", fraction, None);
        },
        move |pid| {
            pid_map.lock().unwrap().insert(job_id, pid);
        },
    )
    .await;

    running_pids.lock().unwrap().remove(&job_id);

    match result {
        Ok(()) => {
            let output_str = output_path.to_string_lossy().to_string();
            if let Ok(conn) = app.state::<DbState>().0.lock() {
                let _ = queries::set_track_output_path(&conn, track.id, &output_str);
            }
            tracing::info!(job_id, track_id = track.id, "conversion job done");
            update_job(&app, job_id, "done", 1.0, None);
            emit(&app, job_id, Some(track.id), "done", 1.0, None);
        }
        Err(e) if e == "cancelled" => {
            update_job(&app, job_id, "cancelled", 0.0, None);
            emit(&app, job_id, Some(track.id), "cancelled", 0.0, None);
        }
        Err(e) => finish_error(&app, job_id, Some(track.id), &e),
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_rip_job(
    app: AppHandle,
    job_id: i64,
    track: Track,
    album_title: String,
    album_artist: Option<String>,
    device: String,
    track_number: i32,
    total_sectors: i64,
    output_path: PathBuf,
    running_pids: Arc<Mutex<HashMap<i64, u32>>>,
) {
    tracing::info!(job_id, track_id = track.id, device = %device, "rip job started");
    update_job(&app, job_id, "running", 0.0, None);
    emit(&app, job_id, Some(track.id), "running", 0.0, None);

    let progress_app = app.clone();
    let pid_map = running_pids.clone();
    let track_id = track.id;

    let result = cd_rip::rip_track_with_progress(
        &device,
        track_number,
        total_sectors,
        &output_path,
        move |fraction| {
            update_job(&progress_app, job_id, "running", fraction, None);
            emit(&progress_app, job_id, Some(track_id), "running", fraction, None);
        },
        move |pid| {
            pid_map.lock().unwrap().insert(job_id, pid);
        },
    )
    .await;

    running_pids.lock().unwrap().remove(&job_id);

    match result {
        Ok(()) => {
            let patch = TagPatch {
                title: Some(track.title.clone()),
                artist: track.artist.clone(),
                album: Some(album_title),
                album_artist,
                track_number: track.track_number,
                disc_number: track.disc_number,
                ..Default::default()
            };
            if let Err(e) = tags::write_tags(&output_path, &patch) {
                finish_error(&app, job_id, Some(track_id), &e.to_string());
                return;
            }
            if let Ok(conn) = app.state::<DbState>().0.lock() {
                let _ = queries::set_track_ripped(&conn, track_id);
            }
            tracing::info!(job_id, track_id, "rip job done");
            update_job(&app, job_id, "done", 1.0, None);
            emit(&app, job_id, Some(track_id), "done", 1.0, None);
        }
        Err(e) if e == "cancelled" => {
            update_job(&app, job_id, "cancelled", 0.0, None);
            emit(&app, job_id, Some(track_id), "cancelled", 0.0, None);
        }
        Err(e) => finish_error(&app, job_id, Some(track_id), &e),
    }
}

fn finish_error(app: &AppHandle, job_id: i64, track_id: Option<i64>, error: &str) {
    tracing::error!(job_id, track_id, error, "job failed");
    update_job(app, job_id, "error", 0.0, Some(error));
    emit(app, job_id, track_id, "error", 0.0, Some(error.to_string()));
}

fn update_job(app: &AppHandle, job_id: i64, status: &str, progress: f64, error: Option<&str>) {
    if let Ok(conn) = app.state::<DbState>().0.lock() {
        let _ = queries::update_job_progress(&conn, job_id, status, progress, error);
    }
}

fn emit(app: &AppHandle, job_id: i64, track_id: Option<i64>, status: &str, progress: f64, error: Option<String>) {
    let _ = app.emit(
        "job-progress",
        JobProgressEvent {
            job_id,
            track_id,
            status: status.to_string(),
            progress,
            error,
        },
    );
}
