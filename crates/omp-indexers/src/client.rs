//! The HTTP side: fetching capabilities, RSS and searches with Radarr's
//! tier and paging rules. Ported from `HttpIndexerBase` (GPL-3.0).

use std::time::Duration;

use chrono::{DateTime, Utc};
use omp_core::release::ReleaseInfo;
use tokio::{
    sync::{Mutex, OnceCell},
    time::Instant,
};

use crate::{
    caps::Capabilities,
    error::IndexerError,
    parse::{Source, parse_feed},
    request::{
        MAX_PAGES, MovieSearch, PAGE_SIZE, RequestSpec, generate_movie_requests,
        generate_recent_requests,
    },
    settings::{Flavor, IndexerSettings},
};

/// Stop paging an unbounded query after this many results.
const MAX_RESULTS_PER_QUERY: usize = 1000;

#[derive(Debug, Clone)]
pub struct IndexerIdentity {
    pub id: i32,
    pub name: String,
    pub priority: i32,
}

/// The newest release the last RSS sync saw, so the next sync knows where to stop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastSeen {
    pub publish_date: DateTime<Utc>,
    pub download_url: String,
}

/// What an RSS fetch found.
#[derive(Debug, Clone)]
pub struct RecentFeed {
    pub releases: Vec<ReleaseInfo>,
    /// False when the page limit was hit before reaching the last-seen
    /// release, meaning some releases may have been missed.
    pub fully_updated: bool,
    /// The newest release, to store as the next [`LastSeen`].
    pub newest: Option<LastSeen>,
}

pub struct IndexerClient {
    http: reqwest::Client,
    settings: IndexerSettings,
    flavor: Flavor,
    identity: IndexerIdentity,
    caps: OnceCell<Capabilities>,
    last_request: Mutex<Option<Instant>>,
    rate_limit: Duration,
}

impl IndexerClient {
    /// Validates the settings first, like Radarr does on save.
    pub fn new(
        flavor: Flavor,
        settings: IndexerSettings,
        identity: IndexerIdentity,
    ) -> Result<Self, String> {
        settings.validate()?;
        Ok(Self {
            http: reqwest::Client::builder()
                .user_agent(concat!("OpenMediaProject/", env!("CARGO_PKG_VERSION")))
                .timeout(Duration::from_secs(100))
                .build()
                .map_err(|e| e.to_string())?,
            settings,
            flavor,
            identity,
            caps: OnceCell::new(),
            last_request: Mutex::new(None),
            rate_limit: Duration::from_secs(2),
        })
    }

    /// Minimum gap between requests to this indexer (default 2 s).
    pub fn with_rate_limit(mut self, gap: Duration) -> Self {
        self.rate_limit = gap;
        self
    }

    async fn throttle(&self) {
        let mut last = self.last_request.lock().await;
        if let Some(prev) = *last {
            let wait = self.rate_limit.saturating_sub(prev.elapsed());
            if !wait.is_zero() {
                tokio::time::sleep(wait).await;
            }
        }
        *last = Some(Instant::now());
    }

    async fn get(&self, url: &str) -> Result<String, IndexerError> {
        self.throttle().await;
        let response = self
            .http
            .get(url)
            .header("Accept", "application/rss+xml, text/xml, application/xml")
            .send()
            .await
            .map_err(|e| IndexerError::Network(e.to_string()))?;

        let status = response.status();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_lowercase();
        let retry_after = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<u64>().ok())
            .map(Duration::from_secs);

        if status.as_u16() == 429 {
            return Err(IndexerError::TooManyRequests { retry_after });
        }
        let body = response
            .text()
            .await
            .map_err(|e| IndexerError::Network(e.to_string()))?;

