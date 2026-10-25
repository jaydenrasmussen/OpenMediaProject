//! Searches a site by its definition: builds the requests, signs in when the
//! site wants it, reads the pages (HTML, JSON or XML) into releases, and
//! finds the file to download. Follows Prowlarr's Cardigann implementation
//! (GPL-3.0) step for step so the shared definitions behave the same.

use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::Error;
use crate::cats::{self, Categories};
use crate::def::{Definition, Filter, Rows, SearchPath, Selector, SelectorField};
use crate::dom::{Dom, Id};
use crate::filters::{self, Context};
use crate::http::{Client, HttpRequest, Jar, Method, Page, form_body};
use crate::json;
use crate::select::SelectorList;
use crate::template::{self, Val, Vars};
use crate::util;

/// What the user filled in for a site: its settings by name, plus `base_url`.
pub type Config = BTreeMap<String, Value>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QueryKind {
    /// Just words.
    #[default]
    Search,
    Movie,
    Tv,
}

#[derive(Debug, Clone, Default)]
pub struct Query {
    pub kind: QueryKind,
    /// The words to search for; empty asks for the newest.
    pub q: String,
    pub year: Option<i32>,
    /// "tt0133093".
    pub imdb_id: Option<String>,
    pub tmdb_id: Option<i64>,
    pub tvdb_id: Option<i64>,
    pub season: Option<i32>,
    pub ep: Option<i32>,
    /// "S01E02", or a date for daily shows.
    pub episode: Option<String>,
    /// Newznab categories.
    pub categories: Vec<i32>,
}

/// What a site lists for one result.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Release {
    pub guid: String,
    pub title: String,
    pub description: Option<String>,
    pub details: Option<String>,
    pub comments: Option<String>,
    pub download_url: Option<String>,
    pub magnet: Option<String>,
    pub info_hash: Option<String>,
    pub size: u64,
    pub seeders: Option<i32>,
    /// Seeders and leechers together.
    pub peers: Option<i32>,
    pub grabs: Option<i32>,
    pub files: Option<i32>,
    pub publish_date: Option<DateTime<Utc>>,
    pub categories: Vec<i32>,
    pub download_volume_factor: Option<f64>,
    pub upload_volume_factor: Option<f64>,
    pub minimum_ratio: Option<f64>,
    /// Seconds.
    pub minimum_seed_time: Option<i64>,
    pub imdb_id: Option<i32>,
    pub tmdb_id: Option<i32>,
    pub tvdb_id: Option<i32>,
}

/// What a download link turned out to be.
#[derive(Debug, Clone, PartialEq)]
pub enum Fetched {
    Magnet(String),
    Torrent(Vec<u8>),
}

struct Req {
    url: String,
    method: Method,
    form: Vec<(String, String)>,
    headers: Vec<(String, String)>,
    follow: bool,
    path: SearchPath,
    vars: Vars,
}

pub struct Indexer {
    def: Arc<Definition>,
    config: Config,
    site: String,
    client: Client,
    cats: Categories,
    default_cats: Vec<String>,
    logged_in: Mutex<bool>,
}

const OPTIONAL_FIELDS: [&str; 12] = [
    "imdb",
    "imdbid",
    "tmdbid",
    "rageid",
    "tvdbid",
    "tvmazeid",
    "traktid",
    "doubanid",
    "poster",
    "banner",
    "description",
    "genre",
];

