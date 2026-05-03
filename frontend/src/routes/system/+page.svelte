<script lang="ts">
  import { onMount } from "svelte";
  import {
    Card,
    CardContent,
    CardHeader,
    CardTitle,
  } from "$lib/components/ui/card";
  import { Button } from "$lib/components/ui/button";
  import { systemApi, type TaskInfo } from "$lib/api/index";

  let loading = $state(true);
  let error = $state("");
  let tasks: TaskInfo[] = $state([]);

  function statusClass(status: string): string {
    if (status === "running") return "text-blue-600 font-semibold";
    if (status === "error") return "text-red-600 font-semibold";
    return "text-green-600";
  }

  function formatDate(iso: string | undefined): string {
    if (!iso) return "Never";
    try {
      return new Date(iso).toLocaleString();
    } catch {
      return iso;
    }
  }

  async function load() {
    loading = true;
    error = "";
    const res = await systemApi.getTasks();
    if (res.error || !res.data) {
      error = res.message || "Failed to load task list";
    } else {
      tasks = (res.data.tasks ?? []).sort((a, b) => a.name.localeCompare(b.name));
    }
    loading = false;
  }

  onMount(load);
</script>

<svelte:head>
  <title>System — OpenMediaProject</title>
</svelte:head>

<div class="space-y-8">
  <div class="flex items-center justify-between">
    <div>
      <h1 class="text-3xl font-bold">System</h1>
      <p class="text-sm text-muted-foreground mt-1">
        Background task status and health information.
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
  {:else}
    <Card>
      <CardHeader>
        <CardTitle>
          Background Tasks
          <span class="ml-2 text-base font-normal text-muted-foreground">
            {tasks.length} registered
          </span>
        </CardTitle>
      </CardHeader>
      <CardContent>
        {#if tasks.length === 0}
          <div class="text-center py-10 text-muted-foreground">No tasks registered.</div>
        {:else}
          <div class="overflow-x-auto">
            <table class="w-full text-sm">
              <thead>
                <tr class="border-b text-left text-muted-foreground">
                  <th class="pb-2 pr-4 font-medium">Task</th>
                  <th class="pb-2 pr-4 font-medium">Status</th>
                  <th class="pb-2 pr-4 font-medium">Last Run</th>
                  <th class="pb-2 pr-4 font-medium">Run Count</th>
                  <th class="pb-2 pr-4 font-medium">Schedule</th>
                  <th class="pb-2 font-medium">Last Error</th>
                </tr>
              </thead>
              <tbody>
                {#each tasks as t}
                  <tr class="border-b last:border-0 hover:bg-muted/30">
                    <td class="py-2 pr-4 font-medium">{t.name}</td>
                    <td class="py-2 pr-4">
                      <span class={statusClass(t.status)}>{t.status}</span>
                    </td>
                    <td class="py-2 pr-4 whitespace-nowrap text-xs text-muted-foreground">
                      {formatDate(t.last_run_at)}
                    </td>
                    <td class="py-2 pr-4 text-right">{t.run_count}</td>
                    <td class="py-2 pr-4 text-muted-foreground text-xs">
                      {t.next_run_hint || "—"}
                    </td>
                    <td class="py-2 text-red-600 text-xs">
                      {t.last_error || ""}
                    </td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        {/if}
      </CardContent>
    </Card>
  {/if}
</div>
