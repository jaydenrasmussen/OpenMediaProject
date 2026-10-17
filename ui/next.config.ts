import type { NextConfig } from "next";

const config: NextConfig = {
  // Static export: `next build` writes plain files to ui/out; no Node at runtime.
  output: "export",
  trailingSlash: true,
  images: { unoptimized: true },
};

export default config;
