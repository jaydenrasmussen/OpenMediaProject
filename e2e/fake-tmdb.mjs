// A stand-in for TMDB that serves the captured responses for one series
// (Breaking Bad, id 1396), so the browser tests need no token or internet.
// node fake-tmdb.mjs [port]   then start the server with
//   TMDB_READ_TOKEN=fake OMP_TMDB_BASE_URL=http://127.0.0.1:<port> OMP_SKYHOOK=off
import { createServer } from "node:http";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const data = join(dirname(fileURLToPath(import.meta.url)), "../crates/omp-metadata/tests/data");
const load = (name) => JSON.parse(readFileSync(join(data, name), "utf8"));
const show = load("tmdb_tv_1396.json");
const seasons = { 0: load("tmdb_tv_1396_season_0.json"), 1: load("tmdb_tv_1396_season_1.json") };
const search = load("tmdb_search_tv_breaking_bad.json");

createServer((req, res) => {
  const url = new URL(req.url, "http://x");
  const send = (code, body) => {
    res.writeHead(code, { "content-type": "application/json" });
    res.end(JSON.stringify(body));
  };
  if (url.pathname === "/tv/1396") {
    const out = { ...show };
    for (const part of (url.searchParams.get("append_to_response") ?? "").split(",")) {
      const n = part.match(/^season\/(\d+)$/)?.[1];
      if (n !== undefined && seasons[n]) out[part] = seasons[n];
    }
    return send(200, out);
  }
  if (url.pathname === "/search/tv") return send(200, search);
  send(404, { status_message: "not found" });
}).listen(Number(process.argv[2] ?? 18500), "127.0.0.1");
