# Podforge

Local music library manager: import from folder or CD, fix tags for iPod compatibility, fetch cover art, convert between formats (MP3/ALAC/FLAC/AAC).

Built with Tauri 2 (Rust backend) + SvelteKit + TypeScript + Tailwind. See `plan.md` for architecture and build phase history.

## Stack

- **Shell**: Tauri 2.x, Rust backend
- **Frontend**: Svelte + TypeScript + Tailwind
- **DB**: SQLite via `rusqlite`, migrations via `refinery`
- **Tags**: `lofty` (reads/writes FLAC/MP3/M4A/ALAC/ID3v2/Vorbis comments)
- **Convert**: `ffmpeg` subprocess (`libmp3lame`, `alac`, `flac` encoders)
- **CD rip**: `cd-paranoia` (from `libcdio-paranoia`) subprocess
- **Disc ID**: `libdiscid` via Rust `discid` crate (FFI), TOC lookup against MusicBrainz
- **Metadata lookup**: MusicBrainz API (rate-limited 1 req/sec, custom User-Agent required) + Cover Art Archive
- **Jobs**: `tokio` async runtime, bounded worker pool (semaphore), job state persisted in SQLite, progress pushed to frontend via Tauri events

App identifier: `com.pranavr.podforge`. Product name: `Podforge`.

## Requirements

```
brew install libcdio-paranoia libdiscid cd-discid pkg-config
```

`cdparanoia` is deprecated/removed from Homebrew — use `libcdio-paranoia` (`cd-paranoia` CLI) instead. `pkg-config` is needed to link `libdiscid` for the `discid` crate build. `ffmpeg` must already be installed and on `PATH` with `libmp3lame`/`alac`/`flac` encoders.

Node deps: `npm install`. Rust deps resolve automatically via `cargo`/`tauri`.

## Running the app (dev mode)

No packaged `.app` has been built yet — run in dev mode:

```
npm run tauri dev
```

(equivalent to `cargo tauri dev` from `src-tauri/`). This starts the Vite dev server (`http://localhost:1420`) and opens the native Tauri window.

## Building a packaged app

```
npm run tauri build
```

Produces an unsigned `.app` (multi-minute release/LTO build). Not yet run/verified this session — first launch will need a right-click → Open once for Gatekeeper. Icons and bundle identifier are already configured in `src-tauri/tauri.conf.json`.

## Data model (SQLite)

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
  title TEXT, artist TEXT,
  duration_ms INTEGER,
  source_path TEXT,        -- original file or ripped WAV
  output_path TEXT,        -- converted file, null until job done
  status TEXT,             -- pending|ripped|tagged|converted|error
  musicbrainz_recording_id TEXT,
  codec TEXT, container TEXT
)

jobs(
  id INTEGER PK,
  type TEXT,               -- rip|convert|tag_write|art_fetch|ipod_fix
  track_id INTEGER FK NULL,
  album_id INTEGER FK NULL,
  status TEXT,             -- queued|running|done|error|cancelled
  progress REAL,
  error TEXT,
  created_at TEXT, completed_at TEXT
)

settings(key TEXT PK, value TEXT)   -- output_dir, default_format, default_quality, mb_user_agent, concurrency
```

## Rust backend modules (`src-tauri/src/`)

```
lib.rs               -- Tauri setup, command registration, event emitter wiring, logging init
db/
  mod.rs             -- connection pool, migrations
  models.rs          -- Album, Track, Job, Settings structs (serde)
  queries.rs         -- CRUD, get_setting/set_setting
import/
  folder_scan.rs     -- walk dir, read existing tags via lofty, group into albums
  cd_detect.rs        -- poll for inserted disc (diskutil list), extract device path
  cd_rip.rs            -- shell cd-paranoia per track, write WAV to temp
  disc_id.rs            -- compute MB disc ID via libdiscid FFI (module named disc_id to avoid shadowing the discid crate)
metadata/
  musicbrainz.rs        -- release search, disc-id lookup, release detail fetch, rate limiter
  cover_art.rs           -- Cover Art Archive fetch
  tags.rs                 -- lofty read/write wrapper, bulk apply, embed_cover_art
  ipod_fix.rs              -- force album/album_artist uniform, ffprobe codec check, re-encode FLAC-in-mp3
convert/
  ffmpeg.rs              -- command builder per format (mp3/alac/flac/aac), maps audio+cover streams
  job_queue.rs            -- tokio semaphore worker pool, persists job state, emits progress events, spawn_rip + conversion jobs share the same path
