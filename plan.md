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

**Phase 0 — Scaffold — DONE**
`npm create tauri-app` (Svelte-TS template), wire rusqlite + refinery migrations, empty window boots, `cargo tauri dev` works.

**Phase 1 — Folder import + library view — DONE**
`folder_scan.rs` walks a dir, reads tags via lofty, groups by folder (or embedded album tag if present), inserts into DB. Library screen renders it. Test against existing `FLAC1`/`FLAC2`/`808s and heartbreaks` folders.

**Phase 2 — Bulk tag edit + iPod compat fix — DONE**
Inline track edit, bulk-edit panel, `ipod_fix.rs` port of instructions.md steps 2–4 (force album/album_artist uniform, ffprobe codec check, re-encode FLAC-in-mp3). This alone replaces the manual shell workflow.
Shipped in commit `4b53d25`: `commands::update_track_tags`, `bulk_update_tags`, `apply_ipod_compat_fix`; `metadata/tags.rs` (lofty read/write wrapper), `metadata/ipod_fix.rs` (ffprobe codec check + ffmpeg re-encode-in-place). Frontend: inline title/track# edit (dblclick), bulk-edit slide-over, "Fix iPod Compatibility" button + report panel in `+page.svelte`.

**Phase 3 — Cover art — DONE**
MusicBrainz Cover Art Archive fetch by release id (once matched) + manual drag-drop image fallback, embed via lofty, one-click apply to whole album.
Shipped this session (uncommitted — see Handoff below): `metadata/musicbrainz.rs` (text search `release:"album" AND artist:"artist"` against `/ws/2/release/`, 1 req/sec self-rate-limited, returns candidates for manual disambiguation — no disc-ID lookup yet, that's Phase 5), `metadata/cover_art.rs` (Cover Art Archive `/release/{mbid}/front` fetch), `metadata/tags.rs::embed_cover_art` (lofty `Picture`/`PictureType::CoverFront`, replaces any existing front cover before pushing). New commands: `lookup_musicbrainz`, `apply_musicbrainz_match` (sets release id + best-effort auto-fetches art), `set_cover_art` (manual file picker fallback — no drag-drop yet, uses the existing `tauri-plugin-dialog` file picker instead, same UX outcome). Covers are copied to `$APPDATA/covers/{album_id}.{ext}` and displayed via `convertFileSrc` (needed adding `assetProtocol` scope to `tauri.conf.json` + the `protocol-asset` cargo feature on `tauri`). Frontend: cover thumbnail in sidebar + album header, "Find Cover Art" modal (lists MB candidates, click to apply), "Upload Cover" button.
Known gap: MusicBrainz User-Agent is hardcoded in `musicbrainz.rs`/`cover_art.rs` (`const USER_AGENT`) — plan's Phase 6 wants this as a required Settings field (`mb_user_agent`); move it there once Settings exists instead of leaving it hardcoded.

**Phase 4 — Conversion pipeline — NEXT UP**
`ffmpeg.rs` command builder (mirrors instructions.md ffmpeg invocation: map audio+cover streams, correct codec flags per target format), `job_queue.rs` worker pool, Convert dialog, Jobs panel with live progress (`ffmpeg -progress pipe:1` parsed per job).

**Phase 5 — CD import**
`cd_detect.rs` polls `diskutil list` for audio CD; `discid.rs` computes MB disc ID; `musicbrainz.rs` looks up release by disc ID (manual search UI if no match); `cd_rip.rs` shells `cd-paranoia` per track to WAV; feeds into Phase 1–4 pipeline (tag, art, convert) as one flow.

**Phase 6 — Settings, error handling, packaging**
Settings screen, structured logging (`tracing` crate, log file under `~/Library/Logs/`), `cargo tauri build` for a local `.app` — since this is personal single-machine use, unsigned build is fine (Gatekeeper right-click-open once); skip notarization unless sharing the binary.

**Phase 7 — Tests**
Unit tests: tag round-trip (write then read back via lofty), ffmpeg command construction (assert argv, no actual encode), MusicBrainz JSON parsing against saved fixture responses. Manual QA matrix: FLAC→MP3, FLAC→ALAC, MP3→ALAC, bulk 20+ track album, CD rip end-to-end with a physical disc.

## Handoff (2026-09-19)

Repo state right now:
- `main` branch, last pushed commit `4b53d25` ("feat: bulk tag edit + iPod compat fix (Phase 2)").
- Working tree has **uncommitted Phase 3 (cover art) changes**, not yet committed or pushed:
  - New: `src-tauri/src/metadata/musicbrainz.rs`, `src-tauri/src/metadata/cover_art.rs`
  - Modified: `src-tauri/Cargo.toml` (added `reqwest` blocking+json+rustls-tls, `tauri` gained `protocol-asset` feature), `src-tauri/tauri.conf.json` (added `app.security.assetProtocol` scope for `$APPDATA/covers/*`), `src-tauri/src/metadata/mod.rs`, `src-tauri/src/metadata/tags.rs` (added `embed_cover_art`), `src-tauri/src/db/queries.rs` (added `set_album_musicbrainz_release`, `set_album_cover_art_path`), `src-tauri/src/commands.rs` (added `lookup_musicbrainz`, `apply_musicbrainz_match`, `set_cover_art`), `src-tauri/src/lib.rs` (registered the 3 new commands), `src/lib/types.ts` (added `MbCandidate`), `src/routes/+page.svelte` (cover thumbnails, "Find Cover Art" modal, "Upload Cover" button).
  - Both `cargo build --manifest-path src-tauri/Cargo.toml` and `npx svelte-check` pass clean as of this session. Not yet run through `cargo tauri dev` for a manual click-through — do that before committing, then commit as a Phase 3 feat commit and push.
- Next action for a fresh session: manually smoke-test cover art (folder-import an album, "Find Cover Art" → pick a match → confirm image shows in sidebar+header and is embedded in the actual file via `ffprobe`/a player; also test "Upload Cover" fallback), then commit + push, then start **Phase 4 — Conversion pipeline** (`convert/ffmpeg.rs` command builder, `convert/job_queue.rs` tokio semaphore worker pool + SQLite-persisted job state, Convert dialog + Jobs panel with live progress via `ffmpeg -progress pipe:1`).
- `jobs` table and `settings` table exist in the schema (`V1__init.sql`) but have no queries/commands touching them yet — Phase 4 is the first phase that needs them.
- No `tokio` crate dependency yet — Phase 4's job queue needs it; Tauri 2 bundles an async runtime for `async fn` commands but the plan's semaphore worker pool wants explicit `tokio` (with `sync`/`process` features) added to `Cargo.toml`.

## Known risks to plan around
- MusicBrainz disc-ID lookup fails for many pressings (no exact TOC match) — must build manual search/match fallback from day one, not as an edge case.
- `cd-paranoia` error-correction retries can make ripping slow (minutes per disc) — show per-track progress, don't block UI.
- Concurrency: ffmpeg jobs are CPU-heavy, default worker pool to `num_cpus / 2` so ripping/UI stay responsive during bulk convert.
- ALAC output container must be `.m4a`, not `.alac` — ffmpeg command builder needs explicit `-f ipod`/mov,mp4 muxer mapping per target.