        // An XML body is parsed even on an error status: indexers report
        // problems (bad key, limits) as XML with a 4xx.
        if !status.is_success() && !content_type.contains("xml") {
            return Err(IndexerError::Http(status.as_u16()));
        }
        if content_type.contains("text/html") {
            return Err(IndexerError::Html);
        }
        Ok(body)
    }

    /// Capabilities, fetched once per client.
    pub async fn capabilities(&self) -> Result<&Capabilities, IndexerError> {
        self.caps
            .get_or_try_init(|| async {
                let mut url = format!("{}?t=caps", self.settings.api_url());
                if !self.settings.api_key.trim().is_empty() {
                    url.push_str("&apikey=");
                    url.push_str(&self.settings.api_key);
                }
                let body = self.get(&url).await?;
                match Capabilities::parse(&body) {
                    Ok(caps) => Ok(caps),
                    // The indexer answered with an error element (not a bad key):
                    // fall back to defaults, as Radarr does, instead of failing.
                    Err(IndexerError::Api { message, .. }) => {
                        tracing::error!(indexer = %self.identity.name, "caps failed ({message}); using defaults");
                        Ok(Capabilities::default())
                    }
                    Err(other) => Err(other),
                }
            })
            .await
    }

    async fn fetch_page(&self, url: &str) -> Result<Vec<ReleaseInfo>, IndexerError> {
        let body = self.get(url).await?;
        parse_feed(
            &body,
            &Source {
                id: self.identity.id,
                name: &self.identity.name,
                priority: self.identity.priority,
                flavor: self.flavor,
                request_url: url,
            },
        )
    }

    /// Runs tiers of request specs. Returns the releases of the first tier
    /// that produced any, de-duplicated by guid.
    async fn run_tiers(
        &self,
        tiers: Vec<Vec<RequestSpec>>,
        recent: Option<&Option<LastSeen>>,
    ) -> Result<(Vec<ReleaseInfo>, bool), IndexerError> {
        let mut releases: Vec<ReleaseInfo> = Vec::new();
        let mut fully_updated = false;

        for tier in tiers {
            for spec in tier {
                let mut paged: Vec<ReleaseInfo> = Vec::new();
                for page_number in 0..MAX_PAGES {
                    let page = self.fetch_page(&spec.page_url(page_number)).await?;
                    paged.extend(page.iter().cloned());

                    if let (Some(last_seen), false) = (recent, page.is_empty()) {
                        match last_seen {
                            None => {
                                fully_updated = true;
                                break;
                            }
                            Some(last) => {
                                let oldest = page.iter().map(|r| r.publish_date).min();
                                let reached = oldest.is_some_and(|o| o < last.publish_date)
                                    || page.iter().any(|r| r.download_url == last.download_url);
                                if reached {
                                    fully_updated = true;
                                    break;
                                }
                                if paged.len() >= MAX_RESULTS_PER_QUERY
                                    && oldest.is_some_and(|o| {
                                        o < Utc::now() - chrono::Duration::hours(24)
                                    })
                                {
                                    fully_updated = false;
                                    break;
                                }
                            }
                        }
                    } else if paged.len() >= MAX_RESULTS_PER_QUERY {
                        break;
                    }
                    if page.len() < PAGE_SIZE {
                        break;
                    }
                }
                releases.extend(
                    paged.into_iter().filter(|r| {
                        !r.title.trim().is_empty() && !r.download_url.trim().is_empty()
                    }),
                );
            }
            if !releases.is_empty() {
                break;
            }
        }

        let mut seen = std::collections::HashSet::new();
        releases.retain(|r| seen.insert(r.guid.clone()));
        Ok((releases, fully_updated))
    }

    /// The indexer's newest releases (RSS sync). Pass the previous sync's
    /// [`LastSeen`] so paging stops where it left off.
    pub async fn fetch_recent(
        &self,
        last_seen: Option<LastSeen>,
    ) -> Result<RecentFeed, IndexerError> {
        let caps = self.capabilities().await?;
        let tiers = generate_recent_requests(&self.settings, caps);
        let (mut releases, fully_updated) = self.run_tiers(tiers, Some(&last_seen)).await?;
        releases.sort_by_key(|r| std::cmp::Reverse(r.publish_date));
        let newest = releases.first().map(|r| LastSeen {
            publish_date: r.publish_date,
            download_url: r.download_url.clone(),
        });
        Ok(RecentFeed {
            releases,
            fully_updated,
            newest,
        })
    }

    pub async fn search_movie(
        &self,
        search: &MovieSearch,
    ) -> Result<Vec<ReleaseInfo>, IndexerError> {
        let caps = self.capabilities().await?;
        let tiers = generate_movie_requests(&self.settings, caps, search);
        Ok(self.run_tiers(tiers, None).await?.0)
    }

    /// Checks a request can be made and answered. Used by "Test" in settings.
    pub async fn test(&self) -> Result<(), IndexerError> {
        let caps = self.capabilities().await?;
        let first = generate_recent_requests(&self.settings, caps)
            .into_iter()
            .flatten()
            .next()
            .ok_or_else(|| {
                IndexerError::UnsupportedFeed(
                    "No rss feed query available. This may be an issue with the indexer or your category settings.".into(),
                )
            })?;
        self.fetch_page(&first.page_url(0)).await?;
        Ok(())
    }
}
