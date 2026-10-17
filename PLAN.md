# OpenMediaProject — Rust Rewrite Plan

Status: draft v0.3. Detailed facts from the Sonarr/Radarr source are in [`REFERENCE.md`](REFERENCE.md); check it before re-reading their code. This document is the starting point for the rewrite. Section 10 records decisions made so far and the questions still open.

## 1. Goal and rules

Replace the *arr stack (Radarr, Sonarr, Lidarr, Readarr, Bazarr, and so on) with one Rust application.

| # | Rule | What it means for the design |
|---|------|------------------------------|
| 1 | Everything fits in ONE container | One process, one image, one data volume. No sidecars, no external DB, no message broker. |
| 2 | Extra features come from PLUGINS | The core stays small. Anything not needed for the default experience is a plugin, not a built-in. |
| 3 | Default = simple NZB client + BitTorrent client | A fresh install can download from Usenet and from torrents with no other software. Think SABnzbd + Transmission. |
| 4 | UI is Next.js, built STATICALLY | `next build` with `output: 'export'`. No Node.js at runtime. The Rust binary serves the files. |
| 5 | Start with the Sonarr/Radarr core, ALL features | Phase 1 is feature parity with Sonarr and Radarr. Lidarr, Readarr and Bazarr come later as plugins. |

### Non-goals (for now)
- Music, books and subtitles in the core. These become plugins after the core is done.
- A media server (Plex/Jellyfin replacement). We manage files. We do not stream them.
- Backwards compatibility with existing Sonarr/Radarr databases. (API compatibility is a later optional plugin; see Q2.)

## 2. Architecture

```
┌──────────────────────────── one container ────────────────────────────┐
│                                                                        │
│  Static UI (Next.js export, embedded in binary)                        │
│        │  HTTP/JSON + WebSocket                                        │
│  ┌─────▼──────────────────────── omp (Rust binary) ─────────────────┐  │
│  │  API layer (axum)  ── auth ── OpenAPI                            │  │
│  │                                                                  │  │
│  │  Core services                                                   │  │
│  │   Library │ Metadata │ Search │ Decision │ Import │ Scheduler    │  │
│  │                                                                  │  │
│  │  Built-in downloaders          Plugin host (WASM)                │  │
│  │   Usenet (NNTP) │ BitTorrent    ├─ indexers  ├─ notifiers        │  │
│  │                                 ├─ lists     └─ future: music,   │  │
│  │  Event bus (in-process)         │               books, subs     │  │
│  │  Storage: SQLite (embedded)                                      │  │
│  └──────────────────────────────────────────────────────────────────┘  │
└────────────────────────────────────────────────────────────────────────┘
        /config (db, plugins, settings)   /downloads   /media
```

### Process model
One tokio runtime. Services talk through typed in-process channels and an event bus. No internal HTTP between services.

### Storage
- **SQLite** through `sqlx` (WAL mode, compile-time checked queries, embedded migrations). One file in `/config`.
- Why not Postgres: breaks rule 1. Why not a KV store: the data is relational (series → seasons → episodes → files → history).

### Crate layout (Cargo workspace)
```
crates/
  omp-server/       binary: wiring, config, startup, static file serving
  omp-api/          axum routes, auth, OpenAPI (utoipa), WebSocket events
  omp-core/         domain types, event bus, errors (no I/O)
  omp-db/           sqlx models, migrations, repositories
  omp-metadata/     TMDB / TVDB clients, caching, refresh
  omp-library/      series, movies, episodes, root folders, scanning
  omp-parser/       release-name and file-name parsing (pure, heavily tested)
  omp-quality/      quality definitions, profiles, custom formats, scoring
  omp-decision/     release decision engine (accept/reject + reasons)
  omp-search/       search orchestration across indexers
  omp-import/       completed-download handling, rename, move/hardlink
  omp-usenet/       NNTP client, yEnc, PAR2, archive extraction
  omp-torrent/      BitTorrent client (wraps an existing engine, see §6)
  omp-downloads/    unified download queue over usenet + torrent + external clients
  omp-plugin-host/  WASM runtime, plugin manifest, host API
  omp-plugin-sdk/   crate that plugin authors depend on
  omp-scheduler/    recurring jobs, RSS sync, refresh, housekeeping
ui/                 Next.js app (static export)
plugins/            first-party plugins (built separately, shipped as .wasm)
```
`omp-parser`, `omp-quality` and `omp-decision` have no I/O so they can be tested with plain unit tests and fixtures.

## 3. Plugin system (rule 2)

This is the riskiest design choice. It must be decided before the core grows around it.

### Recommendation: WebAssembly (WASI component model) via `wasmtime`
| Option | Pros | Cons |
|---|---|---|
| **WASM components (recommended)** | Sandboxed; capability-based (a plugin only gets the network hosts and folders it declares); any language; one portable `.wasm`; works in a minimal container | Component-model tooling is still maturing; host API must be designed carefully; async I/O across the boundary needs care |
| Native dynamic libraries (`.so`) | Fast, simple for Rust-only | No stable Rust ABI; no sandbox; per-arch builds; one bad plugin crashes the process |
| Subprocess + JSON-RPC | Any language; process isolation | Needs a runtime in the image (Python, Node); violates the "small single container" spirit |
| Embedded scripting (Lua/Rhai/JS) | Easy to write | Weak sandboxing; poor tooling; slow for heavy work |

### Plugin types (extension points)
Defined as WIT interfaces in `omp-plugin-sdk`:
1. **Indexer** — `search(query) -> releases`, `rss() -> releases`, `capabilities()`.
2. **Download client** — `add`, `remove`, `status` for external clients (qBittorrent, Deluge, NZBGet…).
3. **Notifier** — `on_event(event)` for Discord, Telegram, Plex/Jellyfin scan, and so on.
4. **Import list** — `fetch() -> items` for Trakt, IMDb lists, Plex watchlist, and so on.
5. **Metadata provider** — alternate sources.
6. **Media type** — a full domain module (music, books, subtitles). Needs its own DB tables, UI pages and API routes. This is the hardest type and is deferred until the core ships (see Q5).

