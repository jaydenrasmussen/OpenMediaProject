# Architecture & Roadmap

> Deep-dive reference for contributors and anyone evaluating OpenMediaProject.

---

## 1. The Problem

The dominant self-hosted media automation stack in 2024 looks like this:

```
Overseerr  →  Radarr / Sonarr  →  Prowlarr  →  Torrent Client  →  Plex
(requests)    (library mgmt)      (indexers)    (downloading)      (serving)
```

Each node in that chain is a separate application:

- Separate Docker containers
- Separate databases (SQLite × 4)
- Separate web UIs (× 4)
- Separate update cycles
- Complex inter-app API wiring that breaks whenever one updates
- Duplicated concepts (every app has its own notion of a "quality profile")

The total RAM footprint of a standard *arr stack is typically 600 MB–1.2 GB at idle.

**OpenMediaProject collapses the entire management layer into a single Go binary + SvelteKit frontend.** One container. One database. One UI. The only external services required are a torrent client and a media server — both of which are fundamentally hard to replace because they handle raw data.

---

## 2. What Each Tool Does and How We Replace It

### 2.1 Overseerr → Request & Discovery UI

Overseerr's job is to give users a friendly interface to request media and see what's been requested. It does nothing else — it just calls Radarr/Sonarr's APIs to action those requests.

**We replace this with:**
- TMDB and TVDB search built directly into our API and frontend
- A `wanted` queue that sits between "user expressed interest" and "download started"
- A request system where non-admin users can request media without triggering an immediate download
- A Discovery page backed by our existing TMDB popular/trending endpoints
- Admin approve/reject workflow for pending requests

### 2.2 Radarr → Movie Management

Radarr monitors movies, queries indexers for them, picks the best release based on quality profiles, sends it to the torrent client, renames the file on completion, and refreshes Plex.

**We replace this with:**
- TMDB client for movie metadata (already built)
- Built-in indexer registry with YTS, PirateBay, 1337x (already built)
- Quality scoring and fallback system (already built)
- `POST /api/v1/movies/:id/download` pipeline (already built)
- `FileMover` for post-download file organisation (partially built — needs structured renaming)
- Media server scan on completion via `MediaServerRegistry` (Plex, Jellyfin, Emby)

### 2.3 Sonarr → TV Show Management

Sonarr is Radarr's TV equivalent but significantly more complex: it tracks individual episodes, monitors shows for new airings, and handles season packs vs. individual episodes.

**We replace this with:**
- TVDB client for show metadata (already built)
- Season/episode data model (not yet built — highest priority gap)
- Per-episode wanted/downloaded status tracking (not yet built)
- Scheduled job to check for new episodes and trigger downloads (not yet built)
- `POST /api/v1/tvshows/:id/download?season=1&episode=1` pipeline (already built)

### 2.4 Prowlarr → Indexer Aggregation

Prowlarr's job is to manage a list of torrent indexers and expose them all through a unified Torznab API that Radarr and Sonarr query.

**We replace this with:**
- A pluggable `Indexer` interface (already built)
- Four built-in indexers: YTS, EZTV, PirateBay, 1337x (already built)
- A concurrent fan-out registry that queries all enabled indexers and aggregates results (already built)
- Quality-aware scoring and deduplication (already built)
- The indexer registry lives in-process — no HTTP proxying required

---

## 3. Current Architecture

### 3.1 Repository Layout

