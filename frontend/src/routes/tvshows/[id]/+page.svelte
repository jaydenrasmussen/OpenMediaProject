<script lang="ts">
  import { onMount } from "svelte";
  import { page } from "$app/stores";
  import { goto } from "$app/navigation";
  import { tvApi } from "$lib/api/index";
  import type { TVShow, Episode } from "$lib/api/index";
  import {
    Card,
    CardContent,
    CardHeader,
    CardTitle,
  } from "$lib/components/ui/card";
  import { Button } from "$lib/components/ui/button";

  let show: TVShow | null = null;
  let episodes: Episode[] = [];
  let loading = true;
  let episodesLoading = true;
  let error = "";
  let requestStatus: "idle" | "loading" | "success" | "error" = "idle";
  let requestMessage = "";
  let monitorStatus: "idle" | "loading" | "success" | "error" = "idle";
  let monitorMessage = "";
  let posterError = false;
  let backdropError = false;

  $: id = $page.params.id as string;

  // Group episodes by season
  $: seasonMap = episodes.reduce<Record<number, Episode[]>>((acc, ep) => {
    if (!acc[ep.season]) acc[ep.season] = [];
    acc[ep.season].push(ep);
    return acc;
  }, {});

  // Seasons in descending order (newest first)
  $: seasons = Object.keys(seasonMap)
    .map(Number)
    .sort((a, b) => b - a);

  // Track which seasons are collapsed; all collapsed except the newest (first in the list)
  let collapsedSeasons: Set<number> = new Set();
  let seasonsInitialized = false;

  $: if (seasons.length > 0 && !seasonsInitialized) {
    seasonsInitialized = true;
    collapsedSeasons = new Set(seasons.slice(1)); // collapse all but the newest
  }

  function toggleSeason(season: number) {
    if (collapsedSeasons.has(season)) {
      collapsedSeasons.delete(season);
    } else {
      collapsedSeasons.add(season);
    }
    collapsedSeasons = collapsedSeasons;
  }

  function formatYear(dateStr?: string) {
    if (!dateStr) return "—";
    const y = new Date(dateStr).getFullYear();
    return isNaN(y) ? "—" : String(y);
  }

  function formatDate(dateStr?: string) {
    if (!dateStr) return "—";
    return new Date(dateStr).toLocaleDateString(undefined, {
      year: "numeric",
      month: "short",
      day: "numeric",
    });
  }

  function episodeStatusClass(status: string) {
    switch (status) {
      case "downloaded": return "bg-green-100 text-green-800";
      case "downloading": return "bg-yellow-100 text-yellow-800";
      case "wanted": return "bg-blue-100 text-blue-800";
      case "skipped": return "bg-gray-100 text-gray-600";
      case "missing": return "bg-red-100 text-red-800";
      default: return "bg-gray-100 text-gray-600";
    }
  }

  function showStatusClass(status: string) {
    switch (status?.toLowerCase()) {
      case "continuing": return "bg-green-100 text-green-800";
      case "ended": return "bg-gray-100 text-gray-600";
      case "upcoming": return "bg-blue-100 text-blue-800";
      default: return "bg-gray-100 text-gray-600";
    }
  }

  // Convert Metacritic score (0-100) to a colour class
  function metacriticClass(score: string) {
    const n = parseInt(score, 10);
    if (isNaN(n)) return "bg-gray-200 text-gray-700";
    if (n >= 61) return "bg-green-600 text-white";
    if (n >= 40) return "bg-yellow-500 text-white";
    return "bg-red-600 text-white";
  }

  // Convert RT score to a colour
  function rtClass(score: string) {
    const n = parseInt(score, 10);
    if (isNaN(n)) return "bg-gray-200 text-gray-700";
    return n >= 60 ? "bg-red-600 text-white" : "bg-green-700 text-white";
  }

  function hideBrokenImage(e: Event) {
    const el = e.currentTarget as HTMLElement;
    if (el) el.style.display = "none";
  }

  async function handleMonitor() {
    if (!show) return;
    monitorStatus = "loading";
    monitorMessage = "";
    const result = await tvApi.monitorTVShow(id);
    if (result.error) {
      monitorStatus = "error";
      monitorMessage = result.message || "Failed to enable monitoring.";
    } else {
      monitorStatus = "success";
      const queued = result.data?.queued ?? 0;
      monitorMessage = `Monitoring enabled — ${queued} episode${queued !== 1 ? "s" : ""} queued.`;
      if (show) show = { ...show, monitored: true };
      const epsResult = await tvApi.getEpisodes(id);
      if (!epsResult.error) episodes = epsResult.data?.episodes ?? [];
    }
  }

  async function handleRequest() {
    if (!show) return;
    requestStatus = "loading";
    requestMessage = "";
    const result = await tvApi.requestTVShow(id);
    if (result.error) {
      requestStatus = "error";
      requestMessage = result.message || "Failed to request TV show.";
    } else {
      requestStatus = "success";
      requestMessage = result.data?.message || "TV show requested successfully!";
    }
  }

  onMount(async () => {
    const [showResult, epsResult] = await Promise.all([
      tvApi.getTVShow(id),
      tvApi.getEpisodes(id),
    ]);

    if (showResult.error) {
      error = showResult.message || "Failed to load TV show.";
    } else {
      show = showResult.data ?? null;
    }
    loading = false;

    if (!epsResult.error) episodes = epsResult.data?.episodes ?? [];
    episodesLoading = false;
  });
