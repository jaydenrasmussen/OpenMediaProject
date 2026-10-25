import type { NextConfig } from "next";

// The URL base (a reverse-proxy prefix like /media) is only known when the
// server starts, so a production build is made for a stand-in prefix and the
// server swaps in the real one as it serves each file.
export const BASE_PLACEHOLDER = "/__omp_base__";

const config: NextConfig = {
  // Static export: `next build` writes plain files to ui/out; no Node at runtime.
  output: "export",
  trailingSlash: true,
  images: { unoptimized: true },
  ...(process.env.NODE_ENV === "production" && { basePath: BASE_PLACEHOLDER }),
};

export default config;
