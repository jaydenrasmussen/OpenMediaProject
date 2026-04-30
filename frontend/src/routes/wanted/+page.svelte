<script lang="ts">
  import { onMount } from "svelte";
  import {
    Card,
    CardContent,
    CardHeader,
    CardTitle,
  } from "$lib/components/ui/card";
  import { Button } from "$lib/components/ui/button";
  import { wantedApi, type WantedItem } from "$lib/api/index";

  let items: WantedItem[] = [];
  let loading = true;
  let error = "";

  // Read the logged-in role so we can show admin-only actions
  let isAdmin = false;

  onMount(async () => {
    if (typeof localStorage !== "undefined") {
      isAdmin = localStorage.getItem("auth_role") === "admin";
    }
    await loadWanted();
  });

  async function loadWanted() {
    loading = true;
    error = "";
    const result = await wantedApi.getWanted();
    if (result.error) {
      error = result.message || "Failed to load wanted list";
    } else {
      items = result.data?.items ?? [];
    }
    loading = false;
  }

  async function handleDelete(id: string) {
    const result = await wantedApi.deleteWanted(id);
    if (result.error) {
      alert(result.message || "Failed to delete item");
    } else {
      items = items.filter((i) => i.id !== id);
    }
  }

  async function handleApprove(id: string) {
    const result = await wantedApi.approveWanted(id);
    if (result.error) {
      alert(result.message || "Failed to approve item");
    } else {
      // Refresh to get updated status
      await loadWanted();
    }
  }

  async function handleReject(id: string) {
    const result = await wantedApi.rejectWanted(id);
    if (result.error) {
      alert(result.message || "Failed to reject item");
    } else {
      await loadWanted();
    }
  }

  function formatDate(dateStr: string) {
    if (!dateStr) return "—";
    try {
      return new Date(dateStr).toLocaleDateString();
    } catch {
      return dateStr;
    }
  }

  function statusClass(status: string) {
    switch (status?.toLowerCase()) {
      case "pending":
        return "bg-orange-100 text-orange-800";
      case "wanted":
        return "bg-blue-100 text-blue-800";
      case "downloading":
        return "bg-yellow-100 text-yellow-800";
      case "available":
      case "downloaded":
        return "bg-green-100 text-green-800";
      case "rejected":
      case "unavailable":
      case "missing":
        return "bg-red-100 text-red-800";
      default:
        return "bg-gray-100 text-gray-700";
    }
  }

  function typeClass(type: string) {
    return type?.toLowerCase() === "movie"
      ? "bg-purple-100 text-purple-800"
      : "bg-cyan-100 text-cyan-800";
  }
</script>

<svelte:head>
  <title>Wanted — OpenMedia</title>
</svelte:head>

<div class="space-y-6">
  <div class="flex items-center justify-between">
    <h1 class="text-3xl font-bold">Wanted</h1>
    <Button variant="outline" on:click={loadWanted} disabled={loading}>
      {loading ? "Refreshing…" : "Refresh"}
    </Button>
  </div>

  {#if error}
    <div class="bg-red-50 border border-red-200 text-red-700 rounded-lg p-4">
      {error}
    </div>
  {/if}

  {#if loading}
    <div class="text-center py-20 text-muted-foreground">Loading…</div>
  {:else if items.length === 0}
    <Card>
      <CardContent class="py-20 text-center text-muted-foreground">
        No items in the wanted list.
      </CardContent>
    </Card>
  {:else}
    <Card>
      <CardHeader>
        <CardTitle>Wanted Items ({items.length})</CardTitle>
      </CardHeader>
      <CardContent class="p-0">
        <div class="overflow-x-auto">
          <table class="w-full text-sm">
            <thead>
              <tr class="border-b bg-muted/50 text-muted-foreground">
                <th class="text-left px-4 py-3 font-medium">Title</th>
                <th class="text-left px-4 py-3 font-medium">Type</th>
                <th class="text-left px-4 py-3 font-medium">Season / Ep</th>
                <th class="text-left px-4 py-3 font-medium">Quality</th>
                <th class="text-left px-4 py-3 font-medium">Status</th>
                <th class="text-left px-4 py-3 font-medium">Added</th>
                <th class="px-4 py-3"></th>
              </tr>
            </thead>
            <tbody>
              {#each items as item (item.id)}
                <tr class="border-b last:border-0 hover:bg-muted/30 transition-colors">
                  <td class="px-4 py-3 font-medium">{item.title}</td>
                  <td class="px-4 py-3">
                    <span class="px-2 py-0.5 rounded-full text-xs font-medium {typeClass(item.media_type)}">
                      {item.media_type === "movie" ? "Movie" : "TV"}
                    </span>
                  </td>
                  <td class="px-4 py-3 text-muted-foreground">
                    {#if item.season != null && item.episode != null}
                      S{String(item.season).padStart(2, "0")}E{String(item.episode).padStart(2, "0")}
                    {:else if item.season != null}
                      Season {item.season}
                    {:else}
                      —
                    {/if}
                  </td>
                  <td class="px-4 py-3 text-muted-foreground">
                    {item.min_quality || "Any"}
                  </td>
                  <td class="px-4 py-3">
                    <span class="px-2 py-0.5 rounded-full text-xs font-medium {statusClass(item.status)}">
                      {item.status}
                    </span>
                  </td>
                  <td class="px-4 py-3 text-muted-foreground whitespace-nowrap">
                    {formatDate(item.added_at)}
                  </td>
                  <td class="px-4 py-3">
                    <div class="flex items-center gap-2 justify-end">
                      {#if isAdmin && item.status === "pending"}
                        <Button
                          variant="outline"
                          size="sm"
                          class="text-green-700 border-green-300 hover:bg-green-50"
                          on:click={() => handleApprove(item.id)}
                        >
                          Approve
                        </Button>
                        <Button
                          variant="outline"
                          size="sm"
                          class="text-red-700 border-red-300 hover:bg-red-50"
                          on:click={() => handleReject(item.id)}
                        >
                          Reject
                        </Button>
                      {/if}
                      <Button
                        variant="destructive"
                        size="sm"
                        on:click={() => handleDelete(item.id)}
                      >
                        Delete
                      </Button>
                    </div>
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
