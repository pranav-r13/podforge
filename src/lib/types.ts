export interface Track {
  id: number;
  album_id: number;
  disc_number: number | null;
  track_number: number | null;
  title: string;
  artist: string | null;
  duration_ms: number | null;
  source_path: string;
  output_path: string | null;
  status: string;
  musicbrainz_recording_id: string | null;
  codec: string | null;
  container: string | null;
}

export interface Album {
  id: number;
  title: string;
  album_artist: string | null;
  year: number | null;
  genre: string | null;
  musicbrainz_release_id: string | null;
  cover_art_path: string | null;
  source_type: string;
  source_path: string | null;
  created_at: string;
  tracks: Track[];
}

/** Partial tag edit; omitted/undefined fields are left untouched. */
export interface TagPatch {
  title?: string;
  artist?: string;
  album?: string;
  album_artist?: string;
  year?: number;
  genre?: string;
  track_number?: number;
  disc_number?: number;
}

export interface FixReport {
  album_id: number;
  tags_normalized: number;
  tracks_reencoded: string[];
  errors: string[];
}
