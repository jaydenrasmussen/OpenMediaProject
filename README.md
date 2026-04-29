# OpenMediaProject

> **One app to replace them all.**
> Sonarr + Radarr + Prowlarr + Overseerr — unified into a single, self-hosted application.

OpenMediaProject is a modern, self-contained media management platform. Search for movies and TV shows, automatically find and download the best available torrent, track download progress, and add completed media to Plex, Jellyfin, or Emby — all from one place, with zero external dependencies on other *arr applications.

---

## Vision

The *arr ecosystem is powerful but fragmented. A typical self-hosted media stack requires:

- **Overseerr** — request UI
- **Radarr** — movie management + indexer queries
- **Sonarr** — TV show management + indexer queries
- **Prowlarr** — indexer aggregation and proxy
- **A torrent client** — actual downloading
- **Plex/Jellyfin/Emby** — media server

Each of these is a separate application, with its own database, its own configuration, its own update cycle, and its own web UI. They communicate over internal APIs and are notoriously brittle to set up and keep in sync.

**OpenMediaProject replaces all of the management layer** (Overseerr, Radarr, Sonarr, Prowlarr) with a single Go binary and a single SvelteKit frontend, deployed as one Docker container. The only external services you still need are a torrent client and a media server.

---

## What It Does Today

| Capability | Status |
|---|---|
| Search movies via TMDB | ✅ |
| Search TV shows via TVDB | ✅ |
| Browse popular / now-playing / recent media | ✅ |
| Built-in torrent indexers (YTS, EZTV, PirateBay, 1337x) | ✅ |
| Pluggable indexer registry — add new sources without changing core | ✅ |
| Quality preference system (2160p → 1080p → 720p fallback) | ✅ |
| Release profile filtering (Require / Reject / Prefer keyword rules) | ✅ |
| One-click movie download pipeline | ✅ |
| One-click TV show / season / episode download pipeline | ✅ |
| Torrent client support (ruTorrent, Transmission, Deluge, qBittorrent, SABnzbd) | ✅ |
| Download progress tracking | ✅ |
| Quality upgrade monitoring | ✅ |
| Automatic file moving on completion | ✅ |
| Plex, Jellyfin, and Emby library scan on completion | ✅ |
| Notifications (Discord, Email, Ntfy, Gotify, Apprise, Telegram) | ✅ |
| Wanted queue — request media without triggering an immediate download | ✅ |
| Admin approve / reject requests | ✅ |
| Multi-user support with role-based access (admin / user) | ✅ |
| JWT session authentication + API key for programmatic access | ✅ |
| Activity history log | ✅ |
| Settings UI — configure at runtime, persisted across restarts | ✅ |
| Background task visibility (last run, next run, errors) | ✅ |
| TVDB episode refresh (daily background job) | ✅ |
| API key authentication | ✅ |
| Single Docker container deployment | ✅ |
| Embedded NutsDB — no external database required | ✅ |

---

## High-Level Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                        Docker Container                     │
│                                                             │
│  ┌──────────────────────────────────────────────────────┐   │
│  │                   SvelteKit Frontend                 │   │
│  │         (served as static files by the backend)      │   │
│  └───────────────────────┬──────────────────────────────┘   │
│                          │ HTTP / REST                      │
│  ┌───────────────────────▼──────────────────────────────┐   │
│  │                    Go / Fiber API                    │   │
│  │  API Key + JWT middleware on every request           │   │
│  │                                                      │   │
│  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐   │   │
│  │  │   Media     │  │  Download   │  │   Torrent   │   │   │
│  │  │  Handlers   │  │  Pipeline   │  │   Handlers  │   │   │
│  │  │(Movies/TV)  │  │             │  │             │   │   │
│  │  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘   │   │
│  │         │                │                │          │   │
│  │  ┌──────▼──────┐  ┌──────▼──────┐         │          │   │
│  │  │ TMDB / TVDB │  │  Indexer    │         │          │   │
│  │  │   Clients   │  │  Registry   │         │          │   │
│  │  └─────────────┘  │             │         │          │   │
│  │                   │ YTS  EZTV   │         │          │   │
│  │                   │ TPB  1337x  │         │          │   │
│  │                   │ [pluggable] │         │          │   │
│  │                   └──────┬──────┘         │          │   │
│  │                          │                │          │   │
│  │  ┌───────────────────────▼────────────────▼──────┐   │   │
│  │  │                   NutsDB                      │   │   │
│  │  │  movies | tvshows | downloads | wanted | …    │   │   │
│  │  └───────────────────────────────────────────────┘   │   │
│  └──────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
           │                              │
           ▼                              ▼
   ┌───────────────┐             ┌─────────────────┐
   │ Torrent Client│             │  Media Server   │
   │  (ruTorrent / │             │  (Plex /        │
   │  Transmission │             │   Jellyfin /    │
   │  / Deluge /   │             │   Emby)         │
   │  qBittorrent  │             └─────────────────┘
   │  / SABnzbd)   │
   └───────────────┘
