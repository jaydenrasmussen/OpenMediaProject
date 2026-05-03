<script lang="ts">
	import { ArrowRight } from "lucide-svelte";
	import { cn } from "$lib/utils";
	import type { Snippet } from "svelte";

	let {
		class: className,
		disabled = false,
		type = "button",
		children,
		...restProps
	}: {
		class?: string;
		disabled?: boolean;
		type?: HTMLButtonElement["type"];
		children?: Snippet;
		[key: string]: unknown;
	} = $props();
</script>

<button
	{type}
	{disabled}
	class={cn(
		"group relative w-auto cursor-pointer overflow-hidden rounded-full border bg-background p-2 px-6 text-center font-semibold transition-colors disabled:pointer-events-none disabled:opacity-50",
		className
	)}
	{...restProps}
>
	<div class="flex items-center gap-2">
		<div class="h-2 w-2 rounded-full bg-primary transition-all duration-300 group-hover:scale-[100.8]"></div>
		<span class="inline-block transition-all duration-300 group-hover:translate-x-12 group-hover:opacity-0">
			{@render children?.()}
		</span>
	</div>
	<div class="absolute top-0 z-10 flex h-full w-full translate-x-12 items-center justify-center gap-2 text-primary-foreground opacity-0 transition-all duration-300 group-hover:-translate-x-5 group-hover:opacity-100">
		<span>{@render children?.()}</span>
		<ArrowRight size={16} />
	</div>
</button>
