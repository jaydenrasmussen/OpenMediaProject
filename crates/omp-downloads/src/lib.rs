//! What a download client is to the rest of the app: something that accepts a
//! release, reports progress, and gives back finished files. The built-in
//! torrent and usenet clients and the plugin clients all implement
//! [`DownloadClient`].

mod client;
mod fake;
mod registry;

pub use client::{
    AddRequest, ClientError, ClientItem, DownloadClient, DownloadItemStatus, INDEXER_LINK_PREFIX,
    LinkResolver, QueuePriority, ResolvedLink,
};
pub use fake::FakeClient;
pub use registry::{ClientRegistry, RegisteredClient};
