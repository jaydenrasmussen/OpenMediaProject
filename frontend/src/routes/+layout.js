// This file disables SSR for all routes
export const ssr = false;
export const prerender = false;

// Enable client-side routing
export const csr = true;

// This empty load function ensures the layout is loaded client-side
export function load() {
    return {};
}
