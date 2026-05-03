<script lang="ts">
  import { onMount } from "svelte";
  import {
    Card,
    CardContent,
    CardHeader,
    CardTitle,
  } from "$lib/components/ui/card";
  import { Button } from "$lib/components/ui/button";
  import { libraryApi, type LibraryFile } from "$lib/api/index";

  let loading = $state(true);
  let error = $state("");
  let scannedAt = $state("");

  let moviesMatched: LibraryFile[] = $state([]);
  let moviesUnmatched: LibraryFile[] = $state([]);
  let tvMatched: LibraryFile[] = $state([]);
  let tvUnmatched: LibraryFile[] = $state([]);

  let showMoviesUnmatched = $state(false);
  let showTVUnmatched = $state(false);

  function formatSize(bytes: number): string {
    if (bytes >= 1_073_741_824) return (bytes / 1_073_741_824).toFixed(2) + " GB";
    if (bytes >= 1_048_576) return (bytes / 1_048_576).toFixed(1) + " MB";
    return (bytes / 1024).toFixed(0) + " KB";
  }

  function formatDate(iso?: string): string {
    if (!iso) return "—";
    try {
      return new Date(iso).toLocaleString();
    } catch {
      return iso;
    }
  }

  function truncatePath(path: string, max = 60): string {
    if (path.length <= max) return path;
    return "…" + path.slice(-(max - 1));
  }

  async function load() {
    loading = true;
    error = "";
    const res = await libraryApi.scan();
    if (res.error || !res.data) {
      error = res.message || "Failed to load library";
    } else {
      moviesMatched = res.data.movies?.matched ?? [];
      moviesUnmatched = res.data.movies?.unmatched ?? [];
      tvMatched = res.data.tvshows?.matched ?? [];
      tvUnmatched = res.data.tvshows?.unmatched ?? [];
      scannedAt = res.data.scanned_at ?? "";
    }
    loading = false;
  }

  onMount(load);
</script>

<svelte:head>
  <title>Library — OpenMediaProject</title>
</svelte:head>

