-- Per-plugin key/value storage, so a plugin (an indexer remembering a
-- cursor, a notifier remembering de-dup state, ...) can persist state
-- between calls. Scoped by plugin id; a plugin can only see its own rows.
CREATE TABLE plugin_storage (
    plugin_id TEXT NOT NULL,
    key       TEXT NOT NULL,
    value     TEXT NOT NULL,
    PRIMARY KEY (plugin_id, key)
);
