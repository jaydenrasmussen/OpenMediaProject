-- Log database: kept apart so log churn never blocks library writes.
CREATE TABLE logs (
    id        INTEGER PRIMARY KEY,
    time      TEXT NOT NULL,
    level     TEXT NOT NULL,
    logger    TEXT NOT NULL,
    message   TEXT NOT NULL,
    exception TEXT
);
CREATE INDEX logs_time ON logs(time);
