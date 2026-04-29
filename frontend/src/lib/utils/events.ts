import { get_current_component } from "svelte/internal";

/**
 * Forward all events registered by a parent component to the underlying DOM node.
 *
 * The previous implementation created a `forward` closure and an `events` map
 * but never called `node.addEventListener`, so no events were ever forwarded.
 * This version reads the callbacks that the parent registered on the component
 * (`component.$$.callbacks`) and wires each one up to the DOM node so they fire
 * correctly (e.g. `<Button on:click={…}>` now works as expected).
 */
export function forwardEvents() {
    const component = get_current_component();

    return (node: HTMLElement) => {
        const listeners: Array<[string, (e: Event) => void]> = [];

        function forward(e: Event) {
            // Invoke every Svelte callback registered for this event type.
            const cbs: Array<(e: Event) => void> = (
                component as { $$: { callbacks: Record<string, Array<(e: Event) => void>> } }
            ).$$.callbacks[e.type];
            if (cbs) {
                cbs.slice().forEach((fn) => fn.call(component, e));
            }
        }

        const callbacks: Record<string, unknown> =
            (component as { $$: { callbacks: Record<string, unknown> } })?.$$.callbacks ?? {};

        for (const event of Object.keys(callbacks)) {
            node.addEventListener(event, forward);
            listeners.push([event, forward]);
        }

        return {
            destroy() {
                for (const [event, listener] of listeners) {
                    node.removeEventListener(event, listener);
                }
            },
        };
    };
}