```
OpenMediaProject/
├── backend/                         # Go 1.25, module: "backend"
│   ├── main.go                      # Entry point, wiring, admin bootstrap, graceful shutdown
│   ├── api/
│   │   ├── routes.go                # All route registration in one place
│   │   ├── download_handlers.go     # Download pipeline (search → grab → track)
│   │   ├── indexer_handlers.go      # Indexer CRUD + ad-hoc search/grab
│   │   ├── movie_handlers.go        # Movie browse, search, delete
│   │   ├── tvshow_handlers.go       # TV show browse, search, delete
│   │   ├── torrent_handlers.go      # Torrent client proxy + completion poller
│   │   ├── plex_handlers.go         # Plex config CRUD
│   │   ├── config_handlers.go       # System config read/write
│   │   ├── auth_handlers.go         # API key management
│   │   ├── jwt_auth_handlers.go     # JWT login, register, token issuance
│   │   ├── wanted_handlers.go       # Wanted queue CRUD + approve/reject
│   │   ├── history_handlers.go      # Activity history read
│   │   ├── settings_handlers.go     # Persistent settings read/write
│   │   ├── system_handlers.go       # Background task status
│   │   ├── task_tracker.go          # In-memory TaskTracker
│   │   ├── wanted_poller.go         # Background wanted-queue poller
│   │   ├── tvdb_refresh.go          # Background TVDB episode refresh
│   │   ├── mover_handlers.go        # File mover controls
│   │   └── handlers.go              # Shared handler utilities
│   ├── clients/
│   │   ├── indexers/                # Pluggable torrent indexer system
│   │   │   ├── interface.go         # Indexer interface, SearchRequest, SearchResult, Quality types
│   │   │   ├── registry.go          # Concurrent fan-out, dedup, sorting
│   │   │   ├── quality.go           # ParseQuality(), QualityScore()
│   │   │   ├── yts.go               # YTS JSON API (movies, priority 1)
│   │   │   ├── eztv.go              # EZTV JSON API (TV, priority 2)
│   │   │   ├── piratebay.go         # apibay.org JSON API (both, priority 3)
│   │   │   └── leet1337x.go         # 1337x HTML scraper (both, priority 4)
│   │   ├── notifications/           # Notification target implementations
│   │   │   ├── interface.go         # Notifier interface + NotificationEvent
│   │   │   ├── registry.go          # NotificationRegistry fan-out
│   │   │   ├── discord.go
│   │   │   ├── email.go
│   │   │   ├── ntfy.go
│   │   │   ├── gotify.go
│   │   │   └── apprise.go
│   │   ├── factory.go               # TorrentClient factory (switch on ClientType)
│   │   ├── torrent.go               # TorrentClient interface + TorrentInfo types
│   │   ├── rutorrent.go             # ruTorrent XML-RPC client
│   │   ├── transmission.go          # Transmission RPC client
│   │   ├── deluge.go                # Deluge Web API client
│   │   ├── qbittorrent.go           # qBittorrent Web API v2 client
│   │   ├── sabnzbd.go               # SABnzbd API client
│   │   ├── media_server.go          # MediaServer interface
│   │   ├── media_server_registry.go # MediaServerRegistry (Register + Scan)
│   │   ├── plex.go                  # Plex API client (implements MediaServer)
│   │   ├── jellyfin.go              # Jellyfin API client (implements MediaServer)
│   │   ├── emby.go                  # Emby API client (implements MediaServer)
│   │   ├── torrent_file.go          # .torrent file parsing utilities
│   │   ├── file_mover.go            # Post-download file moving
│   │   ├── tmdb.go                  # TMDB REST client
│   │   └── tvdb.go                  # TVDB REST client
│   ├── config/
│   │   └── config.go                # Config struct + env var loading
│   ├── db/
│   │   ├── db.go                    # NutsDB connection, bucket init
│   │   ├── movie_repository.go
│   │   ├── tvshow_repository.go
│   │   ├── episode_repository.go
│   │   ├── download_repository.go   # PendingDownload CRUD
│   │   ├── indexer_repository.go    # Saved indexer configs CRUD
│   │   ├── wanted_repository.go     # WantedItem CRUD
│   │   ├── history_repository.go    # HistoryEntry append + list
│   │   ├── config_repository.go     # Persistent settings map
│   │   ├── release_profile_repository.go
│   │   ├── plex_repository.go
│   │   ├── mover_repository.go
│   │   └── user_repository.go
│   ├── models/
│   │   ├── movie.go                 # Movie struct + TMDB mapping
│   │   ├── tvshow.go                # TVShow struct + TVDB mapping
│   │   ├── episode.go               # Episode struct
│   │   ├── download.go              # PendingDownload + status constants
│   │   ├── wanted.go                # WantedItem + status constants
│   │   ├── history.go               # HistoryEntry + event type constants
│   │   ├── indexer.go               # Indexer config + search result models
│   │   ├── release_profile.go       # ReleaseProfile model
│   │   ├── user.go                  # User model (role, password_hash, NotificationPreferences)
│   │   ├── plex.go                  # Plex config model
│   │   └── mover.go                 # FileMover config model
│   ├── middleware/
│   │   ├── jwt.go                   # JWTMiddleware, RequireRole
│   │   └── (auth, logging)
│   └── logger/
├── frontend/                        # SvelteKit, TailwindCSS, shadcn-svelte
│   └── src/
│       ├── routes/
│       │   ├── +layout.svelte       # Global nav (includes History, Settings, System)
│       │   ├── discover/
│       │   ├── search/
│       │   ├── movies/
│       │   ├── tvshows/
│       │   ├── library/
│       │   ├── wanted/
│       │   ├── history/             # Activity history page
│       │   ├── settings/            # Settings UI page
│       │   ├── system/              # Background task status page
│       │   └── users/
│       └── lib/
│           ├── api/index.ts         # Typed API client (includes historyApi, systemApi)
│           └── components/
├── docs/
│   ├── api-authentication.md
│   ├── configuration-api.md
│   ├── torrent-clients.md
│   ├── indexers.md
│   └── architecture-and-roadmap.md  # This file
├── Dockerfile                       # Multi-stage: builds frontend, embeds in Go binary
├── docker-compose.yml
└── Taskfile.yml
```

