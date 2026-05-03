<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { movieApi, tvApi } from "$lib/api/index";
  import type { Movie, TVShow } from "$lib/api/index";
  import MediaCard from "$lib/components/MediaCard.svelte";
  import { InteractiveHoverButton } from "$lib/components/magic/interactive-hover-button";
  import { BlurFade } from "$lib/components/magic/blur-fade";

  let trendingMoviesRows: Movie[][] = $state([]);
  let trendingMoviesPage = $state(1);
  let trendingMoviesTotalPages = $state(1);
  let trendingMoviesLoadingMore = $state(false);

  let nowPlayingRows: Movie[][] = $state([]);
  let nowPlayingPage = $state(1);
  let nowPlayingTotalPages = $state(1);
  let nowPlayingLoadingMore = $state(false);

  let popularMoviesRows: Movie[][] = $state([]);
  let popularMoviesPage = $state(1);
  let popularMoviesTotalPages = $state(1);
  let popularMoviesLoadingMore = $state(false);

  let topRatedRows: Movie[][] = $state([]);
  let topRatedPage = $state(1);
  let topRatedTotalPages = $state(1);
  let topRatedLoadingMore = $state(false);

  let upcomingRows: Movie[][] = $state([]);
  let upcomingPage = $state(1);
  let upcomingTotalPages = $state(1);
  let upcomingLoadingMore = $state(false);

  let trendingTVRows: TVShow[][] = $state([]);
  let trendingTVPage = $state(1);
  let trendingTVTotalPages = $state(1);
  let trendingTVLoadingMore = $state(false);

  let popularTV: TVShow[] = $state([]);
  let hero: Movie | null = $state(null);
  let loading = $state(true);
  let searchQuery = $state("");

  function getYear(dateStr: string | undefined) {
    if (!dateStr) return "";
    return dateStr.slice(0, 4);
  }

  function handleSearch(e: Event) {
    e.preventDefault();
    if (searchQuery.trim()) {
      goto(`/search?q=${encodeURIComponent(searchQuery.trim())}`);
    }
  }

  async function loadData() {
    loading = true;

    const [trending, nowPlaying, popular, topRated, upcoming, trendTV, popTV] =
      await Promise.all([
        movieApi.fetchTrending(1),
        movieApi.fetchNowPlaying(1),
        movieApi.fetchPopular(1),
        movieApi.fetchTopRated(1),
        movieApi.fetchUpcoming(1),
        tvApi.fetchTrendingTV(1),
        tvApi.fetchPopularTV(),
      ]);

    const tm = trending.data?.movies ?? [];
    trendingMoviesRows       = tm.length ? [tm] : [];
    trendingMoviesPage       = trending.data?.page        ?? 1;
    trendingMoviesTotalPages = trending.data?.total_pages ?? 1;

    const np = nowPlaying.data?.movies ?? [];
    nowPlayingRows       = np.length ? [np] : [];
    nowPlayingPage       = nowPlaying.data?.page        ?? 1;
    nowPlayingTotalPages = nowPlaying.data?.total_pages ?? 1;

    const pm = popular.data?.movies ?? [];
    popularMoviesRows       = pm.length ? [pm] : [];
    popularMoviesPage       = popular.data?.page        ?? 1;
    popularMoviesTotalPages = popular.data?.total_pages ?? 1;

    const tr = topRated.data?.movies ?? [];
    topRatedRows       = tr.length ? [tr] : [];
    topRatedPage       = topRated.data?.page        ?? 1;
    topRatedTotalPages = topRated.data?.total_pages ?? 1;

    const up = upcoming.data?.movies ?? [];
    upcomingRows       = up.length ? [up] : [];
    upcomingPage       = upcoming.data?.page        ?? 1;
    upcomingTotalPages = upcoming.data?.total_pages ?? 1;

    const tt = trendTV.data?.shows ?? [];
    trendingTVRows       = tt.length ? [tt] : [];
    trendingTVPage       = trendTV.data?.page        ?? 1;
    trendingTVTotalPages = trendTV.data?.total_pages ?? 1;

    popularTV = popTV.data?.shows ?? [];

    hero = (trendingMoviesRows[0]?.[0]) ?? (nowPlayingRows[0]?.[0]) ?? null;
    loading = false;
  }

  async function loadMoreTrendingMovies() {
    trendingMoviesLoadingMore = true;
    const res = await movieApi.fetchTrending(trendingMoviesPage + 1);
    if (res.data?.movies?.length) {
      trendingMoviesRows       = [...trendingMoviesRows, res.data.movies];
      trendingMoviesPage       = res.data.page;
      trendingMoviesTotalPages = res.data.total_pages;
    }
    trendingMoviesLoadingMore = false;
  }

  async function loadMoreNowPlaying() {
    nowPlayingLoadingMore = true;
    const res = await movieApi.fetchNowPlaying(nowPlayingPage + 1);
    if (res.data?.movies?.length) {
      nowPlayingRows       = [...nowPlayingRows, res.data.movies];
      nowPlayingPage       = res.data.page;
      nowPlayingTotalPages = res.data.total_pages;
    }
    nowPlayingLoadingMore = false;
  }

  async function loadMorePopularMovies() {
    popularMoviesLoadingMore = true;
    const res = await movieApi.fetchPopular(popularMoviesPage + 1);
    if (res.data?.movies?.length) {
      popularMoviesRows       = [...popularMoviesRows, res.data.movies];
      popularMoviesPage       = res.data.page;
      popularMoviesTotalPages = res.data.total_pages;
    }
    popularMoviesLoadingMore = false;
  }

  async function loadMoreTopRated() {
    topRatedLoadingMore = true;
    const res = await movieApi.fetchTopRated(topRatedPage + 1);
    if (res.data?.movies?.length) {
      topRatedRows       = [...topRatedRows, res.data.movies];
      topRatedPage       = res.data.page;
      topRatedTotalPages = res.data.total_pages;
    }
    topRatedLoadingMore = false;
  }

  async function loadMoreUpcoming() {
    upcomingLoadingMore = true;
    const res = await movieApi.fetchUpcoming(upcomingPage + 1);
    if (res.data?.movies?.length) {
      upcomingRows       = [...upcomingRows, res.data.movies];
      upcomingPage       = res.data.page;
      upcomingTotalPages = res.data.total_pages;
    }
    upcomingLoadingMore = false;
  }

  async function loadMoreTrendingTV() {
    trendingTVLoadingMore = true;
    const res = await tvApi.fetchTrendingTV(trendingTVPage + 1);
    if (res.data?.shows?.length) {
      trendingTVRows       = [...trendingTVRows, res.data.shows];
      trendingTVPage       = res.data.page;
      trendingTVTotalPages = res.data.total_pages;
    }
    trendingTVLoadingMore = false;
  }

  onMount(loadData);
