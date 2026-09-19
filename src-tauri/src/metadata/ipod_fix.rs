use std::path::{Path, PathBuf};
use std::process::Command;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::db::queries;
use crate::metadata::tags::{self, TagPatch};

#[derive(Debug, thiserror::Error)]
pub enum IpodFixError {
    #[error(transparent)]
    Db(#[from] rusqlite::Error),
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct FixReport {
    pub album_id: i64,
    pub tags_normalized: usize,
    pub tracks_reencoded: Vec<String>,
    pub errors: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct FfprobeOutput {
    #[serde(default)]
    streams: Vec<FfprobeStream>,
}

#[derive(Debug, Deserialize)]
struct FfprobeStream {
    codec_type: Option<String>,
    codec_name: Option<String>,
}

/// Codecs a given container extension is actually allowed to hold. A file
/// whose real codec (per ffprobe) isn't in this list is mislabeled -- the
/// classic case being FLAC audio saved with a `.mp3` extension, which
/// iPods will refuse to play despite the file "looking" fine.
fn expected_codecs(extension: &str) -> &'static [&'static str] {
    match extension {
        "mp3" => &["mp3"],
        "m4a" | "mp4" => &["alac", "aac"],
        "flac" => &["flac"],
        "wav" | "aiff" | "aif" => &[
            "pcm_s16le", "pcm_s24le", "pcm_s32le", "pcm_s16be", "pcm_s24be",
        ],
        "ogg" => &["vorbis"],
        _ => &[],
    }
}

fn probe_codec(path: &Path) -> Result<String, String> {
    let output = Command::new("ffprobe")
        .args(["-v", "quiet", "-print_format", "json", "-show_streams"])
        .arg(path)
        .output()
        .map_err(|e| format!("ffprobe failed to launch: {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "ffprobe exited with error: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let parsed: FfprobeOutput = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("could not parse ffprobe output: {e}"))?;

    parsed
        .streams
        .into_iter()
        .find(|s| s.codec_type.as_deref() == Some("audio"))
        .and_then(|s| s.codec_name)
        .ok_or_else(|| "no audio stream found".to_string())
}

fn re_encode_to_match_extension(path: &Path, extension: &str) -> Result<(), String> {
    let mut tmp_name = path.as_os_str().to_os_string();
    tmp_name.push(".ipodfix.tmp");
    let tmp_path = PathBuf::from(tmp_name);

    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-y", "-i"]).arg(path);
    match extension {
        "mp3" => {
            cmd.args(["-c:a", "libmp3lame", "-q:a", "2"]);
        }
        "m4a" | "mp4" => {
            cmd.args(["-c:a", "alac"]);
        }
        "flac" => {
            cmd.args(["-c:a", "flac"]);
        }
        _ => {
            cmd.args(["-c:a", "copy"]);
        }
    }
    cmd.arg(&tmp_path);

    let output = cmd
        .output()
        .map_err(|e| format!("ffmpeg failed to launch: {e}"))?;

    if !output.status.success() {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(format!(
            "ffmpeg re-encode failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    std::fs::rename(&tmp_path, path).map_err(|e| format!("could not replace original file: {e}"))
}

/// Runs the manual "make it iPod-safe" workflow against every track in an
/// album: force album/album_artist tags uniform across all tracks, then
/// verify each file's real audio codec matches what its extension promises,
/// re-encoding in place when it doesn't.
pub fn apply_ipod_compat_fix(conn: &mut Connection, album_id: i64) -> Result<FixReport, IpodFixError> {
    let album = queries::get_album(conn, album_id)?;
    let mut report = FixReport {
        album_id,
        ..Default::default()
    };

    for track in &album.tracks {
        let path = Path::new(&track.source_path);

        let normalize = TagPatch {
            album: Some(album.title.clone()),
            album_artist: album.album_artist.clone(),
            ..Default::default()
        };
        match tags::write_tags(path, &normalize) {
            Ok(()) => report.tags_normalized += 1,
            Err(e) => {
                report.errors.push(format!("{}: {e}", track.title));
                continue;
            }
        }

        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        let expected = expected_codecs(&extension);
        if expected.is_empty() {
            continue; // unrecognized container, nothing to validate against
        }

        let actual_codec = match probe_codec(path) {
            Ok(codec) => codec,
            Err(e) => {
                report.errors.push(format!("{}: {e}", track.title));
                continue;
            }
        };

        if !expected.contains(&actual_codec.as_str()) {
            if let Err(e) = re_encode_to_match_extension(path, &extension) {
                report.errors.push(format!("{}: {e}", track.title));
                continue;
            }
            report.tracks_reencoded.push(track.title.clone());
        }

        if let Ok(codec) = probe_codec(path) {
            let _ = queries::update_track_codec(conn, track.id, &codec, &extension);
        }
    }

    Ok(report)
}