### 3.2 Request Lifecycle (Download Pipeline)

```
Client
  │
  │  POST /api/v1/movies/:id/download?quality=1080p
  ▼
DownloadHandler.DownloadMovie()
  │
  ├─ Load Movie from NutsDB
  │
  ├─ Build SearchRequest { Title, Quality, Year, MediaType: movie }
  │
  ├─ indexers.Registry.Search(req)          ← concurrent fan-out
  │     ├─ YTS.Search()       ─────────────────┐
  │     ├─ PirateBay.Search() ─────────────────┤ goroutines
  │     └─ [EZTV skipped — TV only]            │
  │                                            │ results via channel
  │     ◄──────────────────────────────────────┘
  │     Deduplicate by lowercase InfoHash
  │     Score: QualityScore(preferred, available) + Seeders + Priority
  │     Sort descending
  │
  ├─ Pick results[0]
  │
  ├─ grabTorrent(torrentClient, best)
  │     ├─ if MagnetURL:     client.AddTorrentURL(magnet, true)
  │     └─ if DownloadURL:   GET url → client.AddTorrentFile(bytes, true)
  │
  ├─ if InfoHash != "":
  │     db.DownloadRepository.Save(PendingDownload{
  │         Hash, MediaType:"movie", MediaID, Title,
  │         Quality, Source, Status:"downloading"
  │     })
  │
  └─ 202 Accepted { success, title, quality, source, hash }


Background Goroutine (every 5 min)
  │
  TorrentHandler.CheckForCompletedTorrents()
  │
  ├─ client.GetTorrents()
  │
  ├─ for each torrent where State == "seeding" || "finished":
  │     │
  │     ├─ downloadRepo.GetByHash(hash)
  │     │     ├─ found, status != completed:
  │     │     │     ├─ client.GetTorrentFiles(hash)
  │     │     │     ├─ fileMover.MoveFile(file, download.MediaType)  ← TV or movie dir
  │     │     │     ├─ downloadRepo.UpdateStatus(hash, "completed")
  │     │     │     └─ historyRepo.Add(HistoryEntry{download_complete})
  │     │     ├─ ErrNotFound:
  │     │     │     └─ legacy path: move as "movie" (backward compat)
  │     │     └─ other DB error: log + skip
  │     │
  │     └─ set plexScanNeeded = true
  │
  └─ if plexScanNeeded: mediaServers.Scan()   ← all registered servers notified
```

### 3.3 Indexer Architecture

The indexer system is designed to be extended without touching core code.

