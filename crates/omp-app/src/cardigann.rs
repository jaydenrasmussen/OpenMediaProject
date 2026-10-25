//! Torrent sites read from Prowlarr's definitions: the list the Settings page
//! offers, the engine's network access, and the glue to the indexer pool and
//! the download clients.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, RwLock},
    time::{Duration, Instant},
};

use async_trait::async_trait;
use chrono::Utc;
use omp_cardigann::{
    Config, Fetched, Http, HttpRequest, HttpResponse, Indexer, Library, Method, Query, QueryKind,
    Release, Summary, def::Definition,
};
use omp_core::release::{DownloadProtocol, IndexerFlags, ReleaseInfo, TorrentRelease};
use omp_db::{
    Db,
    repo::{Provider, ProviderKind},
};
use omp_downloads::{INDEXER_LINK_PREFIX, LinkResolver, ResolvedLink};
use omp_indexers::{MovieSearch, TvSearch};
use serde_json::Value;

const MAX_BODY: usize = 16 * 1024 * 1024;
const BROWSER: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

/// The engine's network: the app's own HTTP client (so the proxy settings
/// apply), no redirects (the engine follows them itself), and a pause
/// between requests to one site.
struct Web {
    client: reqwest::Client,
    gap: Duration,
    last: tokio::sync::Mutex<Option<Instant>>,
    /// FlareSolverr's address, when one is set up, and the browser identity it used.
    solver: Arc<RwLock<Option<String>>>,
    identity: Mutex<Option<String>>,
}

/// Cloudflare's "checking your browser" page.
fn is_challenge(status: u16, headers: &[(String, String)], body: &[u8]) -> bool {
    (status == 403 || status == 503)
        && (headers.iter().any(|(k, v)| {
            k.eq_ignore_ascii_case("server") && v.to_lowercase().contains("cloudflare")
        }) || String::from_utf8_lossy(&body[..body.len().min(4096)]).contains("Just a moment"))
}

#[async_trait]
impl Http for Web {
    async fn send(&self, r: HttpRequest) -> Result<HttpResponse, String> {
        {
            let mut last = self.last.lock().await;
            if let Some(prev) = *last {
                let wait = self.gap.saturating_sub(prev.elapsed());
                if !wait.is_zero() {
                    tokio::time::sleep(wait).await;
                }
            }
            *last = Some(Instant::now());
        }
        let mut b = match r.method {
            Method::Get => self.client.get(&r.url),
            Method::Post => self.client.post(&r.url),
        };
        // After FlareSolverr has passed a check, the same browser identity must follow.
        let identity = self.identity.lock().expect("identity").clone();
        for (k, v) in &r.headers {
            if identity.is_some() && k.eq_ignore_ascii_case("user-agent") {
                continue;
            }
            b = b.header(k, v);
        }
        if let Some(ua) = &identity {
            b = b.header("user-agent", ua);
        }
        if let Some((content_type, body)) = &r.body {
            b = b.header("content-type", content_type).body(body.clone());
        }
        // The address can hold a key or a passkey; say what went wrong without it.
        let res = b.send().await.map_err(|e| e.without_url().to_string())?;
        let status = res.status().as_u16();
        let headers: Vec<(String, String)> = res
            .headers()
            .iter()
            .map(|(k, v)| {
                (
                    k.to_string(),
                    String::from_utf8_lossy(v.as_bytes()).into_owned(),
                )
            })
            .collect();
        let body = res.bytes().await.map_err(|e| e.without_url().to_string())?;
        if body.len() > MAX_BODY {
            return Err(format!(
                "the answer is larger than {} MB",
                MAX_BODY / 1024 / 1024
            ));
        }
        let solver = self.solver.read().expect("solver").clone();
        if let Some(solver) = solver
            && is_challenge(status, &headers, &body)
        {
            return self.solve(&solver, &r).await;
        }
        Ok(HttpResponse {
            status,
            headers,
            body: body.to_vec(),
        })
    }
}

