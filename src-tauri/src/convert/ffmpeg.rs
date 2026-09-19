use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

use crate::db::models::{Album, Track};

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ConvertOptions {
    pub format: String, // "mp3" | "alac" | "flac" | "aac"
    pub quality: String, // libmp3lame -q:a value, flac -compression_level, or aac -b:a bitrate (e.g. "256k")
    pub output_dir: String,
}

/// Container extension for a target format. ALAC and AAC both need `.m4a`,
/// not their codec name, or the iPod refuses to recognize the file.
fn extension_for_format(format: &str) -> Result<&'static str, String> {
    match format {
        "mp3" => Ok("mp3"),
        "alac" => Ok("m4a"),
        "aac" => Ok("m4a"),
        "flac" => Ok("flac"),
        other => Err(format!("unsupported target format: {other}")),
    }
}

/// Strips characters that are awkward or illegal in macOS/iPod filesystem
/// paths (`/` above all -- artist/album/title text routinely contains it,
/// e.g. "AC/DC" or "Rock/Pop").
fn sanitize_path_component(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '/' | '\\' | ':' => '-',
            c => c,
        })
        .collect::<String>()
        .trim()
        .to_string()
}

/// Builds `{output_dir}/{Artist}/{Album}/{NN} {Title}.ext`, matching the
/// layout the plan calls out as the default Convert dialog output path.
pub fn build_output_path(album: &Album, track: &Track, options: &ConvertOptions) -> Result<PathBuf, String> {
    let ext = extension_for_format(&options.format)?;
    let artist = sanitize_path_component(
        track.artist.as_deref().or(album.album_artist.as_deref()).unwrap_or("Unknown Artist"),
    );
    let album_title = sanitize_path_component(&album.title);
    let track_number = track
        .track_number
        .map(|n| format!("{n:02}"))
        .unwrap_or_else(|| "00".to_string());
    let title = sanitize_path_component(&track.title);

    Ok(Path::new(&options.output_dir)
        .join(artist)
        .join(album_title)
        .join(format!("{track_number} {title}.{ext}")))
}

fn codec_args(options: &ConvertOptions) -> Result<Vec<String>, String> {
    let args = match options.format.as_str() {
        "mp3" => vec!["-c:a".into(), "libmp3lame".into(), "-q:a".into(), options.quality.clone()],
        "flac" => vec!["-c:a".into(), "flac".into(), "-compression_level".into(), options.quality.clone()],
        "alac" => vec!["-c:a".into(), "alac".into(), "-f".into(), "ipod".into()],
        "aac" => vec!["-c:a".into(), "aac".into(), "-b:a".into(), options.quality.clone(), "-f".into(), "ipod".into()],
        other => return Err(format!("unsupported target format: {other}")),
    };
    Ok(args)
}

/// Spawns `ffmpeg` with `-progress pipe:1` and maps each `out_time_us=`
/// line to a fraction of `total_duration_ms`, invoking `on_progress` as
/// lines arrive. Returns once ffmpeg exits; a non-zero exit is an `Err`
/// carrying its stderr tail.
pub async fn convert_with_progress<F, S>(
    source_path: &Path,
    output_path: &Path,
    options: &ConvertOptions,
    total_duration_ms: Option<i64>,
    mut on_progress: F,
    on_started: S,
) -> Result<(), String>
where
    F: FnMut(f64) + Send,
    S: FnOnce(u32),
{
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("could not create output directory: {e}"))?;
    }

    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-y", "-i"])
        .arg(source_path)
        .args(["-map", "0:a", "-map", "0:v?"])
        .args(codec_args(options)?)
        .args(["-c:v", "copy", "-disposition:v", "attached_pic"])
        .args(["-progress", "pipe:1", "-nostats", "-loglevel", "error"])
        .arg(output_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child: Child = cmd.spawn().map_err(|e| format!("ffmpeg failed to launch: {e}"))?;
    if let Some(pid) = child.id() {
        on_started(pid);
    }

    let stdout = child.stdout.take().expect("stdout was piped");
    let mut lines = BufReader::new(stdout).lines();
    let progress_task = async {
        while let Ok(Some(line)) = lines.next_line().await {
            if let Some(us) = line.strip_prefix("out_time_us=").and_then(|v| v.parse::<i64>().ok()) {
                if let Some(total_ms) = total_duration_ms {
                    if total_ms > 0 {
                        let fraction = (us as f64 / 1000.0) / total_ms as f64;
                        on_progress(fraction.clamp(0.0, 1.0));
                    }
                }
            }
        }
    };

    let stderr_task = async {
        let stderr = child.stderr.take().expect("stderr was piped");
        let mut lines = BufReader::new(stderr).lines();
        let mut collected = String::new();
        while let Ok(Some(line)) = lines.next_line().await {
            collected.push_str(&line);
            collected.push('\n');
        }
        collected
    };

    let (_, stderr_output) = tokio::join!(progress_task, stderr_task);

    let status = child.wait().await.map_err(|e| format!("ffmpeg process error: {e}"))?;
    if !status.success() {
        let _ = std::fs::remove_file(output_path);
        if status.code().is_none() {
            return Err("cancelled".to_string());
        }
        return Err(format!("ffmpeg exited with error: {stderr_output}"));
    }

    Ok(())
}