```
clients/indexers/interface.go
  └── type Indexer interface {
          Name()        string
          Supports()    MediaType   // movie | tv | both
          Priority()    int
          Enabled()     bool
          SetEnabled(bool)
          Search(*SearchRequest) ([]*SearchResult, error)
      }

clients/indexers/registry.go
  └── type Registry struct { indexers []Indexer }
      ├── Register(Indexer)
      ├── All() []Indexer
      ├── Enabled() []Indexer
      └── Search(*SearchRequest) ([]*SearchResult, []error)
              ├── Filter by Enabled() && Supports(req.MediaType)
              ├── Fan out concurrently (one goroutine per indexer)
              ├── Collect via buffered channel
              ├── Deduplicate: strings.ToLower(InfoHash) → keep higher-seeder copy
              └── Sort: QualityScore desc → Seeders desc → Priority asc

Quality scoring (quality.go):
  QualityFallbackOrder = [1080p, 720p, 480p, any]
  2160p treated as index -1 (above all others)
  QualityScore(preferred, available):
    exact match → 100
    preferred == any → 80 (everything equally good)
    distance-based → 80 - (distance × 20), floor 0
```

**Adding a new indexer:** create `clients/indexers/mysource.go`, implement `Indexer`, call `r.Register(NewMySourceIndexer())` in `NewDefaultRegistry()`. Nothing else changes.

### 3.4 Media Server Architecture

All three supported media servers implement a single `MediaServer` interface. `TorrentHandler` holds a `*MediaServerRegistry` and calls `Scan()` once per completion batch, which fans out to every registered server.

```go
// clients/media_server.go
type MediaServer interface {
    Name() string
    ScanLibrary() error
}

// clients/media_server_registry.go
type MediaServerRegistry struct { servers []MediaServer }
    Register(MediaServer)
    Scan()   // calls ScanLibrary() on all registered servers; logs errors
```

At startup `main.go` constructs the registry and conditionally registers Plex, Jellyfin, and Emby based on which env vars are set.

### 3.5 Authentication Architecture

Two auth layers run in parallel. Both are registered globally in `main.go` before any route handlers.

```
Request
  │
  ▼
APIKeyMiddleware          (middleware/auth.go)
  ├─ Checks X-API-Key header (or query param if ALLOW_API_KEY_QUERY=true)
  ├─ Passes if key matches or if endpoint is in SKIP_AUTH_ENDPOINTS
  └─ 401 if auth enabled and key missing/wrong
  │
  ▼
JWTMiddleware             (middleware/jwt.go)
  ├─ If no Bearer token → Next() (lenient)
  ├─ If valid token → sets Locals: jwt_user_id, jwt_username, jwt_role
  └─ If invalid/expired token → 401
  │
  ▼
Route Handler
  ├─ Public routes: no further checks
  ├─ Admin-only routes (approve/reject): RequireRole("admin") middleware
  │     └─ Reads jwt_role from Locals; 403 if not "admin"
  └─ Programmatic routes: API key from step 1 is sufficient
```

**Admin bootstrap:** on startup, if no user with `role=admin` exists in the database, `main.go` creates an `admin` account. The password is read from `ADMIN_PASSWORD` env var; if absent, a random 24-character hex password is generated and logged once at `WARN` level. Change it immediately on first login.

### 3.6 Data Layer

NutsDB is an embedded, pure-Go key/value store. All data is stored as JSON-marshalled values in named buckets. There is no query language — all filtering is done in Go after a full-bucket scan.

```
Bucket              Key                     Value
─────────────────   ─────────────────────   ──────────────────────────────
movies              tmdb_id                 JSON(Movie)
tvshows             tvdb_id                 JSON(TVShow)
episodes            <show_id>:<s>:<ep>      JSON(Episode)
pending_downloads   torrent_hash            JSON(PendingDownload)
wanted              nanoid                  JSON(WantedItem)
history             nanoid                  JSON(HistoryEntry)
indexers            nanoid                  JSON(Indexer)
release_profiles    nanoid                  JSON(ReleaseProfile)
config              "settings"              JSON(map[string]interface{})
plex                "config"                JSON(PlexConfig)
users               user_id                 JSON(User)
mover               "config"                JSON(MoverConfig)
```

Each bucket has a corresponding `*Repository` struct in `db/` that wraps NutsDB transactions and exposes typed CRUD methods. The `_init` key in each bucket is a timestamp written at startup and is skipped by all `GetAll` implementations.

