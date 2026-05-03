<script lang="ts">
  import { page } from "$app/stores";
  import { goto } from "$app/navigation";
  import { movieApi, tvApi } from "$lib/api/index";
  import type { Movie, TVShow } from "$lib/api/index";
  import { Card, CardContent } from "$lib/components/ui/card";
  import { BlurFade } from "$lib/components/magic/blur-fade";

  let query = $state("");
  let searchInput = $state("");
  let movies: Movie[] = $state([]);
  let tvshows: TVShow[] = $state([]);
  let loading = $state(false);
  let error = $state("");

  async function runSearch(q: string) {
    if (!q.trim()) return;
    loading = true;
    error = "";
    movies = [];
    tvshows = [];
    const [movieRes, tvRes] = await Promise.all([
      movieApi.searchMovies(q),
      tvApi.searchTVShows(q),
    ]);
    if (movieRes.error) {
      error = movieRes.message || "Failed to search movies";
    } else {
      movies = movieRes.data?.movies ?? [];
    }
    if (tvRes.error) {
      error =
        (error ? error + "; " : "") +
        (tvRes.message || "Failed to search TV shows");
    } else {
      tvshows = tvRes.data?.tvshows ?? [];
    }
    loading = false;
  }

  function handleSubmit(e: Event) {
    e.preventDefault();
    if (searchInput.trim()) {
      goto(`/search?q=${encodeURIComponent(searchInput.trim())}`);
    }
  }

  function getYear(dateStr: string | undefined) {
    if (!dateStr) return "";
    return dateStr.slice(0, 4);
  }

  function handleImgError(e: Event) {
    const img = e.currentTarget as HTMLImageElement;
    img.style.display = "none";
    const placeholder = img.nextElementSibling as HTMLElement;
    if (placeholder) placeholder.style.display = "flex";
  }

  $effect(() => {
    const newQuery = $page.url.searchParams.get("q") ?? "";
    if (newQuery !== query) {
      query = newQuery;
      searchInput = newQuery;
      if (newQuery) runSearch(newQuery);
    }
  });
</script>

<svelte:head>
  <title>{query ? `Search: ${query}` : "Search"} — OpenMediaProject</title>
</svelte:head>

<!-- Break out of layout padding so we can control centering ourselves -->
<div class="-mx-4 -mt-4 md:-mx-8 md:-mt-8">
  <div
    class="flex flex-col items-center px-4 {query
      ? 'pt-10 pb-6 border-b border-border'
      : 'min-h-[75vh] justify-center gap-0'}"
  >
    <h1
      class="font-bold tracking-tight {query
        ? 'text-2xl mb-4'
        : 'text-5xl mb-8'}"
    >
      OpenMediaProject
    </h1>

    <form onsubmit={handleSubmit} class="w-full max-w-2xl">
      <input
        type="text"
        bind:value={searchInput}
        placeholder="Search movies and TV shows…"
        autofocus={!query}
        class="w-full h-14 rounded-full border border-input bg-background px-6 text-lg shadow-sm placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
      />
    </form>

    {#if !query}
      <p class="mt-4 text-sm text-muted-foreground">Press Enter to search</p>
    {/if}
  </div>

  <!-- Results -->
  {#if query}
    <div class="px-6 py-6 space-y-8">
      {#if loading}
        <div
          class="flex items-center justify-center py-20 text-muted-foreground"
        >
          <span class="text-lg">Searching for "{query}"…</span>
        </div>
      {:else}
        <BlurFade>
          {#if error}
            <div class="bg-red-100 text-red-800 px-4 py-3 rounded">{error}</div>
          {/if}

          <!-- Movies -->
          <section>
            <h2 class="text-xl font-semibold mb-4">
              Movies
              <span class="text-sm font-normal text-muted-foreground ml-2"
                >({movies.length} results)</span
              >
            </h2>
            {#if movies.length === 0}
              <p class="text-muted-foreground">
                No movies found for "{query}".
              </p>
            {:else}
              <div
                class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 gap-4"
              >
                {#each movies as movie}
                  <a href="/movies/{movie.id}" class="group">
                    <Card
                      class="overflow-hidden hover:ring-2 hover:ring-primary transition-all h-full"
                    >
                      <div class="relative aspect-[2/3] bg-muted">
                        {#if movie.poster_url}
                          <img
                            src={movie.poster_url}
                            alt={movie.title}
                            class="w-full h-full object-cover"
                            onerror={handleImgError}
                          />
                        {/if}
                        <div
                          class="w-full h-full bg-muted items-center justify-center text-muted-foreground text-sm"
                          style="display: {movie.poster_url ? 'none' : 'flex'}"
                        >
                          No Image
                        </div>
                      </div>
                      <CardContent class="p-2">
                        <p
                          class="font-medium text-sm leading-tight line-clamp-2 group-hover:text-primary transition-colors"
                        >
                          {movie.title}
                        </p>
                        <div
                          class="flex items-center justify-between mt-1 text-xs text-muted-foreground"
                        >
                          <span>{getYear(movie.release_date)}</span>
                          {#if movie.rating}
                            <span>⭐ {movie.rating.toFixed(1)}</span>
                          {/if}
                        </div>
                      </CardContent>
                    </Card>
                  </a>
                {/each}
              </div>
            {/if}
          </section>

          <!-- TV Shows -->
          <section>
            <h2 class="text-xl font-semibold mb-4">
              TV Shows
              <span class="text-sm font-normal text-muted-foreground ml-2"
                >({tvshows.length} results)</span
              >
            </h2>
            {#if tvshows.length === 0}
              <p class="text-muted-foreground">
                No TV shows found for "{query}".
              </p>
            {:else}
              <div
                class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 gap-4"
              >
                {#each tvshows as show}
                  <a href="/tvshows/{show.id}" class="group">
                    <Card
                      class="overflow-hidden hover:ring-2 hover:ring-primary transition-all h-full"
                    >
                      <div class="relative aspect-[2/3] bg-muted">
                        {#if show.poster_url}
                          <img
                            src={show.poster_url}
                            alt={show.name}
                            class="w-full h-full object-cover"
                            onerror={handleImgError}
                          />
                        {/if}
                        <div
                          class="w-full h-full bg-muted items-center justify-center text-muted-foreground text-sm"
                          style="display: {show.poster_url ? 'none' : 'flex'}"
                        >
                          No Image
                        </div>
                      </div>
                      <CardContent class="p-2">
                        <p
                          class="font-medium text-sm leading-tight line-clamp-2 group-hover:text-primary transition-colors"
                        >
                          {show.name}
                        </p>
                        <div
                          class="flex items-center justify-between mt-1 text-xs text-muted-foreground"
                        >
                          <span>{getYear(show.first_aired)}</span>
                          {#if show.rating}
                            <span>⭐ {show.rating.toFixed(1)}</span>
                          {/if}
                        </div>
                      </CardContent>
                    </Card>
                  </a>
                {/each}
              </div>
            {/if}
          </section>
        </BlurFade>
      {/if}
    </div>
  {/if}
</div>
