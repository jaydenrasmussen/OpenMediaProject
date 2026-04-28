<script lang="ts">
  import { onMount } from "svelte";
  import { page } from "$app/stores";
  import { goto } from "$app/navigation";
  import { movieApi, tvApi } from "$lib/api/index";
  import type { Movie, TVShow } from "$lib/api/index";
  import {
    Card,
    CardContent,
  } from "$lib/components/ui/card";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";

  let query = "";
  let searchInput = "";
  let movies: Movie[] = [];
  let tvshows: TVShow[] = [];
  let loading = false;
  let error = "";

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
      error = (error ? error + "; " : "") + (tvRes.message || "Failed to search TV shows");
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

  $: {
    query = $page.url.searchParams.get("q") ?? "";
    searchInput = query;
    if (query) runSearch(query);
  }
</script>

<svelte:head>
  <title>{query ? `Search: ${query}` : "Search"} — OpenMedia</title>
</svelte:head>

<div class="space-y-8">
  <div>
    <h1 class="text-3xl font-bold mb-4">Search</h1>
    <form on:submit={handleSubmit} class="flex gap-2 max-w-xl">
      <Input
        bind:value={searchInput}
        placeholder="Search movies and TV shows..."
        class="flex-1"
      />
      <Button type="submit">Search</Button>
    </form>
  </div>

  {#if !query}
    <p class="text-muted-foreground">Enter a search term above to find movies and TV shows.</p>
  {:else if loading}
    <div class="flex items-center justify-center py-20 text-muted-foreground">
      <span class="text-lg">Searching for "{query}"…</span>
    </div>
  {:else}
    {#if error}
      <div class="bg-red-100 text-red-800 px-4 py-3 rounded">{error}</div>
    {/if}

    <!-- Movies section -->
    <section>
      <h2 class="text-2xl font-semibold mb-4">
        Movies
        <span class="text-base font-normal text-muted-foreground ml-2">({movies.length} results)</span>
      </h2>
      {#if movies.length === 0}
        <p class="text-muted-foreground">No movies found for "{query}".</p>
      {:else}
        <div class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 gap-4">
          {#each movies as movie}
            <a href="/movies/{movie.id}" class="group">
              <Card class="overflow-hidden hover:ring-2 hover:ring-primary transition-all h-full">
                <div class="relative aspect-[2/3] bg-muted">
                  {#if movie.poster_url}
                    <img
                      src={movie.poster_url}
                      alt={movie.title}
                      class="w-full h-full object-cover"
                      on:error={handleImgError}
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
                  <p class="font-medium text-sm leading-tight line-clamp-2 group-hover:text-primary transition-colors">
                    {movie.title}
                  </p>
                  <div class="flex items-center justify-between mt-1 text-xs text-muted-foreground">
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

    <!-- TV Shows section -->
    <section>
      <h2 class="text-2xl font-semibold mb-4">
        TV Shows
        <span class="text-base font-normal text-muted-foreground ml-2">({tvshows.length} results)</span>
      </h2>
      {#if tvshows.length === 0}
        <p class="text-muted-foreground">No TV shows found for "{query}".</p>
      {:else}
        <div class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 gap-4">
          {#each tvshows as show}
            <a href="/tvshows/{show.id}" class="group">
              <Card class="overflow-hidden hover:ring-2 hover:ring-primary transition-all h-full">
                <div class="relative aspect-[2/3] bg-muted">
                  {#if show.poster_url}
                    <img
                      src={show.poster_url}
                      alt={show.name}
                      class="w-full h-full object-cover"
                      on:error={handleImgError}
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
                  <p class="font-medium text-sm leading-tight line-clamp-2 group-hover:text-primary transition-colors">
                    {show.name}
                  </p>
                  <div class="flex items-center justify-between mt-1 text-xs text-muted-foreground">
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
  {/if}
</div>