impl Web {
    /// Asks FlareSolverr (a headless browser) to load the page and pass the
    /// check; its cookies and browser identity are kept for what follows.
    async fn solve(&self, solver: &str, r: &HttpRequest) -> Result<HttpResponse, String> {
        let post = r.method == Method::Post;
        let mut ask = serde_json::json!({
            "cmd": if post { "request.post" } else { "request.get" },
            "url": r.url,
            "maxTimeout": 60000,
        });
        if post && let Some((_, body)) = &r.body {
            ask["postData"] = Value::String(String::from_utf8_lossy(body).into_owned());
        }
        let client = omp_net::client_builder()
            .timeout(Duration::from_secs(90))
            .build()
            .map_err(|e| e.to_string())?;
        let reply: Value = client
            .post(format!("{}/v1", solver.trim_end_matches('/')))
            .json(&ask)
            .send()
            .await
            .map_err(|e| format!("FlareSolverr could not be reached: {}", e.without_url()))?
            .json()
            .await
            .map_err(|e| {
                format!(
                    "FlareSolverr answered something unexpected: {}",
                    e.without_url()
                )
            })?;
        if reply["status"] != "ok" {
            return Err(format!(
                "FlareSolverr could not pass the check: {}",
                reply["message"].as_str().unwrap_or("no reason given")
            ));
        }
        let solution = &reply["solution"];
        if let Some(ua) = solution["userAgent"].as_str() {
            *self.identity.lock().expect("identity") = Some(ua.to_owned());
        }
        let mut headers = vec![(
            "content-type".to_owned(),
            "text/html; charset=utf-8".to_owned(),
        )];
        for c in solution["cookies"].as_array().into_iter().flatten() {
            if let (Some(n), Some(v)) = (c["name"].as_str(), c["value"].as_str()) {
                headers.push(("set-cookie".into(), format!("{n}={v}")));
            }
        }
        Ok(HttpResponse {
            status: solution["status"].as_u64().unwrap_or(200) as u16,
            headers,
            body: solution["response"]
                .as_str()
                .unwrap_or_default()
                .as_bytes()
                .to_vec(),
        })
    }
}

pub struct Hub {
    library: Library,
    db: Db,
    public_url: String,
    cache: Mutex<HashMap<i64, (String, Arc<Indexer>)>>,
    solver: Arc<RwLock<Option<String>>>,
}

/// The saved address of FlareSolverr.
pub const FLARESOLVERR_KEY: &str = "flaresolverr_url";

/// What a person typed for a site, read from the saved settings.
struct Settings {
    definition: String,
    config: Config,
    minimum_seeders: i32,
    seed_ratio: Option<f64>,
    seed_time_minutes: Option<u32>,
}

fn read_settings(p: &Provider) -> Result<Settings, String> {
    let s = &p.settings;
    let definition = s["definition"]
        .as_str()
        .filter(|d| !d.is_empty())
        .ok_or("no site is chosen")?
        .to_owned();
    let mut config = Config::new();
    if let Some(map) = s["config"].as_object() {
        for (k, v) in map {
            config.insert(k.clone(), v.clone());
        }
    }
    if let Some(base) = s["base_url"].as_str().filter(|b| !b.trim().is_empty()) {
        config.insert("base_url".into(), Value::String(base.trim().to_owned()));
    }
    Ok(Settings {
        definition,
        config,
        minimum_seeders: s["minimum_seeders"].as_i64().unwrap_or(1) as i32,
        seed_ratio: s["seed_ratio"].as_f64(),
        seed_time_minutes: s["seed_time_minutes"].as_u64().map(|m| m as u32),
    })
}

impl Hub {
    /// `extra_definitions` is a folder of the owner's own definitions.
    pub fn new(db: Db, config: &crate::AppConfig) -> Self {
        let extra = config.data_dir.join("indexers");
        let library = Library::with_dir(Some(&extra));
        for problem in &library.problems {
            tracing::warn!("indexer definition: {problem}");
        }
        let public_url = std::env::var("OMP_PUBLIC_URL")
            .ok()
            .map(|u| u.trim_end_matches('/').to_owned())
            .filter(|u| !u.is_empty())
            .unwrap_or_else(|| {
                let scheme = if config.tls.is_some() {
                    "https"
                } else {
                    "http"
                };
                format!(
                    "{scheme}://127.0.0.1:{}{}",
                    config.addr.port(),
                    config.url_base
                )
            });
        Self {
            library,
            db,
            public_url,
            cache: Mutex::default(),
            solver: Arc::default(),
        }
    }

