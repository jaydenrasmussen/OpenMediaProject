#![allow(dead_code)]
use omp_indexers::{Flavor, IndexerIdentity, Source};

/// A parse source for tests that call `parse_feed` directly.
pub fn source(flavor: Flavor) -> Source<'static> {
    source_with_url(flavor, "https://idx.example/api?t=movie")
}

pub fn source_with_url(flavor: Flavor, request_url: &str) -> Source<'_> {
    Source {
        id: 7,
        name: "Example",
        priority: 25,
        flavor,
        request_url,
        seed_ratio: None,
        seed_time_minutes: None,
    }
}

pub fn identity() -> IndexerIdentity {
    IndexerIdentity {
        id: 7,
        name: "Example".into(),
        priority: 25,
    }
}
