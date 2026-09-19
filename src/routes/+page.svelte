<script lang="ts">
  import { convertFileSrc, invoke } from "@tauri-apps/api/core";
  import { open } from "@tauri-apps/plugin-dialog";
  import { onMount } from "svelte";
  import type { Album, FixReport, MbCandidate, TagPatch } from "$lib/types";

  let albums = $state<Album[]>([]);
  let selectedAlbumId = $state<number | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);

  let selectedTrackIds = $state<Set<number>>(new Set());
  let bulkAlbum = $state("");
  let bulkAlbumArtist = $state("");
  let bulkYear = $state("");
  let bulkGenre = $state("");
  let bulkSaving = $state(false);

  let editingTrackId = $state<number | null>(null);
  let editTitle = $state("");
  let editTrackNumber = $state("");

  let fixing = $state(false);
  let fixReport = $state<FixReport | null>(null);

  let coverVersion = $state(0);
  let showMbDialog = $state(false);
  let mbSearching = $state(false);
  let mbCandidates = $state<MbCandidate[]>([]);
  let mbApplyingId = $state<string | null>(null);
  let coverUploading = $state(false);

  let selectedAlbum = $derived(
    albums.find((a) => a.id === selectedAlbumId) ?? null,
  );

  function coverSrc(path: string): string {
    // cache-bust: cover_art_path is stable but its file contents can change
    // in place (re-match, re-upload), so an unchanged path wouldn't reload.
    return `${convertFileSrc(path)}?v=${coverVersion}`;
  }

  async function loadLibrary() {
    try {
      albums = await invoke<Album[]>("get_library");
      if (selectedAlbumId === null && albums.length > 0) {
        selectedAlbumId = albums[0].id;
      }
    } catch (e) {
      error = String(e);
    }
  }

  function selectAlbum(id: number) {
    selectedAlbumId = id;
    selectedTrackIds = new Set();
    fixReport = null;
    editingTrackId = null;
  }

  async function importFolder() {
    const path = await open({ directory: true, multiple: false });
    if (!path) return;
    loading = true;
    error = null;
    try {
      await invoke("scan_folder", { path });
      await loadLibrary();
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  function formatDuration(ms: number | null): string {
    if (ms === null) return "--:--";
    const totalSeconds = Math.round(ms / 1000);
    const minutes = Math.floor(totalSeconds / 60);
    const seconds = totalSeconds % 60;
    return `${minutes}:${seconds.toString().padStart(2, "0")}`;
  }

  function toggleTrack(id: number) {
    const next = new Set(selectedTrackIds);
    if (next.has(id)) {
      next.delete(id);
    } else {
      next.add(id);
    }
    selectedTrackIds = next;
  }

  function toggleSelectAll() {
    if (!selectedAlbum) return;
    if (selectedTrackIds.size === selectedAlbum.tracks.length) {
      selectedTrackIds = new Set();
    } else {
      selectedTrackIds = new Set(selectedAlbum.tracks.map((t) => t.id));
    }
  }

  function startEdit(trackId: number, title: string, trackNumber: number | null) {
    editingTrackId = trackId;
    editTitle = title;
    editTrackNumber = trackNumber?.toString() ?? "";
  }

  async function saveEdit() {
    if (editingTrackId === null) return;
    const trackId = editingTrackId;
    const patch: TagPatch = {};
    if (editTitle.trim()) patch.title = editTitle.trim();
    if (editTrackNumber.trim()) {
      const n = parseInt(editTrackNumber, 10);
      if (!Number.isNaN(n)) patch.track_number = n;
    }
    editingTrackId = null;
    if (Object.keys(patch).length === 0) return;
    try {
      await invoke("update_track_tags", { trackId, patch });
      await loadLibrary();
    } catch (e) {
      error = String(e);
    }
  }

  function cancelEdit() {
    editingTrackId = null;
  }

  async function applyBulkEdit() {
    const patch: TagPatch = {};
    if (bulkAlbum.trim()) patch.album = bulkAlbum.trim();
    if (bulkAlbumArtist.trim()) patch.album_artist = bulkAlbumArtist.trim();
    if (bulkGenre.trim()) patch.genre = bulkGenre.trim();
    if (bulkYear.trim()) {
      const y = parseInt(bulkYear, 10);
      if (!Number.isNaN(y)) patch.year = y;
    }
    if (Object.keys(patch).length === 0) return;

    bulkSaving = true;
    error = null;
    try {
      await invoke("bulk_update_tags", {
        trackIds: Array.from(selectedTrackIds),
        patch,
      });
      await loadLibrary();
      selectedTrackIds = new Set();
      bulkAlbum = "";
      bulkAlbumArtist = "";
      bulkYear = "";
      bulkGenre = "";
    } catch (e) {
      error = String(e);
    } finally {
      bulkSaving = false;
    }
  }

  function cancelBulkEdit() {
    selectedTrackIds = new Set();
    bulkAlbum = "";
    bulkAlbumArtist = "";
    bulkYear = "";
    bulkGenre = "";
  }

  async function fixIpodCompat() {
    if (!selectedAlbum) return;
    fixing = true;
    fixReport = null;
    error = null;
    try {
      fixReport = await invoke<FixReport>("apply_ipod_compat_fix", {
        albumId: selectedAlbum.id,
      });
      await loadLibrary();
    } catch (e) {
      error = String(e);
    } finally {
      fixing = false;
    }
  }

  async function openMbDialog() {
    if (!selectedAlbum) return;
    showMbDialog = true;
    mbSearching = true;
    mbCandidates = [];
    error = null;
    try {
      mbCandidates = await invoke<MbCandidate[]>("lookup_musicbrainz", {
        albumId: selectedAlbum.id,
      });
    } catch (e) {
      error = String(e);
    } finally {
      mbSearching = false;
    }
  }

  function closeMbDialog() {
    showMbDialog = false;
    mbCandidates = [];
  }

  async function applyMbMatch(releaseId: string) {
    if (!selectedAlbum) return;
    mbApplyingId = releaseId;
    error = null;
    try {
      await invoke("apply_musicbrainz_match", {
        albumId: selectedAlbum.id,
        releaseId,
      });
      await loadLibrary();
      coverVersion += 1;
      showMbDialog = false;
      mbCandidates = [];
    } catch (e) {
      error = String(e);
    } finally {
      mbApplyingId = null;
    }
  }

  async function uploadCoverArt() {
    if (!selectedAlbum) return;
    const path = await open({
      multiple: false,
      filters: [{ name: "Image", extensions: ["jpg", "jpeg", "png"] }],
    });
    if (!path) return;
    coverUploading = true;
    error = null;
    try {
      await invoke("set_cover_art", {
        albumId: selectedAlbum.id,
        imagePath: path,
      });
      await loadLibrary();
      coverVersion += 1;
    } catch (e) {
      error = String(e);
    } finally {
      coverUploading = false;
    }
  }

  onMount(loadLibrary);
</script>

<div class="flex h-screen bg-white text-neutral-900 dark:bg-neutral-950 dark:text-neutral-100">
  <aside class="flex w-72 shrink-0 flex-col border-r border-neutral-200 dark:border-neutral-800">
    <div class="flex items-center justify-between px-4 py-3">
      <h1 class="text-sm font-medium text-neutral-500 dark:text-neutral-400">Albums</h1>
      <button
        onclick={importFolder}
        disabled={loading}
        class="rounded-md bg-neutral-900 px-3 py-1.5 text-xs font-medium text-white hover:bg-neutral-700 disabled:opacity-50 dark:bg-white dark:text-neutral-900 dark:hover:bg-neutral-200"
      >
        {loading ? "Scanning…" : "Import Folder"}
      </button>
    </div>

    {#if error}
      <p class="mx-4 mb-2 rounded bg-red-50 px-2 py-1 text-xs text-red-600 dark:bg-red-950 dark:text-red-400">
        {error}
      </p>
    {/if}

    <div class="flex-1 overflow-y-auto">
      {#if albums.length === 0}
        <p class="px-4 py-6 text-sm text-neutral-400">
          No albums yet. Import a folder to get started.
        </p>
      {/if}
      {#each albums as album (album.id)}
        <button
          onclick={() => selectAlbum(album.id)}
          class="flex w-full items-center gap-3 px-4 py-2 text-left hover:bg-neutral-100 dark:hover:bg-neutral-900 {selectedAlbumId ===
          album.id
            ? 'bg-neutral-100 dark:bg-neutral-900'
            : ''}"
        >
          {#if album.cover_art_path}
            <img
              src={coverSrc(album.cover_art_path)}
              alt=""
              class="h-10 w-10 shrink-0 rounded object-cover"
            />
          {:else}
            <div
              class="flex h-10 w-10 shrink-0 items-center justify-center rounded bg-neutral-200 text-sm font-medium text-neutral-500 dark:bg-neutral-800 dark:text-neutral-400"
            >
              {album.title.charAt(0).toUpperCase()}
            </div>
          {/if}
          <div class="min-w-0 flex-1">
            <p class="truncate text-sm font-medium">{album.title}</p>
            <p class="truncate text-xs text-neutral-500 dark:text-neutral-400">
              {album.album_artist ?? "Unknown Artist"} · {album.tracks.length}
              {album.tracks.length === 1 ? "track" : "tracks"}
            </p>
          </div>
        </button>
      {/each}
    </div>
  </aside>

  <main class="flex-1 overflow-y-auto">
    {#if selectedAlbum}
      <div class="px-8 py-6">
        <div class="flex items-start justify-between">
          <div class="flex items-start gap-4">
            {#if selectedAlbum.cover_art_path}
              <img
                src={coverSrc(selectedAlbum.cover_art_path)}
                alt=""
                class="h-20 w-20 shrink-0 rounded object-cover"
              />
            {:else}
              <div
                class="flex h-20 w-20 shrink-0 items-center justify-center rounded bg-neutral-200 text-2xl font-medium text-neutral-400 dark:bg-neutral-800 dark:text-neutral-600"
              >
                {selectedAlbum.title.charAt(0).toUpperCase()}
              </div>
            {/if}
            <div>
              <h2 class="text-xl font-semibold">{selectedAlbum.title}</h2>
              <p class="text-sm text-neutral-500 dark:text-neutral-400">
                {selectedAlbum.album_artist ?? "Unknown Artist"}
                {#if selectedAlbum.year}· {selectedAlbum.year}{/if}
              </p>
              <div class="mt-2 flex gap-2">
                <button
                  onclick={openMbDialog}
                  class="rounded-md border border-neutral-300 px-2 py-1 text-xs font-medium hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-900"
                >
                  Find Cover Art
                </button>
                <button
                  onclick={uploadCoverArt}
                  disabled={coverUploading}
                  class="rounded-md border border-neutral-300 px-2 py-1 text-xs font-medium hover:bg-neutral-100 disabled:opacity-50 dark:border-neutral-700 dark:hover:bg-neutral-900"
                >
                  {coverUploading ? "Uploading…" : "Upload Cover"}
                </button>
              </div>
            </div>
          </div>
          <button
            onclick={fixIpodCompat}
            disabled={fixing}
            class="shrink-0 rounded-md border border-neutral-300 px-3 py-1.5 text-xs font-medium hover:bg-neutral-100 disabled:opacity-50 dark:border-neutral-700 dark:hover:bg-neutral-900"
          >
            {fixing ? "Fixing…" : "Fix iPod Compatibility"}
          </button>
        </div>

        {#if fixReport}
          <div class="mt-3 rounded-md border border-neutral-200 px-3 py-2 text-xs dark:border-neutral-800">
            <p>
              Normalized tags on {fixReport.tags_normalized}
              {fixReport.tags_normalized === 1 ? "track" : "tracks"}.
              {#if fixReport.tracks_reencoded.length > 0}
                Re-encoded {fixReport.tracks_reencoded.length}
                mislabeled {fixReport.tracks_reencoded.length === 1 ? "file" : "files"}: {fixReport.tracks_reencoded.join(", ")}.
              {:else}
                No codec/container mismatches found.
              {/if}
            </p>
            {#if fixReport.errors.length > 0}
              <ul class="mt-1 list-inside list-disc text-red-600 dark:text-red-400">
                {#each fixReport.errors as err}
                  <li>{err}</li>
                {/each}
              </ul>
            {/if}
          </div>
        {/if}

        <table class="mt-6 w-full text-left text-sm">
          <thead>
            <tr class="border-b border-neutral-200 text-xs uppercase tracking-wide text-neutral-400 dark:border-neutral-800">
              <th class="w-8 py-2 pr-2">
                <input
                  type="checkbox"
                  checked={selectedAlbum.tracks.length > 0 && selectedTrackIds.size === selectedAlbum.tracks.length}
                  onchange={toggleSelectAll}
                />
              </th>
              <th class="py-2 pr-4 font-medium">#</th>
              <th class="py-2 pr-4 font-medium">Title</th>
              <th class="py-2 pr-4 font-medium">Artist</th>
              <th class="py-2 pr-4 font-medium">Codec</th>
              <th class="py-2 pr-4 font-medium text-right">Duration</th>
            </tr>
          </thead>
          <tbody>
            {#each selectedAlbum.tracks as track (track.id)}
              <tr class="border-b border-neutral-100 dark:border-neutral-900">
                <td class="py-2 pr-2">
                  <input
                    type="checkbox"
                    checked={selectedTrackIds.has(track.id)}
                    onchange={() => toggleTrack(track.id)}
                  />
                </td>
                {#if editingTrackId === track.id}
                  <td class="py-1 pr-4">
                    <input
                      type="text"
                      bind:value={editTrackNumber}
                      class="w-12 rounded border border-neutral-300 bg-transparent px-1 py-0.5 text-sm dark:border-neutral-700"
                    />
                  </td>
                  <td class="py-1 pr-4" colspan="3">
                    <input
                      type="text"
                      bind:value={editTitle}
                      onkeydown={(e) => e.key === "Enter" && saveEdit()}
                      class="w-full rounded border border-neutral-300 bg-transparent px-1 py-0.5 text-sm dark:border-neutral-700"
                    />
                  </td>
                  <td class="py-1 pr-4 text-right">
                    <button onclick={saveEdit} class="text-xs font-medium text-neutral-900 dark:text-neutral-100">Save</button>
                    <button onclick={cancelEdit} class="ml-2 text-xs text-neutral-400">Cancel</button>
                  </td>
                {:else}
                  <td class="py-2 pr-4 text-neutral-400">{track.track_number ?? "-"}</td>
                  <td
                    class="cursor-text py-2 pr-4"
                    ondblclick={() => startEdit(track.id, track.title, track.track_number)}
                    title="Double-click to edit"
                  >
                    {track.title}
                  </td>
                  <td class="py-2 pr-4 text-neutral-500 dark:text-neutral-400">
                    {track.artist ?? ""}
                  </td>
                  <td class="py-2 pr-4 text-neutral-500 dark:text-neutral-400">
                    {track.codec ?? ""}
                  </td>
                  <td class="py-2 pr-4 text-right text-neutral-500 dark:text-neutral-400">
                    {formatDuration(track.duration_ms)}
                  </td>
                {/if}
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {:else}
      <div class="flex h-full items-center justify-center text-sm text-neutral-400">
        Select an album to view its tracks.
      </div>
    {/if}
  </main>

  {#if selectedTrackIds.size > 0}
    <aside class="flex w-80 shrink-0 flex-col border-l border-neutral-200 px-4 py-4 dark:border-neutral-800">
      <h3 class="text-sm font-medium">
        Bulk Edit — {selectedTrackIds.size} selected
      </h3>
      <p class="mt-1 text-xs text-neutral-400">
        Leave a field blank to keep each track's existing value.
      </p>

      <label for="bulk-album" class="mt-4 block text-xs font-medium text-neutral-500 dark:text-neutral-400">Album</label>
      <input
        id="bulk-album"
        type="text"
        bind:value={bulkAlbum}
        class="mt-1 w-full rounded border border-neutral-300 bg-transparent px-2 py-1 text-sm dark:border-neutral-700"
      />

      <label for="bulk-album-artist" class="mt-3 block text-xs font-medium text-neutral-500 dark:text-neutral-400">Album Artist</label>
      <input
        id="bulk-album-artist"
        type="text"
        bind:value={bulkAlbumArtist}
        class="mt-1 w-full rounded border border-neutral-300 bg-transparent px-2 py-1 text-sm dark:border-neutral-700"
      />

      <label for="bulk-year" class="mt-3 block text-xs font-medium text-neutral-500 dark:text-neutral-400">Year</label>
      <input
        id="bulk-year"
        type="text"
        bind:value={bulkYear}
        class="mt-1 w-full rounded border border-neutral-300 bg-transparent px-2 py-1 text-sm dark:border-neutral-700"
      />

      <label for="bulk-genre" class="mt-3 block text-xs font-medium text-neutral-500 dark:text-neutral-400">Genre</label>
      <input
        id="bulk-genre"
        type="text"
        bind:value={bulkGenre}
        class="mt-1 w-full rounded border border-neutral-300 bg-transparent px-2 py-1 text-sm dark:border-neutral-700"
      />

      <div class="mt-6 flex gap-2">
        <button
          onclick={applyBulkEdit}
          disabled={bulkSaving}
          class="flex-1 rounded-md bg-neutral-900 px-3 py-1.5 text-xs font-medium text-white hover:bg-neutral-700 disabled:opacity-50 dark:bg-white dark:text-neutral-900 dark:hover:bg-neutral-200"
        >
          {bulkSaving ? "Applying…" : `Apply to ${selectedTrackIds.size} selected`}
        </button>
        <button
          onclick={cancelBulkEdit}
          class="rounded-md border border-neutral-300 px-3 py-1.5 text-xs font-medium hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-900"
        >
          Cancel
        </button>
      </div>
    </aside>
  {/if}

  {#if showMbDialog}
    <div
      class="fixed inset-0 z-10 flex items-center justify-center bg-black/40"
      role="button"
      tabindex="-1"
      onclick={closeMbDialog}
      onkeydown={(e) => e.key === "Escape" && closeMbDialog()}
    >
      <div
        role="dialog"
        tabindex="-1"
        class="max-h-[70vh] w-[32rem] overflow-y-auto rounded-lg bg-white p-4 shadow-xl dark:bg-neutral-900"
        onclick={(e) => e.stopPropagation()}
        onkeydown={(e) => e.stopPropagation()}
      >
        <div class="mb-3 flex items-center justify-between">
          <h3 class="text-sm font-medium">MusicBrainz matches</h3>
          <button onclick={closeMbDialog} class="text-xs text-neutral-400">Close</button>
        </div>

        {#if mbSearching}
          <p class="text-sm text-neutral-400">Searching…</p>
        {:else if mbCandidates.length === 0}
          <p class="text-sm text-neutral-400">
            No matches found. Try adjusting the album/artist tags, or upload a cover manually.
          </p>
        {:else}
          <ul class="space-y-1">
            {#each mbCandidates as candidate (candidate.release_id)}
              <li>
                <button
                  onclick={() => applyMbMatch(candidate.release_id)}
                  disabled={mbApplyingId !== null}
                  class="flex w-full flex-col items-start rounded-md border border-neutral-200 px-3 py-2 text-left text-sm hover:bg-neutral-100 disabled:opacity-50 dark:border-neutral-800 dark:hover:bg-neutral-800"
                >
                  <span class="font-medium">{candidate.title}</span>
                  <span class="text-xs text-neutral-500 dark:text-neutral-400">
                    {candidate.artist}
                    {#if candidate.date}· {candidate.date}{/if}
                    {#if candidate.country}· {candidate.country}{/if}
                    {#if candidate.disambiguation}· {candidate.disambiguation}{/if}
                  </span>
                  {#if mbApplyingId === candidate.release_id}
                    <span class="mt-1 text-xs text-neutral-400">Applying…</span>
                  {/if}
                </button>
              </li>
            {/each}
          </ul>
        {/if}
      </div>
    </div>
  {/if}
</div>