**Limitation to be aware of:** NutsDB has no secondary indices, joins, or range queries. As the data model grows (especially once episodes are tracked), a full-bucket scan for every episode lookup will become a performance concern. The current approach is fine for a personal media server with hundreds of shows, but a migration path to SQLite (via `modernc.org/sqlite`, a pure-Go driver requiring no CGo) should be considered before implementing episode-level tracking.

### 3.7 Configuration

All configuration is loaded at startup from environment variables via `config.Load()`. No config file is required. Key/value pairs saved through the `/api/v1/settings` endpoints are persisted in the `config` NutsDB bucket **and** applied immediately to the live `*config.Config` object, so most changes take effect without a restart.

Sensitive credentials (`PLEX_TOKEN`, `TORRENT_CLIENT_PASSWORD`, SMTP passwords, notification tokens) are intentionally excluded from the settings UI and must be set via environment variables.

```
Config
├── ServerAddress, CorsAllowOrigins, Environment, LogLevel
├── Database.DBPath
├── Auth { APIKey, EnableAuth, AllowQueryParam, SkipAuthEndpoints }
├── TVDB { APIKey }
├── TMDB { APIKey }
├── TorrentClient { ClientType, BaseURL, Username, Password }
├── Plex { URL, Token }
├── Jellyfin { URL, APIKey }
├── Emby { URL, APIKey }
├── FileMover { MovieDir, TVShowDir }
├── Downloads { PreferredQuality, QualityFallback[] }
├── Notifications { DiscordWebhookURL, NtfyTopicURL, GotifyURL, GotifyToken,
│                   AppriseURL, EmailSMTPHost, EmailSMTPPort, EmailUsername,
│                   EmailPassword, EmailFrom, EmailTo }
└── Indexers[] { URL, APIKey, Name }   ← env-var bootstrap for first indexer
```

---

## 4. Roadmap

Priorities are ordered by how much they block the core "replace the *arr stack" goal.

---

### Priority 1 — Core Functionality Gaps 🔴

These are blockers. The app cannot reliably replace Sonarr/Radarr without them.

#### 1.1 — TV Episode & Season Data Model

**Replaces: Sonarr's episode tracking**

Currently `TVShow` is a flat record with no children. Sonarr's core value is per-episode status tracking.

What needs to be built:

```go
// models/episode.go
type Episode struct {
    ID          string    // TVDB episode ID
    ShowID      string    // parent TVShow.ID
    Season      int
    Number      int       // episode number within season
    Title       string
    AirDate     string    // "YYYY-MM-DD"
    Overview    string
    Status      string    // "wanted" | "downloading" | "downloaded" | "skipped" | "missing"
    DownloadedAt *time.Time
    Quality     string
    FetchedAt   time.Time
}
```

New DB bucket: `episodes`, keyed by `<show_id>:<season>:<episode>` for efficient prefix lookups.

The TVDB client already exists and can be extended to fetch episode lists (`GET /series/:id/episodes`). A new `EpisodeRepository` follows the same pattern as `MovieRepository`.

New API endpoints:
```
GET  /api/v1/tvshows/:id/episodes              → all episodes for a show
GET  /api/v1/tvshows/:id/seasons/:n/episodes   → episodes for one season
PUT  /api/v1/tvshows/:id/episodes/:eid         → update episode status (e.g. mark skipped)
```

#### 1.2 — Wanted / Monitored Queue

**Replaces: Sonarr's and Radarr's monitoring system** — **Status: Partially complete.**

The `WantedItem` model, `wanted` bucket, request endpoints, background poller, and approve/reject workflow are all implemented. What remains:

- Release date threshold — don't download until a movie's release date is reached
- "Monitor all" mode — when a show is added, automatically add all future episodes as wanted items

#### 1.3 — Structured File & Library Management

**Replaces: Sonarr's and Radarr's file renaming and library organisation**

The current `FileMover` moves files to flat `Movies/` and `TVShows/` directories. That's not how media servers expect content.

Required naming conventions:
```
# Movies
Movies/
  Inception (2010)/
    Inception (2010).mkv

# TV Shows
TVShows/
  Breaking Bad/
    Season 01/
      Breaking Bad - S01E01 - Pilot.mkv
      Breaking Bad - S01E02 - Cat's in the Bag.mkv
```