```

### Key Design Decisions

- **Single binary, single container.** The Go backend compiles the SvelteKit frontend into its `public/` directory and serves it as static files. One `docker run` is all it takes.
- **Embedded database.** NutsDB is a pure-Go key/value store included in the binary. No Postgres, no Redis, no SQLite driver headaches.
- **Pluggable indexer registry.** Each torrent source implements a single `Indexer` interface. Adding a new source is one file and one `registry.Register()` call — no changes to the core.
- **Quality-aware scoring.** Search results from all indexers are deduplicated, scored by quality match and seeder count, and sorted before the best result is picked.
- **Stateful download tracking.** Every initiated download is recorded in NutsDB with its torrent hash, media type, and status. A background poller checks the torrent client every 5 minutes, moves completed files to the correct library folder, and triggers a media server scan — automatically.
- **Dual auth.** API key auth for programmatic/script access; JWT session tokens for browser use. Both layers run in parallel.

---

## Download Pipeline

```
User clicks "Download" (or WantedPoller finds a match)
        │
        ▼
Load movie/show from DB
        │
        ▼
Fan-out search → all enabled indexers (concurrent)
  ├── YTS         (movies, JSON API)
  ├── EZTV        (TV, JSON API)
  ├── PirateBay   (both, JSON API)
  └── 1337x       (both, HTML scrape)
        │
        ▼
Deduplicate by InfoHash, score by quality + seeders
Apply release profile filters (Require / Reject / Prefer)
        │
        ▼
Pick best result
        │
        ▼
Send to torrent client (magnet or .torrent file)
        │
        ▼
Save PendingDownload record (hash → media metadata)
Write history entry: torrent_added
        │
        ▼
        … (background poller, every 5 min) …
        │
        ▼
Torrent completes (state: seeding / finished)
        │
        ▼
Move files to media library (Movies/ or TVShows/)
Write history entry: download_complete
        │
        ▼
Mark download completed in DB
        │
        ▼
