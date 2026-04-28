<script lang="ts">
	import { cva, type VariantProps } from "class-variance-authority";
	import { cn } from "$lib/utils";
	import { forwardEvents } from "$lib/utils/events";

	const buttonVariants = cva(
		"inline-flex items-center justify-center whitespace-nowrap rounded-md text-sm font-medium ring-offset-background transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:pointer-events-none disabled:opacity-50",
		{
			variants: {
				variant: {
					default: "bg-primary text-primary-foreground hover:bg-primary/90",
					destructive:
						"bg-destructive text-destructive-foreground hover:bg-destructive/90",
					outline:
						"border border-input hover:bg-accent hover:text-accent-foreground",
					secondary:
						"bg-secondary text-secondary-foreground hover:bg-secondary/80",
					ghost: "hover:bg-accent hover:text-accent-foreground",
					link: "text-primary underline-offset-4 hover:underline"
				},
				size: {
					default: "h-10 px-4 py-2",
					sm: "h-9 rounded-md px-3",
					lg: "h-11 rounded-md px-8",
					icon: "h-10 w-10"
				}
			},
			defaultVariants: {
				variant: "default",
				size: "default"
			}
		}
	);

	type ButtonVariants = VariantProps<typeof buttonVariants>;

	export let variant: ButtonVariants["variant"] = "default";
	export let size: ButtonVariants["size"] = "default";

	let className: string | undefined | null = undefined;
	export { className as class };

	let element: HTMLButtonElement;
	export { element as el };

	export let type: HTMLButtonElement["type"] = "button";
	export let disabled: boolean = false;
	export let form: string | undefined = undefined;
	export let formaction: string | undefined = undefined;
	export let formenctype: string | undefined = undefined;
	export let formmethod: string | undefined = undefined;
	export let formnovalidate: boolean | undefined = undefined;
	export let formtarget: string | undefined = undefined;
	export let name: string | undefined = undefined;
	export let value: string | undefined = undefined;

	const events = forwardEvents();
</script>

<button
	bind:this={element}
	{type}
	{disabled}
	{form}
	{formaction}
	{formenctype}
	{formmethod}
	{formnovalidate}
	{formtarget}
	{name}
	{value}
	class={cn(buttonVariants({ variant, size }), className)}
	use:events
	{...$$restProps}
>
	<slot />
</button>
