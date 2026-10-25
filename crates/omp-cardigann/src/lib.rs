//! Prowlarr's indexer definitions, and an engine that searches the sites they describe.

pub mod cats;
pub mod def;
pub mod dom;
pub mod engine;
pub mod filters;
pub mod http;
pub mod json;
pub mod library;
pub mod select;
pub mod template;
pub mod util;

pub use engine::{Config, Fetched, Indexer, Query, QueryKind, Release};
pub use http::{Http, HttpRequest, HttpResponse, Method};
pub use library::{Field, Library, Summary};

/// Why a search, sign-in or download did not work.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("network error: {0}")]
    Network(String),
    #[error("{0}")]
    Http(String),
    #[error("sign-in failed: {0}")]
    Login(String),
    #[error("{0}")]
    Blocked(String),
    #[error("could not read the site's answer: {0}")]
    Parse(String),
    #[error("problem in the site's definition: {0}")]
    Definition(String),
    #[error("not supported yet: {0}")]
    Unsupported(String),
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_bundled_definition_is_read() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("definitions");
        let mut failed = vec![];
        let mut n = 0;
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.extension().is_some_and(|x| x == "yml") {
                n += 1;
                let text = std::fs::read_to_string(&p).unwrap();
                if let Err(e) = crate::def::Definition::from_yaml(&text) {
                    failed.push(format!("{}: {e}", p.file_name().unwrap().to_string_lossy()));
                }
            }
        }
        assert!(n > 500, "found {n}");
        assert!(
            failed.is_empty(),
            "{} failed:\n{}",
            failed.len(),
            failed.join("\n")
        );
    }
}
