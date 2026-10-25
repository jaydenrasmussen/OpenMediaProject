-- The two built-in downloaders are there from the start, so nobody has to know
-- to add them. Only a library with no download client gets them: one that
-- already has its own setup is left as it is.
INSERT INTO providers (kind, name, implementation, settings)
SELECT 'download_client', 'Built-in torrent', 'torrent',
       '{"download_dir":null,"seed_ratio_limit":0,"enable_dht":true}'
WHERE NOT EXISTS (SELECT 1 FROM providers WHERE kind = 'download_client');

INSERT INTO providers (kind, name, implementation, settings)
SELECT 'download_client', 'Built-in usenet', 'usenet', '{"servers":[]}'
WHERE NOT EXISTS (SELECT 1 FROM providers WHERE kind = 'download_client' AND implementation = 'usenet')
  AND (SELECT COUNT(*) FROM providers WHERE kind = 'download_client') <= 1
  AND NOT EXISTS (SELECT 1 FROM providers WHERE kind = 'download_client' AND name <> 'Built-in torrent');
