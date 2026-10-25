//! Public sites on the real internet. Ignored by default: they need a network
//! and the sites change. `OMP_LIVE_IDS=nyaasi,thepiratebay cargo test -p
//! omp-cardigann --test live -- --ignored --nocapture`.

use std::sync::Arc;

use omp_cardigann::{
    Config, Http, HttpRequest, HttpResponse, Indexer, Method, Query, def::Definition,
};

struct Web(reqwest::Client);

#[async_trait::async_trait]
impl Http for Web {
    async fn send(&self, r: HttpRequest) -> Result<HttpResponse, String> {
        let mut b = match r.method {
            Method::Get => self.0.get(&r.url),
            Method::Post => self.0.post(&r.url),
        };
        for (k, v) in &r.headers {
            b = b.header(k, v);
        }
        if let Some((ct, body)) = r.body {
            b = b.header("content-type", ct).body(body);
        }
        let res = b.send().await.map_err(|e| e.without_url().to_string())?;
        let status = res.status().as_u16();
        let headers = res
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or_default().to_owned()))
            .collect();
        let body = res.bytes().await.map_err(|e| e.to_string())?.to_vec();
        Ok(HttpResponse {
            status,
            headers,
            body,
        })
    }
}

fn web() -> Arc<Web> {
    Arc::new(Web(
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36")
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap(),
    ))
}

#[tokio::test]
#[ignore = "needs the internet"]
async fn public_sites_answer_a_search() {
    let ids = std::env::var("OMP_LIVE_IDS").unwrap_or_else(|_| "nyaasi,thepiratebay,yts,eztv,limetorrents,1337x,torrentdownloads,tokyotosho,linuxtracker".into());
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("definitions");
    let query = std::env::var("OMP_LIVE_QUERY").unwrap_or_else(|_| "ubuntu".into());
    let mut report = vec![];
    for id in ids.split(',') {
        let def =
            Definition::from_yaml(&std::fs::read_to_string(dir.join(format!("{id}.yml"))).unwrap())
                .unwrap();
        let idx = Indexer::new(Arc::new(def), Config::new(), web()).unwrap();
        let line = match idx
            .search(&Query {
                q: query.clone(),
                ..Default::default()
            })
            .await
        {
            Ok(found) => {
                let first = found
                    .first()
                    .map(|r| {
                        format!(
                            "{} | {} B | seeders {:?} | {:?}",
                            r.title,
                            r.size,
                            r.seeders,
                            r.download_url
                                .as_deref()
                                .or(r.magnet.as_deref())
                                .map(|s| &s[..s.len().min(60)])
                        )
                    })
                    .unwrap_or_default();
                format!("{id}: {} results; {first}", found.len())
            }
            Err(e) => format!("{id}: ERROR {e}"),
        };
        println!("{line}");
        report.push(line);
    }
}