Scan all registered media servers (Plex / Jellyfin / Emby)
Send notifications (Discord / Email / Ntfy / Gotify / Apprise / Telegram)
```

---

## Project Structure

```
OpenMediaProject/
├── backend/
│   ├── api/                    # HTTP handlers and route registration
│   │   ├── download_handlers.go    # Download pipeline endpoints
│   │   ├── indexer_handlers.go     # Indexer management endpoints
│   │   ├── movie_handlers.go       # Movie search / browse endpoints
│   │   ├── tvshow_handlers.go      # TV show search / browse endpoints
│   │   ├── torrent_handlers.go     # Torrent client proxy + poller
│   │   ├── wanted_handlers.go      # Wanted queue + approve/reject
│   │   ├── history_handlers.go     # Activity history
│   │   ├── settings_handlers.go    # Persistent settings
│   │   ├── system_handlers.go      # Background task status
│   │   ├── jwt_auth_handlers.go    # Login, register, JWT issuance
│   │   ├── task_tracker.go         # In-memory task state
│   │   ├── wanted_poller.go        # Background wanted-queue poller
│   │   └── routes.go               # Central route registration
│   ├── clients/
│   │   ├── indexers/               # Pluggable torrent indexer system
│   │   │   ├── interface.go            # Indexer interface + types
│   │   │   ├── registry.go             # Fan-out, dedup, scoring
│   │   │   ├── quality.go              # Quality parsing and scoring
│   │   │   ├── yts.go                  # YTS (movies)
│   │   │   ├── eztv.go                 # EZTV (TV)
│   │   │   ├── piratebay.go            # The Pirate Bay (both)
│   │   │   └── leet1337x.go            # 1337x (both, scraper)
│   │   ├── notifications/          # Notification target implementations
│   │   │   ├── interface.go
│   │   │   ├── registry.go
│   │   │   ├── discord.go
│   │   │   ├── email.go
│   │   │   ├── ntfy.go
│   │   │   ├── gotify.go
│   │   │   ├── apprise.go
│   │   │   └── telegram.go
│   │   ├── media_server.go         # MediaServer interface
│   │   ├── media_server_registry.go # Fan-out registry
│   │   ├── plex.go                 # Plex API client
│   │   ├── jellyfin.go             # Jellyfin API client
│   │   ├── emby.go                 # Emby API client
│   │   ├── rutorrent.go            # ruTorrent torrent client
│   │   ├── transmission.go         # Transmission torrent client
│   │   ├── deluge.go               # Deluge torrent client
│   │   ├── qbittorrent.go          # qBittorrent Web API v2 client
│   │   ├── sabnzbd.go              # SABnzbd API client
│   │   ├── factory.go              # Torrent client factory
│   │   ├── tmdb.go                 # TMDB API client
│   │   └── tvdb.go                 # TVDB API client
│   ├── config/
│   │   └── config.go               # All configuration, env var loading
│   ├── db/
│   │   ├── db.go                   # NutsDB connection + bucket init
│   │   ├── movie_repository.go
│   │   ├── tvshow_repository.go
│   │   ├── episode_repository.go
│   │   ├── download_repository.go  # Pending download tracking
│   │   ├── wanted_repository.go    # Wanted queue
│   │   ├── history_repository.go   # Activity history
│   │   ├── config_repository.go    # Persistent settings
│   │   ├── indexer_repository.go   # Saved indexer configs
│   │   └── user_repository.go
│   ├── middleware/
│   │   └── jwt.go                  # JWTMiddleware, RequireRole
│   └── main.go
├── frontend/                   # SvelteKit application
│   └── src/
│       ├── routes/
│       │   ├── +layout.svelte      # Global nav
│       │   ├── discover/           # Trending media
│       │   ├── search/             # Unified search
│       │   ├── movies/             # Movie detail + download
│       │   ├── tvshows/            # TV show detail + download
│       │   ├── library/            # Downloaded media
│       │   ├── wanted/             # Request queue
│       │   ├── history/            # Activity log
│       │   ├── settings/           # Settings UI
│       │   ├── system/             # Background task status
│       │   └── users/              # User management
│       └── lib/
│           ├── api/index.ts        # Typed API client
│           └── components/         # Shared UI components
├── docs/
│   ├── api-authentication.md
│   ├── configuration-api.md
│   ├── indexers.md
│   ├── torrent-clients.md
│   └── architecture-and-roadmap.md
├── Dockerfile
├── docker-compose.yml
└── Taskfile.yml
```

---

## Roadmap

### ✅ Complete

- All of **Priority 2** (Discovery UI, Notifications, TVDB refresh)
- All of **Priority 4** (Jellyfin/Emby, qBittorrent/SABnzbd, Activity History, Settings UI, Task Visibility, Multi-User + JWT)
- From **Priority 1**: P1.2 Wanted Queue, P1.4 Quality Upgrades
- From **Priority 3**: P3.3 Release Profile Filtering
- **P1.1** TV Show Episode & Season Tracking — per-episode status model, `GET /api/v1/tvshows/:id/episodes` and season endpoints, `POST /api/v1/tvshows/:id/monitor` to queue all undownloaded episodes, episode status lifecycle (missing → downloading → downloaded)
- **P1.3** Structured File & Library Management — `Movies/Title (Year)/Title (Year).mkv` naming, TV `Season NN/Show - S01E01 - Title.mkv` layout, subtitle and extras handling, duplicate detection before every download

---

## Getting Started

### Docker (recommended)

```bash
docker run -d \
  --name openmedia \
  -p 8080:8080 \
  -v /your/data:/app/data \
  -v /your/media/movies:/movies \
  -v /your/media/tvshows:/tvshows \
  -e TMDB_API_KEY=your_tmdb_key \
  -e TVDB_API_KEY=your_tvdb_key \
  -e TORRENT_CLIENT_TYPE=transmission \
  -e TORRENT_CLIENT_URL=http://transmission:9091 \
  -e TORRENT_CLIENT_USERNAME=admin \
  -e TORRENT_CLIENT_PASSWORD=password \
  -e PLEX_URL=http://plex:32400 \
  -e PLEX_TOKEN=your_plex_token \
  -e MOVER_MOVIE_DIR=/movies \
  -e MOVER_TVSHOW_DIR=/tvshows \
  -e ENABLE_AUTH=true \
  -e ADMIN_PASSWORD=changeme \
  ghcr.io/yourusername/openmediaproject:latest