    /// Where FlareSolverr is, or none.
    pub fn set_solver(&self, url: Option<String>) {
        *self.solver.write().expect("solver") = url.filter(|u| !u.trim().is_empty());
    }

    pub fn solver(&self) -> Option<String> {
        self.solver.read().expect("solver").clone()
    }

    pub async fn load_solver(&self) {
        if let Ok(url) = self.db.config().get(FLARESOLVERR_KEY).await {
            self.set_solver(url);
        }
    }

    pub async fn save_solver(&self, url: Option<String>) -> Result<(), String> {
        self.db
            .config()
            .set(FLARESOLVERR_KEY, url.as_deref().unwrap_or_default())
            .await
            .map_err(|e| e.to_string())?;
        self.set_solver(url);
        Ok(())
    }

    pub fn library(&self) -> &Library {
        &self.library
    }

    pub fn summaries(&self) -> Vec<Summary> {
        self.library.summaries()
    }

    /// The site's engine, kept so a sign-in lasts between searches. A change
    /// to the settings makes a fresh one.
    fn indexer(&self, p: &Provider) -> Result<(Arc<Indexer>, Settings), String> {
        let settings = read_settings(p)?;
        let def = self.library.get(&settings.definition).ok_or_else(|| {
            format!(
                "the site '{}' is not in the list of sites",
                settings.definition
            )
        })?;
        let fingerprint = p.settings.to_string();
        if let Some((fp, i)) = self.cache.lock().expect("site cache").get(&p.id)
            && *fp == fingerprint
        {
            return Ok((i.clone(), settings));
        }
        let indexer = self.build(def, settings.config.clone())?;
        self.cache
            .lock()
            .expect("site cache")
            .insert(p.id, (fingerprint, indexer.clone()));
        Ok((indexer, settings))
    }

