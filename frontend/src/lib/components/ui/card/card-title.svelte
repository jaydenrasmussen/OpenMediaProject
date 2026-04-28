<script lang="ts">
  import { cn } from "$lib/utils";
  import type { HTMLAttributes, HTMLHeadingElement } from "svelte/elements";

  let className: string | undefined | null = undefined;
  export { className as class };

  export let tag: keyof HTMLAttributes<HTMLHeadingElement> = "h3";

  // Enable forwarding of the ref to the element
  let element: HTMLElement;
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
