-- Saved filters for a library view (e.g. movies, series). `filters` is a
-- JSON array of free-form filter rows (key/value pairs the UI understands
-- for that view); the database does not interpret them, same as Sonarr and
-- Radarr's own CustomFilter table.
CREATE TABLE custom_filters (
    id      INTEGER PRIMARY KEY,
    type    TEXT NOT NULL,
    label   TEXT NOT NULL,
    filters TEXT NOT NULL DEFAULT '[]'
);
CREATE INDEX custom_filters_type ON custom_filters(type);