Implementation plan:
- Extend `FileMover` to accept a `Movie` or `Episode` struct and derive the target path from it
- Handle multi-file torrents (identify the primary video file by extension and size, move extras to a sidecar)
- Detect and preserve subtitle files (`.srt`, `.ass`, `.sub`)
- Add duplicate detection: before downloading, check if the library already contains a copy at equal or better quality
- New endpoint: `GET /api/v1/library/scan` — walk the filesystem, reconcile with DB, report unmatched files

#### 1.4 — Quality Upgrade Logic

**Replaces: Sonarr's and Radarr's "upgrade until" quality setting** — **Status: Implemented.**

`UpgradeUntil` field on `PendingDownload` and `WantedItem`; `WantedPoller.ProcessUpgradeQueue()` runs every 15 minutes alongside the main wanted queue poll. History entry `upgrade_found` is written on success.

---

### Priority 2 — Request & Discovery Experience 🟠

**Replaces: Overseerr's entire UI**

#### 2.1 — Frontend: Discovery Pages

> **Status: Complete.** `/discover`, `/search`, `/movies/:id`, `/tvshows/:id`, `/library`, `/wanted` all implemented.

#### 2.2 — Notifications

> **Status: Complete.** `clients/notifications/` package with Discord, Email (SMTP), Ntfy, Gotify, and Apprise implementations. A `NotificationRegistry` fans out to all configured targets. `CheckForCompletedTorrents` fires notifications on `download_complete` and `download_failed` events.

#### 2.3 — Scheduled TVDB Refresh ✅

> **Status: Complete.** Implemented in `backend/api/tvdb_refresh.go` (`TVDBRefresher.RefreshAll()`) and wired into `main.go` as a 24-hour background ticker.

---

### Priority 3 — Indexer Improvements 🟡

#### 3.1 — More Built-in Indexers

The pluggable registry makes additions cheap. Each is one new file:

| Source | Type | Notes |
|---|---|---|
| Nyaa.si | Anime torrent | Has RSS; use `encoding/xml` |
| TorrentGalaxy | Both | HTML scrape |
| Magnetdl | Both | HTML scrape |
| RARBG mirrors | Both | Several community JSON APIs |
| Generic RSS adapter | Configurable | Parse any RSS/Atom feed as a torrent source |

#### 3.2 — Per-Item Quality Overrides

Allow setting a different quality preference per movie or show, stored in the DB record. The download pipeline checks for an item-level override before falling back to the global config.

#### 3.3 — Release Profile Filtering ✅

> **Status: Complete.** `ReleaseProfile` model and `release_profiles` bucket implemented. `applyReleaseProfiles()` filters and re-orders search results by Require/Reject/Prefer keyword rules. CRUD endpoints at `/api/v1/release-profiles`.

#### 3.4 — Indexer Health Dashboard

Surface per-indexer statistics in the UI:
- Last successful search timestamp
- Average response time (rolling 10-search window)
- Error rate (errors / total searches)
- Result count on last search

Stored in an in-memory map in the registry (not persisted — resets on restart).

---

### Priority 4 — Polish & Operations 🟢

All P4 items are **complete**.

#### 4.1 — Additional Media Servers ✅

`MediaServer` interface in `clients/media_server.go`. Implementations:
- `clients/plex.go` — existing Plex client, now implements the interface
- `clients/jellyfin.go` — Jellyfin REST API
- `clients/emby.go` — Emby REST API

`MediaServerRegistry` holds all registered servers and calls `ScanLibrary()` on each when `Scan()` is invoked. Wired into `TorrentHandler`; all scan call-sites use the registry.

Configure with: `JELLYFIN_URL`, `JELLYFIN_API_KEY`, `EMBY_URL`, `EMBY_API_KEY`.

#### 4.2 — Additional Torrent Clients ✅

- `clients/qbittorrent.go` — qBittorrent Web API v2 (cookie-jar session auth, full interface implementation)
- `clients/sabnzbd.go` — SABnzbd API (NZB/Usenet backend adapted to the `TorrentClient` interface)

