<script lang="ts">
  import { onMount } from "svelte";
  import { page } from "$app/stores";
  import { goto } from "$app/navigation";
  import { movieApi, type Movie } from "$lib/api/index";
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
    loading = true;
    error = "";
    const result = await movieApi.getMovie(id);
    if (result.error) {
      error = result.message || "Failed to load movie";
    } else {
      movie = result.data ?? null;
    }
    loading = false;
  });

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
    if (!minutes) return "";
    const h = Math.floor(minutes / 60);
    const m = minutes % 60;
    return h > 0 ? `${h}h ${m}m` : `${m}m`;
  }

  function formatYear(dateStr: string): string {
    if (!dateStr) return "";
    return dateStr.slice(0, 4);
  }

  function metacriticClass(score: string): string {
    const n = parseInt(score, 10);
    if (isNaN(n)) return "bg-gray-200 text-gray-700";
    if (n >= 61) return "bg-green-600 text-white";
    if (n >= 40) return "bg-yellow-500 text-white";
    return "bg-red-600 text-white";
  }

  function rtClass(score: string): string {
    const n = parseInt(score, 10);
    if (isNaN(n)) return "bg-gray-200 text-gray-700";
    return n >= 60 ? "bg-red-600 text-white" : "bg-green-700 text-white";
  }

  function hideBrokenImage(e: Event) {
    const el = e.currentTarget as HTMLElement;
    if (el) el.style.display = "none";
  }
</script>

<svelte:head>
  <title>{movie ? movie.title : "Movie"} — OpenMedia Project</title>
</svelte:head>

