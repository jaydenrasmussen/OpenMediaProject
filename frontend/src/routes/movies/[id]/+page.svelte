<script lang="ts">
  import { onMount } from "svelte";
  import { page } from "$app/stores";
  import { goto } from "$app/navigation";
  import { movieApi, type Movie } from "$lib/api/index";
  import {
    Card,
    CardContent,
    CardHeader,
    CardTitle,
  } from "$lib/components/ui/card";
  import { Button } from "$lib/components/ui/button";

  let movie: Movie | null = null;
  let loading = true;
  let error = "";
  let requestStatus: "idle" | "loading" | "success" | "error" = "idle";
  let requestMessage = "";
  let posterError = false;
  let backdropError = false;

  $: id = $page.params.id;

  onMount(async () => {
    await loadMovie();
  });

  async function loadMovie() {
    loading = true;
    error = "";
    const result = await movieApi.getMovie(id);
    if (result.error) {
      error = result.message || "Failed to load movie";
    } else {
      movie = result.data ?? null;
    }
    loading = false;
  }

  async function requestDownload() {
    if (!movie) return;
    requestStatus = "loading";
    requestMessage = "";
    const result = await movieApi.requestMovie(movie.id);
    if (result.error) {
      requestStatus = "error";
      requestMessage = result.message || "Request failed";
    } else {
      requestStatus = "success";
      requestMessage = result.data?.message || "Download requested successfully!";
    }
  }

  function formatRuntime(minutes: number): string {
    if (!minutes) return "Unknown";
    const h = Math.floor(minutes / 60);
    const m = minutes % 60;
    return h > 0 ? `${h}h ${m}m` : `${m}m`;
  }

  function formatYear(dateStr: string): string {
    if (!dateStr) return "Unknown";
    return new Date(dateStr).getFullYear().toString();
  }
</script>

<svelte:head>
  <title>{movie ? movie.title : "Movie"} — OpenMedia Project</title>
</svelte:head>

{#if loading}
  <div class="flex items-center justify-center py-24">
    <p class="text-muted-foreground text-lg">Loading…</p>
  </div>
{:else if error}
  <div class="max-w-2xl mx-auto py-12 text-center space-y-4">
    <p class="text-destructive text-lg">{error}</p>
    <Button variant="outline" on:click={() => goto("/discover")}>← Back to Discover</Button>
  </div>
{:else if movie}
  <!-- Backdrop -->
  {#if movie.backdrop_url && !backdropError}
    <div class="relative -mx-4 -mt-8 mb-8 h-64 md:h-80 overflow-hidden">
      <img
        src={movie.backdrop_url}
        alt="{movie.title} backdrop"
        class="w-full h-full object-cover"
        on:error={() => (backdropError = true)}
      />
      <div class="absolute inset-0 bg-gradient-to-t from-background via-background/60 to-transparent" />
    </div>
  {:else}
    <div class="h-8" />
  {/if}

  <div class="max-w-5xl mx-auto space-y-8">
    <!-- Back link -->
    <a href="/discover" class="inline-flex items-center text-sm text-muted-foreground hover:text-foreground transition-colors">
      ← Back to Discover
    </a>

    <!-- Main detail block -->
    <div class="flex flex-col md:flex-row gap-8">
      <!-- Poster -->
      <div class="flex-shrink-0 w-full md:w-56">
        {#if movie.poster_url && !posterError}
          <img
            src={movie.poster_url}
            alt={movie.title}
            class="w-full h-auto rounded-lg shadow-lg"
            on:error={() => (posterError = true)}
          />
        {:else}
          <div class="w-full h-80 md:h-auto aspect-[2/3] bg-muted rounded-lg flex items-center justify-center">
            <span class="text-muted-foreground text-4xl">🎬</span>
          </div>
        {/if}
      </div>

      <!-- Details -->
      <div class="flex-1 space-y-5">
        <div>
          <h1 class="text-3xl md:text-4xl font-bold leading-tight">{movie.title}</h1>
          <div class="flex flex-wrap items-center gap-3 mt-2 text-muted-foreground text-sm">
            <span>{formatYear(movie.release_date)}</span>
            {#if movie.runtime}
              <span>·</span>
              <span>{formatRuntime(movie.runtime)}</span>
            {/if}
            {#if movie.rating}
              <span>·</span>
              <span class="flex items-center gap-1">⭐ {movie.rating.toFixed(1)}</span>
            {/if}
          </div>
        </div>

        <!-- Genres -->
        {#if movie.genres && movie.genres.length > 0}
          <div class="flex flex-wrap gap-2">
            {#each movie.genres as genre}
              <span class="bg-secondary text-secondary-foreground text-xs px-3 py-1 rounded-full font-medium">
                {genre}
              </span>
            {/each}
          </div>
        {/if}

        <!-- Director -->
        {#if movie.director}
          <div class="text-sm">
            <span class="text-muted-foreground">Director: </span>
            <span class="font-medium">{movie.director}</span>
          </div>
        {/if}

        <!-- Overview -->
        {#if movie.overview}
          <p class="text-muted-foreground leading-relaxed">{movie.overview}</p>
        {/if}

        <!-- Request button -->
        <div class="pt-2 space-y-3">
          <Button
            on:click={requestDownload}
            disabled={requestStatus === "loading" || requestStatus === "success"}
            size="lg"
          >
            {#if requestStatus === "loading"}
              Requesting…
            {:else if requestStatus === "success"}
              ✓ Requested!
            {:else}
              Request Download
            {/if}
          </Button>

          {#if requestMessage}
            <p class={requestStatus === "error" ? "text-destructive text-sm" : "text-green-600 text-sm"}>
              {requestMessage}
            </p>
          {/if}
        </div>
      </div>
    </div>

    <!-- Additional info card -->
    {#if movie.vote_count || movie.popularity_index}
      <Card>
        <CardHeader>
          <CardTitle class="text-base">More Info</CardTitle>
        </CardHeader>
        <CardContent>
          <dl class="grid grid-cols-2 md:grid-cols-3 gap-4 text-sm">
            {#if movie.vote_count}
              <div>
                <dt class="text-muted-foreground">Vote Count</dt>
                <dd class="font-medium">{movie.vote_count.toLocaleString()}</dd>
              </div>
            {/if}
            {#if movie.popularity_index}
              <div>
                <dt class="text-muted-foreground">Popularity</dt>
                <dd class="font-medium">{movie.popularity_index.toFixed(1)}</dd>
              </div>
            {/if}
            {#if movie.release_date}
              <div>
                <dt class="text-muted-foreground">Release Date</dt>
                <dd class="font-medium">{movie.release_date}</dd>
              </div>
            {/if}
          </dl>
        </CardContent>
      </Card>
    {/if}
  </div>
{/if}
