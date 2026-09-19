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

**Phase 4 — Conversion pipeline — DONE**
Shipped in commit `bfd8d66`: `convert/ffmpeg.rs` (command builder for mp3/alac/flac/aac, maps audio+cover streams, ALAC/AAC forced to `.m4a` with `-f ipod` muxer, output path templated `{output_dir}/{Artist}/{Album}/{NN} {Title}.ext`), `convert/job_queue.rs` (tokio semaphore worker pool sized `num_cpus/2`, persists job state to the existing `jobs` table, emits `job-progress` events, cancels by killing the ffmpeg pid). New commands: `enqueue_conversion`, `get_job_status`, `list_jobs`, `cancel_job`. Frontend: Convert dialog (format/quality/output folder picker) off the bulk-selection panel, persistent bottom Jobs drawer with progress bars and cancel, subscribes to `job-progress` instead of polling.
Added `tokio` dependency (`process`, `sync`, `rt-multi-thread`, `io-util`, `macros` features).
`cargo build` and `svelte-check` pass clean, `cargo tauri dev` boots with no runtime error. Not yet manually click-tested end-to-end (native file picker + a real ffmpeg run) -- do that next: select tracks, Convert, pick a format/output folder, watch the Jobs drawer, confirm the output file plays and has correct tags/cover.

**Phase 5 — CD import — DONE (code), needs a physical disc to verify**
Shipped this session: `import/cd_detect.rs` (parses `diskutil list` plain output for a `CD_DA` partition scheme, returns the whole-disk BSD device path e.g. `/dev/disk3`), `import/disc_id.rs` (wraps the `discid` crate/libdiscid FFI — named `disc_id` not `discid` to avoid shadowing the external crate inside a same-named module), `import/cd_rip.rs` (shells `cd-paranoia -d <device> -e -w <track> <outfile>` per track, parses `##: n [label] @ <sector>` stderr lines into a 0..1 progress fraction). `metadata/musicbrainz.rs` gained `lookup_by_discid` (`/ws/2/discid/{id}`, 404 = no match, not an error) and `fetch_release_detail` (`/ws/2/release/{id}?inc=recordings+artist-credits`, first medium only — multi-disc box sets aren't disambiguated by disc number yet). `convert/job_queue.rs`'s existing `JobQueue` gained `spawn_rip`, sharing the same semaphore/pid-map/cancel path as conversion jobs (job type `"rip"`, same `jobs` table and `job-progress` event, so the existing Jobs drawer needed no changes). New commands: `detect_cd() -> Option<CdInfo>`, `lookup_cd_release(disc_id) -> Vec<MbCandidate>`, `rip_and_import_cd(device, release_id?) -> CdImportResult { album_id, job_ids }` (creates the album + all track rows up front with real or MB-sourced titles, then spawns one rip job per track; each job writes DB-known tags into the WAV via the existing `tags::write_tags` right after ripping, so a later Phase 4 conversion picks them up). Frontend: "Import CD" button next to "Import Folder", a dialog mirroring the MusicBrainz-match dialog's styling (detect → auto disc-ID lookup → pick a candidate or "Rip without a match" → jobs drawer takes over).
Deps installed this session: `brew install libcdio-paranoia libdiscid cd-discid pkg-config` (pkg-config wasn't present; the `discid` crate's build needs it to link `libdiscid`). `cargo build`, `svelte-check`, and a 20s `cargo tauri dev` boot all pass clean.
**Not verified — no physical CD drive in this session's environment:** `diskutil list`'s exact `CD_DA` line shape, whether libdiscid/`cd-paranoia` accept `/dev/diskN` vs `/dev/rdiskN` on this Mac, and the `cd-paranoia -e` progress line format (parser has a silent-fallback: if lines don't match, the job still completes correctly, just without a live progress bar). First real session with a disc should: insert an audio CD, click Import CD, confirm device/disc-ID/track count look right, try both a disc-ID match and "Rip without a match", watch the Jobs drawer, confirm the ripped WAV plays and carries correct tags, then run it through the existing Convert pipeline end-to-end.

**Phase 6 — Settings, error handling, packaging**
Settings screen, structured logging (`tracing` crate, log file under `~/Library/Logs/`), `cargo tauri build` for a local `.app` — since this is personal single-machine use, unsigned build is fine (Gatekeeper right-click-open once); skip notarization unless sharing the binary.

**Phase 7 — Tests**
Unit tests: tag round-trip (write then read back via lofty), ffmpeg command construction (assert argv, no actual encode), MusicBrainz JSON parsing against saved fixture responses. Manual QA matrix: FLAC→MP3, FLAC→ALAC, MP3→ALAC, bulk 20+ track album, CD rip end-to-end with a physical disc.

## Handoff (2026-09-20, later same day)

Repo state right now:
- `main` branch, last pushed commit still `bfd8d66` — this session's Phase 5 (CD import) work is **uncommitted**. Also uncommitted: this file's Phase 4/5 handoff edits from earlier today.
- Everything builds clean (`cargo build`, `svelte-check`, 20s `cargo tauri dev` boot) but is **unverified against real hardware**: no physical CD drive in this session's sandbox, and Phase 4's conversion pipeline is still only smoke-tested by boot, not by an actual click-through (select tracks → Convert → confirm output file). Both need a real session at the keyboard.
- Next action for a fresh session, in order: (1) commit Phase 5 as its own commit, (2) click-test Phase 4 (folder-import → select tracks → Convert → watch Jobs drawer → confirm output file plays with correct tags/cover, also test Cancel), (3) with a physical audio CD inserted, click-test Phase 5 per the notes above, fixing whatever the real `diskutil list`/`cd-paranoia` output shapes turn out to need, (4) commit any Phase 5 fixes, then start **Phase 6 — Settings, error handling, packaging**.
- `settings` table exists in the schema (`V1__init.sql`) but has no queries/commands touching it yet -- Phase 6 is the first phase that needs it.
- Known gap carried from Phase 3: MusicBrainz User-Agent is hardcoded in `musicbrainz.rs`/`cover_art.rs` (`const USER_AGENT`) -- move it to a required Settings field (`mb_user_agent`) once Settings exists (Phase 6).

## Known risks to plan around
- MusicBrainz disc-ID lookup fails for many pressings (no exact TOC match) — must build manual search/match fallback from day one, not as an edge case.
- `cd-paranoia` error-correction retries can make ripping slow (minutes per disc) — show per-track progress, don't block UI.
- Concurrency: ffmpeg jobs are CPU-heavy, default worker pool to `num_cpus / 2` so ripping/UI stay responsive during bulk convert.
- ALAC output container must be `.m4a`, not `.alac` — ffmpeg command builder needs explicit `-f ipod`/mov,mp4 muxer mapping per target.
