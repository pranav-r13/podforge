use std::process::Command;

use serde::Serialize;

/// Info about a detected audio CD, returned to the frontend before ripping
/// starts. `disc_id` is `None` when libdiscid couldn't read the TOC (drive
/// busy, disc still spinning up) -- the caller should still allow ripping,
/// just without a MusicBrainz disc-ID match.
#[derive(Debug, Clone, Serialize)]
pub struct CdInfo {
    pub device: String,
    pub disc_id: Option<String>,
    pub track_count: i32,
}

/// Scans `diskutil list` for a disk whose partition scheme includes a
/// `CD_DA` slice -- macOS's marker for an inserted audio CD -- and returns
/// its whole-disk BSD device path (e.g. `/dev/disk3`). This is the same
/// device path libdiscid and `cd-paranoia` both expect via `-d`.
pub fn detect_cd() -> Option<String> {
    let output = Command::new("diskutil").arg("list").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);

    let mut current_disk: Option<String> = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("/dev/") {
            current_disk = rest.split_whitespace().next().map(|s| s.to_string());
            continue;
        }
        if trimmed.contains("CD_DA") {
            if let Some(disk) = &current_disk {
                return Some(format!("/dev/{disk}"));
            }
        }
    }
    None
}
