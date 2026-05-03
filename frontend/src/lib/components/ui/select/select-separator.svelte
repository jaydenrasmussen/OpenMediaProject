<script lang="ts">
	import { cn } from "$lib/utils";

	let { class: className, ...restProps } = $props();
</script>

<div class={cn("bg-muted -mx-1 my-1 h-px", className)} {...restProps}></div>
