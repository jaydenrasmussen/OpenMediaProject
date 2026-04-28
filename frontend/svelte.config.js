import adapter from "@sveltejs/adapter-static";
import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";

/** @type {import('@sveltejs/kit').Config} */
const config = {
  // Consult https://kit.svelte.dev/docs/integrations#preprocessors
  // for more information about preprocessors
  preprocess: vitePreprocess(),

  kit: {
    // adapter-static is used for static site generation
    adapter: adapter({
      // Output directory for the static build
      pages: "build",
      assets: "build",
      fallback: "index.html",
      precompress: true,
    }),
    alias: {
      $lib: "./src/lib",
      $components: "./src/lib/components",
    },
  },

  // Configure vite through svelte.config.js
  vite: {
    server: {
      // For development, proxy API requests to the backend
      proxy: {
        "/api": {
          target: process.env.VITE_API_URL || "http://localhost:8080",
          changeOrigin: true,
          secure: false,
        },
      },
    },
  },

  // Disable SSR
  ssr: false,
  // Treat all pages as SPAs
  prerender: {
    default: false,
  },
};

export default config;
