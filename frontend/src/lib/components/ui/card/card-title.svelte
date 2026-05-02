<script lang="ts">
  import { cn } from "$lib/utils";

  let className: string | undefined | null = undefined;
  export { className as class };

  export let tag: string = "h3";

  let element: HTMLElement | undefined = undefined;
  export { element as el };
</script>

<svelte:element
  this={tag}
  bind:this={element}
  class={cn("text-2xl font-semibold leading-none tracking-tight", className)}
  {...$$restProps}
>
  <slot />
</svelte:element>