    /// An engine for settings that are not saved yet (the Test button).
    pub fn build(&self, def: Arc<Definition>, config: Config) -> Result<Arc<Indexer>, String> {
        let client = omp_net::client_builder()
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(BROWSER)
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| e.to_string())?;
        let gap = Duration::from_secs_f64(def.requestdelay.unwrap_or(0.0).max(1.0));
        let web = Web {
            client,
            gap,
            last: tokio::sync::Mutex::new(None),
            solver: self.solver.clone(),
            identity: Mutex::new(None),
        };
        Indexer::new(def, config, Arc::new(web))
            .map(Arc::new)
            .map_err(|e| e.to_string())
    }

    pub fn forget(&self, id: i64) {
        self.cache.lock().expect("site cache").remove(&id);
    }

    /// Whether the saved site is one we can read.
    pub fn handles(&self, p: &Provider) -> bool {
        read_settings(p)
            .ok()
            .is_some_and(|s| self.library.get(&s.definition).is_some())
    }

    /// Signs in and asks the site for its newest releases.
    pub async fn test(&self, p: &Provider) -> Result<(), String> {
        let settings = read_settings(p)?;
        let def = self.library.get(&settings.definition).ok_or_else(|| {
            format!(
                "the site '{}' is not in the list of sites",
                settings.definition
            )
        })?;
        let indexer = self.build(def, settings.config)?;
        indexer.test().await.map_err(|e| e.to_string())
    }

    fn to_release(
        &self,
        r: Release,
        p: &Provider,
        s: &Settings,
        def: &Definition,
    ) -> Option<ReleaseInfo> {
        if let Some(seeders) = r.seeders
            && seeders < s.minimum_seeders
        {
            return None;
        }
        let needs_app = def.privacy != "public" || def.download.is_some();
        let download_url = match (&r.magnet, &r.download_url) {
            (Some(m), _) => m.clone(),
            (None, Some(u)) if needs_app => format!(
                "{INDEXER_LINK_PREFIX}{}?link={}",
                p.id,
                omp_cardigann::util::url_encode(u, encoding_rs::UTF_8)
            ),
            (None, Some(u)) => u.clone(),
            (None, None) => return None,
        };
        let mut flags = IndexerFlags::NONE;
        match r.download_volume_factor {
            Some(f) if f <= 0.0 => flags = flags | IndexerFlags::G_FREELEECH,
            Some(f) if (f - 0.25).abs() < 0.01 => flags = flags | IndexerFlags::G_FREELEECH25,
            Some(f) if (f - 0.5).abs() < 0.01 => flags = flags | IndexerFlags::G_HALFLEECH,
            Some(f) if (f - 0.75).abs() < 0.01 => flags = flags | IndexerFlags::G_FREELEECH75,
            _ => {}
        }
        if r.upload_volume_factor.is_some_and(|f| f >= 2.0) {
            flags = flags | IndexerFlags::G_DOUBLE_UPLOAD;
        }
        if r.description
            .as_deref()
            .is_some_and(|d| d.starts_with("Internal"))
        {
            flags = flags | IndexerFlags::G_INTERNAL;
        }
        Some(ReleaseInfo {
            guid: r.guid,
            title: r.title,
            size: r.size,
            download_url,
            indexer_id: p.id as i32,
            indexer: p.name.clone(),
            indexer_priority: p.priority,
            protocol: DownloadProtocol::Torrent,
            publish_date: r.publish_date.unwrap_or_else(Utc::now),
            container: None,
            indexer_flags: flags,
            imdb_id: r.imdb_id.filter(|i| *i > 0).map(|i| format!("tt{i:07}")),
            tmdb_id: r.tmdb_id.unwrap_or(0),
            torrent: Some(TorrentRelease {
                magnet_url: r.magnet,
                info_hash: r
                    .info_hash
                    .map(|h| h.to_lowercase())
                    .filter(|h| !h.is_empty()),
                seeders: r.seeders,
                peers: r.peers,
                seed_ratio: s.seed_ratio.or(r.minimum_ratio),
                seed_time_minutes: s
                    .seed_time_minutes
                    .or(r.minimum_seed_time.map(|t| (t / 60).max(0) as u32)),
            }),
        })
    }

    async fn run(&self, p: &Provider, queries: Vec<Query>) -> Result<Vec<ReleaseInfo>, String> {
        let (indexer, settings) = self.indexer(p)?;
        let def = self
            .library
            .get(&settings.definition)
            .ok_or("the site is not in the list of sites")?;
        let mut out: Vec<ReleaseInfo> = vec![];
        let mut last_error = None;
        for q in queries {
            match indexer.search(&q).await {
                Ok(found) => {
                    let converted: Vec<ReleaseInfo> = found
                        .into_iter()
                        .filter_map(|r| self.to_release(r, p, &settings, &def))
                        .collect();
                    let empty = converted.is_empty();
                    out.extend(converted);
                    // The first title that finds something is enough.
                    if !empty {
                        return Ok(out);
                    }
                }
                Err(e) => last_error = Some(e.to_string()),
            }
        }
        match (out.is_empty(), last_error) {
            (true, Some(e)) => Err(e),
            _ => Ok(out),
        }
    }

    pub async fn recent(&self, p: &Provider, tv: bool) -> Result<Vec<ReleaseInfo>, String> {
        let q = Query {
            kind: QueryKind::Search,
            categories: vec![if tv { 5000 } else { 2000 }],
            ..Default::default()
        };
        self.run(p, vec![q]).await
    }

    pub async fn search_movie(
        &self,
        p: &Provider,
        m: &MovieSearch,
    ) -> Result<Vec<ReleaseInfo>, String> {
        let queries = m
            .titles
            .iter()
            .filter(|t| !t.trim().is_empty())
            .take(2)
            .map(|t| Query {
                kind: QueryKind::Movie,
                q: t.clone(),
                year: (m.year > 0).then_some(m.year),
                imdb_id: m.imdb_id.clone(),
                tmdb_id: (m.tmdb_id > 0).then_some(m.tmdb_id),
                categories: vec![2000],
                ..Default::default()
            })
            .collect();
        self.run(p, queries).await
    }

    pub async fn search_tv(
        &self,
        p: &Provider,
        search: &TvSearch,
    ) -> Result<Vec<ReleaseInfo>, String> {
        use TvSearch::*;
        let (series, season, ep, episode, extra_titles): (
            _,
            Option<i32>,
            Option<i32>,
            Option<String>,
            Vec<String>,
        ) = match search {
            Episode {
                series,
                season,
                episode,
            } => (
                series,
                Some(*season),
                Some(*episode),
                Some(format!("S{season:02}E{episode:02}")),
                vec![],
            ),
            Season { series, season } => (
                series,
                Some(*season),
                None,
                Some(format!("S{season:02}")),
                vec![],
            ),
            DailyEpisode { series, air_date } => (
                series,
                None,
                None,
                Some(air_date.format("%Y.%m.%d").to_string()),
                vec![],
            ),
            DailySeason { series, year } => (series, None, None, Some(year.to_string()), vec![]),
            AnimeEpisode {
                series,
                absolute,
                season,
                episode,
            } => (
                series,
                Some(*season),
                Some(*episode),
                Some(format!("{absolute:02}")),
                vec![],
            ),
            AnimeSeason {
                series,
                season,
                season_titles,
            } => (
                series,
                Some(*season),
                None,
                season_titles.is_empty().then(|| format!("S{season:02}")),
                season_titles.clone(),
            ),
            Special {
                series,
                episode_titles,
            } => (series, None, None, None, episode_titles.clone()),
        };
        let anime = series.series_type == omp_core::tv::SeriesType::Anime;
        let base = |title: &str, episode: Option<String>| Query {
            kind: QueryKind::Tv,
            q: title.to_owned(),
            season,
            ep,
            episode,
            tvdb_id: series.tvdb_id,
            tmdb_id: (series.tmdb_id > 0).then_some(series.tmdb_id),
            imdb_id: series.imdb_id.clone(),
            categories: if anime { vec![5070, 5000] } else { vec![5000] },
            ..Default::default()
        };
        let mut queries: Vec<Query> = vec![];
        // A name that already includes the season, or an episode named in words, is searched as it is.
        for t in extra_titles.iter().take(2) {
            let words = if matches!(search, Special { .. }) {
                format!("{} {t}", series.titles.first().cloned().unwrap_or_default())
            } else {
                t.clone()
            };
            queries.push(base(words.trim(), None));
        }
        if episode.is_some() {
            for t in series
                .titles
                .iter()
                .filter(|t| !t.trim().is_empty())
                .take(2)
            {
                queries.push(base(t, episode.clone()));
            }
        }
        if queries.is_empty() {
            return Ok(vec![]);
        }
        self.run(p, queries).await
    }

    /// The torrent (or magnet) behind a release, for a client to add.
    pub async fn fetch(&self, provider_id: i64, link: &str) -> Result<Fetched, String> {
        let p = self
            .db
            .providers()
            .get(provider_id)
            .await
            .map_err(|e| e.to_string())?;
        if p.kind != ProviderKind::Indexer {
            return Err("not an indexer".into());
        }
        let (indexer, _) = self.indexer(&p)?;
        indexer.download(link).await.map_err(|e| e.to_string())
    }
}

