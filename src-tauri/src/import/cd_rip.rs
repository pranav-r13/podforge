use std::path::Path;
use std::process::Stdio;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

/// `cd-paranoia -e` prints progress lines shaped like
/// `##: 2 [wrote] @ 5238` -- extracts the trailing sector offset so it can
/// be reported as a fraction of the track's total sector count.
fn parse_sector(line: &str) -> Option<i64> {
    let at_pos = line.rfind('@')?;
    line[at_pos + 1..].trim().split_whitespace().next()?.parse().ok()
}

/// Rips one track from `device` to `output_path` as a WAV file via
/// `cd-paranoia`, invoking `on_progress` with a 0..1 fraction as sector
/// offsets are parsed off stderr. Returns once cd-paranoia exits; a
/// non-zero exit (including a kill from `cancel_job`, which shows up as no
/// exit code) is an `Err`.
pub async fn rip_track_with_progress<F, S>(
    device: &str,
    track_number: i32,
    total_sectors: i64,
    output_path: &Path,
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

    let mut cmd = Command::new("cd-paranoia");
    cmd.args(["-d", device, "-e", "-w"])
        .arg(track_number.to_string())
        .arg(output_path)
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    let mut child: Child = cmd.spawn().map_err(|e| format!("cd-paranoia failed to launch: {e}"))?;
    if let Some(pid) = child.id() {
        on_started(pid);
    }

    let stderr = child.stderr.take().expect("stderr was piped");
    let mut lines = BufReader::new(stderr).lines();
    let mut collected = String::new();
    while let Ok(Some(line)) = lines.next_line().await {
        if total_sectors > 0 {
            if let Some(sector) = parse_sector(&line) {
                on_progress((sector as f64 / total_sectors as f64).clamp(0.0, 1.0));
            }
        }
        collected.push_str(&line);
        collected.push('\n');
    }

    let status = child.wait().await.map_err(|e| format!("cd-paranoia process error: {e}"))?;
    if !status.success() {
        let _ = std::fs::remove_file(output_path);
        if status.code().is_none() {
            return Err("cancelled".to_string());
        }
        return Err(format!("cd-paranoia exited with error: {collected}"));
    }

    Ok(())
}