Both are registered in `factory.go`. Set `TORRENT_CLIENT_TYPE=qbittorrent` or `TORRENT_CLIENT_TYPE=sabnzbd`.

For SABnzbd, `TORRENT_CLIENT_PASSWORD` is used as the API key.

#### 4.3 — Activity History ✅

`models/history.go` defines `HistoryEntry` and the following event types:

| Event | When |
|---|---|
| `search_started` | Once per poll cycle when the wanted queue has items |
| `torrent_added` | After a torrent is successfully sent to the client |
| `download_complete` | After a completed torrent's files are moved |
| `download_failed` | When move fails or torrent errors |
| `upgrade_found` | When a quality upgrade torrent is grabbed |

Stored in the `history` NutsDB bucket (append-only; newest-first on read). Exposed via `GET /api/v1/history`. Frontend `/history` page shows a table with colour-coded event badges.

#### 4.4 — Settings UI ✅

`GET /api/v1/settings` returns the merged view of env defaults and any persisted overrides. `PUT /api/v1/settings` accepts a partial map, merges it, persists it to the `config` bucket, and applies it to the live `*config.Config` pointer immediately.

Settings that can be changed at runtime (no restart required):
- Torrent client type, URL, username
- Movie and TV show library paths
- Preferred quality
- Media server URLs (Plex, Jellyfin, Emby)
- Notification URLs (Discord, Ntfy, Gotify, Apprise, SMTP host + from address)

Sensitive credentials (passwords, tokens, API keys) are not exposed or modifiable through the settings API — use environment variables.

Frontend: `/settings` page with sections for each category.

#### 4.5 — Scheduled Task Visibility ✅

`api/task_tracker.go` maintains an in-memory map of `TaskInfo` records. Each background goroutine calls `tracker.Start()` / `tracker.Done()` / `tracker.Fail()` around its work.

`GET /api/v1/system/tasks` returns the current state of all three background tasks:

```json
{
  "tasks": [
    {
      "name": "torrent_completion_check",
      "status": "idle",
      "last_run_at": "2024-01-15T14:22:00Z",
      "run_count": 42,
      "last_error": "",
      "next_run_hint": "every 5 min"
    }
  ]
}
```

Frontend: `/system` page with a status table.

#### 4.6 — Proper Multi-User Support ✅

**User model** (`models/user.go`):
- `Role UserRole` — `"admin"` or `"user"`
- `PasswordHash string` — bcrypt hash (never serialised to JSON)
- `NotificationPreferences` — per-channel toggles (Discord, Email, Ntfy, Gotify, Apprise)

**Authentication endpoints:**
- `POST /api/v1/auth/login` — accepts `{username, password}`, returns a signed JWT (24-hour expiry)
- `POST /api/v1/auth/register` — creates a new `user`-role account with a bcrypt-hashed password

**JWT middleware** (`middleware/jwt.go`):
- `JWTMiddleware(secret)` — lenient: passes requests without a Bearer token, rejects requests with an invalid/expired token
- `RequireRole("admin")` — 403 if the JWT role claim does not match; applied to `POST /api/v1/wanted/:id/approve` and `POST /api/v1/wanted/:id/reject`

**Admin bootstrap:** if no admin user exists at startup, one is created automatically. See §3.5.

**Request attribution:** `WantedItem.AddedBy` stores the user ID of whoever made the request.

---

## 5. Database Migration Consideration

NutsDB is excellent for the current scale but has meaningful limitations as the data model grows:

| Limitation | Impact |
|---|---|
| No secondary indices | Episode lookups by show+season require full-bucket scan |
| No range queries | Date-based filtering done in Go after full scan |
| No joins | Correlating episodes → shows → downloads requires multiple lookups |
| Single-writer | High write concurrency (many concurrent downloads completing) could serialise |

**Recommendation:** Before implementing episode-level tracking (Priority 1.1), evaluate migrating the data layer to SQLite using `modernc.org/sqlite` (a pure-Go, CGo-free driver). The repository pattern already abstracts the storage layer cleanly — only the `db/*.go` files would need to change. All handler and model code would be unaffected.

