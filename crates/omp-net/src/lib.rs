//! One place that decides how the app talks to the web: the user agent and,
//! when configured, an outbound proxy. Every HTTP client starts from
//! [`client_builder`].

use std::sync::RwLock;

/// An outbound HTTP(S) proxy.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Proxy {
    /// `http`, `https` or `socks5`.
    pub scheme: String,
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
    /// Hosts, domains or addresses that are reached directly.
    pub bypass: Vec<String>,
}

static PROXY: RwLock<Option<Proxy>> = RwLock::new(None);

/// Sets the proxy clients built from now on will use. Clients already built
/// keep what they had, which is why the setting applies at start-up.
pub fn set_proxy(proxy: Option<Proxy>) {
    *PROXY.write().unwrap() = proxy;
}

pub fn proxy() -> Option<Proxy> {
    PROXY.read().unwrap().clone()
}

/// The URL of a proxy, with credentials when it has them.
pub fn proxy_url(p: &Proxy) -> String {
    let scheme = match p.scheme.as_str() {
        "https" | "socks5" => p.scheme.as_str(),
        _ => "http",
    };
    match (&p.username, &p.password) {
        (Some(user), pass) if !user.is_empty() => format!(
            "{scheme}://{}:{}@{}:{}",
            encode(user),
            encode(pass.as_deref().unwrap_or_default()),
            p.host,
            p.port
        ),
        _ => format!("{scheme}://{}:{}", p.host, p.port),
    }
}

fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// A client builder with our user agent and the configured proxy.
pub fn client_builder() -> reqwest::ClientBuilder {
    let builder = reqwest::Client::builder()
        .user_agent(concat!("OpenMediaProject/", env!("CARGO_PKG_VERSION")));
    with_proxy(builder, proxy().as_ref())
}

fn with_proxy(builder: reqwest::ClientBuilder, proxy: Option<&Proxy>) -> reqwest::ClientBuilder {
    let Some(p) = proxy.filter(|p| !p.host.is_empty() && p.port != 0) else {
        return builder;
    };
    match reqwest::Proxy::all(proxy_url(p)) {
        Ok(proxy) => {
            builder.proxy(proxy.no_proxy(reqwest::NoProxy::from_string(&p.bypass.join(","))))
        }
        // A bad address is the user's to fix; going direct would leak what
        // the proxy is there to hide, so the request fails instead.
        Err(_) => builder
            .proxy(reqwest::Proxy::all("http://127.0.0.1:1").expect("a fixed, valid address")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_carry_credentials_escaped() {
        let mut p = Proxy {
            scheme: "socks5".into(),
            host: "10.0.0.2".into(),
            port: 1080,
            ..Default::default()
        };
        assert_eq!(proxy_url(&p), "socks5://10.0.0.2:1080");
        p.username = Some("me@home".into());
        p.password = Some("p:w/d".into());
        assert_eq!(proxy_url(&p), "socks5://me%40home:p%3Aw%2Fd@10.0.0.2:1080");
        p.scheme = "ftp".into();
        p.username = None;
        assert_eq!(
            proxy_url(&p),
            "http://10.0.0.2:1080",
            "unknown schemes are http"
        );
    }
}
