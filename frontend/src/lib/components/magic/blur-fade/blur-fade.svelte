<script lang="ts">
	import { onMount } from "svelte";
	import { cn } from "$lib/utils";
	import type { Snippet } from "svelte";

	let {
		class: className,
		duration = 0.4,
		delay = 0,
		offset = 6,
		direction = "down",
		blur = "6px",
		children,
	}: {
		class?: string;
		duration?: number;
		delay?: number;
		offset?: number;
		direction?: "up" | "down" | "left" | "right";
		blur?: string;
		children?: Snippet;
	} = $props();

	let visible = $state(false);

	onMount(() => {
		requestAnimationFrame(() => {
			visible = true;
		});
	});

	function hiddenTransform(): string {
		switch (direction) {
			case "down":  return `translateY(-${offset}px)`;
			case "up":    return `translateY(${offset}px)`;
			case "right": return `translateX(-${offset}px)`;
			case "left":  return `translateX(${offset}px)`;
		}
	}
</script>

<div
	class={cn(className)}
	style="
		opacity: {visible ? 1 : 0};
		filter: blur({visible ? '0px' : blur});
		transform: {visible ? 'translate(0,0)' : hiddenTransform()};
		transition:
			opacity {duration}s ease-out {delay}s,
			filter  {duration}s ease-out {delay}s,
			transform {duration}s ease-out {delay}s;
	"
>
	{@render children?.()}
</div>
