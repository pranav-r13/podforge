# Podforge — Implementation Plan (Local Music Library Manager, Mac)

## Stack
- **Shell**: Tauri 2.x, Rust backend
- **Frontend**: Svelte + TypeScript + Tailwind (minimal utility CSS, no heavy component lib)
- **DB**: SQLite via `rusqlite`, migrations via `refinery`
- **Tags**: `lofty` (pure Rust, reads/writes FLAC/MP3/M4A/ALAC/ID3v2/Vorbis comments — no C bindgen needed)
- **Convert**: `ffmpeg` subprocess (already installed, has `libmp3lame`, `alac`, `flac` encoders confirmed)
- **CD rip**: `cd-paranoia` (from `libcdio-paranoia`) subprocess for audio extraction
- **Disc ID**: `libdiscid` via Rust `discid` crate (FFI) for MusicBrainz TOC lookup, or shell `cd-discid` as fallback
- **Metadata lookup**: MusicBrainz API (`reqwest`, rate-limited 1 req/sec, requires custom User-Agent per their ToS) + Cover Art Archive
- **Jobs**: `tokio` async runtime, bounded worker pool (semaphore), jobs persisted in SQLite, progress pushed to frontend via Tauri event emitter

Required installs before starting:
```
brew install libcdio-paranoia libdiscid cd-discid
```

Note: `cdparanoia` is deprecated/removed from Homebrew. Use `libcdio-paranoia` (provides `cd-paranoia` CLI) instead.

## Data Model (SQLite)

```sql
albums(
  id INTEGER PK,
  title TEXT, album_artist TEXT, year INTEGER, genre TEXT,
  musicbrainz_release_id TEXT,
  cover_art_path TEXT,
  source_type TEXT,        -- 'cd' | 'folder'
  created_at TEXT
)

tracks(
  id INTEGER PK,
  album_id INTEGER FK,
  disc_number INTEGER, track_number INTEGER,
  title TEXT, artist TEXT,          -- per-track artist, feat. stays here per iPod workflow
  duration_ms INTEGER,
  source_path TEXT,                 -- original file or ripped WAV
  output_path TEXT,                 -- converted file, null until job done
  status TEXT,                      -- pending|ripped|tagged|converted|error
  musicbrainz_recording_id TEXT,
  codec TEXT, container TEXT        -- detected via ffprobe, catches FLAC-in-.mp3 bug
)

jobs(
  id INTEGER PK,
  type TEXT,                        -- rip|convert|tag_write|art_fetch|ipod_fix
  track_id INTEGER FK NULL,
  album_id INTEGER FK NULL,
  status TEXT,                      -- queued|running|done|error|cancelled
  progress REAL,
  error TEXT,
  created_at TEXT, completed_at TEXT
)

settings(key TEXT PK, value TEXT)   -- output_dir, default_format, default_quality, mb_user_agent, concurrency
```

## Rust Backend Modules (`src-tauri/src/`)

```
lib.rs               -- Tauri setup, command registration, event emitter wiring
db/
  mod.rs             -- connection pool, migrations
  models.rs           -- Album, Track, Job structs (serde)
  queries.rs           -- CRUD
import/
  folder_scan.rs      -- walk dir, read existing tags via lofty, group into albums
  cd_detect.rs         -- poll for inserted disc (diskutil list), extract device path
  cd_rip.rs             -- shell cd-paranoia per track, write WAV to temp
  discid.rs             -- compute MB disc ID via libdiscid FFI
metadata/
  musicbrainz.rs        -- release lookup by disc id, search by artist/album fallback, rate limiter
  cover_art.rs           -- Cover Art Archive fetch
  tags.rs                 -- lofty read/write wrapper, bulk apply
  ipod_fix.rs              -- port of instructions.md workflow: force album/album_artist uniform,
                              detect FLAC-in-mp3 via ffprobe codec check, re-encode broken files
convert/
  ffmpeg.rs              -- command builder per format (mp3/alac/flac/aac), maps audio+cover streams
  job_queue.rs            -- tokio semaphore worker pool, persists job state, emits progress events
commands.rs              -- #[tauri::command] functions exposed to frontend (list below)
```

## Tauri Commands (IPC surface)

```
scan_folder(path) -> Vec<Album>
detect_cd() -> Option<CdInfo>
rip_and_import_cd(device_path) -> job_id
lookup_musicbrainz(album_id) -> MbCandidates      -- manual disambiguation if multiple matches
apply_musicbrainz_match(album_id, release_id)      -- writes tags+art from chosen release
get_library(filter?) -> Vec<Album>                 -- with nested tracks
get_album(album_id) -> Album
update_track_tags(track_id, TagPatch)
bulk_update_tags(track_ids[], TagPatch)             -- shared-field bulk edit
apply_ipod_compat_fix(album_id)                     -- runs instructions.md-style fix
fetch_cover_art(album_id) -> ArtCandidates
set_cover_art(album_id, image_path_or_url)
enqueue_conversion(track_ids[], format, quality, output_dir) -> job_ids[]
get_job_status(job_id) / list_jobs(status?)
cancel_job(job_id)
get_settings() / update_settings(patch)
```

