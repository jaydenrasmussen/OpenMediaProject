-- Plex and Jellyfin servers whose libraries are read, to show what is already there.

-- The providers table lists its kinds in a CHECK, which SQLite cannot change in
-- place: rebuild it. Dropping it removes the rows that reference it, so those
-- are kept aside and put back.
CREATE TABLE provider_status_keep AS SELECT * FROM provider_status;
CREATE TABLE remote_path_mappings_keep AS SELECT * FROM remote_path_mappings;

CREATE TABLE providers_new (
    id                        INTEGER PRIMARY KEY,
    kind                      TEXT NOT NULL CHECK (kind IN
                                ('indexer','download_client','notification','import_list','metadata','media_server')),
    name                      TEXT NOT NULL,
    implementation            TEXT NOT NULL,
    plugin_id                 TEXT,
    settings                  TEXT NOT NULL DEFAULT '{}',
    tags                      TEXT NOT NULL DEFAULT '[]',
    enabled                   INTEGER NOT NULL DEFAULT 1,
    enable_rss                INTEGER NOT NULL DEFAULT 1,
    enable_automatic_search   INTEGER NOT NULL DEFAULT 1,
    enable_interactive_search INTEGER NOT NULL DEFAULT 1,
    priority                  INTEGER NOT NULL DEFAULT 25,
    UNIQUE (kind, name)
);
INSERT INTO providers_new SELECT id, kind, name, implementation, plugin_id, settings, tags, enabled,
    enable_rss, enable_automatic_search, enable_interactive_search, priority FROM providers;
DROP TABLE providers;
ALTER TABLE providers_new RENAME TO providers;

-- Dropping the table removed only the rows that pointed at a provider, so clear the rest before restoring all.
DELETE FROM provider_status;
DELETE FROM remote_path_mappings;
INSERT INTO provider_status SELECT * FROM provider_status_keep;
INSERT INTO remote_path_mappings SELECT * FROM remote_path_mappings_keep;
DROP TABLE provider_status_keep;
DROP TABLE remote_path_mappings_keep;

-- What a media server holds, as of its last sync. Rebuilt in full each sync.
CREATE TABLE media_server_items (
    provider_id INTEGER NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
    item_key    TEXT NOT NULL,
    kind        TEXT NOT NULL CHECK (kind IN ('movie','series','episode')),
    series_key  TEXT,
    title       TEXT NOT NULL,
    year        INTEGER,
    tmdb_id     INTEGER,
    imdb_id     TEXT,
    tvdb_id     INTEGER,
    season      INTEGER,
    episode     INTEGER,
    PRIMARY KEY (provider_id, item_key)
);
CREATE INDEX media_server_items_tmdb ON media_server_items (kind, tmdb_id);
CREATE INDEX media_server_items_imdb ON media_server_items (kind, imdb_id);
CREATE INDEX media_server_items_tvdb ON media_server_items (kind, tvdb_id);
CREATE INDEX media_server_items_series ON media_server_items (provider_id, series_key);

CREATE TABLE media_server_sync (
    provider_id INTEGER PRIMARY KEY REFERENCES providers(id) ON DELETE CASCADE,
    synced_at   TEXT,
    error       TEXT,
    movies      INTEGER NOT NULL DEFAULT 0,
    series      INTEGER NOT NULL DEFAULT 0,
    episodes    INTEGER NOT NULL DEFAULT 0
);