{#if loading}
  <div class="flex items-center justify-center py-24">
    <div class="text-muted-foreground text-lg animate-pulse">Loading…</div>
  </div>
{:else if error}
  <div class="max-w-2xl mx-auto py-12 text-center">
    <p class="text-red-600 mb-4">{error}</p>
    <Button on:click={() => goto("/movies")}>Back to Movies</Button>
  </div>
{:else if movie}
  <div class="relative">
    <!-- Backdrop: absolute, sits behind page content -->
    {#if movie.backdrop_url && !backdropError}
      <div class="absolute inset-x-0 top-0 h-[32rem] overflow-hidden pointer-events-none">
        <img
          src={movie.backdrop_url}
          alt="{movie.title} backdrop"
          class="w-full h-full object-cover"
          on:error={() => (backdropError = true)}
        />
        <div class="absolute inset-0 bg-gradient-to-b from-transparent via-background/60 to-background" />
      </div>
    {/if}

    <div class="relative max-w-5xl mx-auto space-y-10 {movie.backdrop_url && !backdropError ? 'pt-48 md:pt-64' : ''}">
    <Button size="sm" on:click={() => goto("/movies")}>← Back to Movies</Button>

    <!-- Main detail block -->
    <div class="flex flex-col md:flex-row gap-8">
      <!-- Poster -->
      <div class="flex-shrink-0 w-full md:w-52">
        {#if movie.poster_url && !posterError}
          <img
            src={movie.poster_url}
            alt={movie.title}
            class="w-full h-auto rounded-lg shadow-lg"
            on:error={() => (posterError = true)}
          />
        {:else}
          <div class="w-full aspect-[2/3] bg-muted rounded-lg flex items-center justify-center">
            <span class="text-muted-foreground text-4xl">🎬</span>
          </div>
        {/if}
      </div>

      <!-- Details -->
      <div class="flex-1 space-y-4">
        <!-- Title + meta badges -->
        <div>
          <h1 class="text-3xl md:text-4xl font-bold leading-tight">{movie.title}</h1>
          <div class="flex flex-wrap items-center gap-2 mt-2">
            {#if movie.release_date}
              <span class="text-muted-foreground text-sm">{formatYear(movie.release_date)}</span>
            {/if}
            {#if movie.runtime}
              <span class="text-muted-foreground text-sm">· {formatRuntime(movie.runtime)}</span>
            {/if}
            {#if movie.content_rating}
              <span class="text-xs font-semibold px-2 py-0.5 rounded border border-muted-foreground text-muted-foreground">
                {movie.content_rating}
              </span>
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

        <!-- Ratings row -->
        <div class="flex flex-wrap items-center gap-3">
          {#if movie.rating && movie.rating > 0}
            <div class="flex items-center gap-1.5">
              <span class="text-xs font-semibold text-muted-foreground uppercase tracking-wide">TMDB</span>
              <span class="text-sm font-bold text-blue-600">{movie.rating.toFixed(1)}</span>
            </div>
          {/if}
          {#if movie.imdb_rating && movie.imdb_rating !== "N/A"}
            <div class="flex items-center gap-1.5">
              <span class="text-xs font-semibold px-1.5 py-0.5 rounded bg-yellow-400 text-black font-bold">IMDb</span>
              <span class="text-sm font-bold">{movie.imdb_rating}</span>
              {#if movie.imdb_id}
                <a
                  href="https://www.imdb.com/title/{movie.imdb_id}/"
                  target="_blank"
                  rel="noopener noreferrer"
                  class="text-xs text-muted-foreground hover:underline"
                >↗</a>
              {/if}
            </div>
          {:else if movie.imdb_id}
            <a
              href="https://www.imdb.com/title/{movie.imdb_id}/"
              target="_blank"
              rel="noopener noreferrer"
              class="text-xs px-1.5 py-0.5 rounded bg-yellow-400 text-black font-bold hover:bg-yellow-300"
            >IMDb ↗</a>
          {/if}
          {#if movie.rotten_tomatoes && movie.rotten_tomatoes !== "N/A"}
            <div class="flex items-center gap-1.5">
              {#if movie.rt_url}
                <a href={movie.rt_url} target="_blank" rel="noopener noreferrer"
                  class="text-xs font-semibold px-1.5 py-0.5 rounded {rtClass(movie.rotten_tomatoes)} hover:opacity-80">🍅 RT</a>
              {:else}
                <span class="text-xs font-semibold px-1.5 py-0.5 rounded {rtClass(movie.rotten_tomatoes)}">🍅 RT</span>
              {/if}
              <span class="text-sm font-bold">{movie.rotten_tomatoes}</span>
              {#if movie.rt_audience_score && movie.rt_audience_score !== "N/A"}
                <span class="text-xs text-muted-foreground">/ 🍿 {movie.rt_audience_score}</span>
              {/if}
            </div>
          {/if}
          {#if movie.metacritic && movie.metacritic !== "N/A"}
            <div class="flex items-center gap-1.5">
              <span class="text-xs font-bold px-1.5 py-0.5 rounded {metacriticClass(movie.metacritic)}">{movie.metacritic}</span>
              <span class="text-xs text-muted-foreground">Metacritic</span>
            </div>
          {/if}
        </div>

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
        <div class="pt-2">
          <Button
            on:click={requestDownload}
            disabled={requestStatus === "loading" || requestStatus === "success"}
            class="min-w-40"
          >
            {#if requestStatus === "loading"}Requesting…
            {:else if requestStatus === "success"}✓ Requested!
            {:else}Request Download{/if}
          </Button>
          {#if requestMessage}
            <p class="mt-2 text-sm {requestStatus === 'error' ? 'text-red-600' : 'text-green-600'}">{requestMessage}</p>
          {/if}
        </div>
      </div>
    </div>

    <!-- Cast -->
    {#if movie.cast && movie.cast.length > 0}
      <div>
        <h2 class="text-xl font-semibold mb-3">Cast</h2>
        <div class="flex flex-wrap gap-4">
          {#each movie.cast as member}
            <div class="w-24 text-center">
              {#if member.image}
                <img
                  src={member.image}
                  alt={member.name}
                  class="w-24 h-24 rounded-full object-cover mx-auto mb-1 bg-muted"
                  on:error={hideBrokenImage}
                />
              {:else}
                <div class="w-24 h-24 rounded-full bg-muted flex items-center justify-center mx-auto mb-1">
                  <span class="text-2xl text-muted-foreground">👤</span>
                </div>
              {/if}
              <p class="text-xs font-semibold leading-tight">{member.name}</p>
              {#if member.character}
                <p class="text-xs text-muted-foreground leading-tight">{member.character}</p>
              {/if}
            </div>
          {/each}
        </div>
      </div>
    {/if}
  </div>
  </div>
{/if}
