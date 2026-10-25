//! The definitions that come with the app, plus any the owner adds, and the
//! short descriptions the Settings page lists them by.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, LazyLock};

use rust_embed::RustEmbed;
use serde::Serialize;

use crate::cats;
use crate::def::{Definition, SettingField};
use crate::engine::effective_settings;

#[derive(RustEmbed)]
#[folder = "definitions/"]
#[include = "*.yml"]
struct Bundled;

/// All the definitions, by id.
#[derive(Clone, Default)]
pub struct Library {
    defs: BTreeMap<String, Arc<Definition>>,
    /// Files that could not be read, with why.
    pub problems: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Field {
    pub name: String,
    /// "text", "password", "checkbox", "select" or "info".
    pub kind: String,
    pub label: String,
    pub default: Option<String>,
    /// For a choice: value and label.
    pub options: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Summary {
    pub id: String,
    pub name: String,
    pub description: String,
    pub language: String,
    /// "public", "semi-private" or "private".
    pub privacy: String,
    pub links: Vec<String>,
    /// Kinds of media it lists: "Movies", "TV", "Anime", "Audio", "Books", ...
    pub categories: Vec<String>,
    pub movies: bool,
    pub tv: bool,
    /// The site wants an account (or a cookie) before it lists anything.
    pub needs_account: bool,
    /// The site is behind Cloudflare's check and may need a browser to pass it.
    pub cloudflare: bool,
    pub fields: Vec<Field>,
}

static BUNDLED: LazyLock<Library> = LazyLock::new(|| {
    let mut lib = Library::default();
    for name in Bundled::iter() {
        let Some(file) = Bundled::get(&name) else {
            continue;
        };
        lib.add(&name, &String::from_utf8_lossy(&file.data));
    }
    lib
});

/// What a link with these words in it is called.
fn group_name(id: i32) -> &'static str {
    match id {
        1000..=1999 => "Console",
        2000..=2999 => "Movies",
        3000..=3999 => "Audio",
        4000..=4999 => "PC",
        5070 => "Anime",
        5000..=5999 => "TV",
        6000..=6999 => "XXX",
        7000..=7999 => "Books",
        _ => "Other",
    }
}

impl Library {
    fn add(&mut self, file: &str, text: &str) {
        match Definition::from_yaml(text) {
            Ok(d) => {
                self.defs.insert(d.id.clone(), Arc::new(d));
            }
            Err(e) => self.problems.push(format!("{file}: {e}")),
        }
    }

    /// The names of the bundled definition files, without ".yml": what a plugin
    /// can be told to read, without reading them all.
    pub fn bundled_names() -> Vec<String> {
        let mut v: Vec<String> = Bundled::iter()
            .filter_map(|n| n.strip_suffix(".yml").map(str::to_owned))
            .collect();
        v.sort();
        v
    }

    /// One bundled definition, by file name or by id, reading only what it must.
    pub fn bundled_one(key: &str) -> Option<Arc<Definition>> {
        if let Some(file) = Bundled::get(&format!("{key}.yml"))
            && let Ok(d) = Definition::from_yaml(&String::from_utf8_lossy(&file.data))
        {
            return Some(Arc::new(d));
        }
        // A definition whose id differs from its file name.
        Bundled::iter().find_map(|name| {
            let file = Bundled::get(&name)?;
            let text = String::from_utf8_lossy(&file.data);
            text.lines()
                .any(|l| l.trim_start_matches('\u{feff}').trim() == format!("id: {key}"))
                .then(|| Definition::from_yaml(&text).ok().map(Arc::new))
                .flatten()
        })
    }

    /// The definitions inside the app.
    pub fn bundled() -> &'static Library {
        &BUNDLED
    }

    /// The bundled definitions, with those in `dir` added or replacing them.
    pub fn with_dir(dir: Option<&Path>) -> Library {
        let mut lib = BUNDLED.clone();
        if let Some(dir) = dir
            && let Ok(entries) = std::fs::read_dir(dir)
        {
            for e in entries.flatten() {
                let p = e.path();
                if p.extension().is_some_and(|x| x == "yml" || x == "yaml") {
                    match std::fs::read_to_string(&p) {
                        Ok(text) => lib.add(&p.display().to_string(), &text),
                        Err(err) => lib.problems.push(format!("{}: {err}", p.display())),
                    }
                }
            }
        }
        lib
    }

    pub fn get(&self, id: &str) -> Option<Arc<Definition>> {
        self.defs.get(id).cloned()
    }

