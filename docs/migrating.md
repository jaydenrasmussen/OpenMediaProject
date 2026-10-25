# Moving from Radarr or Sonarr

OpenMediaProject starts with its own database. Nothing is read from Radarr or Sonarr, and nothing in their folders is changed.

1. **Install and sign in.** Start the container with the same media volume paths your old setup used, so folders line up.
2. **Root folders.** Settings, Media management: add your movie and series folders.
3. **Import the library.** Movies, Import (and Series, Import): the app finds folders under a root folder, matches them to the metadata provider and adopts the files. Nothing is moved or renamed unless you turn renaming on.
4. **Quality profiles and custom formats.** Create them again in Settings. The defaults match Radarr's. Custom formats use the same specification types.
5. **Indexers.** Add each Newznab or Torznab indexer with its URL and API key. Torznab indexers can set their own seed ratio and time.
6. **Download clients.** Use the built-in usenet and torrent clients, or install the SABnzbd, NZBGet, qBittorrent or Transmission plugin. If the client sees paths differently, set a remote path mapping.
7. **Notifications and lists.** Email is built in; install plugins for the rest.
8. **Turn the old apps off** once searches work, so two programs do not grab the same release.

What differs: no Postgres, one SQLite file per concern (`main.db`, `logs.db`); the API lives at `/api/v1` and is not Radarr-compatible; third-party apps that talk to Radarr's API (Overseerr and similar) will not work against it yet.