</script>

<svelte:head>
  <title>{show ? show.name : "TV Show"} — OpenMedia Project</title>
</svelte:head>

{#if loading}
  <div class="flex items-center justify-center py-24">
    <div class="text-muted-foreground text-lg animate-pulse">Loading…</div>
  </div>
{:else if error}
  <div class="max-w-2xl mx-auto py-12 text-center">
    <p class="text-red-600 mb-4">{error}</p>
    <Button on:click={() => goto("/tvshows")}>Back to TV Shows</Button>
  </div>
{:else if show}
  <div class="relative">
    <!-- Backdrop: absolute, sits behind page content -->
    {#if show.backdrop_url && !backdropError}
      <div class="absolute inset-x-0 top-0 h-[32rem] overflow-hidden pointer-events-none">
        <img
          src={show.backdrop_url}
          alt="{show.name} backdrop"
          class="w-full h-full object-cover"
          on:error={() => (backdropError = true)}
        />
        <div class="absolute inset-0 bg-gradient-to-b from-transparent via-background/60 to-background" />
      </div>
    {/if}

    <div class="relative max-w-5xl mx-auto space-y-8 {show.backdrop_url && !backdropError ? 'pt-48 md:pt-64' : ''}">
    <!-- Poster + main info -->
    <div class="flex flex-col md:flex-row gap-8">
      <!-- Poster -->
      <div class="flex-shrink-0 w-48 mx-auto md:mx-0">
        {#if show.poster_url && !posterError}
          <img
            src={show.poster_url}
            alt={show.name}
            class="w-full h-auto rounded-lg shadow-lg"
            on:error={() => (posterError = true)}
          />
        {:else}
          <div class="w-48 h-72 rounded-lg bg-muted flex items-center justify-center shadow-lg">
            <span class="text-muted-foreground text-4xl">📺</span>
          </div>
        {/if}
      </div>

      <!-- Details -->
      <div class="flex-1 space-y-4">
        <div>
          <h1 class="text-3xl md:text-4xl font-bold">{show.name}</h1>

          <!-- Year · Network · Content rating · Status -->
          <div class="flex flex-wrap items-center gap-2 mt-2 text-muted-foreground text-sm">
            {#if show.first_aired}
              <span class="font-medium text-foreground">{formatYear(show.first_aired)}</span>
            {/if}
            {#if show.network}
              <span>· {show.network}</span>
            {/if}
            {#if show.content_rating}
              <span class="inline-block text-xs font-semibold px-1.5 py-0.5 rounded border border-muted-foreground/40">
                {show.content_rating}
              </span>
            {/if}
            {#if show.status}
              <span class="inline-block text-xs font-semibold px-2.5 py-1 rounded-full {showStatusClass(show.status)}">
                {show.status}
              </span>
            {/if}
          </div>
        </div>

        <!-- Genres -->
        {#if show.genres && show.genres.length > 0}
          <div class="flex flex-wrap gap-2">
            {#each show.genres as genre}
              <span class="text-xs px-2.5 py-1 rounded-full bg-secondary text-secondary-foreground font-medium">
                {genre}
              </span>
            {/each}
          </div>
        {/if}

        <!-- Ratings row -->
        <div class="flex flex-wrap items-center gap-3">
          {#if show.rating && show.rating > 0 && show.rating <= 10}
            <div class="flex items-center gap-1.5">
              <span class="text-xs font-semibold text-muted-foreground uppercase tracking-wide">TVDB</span>
              <span class="text-sm font-bold">⭐ {show.rating.toFixed(1)}</span>
            </div>
          {/if}
          {#if show.tmdb_rating && show.tmdb_rating > 0}
            <div class="flex items-center gap-1.5">
              <span class="text-xs font-semibold text-muted-foreground uppercase tracking-wide">TMDB</span>
              <span class="text-sm font-bold text-blue-600">{show.tmdb_rating.toFixed(1)}</span>
            </div>
          {/if}
          {#if show.imdb_rating && show.imdb_rating !== "N/A"}
            <div class="flex items-center gap-1.5">
              <span class="text-xs font-semibold px-1.5 py-0.5 rounded bg-yellow-400 text-black font-bold">IMDb</span>
              <span class="text-sm font-bold">{show.imdb_rating}</span>
              {#if show.imdb_id}
                <a
                  href="https://www.imdb.com/title/{show.imdb_id}/"
                  target="_blank"
                  rel="noopener noreferrer"
                  class="text-xs text-muted-foreground hover:underline"
                >↗</a>
              {/if}
            </div>
          {:else if show.imdb_id}
            <a
              href="https://www.imdb.com/title/{show.imdb_id}/"
              target="_blank"
              rel="noopener noreferrer"
              class="text-xs px-1.5 py-0.5 rounded bg-yellow-400 text-black font-bold hover:bg-yellow-300"
            >IMDb ↗</a>
          {/if}
          {#if show.rotten_tomatoes && show.rotten_tomatoes !== "N/A"}
            <div class="flex items-center gap-1.5">
              {#if show.rt_url}
                <a href={show.rt_url} target="_blank" rel="noopener noreferrer"
                  class="text-xs font-semibold px-1.5 py-0.5 rounded {rtClass(show.rotten_tomatoes)} hover:opacity-80">🍅 RT</a>
              {:else}
                <span class="text-xs font-semibold px-1.5 py-0.5 rounded {rtClass(show.rotten_tomatoes)}">🍅 RT</span>
              {/if}
              <span class="text-sm font-bold">{show.rotten_tomatoes}</span>
              {#if show.rt_audience_score && show.rt_audience_score !== "N/A"}
                <span class="text-xs text-muted-foreground">/ 🍿 {show.rt_audience_score}</span>
              {/if}
            </div>
          {/if}
          {#if show.metacritic && show.metacritic !== "N/A"}
            <div class="flex items-center gap-1.5">
              <span class="text-xs font-bold px-1.5 py-0.5 rounded {metacriticClass(show.metacritic)}">{show.metacritic}</span>
              <span class="text-xs text-muted-foreground">Metacritic</span>
            </div>
          {/if}
          {#if show.popularity && show.popularity > 0}
            <div class="flex items-center gap-1.5">
              <span class="text-xs font-semibold text-muted-foreground uppercase tracking-wide">Popularity</span>
              <span class="text-sm font-bold">{show.popularity.toFixed(0)}</span>
            </div>
          {/if}
        </div>

        {#if show.overview}
          <p class="text-muted-foreground leading-relaxed">{show.overview}</p>
        {/if}

        <!-- Action buttons -->
        <div class="pt-2 flex flex-wrap gap-3 items-start">
          <div>
            <Button
              on:click={handleRequest}
              disabled={requestStatus === "loading" || requestStatus === "success"}
              class="min-w-40"
            >
              {#if requestStatus === "loading"}Requesting…
              {:else if requestStatus === "success"}✓ Requested
              {:else}Request Download{/if}
            </Button>
            {#if requestMessage}
              <p class="mt-2 text-sm {requestStatus === 'error' ? 'text-red-600' : 'text-green-600'}">{requestMessage}</p>
            {/if}
          </div>

          <div>
            <Button
              on:click={handleMonitor}
              disabled={monitorStatus === "loading" || show.monitored}
              variant={show.monitored ? "outline" : "secondary"}
              class="min-w-40"
            >
              {#if monitorStatus === "loading"}Enabling…
              {:else if show.monitored}✓ Monitored
              {:else}Monitor{/if}
            </Button>
            {#if monitorMessage}
              <p class="mt-2 text-sm {monitorStatus === 'error' ? 'text-red-600' : 'text-green-600'}">{monitorMessage}</p>
            {/if}
          </div>
        </div>
      </div>
    </div>

    <!-- Cast -->
    {#if show.cast && show.cast.length > 0}
      <div>
        <h2 class="text-xl font-semibold mb-3">Cast</h2>
        <div class="flex flex-wrap gap-4">
          {#each show.cast as member}
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

    <!-- Episodes by season (newest first, newest open) -->
    <div>
      <h2 class="text-2xl font-semibold mb-4">Episodes</h2>

      {#if episodesLoading}
        <p class="text-muted-foreground animate-pulse">Loading episodes…</p>
      {:else if seasons.length === 0}
        <p class="text-muted-foreground">No episodes found.</p>
      {:else}
        <div class="space-y-4">
          {#each seasons as season}
            <Card>
              <CardHeader class="pb-2">
                <button
                  class="flex items-center justify-between w-full text-left"
                  on:click={() => toggleSeason(season)}
                >
                  <CardTitle class="text-lg">
                    {season === 0 ? "Specials" : `Season ${season}`}
                    <span class="ml-2 text-sm font-normal text-muted-foreground">
                      ({seasonMap[season].length} episode{seasonMap[season].length !== 1 ? "s" : ""})
                    </span>
                  </CardTitle>
                  <span class="text-muted-foreground text-sm">
                    {collapsedSeasons.has(season) ? "▶" : "▼"}
                  </span>
                </button>
              </CardHeader>

              {#if !collapsedSeasons.has(season)}
                <CardContent class="pt-0">
                  <div class="overflow-x-auto">
                    <table class="w-full text-sm">
                      <thead>
                        <tr class="border-b text-muted-foreground text-left">
                          <th class="pb-2 pr-4 font-medium w-12">Ep #</th>
                          <th class="pb-2 pr-4 font-medium">Title</th>
                          <th class="pb-2 pr-4 font-medium hidden sm:table-cell">Air Date</th>
                          <th class="pb-2 font-medium">Status</th>
                        </tr>
                      </thead>
                      <tbody class="divide-y divide-border">
                        {#each seasonMap[season].sort((a, b) => a.number - b.number) as ep}
                          <tr class="hover:bg-muted/30 transition-colors">
                            <td class="py-2 pr-4 text-muted-foreground">{ep.number}</td>
                            <td class="py-2 pr-4 font-medium">
                              {ep.title || "TBA"}
                              {#if ep.quality}
                                <span class="ml-2 text-xs text-muted-foreground">{ep.quality}</span>
                              {/if}
                            </td>
                            <td class="py-2 pr-4 text-muted-foreground hidden sm:table-cell">
                              {formatDate(ep.air_date)}
                            </td>
                            <td class="py-2">
                              <span class="text-xs font-semibold px-2 py-0.5 rounded-full {episodeStatusClass(ep.status)}">
                                {ep.status}
                              </span>
                            </td>
                          </tr>
                        {/each}
                      </tbody>
                    </table>
                  </div>
                </CardContent>
              {/if}
            </Card>
          {/each}
        </div>
      {/if}
    </div>
  </div>
  </div>
{/if}