### Plugin lifecycle
- Install by dropping a `.wasm` plus `plugin.toml` into `/config/plugins`, or by URL from the UI.
- `plugin.toml` declares: id, version, type, permissions (network hosts, settings schema), min host API version.
- Settings are declared as JSON Schema. The UI renders the settings form from the schema, so plugins ship no UI code.
- Host API is versioned. Plugins built for API v1 keep working across v1.x.
- A plugin crash or timeout is isolated: the call fails, an event is logged, the app keeps running.

### What is NOT a plugin
Anything the default install needs: metadata for movies and TV, the Usenet client, the torrent client, the quality/decision engine, rename/import, and the web UI. Newznab/Torznab is the one built-in indexer protocol (Q1, decided).

## 4. Feature scope: Sonarr + Radarr core (rule 5)

"All features" is tracked as a checklist. Each item gets a test and a UI page before it is marked done. The list below is the working parity matrix; we should verify it against the current Sonarr/Radarr docs during Phase 0.

### 4.1 Library management
- [ ] Add/edit/delete movies and series; monitored flags (series, season, episode)
- [ ] Root folders, free-space checks, path validation
- [ ] Library import: scan existing folders, match to metadata, bulk import
- [ ] Series types: standard, daily, anime (absolute numbering)
- [ ] Scene numbering / episode mappings and season/episode remaps
- [ ] Movie collections; minimum availability (announced / in cinemas / released)
- [ ] Tags (applied to items, profiles, indexers, notifiers, delay profiles)
- [ ] Series/movie editor (bulk edit), mass delete
- [ ] Manual file import and interactive import with manual matching
- [ ] Unmapped-file handling; file deletion and recycle bin
- [ ] Extra/other file handling (subtitles, nfo, artwork)
- [ ] Metadata files and artwork (kodi/nfo, poster/fanart downloads)

### 4.2 Metadata
- [ ] Movies: the Radarr metadata server (`api.radarr.video`) as primary, TMDB as fallback (search, details, credits, images, release dates, collections) — see §13.3
- [ ] TV: TVDB via SkyHook as primary, TMDB as fallback (search, episodes, seasons, air dates, absolute numbers)
- [ ] Periodic refresh; air-date change tracking; episode additions/removals
- [ ] Image cache served by the app
- [ ] Rate limiting and response caching for provider APIs

### 4.3 Quality and decisions
- [ ] Quality definitions (source + resolution) with size limits (MB/min)
- [ ] Quality profiles: ordered allowed list, cutoff, upgrade-allowed
- [ ] Custom formats: conditions (release title regex, source, resolution, language, release group, size, indexer flag…), scores, min format score, upgrade-until score
- [ ] Release profiles / must contain / must not contain
- [ ] Language profiles/settings
- [ ] Delay profiles (preferred protocol, usenet/torrent delay, bypass-if-highest-quality)
- [ ] Release restrictions, seeders minimum, seed ratio/time goals
- [ ] Decision engine returns accept/reject with human-readable reasons (shown in interactive search)
- [ ] Upgrade logic: quality, custom-format score, proper/repack handling
- [ ] Blocklist (failed or manually blocked releases)

### 4.4 Search and acquisition
- [ ] Indexer management: priority, per-indexer RSS/search toggles, tags, seed settings
- [ ] RSS sync on a schedule, matched against wanted items
- [ ] Automatic search: on add, per item, per season, missing, cutoff unmet
- [ ] Interactive search with filters, sort and manual grab
- [ ] Season pack vs. single-episode logic; multi-episode releases
- [ ] Wanted: Missing and Cutoff Unmet views with bulk search
- [ ] Calendar view and iCal feed
- [ ] Import lists with sync interval, quality profile, root folder, monitor behavior
- [ ] Release push endpoint (for external tools to push releases)

### 4.5 Download and import
- [ ] Unified queue (built-in Usenet, built-in torrent, plugin clients) with progress, ETA, remove/blocklist actions
- [ ] Completed-download handling: detect, import, optionally remove from client
- [ ] Failed-download handling: blocklist and re-search
- [ ] Import: hardlink / copy / move, with fallback; atomic move across filesystems
- [ ] Remote path mappings
- [ ] Episode and movie renaming with token-based naming formats; preview; bulk rename of existing files
- [ ] Folder naming, season folders, multi-episode styles, colon/illegal-character replacement
- [ ] Media info extraction via bundled `ffprobe` (see Q6) for codec, audio, HDR, languages
- [ ] Upgrade replaces the old file (recycle bin or delete)
- [ ] Unmonitor-deleted-files behavior

### 4.6 System
- [ ] Authentication: forms login, API key, optional "disabled for local addresses"; users stored hashed (argon2)
- [ ] Notifications (as plugins; see §7 Phase 3 for first-party set) with per-event triggers and tags
- [ ] Scheduled tasks page, manual run, last/next run
- [ ] History (grabbed, imported, failed, deleted, renamed) with filters and per-item history
- [ ] Event log, log levels, log file rotation, log download
- [ ] Backup and restore (DB + settings + plugins list), scheduled backups
- [ ] Update mechanism: image-based (pull new tag). No in-app self-update.
- [ ] Health checks (root folder missing, no indexers, client unreachable, disk space, etc.)
- [ ] Disk-space and status pages
- [ ] Global settings: media management, UI (language, date format, theme), general (port, URL base, proxy, SSL)
- [ ] Multi-language UI strings (i18n scaffold; English first)
- [ ] REST API with OpenAPI spec, API key auth, WebSocket for live updates

