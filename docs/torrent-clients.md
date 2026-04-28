# Torrent Client Configuration Guide

The OpenMedia Project supports multiple torrent clients, allowing you to choose the one that best fits your needs. This guide provides instructions for configuring each supported client and explains how to use the health check feature.

## Supported Clients

The application currently supports:

1. **ruTorrent**: A web UI for rTorrent
2. **Transmission**: A lightweight, cross-platform BitTorrent client
3. **Deluge**: A lightweight, free BitTorrent client
4. **qBittorrent**: A popular cross-platform BitTorrent client with a clean Web UI
5. **SABnzbd**: A Usenet NZB downloader (adapted to the torrent client interface for unified pipeline support)

## Configuration

To configure your torrent client, set the following environment variables:

```env
# Choose one of: rutorrent, transmission, deluge, qbittorrent, sabnzbd
TORRENT_CLIENT_TYPE=rutorrent
TORRENT_CLIENT_URL=http://your_torrent_client_url
TORRENT_CLIENT_USERNAME=your_username
TORRENT_CLIENT_PASSWORD=your_password
```

### ruTorrent Configuration

For ruTorrent, you need to provide:

1. **URL**: Full URL to your ruTorrent installation (e.g., `http://localhost:80/rutorrent`)
2. **Username**: Basic auth username (if configured)
3. **Password**: Basic auth password (if configured)

Example:

```env
TORRENT_CLIENT_TYPE=rutorrent
TORRENT_CLIENT_URL=http://192.168.1.100:80/rutorrent
TORRENT_CLIENT_USERNAME=admin
TORRENT_CLIENT_PASSWORD=password123
```

### Transmission Configuration

For Transmission, you need to provide:

1. **URL**: Base URL of your Transmission installation (e.g., `http://localhost:9091`)
2. **Username**: RPC authentication username (if configured)
3. **Password**: RPC authentication password (if configured)

Example:

```env
TORRENT_CLIENT_TYPE=transmission
TORRENT_CLIENT_URL=http://192.168.1.100:9091
TORRENT_CLIENT_USERNAME=transmission
TORRENT_CLIENT_PASSWORD=password123
```

### Deluge Configuration

For Deluge with web UI, you need to provide:

1. **URL**: Base URL of your Deluge web UI (e.g., `http://localhost:8112`)
2. **Password**: Deluge web UI password (username is not used but can be set to any value)

Example:

```env
TORRENT_CLIENT_TYPE=deluge
TORRENT_CLIENT_URL=http://192.168.1.100:8112
TORRENT_CLIENT_USERNAME=
TORRENT_CLIENT_PASSWORD=deluge
```

Note: For Deluge, the default password is often `deluge` unless you've changed it.

### qBittorrent Configuration

For qBittorrent, you need to provide:

1. **URL**: Base URL of your qBittorrent Web UI (e.g., `http://localhost:8080`)
2. **Username**: Web UI username (default is `admin`)
3. **Password**: Web UI password

The client authenticates via the qBittorrent Web API v2 session cookie.

Example:

```env
TORRENT_CLIENT_TYPE=qbittorrent
TORRENT_CLIENT_URL=http://192.168.1.100:8080
TORRENT_CLIENT_USERNAME=admin
TORRENT_CLIENT_PASSWORD=adminadmin
```

Recommended qBittorrent version: 4.3+.

### SABnzbd Configuration

SABnzbd is a Usenet NZB downloader. It is exposed through the same `TorrentClient` interface to allow the unified download pipeline to route NZB files through it.

For SABnzbd, you need to provide:

1. **URL**: Base URL of your SABnzbd instance (e.g., `http://localhost:8080`)
2. **Password**: SABnzbd API key (set `TORRENT_CLIENT_PASSWORD` to your SABnzbd API key)

The `TORRENT_CLIENT_USERNAME` field is not used for SABnzbd.

Example:

```env
TORRENT_CLIENT_TYPE=sabnzbd
TORRENT_CLIENT_URL=http://192.168.1.100:8080
TORRENT_CLIENT_PASSWORD=your_sabnzbd_api_key
```

Find your SABnzbd API key under **Config → General → API Key** in the SABnzbd web UI.

## Docker Setup

If you're running your torrent client in Docker, make sure the OpenMedia Project container can reach it. If they're on the same Docker network, use the container name as the hostname.

Example docker-compose.yml with Transmission:

```yaml
version: '3'
services:
  openmedia:
    build: .
    ports:
      - "8080:8080"
    environment:
      - TORRENT_CLIENT_TYPE=transmission
      - TORRENT_CLIENT_URL=http://transmission:9091
      - TORRENT_CLIENT_USERNAME=transmission
      - TORRENT_CLIENT_PASSWORD=password123
    volumes:
      - ./data:/app/data
    depends_on:
      - transmission
  
  transmission:
    image: linuxserver/transmission
    environment:
      - PUID=1000
      - PGID=1000
      - TZ=Europe/London
      - USER=transmission
      - PASS=password123
    volumes:
      - ./transmission/config:/config
      - ./transmission/downloads:/downloads
      - ./transmission/watch:/watch
    ports:
      - "9091:9091"
      - "51413:51413"
      - "51413:51413/udp"
```

## Troubleshooting

Common issues and solutions:

### Cannot Connect to Torrent Client

- Verify the URL is correct and includes the protocol (http:// or https://)
- Check that the port is open and not blocked by a firewall
- For Docker setups, ensure containers are on the same network

### Authentication Failures

- Double-check username and password
- For ruTorrent, ensure basic authentication is properly configured
- For Transmission, verify RPC authentication settings in settings.json
- For Deluge, check that the password matches the one in the web UI settings

### API Errors

- Check logs for specific error messages
3. Ensure your torrent client version is compatible (recommended versions):
  - ruTorrent/rTorrent: 3.10+
  - Transmission: 3.0+
  - Deluge: 2.0+
  - qBittorrent: 4.3+
  - SABnzbd: 3.0+

## Health Check Feature

The OpenMedia Project includes a health check feature that allows you to monitor the status of your torrent client. This feature is accessible through the API endpoint:

```
GET /api/v1/torrents/health
```

### Health Check Response

The health check returns the following information:

```json
{
  "client_type": "Transmission",
  "version": "3.00",
  "connected": true,
  "message": "Connected successfully",
  "download_speed": 1024000,
  "upload_speed": 512000,
  "torrents_count": 5,
  "response_time_ms": 230,
  "last_checked": "2023-05-15T14:22:33Z"
}
```

| Field | Description |
|-------|-------------|
| `client_type` | The type of torrent client (ruTorrent, Transmission, Deluge, qBittorrent, SABnzbd) |
| `version` | The version of the torrent client (when available) |
| `connected` | Whether the connection to the torrent client is working |
| `message` | Status message or error details |
| `download_speed` | Current global download speed in bytes/s |
| `upload_speed` | Current global upload speed in bytes/s |
| `torrents_count` | Number of torrents in the client |
| `response_time_ms` | Time taken to check the health in milliseconds |
| `last_checked` | Timestamp of when the check was performed |

### Using Health Check

The health check feature is useful for:

1. **Monitoring**: Verify your torrent client is online and responding
2. **Debugging**: Identify connection issues or authentication problems
3. **Statistics**: Track global download/upload speeds and torrent counts

You can set up a monitoring system to periodically check this endpoint to ensure your torrent client remains operational.