# Indexers

Settings, Indexers lists what is set up and has an **Add indexer** button with two lists:

- **Torrent sites**: 577 sites from [Prowlarr's definitions](https://github.com/Prowlarr/Indexers). Search by name, and filter by who can join (public, semi-private, private), language, and whether the site has movies, TV or anime. Tick "Works without an account or Cloudflare" to see only the sites that need nothing extra. Public sites are listed first.
- **Usenet indexers**: the Newznab indexers Prowlarr lists (NZBgeek, DrunkenSlug, NZBFinder and others), and "another Newznab indexer" for any not on the list. You need an account and its API key.

Pick a site, fill in what it asks for (a user name and password, a cookie, an API key; public sites ask for nothing), press **Test**, then **Save**. Test signs in if the site needs it and reads the newest releases. Torznab (Jackett and similar) is the third choice on the usenet tab.

## How it works

Each site is described by a definition file: where to search, how to sign in, and where the title, size, seeders and download link are on the page. The definitions are Prowlarr's own, read by an engine in `crates/omp-cardigann` that follows Prowlarr's rules, so a definition behaves the same. It reads HTML, JSON and XML sites, signs in by form, cookie, POST or GET, and finds the file to download behind a details page when the site hides it.

- **Movies and TV** are searched for by title, season and episode (`S01E02`), or by date for daily shows. Anime is searched by absolute number.
- **Private sites** are signed in to by the app. The torrent file is fetched by the app too (the download client is given a link that stays inside the app), so the cookie never leaves it.
- **Seeding goals** come from the site when it states them, or from the indexer's settings.

## Adding or updating definitions

The 577 definitions are inside the app. To add a site, or replace one with a newer version, put its `.yml` file in an `indexers` folder inside the config folder (`/config/indexers`) and restart. A file there with the same `id` replaces the bundled one. Prowlarr's [Indexers repository](https://github.com/Prowlarr/Indexers) has newer versions. Files that cannot be read are logged.

## Limits

- **Cloudflare.** Some sites, such as 1337x and EZTV, sit behind Cloudflare's browser check, which this app cannot pass by itself. The search then fails with a message that says so. Sites marked Cloudflare in the list are the ones whose definition says they often need it. If you run [FlareSolverr](https://github.com/FlareSolverr/FlareSolverr), put its address (for example `http://flaresolverr:8191`) in the FlareSolverr box on the Indexers tab: the app checks that FlareSolverr answers, and from then on asks it to pass the check, keeping the cookie and the browser identity it used. FlareSolverr is a separate container; it is not part of this image.
- **CAPTCHAs.** A site that asks for a CAPTCHA when signing in needs the answer typed into its CAPTCHA field in the editor.
- **Search by id.** Torrent sites are searched by words, not by IMDb or TVDB id, as in Prowlarr.

## The plugin

The same engine is also an ordinary plugin, `prowlarr-indexers` ("Torrent sites (Prowlarr definitions)"), for people who want a site as a plugin indexer. Choose the site by name in its settings. In the sandbox the host passes pages as text and cannot hand over a torrent file, so it works for sites that give magnet links or direct downloads, which is most public sites. For private sites, use the built-in list above.

## Your own TMDB token

Posters, banners, cast and release dates come from TMDB. OpenMediaProject carries the read-only token that Radarr publishes in its own source, so this works with no set-up. To use your own, set `TMDB_READ_TOKEN` (your "API Read Access Token" from themoviedb.org) or paste it under Settings → Network. The environment wins over the saved one, and "Use the built-in key" goes back.
