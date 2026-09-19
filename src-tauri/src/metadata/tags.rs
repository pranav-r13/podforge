use std::path::Path;

use lofty::config::WriteOptions;
use lofty::file::TaggedFileExt;
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::probe::Probe;
use lofty::tag::{Accessor, ItemKey, Tag, TagExt};
use serde::Deserialize;

/// Partial set of tag fields to apply to a file and/or DB row. `None` means
/// "leave as-is" -- this is shared by per-track edits and the bulk-edit
/// panel's shared-field patch, so every field must be independently optional.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct TagPatch {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub year: Option<i64>,
    pub genre: Option<String>,
    pub track_number: Option<i64>,
    pub disc_number: Option<i64>,
}

#[derive(Debug, thiserror::Error)]
pub enum TagError {
    #[error("failed to read tags from {0}: {1}")]
    Read(String, lofty::error::LoftyError),
    #[error("failed to write tags to {0}: {1}")]
    Write(String, lofty::error::LoftyError),
}

/// Reads the file at `path`, applies every `Some` field in `patch` to its
/// primary tag, and writes the result back in place.
pub fn write_tags(path: &Path, patch: &TagPatch) -> Result<(), TagError> {
    let mut tagged_file = Probe::open(path)
        .and_then(|p| p.read())
        .map_err(|e| TagError::Read(path.display().to_string(), e))?;

    if tagged_file.primary_tag().is_none() {
        let tag_type = tagged_file.primary_tag_type();
        tagged_file.insert_tag(Tag::new(tag_type));
    }
    let tag = tagged_file
        .primary_tag_mut()
        .expect("tag was just inserted if missing");

    if let Some(v) = &patch.title {
        tag.set_title(v.clone());
    }
    if let Some(v) = &patch.artist {
        tag.set_artist(v.clone());
    }
    if let Some(v) = &patch.album {
        tag.set_album(v.clone());
    }
    if let Some(v) = &patch.album_artist {
        tag.insert_text(ItemKey::AlbumArtist, v.clone());
    }
    if let Some(v) = &patch.genre {
        tag.set_genre(v.clone());
    }
    if let Some(v) = patch.year {
        tag.set_year(v as u32);
    }
    if let Some(v) = patch.track_number {
        tag.set_track(v as u32);
    }
    if let Some(v) = patch.disc_number {
        tag.set_disk(v as u32);
    }

    tag.save_to_path(path, WriteOptions::default())
        .map_err(|e| TagError::Write(path.display().to_string(), e))
}

fn mime_type_from_str(mime: &str) -> MimeType {
    match mime {
        "image/png" => MimeType::Png,
        "image/gif" => MimeType::Gif,
        "image/bmp" => MimeType::Bmp,
        "image/tiff" => MimeType::Tiff,
        "image/jpeg" | "image/jpg" => MimeType::Jpeg,
        other => MimeType::Unknown(other.to_string()),
    }
}

/// Replaces the front cover art on the file at `path`, leaving every other
/// tag field untouched. Any existing front cover is dropped first -- lofty
/// otherwise just appends, leaving stale art alongside the new image.
pub fn embed_cover_art(path: &Path, image_bytes: Vec<u8>, mime: &str) -> Result<(), TagError> {
    let mut tagged_file = Probe::open(path)
        .and_then(|p| p.read())
        .map_err(|e| TagError::Read(path.display().to_string(), e))?;

    if tagged_file.primary_tag().is_none() {
        let tag_type = tagged_file.primary_tag_type();
        tagged_file.insert_tag(Tag::new(tag_type));
    }
    let tag = tagged_file
        .primary_tag_mut()
        .expect("tag was just inserted if missing");

    tag.remove_picture_type(PictureType::CoverFront);
    tag.push_picture(Picture::new_unchecked(
        PictureType::CoverFront,
        Some(mime_type_from_str(mime)),
        None,
        image_bytes,
    ));

    tag.save_to_path(path, WriteOptions::default())
        .map_err(|e| TagError::Write(path.display().to_string(), e))
}
