<script lang="ts">
  import { onMount } from "svelte";
  import { tvApi, type TVShow } from "$lib/api/index";
  import MediaCard from "$lib/components/MediaCard.svelte";

  let allShows: TVShow[] = [];
  let loading = true;
  let error = "";
  let query = "";

  $: filtered = query.trim()
    ? allShows.filter((s) =>
        s.name.toLowerCase().includes(query.trim().toLowerCase()),
      )
    : allShows;

  $: monitored = filtered.filter((s) => s.monitored);
  $: unmonitored = filtered.filter((s) => !s.monitored);

  function getYear(dateStr: string | undefined) {
    if (!dateStr) return "";
    return dateStr.slice(0, 4);
  }

  onMount(async () => {
    loading = true;
    error = "";
    const res = await tvApi.getTVShows();
    if (res.error) {
      error = res.message || "Failed to load TV shows";
    } else {
      allShows = res.data?.tvshows ?? [];
    }
    loading = false;
  });
</script>

<svelte:head>
  <title>TV Shows — OpenMedia</title>
</svelte:head>

<div class="space-y-6">
  <div class="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
    <h1 class="text-3xl font-bold">TV Shows</h1>
    <input
      type="text"
      bind:value={query}
      placeholder="Filter by title…"
      class="flex h-9 rounded-md border border-input bg-background px-3 py-1 text-sm placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring w-full sm:w-64"
    />
  </div>

  {#if loading}
    <div class="text-center py-24 text-muted-foreground">Loading…</div>
  {:else if error}
    <div class="bg-red-50 border border-red-200 text-red-700 rounded-lg p-4">{error}</div>
  {:else if allShows.length === 0}
    <div class="text-center py-24 space-y-3">
      <p class="text-muted-foreground">No TV shows in your library yet.</p>
      <a href="/discover" class="text-sm text-primary hover:underline">Browse Discover to add some →</a>
    </div>
  {:else if filtered.length === 0}
    <div class="text-center py-24 text-muted-foreground">
      No shows match "{query}".
    </div>
  {:else}
    {#if monitored.length > 0}
      <section class="space-y-3">
        <h2 class="text-lg font-semibold text-muted-foreground">
          Monitored
          <span class="ml-1 text-sm font-normal">({monitored.length})</span>
        </h2>
        <div class="flex flex-wrap gap-4">
          {#each monitored as show (show.id)}
            <div class="relative">
              <MediaCard
                id={show.id}
                title={show.name}
                year={getYear(show.first_aired)}
                posterUrl={show.poster_url}
                type="tv"
              />
              <span
                class="absolute top-1.5 left-1.5 bg-primary text-primary-foreground text-[10px] font-semibold px-1.5 py-0.5 rounded-sm leading-none"
              >
                Monitored
              </span>
            </div>
          {/each}
        </div>
      </section>
    {/if}

    {#if unmonitored.length > 0}
      <section class="space-y-3">
        <h2 class="text-lg font-semibold text-muted-foreground">
          Unmonitored
          <span class="ml-1 text-sm font-normal">({unmonitored.length})</span>
        </h2>
        <div class="flex flex-wrap gap-4">
          {#each unmonitored as show (show.id)}
            <MediaCard
              id={show.id}
              title={show.name}
              year={getYear(show.first_aired)}
              posterUrl={show.poster_url}
              type="tv"
            />
          {/each}
        </div>
      </section>
    {/if}
  {/if}
</div>