### 4.7 Built-in downloaders (rule 3)
Usenet (see §5) and BitTorrent (see §6). Both are "simple": add by file/URL/magnet, queue, pause/resume, priority, categories, speed limits, post-processing, and a clean completion signal to the import pipeline. Features like RSS feeds inside the client, scripting, and advanced scheduling are out of scope.

## 5. Usenet client (built-in)

Components:
- **NNTP**: connection pool, multiple servers with priority and fill servers, TLS, retention-aware retry across servers.
- **NZB parser**: segments, groups, password in metadata.
- **yEnc decoder**: SIMD where available; verify CRC32.
- **PAR2**: verify and repair with the bundled `par2cmdline-turbo` binary. 7-Zip cannot do PAR2. A pure-Rust replacement is a later optimization, not a goal.
- **Archives**: bundle the official `7zz` binary from 7-zip.org (not a distro package; see Phase 0 findings) for RAR/RAR5 (including multi-volume), 7z and zip. One tool covers every format, so we do not ship `unrar`. Run it as a subprocess with a timeout and a password list from the NZB.
- **Queue**: priority, pause, categories, duplicate detection, article-cache, disk-space guard, obfuscated-name handling, unwanted-extension rejection, fake-release detection.
- **Output**: completed folder per category, then hand off to import.

Licence note: 7-Zip is LGPL, and its RAR code carries the unRAR restriction (it must not be used to create a RAR compressor). We only extract, so this is fine. Record the notices in the image and in `cargo deny`/third-party notes. Because we call it as a separate process, no linking concerns apply.

## 6. BitTorrent client (built-in)

- Do not write a BitTorrent engine from scratch. Evaluate `librqbit` first (async, Rust, DHT, uTP, magnet, selective files). Fall back to `rust-bittorrent`-style crates or `libtorrent` bindings only if the evaluation fails.
- Required: .torrent and magnet, DHT/PEX/LSD, encryption, uTP, port forwarding (UPnP/NAT-PMP), speed limits, queueing, seeding goals (ratio/time), file selection, move-on-complete, category save paths, IPv6.
- Optional later: bind-to-interface (VPN users), blocklists.
- Phase 0 spike: download a public-domain torrent end to end, measure CPU and memory, test resume after restart.

## 7. Phased delivery

Each phase ends with something that runs in the container. No phase ends with "internal refactor only".

### Phase 0 — Foundations and risk spikes (2–3 weeks)
- Cargo workspace, CI (fmt, clippy, tests, `cargo deny`), multi-arch Docker build (amd64, arm64).
- Spike: WASM plugin host with one toy indexer plugin and the settings-schema flow.
- Spike: torrent engine evaluation (§6).
- Spike: bundle `7zz` and `par2`; extract a multi-volume RAR5 set and repair a damaged set (§5).
- Spike: Next.js static export served by axum with client-side routing and URL base support.
- Q1–Q6 are decided (see §10). Freeze plugin WIT v0.
- **Exit:** a container that serves a static page, runs a toy plugin, and downloads one torrent.

#### Phase 0 progress (updated 2026-10-16)
| Item | Status | Notes |
|---|---|---|
| Cargo workspace, CI (fmt, clippy, test, cargo-deny), GPL-3.0 `LICENSE` | Done | `crates/omp-server` serves `/api/v1/health` and the static UI. CI also builds with the `wasm32-wasip2` target. |
| Spike: WASM plugin host | **Passed** | `wit/omp-plugin.wit` defines the `indexer-plugin` world. `crates/omp-plugin-host` (wasmtime 41, component model, async) runs `plugins/toy-indexer` (82 KB). 5 tests cover: schema and search, allowed HTTP host, denied HTTP host, plugin panic contained, runaway CPU stopped by fuel. A fresh instance per call means a trapped plugin cannot affect later calls. |
| Spike: Next.js static export served by axum | **Passed with caveats** | `ui/` builds with `output: 'export'` and the Rust server serves it with SPA fallback. See caveats below. |
| Spike: torrent engine (`librqbit`) | **Passed** | `crates/omp-torrent/examples/spike.rs` (librqbit 9.0.1, DHT on, JSON session persistence + fastresume). Downloaded the 129 MB Sintel torrent from a public swarm at ~50 MiB/s with ~28 live peers; peak RSS 23 MB. Stopped the session at 45%, restarted it: state went `initializing` (re-check) then `live` at 56%, so progress survived the restart; it then completed. Not yet tested: seeding goals, rate limits, IPv6, bind-to-interface, magnet links, and behavior with 100+ torrents. |
| Spike: bundle `7zz` and `par2` | **Passed, with a packaging rule** | Tested on libarchive's RAR5 fixtures (8-volume set, encrypted, damaged volume) and on a 5 MB file with PAR2 recovery data. See findings below. Not yet tested inside a container image (the Docker daemon was not running). |
| Container image | **Passed** | Root `Dockerfile` (4 stages: UI, server, pinned tools, runtime on `debian:bookworm-slim`) builds in ~50 s and runs as non-root user `omp`. Verified in the running container: `/api/v1/health`, the static UI, `HEALTHCHECK` reports healthy (via `omp-server --healthcheck`, no curl needed), and `7zz` extracts the 8-volume RAR5 set inside the image. 7-Zip and par2 are downloaded from their official releases with pinned SHA-256 per architecture. Built for arm64 only so far; amd64 is built by the existing publish workflow (QEMU) and is untested. |
| Metadata-server terms (Q10) | **Checked: no published terms** | See finding below. |
| TMDB app registration (Q9) | Waiting on you | Needs a TMDB application registered under your account. |

Phase 0 still missing from the exit criteria: the image does not yet run the plugin host or the torrent client (they are separate crates not wired into `omp-server`).

