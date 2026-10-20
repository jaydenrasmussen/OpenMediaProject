//! Newznab and Torznab indexer support, ported from Radarr's `Indexers`
//! namespace (GPL-3.0). These two protocols are built in (PLAN.md, Q1);
//! site-specific indexers are plugins.

mod caps;
mod client;
mod error;
mod parse;
mod request;
mod request_tv;
mod settings;

pub use caps::{Capabilities, Category};
pub use client::{IndexerClient, IndexerIdentity, LastSeen, RecentFeed};
pub use error::IndexerError;
pub use parse::{Source, clean_xml, parse_feed, parse_size};
pub use request::{
    MovieSearch, RequestSpec, clean_scene_title, generate_movie_requests, generate_recent_requests,
};
pub use request_tv::{TvSearch, TvSeries, generate_recent_tv_requests, generate_tv_requests};
pub use settings::{Flavor, IndexerSettings};
