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

export interface MbCandidate {
  release_id: string;
  title: string;
  artist: string;
  date: string | null;
  country: string | null;
  disambiguation: string | null;
}

export interface CdInfo {
  device: string;
  disc_id: string | null;
  track_count: number;
}

export interface CdImportResult {
  album_id: number;
  job_ids: number[];
}

export type ConvertFormat = "mp3" | "alac" | "flac" | "aac";

export interface Job {
  id: number;
  type: string;
  track_id: number | null;
  album_id: number | null;
  status: string;
  progress: number;
  error: string | null;
  created_at: string;
  completed_at: string | null;
}

export interface JobProgressEvent {
  job_id: number;
  track_id: number | null;
  status: string;
  progress: number;
  error: string | null;
}
