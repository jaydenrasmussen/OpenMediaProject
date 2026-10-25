# Plex, Jellyfin and Emby

Connect a media server and the app reads what its libraries hold. Movies, series and episodes then show **On Plex** (or the name you gave the server) when the server already has them. The app only reads; it never changes anything on the server.

## Connect a server

Settings, Media servers, Add media server. Pick the type, fill in the address and the key, press Test, then Save. The library is read right away and again every hour. "Read libraries now" does it on demand.

| Type | Address | Key |
| --- | --- | --- |
| Plex | `http://host:32400` | Your Plex token (Plex's help: *Finding an authentication token*) |
| Jellyfin or Emby | `http://host:8096` | An API key from the server's dashboard, under API Keys |

If the container runs with the built-in VPN, allow your local network first (`FIREWALL_OUTBOUND_SUBNETS`, see [vpn.md](vpn.md)) or the app cannot reach a server on it.

## What it shows

- **Movies and series pages:** a line under each poster, and a badge on the detail page.
- **Episodes:** an episode with no file in this library but on a server shows *On Plex* instead of *Missing*. The Wanted lists and the missing counts still treat it as missing here, because it is not in this library.
- **Add pages:** a search result shows *On Plex* so you do not add what you already have.
- **Settings, Media servers:** for each server, how many movies, series and episodes were read and when. If a read fails, the last good copy stays and the error is shown. A list of what is on your servers but not in this library is below.

## How things are matched

Movies match by TMDB id, then IMDb id, then by title and year. Series match by TMDB, TVDB or IMDb id. A series with no id on the server matches by title alone. An episode matches on its series and its season and episode numbers. Make sure the server has fetched its metadata; items it could not identify have no id.

## Notes

- The server's address and key are stored in the database in clear text, like other provider settings. Use a key made for this app and revoke it if it leaks.
- Plex and Jellyfin are plugins (`plex-library`, `jellyfin-library`) built on the media server interface in [plugins.md](plugins.md). Another server can be added the same way.
