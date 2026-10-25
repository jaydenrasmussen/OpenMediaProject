//! A download client that is a plugin: the plugin speaks the client's own
//! API, this adapts it to the app's [`DownloadClient`] trait.

use std::sync::Arc;

use async_trait::async_trait;
use omp_core::release::DownloadProtocol;
use omp_downloads::{
    AddRequest, ClientError, ClientItem, DownloadClient, DownloadItemStatus, INDEXER_LINK_PREFIX,
    LinkResolver, ResolvedLink,
};
use omp_plugin_host::{
    ClientAddRequest, ClientItemInfo, ClientStatus, DownloadClient as Kind, Plugin,
};

pub struct PluginClient {
    plugin: Arc<Plugin<Kind>>,
    settings: String,
    protocol: DownloadProtocol,
    resolver: Option<Arc<dyn LinkResolver>>,
}

impl PluginClient {
    pub fn new(
        plugin: Arc<Plugin<Kind>>,
        settings: &serde_json::Value,
        protocol: &str,
        resolver: Option<Arc<dyn LinkResolver>>,
    ) -> Self {
        Self {
            resolver,
            plugin,
            settings: settings.to_string(),
            protocol: if protocol.eq_ignore_ascii_case("usenet") {
                DownloadProtocol::Usenet
            } else {
                DownloadProtocol::Torrent
            },
        }
    }

    pub async fn test(&self) -> Result<(), String> {
        self.plugin
            .test(&self.settings)
            .await
            .map_err(|e| format!("{e:#}"))
    }
}

fn unavailable(e: anyhow::Error) -> ClientError {
    ClientError::Unavailable(format!("{e:#}"))
}

fn item(i: ClientItemInfo) -> ClientItem {
    ClientItem {
        download_id: i.id,
        category: i.category,
        title: i.title,
        total_size: i.total_size,
        remaining_size: i.remaining_size,
        status: match i.status {
            ClientStatus::Queued => DownloadItemStatus::Queued,
            ClientStatus::Paused => DownloadItemStatus::Paused,
            ClientStatus::Downloading => DownloadItemStatus::Downloading,
            ClientStatus::Completed => DownloadItemStatus::Completed,
            ClientStatus::Failed => DownloadItemStatus::Failed,
            ClientStatus::Warning => DownloadItemStatus::Warning,
        },
        output_path: i.output_path,
        seed_ratio: i.seed_ratio,
        can_move_files: i.can_move_files,
        can_be_removed: i.can_be_removed,
        message: i.message,
        eta_seconds: i.eta_seconds,
        download_speed: i.download_speed,
        ..Default::default()
    }
}

#[async_trait]
impl DownloadClient for PluginClient {
    fn protocol(&self) -> DownloadProtocol {
        self.protocol
    }

    async fn add(&self, request: &AddRequest<'_>) -> Result<String, ClientError> {
        let r = request.release;
        let mut url = r
            .torrent
            .as_ref()
            .and_then(|t| t.magnet_url.clone())
            .filter(|m| !m.is_empty())
            .unwrap_or_else(|| r.download_url.clone());
        // A link to a site the app reads itself: a magnet is passed on; a torrent
        // file is offered from the app's own address.
        if url.starts_with(INDEXER_LINK_PREFIX) {
            let resolver = self.resolver.as_ref().ok_or_else(|| {
                ClientError::ReleaseUnavailable(
                    "this link needs the app's indexer to fetch it".into(),
                )
            })?;
            let resolved = resolver.resolve(&url).await.map_err(|message| {
                ClientError::ReleaseDownloadFailed {
                    message,
                    retry_after: None,
                }
            })?;
            url = match resolved {
                ResolvedLink::Magnet(m) => m,
                ResolvedLink::Torrent(_) => resolver.proxy_url(&url).await.ok_or_else(|| {
                    ClientError::ReleaseUnavailable(
                        "the app has no address to offer the torrent from".into(),
                    )
                })?,
            };
        }
        if url.is_empty() {
            return Err(ClientError::ReleaseUnavailable(
                "release has no download link".into(),
            ));
        }
        let protocol = if self.protocol == DownloadProtocol::Usenet {
            "usenet"
        } else {
            "torrent"
        };
        self.plugin
            .add(
                &ClientAddRequest {
                    title: r.title.clone(),
                    url,
                    protocol: protocol.into(),
                    category: request.category.map(str::to_owned),
                    info_hash: r
                        .torrent
                        .as_ref()
                        .and_then(|t| t.info_hash.as_ref())
                        .map(|h| h.to_uppercase()),
                },
                &self.settings,
            )
            .await
            .map_err(unavailable)
    }

    async fn items(&self) -> Result<Vec<ClientItem>, ClientError> {
        Ok(self
            .plugin
            .items(&self.settings)
            .await
            .map_err(unavailable)?
            .into_iter()
            .map(item)
            .collect())
    }

    async fn remove(&self, download_id: &str, delete_data: bool) -> Result<(), ClientError> {
        self.plugin
            .remove(download_id, delete_data, &self.settings)
            .await
            .map_err(|e| ClientError::Other(format!("{e:#}")))
    }

    async fn pause(&self, download_id: &str) -> Result<(), ClientError> {
        self.plugin
            .pause(download_id, &self.settings)
            .await
            .map_err(|e| ClientError::Other(format!("{e:#}")))
    }

    async fn resume(&self, download_id: &str) -> Result<(), ClientError> {
        self.plugin
            .resume(download_id, &self.settings)
            .await
            .map_err(|e| ClientError::Other(format!("{e:#}")))
    }
}