```

Then open `http://localhost:8080` and log in with username `admin` and your chosen password. If `ADMIN_PASSWORD` is not set, a random password is printed once in the startup logs.

### Docker Compose

```bash
git clone https://github.com/yourusername/OpenMediaProject.git
cd OpenMediaProject
cp .env.example .env   # edit with your values
docker compose up -d
```

### Local Development

```bash
# Backend (hot reload via Air)
cd backend && air

# Frontend (hot reload via Vite)
cd frontend && npm install && npm run dev
```

---

## Environment Variables

### Required

| Variable | Description |
|---|---|
| `TMDB_API_KEY` | API key from [themoviedb.org](https://www.themoviedb.org/settings/api) |
| `TVDB_API_KEY` | API key from [thetvdb.com](https://thetvdb.com/api-information) |
| `TORRENT_CLIENT_TYPE` | `rutorrent`, `transmission`, `deluge`, `qbittorrent`, or `sabnzbd` |
| `TORRENT_CLIENT_URL` | Base URL of your torrent client |

### Torrent Client

| Variable | Description | Default |
|---|---|---|
| `TORRENT_CLIENT_USERNAME` | Client username | — |
| `TORRENT_CLIENT_PASSWORD` | Client password (or SABnzbd API key) | — |

### Media Library

| Variable | Description | Default |
|---|---|---|
| `MOVER_MOVIE_DIR` | Destination directory for movies | — |
| `MOVER_TVSHOW_DIR` | Destination directory for TV shows | — |

### Media Servers

| Variable | Description | Default |
|---|---|---|
| `PLEX_URL` | Plex server URL | — |
| `PLEX_TOKEN` | Plex authentication token | — |
| `JELLYFIN_URL` | Jellyfin server URL | — |
| `JELLYFIN_API_KEY` | Jellyfin API key | — |
| `EMBY_URL` | Emby server URL | — |
| `EMBY_API_KEY` | Emby API key | — |

### Notifications

| Variable | Description |
|---|---|
| `DISCORD_WEBHOOK_URL` | Discord webhook URL |
| `NTFY_TOPIC_URL` | Ntfy topic URL (e.g. `https://ntfy.sh/mytopic`) |
| `GOTIFY_URL` | Gotify server base URL |
| `GOTIFY_TOKEN` | Gotify application token |
| `APPRISE_URL` | Apprise endpoint URL |
| `EMAIL_SMTP_HOST` | SMTP hostname |
| `EMAIL_SMTP_PORT` | SMTP port (default `587`) |
| `EMAIL_USERNAME` | SMTP username |
| `EMAIL_PASSWORD` | SMTP password |
| `EMAIL_FROM` | Sender address |
| `EMAIL_TO` | Recipient address |
| `TELEGRAM_BOT_TOKEN` | Telegram bot token (from BotFather) |
| `TELEGRAM_CHAT_ID` | Telegram chat or channel ID to send messages to |

### Setting up Telegram Notifications

1. **Create a bot** — open Telegram and start a chat with [@BotFather](https://t.me/BotFather).
2. Send `/newbot` and follow the prompts to choose a name and username for your bot.
3. BotFather will reply with a token that looks like `123456789:ABCdef...`. Set this as `TELEGRAM_BOT_TOKEN`.
4. **Get your chat ID** — start a conversation with your new bot (send it any message), then open this URL in a browser, replacing `<token>` with your token:
   ```
   https://api.telegram.org/bot<token>/getUpdates
   ```
   Find the `"chat"` object in the JSON response and copy its `"id"` value. Set that as `TELEGRAM_CHAT_ID`.
   > For a public channel, use the channel username prefixed with `@` (e.g. `@mychannel`). For a private channel or group, add the bot as an admin first and use the numeric ID from `getUpdates`.
5. Set both variables and restart (or redeploy) the container. OpenMediaProject will send a Telegram message on each completed download.

### Download Quality

| Variable | Description | Default |
|---|---|---|
| `PREFERRED_QUALITY` | `2160p`, `1080p`, `720p`, `480p`, or `any` | `1080p` |
| `QUALITY_FALLBACK` | Comma-separated fallback order | `1080p,720p,any` |

### Indexers (optional bootstrap)

| Variable | Description |
|---|---|
| `INDEXER_URL` | Torznab endpoint URL for a custom indexer |
| `INDEXER_API_KEY` | API key for the custom indexer |
| `INDEXER_NAME` | Display name for the custom indexer |

### Security & Server

| Variable | Description | Default |
|---|---|---|
| `ENABLE_AUTH` | Enable API key authentication | `false` |
| `API_KEY` | Set a fixed API key (auto-generated if blank) | — |
| `ALLOW_API_KEY_QUERY` | Allow `?api_key=` query param | `false` |
| `JWT_SECRET` | HMAC secret for session JWTs (random per-process if unset) | — |
| `ADMIN_PASSWORD` | Password for the auto-created `admin` user | random (logged once) |
| `SERVER_ADDRESS` | Listen address | `:8080` |
| `DB_PATH` | Path to NutsDB data directory | `./data/nutsdb` |
| `LOG_LEVEL` | `debug`, `info`, `warn`, `error` | `info` |
| `CORS_ALLOW_ORIGINS` | Allowed CORS origins | `*` |

---

## API Overview

Full documentation for each group is in the `docs/` directory.

| Group | Base Path | Description |
|---|---|---|
| Auth | `/api/v1/auth` | API key management, login, register |
| Config | `/api/v1/config` | System configuration (in-memory) |
| Settings | `/api/v1/settings` | Persistent settings (survives restart) |
| Movies | `/api/v1/movies` | Search, browse, download movies |
| TV Shows | `/api/v1/tvshows` | Search, browse, download TV shows |
| Wanted | `/api/v1/wanted` | Request queue + admin approve/reject |
| Downloads | `/api/v1/downloads` | Download status |
| Torrents | `/api/v1/torrents` | Torrent client proxy |
| Indexers | `/api/v1/indexers` | Indexer management and search |
| History | `/api/v1/history` | Activity log |
| System | `/api/v1/system` | Background task status |
| Plex | `/api/v1/plex` | Plex configuration |

### Core Endpoints

```
# Authentication
POST /api/v1/auth/login          { username, password } → { token, user_id, role }
POST /api/v1/auth/register       { username, password, email }

# Download
POST /api/v1/movies/:id/download?quality=1080p
POST /api/v1/tvshows/:id/download?season=1&episode=1&quality=1080p
POST /api/v1/tvshows/:id/monitor
GET  /api/v1/downloads

# Request queue
POST /api/v1/movies/:id/request
POST /api/v1/tvshows/:id/request
GET  /api/v1/wanted
POST /api/v1/wanted/:id/approve  (admin only)
POST /api/v1/wanted/:id/reject   (admin only)

# Persistent settings
GET  /api/v1/settings
PUT  /api/v1/settings

# Activity & tasks
GET  /api/v1/history
GET  /api/v1/system/tasks
```

---

## Adding a New Indexer

Implement the `Indexer` interface in `backend/clients/indexers/`:

```go
type Indexer interface {
    Name()     string
    Supports() MediaType   // MediaTypeMovie | MediaTypeTV | MediaTypeBoth
    Priority() int         // lower = higher priority
    Enabled()  bool
    SetEnabled(bool)
    Search(req *SearchRequest) ([]*SearchResult, error)
}
```

Then register it in `NewDefaultRegistry()` in `registry.go`:

```go
r.Register(NewMyNewIndexer())
```

That's it. The fan-out, deduplication, quality scoring, and sorting are all handled by the registry.

---

## Docs

- [API Authentication](docs/api-authentication.md)
- [Configuration API](docs/configuration-api.md)
- [Torrent Client Setup](docs/torrent-clients.md)
- [Indexer System](docs/indexers.md)
- [Architecture & Roadmap](docs/architecture-and-roadmap.md)

---

## License

MIT
