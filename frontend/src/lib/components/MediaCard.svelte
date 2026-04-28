<script lang="ts">
  export let id: string;
  export let title: string;
  export let year: string;
  export let posterUrl: string;
  export let type: "movie" | "tv";

  let imgError = false;

  $: href = type === "movie" ? `/movies/${id}` : `/tvshows/${id}`;

  function handleImgError() {
    imgError = true;
  }
</script>

<a {href} class="group flex-none w-36 md:w-44 block">
  <div
    class="relative aspect-[2/3] rounded-md overflow-hidden bg-muted transition-transform duration-200 group-hover:scale-105"
  >
    {#if posterUrl && !imgError}
      <img
        src={posterUrl}
        alt={title}
        class="w-full h-full object-cover"
        on:error={handleImgError}
      />
    {:else}
      <div
        class="w-full h-full flex items-center justify-center text-muted-foreground text-xs"
      >
        No Image
      </div>
    {/if}
    <div
      class="absolute inset-0 bg-black/0 group-hover:bg-black/20 transition-colors duration-200"
    />
  </div>
  <div class="mt-1.5 px-0.5">
    <p class="text-sm font-medium leading-tight truncate">{title}</p>
    {#if year}
      <p class="text-xs text-muted-foreground mt-0.5">{year}</p>
    {/if}
  </div>
</a>
