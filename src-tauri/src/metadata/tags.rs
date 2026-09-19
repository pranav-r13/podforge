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

/// Reads back the primary tag's fields into a `TagPatch` for round-trip
/// assertions in tests.
#[cfg(test)]
fn read_tags(path: &Path) -> TagPatch {
    let tagged_file = Probe::open(path).and_then(|p| p.read()).expect("read fixture");
    let tag = tagged_file.primary_tag().expect("fixture has a tag after write_tags");
    TagPatch {
        title: tag.title().map(|s| s.to_string()),
        artist: tag.artist().map(|s| s.to_string()),
        album: tag.album().map(|s| s.to_string()),
        album_artist: tag.get_string(&ItemKey::AlbumArtist).map(|s| s.to_string()),
        year: tag.year().map(i64::from),
        genre: tag.genre().map(|s| s.to_string()),
        track_number: tag.track().map(i64::from),
        disc_number: tag.disk().map(i64::from),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_copy(name: &str, dir: &std::path::Path) -> std::path::PathBuf {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
        let dst = dir.join(name);
        std::fs::copy(&src, &dst).expect("copy fixture into tempdir");
        dst
    }

    fn full_patch() -> TagPatch {
        TagPatch {
            title: Some("Test Title".into()),
            artist: Some("Test Artist".into()),
            album: Some("Test Album".into()),
            album_artist: Some("Test Album Artist".into()),
            year: Some(1999),
            genre: Some("Electronic".into()),
            track_number: Some(7),
            disc_number: Some(2),
        }
    }

    #[test]
    fn write_tags_round_trips_every_field_on_flac() {
        let dir = tempfile::tempdir().unwrap();
        let path = fixture_copy("sample.flac", dir.path());

        write_tags(&path, &full_patch()).expect("write_tags should succeed on a valid FLAC");
        let read_back = read_tags(&path);

        assert_eq!(read_back.title.as_deref(), Some("Test Title"));
        assert_eq!(read_back.artist.as_deref(), Some("Test Artist"));
        assert_eq!(read_back.album.as_deref(), Some("Test Album"));
        assert_eq!(read_back.album_artist.as_deref(), Some("Test Album Artist"));
        assert_eq!(read_back.year, Some(1999));
        assert_eq!(read_back.genre.as_deref(), Some("Electronic"));
        assert_eq!(read_back.track_number, Some(7));
        assert_eq!(read_back.disc_number, Some(2));
    }

    #[test]
    fn write_tags_round_trips_every_field_on_mp3() {
        let dir = tempfile::tempdir().unwrap();
        let path = fixture_copy("sample.mp3", dir.path());

        write_tags(&path, &full_patch()).expect("write_tags should succeed on a valid MP3");
        let read_back = read_tags(&path);

        assert_eq!(read_back.title.as_deref(), Some("Test Title"));
        assert_eq!(read_back.artist.as_deref(), Some("Test Artist"));
        assert_eq!(read_back.album.as_deref(), Some("Test Album"));
        assert_eq!(read_back.album_artist.as_deref(), Some("Test Album Artist"));
        assert_eq!(read_back.year, Some(1999));
        assert_eq!(read_back.genre.as_deref(), Some("Electronic"));
        assert_eq!(read_back.track_number, Some(7));
        assert_eq!(read_back.disc_number, Some(2));
    }

    #[test]
    fn write_tags_leaves_unset_fields_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let path = fixture_copy("sample.flac", dir.path());

        write_tags(&path, &full_patch()).unwrap();
        write_tags(
            &path,
            &TagPatch {
                title: Some("Renamed".into()),
                ..Default::default()
            },
        )
        .unwrap();

        let read_back = read_tags(&path);
        assert_eq!(read_back.title.as_deref(), Some("Renamed"));
        // everything else from the first patch must survive an untouched-field patch
        assert_eq!(read_back.artist.as_deref(), Some("Test Artist"));
        assert_eq!(read_back.album.as_deref(), Some("Test Album"));
        assert_eq!(read_back.year, Some(1999));
    }

    #[test]
    fn round_trips_and_replaces_existing_front_cover() {
        let dir = tempfile::tempdir().unwrap();
        let path = fixture_copy("sample.flac", dir.path());

        embed_cover_art(&path, vec![1, 2, 3, 4], "image/png").unwrap();
        embed_cover_art(&path, vec![9, 9, 9], "image/jpeg").unwrap();

        let tagged_file = Probe::open(&path).and_then(|p| p.read()).unwrap();
        let tag = tagged_file.primary_tag().unwrap();
        let covers: Vec<_> = tag
            .pictures()
            .iter()
            .filter(|p| p.pic_type() == PictureType::CoverFront)
            .collect();

        assert_eq!(covers.len(), 1, "old front cover must be replaced, not appended");
        assert_eq!(covers[0].data(), &[9, 9, 9]);
        assert_eq!(covers[0].mime_type(), Some(&MimeType::Jpeg));
    }
}
