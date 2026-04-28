# Configuration API Guide

The OpenMedia Project provides two complementary APIs for managing configuration:

- **`/api/v1/config`** — reads and writes the in-memory runtime config (not persisted across restarts). Useful for one-shot overrides and connection testing.
- **`/api/v1/settings`** — reads and writes a persistent settings map stored in NutsDB. Changes are applied to the live config immediately **and** survive restarts. Prefer this API for any setting you want to keep.

This document covers both APIs.

---

## Authentication

All configuration endpoints require API key authentication (unless disabled in your server settings). Include your API key in requests using the `X-API-Key` header:

```bash
curl -H "X-API-Key: your_api_key_here" http://localhost:8080/api/config
```

## Endpoints

### Get Current Configuration

Retrieves the current system configuration including server settings, authentication status, database settings, and torrent client configuration.

**Endpoint:** `GET /api/v1/config`

**Response Example:**
```json
{
  "server": {
    "address": ":8080",
    "cors_allow_origins": "*",
    "environment": "development",
    "log_level": "info"
  },
  "auth": {
    "enabled": true,
    "allow_query_param": false,
    "skip_endpoints": ["/api/health", "/api/v1/health"]
  },
  "database": {
    "path": "./data/nutsdb"
  },
  "torrent_client": {
    "client_type": "transmission",
    "url": "http://transmission:9091",
    "username": "admin",
    "connected": true,
    "message": "Connected successfully"
  },
  "tvdb": {
    "configured": true
  },
  "tmdb": {
    "configured": true
  },
  "system_info": {
    "version": "1.0.0",
    "start_time": "2023-05-15T14:22:33Z",
    "uptime": "2d 5h 30m 45s",
    "go_version": "go1.25",
    "os_arch": "linux/amd64",
    "num_goroutine": 10
  },
  "allowed_settings": ["torrent_client", "tvdb", "tmdb", "server"]
}
```

**Note:** For security reasons, sensitive information like passwords and API keys are not included in the response.

### Update Configuration

Updates system configuration with the provided settings.

**Endpoint:** `POST /api/v1/config`

**Request Body:**

The request body can include one or more of the following sections:

```json
{
  "torrent_client": {
    "client_type": "transmission",
    "url": "http://transmission:9091",
    "username": "admin",
    "password": "password123"
  },
  "tvdb": {
    "api_key": "your_tvdb_api_key"
  },
  "tmdb": {
    "api_key": "your_tmdb_api_key"
  },
  "server": {
    "cors_allow_origins": "*",
    "log_level": "debug"
  }
}
```

You can include only the sections you want to update. For example, to update only the TMDB API key:

```json
{
  "tmdb": {
    "api_key": "your_tmdb_api_key"
  }
}
```

**Response Example:**
```json
{
  "success": true,
  "message": "Configuration updated successfully",
  "config": {
    "server": {
      "address": ":8080",
      "cors_allow_origins": "*",
      "environment": "development",
      "log_level": "debug"
    },
    "auth": {
      "enabled": true,
      "allow_query_param": false,
      "skip_endpoints": ["/api/health", "/api/v1/health"]
    },
    "database": {
      "path": "./data/nutsdb"
    },
    "torrent_client": {
      "client_type": "transmission",
      "url": "http://transmission:9091",
      "username": "admin",
      "connected": true
    },
    "tvdb": {
      "configured": true
    },
    "tmdb": {
      "configured": true
    }
  }
}
```

### Test Torrent Connection

Tests the connection to a torrent client without updating the configuration.

**Endpoint:** `POST /api/v1/config/test-torrent`

**Request Body:**
```json
{
  "client_type": "transmission",
  "url": "http://transmission:9091",
  "username": "admin",
  "password": "password123"
}
```

**Response Example (Success):**
```json
{
  "connected": true,
  "message": "Connected successfully",
  "client_info": {
    "client_type": "Transmission",
    "version": "3.00",
    "connected": true,
    "message": "Connected to Transmission 3.00 (RPC v16)",
    "download_speed": 1024000,
    "upload_speed": 512000,
    "torrents_count": 5
  }
}
```

**Response Example (Failure):**
```json
{
  "connected": false,
  "message": "Failed to connect: Connection refused",
  "client_info": null
}
```

## Configuration Options

### Torrent Client Configuration

| Field | Description | Required | Example Values |
|-------|-------------|----------|---------------|
| `client_type` | Type of torrent client | Yes | `"rutorrent"`, `"transmission"`, `"deluge"`, `"qbittorrent"`, `"sabnzbd"` |
| `url` | URL to the torrent client | Yes | `"http://transmission:9091"` |
| `username` | Username for authentication | No | `"admin"` |
| `password` | Password for authentication | No | `"password123"` |

### TVDB API Configuration

| Field | Description | Required | Example |
|-------|-------------|----------|---------|
| `api_key` | API key for The TVDB | Yes | `"your_tvdb_api_key"` |

### TMDB API Configuration

| Field | Description | Required | Example |
|-------|-------------|----------|---------|
| `api_key` | API key for The Movie Database | Yes | `"your_tmdb_api_key"` |

### Server Configuration

| Field | Description | Required | Example Values |
|-------|-------------|----------|---------------|
| `cors_allow_origins` | Allowed origins for CORS | No | `"*"` or `"http://localhost:3000,https://example.com"` |
| `log_level` | Logging level | No | `"debug"`, `"info"`, `"warn"`, `"error"` |

## Initial Setup Process

For a new installation, follow these steps using the Configuration API:

1. **Check Current Configuration**:
   ```bash
   curl -H "X-API-Key: your_api_key_here" http://localhost:8080/api/v1/config
   ```

2. **Test Torrent Client Connection**:
   ```bash
   curl -X POST -H "X-API-Key: your_api_key_here" -H "Content-Type: application/json" \
     -d '{"client_type": "transmission", "url": "http://transmission:9091", "username": "admin", "password": "password123"}' \
     http://localhost:8080/api/v1/config/test-torrent
   ```

3. **Update Full Configuration**:
   ```bash
   curl -X POST -H "X-API-Key: your_api_key_here" -H "Content-Type: application/json" \
     -d '{
       "torrent_client": {
         "client_type": "transmission",
         "url": "http://transmission:9091",
         "username": "admin",
         "password": "password123"
       },
       "tvdb": {
         "api_key": "your_tvdb_api_key"
       },
       "tmdb": {
         "api_key": "your_tmdb_api_key"
       }
     }' \
     http://localhost:8080/api/v1/config
   ```

## Frontend Integration

When building a frontend for the OpenMedia Project, you can create a setup wizard using these endpoints:

1. Create a multi-step form that collects:
   - Torrent client details (type, URL, credentials)
   - API keys for external services
   - Other system preferences

2. Use the `/api/v1/config/test-torrent` endpoint to validate torrent client details before submitting the final configuration.

3. Submit the complete configuration using the `/api/v1/config` endpoint.

## Troubleshooting

### Common Issues

1. **Connection Failed**: If testing a torrent client fails, check:
   - The URL is correct and includes protocol (http:// or https://)
   - Credentials are valid
   - The torrent client is running and accessible from the server
   - Any firewall rules that might block the connection

2. **Invalid Configuration**: If updating configuration fails:
   - Check that your JSON is properly formatted
   - Ensure required fields are provided
   - Verify that client_type is one of the supported values

3. **Authentication Issues**: If you get 401 Unauthorized:
   - Make sure you're using the correct API key
   - Check that the API key is sent in the X-API-Key header

## Security Considerations

- The Configuration API handles sensitive information including API keys and passwords
- Always use HTTPS in production to encrypt configuration data in transit
- Consider restricting access to the Configuration API to administrative users only
- API keys and passwords are not included in GET responses for security reasons

---

## Persistent Settings API (`/api/v1/settings`)

Unlike `/api/v1/config` (which only updates in-memory state), the settings API persists changes to the NutsDB `config` bucket **and** applies them to the live process immediately. Settings survive server restarts.

### Get Settings

```
GET /api/v1/settings
```

Returns the merged view of environment defaults and any stored overrides. Sensitive credentials (passwords, tokens) are not included.

**Response:**
```json
{
  "torrent_client_type": "transmission",
  "torrent_client_url": "http://transmission:9091",
  "torrent_client_username": "admin",
  "movie_dir": "/movies",
  "tvshow_dir": "/tvshows",
  "preferred_quality": "1080p",
  "plex_url": "http://plex:32400",
  "jellyfin_url": "",
  "emby_url": "",
  "discord_webhook_url": "",
  "ntfy_topic_url": "",
  "gotify_url": "",
  "apprise_url": "",
  "email_smtp_host": "",
  "email_from": ""
}
```

### Update Settings

```
PUT /api/v1/settings
Content-Type: application/json
```

Accepts a partial map — only the keys you send are updated. Omitted keys retain their current values.

**Example — change library paths:**
```bash
curl -X PUT -H "X-API-Key: ..." -H "Content-Type: application/json" \
  -d '{"movie_dir": "/media/movies", "tvshow_dir": "/media/tv"}' \
  http://localhost:8080/api/v1/settings
```

**Example — set preferred quality and torrent client:**
```bash
curl -X PUT -H "X-API-Key: ..." -H "Content-Type: application/json" \
  -d '{
    "torrent_client_type": "qbittorrent",
    "torrent_client_url": "http://qbittorrent:8080",
    "preferred_quality": "2160p"
  }' \
  http://localhost:8080/api/v1/settings
```

**Response:**
```json
{
  "success": true,
  "message": "Settings saved"
}
```

### Supported Keys

| Key | Type | Maps to |
|---|---|---|
| `torrent_client_type` | string | `cfg.TorrentClient.ClientType` |
| `torrent_client_url` | string | `cfg.TorrentClient.BaseURL` |
| `torrent_client_username` | string | `cfg.TorrentClient.Username` |
| `movie_dir` | string | `cfg.FileMover.MovieDir` |
| `tvshow_dir` | string | `cfg.FileMover.TVShowDir` |
| `preferred_quality` | string | `cfg.Downloads.PreferredQuality` |
| `plex_url` | string | `cfg.Plex.URL` |
| `jellyfin_url` | string | `cfg.Jellyfin.URL` |
| `emby_url` | string | `cfg.Emby.URL` |
| `discord_webhook_url` | string | `cfg.Notifications.DiscordWebhookURL` |
| `ntfy_topic_url` | string | `cfg.Notifications.NtfyTopicURL` |
| `gotify_url` | string | `cfg.Notifications.GotifyURL` |
| `apprise_url` | string | `cfg.Notifications.AppriseURL` |
| `email_smtp_host` | string | `cfg.Notifications.EmailSMTPHost` |
| `email_from` | string | `cfg.Notifications.EmailFrom` |

**Not settable via this API** (use environment variables): torrent client password, Plex token, Jellyfin/Emby API keys, Gotify token, SMTP credentials. These are sensitive and intentionally excluded.