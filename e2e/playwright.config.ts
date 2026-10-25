import { defineConfig } from "@playwright/test";

// CI runners are slow; give pages time to load before an assertion gives up.
export default defineConfig({
  timeout: 60_000,
  expect: { timeout: 15_000 },
  retries: process.env.CI ? 1 : 0,
  workers: 1,
});
