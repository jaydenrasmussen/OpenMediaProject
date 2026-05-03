<script lang="ts">
  import { onMount } from "svelte";
  import {
    Card,
    CardContent,
    CardHeader,
    CardTitle,
  } from "$lib/components/ui/card";
  import { Button } from "$lib/components/ui/button";
  import { downloadsApi, type PendingDownload } from "$lib/api/index";

  let loading = $state(true);
  let error = $state("");
  let downloads: PendingDownload[] = $state([]);

  function statusClass(status: string): string {
    switch (status?.toLowerCase()) {
      case "downloading": return "bg-yellow-100 text-yellow-800";
      case "completed":   return "bg-green-100 text-green-800";
      case "failed":      return "bg-red-100 text-red-800";
      default:            return "bg-gray-100 text-gray-700";
    }
  }

  function typeClass(type: string): string {
    return type?.toLowerCase() === "movie"
      ? "bg-purple-100 text-purple-800"
      : "bg-cyan-100 text-cyan-800";
  }

  function formatDate(iso: string | undefined): string {
    if (!iso) return "—";
    try {
      return new Date(iso).toLocaleString();
    } catch {
      return iso;
    }
  }

  async function load() {
    loading = true;
    error = "";
    const res = await downloadsApi.getDownloads();
    if (res.error || !res.data) {
      error = res.message || "Failed to load downloads";
    } else {
      downloads = [...(res.data.downloads ?? [])].sort(
        (a, b) => new Date(b.added_at).getTime() - new Date(a.added_at).getTime(),
      );
    }
    loading = false;
  }

  onMount(load);
</script>

<svelte:head>
  <title>Downloads — OpenMediaProject</title>
</svelte:head>

<div class="space-y-8">
  <div class="flex items-center justify-between">
    <div>
      <h1 class="text-3xl font-bold">Downloads</h1>
      <p class="text-sm text-muted-foreground mt-1">
        Active and completed download records tracked by the system.
      </p>
    </div>
    <Button onclick={load} disabled={loading} variant="outline">
      {loading ? "Loading…" : "Refresh"}
    </Button>
  </div>

  {#if loading}
    <div class="text-center py-20 text-muted-foreground">Loading…</div>
  {:else if error}
    <div class="text-center py-20 text-red-500">{error}</div>
  {:else if downloads.length === 0}
    <div class="text-center py-20 text-muted-foreground">
      No downloads recorded yet.
    </div>
  {:else}
    <Card>
      <CardHeader>
        <CardTitle>
          Download Queue
          <span class="ml-2 text-base font-normal text-muted-foreground">
            {downloads.length} records
          </span>
        </CardTitle>
      </CardHeader>
      <CardContent>
        <div class="overflow-x-auto">
          <table class="w-full text-sm">
            <thead>
              <tr class="border-b text-left text-muted-foreground">
                <th class="pb-2 pr-4 font-medium">Title</th>
                <th class="pb-2 pr-4 font-medium">Type</th>
                <th class="pb-2 pr-4 font-medium">Quality</th>
                <th class="pb-2 pr-4 font-medium">Source</th>
                <th class="pb-2 pr-4 font-medium">Status</th>
                <th class="pb-2 pr-4 font-medium">Added</th>
                <th class="pb-2 font-medium">Hash</th>
              </tr>
            </thead>
            <tbody>
              {#each downloads as dl (dl.hash)}
                <tr class="border-b last:border-0 hover:bg-muted/30">
                  <td class="py-2 pr-4 font-medium">{dl.title}</td>
                  <td class="py-2 pr-4">
                    <span class="px-2 py-0.5 rounded-full text-xs font-medium {typeClass(dl.media_type)}">
                      {dl.media_type === "movie" ? "Movie" : "TV"}
                    </span>
                  </td>
                  <td class="py-2 pr-4 text-muted-foreground">{dl.quality || "—"}</td>
                  <td class="py-2 pr-4 text-muted-foreground">{dl.source || "—"}</td>
                  <td class="py-2 pr-4">
                    <span class="px-2 py-0.5 rounded-full text-xs font-medium {statusClass(dl.status)}">
                      {dl.status}
                    </span>
                  </td>
                  <td class="py-2 pr-4 whitespace-nowrap text-xs text-muted-foreground">
                    {formatDate(dl.added_at)}
                  </td>
                  <td class="py-2 font-mono text-xs text-muted-foreground" title={dl.hash}>
                    {dl.hash ? dl.hash.slice(0, 12) + "…" : "—"}
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      </CardContent>
    </Card>
  {/if}
</div>