</script>

<svelte:head>
  <title>Discover — OpenMediaProject</title>
</svelte:head>

<!-- Break out of container padding for full-width layout -->
<div class="-mx-4 -mt-4 md:-mx-8 md:-mt-8">

  <!-- Hero -->
  {#if hero}
    {@const h = hero}
    <div class="relative w-full h-72 md:h-96 overflow-hidden bg-black">
      {#if h.backdrop_url}
        <img src={h.backdrop_url} alt={h.title} class="w-full h-full object-cover opacity-60" />
      {/if}
      <div class="absolute inset-0 bg-gradient-to-t from-background via-background/40 to-transparent"></div>

      <!-- Centered search bar overlaid on hero -->
      <div class="absolute inset-0 flex flex-col items-center justify-center gap-4 px-4">
        <h1 class="text-3xl md:text-4xl font-bold text-center text-black dark:text-white">Discover</h1>
        <form onsubmit={handleSearch} class="w-full max-w-xl">
          <input
            type="text"
            bind:value={searchQuery}
            placeholder="Search movies and TV shows…"
            class="w-full h-12 rounded-lg border border-white/30 bg-black/50 backdrop-blur-sm px-4 text-white placeholder:text-white/60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-white/50 text-base"
          />
        </form>
      </div>

      <!-- Hero title at bottom -->
      <div class="absolute bottom-0 left-0 px-6 pb-5">
        <p class="text-xs uppercase tracking-wider mb-1 text-black/60 dark:text-white/60">Featured</p>
        <h2 class="text-2xl font-bold text-black dark:text-white">{h.title}</h2>
        <div class="flex items-center gap-3 mt-1 text-sm text-black/70 dark:text-white/70">
          <span>{getYear(h.release_date)}</span>
          {#if h.rating}
            <span>&#11088; {h.rating.toFixed(1)}</span>
          {/if}
        </div>
      </div>
    </div>
  {:else}
    <!-- No hero yet: show search bar on its own -->
    <div class="px-6 pt-10 pb-6 flex flex-col items-center gap-3">
      <h1 class="text-3xl font-bold">Discover</h1>
      <form onsubmit={handleSearch} class="w-full max-w-xl">
        <input
          type="text"
          bind:value={searchQuery}
          placeholder="Search movies and TV shows…"
          class="w-full h-12 rounded-lg border border-input bg-background px-4 text-base placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
        />
      </form>
    </div>
  {/if}

  {#if loading}
    <div class="flex items-center justify-center py-32 text-muted-foreground animate-pulse">
      Loading content…
    </div>
  {:else}
    <BlurFade>
      <div class="space-y-8 px-6 py-8">

      {#if trendingMoviesRows.length > 0}
        <section>
          <h2 class="text-xl font-semibold mb-3">Trending Movies</h2>
          {#each trendingMoviesRows as row, i}
            <div class="flex gap-3 overflow-x-auto pb-2" class:mt-3={i > 0}>
              {#each row as movie (movie.id)}
                <MediaCard id={movie.id} title={movie.title} year={getYear(movie.release_date)} posterUrl={movie.poster_url} type="movie" />
              {/each}
            </div>
          {/each}
          {#if trendingMoviesPage < trendingMoviesTotalPages}
            <div class="mt-3 flex justify-center">
              <InteractiveHoverButton class="text-sm px-4 py-1" onclick={loadMoreTrendingMovies} disabled={trendingMoviesLoadingMore}>
                {trendingMoviesLoadingMore ? "Loading…" : "More"}
              </InteractiveHoverButton>
            </div>
          {/if}
        </section>
      {/if}

      {#if nowPlayingRows.length > 0}
        <section>
          <h2 class="text-xl font-semibold mb-3">Now Playing</h2>
          {#each nowPlayingRows as row, i}
            <div class="flex gap-3 overflow-x-auto pb-2" class:mt-3={i > 0}>
              {#each row as movie (movie.id)}
                <MediaCard id={movie.id} title={movie.title} year={getYear(movie.release_date)} posterUrl={movie.poster_url} type="movie" />
              {/each}
            </div>
          {/each}
          {#if nowPlayingPage < nowPlayingTotalPages}
            <div class="mt-3 flex justify-center">
              <InteractiveHoverButton class="text-sm px-4 py-1" onclick={loadMoreNowPlaying} disabled={nowPlayingLoadingMore}>
                {nowPlayingLoadingMore ? "Loading…" : "More"}
              </InteractiveHoverButton>
            </div>
          {/if}
        </section>
      {/if}

      {#if popularMoviesRows.length > 0}
        <section>
          <h2 class="text-xl font-semibold mb-3">Popular Movies</h2>
          {#each popularMoviesRows as row, i}
            <div class="flex gap-3 overflow-x-auto pb-2" class:mt-3={i > 0}>
              {#each row as movie (movie.id)}
                <MediaCard id={movie.id} title={movie.title} year={getYear(movie.release_date)} posterUrl={movie.poster_url} type="movie" />
              {/each}
            </div>
          {/each}
          {#if popularMoviesPage < popularMoviesTotalPages}
            <div class="mt-3 flex justify-center">
              <InteractiveHoverButton class="text-sm px-4 py-1" onclick={loadMorePopularMovies} disabled={popularMoviesLoadingMore}>
                {popularMoviesLoadingMore ? "Loading…" : "More"}
              </InteractiveHoverButton>
            </div>
          {/if}
        </section>
      {/if}

      {#if topRatedRows.length > 0}
        <section>
          <h2 class="text-xl font-semibold mb-3">Top Rated Movies</h2>
          {#each topRatedRows as row, i}
            <div class="flex gap-3 overflow-x-auto pb-2" class:mt-3={i > 0}>
              {#each row as movie (movie.id)}
                <MediaCard id={movie.id} title={movie.title} year={getYear(movie.release_date)} posterUrl={movie.poster_url} type="movie" />
              {/each}
            </div>
          {/each}
          {#if topRatedPage < topRatedTotalPages}
            <div class="mt-3 flex justify-center">
              <InteractiveHoverButton class="text-sm px-4 py-1" onclick={loadMoreTopRated} disabled={topRatedLoadingMore}>
                {topRatedLoadingMore ? "Loading…" : "More"}
              </InteractiveHoverButton>
            </div>
          {/if}
        </section>
      {/if}

      {#if upcomingRows.length > 0}
        <section>
          <h2 class="text-xl font-semibold mb-3">Upcoming Movies</h2>
          {#each upcomingRows as row, i}
            <div class="flex gap-3 overflow-x-auto pb-2" class:mt-3={i > 0}>
              {#each row as movie (movie.id)}
                <MediaCard id={movie.id} title={movie.title} year={getYear(movie.release_date)} posterUrl={movie.poster_url} type="movie" />
              {/each}
            </div>
          {/each}
          {#if upcomingPage < upcomingTotalPages}
            <div class="mt-3 flex justify-center">
              <InteractiveHoverButton class="text-sm px-4 py-1" onclick={loadMoreUpcoming} disabled={upcomingLoadingMore}>
                {upcomingLoadingMore ? "Loading…" : "More"}
              </InteractiveHoverButton>
            </div>
          {/if}
        </section>
      {/if}

      {#if trendingTVRows.length > 0}
        <section>
          <h2 class="text-xl font-semibold mb-3">Trending TV Shows</h2>
          {#each trendingTVRows as row, i}
            <div class="flex gap-3 overflow-x-auto pb-2" class:mt-3={i > 0}>
              {#each row as show (show.id)}
                <MediaCard id={show.id} title={show.name} year={getYear(show.first_aired)} posterUrl={show.poster_url} type="tv" />
              {/each}
            </div>
          {/each}
          {#if trendingTVPage < trendingTVTotalPages}
            <div class="mt-3 flex justify-center">
              <InteractiveHoverButton class="text-sm px-4 py-1" onclick={loadMoreTrendingTV} disabled={trendingTVLoadingMore}>
                {trendingTVLoadingMore ? "Loading…" : "More"}
              </InteractiveHoverButton>
            </div>
          {/if}
        </section>
      {/if}

      {#if popularTV.length > 0}
        <section>
          <h2 class="text-xl font-semibold mb-3">Popular TV Shows</h2>
          <div class="flex gap-3 overflow-x-auto pb-2">
            {#each popularTV as show (show.id)}
              <MediaCard id={show.id} title={show.name} year={getYear(show.first_aired)} posterUrl={show.poster_url} type="tv" />
            {/each}
          </div>
        </section>
      {/if}

    </div>
    </BlurFade>
  {/if}

</div>
