# Indexer Integration

OpenMediaProject supports indexing torrents from Torznab-compatible sources such as [Prowlarr](https://github.com/Prowlarr/Prowlarr) and [Jackett](https://github.com/Jackett/Jackett). When a movie or TV show is added, you can search indexers for a matching torrent and send it directly to your configured download client.

## How It Works

1. **Configure indexers** — add one or more Torznab endpoints (Prowlarr indexer proxies, Jackett feeds, etc.)
2. **Search** — the API fans out a search query to all enabled indexers concurrently and returns aggregated results
3. **Filter** — release profile rules (Require / Reject / Prefer keywords) are applied to the aggregated results
4. **Grab** — pick a result and POST its download URL or magnet link to have it sent straight to your torrent client

```
Media added → Search indexers (concurrent) → Apply release profiles → Pick best result → Grab → Download client
```

---

## Quick Start (Single Indexer via Environment Variables)

The simplest way to get started is to point the app at a Prowlarr instance using environment variables. This seeds the database with one indexer automatically on first run.

| Variable | Description | Example |
|---|---|---|
| `INDEXER_URL` | Torznab endpoint URL | `http://prowlarr:9696/1/api` |
| `INDEXER_API_KEY` | API key for the indexer | `abc123def456` |
| `INDEXER_NAME` | Display name (optional) | `Prowlarr - 1337x` |

### Finding Your Prowlarr Torznab URL

1. Open Prowlarr → **Indexers**
2. Click the indexer you want to expose
3. Under **Torznab Feed**, copy the URL — it looks like `http://<host>:9696/<n>/api?apikey=<key>`
4. Set `INDEXER_URL` to the base (`http://<host>:9696/<n>/api`) and `INDEXER_API_KEY` to the key

For Jackett, the URL pattern is `http://<host>:9117/api/v2.0/indexers/<indexer-name>/results/torznab/`.

---

## API Reference

All endpoints are under `/api/v1/indexers` and require the standard `X-API-Key` header when authentication is enabled.

---

### List Indexers

```
GET /api/v1/indexers
```

Returns all configured indexers. If the database is empty and an env-var indexer is configured, it is seeded automatically.

**Response**

```json
{
  "indexers": [
    {
      "id": "abc123",
      "name": "Prowlarr - 1337x",
      "type": "torznab",
      "url": "http://prowlarr:9696/1/api",
      "api_key": "abc123def456",
      "enabled": true,
      "priority": 0,
      "added_at": "2024-01-01T00:00:00Z"
    }
  ],
  "count": 1
}
```

---

### Add an Indexer

```
POST /api/v1/indexers
```

**Request Body**

```json
{
  "name": "Prowlarr - RARBG Mirror",
  "type": "torznab",
  "url": "http://prowlarr:9696/2/api",
  "api_key": "abc123def456",
  "enabled": true,
  "priority": 1
}
```

| Field | Type | Required | Description |
|---|---|---|---|
| `name` | string | ✅ | Human-readable name |
| `url` | string | ✅ | Torznab endpoint base URL |
| `api_key` | string | ✅ | API key |
| `type` | string | | `torznab` (default) or `newznab` |
| `enabled` | bool | | Whether to include in searches (default `true`) |
| `priority` | int | | Lower number = higher priority (default `0`) |

**Response** — the created indexer object with its generated `id`.

---

### Update an Indexer

```
PUT /api/v1/indexers/:id
```

Send the same body shape as the create request. All provided fields are updated.

---

### Delete an Indexer

```
DELETE /api/v1/indexers/:id
```

Returns `204 No Content` on success.

---

### Test an Indexer

```
POST /api/v1/indexers/test/:id
```

Performs a blank search against the indexer and reports how many results came back. Useful for verifying connectivity and API key validity.

**Response**

```json
{
  "success": true,
  "indexer": "Prowlarr - 1337x",
  "result_count": 20
}
```

---

### Search Indexers

```
POST /api/v1/indexers/search
```

Fans out a search to all enabled indexers (or a single one if `indexer_id` is specified). Results are aggregated and sorted by seeders descending.

**Request Body**

```json
{
  "query": "Breaking Bad",
  "type": "tv",
  "tvdb_id": "81189",
  "season": 1,
  "episode": 1,
  "limit": 20
}
```

| Field | Type | Description |
|---|---|---|
| `query` | string | Free-text search query. Can be empty when an ID is provided. |
| `type` | string | `movie`, `tv`, or `search` (generic). Controls which Torznab category is queried. |
| `imdb_id` | string | IMDb ID (e.g. `tt0903747`). Used for movie/tv ID-based search. |
| `tvdb_id` | string | TVDB ID. Used for TV ID-based search. |
| `season` | int | Season number (TV only) |
| `episode` | int | Episode number (TV only) |
| `indexer_id` | string | Restrict search to a single indexer by its `id`. |
| `limit` | int | Max results per indexer (default `20`). |

**Response**

```json
{
  "results": [
    {
      "title": "Breaking.Bad.S01E01.1080p.BluRay.x264",
      "guid": "https://indexer.example/release/12345",
      "link": "https://indexer.example/download/12345",
      "size": 2147483648,
      "published_at": "2024-01-15T12:00:00Z",
      "seeders": 142,
      "leechers": 8,
      "info_hash": "aabbccddeeff00112233445566778899aabbccdd",
      "category": ["TV", "TV/HD"],
      "indexer": "Prowlarr - 1337x",
      "download_url": "https://indexer.example/download/12345.torrent",
      "magnet_url": "magnet:?xt=urn:btih:aabbccdd...",
      "tvdb_id": "81189",
      "season": 1,
      "episode": 1
    }
  ],
  "count": 47
}
```

---

### Grab a Torrent

```
POST /api/v1/indexers/grab
```

Downloads or forwards a torrent to the configured download client. Accepts either a direct `.torrent` URL or a magnet link.

**Request Body**

```json
{
  "download_url": "https://indexer.example/download/12345.torrent",
  "title": "Breaking.Bad.S01E01.1080p.BluRay.x264"
}
```

Or with a magnet link:

```json
{
  "download_url": "magnet:?xt=urn:btih:aabbccdd...",
  "title": "Breaking.Bad.S01E01.1080p.BluRay.x264"
}
```

| Field | Type | Required | Description |
|---|---|---|---|
| `download_url` | string | ✅ | `.torrent` file URL or `magnet:` URI |
| `title` | string | | Used as the filename when downloading a `.torrent` file |
| `indexer_id` | string | | Optional — for logging/audit purposes |

**Response**

```json
{
  "success": true,
  "message": "Torrent 'Breaking.Bad.S01E01.1080p.BluRay.x264' successfully sent to download client"
}
```

The grab endpoint relies on the torrent client being configured. See [torrent-clients.md](./torrent-clients.md) for setup instructions.

---

## Release Profile Filtering

Release profiles are keyword rules applied to torrent titles **after** all indexers have been searched and **before** the best result is picked. They let you enforce encoding preferences (e.g. require BluRay, reject CAM) without writing custom search queries.

### How Profiles Are Applied

- A result **passes** if it satisfies **at least one** enabled profile.
- Within a single profile: the title must contain **all** `Require` terms and **none** of the `Reject` terms.
- `Prefer` terms do not filter — they promote matching results to the front of the list.
- If no profiles are enabled, all results pass through unchanged.

### Release Profile API

```
GET    /api/v1/release-profiles         → list all profiles
POST   /api/v1/release-profiles         → create a profile
PUT    /api/v1/release-profiles/:id     → update a profile
DELETE /api/v1/release-profiles/:id     → delete a profile
```

**Create a profile:**

```bash
curl -X POST -H "X-API-Key: ..." -H "Content-Type: application/json" \
  -d '{
    "name": "Quality BluRay",
    "enabled": true,
    "require": ["BluRay"],
    "reject":  ["CAM", "HDCAM", "TS", "R5"],
    "prefer":  ["x265", "HEVC", "REMUX"]
  }' \
  http://localhost:8080/api/v1/release-profiles
```

| Field | Type | Description |
|---|---|---|
| `name` | string | Human-readable label |
| `enabled` | bool | Whether this profile is applied during searches |
| `require` | []string | Title must contain **all** of these terms (case-insensitive) |
| `reject` | []string | Title must contain **none** of these terms (case-insensitive) |
| `prefer` | []string | Results matching any of these terms are sorted first |

### Common Examples

| Goal | require | reject | prefer |
|---|---|---|---|
| BluRay only, no cams | `["BluRay"]` | `["CAM","HDCAM","TS"]` | `["REMUX","x265"]` |
| English audio required | `["English"]` | `["dubbed"]` | |
| Prefer x265 when available | | | `["x265","HEVC","H265"]` |
| Reject low-quality encodes | | `["xvid","divx","mp4"]` | |

---

## Typical Workflow

### Adding a Movie

1. Search TMDB for the movie via `GET /api/v1/movies/search?q=Inception`
2. Note the movie's `id` and IMDb ID
3. Search indexers:
   ```
   POST /api/v1/indexers/search
   { "type": "movie", "imdb_id": "tt1375666", "query": "Inception" }
   ```
4. Pick the best result (highest seeders, preferred quality)
5. Grab it:
   ```
   POST /api/v1/indexers/grab
   { "download_url": "<result.download_url>", "title": "<result.title>" }
   ```

### Adding a TV Episode

1. Find the show via `GET /api/v1/tvshows/search?q=Breaking+Bad`
2. Search indexers:
   ```
   POST /api/v1/indexers/search
   { "type": "tv", "tvdb_id": "81189", "season": 1, "episode": 1, "query": "" }
   ```
3. Grab the best result

---

## Multiple Indexers

You can configure as many indexers as you like through the API. All enabled indexers are searched concurrently on every `search` request. This is the same approach used by Radarr and Sonarr.

**Priority** is informational for now — results are always sorted by seeders. Future releases may use priority for tie-breaking or to restrict automatic grabs.

**Recommended setup with Prowlarr:**
- Add Prowlarr as an "indexer proxy" — Prowlarr manages its own list of sites and exposes each one as a numbered Torznab URL (`/1/api`, `/2/api`, …)
- Add each Prowlarr-proxied indexer as a separate indexer entry here, giving them meaningful names

---

## Supported Protocols

| Protocol | Support | Notes |
|---|---|---|
| Torznab | ✅ Full | Prowlarr, Jackett, any Torznab-compatible source |
| Newznab | ⚠️ Partial | Model support present; primarily intended for future Usenet use |
| RSS (raw) | ❌ | Not supported — use an indexer proxy like Prowlarr instead |

---

## Security Notes

- API keys for indexers are stored in the local NutsDB database. Ensure your database directory (`DB_PATH`) is not publicly accessible.
- When `ENABLE_AUTH=true`, all indexer endpoints require a valid `X-API-Key` header. See [api-authentication.md](./api-authentication.md).
- Indexer URLs should always point to internal/private network addresses where possible. Avoid exposing indexer endpoints directly to the internet.