/// Usenet indexers that speak Newznab, as Prowlarr lists them: name, address
/// and the API path when it is not `/api`.
pub const USENET_PRESETS: &[(&str, &str, &str)] = &[
    ("abNZB", "https://abnzb.com", "/api"),
    ("altHUB", "https://api.althub.co.za", "/api"),
    ("AnimeTosho (Usenet)", "https://feed.animetosho.org", "/api"),
    ("DOGnzb", "https://api.dognzb.cr", "/api"),
    ("DrunkenSlug", "https://drunkenslug.com", "/api"),
    ("GingaDADDY", "https://www.gingadaddy.com", "/api"),
    ("Miatrix", "https://www.miatrix.com", "/api"),
    ("Newz69", "https://newz69.keagaming.com", "/api"),
    ("NinjaCentral", "https://ninjacentral.co.za", "/api"),
    ("Nzb.life", "https://api.nzb.life", "/api"),
    ("NZBCat", "https://nzb.cat", "/api"),
    ("NZBFinder", "https://nzbfinder.ws", "/api"),
    ("NZBgeek", "https://api.nzbgeek.info", "/api"),
    ("NzbNoob", "https://www.nzbnoob.com", "/api"),
    ("NZBNDX", "https://www.nzbndx.com", "/api"),
    ("NzbPlanet", "https://api.nzbplanet.net", "/api"),
    ("NZBStars", "https://nzbstars.com", "/api"),
    ("Tabula Rasa", "https://www.tabula-rasa.pw", "/api/v1/api"),
];

