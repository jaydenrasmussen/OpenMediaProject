//! Scene numbering and alternate scene titles, from the two public services
//! Sonarr uses: TheXEM (episode renumbering) and Sonarr's scene mapping list.

use std::collections::HashSet;

use async_trait::async_trait;
use serde::Deserialize;

use crate::MetadataError;

const XEM_BASE: &str = "https://thexem.info";
const MAPPING_URL: &str = "https://services.sonarr.tv/v1/scenemapping";

/// An alternate title releases of a series go by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneTitle {
    pub tvdb_id: i64,
    pub title: String,
    /// The title with year and other noise removed, for searching.
    pub search_title: String,
    /// The season the title names, or -1 for the whole series.
    pub season: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Numbering {
    pub season: i32,
    pub episode: i32,
    pub absolute: Option<i32>,
}

/// One episode as the metadata source numbers it and as scene releases do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XemEpisode {
    pub tvdb: Numbering,
    pub scene: Numbering,
}

#[async_trait]
pub trait SceneSource: Send + Sync {
    async fn scene_titles(&self) -> Result<Vec<SceneTitle>, MetadataError>;
    /// TVDB ids TheXEM has a mapping for.
    async fn xem_known_ids(&self) -> Result<HashSet<i64>, MetadataError>;
    async fn xem_map(&self, tvdb_id: i64) -> Result<Vec<XemEpisode>, MetadataError>;
}

pub struct HttpSceneSource {
    http: reqwest::Client,
    xem: String,
    mapping_url: String,
}

impl Default for HttpSceneSource {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpSceneSource {
    pub fn new() -> Self {
        Self {
            http: omp_net::client_builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .expect("static client config"),
            xem: XEM_BASE.to_owned(),
            mapping_url: MAPPING_URL.to_owned(),
        }
    }

    /// For tests: point both services at one server.
    pub fn with_urls(mut self, xem: impl Into<String>, mapping_url: impl Into<String>) -> Self {
        self.xem = xem.into();
        self.mapping_url = mapping_url.into();
        self
    }

    async fn get<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T, MetadataError> {
        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| MetadataError::Http(e.to_string()))?;
        if !response.status().is_success() {
            return Err(MetadataError::Http(format!("HTTP {}", response.status())));
        }
        response
            .json()
            .await
            .map_err(|e| MetadataError::Parse(e.to_string()))
    }
}

#[derive(Deserialize)]
struct RawTitle {
    #[serde(rename = "tvdbId")]
    tvdb_id: i64,
    title: String,
    #[serde(rename = "searchTitle", default)]
    search_title: String,
    #[serde(default = "whole_series")]
    season: i32,
}

fn whole_series() -> i32 {
    -1
}

#[derive(Deserialize)]
struct XemResponse<T> {
    result: String,
    #[serde(default = "Vec::new")]
    data: Vec<T>,
    #[serde(default)]
    message: String,
}

#[derive(Deserialize)]
struct RawNumbering {
    season: i32,
    episode: i32,
    absolute: Option<i32>,
}

#[derive(Deserialize)]
struct RawXem {
    scene: Option<RawNumbering>,
    tvdb: Option<RawNumbering>,
}

impl From<RawNumbering> for Numbering {
    fn from(r: RawNumbering) -> Self {
        Self {
            season: r.season,
            episode: r.episode,
            absolute: r.absolute.filter(|a| *a > 0),
        }
    }
}

#[async_trait]
impl SceneSource for HttpSceneSource {
    async fn scene_titles(&self) -> Result<Vec<SceneTitle>, MetadataError> {
        let raw: Vec<RawTitle> = self.get(&self.mapping_url).await?;
        Ok(raw
            .into_iter()
            .map(|r| SceneTitle {
                tvdb_id: r.tvdb_id,
                search_title: if r.search_title.is_empty() {
                    r.title.clone()
                } else {
                    r.search_title
                },
                title: r.title,
                season: r.season,
            })
            .collect())
    }

    async fn xem_known_ids(&self) -> Result<HashSet<i64>, MetadataError> {
        let r: XemResponse<String> = self
            .get(&format!("{}/map/havemap?origin=tvdb", self.xem))
            .await?;
        if r.result != "success" {
            return Err(MetadataError::Http(r.message));
        }
        Ok(r.data.iter().filter_map(|s| s.parse().ok()).collect())
    }

    async fn xem_map(&self, tvdb_id: i64) -> Result<Vec<XemEpisode>, MetadataError> {
        let r: XemResponse<RawXem> = self
            .get(&format!("{}/map/all?id={tvdb_id}&origin=tvdb", self.xem))
            .await?;
        if r.result != "success" {
            return Err(MetadataError::Http(r.message));
        }
        Ok(r.data
            .into_iter()
            .filter_map(|x| {
                Some(XemEpisode {
                    tvdb: x.tvdb?.into(),
                    scene: x.scene?.into(),
                })
            })
            .collect())
    }
}