Frontend subscribes to `job-progress` events (emitted per job tick) instead of polling.

## Frontend Screens (Svelte)

1. **Library** (default view) — left sidebar: Albums list (cover thumbnail, title, artist, track count). Main pane: table of tracks for selected album, checkboxes, inline-editable title/track#. Multi-album selection via shift-click in sidebar.
2. **Import** — two entry points: "Import Folder" (file picker → scan_folder), "Import CD" (auto-detects disc, shows MB match candidates if ambiguous, confirm → rip starts, live per-track progress).
3. **Bulk Edit panel** (slide-over, triggered by selection) — shared fields: Album, Album Artist, Year, Genre, Cover Art. "Apply to N selected" button. Separate "Fix iPod Compatibility" button runs `apply_ipod_compat_fix`.
4. **Convert dialog** (triggered by selection) — format (MP3/ALAC/FLAC/AAC), quality preset, output folder (default `~/Music/Converted/{Artist}/{Album}/{NN} {Title}.ext`), Convert button → enqueues, opens Jobs panel.
5. **Jobs panel** (bottom drawer, persistent) — active/queued/done/error, progress bars, cancel button, click error to see ffmpeg/cdparanoia stderr.
6. **Settings** — output dir, default format/quality, concurrency slider, MusicBrainz User-Agent string (required field, app won't query MB without it).

Design: single accent color, system font stack, no sidebar icons-as-decoration, whitespace-first, dark/light follows macOS appearance. No component library — hand-rolled Tailwind, keeps it minimal instead of looking like a Bootstrap clone.

## Build Phases (in order, each independently testable)

**Phase 0 — Scaffold**
`npm create tauri-app` (Svelte-TS template), wire rusqlite + refinery migrations, empty window boots, `cargo tauri dev` works.

**Phase 1 — Folder import + library view**
`folder_scan.rs` walks a dir, reads tags via lofty, groups by folder (or embedded album tag if present), inserts into DB. Library screen renders it. Test against existing `FLAC1`/`FLAC2`/`808s and heartbreaks` folders.

**Phase 2 — Bulk tag edit + iPod compat fix**
Inline track edit, bulk-edit panel, `ipod_fix.rs` port of instructions.md steps 2–4 (force album/album_artist uniform, ffprobe codec check, re-encode FLAC-in-mp3). This alone replaces the manual shell workflow.

**Phase 3 — Cover art**
MusicBrainz Cover Art Archive fetch by release id (once matched) + manual drag-drop image fallback, embed via lofty, one-click apply to whole album.

**Phase 4 — Conversion pipeline**
`ffmpeg.rs` command builder (mirrors instructions.md ffmpeg invocation: map audio+cover streams, correct codec flags per target format), `job_queue.rs` worker pool, Convert dialog, Jobs panel with live progress (`ffmpeg -progress pipe:1` parsed per job).

**Phase 5 — CD import**
`cd_detect.rs` polls `diskutil list` for audio CD; `discid.rs` computes MB disc ID; `musicbrainz.rs` looks up release by disc ID (manual search UI if no match); `cd_rip.rs` shells `cd-paranoia` per track to WAV; feeds into Phase 1–4 pipeline (tag, art, convert) as one flow.

**Phase 6 — Settings, error handling, packaging**
Settings screen, structured logging (`tracing` crate, log file under `~/Library/Logs/`), `cargo tauri build` for a local `.app` — since this is personal single-machine use, unsigned build is fine (Gatekeeper right-click-open once); skip notarization unless sharing the binary.

**Phase 7 — Tests**
Unit tests: tag round-trip (write then read back via lofty), ffmpeg command construction (assert argv, no actual encode), MusicBrainz JSON parsing against saved fixture responses. Manual QA matrix: FLAC→MP3, FLAC→ALAC, MP3→ALAC, bulk 20+ track album, CD rip end-to-end with a physical disc.

## Known risks to plan around
- MusicBrainz disc-ID lookup fails for many pressings (no exact TOC match) — must build manual search/match fallback from day one, not as an edge case.
- `cd-paranoia` error-correction retries can make ripping slow (minutes per disc) — show per-track progress, don't block UI.
- Concurrency: ffmpeg jobs are CPU-heavy, default worker pool to `num_cpus / 2` so ripping/UI stay responsive during bulk convert.
- ALAC output container must be `.m4a`, not `.alac` — ffmpeg command builder needs explicit `-f ipod`/mov,mp4 muxer mapping per target.