fn text_of(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Bool(b) => Some(if *b { "true" } else { "false" }.into()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn truthy_text(s: &str) -> bool {
    matches!(s.trim().to_ascii_lowercase().as_str(), "true" | "1" | "yes")
}

impl Indexer {
    pub fn new(
        def: Arc<Definition>,
        config: Config,
        http: Arc<dyn crate::http::Http>,
    ) -> Result<Self, Error> {
        let encoding_name = def.encoding.clone().unwrap_or_else(|| "UTF-8".into());
        let encoding = encoding_rs::Encoding::for_label(encoding_name.as_bytes())
            .ok_or_else(|| Error::Definition(format!("unknown text encoding '{encoding_name}'")))?;
        let explicit = def
            .encoding
            .as_deref()
            .is_some_and(|e| !e.eq_ignore_ascii_case("utf-8") && !e.eq_ignore_ascii_case("utf8"));

        let configured = config
            .get("base_url")
            .and_then(text_of)
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty());
        let first = def
            .links
            .first()
            .cloned()
            .ok_or_else(|| Error::Definition("the definition has no site address".into()))?;
        let mut site = match configured {
            Some(c) if !def.legacylinks.contains(&c) => c,
            _ => first,
        };
        if !site.ends_with('/')
            && url::Url::parse(&site).is_ok_and(|u| u.path().is_empty() || u.path() == "/")
        {
            site.push('/');
        }

        let mut categories = Categories::default();
        for (id, name) in def.caps.categories.iter() {
            match cats::by_name(&name.0) {
                Some(c) => categories.add(id, Some(c), None),
                None => tracing_free_note(&def.id, id, &name.0),
            }
        }
        let mut default_cats = vec![];
        for m in &def.caps.categorymappings {
            let cat = match &m.cat {
                Some(name) => match cats::by_name(name) {
                    Some(c) => Some(c),
                    None => {
                        tracing_free_note(&def.id, &m.id.0, name);
                        continue;
                    }
                },
                None => None,
            };
            categories.add(&m.id.0, cat, m.desc.as_deref());
            if m.default {
                default_cats.push(m.id.0.clone());
            }
        }

        let client = Client {
            http,
            jar: Jar::default(),
            encoding,
            encoding_explicit: explicit,
        };
        // A cookie the user pasted is the session.
        let me = Self {
            def,
            config,
            site,
            client,
            cats: categories,
            default_cats,
            logged_in: Mutex::new(false),
        };
        if me.def.login.as_ref().is_some_and(|l| l.method == "cookie")
            && let Some(c) = me.config.get("cookie").and_then(text_of)
        {
            me.client.jar.load_header(&c);
        }
        Ok(me)
    }

    pub fn definition(&self) -> &Definition {
        &self.def
    }

    pub fn site(&self) -> &str {
        &self.site
    }

    pub fn categories(&self) -> &Categories {
        &self.cats
    }

    fn encoding(&self) -> &'static encoding_rs::Encoding {
        self.client.encoding
    }

    // ---- templates and filters ------------------------------------------

    fn tmpl(&self, t: &str, vars: &Vars) -> Result<String, Error> {
        template::apply(t, vars, None).map_err(Error::Definition)
    }

    fn tmpl_encoded(&self, t: &str, vars: &Vars) -> Result<String, Error> {
        let enc = self.encoding();
        let modifier: &dyn Fn(&str) -> String = &move |s| util::url_encode(s, enc);
        template::apply(t, vars, Some(modifier)).map_err(Error::Definition)
    }

    fn filter(&self, data: String, filters: &[Filter], vars: &Vars) -> Result<String, String> {
        filters::apply(
            data,
            filters,
            &Context {
                vars,
                encoding: self.encoding(),
            },
        )
    }

    /// Every setting as a variable, with the defaults for what is not set.
    fn base_vars(&self) -> Vars {
        let mut v = Vars::new();
        v.insert(".Config.sitelink".into(), Val::str(self.site.clone()));
        v.insert(".True".into(), Val::str("True"));
        v.insert(".False".into(), Val::Null);
        v.insert(
            ".Today.Year".into(),
            Val::str(Utc::now().format("%Y").to_string()),
        );
        for s in self.settings() {
            let key = format!(".Config.{}", s.name);
            let given = self.config.get(&s.name).and_then(text_of);
            let default = s.default.as_ref().map(|d| d.0.clone());
            match s.kind.as_str() {
                "text" | "password" => {
                    v.insert(key, Val::opt(given.or(default)));
                }
                "checkbox" => {
                    let on = given.or(default).is_some_and(|x| truthy_text(&x));
                    v.insert(key, if on { Val::str("True") } else { Val::Null });
                }
                "select" => {
                    let mut chosen = given.filter(|g| s.options.get(g).is_some()).or(default);
                    if chosen.as_ref().is_none_or(|c| s.options.get(c).is_none()) {
                        chosen = s.options.iter().map(|(k, _)| k.clone()).min();
                    }
                    v.insert(key, Val::opt(chosen));
                }
                _ => {}
            }
        }
        v
    }

    /// The fields a user fills in: the definition's, or a name and password.
    pub fn settings(&self) -> Vec<crate::def::SettingField> {
        effective_settings(&self.def)
    }

    // ---- selectors --------------------------------------------------------

    fn query_first(&self, dom: &Dom, el: Id, selector: &str) -> Result<Option<Id>, String> {
        let mut scope = el;
        let mut selector = selector;
        if let Some(rest) = selector.strip_prefix(":root") {
            selector = rest.trim_start();
            while let Some(p) = dom.parent_element(scope) {
                scope = p;
            }
            if selector.is_empty() {
                return Ok(Some(scope));
            }
        }
        Ok(SelectorList::parse(selector)?.select_first(dom, scope))
    }

    fn handle_selector(
        &self,
        dom: &mut Dom,
        el: Id,
        sel: &Selector,
        vars: &Vars,
        required: bool,
    ) -> Result<Option<String>, String> {
        if let Some(text) = &sel.text {
            let t = template::apply(&text.0, vars, None)?;
            return self.filter(t, &sel.filters, vars).map(Some);
        }
        let mut selection = el;
        if let Some(s) = &sel.selector {
            let s = template::apply(s, vars, None)?;
            let list = SelectorList::parse(&s)?;
            selection = if !s.starts_with(":root") && list.matches(dom, el) {
                el
            } else {
                match self.query_first(dom, el, &s)? {
                    Some(found) => found,
                    None if required => return Err(format!("selector \"{s}\" matched nothing")),
                    None => return Ok(None),
                }
            };
        }
        if let Some(remove) = &sel.remove {
            for i in SelectorList::parse(remove)?.select_all(dom, selection) {
                dom.remove(i);
            }
        }
        let value = if !sel.case.is_empty() {
            let mut hit = None;
            for (k, v) in sel.case.iter() {
                let list = SelectorList::parse(k)?;
                if list.matches(dom, selection) || list.select_first(dom, selection).is_some() {
                    hit = Some(template::apply(&v.0, vars, None)?);
                    break;
                }
            }
            match hit {
                Some(h) => h,
                None if required => return Err("none of the case selectors matched".into()),
                None => return Ok(None),
            }
        } else if let Some(attr) = &sel.attribute {
            match dom.attr(selection, attr) {
                Some(v) => v.to_owned(),
                None if required => return Err(format!("attribute \"{attr}\" is not set")),
                None => return Ok(None),
            }
        } else {
            dom.text_content(selection)
        };
        self.filter(value.trim().to_owned(), &sel.filters, vars)
            .map(Some)
    }

    fn handle_json_selector(
        &self,
        parent: &Value,
        sel: &Selector,
        vars: &Vars,
        required: bool,
    ) -> Result<Option<String>, String> {
        if let Some(text) = &sel.text {
            let t = template::apply(&text.0, vars, None)?;
            return self.filter(t, &sel.filters, vars).map(Some);
        }
        let mut value: Option<String> = None;
        if let Some(s) = &sel.selector {
            let s = template::apply(s.trim_start_matches('.'), vars, None)?;
            let path = json::field_selector(parent, &s);
            let selection = path.and_then(|p| {
                if p.trim().is_empty() {
                    Some(parent)
                } else {
                    json::select_token(parent, p)
                }
            });
            match selection {
                Some(v) => value = json::token_text(v),
                None if required => {
                    return Err(format!("selector \"{s}\" matched nothing in the JSON"));
                }
                None => return Ok(None),
            }
        }
        if !sel.case.is_empty() {
            let mut hit = None;
            for (k, v) in sel.case.iter() {
                if value.as_deref() == Some(k.as_str()) || k == "*" {
                    hit = Some(template::apply(&v.0, vars, None)?);
                    break;
                }
            }
            match hit {
                Some(h) => value = Some(h),
                None if required => return Err("none of the case selectors matched".into()),
                None => return Ok(None),
            }
        }
        match value {
            Some(v) => self
                .filter(v.trim().to_owned(), &sel.filters, vars)
                .map(Some),
            None if required => Err("no value".into()),
            None => Ok(None),
        }
    }

    // ---- requests -----------------------------------------------------------

    fn resolve(&self, path: &str, base: Option<&str>) -> Result<String, Error> {
        let base = url::Url::parse(base.unwrap_or(&self.site))
            .map_err(|e| Error::Definition(format!("bad address: {e}")))?;
        base.join(path)
            .map(|u| u.to_string())
            .map_err(|e| Error::Definition(format!("bad address '{path}': {e}")))
    }

    fn headers(
        &self,
        headers: &crate::def::Pairs<Vec<crate::def::Scalar>>,
        vars: &Vars,
    ) -> Result<Vec<(String, String)>, Error> {
        let mut out = vec![];
        for (k, v) in headers.iter() {
            if let Some(first) = v.first() {
                out.push((k.clone(), self.tmpl(&first.0, vars)?));
            }
        }
        Ok(out)
    }

    fn query_vars(&self, q: &Query) -> Vars {
        let mut v = self.base_vars();
        let s = |x: Option<String>| Val::opt(x);
        let n = |x: Option<i64>| Val::opt(x.map(|n| n.to_string()));
        let ty = match q.kind {
            QueryKind::Search => "search",
            QueryKind::Movie => "movie",
            QueryKind::Tv => "tvsearch",
        };
        v.insert(".Query.Type".into(), Val::str(ty));
        v.insert(".Query.Q".into(), Val::str(q.q.clone()));
        v.insert(
            ".Query.Categories".into(),
            Val::List(q.categories.iter().map(|c| c.to_string()).collect()),
        );
        for name in [
            "Limit",
            "Offset",
            "Extended",
            "APIKey",
            "Genre",
            "Movie",
            "Year",
            "IMDBID",
            "IMDBIDShort",
            "TMDBID",
            "Series",
            "Ep",
            "Season",
            "TVDBID",
            "TVRageID",
            "TVMazeID",
            "TraktID",
            "DoubanID",
            "Episode",
            "Album",
            "Artist",
            "Label",
            "Track",
            "Author",
            "Title",
            "Publisher",
        ] {
            v.insert(format!(".Query.{name}"), Val::Null);
        }
        v.insert(".Query.Year".into(), n(q.year.map(i64::from)));
        let imdb = q.imdb_id.as_ref().filter(|i| !i.is_empty());
        v.insert(".Query.IMDBID".into(), s(imdb.cloned()));
        v.insert(
            ".Query.IMDBIDShort".into(),
            s(imdb.map(|i| {
                i.trim_start_matches("tt")
                    .trim_start_matches('0')
                    .to_owned()
            })),
        );
        v.insert(".Query.TMDBID".into(), n(q.tmdb_id));
        v.insert(".Query.TVDBID".into(), n(q.tvdb_id));
        v.insert(".Query.Season".into(), n(q.season.map(i64::from)));
        v.insert(".Query.Ep".into(), n(q.ep.map(i64::from)));
        v.insert(".Query.Episode".into(), s(q.episode.clone()));
        v
    }

    fn requests(&self, q: &Query) -> Result<Vec<Req>, Error> {
        let search = &self.def.search;
        let mut vars = self.query_vars(q);

        let mut mapped = self.cats.to_tracker(&q.categories);
        if mapped.is_empty() {
            mapped = self.default_cats.clone();
        }

        let mut tokens = vec![];
        for key in ["Q", "Series", "Movie", "Year"] {
            if let Val::Str(s) = vars
                .get(&format!(".Query.{key}"))
                .cloned()
                .unwrap_or_default()
                && !s.trim().is_empty()
            {
                tokens.push(s);
            }
        }
        if let Some(e) = q.episode.as_ref().filter(|e| !e.trim().is_empty()) {
            tokens.push(e.clone());
        }
        let keywords = tokens.join(" ");
        vars.insert(".Query.Keywords".into(), Val::str(keywords.clone()));
        let filtered = self
            .filter(keywords, &search.keywordsfilters, &vars)
            .map_err(Error::Definition)?;
        vars.insert(".Keywords".into(), Val::str(filtered));

        let mut paths = search.paths.clone();
        if paths.is_empty()
            && let Some(p) = &search.path
        {
            paths.push(SearchPath {
                path: p.clone(),
                inheritinputs: true,
                ..Default::default()
            });
        }
        let mut out: Vec<Req> = vec![];
        let mut seen = HashSet::new();
        for sp in paths {
            vars.insert(".Categories".into(), Val::List(mapped.clone()));
            let wanted: Vec<String> = sp.categories.iter().map(|c| c.0.clone()).collect();
            if !wanted.is_empty() && !mapped.is_empty() {
                let mut hit = mapped.iter().any(|m| wanted.contains(m));
                if wanted[0] == "!" {
                    hit = !hit;
                }
                if !hit {
                    vars.insert(
                        ".Categories".into(),
                        Val::List(
                            mapped
                                .iter()
                                .filter(|m| !wanted.contains(m))
                                .cloned()
                                .collect(),
                        ),
                    );
                    continue;
                }
                vars.insert(
                    ".Categories".into(),
                    Val::List(
                        mapped
                            .iter()
                            .filter(|m| wanted.contains(m))
                            .cloned()
                            .collect(),
                    ),
                );
            }

            let path = self.tmpl_encoded(&sp.path, &vars)?.replace('+', "%20");
            let mut url = self.resolve(&path, None)?;
            let method = if sp.method.eq_ignore_ascii_case("post") {
                Method::Post
            } else {
                Method::Get
            };

            let mut pairs: Vec<(String, String)> = vec![];
            let mut sets: Vec<&crate::def::Pairs<crate::def::Scalar>> = vec![];
            if sp.inheritinputs {
                sets.push(&search.inputs);
            }
            sets.push(&sp.inputs);
            let mut raw_parts: Vec<String> = vec![];
            for inputs in sets {
                for (k, v) in inputs.iter() {
                    if k == "$raw" {
                        let raw = self.tmpl_encoded(&v.0, &vars)?;
                        for part in raw.split('&') {
                            if !part.is_empty() {
                                raw_parts.push(part.to_owned());
                            }
                        }
                    } else {
                        let value = self.tmpl(&v.0, &vars)?;
                        if !value.trim().is_empty() || search.allowemptyinputs {
                            pairs.push((k.clone(), value));
                        }
                    }
                }
            }
            let separator = sp.queryseparator.clone().unwrap_or_else(|| "&".into());
            let mut form = vec![];
            if method == Method::Get {
                let mut query: Vec<String> = pairs
                    .iter()
                    .map(|(k, v)| {
                        format!(
                            "{}={}",
                            util::url_encode(k, self.encoding()),
                            util::url_encode(v, self.encoding())
                        )
                    })
                    .collect();
                query.extend(raw_parts);
                if !query.is_empty() {
                    url.push(if url.contains('?') { '&' } else { '?' });
                    url.push_str(&query.join(&separator));
                }
                if !seen.insert(url.clone()) {
                    continue;
                }
            } else {
                form = pairs;
                for part in raw_parts {
                    if let Some((k, v)) = part.split_once('=') {
                        form.push((
                            util::url_decode(k, self.encoding()),
                            util::url_decode(v, self.encoding()),
                        ));
                    }
                }
            }
            let headers = self.headers(&search.headers, &vars)?;
            out.push(Req {
                url,
                method,
                form,
                headers,
                follow: sp.followredirect,
                path: sp.clone(),
                vars: vars.clone(),
            });
        }
        Ok(out)
    }

    fn http_request(&self, r: &Req) -> HttpRequest {
        HttpRequest {
            method: r.method,
            url: r.url.clone(),
            headers: r.headers.clone(),
            body: (r.method == Method::Post).then(|| form_body(&r.form, self.encoding())),
        }
    }

    // ---- signing in -----------------------------------------------------------

    fn login_needed(&self, page: &Page) -> bool {
        let Some(login) = &self.def.login else {
            return false;
        };
        if page.redirect.is_some() || page.status >= 400 {
            return true;
        }
        if let Some(sel) = login.test.as_ref().and_then(|t| t.selector.as_ref())
            && (page.content_type.is_empty() || page.content_type.contains("text/html"))
        {
            let dom = Dom::parse_html(&page.text);
            return SelectorList::parse(sel)
                .map(|l| l.select_first(&dom, 0).is_none())
                .unwrap_or(false);
        }
        false
    }

    fn check_for_error(
        &self,
        page: &Page,
        blocks: &[crate::def::ErrorBlock],
        vars: &Vars,
    ) -> Result<(), Error> {
        if page.status == 401 {
            return Err(Error::Login("the site refused the sign-in (401)".into()));
        }
        if blocks.is_empty() {
            return Ok(());
        }
        let mut dom = Dom::parse_html(&page.text);
        for b in blocks {
            let Some(sel) = &b.selector else { continue };
            let Some(hit) = SelectorList::parse(sel)
                .map_err(Error::Definition)?
                .select_first(&dom, 0)
            else {
                continue;
            };
            let message = match &b.message {
                Some(m) => {
                    let root = dom.element_children(0).next().unwrap_or(0);
                    self.handle_selector(&mut dom, root, m, vars, true)
                        .ok()
                        .flatten()
                }
                None => None,
            }
            .unwrap_or_else(|| dom.text_content(hit));
            return Err(Error::Login(message.trim().to_owned()));
        }
        Ok(())
    }

    fn login_headers(&self, vars: &Vars) -> Result<Vec<(String, String)>, Error> {
        let source = self
            .def
            .login
            .as_ref()
            .filter(|l| !l.headers.is_empty())
            .map(|l| &l.headers)
            .unwrap_or(&self.def.search.headers);
        self.headers(source, vars)
    }

    /// Signs in by the definition's method and keeps the cookies.
    pub async fn login(&self) -> Result<(), Error> {
        let Some(login) = self.def.login.clone() else {
            return Ok(());
        };
        let vars = self.base_vars();
        let headers = self.login_headers(&vars)?;
        let login_path = login.path.clone().unwrap_or_default();
        let result = match login.method.as_str() {
            "cookie" => {
                self.client.jar.clear();
                let cookie = self
                    .config
                    .get("cookie")
                    .and_then(text_of)
                    .unwrap_or_default();
                if cookie.trim().is_empty() {
                    return Err(Error::Login(
                        "this site needs the cookie from a browser that is signed in".into(),
                    ));
                }
                self.client.jar.load_header(&cookie);
                for c in &login.cookies {
                    self.client.jar.load_header(&c.0);
                }
                Ok(())
            }
            "post" => {
                let mut pairs = vec![];
                for (k, v) in login.inputs.iter() {
                    pairs.push((k.clone(), self.tmpl(&v.0, &vars)?));
                }
                let url = self.resolve(&self.tmpl(&login_path, &vars)?, None)?;
                self.client.jar.clear();
                for c in &login.cookies {
                    self.client.jar.load_header(&c.0);
                }
                let page = self
                    .client
                    .fetch(
                        HttpRequest {
                            method: Method::Post,
                            url,
                            headers,
                            body: Some(form_body(&pairs, self.encoding())),
                        },
                        true,
                        Some(&self.site),
                    )
                    .await?;
                self.check_for_error(&page, &login.error, &vars)
            }
            "get" | "oneurl" => {
                self.client.jar.clear();
                let url = if login.method == "get" {
                    let mut query = vec![];
                    for (k, v) in login.inputs.iter() {
                        query.push(format!(
                            "{}={}",
                            util::url_encode(k, self.encoding()),
                            util::url_encode(&self.tmpl(&v.0, &vars)?, self.encoding())
                        ));
                    }
                    format!(
                        "{}?{}",
                        self.resolve(&self.tmpl(&login_path, &vars)?, None)?,
                        query.join("&")
                    )
                } else {
                    let one = login
                        .inputs
                        .get("oneurl")
                        .map(|s| s.0.clone())
                        .unwrap_or_default();
                    let one = self.tmpl(&one, &vars)?;
                    self.resolve(&format!("{}{one}", self.tmpl(&login_path, &vars)?), None)?
                };
                let page = self
                    .client
                    .fetch(
                        HttpRequest {
                            method: Method::Get,
                            url,
                            headers,
                            body: None,
                        },
                        false,
                        Some(&self.site),
                    )
                    .await?;
                self.check_for_error(&page, &login.error, &vars)
            }
            "form" => self.login_form(&login, &vars, headers).await,
            other => Err(Error::Unsupported(format!("the sign-in method '{other}'"))),
        };
        result?;
        // Check the sign-in worked, where the definition says how.
        if let Some(test) = &login.test
            && let (Some(path), Some(sel)) = (&test.path, &test.selector)
        {
            let url = self.resolve(&self.tmpl(path, &vars)?, None)?;
            let page = self
                .client
                .fetch(
                    HttpRequest {
                        method: Method::Get,
                        url,
                        headers: vec![],
                        body: None,
                    },
                    true,
                    None,
                )
                .await?;
            let dom = Dom::parse_html(&page.text);
            if SelectorList::parse(sel)
                .map_err(Error::Definition)?
                .select_first(&dom, 0)
                .is_none()
            {
                return Err(Error::Login(
                    "the site did not accept this sign-in; check the name, password or cookie"
                        .into(),
                ));
            }
        }
        *self.logged_in.lock().expect("login state") = true;
        Ok(())
    }

    async fn login_form(
        &self,
        login: &crate::def::Login,
        vars: &Vars,
        headers: Vec<(String, String)>,
    ) -> Result<(), Error> {
        let login_url = self.resolve(
            &self.tmpl(login.path.as_deref().unwrap_or_default(), vars)?,
            None,
        )?;
        self.client.jar.clear();
        for c in &login.cookies {
            self.client.jar.load_header(&c.0);
        }
        let landing = self
            .client
            .fetch(
                HttpRequest {
                    method: Method::Get,
                    url: login_url.clone(),
                    headers: headers.clone(),
                    body: None,
                },
                self.def.followredirect,
                Some(&self.site),
            )
            .await?;
        let mut dom = Dom::parse_html(&landing.text);
        let form_sel = login.form.clone().unwrap_or_else(|| "form".into());
        let form = SelectorList::parse(&form_sel)
            .map_err(Error::Definition)?
            .select_first(&dom, 0)
            .ok_or_else(|| {
                Error::Login(format!("no form found on {login_url} by \"{form_sel}\""))
            })?;

        let mut pairs: Vec<(String, String)> = vec![];
        let put = |pairs: &mut Vec<(String, String)>, k: String, v: String| {
            if let Some(slot) = pairs.iter_mut().find(|(pk, _)| *pk == k) {
                slot.1 = v;
            } else {
                pairs.push((k, v));
            }
        };
        let input_sel = SelectorList::parse("input").map_err(Error::Definition)?;
        for input in input_sel.select_all(&dom, form) {
            let Some(name) = dom.attr(input, "name") else {
                continue;
            };
            if dom.attr(input, "disabled").is_some() {
                continue;
            }
            let kind = dom
                .attr(input, "type")
                .unwrap_or_default()
                .to_ascii_lowercase();
            if matches!(kind.as_str(), "checkbox" | "radio") && dom.attr(input, "checked").is_none()
            {
                continue;
            }
            put(
                &mut pairs,
                name.to_owned(),
                dom.attr(input, "value").unwrap_or_default().to_owned(),
            );
        }
        for (k, v) in login.inputs.iter() {
            let value = self.tmpl(&v.0, vars)?;
            let mut key = k.clone();
            if login.selectors {
                let el = SelectorList::parse(k)
                    .map_err(Error::Definition)?
                    .select_first(&dom, 0)
                    .ok_or_else(|| Error::Login(format!("no input found by \"{k}\"")))?;
                key = dom.attr(el, "name").unwrap_or_default().to_owned();
            }
            put(&mut pairs, key, value);
        }
        let root = dom.element_children(0).next().unwrap_or(0);
        for (k, s) in login.selectorinputs.iter() {
            match self.handle_selector(&mut dom, root, s, vars, !s.optional) {
                Ok(Some(v)) => put(&mut pairs, k.clone(), v),
                Ok(None) => {}
                Err(e) => return Err(Error::Login(format!("reading input {k}: {e}"))),
            }
        }
        let mut get_query: Vec<(String, String)> = vec![];
        for (k, s) in login.getselectorinputs.iter() {
            match self.handle_selector(&mut dom, root, s, vars, !s.optional) {
                Ok(Some(v)) => get_query.push((k.clone(), v)),
                Ok(None) => {}
                Err(e) => return Err(Error::Login(format!("reading input {k}: {e}"))),
            }
        }
        let mut submit = login
            .submitpath
            .clone()
            .or_else(|| dom.attr(form, "action").map(str::to_owned))
            .unwrap_or_default();
        if !get_query.is_empty() {
            submit.push('?');
            submit.push_str(
                &get_query
                    .iter()
                    .map(|(k, v)| {
                        format!(
                            "{}={}",
                            util::url_encode(k, self.encoding()),
                            util::url_encode(v, self.encoding())
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("&"),
            );
        }
        let submit_url = self.resolve(&submit, Some(&login_url))?;

        // A simple image CAPTCHA some sites use is solved by asking the site for its answer.
        if let Some(el) = SelectorList::parse(r#"script[src*="simpleCaptcha"]"#)
            .map_err(Error::Definition)?
            .select_first(&dom, 0)
        {
            let _ = el;
            let url = self.resolve("simpleCaptcha.php?numImages=1", None)?;
            let page = self
                .client
                .fetch(
                    HttpRequest {
                        method: Method::Get,
                        url,
                        headers: headers.clone(),
                        body: None,
                    },
                    false,
                    Some(&login_url),
                )
                .await?;
            let j: Value = serde_json::from_str(&page.text)
                .map_err(|e| Error::Login(format!("the CAPTCHA reply: {e}")))?;
            let hash = j["images"][0]["hash"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            put(&mut pairs, "captchaSelection".into(), hash);
            put(&mut pairs, "submitme".into(), "X".into());
        }
        if let Some(captcha) = &login.captcha {
            match self
                .config
                .get("CAPTCHA")
                .and_then(text_of)
                .filter(|c| !c.trim().is_empty())
            {
                Some(text) => {
                    let mut input = captcha.input.clone().unwrap_or_default();
                    if login.selectors {
                        let el = SelectorList::parse(&input)
                            .map_err(Error::Definition)?
                            .select_first(&dom, 0)
                            .ok_or_else(|| {
                                Error::Login(format!("no CAPTCHA input found by \"{input}\""))
                            })?;
                        input = dom.attr(el, "name").unwrap_or_default().to_owned();
                    }
                    put(&mut pairs, input, text);
                }
                None => {
                    return Err(Error::Unsupported(
                        "a site that asks for a CAPTCHA when signing in".into(),
                    ));
                }
            }
        }

        let multipart = dom.attr(form, "enctype") == Some("multipart/form-data");
        let body = if multipart {
            let boundary = format!("---------------------------{}", Utc::now().timestamp());
            let mut parts: Vec<String> = pairs
                .iter()
                .map(|(k, v)| {
                    format!(
                        "--{boundary}\r\nContent-Disposition: form-data; name=\"{k}\"\r\n\r\n{v}"
                    )
                })
                .collect();
            parts.push(format!("--{boundary}--"));
            (
                format!("multipart/form-data; boundary={boundary}"),
                parts.join("\r\n").into_bytes(),
            )
        } else {
            form_body(&pairs, self.encoding())
        };
        let page = self
            .client
            .fetch(
                HttpRequest {
                    method: Method::Post,
                    url: submit_url,
                    headers,
                    body: Some(body),
                },
                true,
                Some(if multipart { &self.site } else { &login_url }),
            )
            .await?;
        self.check_for_error(&page, &login.error, vars)
    }

    // ---- searching ----------------------------------------------------------------

    async fn execute(&self, r: &Req) -> Result<Page, Error> {
        let mut page = self
            .client
            .fetch(self.http_request(r), r.follow, None)
            .await?;
        if self.def.login.is_some() && self.login_needed(&page) {
            self.login().await?;
            page = self
                .client
                .fetch(self.http_request(r), r.follow, None)
                .await?;
            if self.login_needed(&page) && page.redirect.is_some() {
                return Err(Error::Login(
                    "the site sent the sign-in page again; check the name, password or cookie"
                        .into(),
                ));
            }
        }
        Ok(page)
    }

    pub async fn search(&self, q: &Query) -> Result<Vec<Release>, Error> {
        let requests = self.requests(q)?;
        if self.def.login.is_some() && !*self.logged_in.lock().expect("login state") {
            self.login().await?;
        }
        let mut out: Vec<Release> = vec![];
        let mut seen = HashSet::new();
        let mut last_error = None;
        let total = requests.len();
        let mut failed = 0;
        for r in &requests {
            let page = match self.execute(r).await {
                Ok(p) => p,
                Err(e) => {
                    failed += 1;
                    last_error = Some(e);
                    continue;
                }
            };
            match self.parse(&page, r) {
                Ok(found) => {
                    for rel in found {
                        if seen.insert(rel.guid.clone()) {
                            out.push(rel);
                        }
                    }
                }
                Err(e) => {
                    failed += 1;
                    last_error = Some(e);
                }
            }
        }
        // One failed page among several still gives the others' results.
        if failed == total
            && let Some(e) = last_error
        {
            return Err(e);
        }
        if self
            .def
            .search
            .rows
            .block
            .filters
            .iter()
            .any(|f| f.name == "andmatch")
        {
            out.retain(|r| title_has_all_words(&r.title, &q.q));
        }
        Ok(out)
    }

    /// Checks the site answers and the sign-in (if any) works.
    pub async fn test(&self) -> Result<(), Error> {
        if self.def.login.is_some() {
            self.login().await?;
        }
        let requests = self.requests(&Query::default())?;
        let Some(first) = requests.first() else {
            return Err(Error::Definition("the definition has no search".into()));
        };
        let page = self.execute(first).await?;
        self.parse(&page, first).map(|_| ())
    }

    fn challenge(&self, page: &Page) -> Option<Error> {
        let blocked = (page.status == 403 || page.status == 503)
            && (page.server.contains("cloudflare")
                || page.text.contains("Just a moment")
                || page.text.contains("cf-browser-verification"));
        blocked.then(|| {
            Error::Blocked(
                "the site is protected by Cloudflare's check, which needs a browser to pass".into(),
            )
        })
    }

    // ---- reading pages ----------------------------------------------------------------

    fn parse(&self, page: &Page, r: &Req) -> Result<Vec<Release>, Error> {
        if let Some(e) = self.challenge(page) {
            return Err(e);
        }
        if let Some(to) = &page.redirect {
            if to.to_lowercase().contains("/login.php") {
                return Err(Error::Login(
                    "the site sent the sign-in page; the session may have ended".into(),
                ));
            }
            return Err(Error::Http(format!("redirected to {to}")));
        }
        if page.status != 200 {
            return Err(Error::Http(format!("the site answered {}", page.status)));
        }
        let search = &self.def.search;
        let mut vars = r.vars.clone();
        let kind = r
            .path
            .response
            .as_ref()
            .map(|x| x.kind.as_str())
            .unwrap_or_default();
        let mut releases = vec![];
        match kind {
            "json" => self.parse_json(page, r, &mut vars, &mut releases)?,
            _ => {
                let mut text = page.text.clone();
                if !search.preprocessingfilters.is_empty() {
                    text = self
                        .filter(text, &search.preprocessingfilters, &vars)
                        .map_err(Error::Parse)?;
                }
                let mut dom = if kind == "xml" {
                    Dom::parse_xml(&text)
                        .map_err(|e| Error::Parse(format!("the XML could not be read: {e}")))?
                } else {
                    Dom::parse_html(&text)
                };
                self.parse_dom(&mut dom, page, &mut vars, &mut releases)?;
            }
        }
        for rel in &mut releases {
            if rel.magnet.is_none()
                && let Some(h) = rel.info_hash.as_ref().filter(|h| !h.trim().is_empty())
                && self.def.privacy != "private"
            {
                rel.magnet = magnet_for(h, &rel.title);
            }
            if rel.info_hash.as_deref().is_none_or(str::is_empty)
                && let Some(m) = &rel.magnet
            {
                rel.info_hash = util::query_argument(m, "xt")
                    .map(|x| x.rsplit(':').next().unwrap_or_default().to_owned());
            }
            if rel.guid.is_empty() {
                rel.guid = rel
                    .download_url
                    .clone()
                    .or_else(|| rel.magnet.clone())
                    .or_else(|| rel.details.clone())
                    .unwrap_or_else(|| rel.title.clone());
            }
        }
        releases.retain(|r| !r.title.trim().is_empty());
        Ok(releases)
    }

    fn parse_json(
        &self,
        page: &Page,
        r: &Req,
        vars: &mut Vars,
        out: &mut Vec<Release>,
    ) -> Result<(), Error> {
        let search = &self.def.search;
        if let Some(no) = r
            .path
            .response
            .as_ref()
            .and_then(|x| x.noresultsmessage.as_ref())
        {
            let empty = if no.0.trim().is_empty() {
                page.text.trim().is_empty()
            } else {
                page.text.contains(&no.0)
            };
            if empty {
                return Ok(());
            }
        }
        let root: Value = serde_json::from_str(&page.text)
            .map_err(|e| Error::Parse(format!("the JSON could not be read: {e}")))?;
        if let Some(count) = &search.rows.count
            && let Ok(Some(c)) = self.handle_json_selector(&root, count, vars, true)
            && c.trim().parse::<i64>().is_ok_and(|n| n < 1)
        {
            return Ok(());
        }
        let selector = template::apply(
            search.rows.block.selector.as_deref().unwrap_or_default(),
            vars,
            None,
        )
        .map_err(Error::Definition)?;
        let Some(rows) = json::rows(&root, &selector) else {
            if search.rows.missingattributeequalsnoresults {
                return Ok(());
            }
            return Err(Error::Parse(
                "the rows could not be found in the JSON".into(),
            ));
        };
        for row in rows {
            let mut selected = row;
            if let Some(attr) = &search.rows.block.attribute {
                match json::select_token(row, attr) {
                    Some(v) => selected = v,
                    None if search.rows.missingattributeequalsnoresults => continue,
                    None => return Err(Error::Parse(format!("row attribute '{attr}' is missing"))),
                }
            }
            let each: Vec<&Value> = if search.rows.multiple {
                match selected {
                    Value::Array(a) => a.iter().collect(),
                    Value::Object(m) => m.values().collect(),
                    _ => vec![],
                }
            } else {
                vec![selected]
            };
            for item in each {
                let mut rel = Release::default();
                for (key, sel) in search.fields.iter() {
                    let mut parts = key.split('|');
                    let name = parts.next().unwrap_or_default();
                    let modifiers: Vec<&str> = parts.collect();
                    let var_key = format!(".Result.{name}");
                    let optional = OPTIONAL_FIELDS.contains(&key.as_str())
                        || modifiers.contains(&"optional")
                        || sel.optional;
                    let parent = if sel.selector.as_deref().is_some_and(|s| s.starts_with("..")) {
                        row
                    } else {
                        item
                    };
                    let value = match self.handle_json_selector(parent, sel, vars, !optional) {
                        Ok(v) => v,
                        Err(e) => {
                            if optional {
                                vars.insert(var_key, Val::Null);
                                continue;
                            }
                            return Err(Error::Parse(format!("field {key}: {e}")));
                        }
                    };
                    let value = match self.field_value(value, sel, optional, vars) {
                        Some(v) => v,
                        None => {
                            vars.insert(var_key, Val::Null);
                            continue;
                        }
                    };
                    match self.set_field(&mut rel, name, &modifiers, &value, &page.url) {
                        Ok(v) => {
                            vars.insert(var_key, Val::str(v));
                        }
                        Err(e) if optional => {
                            let _ = e;
                            vars.insert(var_key, Val::Null);
                        }
                        Err(e) => return Err(Error::Parse(format!("field {key}: {e}"))),
                    }
                }
                out.push(rel);
            }
        }
        Ok(())
    }

    /// An optional field that came back empty takes its default, or is left out.
    fn field_value(
        &self,
        value: Option<String>,
        sel: &Selector,
        optional: bool,
        vars: &Vars,
    ) -> Option<String> {
        if optional && value.as_deref().is_none_or(|v| v.trim().is_empty()) {
            let d = sel
                .default
                .as_ref()
                .and_then(|d| template::apply(&d.0, vars, None).ok())
                .unwrap_or_default();
            return (!d.trim().is_empty()).then_some(d);
        }
        value
    }

    fn parse_dom(
        &self,
        dom: &mut Dom,
        page: &Page,
        vars: &mut Vars,
        out: &mut Vec<Release>,
    ) -> Result<(), Error> {
        let search = &self.def.search;
        let selector = template::apply(
            search.rows.block.selector.as_deref().unwrap_or_default(),
            vars,
            None,
        )
        .map_err(Error::Definition)?;
        let mut rows = SelectorList::parse(&selector)
            .map_err(Error::Definition)?
            .select_all(dom, 0);
        merge_rows(dom, &mut rows, &search.rows);
        for row in rows {
            let mut rel = Release::default();
            for (key, sel) in search.fields.iter() {
                let mut parts = key.split('|');
                let name = parts.next().unwrap_or_default();
                let modifiers: Vec<&str> = parts.collect();
                let var_key = format!(".Result.{name}");
                let optional = OPTIONAL_FIELDS.contains(&key.as_str())
                    || modifiers.contains(&"optional")
                    || sel.optional;
                let value = match self.handle_selector(dom, row, sel, vars, !optional) {
                    Ok(v) => v,
                    Err(_) => {
                        if optional || !vars.contains_key(&var_key) {
                            vars.insert(var_key, Val::Null);
                        }
                        continue;
                    }
                };
                let Some(value) = self.field_value(value, sel, optional, vars) else {
                    vars.insert(var_key, Val::Null);
                    continue;
                };
                match self.set_field(&mut rel, name, &modifiers, &value, &page.url) {
                    Ok(v) => {
                        vars.insert(var_key, Val::str(v));
                    }
                    Err(_) => {
                        if optional || !vars.contains_key(&var_key) {
                            vars.insert(var_key, Val::Null);
                        }
                    }
                }
            }
            // The site lists dates in a heading above the rows.
            if rel.publish_date.is_none()
                && let Some(headers) = &search.rows.dateheaders
            {
                let mut value = None;
                let mut prev = dom.prev_element_sibling(row).or_else(|| {
                    dom.parent_element(row)
                        .and_then(|p| dom.prev_element_sibling(p))
                });
                while let Some(p) = prev {
                    if let Ok(Some(v)) = self.handle_selector(dom, p, headers, &Vars::new(), true) {
                        value = Some(v);
                        break;
                    }
                    prev = dom.prev_element_sibling(p).or_else(|| {
                        dom.parent_element(p)
                            .and_then(|x| dom.prev_element_sibling(x))
                    });
                }
                match value {
                    Some(v) => rel.publish_date = util::from_unknown(&v).ok(),
                    None if !headers.optional => continue,
                    None => {}
                }
            }
            out.push(rel);
        }
        Ok(())
    }

    /// Stores one field in the release; returns the value as later fields see it.
    fn set_field(
        &self,
        rel: &mut Release,
        name: &str,
        modifiers: &[&str],
        value: &str,
        search_url: &str,
    ) -> Result<String, String> {
        let number = |v: &str| -> Result<i32, String> { Ok(util::coerce_int(v)) };
        Ok(match name {
            "download" => {
                if value.is_empty() {
                    rel.download_url = None;
                    return Ok(String::new());
                }
                if value.starts_with("magnet:") {
                    rel.magnet = Some(value.to_owned());
                    rel.guid = value.to_owned();
                    value.to_owned()
                } else {
                    let abs = self
                        .resolve(value, Some(search_url))
                        .map_err(|e| e.to_string())?;
                    rel.download_url = Some(abs.clone());
                    rel.guid = abs.clone();
                    abs
                }
            }
            "magnet" => {
                rel.magnet = Some(value.to_owned());
                value.to_owned()
            }
            "infohash" => {
                rel.info_hash = Some(value.to_owned());
                value.to_owned()
            }
            "details" => {
                let abs = self
                    .resolve(value, Some(search_url))
                    .map_err(|e| e.to_string())?;
                rel.details = Some(abs.clone());
                abs
            }
            "comments" => {
                let abs = self
                    .resolve(value, Some(search_url))
                    .map_err(|e| e.to_string())?;
                rel.comments.get_or_insert_with(|| abs.clone());
                abs
            }
            "title" => {
                if modifiers.contains(&"append") {
                    rel.title.push_str(value);
                } else {
                    rel.title = value.to_owned();
                }
                rel.title.clone()
            }
            "description" => {
                if modifiers.contains(&"append") {
                    rel.description
                        .get_or_insert_with(String::new)
                        .push_str(value);
                } else {
                    rel.description = Some(value.to_owned());
                }
                rel.description.clone().unwrap_or_default()
            }
            "category" | "categorydesc" => {
                let found = if name == "category" {
                    self.cats.from_tracker(value)
                } else {
                    self.cats.from_description(value)
                };
                if !found.is_empty() {
                    if modifiers.contains(&"noappend") {
                        rel.categories = found;
                    } else {
                        for c in found {
                            if !rel.categories.contains(&c) {
                                rel.categories.push(c);
                            }
                        }
                    }
                }
                rel.categories
                    .iter()
                    .map(i32::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            }
            "size" => {
                rel.size = util::parse_bytes(value);
                rel.size.to_string()
            }
            "leechers" => {
                let mut n = util::coerce_long(value);
                if n >= 5_000_000 {
                    n = 0;
                }
                rel.peers = Some(rel.peers.unwrap_or(0) + n as i32);
                n.to_string()
            }
            "seeders" => {
                let mut n = number(value)?;
                if n >= 5_000_000 {
                    n = 0;
                }
                rel.seeders = Some(n);
                rel.peers = Some(rel.peers.unwrap_or(0) + n);
                n.to_string()
            }
            "date" => {
                let d = util::from_unknown(value)?;
                rel.publish_date = Some(d);
                util::to_text(d)
            }
            "files" => {
                rel.files = Some(number(value)?);
                value.to_owned()
            }
            "grabs" => {
                rel.grabs = Some(number(value)?);
                value.to_owned()
            }
            "downloadvolumefactor" => {
                rel.download_volume_factor = Some(util::coerce_double(value));
                value.to_owned()
            }
            "uploadvolumefactor" => {
                rel.upload_volume_factor = Some(util::coerce_double(value));
                value.to_owned()
            }
            "minimumratio" => {
                rel.minimum_ratio = Some(util::coerce_double(value));
                value.to_owned()
            }
            "minimumseedtime" => {
                rel.minimum_seed_time = Some(util::coerce_long(value));
                value.to_owned()
            }
            "imdb" | "imdbid" => {
                let n = util::long_from_string(value).unwrap_or(0) as i32;
                rel.imdb_id = Some(n);
                n.to_string()
            }
            "tmdbid" => {
                let n = util::long_from_string(value).unwrap_or(0) as i32;
                rel.tmdb_id = Some(n);
                n.to_string()
            }
            "tvdbid" => {
                let n = util::long_from_string(value).unwrap_or(0) as i32;
                rel.tvdb_id = Some(n);
                n.to_string()
            }
            _ => value.to_owned(),
        })
    }

    // ---- downloading ----------------------------------------------------------------

    fn match_selector(
        &self,
        page: &Page,
        s: &SelectorField,
        vars: &Vars,
    ) -> Result<Option<String>, Error> {
        let sel = template::apply(s.selector.as_deref().unwrap_or_default(), vars, None)
            .map_err(Error::Definition)?;
        let dom = Dom::parse_html(&page.text);
        let Some(el) = SelectorList::parse(&sel)
            .map_err(Error::Definition)?
            .select_first(&dom, 0)
        else {
            return Ok(None);
        };
        let value = match &s.attribute {
            Some(a) => dom
                .attr(el, a)
                .ok_or_else(|| Error::Parse(format!("attribute \"{a}\" is not set")))?
                .to_owned(),
            None => dom.text_content(el),
        };
        self.filter(value, &s.filters, vars)
            .map(Some)
            .map_err(Error::Parse)
    }

    fn download_headers(&self, vars: &Vars) -> Result<Vec<(String, String)>, Error> {
        let source = self
            .def
            .download
            .as_ref()
            .filter(|d| !d.headers.is_empty())
            .map(|d| &d.headers)
            .unwrap_or(&self.def.search.headers);
        self.headers(source, vars)
    }

    async fn get(
        &self,
        url: &str,
        headers: &[(String, String)],
        referer: Option<&str>,
    ) -> Result<Page, Error> {
        self.client
            .fetch(
                HttpRequest {
                    method: Method::Get,
                    url: url.to_owned(),
                    headers: headers.to_vec(),
                    body: None,
                },
                true,
                referer,
            )
            .await
    }

    /// Finds the torrent (or magnet) behind a release's link, signing in first
    /// if the site wants it.
    pub async fn download(&self, link: &str) -> Result<Fetched, Error> {
        if self.def.login.is_some() && !*self.logged_in.lock().expect("login state") {
            self.login().await?;
        }
        let mut vars = self.base_vars();
        if let Ok(u) = url::Url::parse(link) {
            vars.insert(".DownloadUri.AbsoluteUri".into(), Val::str(u.to_string()));
            vars.insert(".DownloadUri.AbsolutePath".into(), Val::str(u.path()));
            vars.insert(".DownloadUri.Scheme".into(), Val::str(u.scheme()));
            vars.insert(
                ".DownloadUri.Host".into(),
                Val::opt(u.host_str().map(str::to_owned)),
            );
            vars.insert(
                ".DownloadUri.Query".into(),
                Val::str(u.query().map(|q| format!("?{q}")).unwrap_or_default()),
            );
            for (k, v) in u.query_pairs() {
                vars.entry(format!(".DownloadUri.Query.{k}"))
                    .or_insert_with(|| Val::str(v.into_owned()));
            }
        }
        let headers = self.download_headers(&vars)?;
        let mut link = link.to_owned();
        let download = self.def.download.clone();
        if let Some(d) = &download {
            let mut response: Option<Page> = None;
            if let Some(before) = &d.before {
                let mut path = before.path.clone();
                if let Some(ps) = &before.pathselector {
                    let first = self.get(&link, &headers, None).await?;
                    path = self.match_selector(&first, ps, &vars)?.unwrap_or_default();
                }
                let url = self.resolve(&self.tmpl(&path, &vars)?, None)?;
                let post = before.method.eq_ignore_ascii_case("post");
                let mut inputs = vec![];
                for (k, v) in before.inputs.iter() {
                    inputs.push((k.clone(), self.tmpl(&v.0, &vars)?));
                }
                let req = if post {
                    HttpRequest {
                        method: Method::Post,
                        url,
                        headers: headers.clone(),
                        body: Some(form_body(&inputs, self.encoding())),
                    }
                } else {
                    let mut url = url;
                    if !inputs.is_empty() {
                        let q = inputs
                            .iter()
                            .map(|(k, v)| {
                                format!(
                                    "{}={}",
                                    util::url_encode(k, self.encoding()),
                                    util::url_encode(v, self.encoding())
                                )
                            })
                            .collect::<Vec<_>>()
                            .join(before.queryseparator.as_deref().unwrap_or("&"));
                        url.push(if url.contains('?') { '&' } else { '?' });
                        url.push_str(&q);
                    }
                    HttpRequest {
                        method: Method::Get,
                        url,
                        headers: headers.clone(),
                        body: None,
                    }
                };
                response = Some(self.client.fetch(req, true, Some(&link)).await?);
            }
            if let Some(h) = &d.infohash {
                let (Some(hash_sel), Some(title_sel)) = (&h.hash, &h.title) else {
                    return Err(Error::Definition(
                        "the info hash block needs a hash and a title".into(),
                    ));
                };
                let page = match (&response, h.usebeforeresponse) {
                    (Some(p), true) => p.clone(),
                    _ => self.get(&link, &headers, None).await?,
                };
                let hash = self
                    .match_selector(&page, hash_sel, &vars)?
                    .ok_or_else(|| Error::Parse("the info hash was not found".into()))?;
                let title = self
                    .match_selector(&page, title_sel, &vars)?
                    .ok_or_else(|| Error::Parse("the title was not found".into()))?;
                return magnet_for(&hash, &title)
                    .map(Fetched::Magnet)
                    .ok_or_else(|| Error::Parse("the info hash is not valid".into()));
            }
            if !d.selectors.is_empty() {
                for s in &d.selectors {
                    let page = match (&response, s.usebeforeresponse) {
                        (Some(p), true) => p.clone(),
                        _ => self.get(&link, &headers, None).await?,
                    };
                    let Some(href) = self.match_selector(&page, s, &vars).ok().flatten() else {
                        continue;
                    };
                    let target = self.resolve(&href, Some(&link))?;
                    if target.starts_with("magnet:") {
                        return Ok(Fetched::Magnet(target));
                    }
                    let got = self.get(&target, &headers, None).await?;
                    if self.def.testlinktorrent && got.bytes.first().is_some_and(|b| *b != b'd') {
                        continue;
                    }
                    return Ok(Fetched::Torrent(got.bytes));
                }
                return Err(Error::Parse(format!(
                    "none of the download selectors matched at {link}"
                )));
            }
            link = self.resolve(&link, None)?;
        }
        let got = self.get(&link, &headers, None).await?;
        if let Some(e) = self.challenge(&got) {
            return Err(e);
        }
        if got.status != 200 {
            return Err(Error::Http(format!("the download answered {}", got.status)));
        }
        if got.bytes.starts_with(b"magnet:") {
            return Ok(Fetched::Magnet(
                String::from_utf8_lossy(&got.bytes).trim().to_owned(),
            ));
        }
        if got.bytes.first() != Some(&b'd') {
            return Err(Error::Parse(
                "the link did not give a torrent file; the sign-in may have ended".into(),
            ));
        }
        Ok(Fetched::Torrent(got.bytes))
    }
}

fn tracing_free_note(_id: &str, _tracker: &str, _name: &str) {}

/// Folds the rows that follow a row into it (a listing that spreads one
/// result over several table rows).
fn merge_rows(dom: &mut Dom, rows: &mut Vec<Id>, spec: &Rows) {
    let after = spec.after;
    if after == 0 {
        return;
    }
    let mut i = 0;
    while i < rows.len() {
        for j in 0..after {
            if let Some(next) = rows.get(i + j + 1).copied() {
                dom.append_children_of(rows[i], next);
            }
        }
        let end = (i + 1 + after).min(rows.len());
        rows.drain(i + 1..end);
        i += 1;
    }
}

fn normalize_words(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Whether every word of the query appears in the title.
fn title_has_all_words(title: &str, query: &str) -> bool {
    let words = normalize_words(query);
    if words.is_empty() {
        return true;
    }
    let have = normalize_words(&util::strip_diacritics(title));
    words.iter().all(|w| have.contains(w))
}

const TRACKERS: [&str; 10] = [
    "http://tracker.opentrackr.org:1337/announce",
    "udp://tracker.auctor.tv:6969/announce",
    "udp://opentracker.i2p.rocks:6969/announce",
    "udp://open.demonii.com:1337/announce",
    "udp://tracker.openbittorrent.com:6969/announce",
    "udp://open.stealth.si:80/announce",
    "udp://tracker.torrent.eu.org:451/announce",
    "udp://explodie.org:6969/announce",
    "udp://exodus.desync.com:6969/announce",
    "udp://tracker.tiny-vps.com:6969/announce",
];

/// A magnet link from a bare info hash (hex, or base32 as some sites write it).
pub fn magnet_for(hash: &str, title: &str) -> Option<String> {
    let hash = hash.trim();
    let hex = if hash.len() == 40 && hash.chars().all(|c| c.is_ascii_hexdigit()) {
        hash.to_ascii_lowercase()
    } else if hash.len() == 32 {
        base32_to_hex(hash)?
    } else {
        return None;
    };
    let mut m = format!(
        "magnet:?xt=urn:btih:{hex}&dn={}",
        util::url_encode(title, encoding_rs::UTF_8)
    );
    for t in TRACKERS {
        m.push_str("&tr=");
        m.push_str(
            &util::url_encode(t, encoding_rs::UTF_8)
                .replace(':', "%3A")
                .replace('/', "%2F"),
        );
    }
    Some(m)
}

fn base32_to_hex(s: &str) -> Option<String> {
    let mut bits = 0u32;
    let mut acc = 0u32;
    let mut bytes = vec![];
    for c in s.to_ascii_uppercase().chars() {
        let v = match c {
            'A'..='Z' => c as u32 - 'A' as u32,
            '2'..='7' => c as u32 - '2' as u32 + 26,
            _ => return None,
        };
        acc = (acc << 5) | v;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            bytes.push(((acc >> bits) & 0xFF) as u8);
        }
    }
    Some(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// The form fields to ask for: those in the definition, or a name and password.
pub fn effective_settings(def: &Definition) -> Vec<crate::def::SettingField> {
    use crate::def::SettingField;
    let mut fields = def.settings.clone().unwrap_or_else(|| {
        vec![
            SettingField {
                name: "username".into(),
                kind: "text".into(),
                label: Some("Username".into()),
                ..Default::default()
            },
            SettingField {
                name: "password".into(),
                kind: "password".into(),
                label: Some("Password".into()),
                ..Default::default()
            },
        ]
    });
    if def.login.as_ref().is_some_and(|l| l.captcha.is_some())
        && !fields.iter().any(|f| f.name == "CAPTCHA")
    {
        fields.push(SettingField {
            name: "CAPTCHA".into(),
            kind: "text".into(),
            label: Some("CAPTCHA".into()),
            ..Default::default()
        });
    }
    fields
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magnets_from_hashes() {
        let hex = "0123456789abcdef0123456789abcdef01234567";
        let m = magnet_for(hex, "A Movie (2020)").unwrap();
        assert!(m.starts_with(&format!("magnet:?xt=urn:btih:{hex}&dn=A+Movie+(2020)&tr=")));
        // base32 of the same 20 bytes
        assert_eq!(
            base32_to_hex("AEJWK2VZ6ZUWZOQXMMPO2T3FZ4EY47IX").map(|h| h.len()),
            Some(40)
        );
        assert!(magnet_for("short", "x").is_none());
    }

    #[test]
    fn the_title_must_hold_all_the_query_words() {
        assert!(title_has_all_words("The.Matrix.1999.1080p", "the matrix"));
        assert!(!title_has_all_words(
            "The.Matrix.Reloaded",
            "the matrix revolutions"
        ));
        assert!(title_has_all_words("Amélie 2001", "amelie"));
        assert!(title_has_all_words("anything", ""));
    }
}