commands.rs              -- #[tauri::command] functions exposed to frontend
```

## Tauri commands (IPC surface)

```
scan_folder(path) -> Vec<Album>
detect_cd() -> Option<CdInfo>
lookup_cd_release(disc_id) -> Vec<MbCandidate>
rip_and_import_cd(device, release_id?) -> CdImportResult { album_id, job_ids }
lookup_musicbrainz(album_id) -> MbCandidates
apply_musicbrainz_match(album_id, release_id)
get_library(filter?) -> Vec<Album>
get_album(album_id) -> Album
update_track_tags(track_id, TagPatch)
bulk_update_tags(track_ids[], TagPatch)
apply_ipod_compat_fix(album_id)
fetch_cover_art(album_id) -> ArtCandidates
set_cover_art(album_id, image_path_or_url)
enqueue_conversion(track_ids[], format, quality, output_dir) -> job_ids[]
get_job_status(job_id) / list_jobs(status?)
cancel_job(job_id)
get_settings() / update_settings(patch)
```

Frontend subscribes to the `job-progress` event instead of polling.

## Frontend screens (Svelte)

1. **Library** — sidebar album list (cover thumbnail, title, artist, track count), main pane track table with inline-editable title/track#, multi-album shift-click selection.
2. **Import** — "Import Folder" (scan_folder) and "Import CD" (auto-detect, MB match candidates dialog, live per-track rip progress).
3. **Bulk Edit panel** (slide-over) — shared fields (Album, Album Artist, Year, Genre, Cover Art), "Apply to N selected", separate "Fix iPod Compatibility" button.
4. **Convert dialog** — format (MP3/ALAC/FLAC/AAC), quality preset, output folder (default `~/Music/Converted/{Artist}/{Album}/{NN} {Title}.ext`), seeds from Settings.
5. **Jobs panel** (bottom drawer, persistent) — active/queued/done/error, progress bars, cancel, click error for ffmpeg/cd-paranoia stderr.
6. **Settings** — output dir, default format/quality, concurrency slider, required MusicBrainz User-Agent field.

Design: single accent color, system font stack, whitespace-first, dark/light follows macOS appearance, no component library (hand-rolled Tailwind).

## Build phase status

| Phase | Description | Status |
|---|---|---|
| 0 | Scaffold | DONE |
| 1 | Folder import + library view | DONE |
| 2 | Bulk tag edit + iPod compat fix | DONE (`4b53d25`) |
| 3 | Cover art (MusicBrainz + manual upload) | DONE |
| 4 | Conversion pipeline | DONE (`bfd8d66`) — not yet manually click-tested end-to-end |
| 5 | CD import (disc-ID lookup + cd-paranoia rip) | DONE, code only — needs physical CD drive to verify |
| 6 | Settings, structured logging, packaging | DONE (`b1515fc`) — `cargo tauri build` not yet run |
| 7 | Tests | Unit tests DONE (`a88938f`, 31 passing) — manual QA matrix still open |

Known gap fixed in Phase 6: MusicBrainz User-Agent was hardcoded in Phase 3, moved to the `mb_user_agent` setting (falls back to `musicbrainz::DEFAULT_USER_AGENT` if unset, only errors if explicitly cleared to blank).

## Outstanding manual verification (needs a human at the keyboard)

1. **Phase 4** — click-test conversion pipeline: select tracks → Convert → confirm output file plays with correct tags/cover, test Cancel.
2. **Phase 5** — with a physical audio CD inserted: confirm `diskutil list`'s `CD_DA` line shape, `/dev/diskN` vs `/dev/rdiskN`, and `cd-paranoia -e` progress line format all match current parser assumptions (parser silently falls back to no live progress bar if lines don't match — job still completes). Pinned unit tests (`cd_detect.rs::parse_diskutil_list`, `cd_rip.rs::parse_sector`) assert current assumptions only, not confirmed real hardware output — update them once verified.
3. **Phase 6** — run `npm run tauri build`, confirm the unsigned `.app` launches (right-click → Open once for Gatekeeper).
4. **Phase 7** — manual QA matrix: FLAC→MP3, FLAC→ALAC, MP3→ALAC, bulk 20+ track album, CD rip end-to-end with a physical disc.

Repo state: `main`, pushed through commit `a88938f`. Nothing outstanding in the working tree.

## Known risks

- MusicBrainz disc-ID lookup fails for many pressings (no exact TOC match) — manual search/match fallback is built in from day one, not an edge case.
- `cd-paranoia` error-correction retries can make ripping slow (minutes per disc) — per-track progress shown, doesn't block UI.
- ffmpeg jobs are CPU-heavy — worker pool defaults to `num_cpus / 2` so ripping/UI stay responsive during bulk convert. Concurrency changes take effect on next launch, not live (`tokio::sync::Semaphore` can't shrink permits once issued).
- ALAC output container must be `.m4a`, not `.alac` — ffmpeg command builder explicitly maps `-f ipod`/mov,mp4 muxer per target.

## Recommended IDE Setup

[VS Code](https://code.visualstudio.com/) + [Svelte](https://marketplace.visualstudio.com/items?itemName=svelte.svelte-vscode) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer).
