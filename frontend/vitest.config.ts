import { defineConfig } from 'vitest/config';
import { resolve } from 'path';

export default defineConfig({
  resolve: {
    alias: {
      // Mirror the SvelteKit $lib alias so tests can use it.
      '$lib': resolve(__dirname, 'src/lib'),
    },
  },
  test: {
    globals: true,
    environment: 'node',
    include: ['src/**/*.test.ts'],
    coverage: {
      provider: 'v8',
      reporter: ['text', 'lcov'],
      include: ['src/lib/**/*.ts'],
      exclude: [
        'src/**/*.test.ts',
        'src/**/*.spec.ts',
        // Svelte component re-exports — not unit-testable without DOM/Svelte runtime
        'src/lib/components/**',
        // Uses svelte/internal (get_current_component) — requires a live Svelte component context
        'src/lib/utils/events.ts',
      ],
      thresholds: {
        lines: 85,
        functions: 85,
        branches: 85,
        statements: 85,
      },
    },
  },
});