/// `omp-indexer://<id>?link=<address>`.
fn parse_link(link: &str) -> Option<(i64, String)> {
    let rest = link.strip_prefix(INDEXER_LINK_PREFIX)?;
    let (id, query) = rest.split_once('?')?;
    let real = omp_cardigann::util::query_argument(&format!("?{query}"), "link")?;
    Some((id.parse().ok()?, real))
}

#[async_trait]
impl LinkResolver for Hub {
    async fn resolve(&self, link: &str) -> Result<ResolvedLink, String> {
        let (id, real) = parse_link(link).ok_or("not a link to a site")?;
        Ok(match self.fetch(id, &real).await? {
            Fetched::Magnet(m) => ResolvedLink::Magnet(m),
            Fetched::Torrent(b) => ResolvedLink::Torrent(b),
        })
    }

    async fn proxy_url(&self, link: &str) -> Option<String> {
        let (id, real) = parse_link(link)?;
        let key = crate::auth::api_key_from(&self.db).await.ok()?;
        Some(format!(
            "{}/api/v1/indexer/{id}/download?link={}&apikey={key}",
            self.public_url,
            omp_cardigann::util::url_encode(&real, encoding_rs::UTF_8)
        ))
    }
}

/// Runs the indexers the pool cannot: plugins and the torrent sites.
pub struct Runner {
    pub plugins: Arc<crate::plugins::Plugins>,
    pub hub: Arc<Hub>,
}

fn is_site(p: &Provider) -> bool {
    p.implementation == "cardigann"
}

#[async_trait]
impl omp_services::PluginIndexers for Runner {
    fn has(&self, provider: &Provider) -> bool {
        if is_site(provider) {
            self.hub.handles(provider)
        } else {
            self.plugins.has(provider)
        }
    }

    async fn recent(&self, provider: &Provider) -> Result<Vec<ReleaseInfo>, String> {
        if is_site(provider) {
            self.hub.recent(provider, false).await
        } else {
            self.plugins.recent(provider).await
        }
    }

    async fn search(
        &self,
        provider: &Provider,
        query: &MovieSearch,
    ) -> Result<Vec<ReleaseInfo>, String> {
        if is_site(provider) {
            self.hub.search_movie(provider, query).await
        } else {
            self.plugins.search(provider, query).await
        }
    }

    async fn recent_tv(&self, provider: &Provider) -> Result<Vec<ReleaseInfo>, String> {
        if is_site(provider) {
            self.hub.recent(provider, true).await
        } else {
            Ok(vec![])
        }
    }

    async fn search_tv(
        &self,
        provider: &Provider,
        search: &TvSearch,
    ) -> Result<Vec<ReleaseInfo>, String> {
        if is_site(provider) {
            self.hub.search_tv(provider, search).await
        } else {
            Ok(vec![])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_site_link_round_trips() {
        let link = format!(
            "{INDEXER_LINK_PREFIX}7?link={}",
            omp_cardigann::util::url_encode("https://x.test/d.php?id=1&k=a b", encoding_rs::UTF_8)
        );
        assert_eq!(
            parse_link(&link),
            Some((7, "https://x.test/d.php?id=1&k=a b".to_owned()))
        );
        assert_eq!(parse_link("https://x.test/"), None);
    }
}
