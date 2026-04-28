<script lang="ts">
  import { cn } from "$lib/utils";

  let className: string | undefined | null = undefined;
  export { className as class };
  export let for_: string | undefined = undefined;

  let element: HTMLLabelElement;
  export { element as el };
</script>

<label
  bind:this={element}
  for={for_}
  class={cn(
    "text-sm font-medium leading-none peer-disabled:cursor-not-allowed peer-disabled:opacity-70",
    className
  )}
  on:click
  on:focus
  on:blur
  {...$$restProps}
>
  <slot />
</label>
