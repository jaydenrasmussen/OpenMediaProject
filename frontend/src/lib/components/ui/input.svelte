<script lang="ts">
  import { cn } from "$lib/utils";

  let className: string | undefined | null = undefined;
  export { className as class };

  export let value: string = "";
  export let name: string | undefined = undefined;
  export let placeholder: string | undefined = undefined;
  export let disabled: boolean = false;
  export let readonly: boolean = false;
  export let type: string = "text";
  export let id: string | undefined = undefined;

  let element: HTMLInputElement | undefined = undefined;
  export { element as el };
</script>

<input
  bind:this={element}
  {value}
  {id}
  {name}
  {placeholder}
  {disabled}
  {readonly}
  {type}
  on:blur
  on:change
  on:click
  on:focus
  on:input={(e) => (value = e.currentTarget.value)}
  on:input
  on:keyup={(e) => (value = e.currentTarget.value)}
  on:keydown
  on:keypress
  on:keyup
  on:mouseover
  on:mouseenter
  on:mouseleave
  on:paste
  class={cn(
    "flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm ring-offset-background file:border-0 file:bg-transparent file:text-sm file:font-medium placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50",
    className
  )}
  {...$$restProps}
/>