Findings from the spikes:
- **Plugin build toolchain:** a Homebrew `rustc` on `PATH` shadows rustup's, and it has no `wasm32-wasip2` target, so plugin builds fail with `can't find crate for core`. Plugin builds must put rustup's toolchain `bin` first (the host test does this via `rustup which cargo`). Docs for plugin authors must say so.
- **Plugin HTTP:** a host-provided `http-get` with a per-plugin hostname allow-list works and keeps plugins off the network by default. Only `GET` and string bodies exist so far; the real interface needs headers, POST and a response status (the Newznab plugin shape in `REFERENCE.md` §6 suggests the host should do HTTP and plugins only build requests and parse responses).
- **Torrent engine:** `librqbit` is usable as the built-in engine. Its API has what §6 needs (`only_files`, `ratelimits`, `peer_limit`, `bind_device_name`, `disable_upload`, `ipv4_only`, persistence, `paused`). Wrap it behind our own trait so it can be swapped. It pulls in reqwest + rustls + aws-lc-sys, which affects the static musl build (check in the Docker spike).
- **Archive tools (important):** the official **7-Zip 26.04** (`7zz` from 7-zip.org) extracts a multi-volume RAR5 set from the first volume alone, reports `Wrong password` distinctly from `Data Error` (corrupt volume), and returns a non-zero exit code on both. The **Homebrew `sevenzip` 26.04 build cannot extract RAR at all** (`Unsupported Method`, even for unencrypted RAR5). Rule: the image must download the official 7-Zip tarball (pin version and SHA-256) and must not use a distro package, because distro builds may leave out the RAR codec. At startup the app should self-test RAR support (extract a tiny embedded RAR) and raise a Health error if it fails. Password handling: try the NZB's password, then the user's list, treating `Wrong password` as "try next".
- **PAR2:** `par2cmdline-turbo` 1.5.0 (static release binaries exist for linux amd64/arm64/armhf and macOS) created 10% recovery data, detected damage (exit 1, "repair is possible"), repaired it, and the result matched the original SHA-256. A file damaged beyond recovery exits 2. Exit codes: 0 clean, 1 damaged but repairable, 2 not repairable.
- **Image size:** Debian's `ffmpeg` package made the image 505 MB (it pulls in LLVM, Mesa, flite). Switching to the pinned static `ffprobe` from johnvansickle.com (7.0.2, SHA-256 checked per arch) gives **166 MB** on arm64: base ~75 MB, `ffprobe` 51 MB, server 2.4 MB, `7zz` 2.5 MB, `par2` 2.6 MB. Verified `ffprobe` reads the Sintel MP4 (H.264 1024x436, AAC 6 ch, language `eng`). Possible later saving: a minimal custom FFmpeg build with only the demuxers and parsers we need (a few MB), at the cost of CI build time.
- **Metadata servers (Q10):** SkyHook (`skyhook.sonarr.tv`) and the Radarr metadata server (`api.radarr.video`) are **closed source** (the Sonarr org has no SkyHook repo; Radarr publishes only `RadarrAPI.TMDB`/`RadarrAPI.Update`, not the metadata service). I found no published terms of use, rate limits or third-party policy in their repos, the Servarr wiki search results, or the forums. What the forums do show: SkyHook is a caching proxy in front of TVDB with up to ~48 h of staleness, and users report occasional outages, high latency and TVDB mismatches. Consequences: (1) we depend on volunteer-run infrastructure with no stated guarantee; (2) the response shapes are undocumented, so our client must be tolerant and must have fixtures captured from real responses; (3) the fallback to direct TMDB/TVDB must be real and tested, not theoretical; (4) identify ourselves with an honest `User-Agent` and cache aggressively to keep load low; (5) before the first public release, ask the Servarr team (Discord/forum) whether third-party clients may use these endpoints. This does not block development. Decision stands (Q10: yes), with the fallback treated as a first-class path.
- **Plugin limits:** fuel bounds CPU per call. Still to add: memory limit, wall-clock timeout (epoch interruption), and cap on result size.
- **Static UI caveats:** (1) `next build` bakes absolute `/_next/...` paths, so a runtime URL base (reverse-proxy prefix) needs the server to rewrite HTML at startup. (2) ~~The SPA fallback returned HTML with a 200 for missing assets~~ — fixed: the fallback now applies only to paths without a file extension, and missing assets return 404 (tested). (3) Dynamic routes like `/movies/[id]` cannot be pre-rendered, so we use the shell-plus-client-routing approach in §8.

#### Phase 1 progress
| Item | Status | Notes |
|---|---|---|
| `omp-core` quality types | Done | 30 qualities, revision, detection sources, `find_by_source_and_resolution`. |
| `omp-parser` quality parser | Done | Port of Radarr `QualityParser`. Runs 244 name cases + repack + detection-source cases extracted from Radarr's own tests (`crates/omp-parser/tests/data/*.tsv`, header names the source commit). All pass. The harness was checked to fail on a wrong expectation. |
| `omp-parser` title/year/edition/hardcoded-subs/AKA parser | Done | Port of `Parser.ParseMovieTitle` and `ParseMoviePath`. ~190 cases from Radarr's `ParserFixture`, `EditionParserFixture`, `CrapParserFixture` and `HashedReleaseFixture` pass, plus 1000 random hashes are rejected. Extracted data is in `tests/data/radarr_parser_cases.json` (all Radarr parser fixtures, including language and slug cases not yet used). |
| `omp-parser` release group | Done | 154 cases pass. |
| `omp-parser` regex layer | Done | `re.rs`: fancy-regex first, `regress` fallback with Unicode translation for variable-length look-behind. 3 title/release-group patterns need the fallback. |
| `omp-core` languages + `omp-parser` language detection | Done | 60 languages (Radarr ids). `parse_languages` passes all 57 release-name test methods from Radarr's `LanguageParserFixture`. Subtitle-file language/tag parsing is not ported yet (needs repeated-group captures; do it with the extras/subtitle work). |
| Title normalization (`clean_movie_title`, `normalize_*`, `to_url_slug`) | Done | All Radarr slug cases pass; normalization tests written to mirror `NormalizeTitleFixture`. |
| `omp-quality`: profiles, custom formats, upgrade rules | Done | Quality profile index/compare (with groups), all 10 custom-format condition types with required/negate/grouping semantics, `is_upgradable`, `cutoff_not_met`, `is_upgrade_allowed` ported branch for branch; 27 tests cover every branch. User regexes compile through the fallible `Re::try_ci`. |
| `omp-decision`: spec engine + release ranking | Done | All 30 Radarr movie specs (size, age, retention, protocol, raw disc, language, hardcoded subs, release restrictions, indexer flags/seeders/tags, monitored, availability, search match, blocklist, blocked indexer, quality/format allowed, upgrade allowed/disk, queue, history, already imported, proper, repack, pending, delay, free space), priority-group engine (all rejections in the first failing group, disk checks last, temporary vs permanent), and the 8-step ranking. 49 tests, one or more per spec. All data comes through the `DecisionData` trait, so the engine does no I/O and the DB layer plugs in later. |
| `omp-db`: SQLite layer | Done | Two files (`omp.db`, `logs.db`), WAL, enforced foreign keys, embedded migrations. Repositories: config, tags, root folders, quality profiles/definitions, custom formats (validated on save), delay and release profiles, movies + provider metadata (upsert by TMDB id), one file per movie, history, blocklist, pending releases, and a generic provider table with persisted back-off. Deleting a movie cascades to its file, history, blocklist and pending rows, so most of Radarr's ~30 housekeeping jobs are not needed. 21 tests run against a real database, including files surviving a reopen. |
| `omp-core` back-off, blocklist matching, history events | Done | Back-off state machine ported exactly (a success steps the level *down*; 15-minute startup grace). Blocklist matching ported with an upstream quirk kept for parity (`SameNzb` requires a *different* indexer in its second clause). |
| Decision snapshot (DB -> `DecisionData`), release matching to library (`ParsingService`) | Not started | Next: a service crate that loads what the sync engine needs for a batch of movies. |
| RSS sync, search orchestration, queue/tracked downloads, import | Not started | |
| `ParsingService` (match a parsed release to a library movie) | Not started | Needs the DB layer. |
| `omp-core` movie metadata + `is_available` | Done | Port of Radarr's `Movie.IsAvailable` (min-availability, 90-day fallback, delay days). |
| `omp-metadata`: provider trait + TMDB provider | Done | Uses `TMDB_READ_TOKEN` (a TMDB v4 read token, from `.env`, gitignored). Maps release dates (earliest worldwide per kind), certification (US by default), collection, studio, ratings, images, alternative titles, credits. Status is derived from dates (our rule; Radarr's server is closed). 8 unit tests on a real captured response; 3 live tests (`--ignored`) and `examples/lookup.rs` verified against the real API. |
| SkyHook / Radarr metadata-server providers | Not started | Primary providers per Q4/Q10; TMDB is the fallback that now exists. |
| Naming-token engine | Not started | |

### Phase 1 — Movies vertical slice (Radarr core) (6–8 weeks)
Do movies first: simpler model than TV. This proves the whole pipeline.
- DB schema, TMDB client, add/search movie, root folders.
- `omp-parser` (release names), quality definitions/profiles, custom formats, decision engine.
- Indexer plugin interface plus a Newznab/Torznab implementation (see Q1).
- Search (auto and interactive), queue, built-in torrent client, import and rename.
- UI: library, add, movie detail, queue, history, settings basics.
- **Exit:** add a movie, find a release, download via torrent, import and rename it. Automated end-to-end test with a fake indexer.

### Phase 2 — TV (Sonarr core) (6–8 weeks)
- Series, seasons, episodes; TVDB/TMDB; daily and anime types; scene mappings.
- Season-pack logic, multi-episode parsing, monitoring modes.
- Calendar, wanted/missing/cutoff, episode-level history.
- **Exit:** full TV workflow including a season pack, an upgrade, and a rename.

### Phase 3 — Usenet client and the rest of the core (6–8 weeks)
- Built-in Usenet downloader (§5).
- Delay profiles, blocklist, failed-download handling, import lists, remote path mappings, external download-client plugins.
- Notification plugins: Discord, Telegram, email, webhook, Plex/Jellyfin/Emby scan, ntfy, Gotify.
- Backup/restore, health checks, scheduler UI, auth hardening.
- **Exit:** every item in §4 is checked, or has a written reason for deferral.

### Phase 4 — Hardening and parity audit (3–4 weeks)
- Line-by-line comparison against Sonarr/Radarr feature lists and settings pages.
- Performance on a 10k-movie / 2k-series library; memory ceiling in the container.
- Security review (auth, plugin sandbox, SSRF on image/URL fetchers, path traversal in file ops).
- Docs, migration notes, plugin author guide.
- **Exit:** v1.0.0.

### Phase 5+ — Plugins for the other *arrs
Music (Lidarr), books (Readarr), subtitles (Bazarr). Depends on Q5 (media-type plugin design).

## 8. UI plan (rule 4)

- Next.js (App Router), TypeScript, `output: 'export'`, `trailingSlash: true`, no server features (no API routes, no middleware, no ISR, no `next/image` optimizer; use `unoptimized`).
- All data is fetched client-side from the Rust API. Dynamic routes (`/movies/[id]`) cannot be pre-rendered for unknown IDs. Use query params (`/movie?id=42`) or a catch-all shell with client routing. The server returns the shell for unknown paths.
- Typed API client generated from the OpenAPI spec (`openapi-typescript`) so UI and API cannot drift.
- Data layer: TanStack Query; WebSocket pushes invalidate cached queries.
- Components: Tailwind plus a headless set (Radix/shadcn). Dark mode by default.
- Plugin settings forms render from JSON Schema (e.g. `@rjsf` or a small in-house renderer).
- Build output is embedded into the Rust binary (`rust-embed`) with a `--ui-dir` override for development.
- Runtime URL base (`/sonarr`-style reverse-proxy prefix) must work with a static export. Plan: relative asset paths plus a small bootstrap script that reads the base from the server. Spike in Phase 0.

## 9. Cross-cutting concerns

| Area | Decision |
|---|---|
| Web framework | `axum` + `tokio` + `tower` |
| Serialization | `serde`; OpenAPI via `utoipa` |
| DB | SQLite + `sqlx`, migrations embedded, nightly backup via `VACUUM INTO` |
| HTTP client | `reqwest` (rustls, no OpenSSL) so the image can be `scratch`/distroless |
| Logging | `tracing`, JSON or pretty, rotating file |
| Config | file in `/config` + env overrides; settings editable in the UI |
| Auth | argon2 password hashes, session cookie, API key |
| Image | multi-stage Dockerfile; static musl binary; non-root user; PUID/PGID support; healthcheck |
| Testing | unit tests for parser/quality/decision using real-world release-name fixtures; integration tests with fake indexer/NNTP server/tracker; end-to-end with Playwright against the container |
| Licensing | Project is GPL-3.0. Check every dependency with `cargo deny` for GPL-3 compatibility. Ship notices for bundled 7-Zip, par2 and ffprobe. |
| Metadata API keys | Bundle project keys for TMDB with a user override. Check provider terms (Q9). |

## 10. Decisions and open questions

### Decided
- **Q1. Newznab/Torznab is built in.** It is the one standard indexer protocol in the core. Site-specific indexers and Prowlarr-style proxies are plugins.
- **Q2. Native REST API first.** A Sonarr/Radarr v3 compatibility layer is a later, optional plugin.
- **Q3. Plugins are WASM components** (§3). Phase 0 validates this.
- **Q4. TV metadata:** use the same TVDB proxy service Sonarr uses (SkyHook) as the primary source, with TMDB as the fallback. Movies: see §13.3 (Radarr's own metadata server is the equivalent of SkyHook). Consequences:
  - Metadata providers sit behind a `MetadataProvider` trait with a fallback chain, so TVDB-direct or other sources can be added as plugins.
  - SkyHook is run by a third party. Check its availability and terms in Phase 0, and cache aggressively so short outages do not block the app.
  - Episode IDs must be stable across providers. Store the provider and the external ID on every series/episode row, and keep a mapping table for fallback cases.
- **Q5. Media-type plugins (music/books) are deferred.** Keep the core schema and routes modular so this stays possible.
- **Q6. Bundled binaries:** `7zz` (extraction, all archive types), `par2` (verify/repair) and `ffprobe` (media info; Sonarr and Radarr both shell out to FFmpeg's probe, and it is the only practical way to get HDR, audio-channel and language data).

### Still open
- **Q7. Licence: GPL-3.0** (decided). Sonarr and Radarr are GPL-3.0, so we may port their logic, regex tables and test cases directly. Keep their copyright notices where code is copied. Project name: **OpenMediaProject** (decided).
- **Q8. Migration from Sonarr/Radarr:** not planned. Out of scope.
- **Q9. Bundled API keys:** yes, we ship keys, with a user override in Settings. What the reference code shows:
  - Radarr embeds a read-only TMDB v4 bearer token in `NzbDrone.Common/Cloud/RadarrCloudRequestBuilder.cs`, used only for TMDB-backed import lists (popular, keyword, list, person, company). Its movie metadata comes through `api.radarr.video`, not TMDB direct.
  - Sonarr embeds no TVDB or TMDB key. All TV metadata goes through SkyHook.
  - **Do not reuse Radarr's token.** It is their application's credential. Register our own TMDB application, ship that read-only token, and let users override it. TVDB needs no key while SkyHook is primary; if we call TVDB directly as a fallback, register a TVDB project key the same way.
- **Q10. Metadata servers: yes**, use them as the primary source (SkyHook for TV, the Radarr metadata server for movies), with TMDB/TVDB direct as the fallback. Check their terms in Phase 0.

All questions in this section are now decided.

## 11. Risks

| Risk | Impact | Mitigation |
|---|---|---|
| WASM component model immaturity | Plugin API churn | Phase 0 spike; keep the interface small; version it |
| Torrent engine quality | Core feature fails | Evaluate early; keep it behind a trait so it can be swapped |
| Bundled `7zz` / `par2` binaries | Larger image, per-arch builds, subprocess handling | Pin versions; build for amd64 and arm64 in CI; wrap behind a trait so a Rust replacement can slot in |
| "All features" scope creep | Never ships | Parity matrix in §4; every item has a test; defer by written decision only |
| Release-name parsing edge cases | Wrong grabs/imports | Large fixture corpus; property tests; fuzzing |
| Static export limits | UI constraints | Client-side data fetching only; spike routing and URL base early |
| Metadata provider terms/rate limits | Blocked requests | Caching, backoff, user-supplied keys |
| Single-container resource use | OOM on small NAS | Memory budgets per subsystem; measure in Phase 4 |

## 12. First actions

1. Create the Cargo workspace and CI skeleton (Phase 0).
2. Run the four spikes: plugin host, torrent engine, PAR2/RAR, Next.js static serving.
3. Revise this document with the spike results, then start Phase 1.

## 13. Findings from the Sonarr and Radarr source

Source read: `Sonarr/Sonarr` and `Radarr/Radarr` (default branches, shallow clones, read-only, outside this repo). Both are C# (.NET) with the same `NzbDrone.Core` skeleton, a React/Vite frontend, and about 600 test files each. Radarr is a fork of Sonarr, so they share most architecture. Items below say what to copy as a *behavior* and what to change.

### 13.1 Architecture patterns worth keeping
| Pattern in their code | Where | What we do |
|---|---|---|
| **Event aggregator + command queue.** Services publish events (`EpisodeGrabbedEvent`, `DownloadCompletedEvent`…) and run named commands with priorities (`CommandPriority`) and a persisted history. The UI shows a command list. | `Messaging/` | Same shape: in-process event bus + a persisted command queue with priority, dedupe (`CommandEqualityComparer`) and status. Commands are what the scheduler, API and UI all trigger. |
| **Persisted scheduled tasks** with an interval in minutes, last/next run. Defaults: refresh monitored downloads 1 min; RSS sync (user setting, min 10); import-list sync 5; health check 6 h; metadata refresh 12 h (series) / 24 h (movies); housekeeping 24 h; recycle-bin cleanup 24 h; backup (user setting); scene-mapping update 3 h; update check 6 h. | `Jobs/TaskManager.cs` | Seed the same defaults. Intervals are data, not code, so plugins can register tasks. |
| **Provider pattern.** Indexers, download clients, notifications and import lists all share one base: a *definition* row (name, implementation, JSON settings, tags, enable flags), a settings *contract* with validation, a *factory*, and a per-provider **status record with escalating back-off** after failures. | `ThingiProvider/`, `Indexers/IndexerStatusService`, `Download/DownloadClientStatusService` | This is exactly what the WASM plugin types in §3 need. One generic `Provider` table + one status/back-off service for all four types. A failing plugin gets disabled with back-off and shows up in Health. |
| **Specification-based decision engine.** Each rule is a small class returning accept/reject with a reason code and a `RejectionType` (`Permanent` or `Temporary`) and a `SpecificationPriority` (parsing → database → disk, so cheap checks run first). | `DecisionEngine/Specifications/` | Same: `trait Spec { fn priority(); fn evaluate() -> Accept | Reject{kind, reason} }`. Temporary rejections (e.g. delay, "already in queue") are what feed the **pending-release** table. |
| **Tracked-download state machine**: `Downloading → ImportPending → Importing → Imported`, plus `ImportBlocked`, `FailedPending`, `Failed`, `Ignored`, each with an Ok/Warning/Error status. A 1-minute refresh polls clients and advances states. | `Download/TrackedDownloads/` | Copy the states and the monitoring loop. Our built-in clients push progress instead of being polled, but they feed the same state machine. |
| **Pending releases**: releases that passed except for a temporary rejection are stored and re-evaluated each RSS sync. This is how delay profiles work. | `Download/Pending/` | Copy. |
| **Separate main and log databases**; 230 migrations in Sonarr, 141 in Radarr. | `Datastore/` | Use two SQLite files (main, logs) so log churn never blocks library writes. Do not support Postgres (rule 1). We start with a clean schema; no need to replay their migrations. |
| **Housekeepers**: ~30 small jobs that delete orphaned rows (history, blocklist, extra files, statuses, pending releases) and fix bad timestamps. | `Housekeeping/` | Use real foreign keys with `ON DELETE CASCADE/SET NULL` so most of these are unnecessary; keep a short housekeeping job for the rest. |
| **Realtime via SignalR** (`NzbDrone.SignalR`) | | WebSocket from axum with the same event names. |
| **Two API versions** (Sonarr has V3 and V5, each with its own OpenAPI file in `Sonarr.Api.V5/openapi.json`). | | Version the API from day one (`/api/v1`). |

### 13.2 Decision engine: what to port exactly
Specification list (Sonarr; Radarr has a near-identical subset). Port each as its own spec with its own tests:
- **Shared:** AcceptableSize, MaximumSize, MinimumAge, Retention, Blocklist, BlockedIndexer, Protocol, QualityAllowedByProfile, CustomFormatAllowedByProfile, UpgradeAllowed, Upgradable, UpgradeDisk, Repack, ReleaseRestrictions, TorrentSeeding, NotSample, RawDisk, FreeSpace, Queue, AlreadyImported.
- **RSS-sync only:** Delay, History, Monitored, Pending, Proper, IndexerTag, DeletedFile.
- **Search only:** episode-requested, season-match, single-episode-match, series.
- **Sonarr only:** AirDate, FullSeason, MultiSeason, SeasonPackOnly, SplitEpisode, SameEpisodes(+Grab), SceneMapping, AnimeVersionUpgrade.
- **Radarr only:** Language, HardcodedSubs, RequiredIndexerFlags.

**Release ranking** (`DownloadDecisionComparer`), first difference wins: quality (profile order, then revision), custom-format score, protocol (preferred wins), episode count and number (Sonarr), indexer priority, log-bucketed seeders/peers (torrent), age bucket (usenet), size closest to the preferred size. Exact rules are in `REFERENCE.md` §5.3. Copy them; users rely on this behavior.

**Delay profiles** (exact logic in `REFERENCE.md` §5.4): fields are `EnableUsenet`, `EnableTorrent`, `PreferredProtocol`, `UsenetDelay`, `TorrentDelay`, `Order`, `BypassIfHighestQuality`, `BypassIfAboveCustomFormatScore`, `MinimumCustomFormatScore`, `Tags`. Matching is by tag, ordered. Use this schema.

### 13.3 Metadata: neither project calls TMDB/TVDB directly
- Sonarr talks to `skyhook.sonarr.tv/v2/tvdb/…` (TV) and `services.sonarr.tv/v1/` (scene mappings, etc.).
- Radarr talks to `api.radarr.video/v1/…` (movie metadata, lists) and `radarr.servarr.com/v1/` (services).
- Both cache and normalize provider data server-side, which is why their clients are small.
- **Consequence for our plan:** the "SkyHook primary, TMDB fallback" decision (Q4) should apply to movies too: **Radarr's metadata server primary, TMDB fallback**. This also means we need a direct TMDB/TVDB client as the fallback anyway, so we must handle their raw shapes ourselves. Scene mappings (`services.sonarr.tv`) matter for anime and season/episode remaps, so the same fallback question applies there: cache the last good copy on disk.
- Both servers are third-party-run. Add a "metadata source unreachable" Health check and never block a library read on a metadata call.
- Decided (Q10): we use these servers as primary. Phase 0 checks their terms.

### 13.4 Parsing is the bulk of the correctness risk
- Sonarr `Parser.cs` has ~149 regex patterns (series/episode forms, daily, anime, absolute, season packs, multi-episode, specials) plus `QualityParser` (~790 lines), `LanguageParser`, `ReleaseGroupParser`, `SceneChecker`, `ParsingService` (matches a parsed title to a library item, including alternate and scene titles). Radarr's is much smaller (44 patterns, a ~640-line `Parser.cs`) because movies are simpler.
- They keep 31 parser test files in Sonarr (`NzbDrone.Core.Test/ParserTests`) of real-world release names.
- **Plan:** build `omp-parser` as a table-driven test corpus first. Generate test cases from their tests' *inputs and expected outputs* (facts about release names, not code) and from our own captured release names. Port patterns using Rust's `regex` crate where possible; where they need look-around or backreferences, use `fancy-regex` or rewrite. Keep a regression corpus that grows with every bug.
- Quality model: 22 qualities with IDs and a (source, resolution) pair, e.g. `WEBDL-1080p`, `Bluray-2160p Remux`, `Raw-HD`. Quality plus **revision** (real/proper/version) is the unit of comparison. Radarr and Sonarr use different ids for qualities 20 to 22, so we cannot keep both. We use Radarr's table (it is a superset; Sonarr's "Bluray-1080p Remux" is our `Remux-1080p`) and its names. No importer exists (Q8), so id compatibility does not matter beyond that.
- Custom-format condition types: edition, indexer flag, language, quality modifier, release group, release title, resolution, size, source, year (Radarr); Sonarr adds series-specific ones. All regex-based specs share one base class.
- Indexer flags are a bitfield (freeleech, halfleech, double upload, PTP golden, etc.). Torznab exposes these; keep the bitfield.

### 13.5 Naming and file handling
- Naming uses a token pattern like `{Series Title} - S{season:00}E{episode:00} - {Episode Title} {Quality Full}`. The tokenizer regex supports prefix/suffix separators, case variants (`{series title}` vs `{Series.Title}`), width formats (`{season:00}`), truncation (`{Series Title:30}`), and MediaInfo tokens (`{MediaInfo VideoDynamicRangeType}`). It also handles multi-episode style (extend, duplicate, range, scene…) and illegal-character replacement. Implement as a small parser with a unit-tested token table. Scope this early; users have strong opinions about it.
- Import decision engine has its own specs, separate from the grab specs: not sample, not unpacking, has audio track, matches folder, matches grab, already imported, upgrade, free space; Sonarr adds absolute-episode, episode title, full season, split episode, unverified scene numbering; Radarr adds not-multi-part, grabbed-release-quality. Import runs as: scan → parse → decide → move/hardlink/copy → update DB → recycle old file → emit events.
- Both have a **recycle bin**, **rejected file extensions**, **script import** hook, **file date** setting, and **extra files** (subs, nfo) handling. Include in Phase 1/2 scope.

### 13.6 Licence (decided: GPL-3.0)
- Sonarr and Radarr are GPL-3.0. We release under GPL-3.0 too, so porting their code, regex tables and test fixtures is allowed.
- When code is ported closely, keep the original copyright and licence notice in that file's header.
- Plugins: the WASM boundary is a process-like interface, so plugin authors may pick their own licence for plugin code. Document this in the plugin guide. (Have a lawyer confirm before v1.0 if this matters.)
- Third-party binaries (7-Zip, par2, ffprobe) are separate programs; ship their licence texts in the image.

### 13.7 Things in their code we deliberately drop or defer
- Postgres support, Windows/Mono service hosting, in-app self-updater (`NzbDrone.Update`), Analytics, `.NET` runtime patches (`Sonarr.RuntimePatches`).
- ~20 notification providers and ~15 download clients (including Plex, Trakt, Telegram, Aria2, Deluge, qBittorrent, rTorrent, Transmission, SABnzbd, NZBGet) become **plugins**. Note that Radarr and Sonarr both now list **RQBit** as a client, which is evidence that `librqbit` is a credible engine for §6.
- Specific indexers (BroadcastheNet, FileList, HDBits, IPTorrents, Nyaa, Torrentleech, Fanzub…) are plugins; Newznab, Torznab and generic RSS stay built in.
- Trakt, Simkl, AniList, MyAnimeList, Plex, Sonarr-as-list import lists become plugins; generic RSS/custom-list URL stays built in.

### 13.8 Plan changes triggered by this review
1. §2: add `omp-commands` (command queue + scheduler persistence) and make `omp-plugin-host` reuse one **Provider** table/status/back-off model (§13.1).
2. §4.3/§4.4: add pending releases, rejection type (permanent/temporary), spec priority, and the exact ranking order (§13.2).
3. §4.5: add recycle bin, rejected extensions, script-import hook, file-date setting.
4. §7 Phase 0: add (a) check metadata-server terms and fallbacks (Q10) and (b) start the parser corpus.
5. §7 Phase 1: parser corpus and naming-token engine land **before** search/grab, because everything depends on them.
6. §9: add `ffprobe` to the bundled-binaries list, and a second SQLite file for logs.

