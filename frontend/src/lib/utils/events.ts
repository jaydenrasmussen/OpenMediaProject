import { get_current_component } from 'svelte/internal';

/**
 * Forward all events from a Svelte component
 * @returns A function that can be used with the `use:` directive
 */
export function forwardEvents() {
  const component = get_current_component();
  const events: Record<string, EventListenerOrEventListenerObject> = {};

  return (node: HTMLElement) => {
    const forward = (e: Event) => {
      component?.dispatchEvent(e);
    };

    return {
      destroy() {
        for (const event in events) {
          node.removeEventListener(event, events[event]);
        }
      }
    };
  };
}
