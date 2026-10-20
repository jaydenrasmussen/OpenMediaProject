-- OpenMediaProject main database, schema v1.
--
-- Conventions
--  * Dates are RFC 3339 UTC text.
--  * Nested structures that are only ever read and written whole (profile
--    items, custom format conditions, images, credits) are JSON text columns.
--  * Real foreign keys with ON DELETE rules replace most of Radarr's
--    "orphan cleanup" housekeeping jobs.

CREATE TABLE config (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE tags (
    id    INTEGER PRIMARY KEY,
    label TEXT NOT NULL UNIQUE COLLATE NOCASE
);

-- Movies import lists must not add again (the user removed them on purpose).
CREATE TABLE import_exclusions (
    tmdb_id INTEGER PRIMARY KEY,
    title   TEXT NOT NULL,
    year    INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE root_folders (
    id   INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE
);

-- ---- profiles --------------------------------------------------------------

CREATE TABLE quality_profiles (
    id                        INTEGER PRIMARY KEY,
    name                      TEXT NOT NULL UNIQUE COLLATE NOCASE,
    cutoff                    INTEGER NOT NULL,
    upgrade_allowed           INTEGER NOT NULL DEFAULT 1,
    min_format_score          INTEGER NOT NULL DEFAULT 0,
    cutoff_format_score       INTEGER NOT NULL DEFAULT 0,
    min_upgrade_format_score  INTEGER NOT NULL DEFAULT 1,
    language_id               INTEGER NOT NULL DEFAULT -1,
    items                     TEXT NOT NULL,           -- JSON [ProfileItem]
    format_items              TEXT NOT NULL DEFAULT '[]'  -- JSON [ProfileFormatItem]
);

CREATE TABLE quality_definitions (
    quality_id     INTEGER PRIMARY KEY,
    weight         INTEGER NOT NULL,
    min_size       REAL,
    max_size       REAL,
    preferred_size REAL
);

CREATE TABLE custom_formats (
    id                    INTEGER PRIMARY KEY,
    name                  TEXT NOT NULL UNIQUE COLLATE NOCASE,
    include_when_renaming INTEGER NOT NULL DEFAULT 0,
    specifications        TEXT NOT NULL              -- JSON [SpecificationDef]
);

CREATE TABLE delay_profiles (
    id                                  INTEGER PRIMARY KEY,
    enable_usenet                       INTEGER NOT NULL DEFAULT 1,
    enable_torrent                      INTEGER NOT NULL DEFAULT 1,
    preferred_protocol                  INTEGER NOT NULL DEFAULT 1,  -- 1 usenet, 2 torrent
    usenet_delay                        INTEGER NOT NULL DEFAULT 0,
    torrent_delay                       INTEGER NOT NULL DEFAULT 0,
    sort_order                          INTEGER NOT NULL,
    bypass_if_highest_quality           INTEGER NOT NULL DEFAULT 0,
    bypass_if_above_custom_format_score INTEGER NOT NULL DEFAULT 0,
    minimum_custom_format_score         INTEGER NOT NULL DEFAULT 0,
    tags                                TEXT NOT NULL DEFAULT '[]'
);

CREATE TABLE release_profiles (
    id         INTEGER PRIMARY KEY,
    name       TEXT NOT NULL,
    enabled    INTEGER NOT NULL DEFAULT 1,
    required   TEXT NOT NULL DEFAULT '[]',
    ignored    TEXT NOT NULL DEFAULT '[]',
    indexer_id INTEGER NOT NULL DEFAULT 0,
    tags       TEXT NOT NULL DEFAULT '[]'
);

-- ---- library ---------------------------------------------------------------

-- Provider data. Separate from `movies` so lists and lookups can share it.
CREATE TABLE movie_metadata (
    id                 INTEGER PRIMARY KEY,
    tmdb_id            INTEGER NOT NULL UNIQUE,
    imdb_id            TEXT,
    title              TEXT NOT NULL,
    clean_title        TEXT NOT NULL,
    sort_title         TEXT NOT NULL,
    original_title     TEXT NOT NULL DEFAULT '',
    original_language  TEXT NOT NULL DEFAULT '',
    overview           TEXT,
    year               INTEGER NOT NULL DEFAULT 0,
    secondary_year     INTEGER,
    runtime            INTEGER NOT NULL DEFAULT 0,
    status             INTEGER NOT NULL DEFAULT 0,
    certification      TEXT,
    genres             TEXT NOT NULL DEFAULT '[]',
    in_cinemas         TEXT,
    physical_release   TEXT,
    digital_release    TEXT,
    collection_tmdb_id INTEGER,
    collection_title   TEXT,
    website            TEXT,
    studio             TEXT,
    rating_votes       INTEGER,
    rating_value       REAL,
    images             TEXT NOT NULL DEFAULT '[]',
    alternative_titles TEXT NOT NULL DEFAULT '[]',
    credits            TEXT NOT NULL DEFAULT '[]',
    popularity         REAL NOT NULL DEFAULT 0,
    last_info_sync     TEXT
);
CREATE INDEX movie_metadata_clean_title ON movie_metadata(clean_title);
CREATE INDEX movie_metadata_imdb ON movie_metadata(imdb_id);

-- Every cleaned title a movie can be found by, for matching releases to the
-- library without loading every movie.
CREATE TABLE movie_titles (
    movie_metadata_id INTEGER NOT NULL REFERENCES movie_metadata(id) ON DELETE CASCADE,
    clean_title       TEXT NOT NULL,
    kind              TEXT NOT NULL CHECK (kind IN ('title','original','alternative')),
    PRIMARY KEY (movie_metadata_id, clean_title, kind)
);
CREATE INDEX movie_titles_clean ON movie_titles(clean_title);

CREATE TABLE movies (
    id                   INTEGER PRIMARY KEY,
    movie_metadata_id    INTEGER NOT NULL UNIQUE REFERENCES movie_metadata(id),
    monitored            INTEGER NOT NULL DEFAULT 1,
    minimum_availability INTEGER NOT NULL DEFAULT 3,
    quality_profile_id   INTEGER NOT NULL REFERENCES quality_profiles(id),
    path                 TEXT NOT NULL,
    root_folder_path     TEXT,
    added                TEXT NOT NULL,
    tags                 TEXT NOT NULL DEFAULT '[]',
    last_search_time     TEXT
);
CREATE INDEX movies_path ON movies(path);

CREATE TABLE movie_files (
    id                 INTEGER PRIMARY KEY,
    movie_id           INTEGER NOT NULL UNIQUE REFERENCES movies(id) ON DELETE CASCADE,
    relative_path      TEXT NOT NULL,
    size               INTEGER NOT NULL,
    date_added         TEXT NOT NULL,
    scene_name         TEXT,
    release_group      TEXT,
    edition            TEXT,
    quality            TEXT NOT NULL,                 -- JSON QualityModel
    languages          TEXT NOT NULL DEFAULT '[]',    -- JSON [language id]
    media_info         TEXT,                          -- JSON
    original_file_path TEXT,
    indexer_flags      INTEGER NOT NULL DEFAULT 0
);

-- ---- TV ---------------------------------------------------------------------------
-- A series keeps its provider metadata in the same row (it is refreshed in place).

CREATE TABLE series (
    id                 INTEGER PRIMARY KEY,
    tmdb_id            INTEGER NOT NULL UNIQUE,
    tvdb_id            INTEGER,
    imdb_id            TEXT,
    title              TEXT NOT NULL,
    clean_title        TEXT NOT NULL,
    sort_title         TEXT NOT NULL,
    original_title     TEXT NOT NULL DEFAULT '',
    original_language  TEXT NOT NULL DEFAULT '',
    overview           TEXT,
    year               INTEGER NOT NULL DEFAULT 0,
    status             INTEGER NOT NULL DEFAULT 0,
    network            TEXT,
    runtime            INTEGER NOT NULL DEFAULT 0,
    genres             TEXT NOT NULL DEFAULT '[]',
    certification      TEXT,
    first_aired        TEXT,
    last_aired         TEXT,
    images             TEXT NOT NULL DEFAULT '[]',
    alternative_titles TEXT NOT NULL DEFAULT '[]',
    rating_votes       INTEGER,
    rating_value       REAL,
    series_type        INTEGER NOT NULL DEFAULT 0,
    monitored          INTEGER NOT NULL DEFAULT 1,
    monitor_new_items  INTEGER NOT NULL DEFAULT 0,   -- 0 all, 1 none
    quality_profile_id INTEGER NOT NULL REFERENCES quality_profiles(id),
    season_folder      INTEGER NOT NULL DEFAULT 1,
    use_scene_numbering INTEGER NOT NULL DEFAULT 0,
    path               TEXT NOT NULL,
    root_folder_path   TEXT,
    tags               TEXT NOT NULL DEFAULT '[]',
    added              TEXT NOT NULL,
    last_info_sync     TEXT
);
CREATE INDEX series_clean_title ON series(clean_title);
CREATE INDEX series_tvdb ON series(tvdb_id);

CREATE TABLE seasons (
    series_id     INTEGER NOT NULL REFERENCES series(id) ON DELETE CASCADE,
    season_number INTEGER NOT NULL,
    monitored     INTEGER NOT NULL DEFAULT 1,
    title         TEXT,
    images        TEXT NOT NULL DEFAULT '[]',
    PRIMARY KEY (series_id, season_number)
);

CREATE TABLE episode_files (
    id                 INTEGER PRIMARY KEY,
    series_id          INTEGER NOT NULL REFERENCES series(id) ON DELETE CASCADE,
    season_number      INTEGER NOT NULL,
    relative_path      TEXT NOT NULL,
    size               INTEGER NOT NULL,
    date_added         TEXT NOT NULL,
    scene_name         TEXT,
    release_group      TEXT,
    quality            TEXT NOT NULL,
    languages          TEXT NOT NULL DEFAULT '[]',
    media_info         TEXT,
    original_file_path TEXT,
    indexer_flags      INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX episode_files_series ON episode_files(series_id);

CREATE TABLE episodes (
    id                     INTEGER PRIMARY KEY,
    series_id              INTEGER NOT NULL REFERENCES series(id) ON DELETE CASCADE,
    tmdb_id                INTEGER NOT NULL DEFAULT 0,
    season_number          INTEGER NOT NULL,
    episode_number         INTEGER NOT NULL,
    absolute_episode_number INTEGER,
    scene_season_number    INTEGER,
    scene_episode_number   INTEGER,
    scene_absolute_episode_number INTEGER,
    title                  TEXT NOT NULL,
    overview               TEXT,
    air_date               TEXT,                    -- yyyy-mm-dd
    air_date_utc           TEXT,
    runtime                INTEGER NOT NULL DEFAULT 0,
    monitored              INTEGER NOT NULL DEFAULT 1,
    episode_file_id        INTEGER REFERENCES episode_files(id) ON DELETE SET NULL,
    last_search_time       TEXT,
    finale_type            TEXT,
    UNIQUE (series_id, season_number, episode_number)
);
CREATE INDEX episodes_file ON episodes(episode_file_id);
CREATE INDEX episodes_air_date ON episodes(air_date);

CREATE TABLE episode_history (
    id          INTEGER PRIMARY KEY,
    series_id   INTEGER NOT NULL REFERENCES series(id) ON DELETE CASCADE,
    episode_id  INTEGER NOT NULL REFERENCES episodes(id) ON DELETE CASCADE,
    event_type  INTEGER NOT NULL,
    date        TEXT NOT NULL,
    source_title TEXT NOT NULL,
    quality     TEXT NOT NULL,
    languages   TEXT NOT NULL DEFAULT '[]',
    download_id TEXT,
    data        TEXT NOT NULL DEFAULT '{}'
);
CREATE INDEX episode_history_episode ON episode_history(episode_id, date);
CREATE INDEX episode_history_download ON episode_history(download_id);

CREATE TABLE tv_blocklist (
    id              INTEGER PRIMARY KEY,
    series_id       INTEGER NOT NULL REFERENCES series(id) ON DELETE CASCADE,
    episode_ids     TEXT NOT NULL DEFAULT '[]',
    source_title    TEXT NOT NULL,
    quality         TEXT NOT NULL,
    languages       TEXT NOT NULL DEFAULT '[]',
    published_date  TEXT,
    size            INTEGER,
    protocol        INTEGER NOT NULL,
    indexer         TEXT,
    indexer_flags   INTEGER NOT NULL DEFAULT 0,
    torrent_info_hash TEXT,
    message         TEXT,
    date            TEXT NOT NULL
);
CREATE INDEX tv_blocklist_series ON tv_blocklist(series_id);

CREATE TABLE pending_tv_releases (
    id          INTEGER PRIMARY KEY,
    series_id   INTEGER NOT NULL REFERENCES series(id) ON DELETE CASCADE,
    episode_ids TEXT NOT NULL DEFAULT '[]',
    title       TEXT NOT NULL,
    added       TEXT NOT NULL,
    release     TEXT NOT NULL,
    reason      INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX pending_tv_series ON pending_tv_releases(series_id);

-- ---- history ------------------------------------------------------------------

CREATE TABLE history (
    id           INTEGER PRIMARY KEY,
    movie_id     INTEGER NOT NULL REFERENCES movies(id) ON DELETE CASCADE,
    event_type   INTEGER NOT NULL,
    date         TEXT NOT NULL,
    source_title TEXT NOT NULL,
    quality      TEXT NOT NULL,
    languages    TEXT NOT NULL DEFAULT '[]',
    download_id  TEXT,
    data         TEXT NOT NULL DEFAULT '{}'
);
CREATE INDEX history_movie_date ON history(movie_id, date DESC);
CREATE INDEX history_download_id ON history(download_id);

CREATE TABLE blocklist (
    id               INTEGER PRIMARY KEY,
    movie_id         INTEGER NOT NULL REFERENCES movies(id) ON DELETE CASCADE,
    source_title     TEXT NOT NULL,
    quality          TEXT NOT NULL,
    languages        TEXT NOT NULL DEFAULT '[]',
    published_date   TEXT,
    size             INTEGER,
    protocol         INTEGER NOT NULL,
    indexer          TEXT,
    indexer_flags    INTEGER NOT NULL DEFAULT 0,
    torrent_info_hash TEXT,
    message          TEXT,
    date             TEXT NOT NULL
);
CREATE INDEX blocklist_movie ON blocklist(movie_id);
CREATE INDEX blocklist_hash ON blocklist(torrent_info_hash);

-- ---- providers --------------------------------------------------------------------
-- Indexers, download clients, notifications, import lists and metadata
-- sources share one table: a definition (implementation + JSON settings)
-- and a status row with escalating back-off. Plugins register
-- implementations; the table does not change.

CREATE TABLE providers (
    id                        INTEGER PRIMARY KEY,
    kind                      TEXT NOT NULL CHECK (kind IN
                                ('indexer','download_client','notification','import_list','metadata')),
    name                      TEXT NOT NULL,
    implementation            TEXT NOT NULL,
    plugin_id                 TEXT,                    -- NULL for built-ins
    settings                  TEXT NOT NULL DEFAULT '{}',
    tags                      TEXT NOT NULL DEFAULT '[]',
    enabled                   INTEGER NOT NULL DEFAULT 1,
    enable_rss                INTEGER NOT NULL DEFAULT 1,
    enable_automatic_search   INTEGER NOT NULL DEFAULT 1,
    enable_interactive_search INTEGER NOT NULL DEFAULT 1,
    priority                  INTEGER NOT NULL DEFAULT 25,
    UNIQUE (kind, name)
);

CREATE TABLE provider_status (
    provider_id     INTEGER PRIMARY KEY REFERENCES providers(id) ON DELETE CASCADE,
    initial_failure TEXT,
    most_recent_failure TEXT,
    escalation_level INTEGER NOT NULL DEFAULT 0,
    disabled_till   TEXT,
    -- The newest release the last RSS sync saw, so the next one knows where to stop.
    last_rss_publish_date TEXT,
    last_rss_download_url TEXT,
    last_tv_rss_publish_date TEXT,
    last_tv_rss_download_url TEXT
);

-- ---- queue and scheduling ------------------------------------------------------------

CREATE TABLE pending_releases (
    id       INTEGER PRIMARY KEY,
    movie_id INTEGER NOT NULL REFERENCES movies(id) ON DELETE CASCADE,
    title    TEXT NOT NULL,
    added    TEXT NOT NULL,
    release  TEXT NOT NULL,                           -- JSON ReleaseInfo
    reason   INTEGER NOT NULL DEFAULT 0               -- 0 delay, 1 client unavailable, 2 fallback
);
CREATE INDEX pending_movie ON pending_releases(movie_id);

CREATE TABLE commands (
    id          INTEGER PRIMARY KEY,
    name        TEXT NOT NULL,
    body        TEXT NOT NULL DEFAULT '{}',
    priority    INTEGER NOT NULL DEFAULT 0,
    status      INTEGER NOT NULL DEFAULT 0,
    result      INTEGER NOT NULL DEFAULT 0,
    queued_at   TEXT NOT NULL,
    started_at  TEXT,
    ended_at    TEXT,
    duration_ms INTEGER,
    exception   TEXT,
    trigger     INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX commands_status ON commands(status);

CREATE TABLE scheduled_tasks (
    type_name       TEXT PRIMARY KEY,
    interval_minutes INTEGER NOT NULL,
    last_execution  TEXT,
    last_start_time TEXT,
    priority        INTEGER NOT NULL DEFAULT 0
);

-- ---- users and naming ----------------------------------------------------------------

CREATE TABLE users (
    id            INTEGER PRIMARY KEY,
    username      TEXT NOT NULL UNIQUE COLLATE NOCASE,
    password_hash TEXT NOT NULL
);

-- Browser sessions. Only a hash of the cookie token is stored, so a copy of the
-- database does not hand out logins.
CREATE TABLE sessions (
    token_hash TEXT PRIMARY KEY,
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created    TEXT NOT NULL,
    expires    TEXT NOT NULL
);

CREATE TABLE naming_config (
    id       INTEGER PRIMARY KEY CHECK (id = 1),
    settings TEXT NOT NULL
);
