//! `?t=caps`: what an indexer says it can do.

use crate::{IndexerError, parse::clean_xml};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Category {
    pub id: i32,
    pub name: String,
    pub description: String,
    pub subcategories: Vec<Category>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capabilities {
    pub default_page_size: i32,
    pub max_page_size: i32,
    /// `None` when text search is not available.
    pub search_params: Option<Vec<String>>,
    /// `None` when movie search is not available.
    pub movie_params: Option<Vec<String>>,
    /// True when movie search takes several ids at once (`tmdbid` + `imdbid`).
    pub supports_aggregate_id_search: bool,
    pub text_search_engine: String,
    pub movie_text_search_engine: String,
    pub categories: Vec<Category>,
}

impl Default for Capabilities {
    /// What Radarr assumes when an indexer's caps cannot be read.
    fn default() -> Self {
        Self {
            default_page_size: 100,
            max_page_size: 100,
            search_params: Some(vec!["q".into()]),
            movie_params: Some(
                ["q", "imdbid", "imdbtitle", "imdbyear"]
                    .map(String::from)
                    .to_vec(),
            ),
            supports_aggregate_id_search: false,
            // "sphinx" for older Newznab installs.
            text_search_engine: "sphinx".into(),
            movie_text_search_engine: "sphinx".into(),
            categories: Vec::new(),
        }
    }
}

fn child<'a, 'i>(parent: roxmltree::Node<'a, 'i>, name: &str) -> Option<roxmltree::Node<'a, 'i>> {
    parent
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == name)
}

impl Capabilities {
    pub fn supports_text_search(&self) -> bool {
        self.search_params
            .as_ref()
            .is_some_and(|p| p.iter().any(|x| x == "q"))
    }

    pub fn supports_imdb_search(&self) -> bool {
        self.movie_params
            .as_ref()
            .is_some_and(|p| p.iter().any(|x| x == "imdbid"))
    }

    pub fn supports_tmdb_search(&self) -> bool {
        self.movie_params
            .as_ref()
            .is_some_and(|p| p.iter().any(|x| x == "tmdbid"))
    }

    /// Parses a caps response. Error elements (bad key, ...) are returned as errors.
    pub fn parse(xml: &str) -> Result<Self, IndexerError> {
        let cleaned = clean_xml(xml);
        let doc = roxmltree::Document::parse(&cleaned)
            .map_err(|e| IndexerError::Parse(format!("caps: {e}")))?;
        crate::parse::check_error(&doc, "")?;

        let root = doc.root_element();
        if root.tag_name().name() != "caps" {
            return Err(IndexerError::Parse("caps: unexpected XML".into()));
        }
        let mut caps = Capabilities::default();
        let int = |n: roxmltree::Node<'_, '_>, attr: &str| {
            n.attribute(attr).and_then(|v| v.parse::<i32>().ok())
        };

        if let Some(limits) = child(root, "limits") {
            caps.default_page_size = int(limits, "default").unwrap_or(caps.default_page_size);
            caps.max_page_size = int(limits, "max").unwrap_or(caps.max_page_size);
        }

        if let Some(searching) = child(root, "searching") {
            match child(searching, "search").filter(|s| s.attribute("available") == Some("yes")) {
                None => caps.search_params = None,
                Some(search) => {
                    if let Some(params) = search.attribute("supportedParams") {
                        caps.search_params = Some(params.split(',').map(str::to_owned).collect());
                    }
                    if let Some(engine) = search.attribute("searchEngine") {
                        caps.text_search_engine = engine.to_owned();
                    }
                }
            }
            match child(searching, "movie-search")
                .filter(|s| s.attribute("available") == Some("yes"))
            {
                None => caps.movie_params = None,
                Some(movie) => {
                    if let Some(params) = movie.attribute("supportedParams") {
                        caps.movie_params = Some(params.split(',').map(str::to_owned).collect());
                        caps.supports_aggregate_id_search = true;
                    }
                    if let Some(engine) = movie.attribute("searchEngine") {
                        caps.movie_text_search_engine = engine.to_owned();
                    }
                }
            }
        }

        if let Some(categories) = child(root, "categories") {
            for cat in categories
                .children()
                .filter(|n| n.is_element() && n.tag_name().name() == "category")
            {
                let Some(id) = int(cat, "id") else { continue };
                let subcategories = cat
                    .children()
                    .filter(|n| n.is_element() && n.tag_name().name() == "subcat")
                    .filter_map(|sub| {
                        Some(Category {
                            id: int(sub, "id")?,
                            name: sub.attribute("name")?.to_owned(),
                            description: sub
                                .attribute("description")
                                .unwrap_or_default()
                                .to_owned(),
                            subcategories: vec![],
                        })
                    })
                    .collect();
                caps.categories.push(Category {
                    id,
                    name: cat.attribute("name").unwrap_or_default().to_owned(),
                    description: cat.attribute("description").unwrap_or_default().to_owned(),
                    subcategories,
                });
            }
        }
        Ok(caps)
    }
}