    pub fn len(&self) -> usize {
        self.defs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.defs.is_empty()
    }

    pub fn summaries(&self) -> Vec<Summary> {
        let mut v: Vec<Summary> = self.defs.values().map(|d| summarize(d)).collect();
        v.sort_by_key(|s| s.name.to_lowercase());
        v
    }
}

fn field(f: &SettingField) -> Field {
    let kind = match f.kind.as_str() {
        "text" | "password" | "checkbox" | "select" => f.kind.clone(),
        _ => "info".to_owned(),
    };
    Field {
        name: f.name.clone(),
        label: f.label.clone().unwrap_or_else(|| f.name.clone()),
        default: f.default.as_ref().map(|d| d.0.clone()),
        options: f
            .options
            .iter()
            .map(|(k, v)| (k.clone(), v.0.clone()))
            .collect(),
        kind,
    }
}

pub fn summarize(d: &Definition) -> Summary {
    let mut ids: Vec<i32> = d
        .caps
        .categorymappings
        .iter()
        .filter_map(|m| m.cat.as_deref().and_then(cats::by_name))
        .collect();
    ids.extend(
        d.caps
            .categories
            .iter()
            .filter_map(|(_, n)| cats::by_name(&n.0)),
    );
    let mut groups: Vec<&str> = vec![];
    for id in &ids {
        let g = group_name(*id);
        if !groups.contains(&g) {
            groups.push(g);
        }
    }
    let order = [
        "Movies", "TV", "Anime", "Audio", "Books", "PC", "Console", "XXX", "Other",
    ];
    groups.sort_by_key(|g| order.iter().position(|o| o == g).unwrap_or(order.len()));
    let fields = effective_settings(d);
    Summary {
        id: d.id.clone(),
        name: d.name.clone(),
        description: d.description.clone(),
        language: d.language.clone(),
        privacy: d.privacy.clone(),
        links: d.links.clone(),
        categories: groups.iter().map(|g| (*g).to_owned()).collect(),
        movies: ids.iter().any(|i| (2000..3000).contains(i)),
        tv: ids.iter().any(|i| (5000..6000).contains(i)),
        needs_account: d.login.is_some(),
        cloudflare: fields.iter().any(|f| f.kind == "info_flaresolverr"),
        fields: fields.iter().map(field).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bundled_library_lists_its_sites() {
        let lib = Library::bundled();
        assert!(lib.problems.is_empty(), "{:?}", lib.problems);
        assert!(lib.len() > 500);
        let tpb = lib
            .summaries()
            .into_iter()
            .find(|s| s.id == "thepiratebay")
            .unwrap();
        assert_eq!(tpb.privacy, "public");
        assert!(tpb.movies && tpb.tv);
        assert!(!tpb.needs_account);
        let private = lib
            .summaries()
            .into_iter()
            .find(|s| s.privacy == "private")
            .unwrap();
        assert!(private.needs_account);
        assert!(
            lib.summaries().iter().any(|s| s.cloudflare),
            "some sites ask for FlareSolverr"
        );
    }

    #[test]
    fn one_definition_is_read_without_reading_the_rest() {
        assert!(Library::bundled_names().len() > 500);
        assert_eq!(
            Library::bundled_one("thepiratebay").unwrap().id,
            "thepiratebay"
        );
        // The file is bluebird.yml; the id inside is bluebirdhd.
        assert_eq!(Library::bundled_one("bluebird").unwrap().id, "bluebirdhd");
        assert_eq!(Library::bundled_one("bluebirdhd").unwrap().id, "bluebirdhd");
        assert!(Library::bundled_one("nosuchsite").is_none());
    }

    #[test]
    fn a_folder_of_definitions_adds_and_replaces() {
        let dir = std::env::temp_dir().join(format!("omp-defs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("mine.yml"),
            "id: mine\nname: Mine\nlanguage: en-US\ntype: public\nlinks: [\"https://mine.test/\"]\ncaps: {categorymappings: [{id: 1, cat: Movies}], modes: {search: [q]}}\nsearch: {paths: [{path: /}], rows: {selector: tr}, fields: {title: {selector: td}}}\n",
        )
        .unwrap();
        std::fs::write(dir.join("broken.yml"), "id: [").unwrap();
        let lib = Library::with_dir(Some(&dir));
        assert!(lib.get("mine").is_some());
        assert!(lib.get("thepiratebay").is_some());
        assert_eq!(lib.problems.len(), 1);
    }
}
