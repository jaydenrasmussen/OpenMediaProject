<script lang="ts">
  import { onMount } from "svelte";
  import {
    Card,
    CardContent,
    CardHeader,
    CardTitle,
  } from "$lib/components/ui/card";
  import { Button } from "$lib/components/ui/button";
  import { indexerApi, type IndexerHealth } from "$lib/api/index";

  let loading = true;
  let error = "";
  let indexers: IndexerHealth[] = [];

  function fmt(n: number, decimals = 1): string {
    return n.toFixed(decimals);
  }

  function formatDate(iso: string): string {
    if (!iso || iso.startsWith("0001")) return "Never";
    try {
      return new Date(iso).toLocaleString();
    } catch {
      return iso;
    }
  }

  function errorRateClass(rate: number): string {
    if (rate >= 0.5) return "text-red-500 font-semibold";
    if (rate >= 0.2) return "text-yellow-600";
    return "text-green-600";
  }

  async function load() {
    loading = true;
    error = "";
    const res = await indexerApi.getHealth();
    if (res.error || !res.data) {
      error = res.message || "Failed to load indexer health";
    } else {
      indexers = res.data.indexers ?? [];
    }
    loading = false;
  }

  onMount(load);
</script>

<svelte:head>
  <title>Indexer Health — OpenMedia</title>
</svelte:head>

<div class="space-y-8">
  <div class="flex items-center justify-between">
    <div>
      <h1 class="text-3xl font-bold">Indexer Health</h1>
      <p class="text-sm text-muted-foreground mt-1">
        Runtime statistics for all registered built-in indexers.
      </p>
    </div>
    <Button on:click={load} disabled={loading} variant="outline">
      {loading ? "Loading…" : "Refresh"}
    </Button>
  </div>

  {#if loading}
    <div class="text-center py-20 text-muted-foreground">Loading…</div>
  {:else if error}
    <div class="text-center py-20 text-red-500">{error}</div>
  {:else if indexers.length === 0}
    <div class="text-center py-20 text-muted-foreground">No indexers registered.</div>
  {:else}
    <Card>
      <CardHeader>
        <CardTitle>
          Indexers
          <span class="ml-2 text-base font-normal text-muted-foreground">
            {indexers.length} registered
          </span>
        </CardTitle>
      </CardHeader>
      <CardContent>
        <div class="overflow-x-auto">
          <table class="w-full text-sm">
            <thead>
              <tr class="border-b text-left text-muted-foreground">
                <th class="pb-2 pr-4 font-medium">Indexer</th>
                <th class="pb-2 pr-4 font-medium">Status</th>
                <th class="pb-2 pr-4 font-medium">Last Success</th>
                <th class="pb-2 pr-4 font-medium">Searches</th>
                <th class="pb-2 pr-4 font-medium">Errors</th>
                <th class="pb-2 pr-4 font-medium">Error Rate</th>
                <th class="pb-2 pr-4 font-medium">Avg Response</th>
                <th class="pb-2 font-medium">Last Results</th>
              </tr>
            </thead>
            <tbody>
              {#each indexers as idx}
                <tr class="border-b last:border-0 hover:bg-muted/30">
                  <td class="py-2 pr-4 font-medium">{idx.name}</td>
                  <td class="py-2 pr-4">
                    {#if idx.enabled}
                      <span class="text-green-600 font-medium">Enabled</span>
                    {:else}
                      <span class="text-muted-foreground">Disabled</span>
                    {/if}
                  </td>
                  <td class="py-2 pr-4 whitespace-nowrap text-muted-foreground text-xs">
                    {formatDate(idx.last_success)}
                  </td>
                  <td class="py-2 pr-4 text-right">{idx.total_searches}</td>
                  <td class="py-2 pr-4 text-right">{idx.total_errors}</td>
                  <td class="py-2 pr-4 text-right {errorRateClass(idx.error_rate)}">
                    {fmt(idx.error_rate * 100, 0)}%
                  </td>
                  <td class="py-2 pr-4 text-right whitespace-nowrap">
                    {idx.total_searches > 0 ? fmt(idx.avg_response_ms, 0) + " ms" : "—"}
                  </td>
                  <td class="py-2 text-right">{idx.total_searches > 0 ? idx.last_result_count : "—"}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      </CardContent>
    </Card>
  {/if}
</div>
