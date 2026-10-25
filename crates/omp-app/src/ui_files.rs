//! Serves the web UI, from a folder or from the binary.
//!
//! The UI is a static export built for the stand-in URL base
//! [`BASE_PLACEHOLDER`]. Text files get the real base (a reverse-proxy
//! prefix such as `/media`, or nothing) swapped in as they are served, so one
//! build works behind any prefix.

use std::{borrow::Cow, path::Path, sync::Arc};

use axum::{
    body::Body,
    http::{HeaderValue, StatusCode, Uri, header},
    response::{IntoResponse, Response},
};

/// What the UI build uses where the URL base goes.
pub const BASE_PLACEHOLDER: &str = "/__omp_base__";

/// A UI compiled into the binary: file name in, bytes out.
pub type EmbeddedUi = fn(&str) -> Option<Cow<'static, [u8]>>;

#[derive(Clone)]
pub struct UiFiles {
    dir: Arc<std::path::PathBuf>,
    embedded: Option<EmbeddedUi>,
    base: Arc<str>,
}

/// `""` for none, else `/prefix` with no trailing slash.
pub fn normalize_base(raw: &str) -> String {
    let trimmed = raw.trim().trim_matches('/');
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("/{trimmed}")
    }
}

impl UiFiles {
    pub fn new(dir: &Path, embedded: Option<EmbeddedUi>, base: &str) -> Self {
        Self {
            dir: Arc::new(dir.to_path_buf()),
            embedded,
            base: normalize_base(base).into(),
        }
    }

    async fn read(&self, path: &str) -> Option<Vec<u8>> {
        if path
            .split('/')
            .any(|part| part == ".." || part.contains('\\'))
        {
            return None;
        }
        if let Some(embedded) = self.embedded {
            return embedded(path).map(Cow::into_owned);
        }
        let file = self.dir.join(path);
        // A folder is not a file.
        match tokio::fs::metadata(&file).await {
            Ok(m) if m.is_file() => tokio::fs::read(&file).await.ok(),
            _ => None,
        }
    }

    /// The file a request path means: a folder means its `index.html`, and a
    /// page path without a slash means the page's folder.
    async fn find(&self, uri_path: &str) -> Option<(String, Vec<u8>)> {
        let rel = uri_path.trim_start_matches('/');
        let mut candidates = Vec::new();
        if rel.is_empty() || rel.ends_with('/') {
            candidates.push(format!("{rel}index.html"));
        } else {
            candidates.push(rel.to_owned());
            candidates.push(format!("{rel}/index.html"));
        }
        for c in candidates {
            if let Some(bytes) = self.read(&c).await {
                return Some((c, bytes));
            }
        }
        None
    }

    pub async fn serve(&self, uri: &Uri) -> Response {
        let path = uri.path();
        let (name, bytes) = match self.find(path).await {
            Some(found) => found,
            None => {
                // Pages are client-routed, so an unknown page is the app's
                // shell. Anything that looks like a file is just missing.
                let last = path.rsplit('/').next().unwrap_or_default();
                if last.contains('.') {
                    return StatusCode::NOT_FOUND.into_response();
                }
                match self.read("index.html").await {
                    Some(bytes) => ("index.html".to_owned(), bytes),
                    None => return StatusCode::NOT_FOUND.into_response(),
                }
            }
        };
        let mime = mime_guess::from_path(&name).first_or_octet_stream();
        let is_text = matches!(
            mime.essence_str(),
            "text/html"
                | "text/css"
                | "text/plain"
                | "application/javascript"
                | "text/javascript"
                | "application/json"
                | "image/svg+xml"
                | "application/manifest+json"
        );
        let body = if is_text {
            let text = String::from_utf8_lossy(&bytes);
            text.replace(BASE_PLACEHOLDER, &self.base).into_bytes()
        } else {
            bytes
        };
        let mut response = Response::new(Body::from(body));
        let headers = response.headers_mut();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_str(mime.as_ref())
                .unwrap_or(HeaderValue::from_static("application/octet-stream")),
        );
        // Built files are named by their content; pages change with releases.
        headers.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static(if name.starts_with("_next/static/") {
                "public, max-age=31536000, immutable"
            } else {
                "no-cache"
            }),
        );
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bases_are_normalised() {
        assert_eq!(normalize_base(""), "");
        assert_eq!(normalize_base("/"), "");
        assert_eq!(normalize_base("media"), "/media");
        assert_eq!(normalize_base(" /media/ "), "/media");
        assert_eq!(normalize_base("/a/b/"), "/a/b");
    }
}
