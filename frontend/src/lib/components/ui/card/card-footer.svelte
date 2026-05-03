<script lang="ts">
  import { cn } from "$lib/utils";
  import type { Snippet } from "svelte";

  let {
    class: className,
    children,
    ...restProps
  }: { class?: string; children?: Snippet; [key: string]: unknown } = $props();
</script>

<div class={cn("flex items-center p-6 pt-0", className)} {...restProps}>
  {@render children?.()}
</div>
