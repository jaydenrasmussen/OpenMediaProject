<script lang="ts">
  import { onMount } from "svelte";
  import {
    Card,
    CardContent,
    CardHeader,
    CardTitle,
  } from "$lib/components/ui/card";
  import { Button } from "$lib/components/ui/button";
  import { historyApi, type HistoryEntry } from "$lib/api/index";

  let loading = $state(true);
  let error = $state("");
  let entries: HistoryEntry[] = $state([]);

  const EVENT_LABELS: Record<string, string> = {
    torrent_added: "Added",
    download_complete: "Complete",
    download_failed: "Failed",
    upgrade_found: "Upgrade",
    search_started: "Search",
  };

  const EVENT_CLASSES: Record<string, string> = {
    torrent_added: "bg-blue-100 text-blue-800",
    download_complete: "bg-green-100 text-green-800",
    download_failed: "bg-red-100 text-red-800",
    upgrade_found: "bg-purple-100 text-purple-800",
    search_started: "bg-gray-100 text-gray-700",
  };

  function eventLabel(type: string): string {
    return EVENT_LABELS[type] ?? type;
  }

  function eventClass(type: string): string {
    return EVENT_CLASSES[type] ?? "bg-gray-100 text-gray-700";
  }

  function formatDate(iso: string): string {
    try {
      return new Date(iso).toLocaleString();
    } catch {
      return iso;
    }
  }

  async function load() {
    loading = true;
    error = "";
    const res = await historyApi.getHistory();
    if (res.error || !res.data) {
      error = res.message || "Failed to load history";
    } else {
      entries = res.data.entries ?? [];
    }
    loading = false;
  }

  onMount(load);
</script>

<svelte:head>
  <title>Activity History — OpenMediaProject</title>
</svelte:head>

<div class="space-y-8">
  <div class="flex items-center justify-between">
    <div>
      <h1 class="text-3xl font-bold">Activity History</h1>
      <p class="text-sm text-muted-foreground mt-1">
        Record of all download, search, and upgrade events.
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
  {:else if entries.length === 0}
    <div class="text-center py-20 text-muted-foreground">No history yet.</div>
  {:else}
    <Card>
      <CardHeader>
        <CardTitle>
          Events
          <span class="ml-2 text-base font-normal text-muted-foreground">
            {entries.length} recorded
          </span>
        </CardTitle>
      </CardHeader>
      <CardContent>
        <div class="overflow-x-auto">
          <table class="w-full text-sm">
            <thead>
              <tr class="border-b text-left text-muted-foreground">
                <th class="pb-2 pr-4 font-medium">Time</th>
                <th class="pb-2 pr-4 font-medium">Event</th>
                <th class="pb-2 pr-4 font-medium">Title</th>
                <th class="pb-2 pr-4 font-medium">Type</th>
                <th class="pb-2 pr-4 font-medium">User</th>
                <th class="pb-2 font-medium">Message</th>
              </tr>
            </thead>
            <tbody>
              {#each entries as e}
                <tr class="border-b last:border-0 hover:bg-muted/30">
                  <td class="py-2 pr-4 whitespace-nowrap text-xs text-muted-foreground">
                    {formatDate(e.created_at)}
                  </td>
                  <td class="py-2 pr-4">
                    <span
                      class="inline-block rounded px-2 py-0.5 text-xs font-medium {eventClass(e.event_type)}"
                    >
                      {eventLabel(e.event_type)}
                    </span>
                  </td>
                  <td class="py-2 pr-4 font-medium">{e.title}</td>
                  <td class="py-2 pr-4 text-muted-foreground capitalize">{e.media_type || "—"}</td>
                  <td class="py-2 pr-4 text-muted-foreground">{e.user || "—"}</td>
                  <td class="py-2 text-muted-foreground text-xs">{e.message || ""}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      </CardContent>
    </Card>
  {/if}
</div>