<div class="space-y-8">
  <div class="flex items-center justify-between">
    <div>
      <h1 class="text-3xl font-bold">Library</h1>
      {#if scannedAt}
        <p class="text-sm text-muted-foreground mt-1">
          Last scanned: {formatDate(scannedAt)}
        </p>
      {/if}
    </div>
    <Button onclick={load} disabled={loading} variant="outline">
      {loading ? "Scanning…" : "Rescan"}
    </Button>
  </div>

  {#if loading}
    <div class="text-center py-20 text-muted-foreground">Scanning library…</div>
  {:else if error}
    <div class="text-center py-20 text-red-500">{error}</div>
  {:else}
    <!-- Movies Section -->
    <Card>
      <CardHeader>
        <CardTitle>
          Movies
          <span class="ml-2 text-base font-normal text-muted-foreground">
            {moviesMatched.length} matched, {moviesUnmatched.length} unmatched
          </span>
        </CardTitle>
      </CardHeader>
      <CardContent>
        {#if moviesMatched.length === 0}
          <p class="text-muted-foreground text-sm">No matched movie files found.</p>
        {:else}
          <div class="overflow-x-auto">
            <table class="w-full text-sm">
              <thead>
                <tr class="border-b text-left text-muted-foreground">
                  <th class="pb-2 pr-4 font-medium">Title</th>
                  <th class="pb-2 pr-4 font-medium">File Path</th>
                  <th class="pb-2 pr-4 font-medium">Size</th>
                  <th class="pb-2 font-medium">Last Modified</th>
                </tr>
              </thead>
              <tbody>
                {#each moviesMatched as file}
                  <tr class="border-b last:border-0 hover:bg-muted/30">
                    <td class="py-2 pr-4 font-medium">{file.title ?? "—"}</td>
                    <td class="py-2 pr-4 font-mono text-xs text-muted-foreground" title={file.path}>
                      {truncatePath(file.path)}
                    </td>
                    <td class="py-2 pr-4 whitespace-nowrap">{formatSize(file.size)}</td>
                    <td class="py-2 whitespace-nowrap text-muted-foreground">{formatDate(file.mod_time)}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        {/if}

        {#if moviesUnmatched.length > 0}
          <div class="mt-4">
            <button
              class="text-sm text-primary hover:underline flex items-center gap-1"
              onclick={() => (showMoviesUnmatched = !showMoviesUnmatched)}
            >
              <span>{showMoviesUnmatched ? "▾" : "▸"}</span>
              {moviesUnmatched.length} unmatched file{moviesUnmatched.length !== 1 ? "s" : ""}
            </button>
            {#if showMoviesUnmatched}
              <div class="mt-2 overflow-x-auto">
                <table class="w-full text-sm">
                  <thead>
                    <tr class="border-b text-left text-muted-foreground">
                      <th class="pb-2 pr-4 font-medium">File Path</th>
                      <th class="pb-2 pr-4 font-medium">Size</th>
                      <th class="pb-2 font-medium">Last Modified</th>
                    </tr>
                  </thead>
                  <tbody>
                    {#each moviesUnmatched as file}
                      <tr class="border-b last:border-0 hover:bg-muted/30">
                        <td class="py-2 pr-4 font-mono text-xs text-muted-foreground" title={file.path}>
                          {truncatePath(file.path)}
                        </td>
                        <td class="py-2 pr-4 whitespace-nowrap">{formatSize(file.size)}</td>
                        <td class="py-2 whitespace-nowrap text-muted-foreground">{formatDate(file.mod_time)}</td>
                      </tr>
                    {/each}
                  </tbody>
                </table>
              </div>
            {/if}
          </div>
        {/if}
      </CardContent>
    </Card>

    <!-- TV Shows Section -->
    <Card>
      <CardHeader>
        <CardTitle>
          TV Shows
          <span class="ml-2 text-base font-normal text-muted-foreground">
            {tvMatched.length} matched, {tvUnmatched.length} unmatched
          </span>
        </CardTitle>
      </CardHeader>
      <CardContent>
        {#if tvMatched.length === 0}
          <p class="text-muted-foreground text-sm">No matched TV show files found.</p>
        {:else}
          <div class="overflow-x-auto">
            <table class="w-full text-sm">
              <thead>
                <tr class="border-b text-left text-muted-foreground">
                  <th class="pb-2 pr-4 font-medium">Title</th>
                  <th class="pb-2 pr-4 font-medium">File Path</th>
                  <th class="pb-2 pr-4 font-medium">Size</th>
                  <th class="pb-2 font-medium">Last Modified</th>
                </tr>
              </thead>
              <tbody>
                {#each tvMatched as file}
                  <tr class="border-b last:border-0 hover:bg-muted/30">
                    <td class="py-2 pr-4 font-medium">{file.title ?? "—"}</td>
                    <td class="py-2 pr-4 font-mono text-xs text-muted-foreground" title={file.path}>
                      {truncatePath(file.path)}
                    </td>
                    <td class="py-2 pr-4 whitespace-nowrap">{formatSize(file.size)}</td>
                    <td class="py-2 whitespace-nowrap text-muted-foreground">{formatDate(file.mod_time)}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        {/if}

        {#if tvUnmatched.length > 0}
          <div class="mt-4">
            <button
              class="text-sm text-primary hover:underline flex items-center gap-1"
              onclick={() => (showTVUnmatched = !showTVUnmatched)}
            >
              <span>{showTVUnmatched ? "▾" : "▸"}</span>
              {tvUnmatched.length} unmatched file{tvUnmatched.length !== 1 ? "s" : ""}
            </button>
            {#if showTVUnmatched}
              <div class="mt-2 overflow-x-auto">
                <table class="w-full text-sm">
                  <thead>
                    <tr class="border-b text-left text-muted-foreground">
                      <th class="pb-2 pr-4 font-medium">File Path</th>
                      <th class="pb-2 pr-4 font-medium">Size</th>
                      <th class="pb-2 font-medium">Last Modified</th>
                    </tr>
                  </thead>
                  <tbody>
                    {#each tvUnmatched as file}
                      <tr class="border-b last:border-0 hover:bg-muted/30">
                        <td class="py-2 pr-4 font-mono text-xs text-muted-foreground" title={file.path}>
                          {truncatePath(file.path)}
                        </td>
                        <td class="py-2 pr-4 whitespace-nowrap">{formatSize(file.size)}</td>
                        <td class="py-2 whitespace-nowrap text-muted-foreground">{formatDate(file.mod_time)}</td>
                      </tr>
                    {/each}
                  </tbody>
                </table>
              </div>
            {/if}
          </div>
        {/if}
      </CardContent>
    </Card>
  {/if}
</div>
