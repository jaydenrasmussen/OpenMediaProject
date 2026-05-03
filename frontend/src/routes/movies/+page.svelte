<script lang="ts">
  import { onMount } from "svelte";
  import { movieApi, type Movie } from "$lib/api/index";
  import MediaCard from "$lib/components/MediaCard.svelte";
  import { BlurFade } from "$lib/components/magic/blur-fade";

  let allMovies: Movie[] = $state([]);
  let loading = $state(true);
  let error = $state("");
  let query = $state("");

  let filtered = $derived(
    query.trim()
      ? allMovies.filter((m) =>
          m.title.toLowerCase().includes(query.trim().toLowerCase()),
        )
      : allMovies
  );

  let sorted = $derived(
    [...filtered].sort(
      (a, b) => (b.popularity_index ?? 0) - (a.popularity_index ?? 0),
    )
  );

  function getYear(dateStr: string | undefined) {
    if (!dateStr) return "";
    return dateStr.slice(0, 4);
  }

  onMount(async () => {
    loading = true;
    error = "";
    const res = await movieApi.getMovies();
    if (res.error) {
      error = res.message || "Failed to load movies";
    } else {
      allMovies = res.data?.movies ?? [];
    }
    loading = false;
  });
</script>

<svelte:head>
  <title>Movies — OpenMediaProject</title>
</svelte:head>

<div class="space-y-6">
  <div class="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
    <h1 class="text-3xl font-bold">
      Movies
      {#if !loading && allMovies.length > 0}
        <span class="text-lg font-normal text-muted-foreground ml-2">({filtered.length})</span>
      {/if}
    </h1>
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
  {:else if allMovies.length === 0}
    <div class="text-center py-24 space-y-3">
      <p class="text-muted-foreground">No movies in your library yet.</p>
      <a href="/discover" class="text-sm text-primary hover:underline">Browse Discover to add some →</a>
    </div>
  {:else if sorted.length === 0}
    <div class="text-center py-24 text-muted-foreground">
      No movies match "{query}".
    </div>
  {:else}
    <BlurFade>
      <div class="flex flex-wrap gap-4">
        {#each sorted as movie (movie.id)}
          <MediaCard
            id={movie.id}
            title={movie.title}
            year={getYear(movie.release_date)}
            posterUrl={movie.poster_url}
            type="movie"
          />
        {/each}
      </div>
    </BlurFade>
  {/if}
</div>
