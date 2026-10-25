//! Every definition that ships can be loaded and asked a question, without
//! touching the network: its selectors parse, its filters exist, and its
//! search builds a request.

use std::sync::Arc;

use omp_cardigann::{
    Config, Error, Http, HttpRequest, HttpResponse, Indexer, Query, QueryKind, def::Definition,
    select::SelectorList,
};

struct NoNetwork;

#[async_trait::async_trait]
impl Http for NoNetwork {
    async fn send(&self, _: HttpRequest) -> Result<HttpResponse, String> {
        Err("no network in this test".into())
    }
}

fn all() -> Vec<(String, Definition)> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("definitions");
    let mut out = vec![];
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_some_and(|x| x == "yml") {
            let text = std::fs::read_to_string(&p).unwrap();
            out.push((
                p.file_name().unwrap().to_string_lossy().into_owned(),
                Definition::from_yaml(&text).unwrap(),
            ));
        }
    }
    out
}

const FILTERS: &[&str] = &[
    "querystring",
    "timeparse",
    "dateparse",
    "regexp",
    "re_replace",
    "split",
    "replace",
    "trim",
    "prepend",
    "append",
    "tolower",
    "toupper",
    "base64decode",
    "base64encode",
    "urldecode",
    "urlencode",
    "htmldecode",
    "htmlencode",
    "timeago",
    "reltime",
    "fuzzytime",
    "validfilename",
    "diacritics",
    "jsonjoinarray",
    "hexdump",
    "strdump",
    "validate",
    "andmatch",
];

fn selectors_of(def: &Definition) -> Vec<String> {
    let mut v: Vec<String> = vec![];
    v.extend(def.search.rows.block.selector.clone());
    for (_, f) in def.search.fields.iter() {
        v.extend(f.selector.clone());
        v.extend(f.remove.clone());
        v.extend(f.case.iter().map(|(k, _)| k.clone()));
    }
    if let Some(l) = &def.login {
        v.extend(l.form.clone());
        v.extend(l.error.iter().filter_map(|e| e.selector.clone()));
        v.extend(l.test.iter().filter_map(|t| t.selector.clone()));
    }
    v
}

#[test]
fn the_selectors_of_html_definitions_parse() {
    let mut bad = vec![];
    for (file, def) in all() {
        let json_like = def
            .search
            .paths
            .iter()
            .any(|p| p.response.as_ref().is_some_and(|r| r.kind == "json"));
        if json_like {
            continue;
        }
        for s in selectors_of(&def) {
            if s.contains("{{") {
                continue;
            }
            // `:root` is handled before parsing.
            let s = s.trim_start_matches(":root").trim().to_owned();
            if s.is_empty() {
                continue;
            }
            if let Err(e) = SelectorList::parse(&s) {
                bad.push(format!("{file}: {s}: {e}"));
            }
        }
    }
    assert!(
        bad.is_empty(),
        "{} selectors do not parse:\n{}",
        bad.len(),
        bad.join("\n")
    );
}

#[test]
fn every_filter_a_definition_uses_exists() {
    let mut unknown = std::collections::BTreeSet::new();
    for (file, def) in all() {
        let mut lists: Vec<&Vec<omp_cardigann::def::Filter>> = vec![
            &def.search.keywordsfilters,
            &def.search.preprocessingfilters,
            &def.search.rows.block.filters,
        ];
        for (_, f) in def.search.fields.iter() {
            lists.push(&f.filters);
        }
        if let Some(d) = &def.download {
            for s in &d.selectors {
                lists.push(&s.filters);
            }
        }
        for l in lists {
            for f in l {
                if !FILTERS.contains(&f.name.as_str()) {
                    unknown.insert(format!("{file}: {}", f.name));
                }
            }
        }
    }
    assert!(unknown.is_empty(), "{unknown:?}");
}

#[tokio::test]
async fn every_definition_builds_a_search_request() {
    let mut problems = vec![];
    for (file, def) in all() {
        let def = Arc::new(def);
        let indexer = match Indexer::new(def.clone(), Config::new(), Arc::new(NoNetwork)) {
            Ok(i) => i,
            Err(e) => {
                problems.push(format!("{file}: {e}"));
                continue;
            }
        };
        for kind in [QueryKind::Search, QueryKind::Movie, QueryKind::Tv] {
            let q = Query {
                kind,
                q: "The Matrix".into(),
                year: Some(1999),
                season: Some(1),
                ep: Some(2),
                episode: Some("S01E02".into()),
                ..Default::default()
            };
            // The search fails only because there is no network; anything else is a definition we cannot read.
            match indexer.search(&q).await {
                Err(Error::Network(_)) | Err(Error::Login(_)) | Err(Error::Unsupported(_)) => {}
                Err(e) => problems.push(format!("{file} {kind:?}: {e}")),
                Ok(_) => {}
            }
        }
    }
    assert!(
        problems.is_empty(),
        "{} problems:\n{}",
        problems.len(),
        problems.join("\n")
    );
}
