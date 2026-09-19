<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { open } from "@tauri-apps/plugin-dialog";
  import { onMount } from "svelte";
  import type { Album } from "$lib/types";

  let albums = $state<Album[]>([]);
  let selectedAlbumId = $state<number | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);

  let selectedAlbum = $derived(
    albums.find((a) => a.id === selectedAlbumId) ?? null,
  );

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
          onclick={() => (selectedAlbumId = album.id)}
          class="flex w-full items-center gap-3 px-4 py-2 text-left hover:bg-neutral-100 dark:hover:bg-neutral-900 {selectedAlbumId ===
          album.id
            ? 'bg-neutral-100 dark:bg-neutral-900'
            : ''}"
        >
          <div
            class="flex h-10 w-10 shrink-0 items-center justify-center rounded bg-neutral-200 text-sm font-medium text-neutral-500 dark:bg-neutral-800 dark:text-neutral-400"
          >
            {album.title.charAt(0).toUpperCase()}
          </div>
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
        <h2 class="text-xl font-semibold">{selectedAlbum.title}</h2>
        <p class="text-sm text-neutral-500 dark:text-neutral-400">
          {selectedAlbum.album_artist ?? "Unknown Artist"}
          {#if selectedAlbum.year}· {selectedAlbum.year}{/if}
        </p>

        <table class="mt-6 w-full text-left text-sm">
          <thead>
            <tr class="border-b border-neutral-200 text-xs uppercase tracking-wide text-neutral-400 dark:border-neutral-800">
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
                <td class="py-2 pr-4 text-neutral-400">{track.track_number ?? "-"}</td>
                <td class="py-2 pr-4">{track.title}</td>
                <td class="py-2 pr-4 text-neutral-500 dark:text-neutral-400">
                  {track.artist ?? ""}
                </td>
                <td class="py-2 pr-4 text-neutral-500 dark:text-neutral-400">
                  {track.codec ?? ""}
                </td>
                <td class="py-2 pr-4 text-right text-neutral-500 dark:text-neutral-400">
                  {formatDuration(track.duration_ms)}
                </td>
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
</div>
