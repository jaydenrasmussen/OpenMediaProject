//! What the engine needs from the network, and the cookies and redirects it
//! looks after itself so the same engine runs over any HTTP client.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use url::Url;

use crate::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
}

#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub method: Method,
    pub url: String,
    pub headers: Vec<(String, String)>,
    /// A body, and its content type.
    pub body: Option<(String, Vec<u8>)>,
}

#[derive(Debug, Clone, Default)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    pub fn set_cookies(&self) -> impl Iterator<Item = &str> {
        self.headers
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case("set-cookie"))
            .map(|(_, v)| v.as_str())
    }
}

/// One request, once: no cookies and no redirects, which the engine adds.
#[async_trait]
pub trait Http: Send + Sync {
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse, String>;
}

/// A response with its text decoded, where it ended up, and any redirect not followed.
#[derive(Debug, Clone)]
pub struct Page {
    pub status: u16,
    pub url: String,
    pub content_type: String,
    pub text: String,
    pub bytes: Vec<u8>,
    /// Where a redirect that was not followed points.
    pub redirect: Option<String>,
    pub server: String,
}

/// The cookies a site has set.
#[derive(Default)]
pub struct Jar(Mutex<BTreeMap<String, String>>);

impl Jar {
    pub fn clear(&self) {
        self.0.lock().expect("cookie jar").clear();
    }

    pub fn is_empty(&self) -> bool {
        self.0.lock().expect("cookie jar").is_empty()
    }

    pub fn set(&self, name: &str, value: &str) {
        self.0
            .lock()
            .expect("cookie jar")
            .insert(name.to_owned(), value.to_owned());
    }

    /// "a=1; b=2" as the user may paste it.
    pub fn load_header(&self, header: &str) {
        for part in header.split(';') {
            if let Some((k, v)) = part.split_once('=')
                && !k.trim().is_empty()
            {
                self.set(k.trim(), v.trim());
            }
        }
    }

    fn header(&self) -> Option<String> {
        let jar = self.0.lock().expect("cookie jar");
        (!jar.is_empty()).then(|| {
            jar.iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join("; ")
        })
    }

    fn absorb(&self, response: &HttpResponse) {
        for line in response.set_cookies() {
            let mut parts = line.split(';');
            let Some((name, value)) = parts.next().and_then(|p| p.split_once('=')) else {
                continue;
            };
            let expired = parts.any(|p| {
                let p = p.trim().to_ascii_lowercase();
                p == "max-age=0"
                    || p.starts_with("max-age=-")
                    || p.starts_with("expires=thu, 01 jan 1970")
            });
            if expired || value.trim().is_empty() || value.trim() == "deleted" {
                self.0.lock().expect("cookie jar").remove(name.trim());
            } else {
                self.set(name.trim(), value.trim());
            }
        }
    }
}

pub struct Client {
    pub http: Arc<dyn Http>,
    pub jar: Jar,
    pub encoding: &'static encoding_rs::Encoding,
    /// The definition names its encoding; otherwise the response says.
    pub encoding_explicit: bool,
}

impl Client {
    pub async fn fetch(
        &self,
        mut request: HttpRequest,
        follow: bool,
        referer: Option<&str>,
    ) -> Result<Page, Error> {
        let mut hops = 0;
        loop {
            let mut sent = request.clone();
            if let Some(cookie) = self.jar.header()
                && !sent
                    .headers
                    .iter()
                    .any(|(k, _)| k.eq_ignore_ascii_case("cookie"))
            {
                sent.headers.push(("Cookie".into(), cookie));
            }
            if let Some(r) = referer
                && !sent
                    .headers
                    .iter()
                    .any(|(k, _)| k.eq_ignore_ascii_case("referer"))
            {
                sent.headers.push(("Referer".into(), r.to_owned()));
            }
            let response = self.http.send(sent).await.map_err(Error::Network)?;
            self.jar.absorb(&response);
            let location = (300..400)
                .contains(&response.status)
                .then(|| response.header("location").map(str::to_owned))
                .flatten()
                .and_then(|l| Url::parse(&request.url).ok()?.join(&l).ok())
                .map(|u| u.to_string());
            match location {
                Some(next) if follow && hops < 10 => {
                    hops += 1;
                    // A redirect after a POST is read with GET.
                    if matches!(response.status, 301..=303) {
                        request.method = Method::Get;
                        request.body = None;
                    }
                    request.url = next;
                }
                other => {
                    let content_type = response
                        .header("content-type")
                        .unwrap_or_default()
                        .to_lowercase();
                    let encoding = if self.encoding_explicit {
                        self.encoding
                    } else {
                        charset(&content_type).unwrap_or(self.encoding)
                    };
                    let text = encoding.decode(&response.body).0.into_owned();
                    return Ok(Page {
                        status: response.status,
                        url: request.url,
                        content_type,
                        text,
                        server: response.header("server").unwrap_or_default().to_lowercase(),
                        bytes: response.body,
                        redirect: other,
                    });
                }
            }
        }
    }
}

fn charset(content_type: &str) -> Option<&'static encoding_rs::Encoding> {
    let at = content_type.find("charset=")?;
    let label = content_type[at + 8..]
        .split(';')
        .next()?
        .trim()
        .trim_matches('"');
    encoding_rs::Encoding::for_label(label.as_bytes())
}

/// Form fields as a request body in the site's encoding.
pub fn form_body(
    pairs: &[(String, String)],
    encoding: &'static encoding_rs::Encoding,
) -> (String, Vec<u8>) {
    let body = pairs
        .iter()
        .map(|(k, v)| {
            format!(
                "{}={}",
                crate::util::url_encode(k, encoding),
                crate::util::url_encode(v, encoding)
            )
        })
        .collect::<Vec<_>>()
        .join("&");
    (
        "application/x-www-form-urlencoded".into(),
        body.into_bytes(),
    )
}
