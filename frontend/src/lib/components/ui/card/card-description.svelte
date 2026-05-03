<script lang="ts">
  import { cn } from "$lib/utils";
  import type { Snippet } from "svelte";

  let {
    class: className,
    children,
    ...restProps
  }: { class?: string; children?: Snippet; [key: string]: unknown } = $props();
</script>

<p class={cn("text-sm text-muted-foreground", className)} {...restProps}>
  {@render children?.()}
</p>
