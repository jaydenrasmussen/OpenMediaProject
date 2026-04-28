<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { Button } from "$lib/components/ui/button";
  import { movieApi, tvApi } from "$lib/api/index";
  import type { Movie, TVShow } from "$lib/api/index";
  import MediaCard from "$lib/components/MediaCard.svelte";

  let trendingMovies: Movie[] = [];
  let popularMovies: Movie[] = [];
  let popularTV: TVShow[] = [];
  let hero: Movie | null = null;
  let loading = true;
  let searchQuery = "";

  function getYear(dateStr: string | undefined) {
    if (!dateStr) return "";
    return dateStr.slice(0, 4);
  }

  async function loadData() {
    loading = true;

    // Step 1: fetch now-playing → snapshot as "Trending"
    await movieApi.fetchNowPlaying();
    const snap1 = await movieApi.getMovies();
    const trendingIds = new Set<string>();
    if (!snap1.error && snap1.data) {
      const sorted = [...(snap1.data.movies ?? [])].sort(
        (a, b) => b.popularity_index - a.popularity_index,
      );
      trendingMovies = sorted.slice(0, 20);
      trendingMovies.forEach((m) => trendingIds.add(m.id));
    }

    // Step 2: fetch popular → subtract trending IDs to get "Popular"
    await movieApi.fetchPopular();
    const snap2 = await movieApi.getMovies();
    if (!snap2.error && snap2.data) {
      popularMovies = (snap2.data.movies ?? [])
        .filter((m) => !trendingIds.has(m.id))
        .sort((a, b) => b.popularity_index - a.popularity_index)
        .slice(0, 20);
    }

    // Step 3: fetch popular TV → get all TV shows
    await tvApi.fetchPopularTV();
    const tvSnap = await tvApi.getTVShows();
    if (!tvSnap.error && tvSnap.data) {
      popularTV = (tvSnap.data.tvshows ?? []).slice(0, 20);
    }

    // Hero: highest-popularity trending movie (already sorted desc)
    hero = trendingMovies[0] ?? popularMovies[0] ?? null;

    loading = false;
  }

  function handleSearch(e: Event) {
    e.preventDefault();
    if (searchQuery.trim()) {
      goto(`/search?q=${encodeURIComponent(searchQuery.trim())}`);
    }
  }

  onMount(loadData);
</script>

<svelte:head>
  <title>Discover -- OpenMedia</title>
</svelte:head>

<!-- Break out of container padding for full-width layout -->
<div class="-mx-4 -mt-8">
  <!-- Search bar -->
  <div class="px-6 pt-6 pb-4">
    <form on:submit={handleSearch} class="flex gap-2 max-w-lg">
      <input
        type="text"
        bind:value={searchQuery}
        placeholder="Search movies and TV shows..."
        class="flex h-10 flex-1 rounded-md border border-input bg-background px-3 py-2 text-sm placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
      />
      <Button type="submit">Search</Button>
    </form>
  </div>

  {#if loading}
    <div class="flex items-center justify-center py-32 text-muted-foreground">
      Loading content…
    </div>
  {:else if trendingMovies.length === 0 && popularMovies.length === 0 && popularTV.length === 0}
    <!-- Empty state -->
    <div class="flex flex-col items-center justify-center py-32 gap-4">
      <p class="text-muted-foreground">No content found.</p>
      <Button on:click={loadData}>Refresh Content</Button>
    </div>
  {:else}
    <!-- Hero -->
    {#if hero}
      {@const h = hero}
      <div class="relative w-full h-64 md:h-80 overflow-hidden bg-black mb-8">
        {#if h.backdrop_url}
          <img
            src={h.backdrop_url}
            alt={h.title}
            class="w-full h-full object-cover opacity-60"
          />
        {/if}
        <div
          class="absolute inset-0 bg-gradient-to-t from-background via-background/40 to-transparent"
        />
        <div class="absolute bottom-0 left-0 px-6 pb-6">
          <h2 class="text-3xl font-bold text-white drop-shadow">{h.title}</h2>
          <div class="flex items-center gap-3 mt-1 text-sm text-white/80">
            <span>{getYear(h.release_date)}</span>
            {#if h.rating}
              <span>&#11088; {h.rating.toFixed(1)}</span>
            {/if}
          </div>
          <Button class="mt-3" on:click={() => goto(`/movies/${h.id}`)}>
            View Details
          </Button>
        </div>
      </div>
    {/if}

    <!-- Content rows -->
    <div class="space-y-8 px-6 pb-8">
      {#if trendingMovies.length > 0}
        <section>
          <h2 class="text-xl font-semibold mb-3">Trending Movies</h2>
          <div class="flex gap-3 overflow-x-auto pb-2">
            {#each trendingMovies as movie (movie.id)}
              <MediaCard
                id={movie.id}
                title={movie.title}
                year={getYear(movie.release_date)}
                posterUrl={movie.poster_url}
                type="movie"
              />
            {/each}
          </div>
        </section>
      {/if}

      {#if popularMovies.length > 0}
        <section>
          <h2 class="text-xl font-semibold mb-3">Popular Movies</h2>
          <div class="flex gap-3 overflow-x-auto pb-2">
            {#each popularMovies as movie (movie.id)}
              <MediaCard
                id={movie.id}
                title={movie.title}
                year={getYear(movie.release_date)}
                posterUrl={movie.poster_url}
                type="movie"
              />
            {/each}
          </div>
        </section>
      {/if}

      {#if popularTV.length > 0}
        <section>
          <h2 class="text-xl font-semibold mb-3">Popular TV Shows</h2>
          <div class="flex gap-3 overflow-x-auto pb-2">
            {#each popularTV as show (show.id)}
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
    </div>
  {/if}
</div>