The migration would be:
1. Add `modernc.org/sqlite` to `go.mod`
2. Rewrite each `*Repository` to use `database/sql` against SQLite
3. Add a schema migration system (a simple version table + ordered migration files)
4. Remove NutsDB dependency

This is not urgent today but should be treated as a prerequisite for the episode tracking feature.

---

## 6. Comparison: OpenMediaProject vs. the *arr Stack

| Capability | Radarr | Sonarr | Prowlarr | Overseerr | OpenMediaProject |
|---|---|---|---|---|---|
| Movie search & metadata | ✅ | — | — | — | ✅ |
| TV search & metadata | — | ✅ | — | — | ✅ |
| Episode tracking | — | ✅ | — | — | 🔲 Roadmap P1 |
| Monitoring / auto-download | ✅ | ✅ | — | — | ✅ (wanted queue + poller) |
| Quality profiles | ✅ | ✅ | — | — | ✅ (global + per-item override roadmap P3) |
| Release profiles | ✅ | ✅ | — | — | ✅ |
| Quality upgrades | ✅ | ✅ | — | — | ✅ |
| Built-in indexers | — | — | ✅ | — | ✅ (4 built-in, pluggable) |
| Indexer aggregation | — | — | ✅ | — | ✅ |
| One-click download | ✅ | ✅ | — | — | ✅ |
| File renaming & organisation | ✅ | ✅ | — | — | 🔲 Roadmap P1 |
| Duplicate detection | ✅ | ✅ | — | — | ✅ (basic) |
| Request system | — | — | — | ✅ | ✅ |
| Admin approve/reject | — | — | — | ✅ | ✅ |
| Discovery / trending UI | — | — | — | ✅ | ✅ |
| Notifications | ✅ | ✅ | — | ✅ | ✅ (Discord, Email, Ntfy, Gotify, Apprise) |
| Plex integration | ✅ | ✅ | — | ✅ | ✅ |
| Jellyfin integration | ✅ | ✅ | — | ✅ | ✅ |
| Emby integration | ✅ | ✅ | — | ✅ | ✅ |
| Activity history | ✅ | ✅ | — | — | ✅ |
| Settings UI | ✅ | ✅ | ✅ | ✅ | ✅ |
| Task visibility | ✅ | ✅ | — | — | ✅ |
| Multi-user + roles | — | — | — | ✅ | ✅ |
| JWT session auth | — | — | — | ✅ | ✅ |
| Single container | — | — | — | — | ✅ |
| No external DB required | — | — | — | — | ✅ |
| Containers required | 1 | 1 | 1 | 1 | **1** |

---

## 7. Contributing

### Adding a Torrent Indexer

1. Create `backend/clients/indexers/mysource.go`
2. Implement the `Indexer` interface (see `interface.go`)
3. Call `r.Register(NewMySourceIndexer())` in `NewDefaultRegistry()` in `registry.go`
4. No other files need to change

### Adding a Torrent Client

1. Create `backend/clients/myclient.go`
2. Implement the `TorrentClient` interface (see `torrent.go`)
3. Add a `case` to `NewTorrentClient()` in `factory.go`
4. Add the type constant to both `clients/factory.go` and `config/config.go`

### Adding a Media Server

1. Create `backend/clients/mymediaserver.go`
2. Implement the `MediaServer` interface (see `media_server.go`): `Name() string`, `ScanLibrary() error`
3. Conditionally register in `main.go`: `mediaServers.Register(NewMyMediaServer(...))`
4. Add config fields to `config/config.go` and load from env vars

### Adding a Notification Target

1. Create `backend/clients/notifications/mytarget.go`
2. Implement `Notifier` interface (see `clients/notifications/interface.go`)
3. Conditionally register in `main.go`: `notifRegistry.Register(&MyNotifier{...})`

### Backend Conventions

- All handlers return `fiber.Map{"error": true, "message": "..."}` on failure
- All repositories skip the `_init` NutsDB key in `GetAll` implementations
- Errors are wrapped with `fmt.Errorf("component: %w", err)` for traceability
- Structured logging uses `zerolog`: `log.Info().Str("key", val).Msg("description")`
- No `log.Fatal` outside of `main.go`
