//! The YAML a definition is written in (Cardigann's format, as Prowlarr reads it).

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Deserializer, de};

/// A scalar the file may write as a string, number or boolean.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Scalar(pub String);

impl<'de> Deserialize<'de> for Scalar {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> de::Visitor<'de> for V {
            type Value = Scalar;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a string, number or boolean")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Scalar, E> {
                Ok(Scalar(v.to_owned()))
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<Scalar, E> {
                Ok(Scalar(v))
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Scalar, E> {
                Ok(Scalar(if v { "True" } else { "False" }.to_owned()))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Scalar, E> {
                Ok(Scalar(v.to_string()))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Scalar, E> {
                Ok(Scalar(v.to_string()))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Scalar, E> {
                Ok(Scalar(v.to_string()))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Scalar, E> {
                Ok(Scalar(String::new()))
            }
            fn visit_none<E: de::Error>(self) -> Result<Scalar, E> {
                Ok(Scalar(String::new()))
            }
        }
        d.deserialize_any(V)
    }
}

/// A map that keeps its order and tolerates the same key twice, as the
/// definitions rely on.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Pairs<V>(pub Vec<(String, V)>);

impl<'de, V: Deserialize<'de>> Deserialize<'de> for Pairs<V> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Vis<V>(std::marker::PhantomData<V>);
        impl<'de, V: Deserialize<'de>> de::Visitor<'de> for Vis<V> {
            type Value = Pairs<V>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a map")
            }
            fn visit_map<A: de::MapAccess<'de>>(self, mut m: A) -> Result<Pairs<V>, A::Error> {
                let mut out = vec![];
                while let Some((k, v)) = m.next_entry::<Scalar, V>()? {
                    out.push((k.0, v));
                }
                Ok(Pairs(out))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Pairs<V>, E> {
                Ok(Pairs(vec![]))
            }
        }
        d.deserialize_any(Vis(std::marker::PhantomData))
    }
}

impl<V> Pairs<V> {
    pub fn iter(&self) -> impl Iterator<Item = &(String, V)> {
        self.0.iter()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn get(&self, key: &str) -> Option<&V> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
}

/// `args:` is a string, a number, or a list of them.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum Args {
    #[default]
    None,
    One(String),
    Many(Vec<String>),
}

impl<'de> Deserialize<'de> for Args {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Many(Vec<Scalar>),
            One(Scalar),
        }
        Ok(match Option::<Raw>::deserialize(d)? {
            None => Args::None,
            Some(Raw::One(s)) => Args::One(s.0),
            Some(Raw::Many(v)) => Args::Many(v.into_iter().map(|s| s.0).collect()),
        })
    }
}

impl Args {
    pub fn first(&self) -> Option<&str> {
        match self {
            Args::None => None,
            Args::One(s) => Some(s),
            Args::Many(v) => v.first().map(String::as_str),
        }
    }
    pub fn nth(&self, i: usize) -> Option<&str> {
        match self {
            Args::Many(v) => v.get(i).map(String::as_str),
            Args::One(s) if i == 0 => Some(s),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Filter {
    pub name: String,
    #[serde(default)]
    pub args: Args,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct Selector {
    pub selector: Option<String>,
    #[serde(default)]
    pub optional: bool,
    pub default: Option<Scalar>,
    pub text: Option<Scalar>,
    pub attribute: Option<String>,
    pub remove: Option<String>,
    #[serde(default)]
    pub filters: Vec<Filter>,
    #[serde(default)]
    pub case: Pairs<Scalar>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct Rows {
    #[serde(flatten)]
    pub block: Selector,
    #[serde(default)]
    pub after: usize,
    pub dateheaders: Option<Selector>,
    pub count: Option<Selector>,
    #[serde(default)]
    pub multiple: bool,
    #[serde(default, alias = "missingAttributeEqualsNoResults")]
    pub missingattributeequalsnoresults: bool,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct SettingField {
    pub name: String,
    #[serde(default, rename = "type")]
    pub kind: String,
    pub label: Option<String>,
    pub default: Option<Scalar>,
    #[serde(default)]
    pub options: Pairs<Scalar>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct CategoryMapping {
    pub id: Scalar,
    pub cat: Option<String>,
    pub desc: Option<String>,
    #[serde(default)]
    pub default: bool,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct Caps {
    #[serde(default)]
    pub categories: Pairs<Scalar>,
    #[serde(default)]
    pub categorymappings: Vec<CategoryMapping>,
    #[serde(default)]
    pub modes: Pairs<Vec<Scalar>>,
    #[serde(default)]
    pub allowrawsearch: bool,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct Captcha {
    #[serde(default, rename = "type")]
    pub kind: String,
    pub selector: Option<String>,
    pub input: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct ErrorBlock {
    pub path: Option<String>,
    pub selector: Option<String>,
    pub message: Option<Selector>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct PageTest {
    pub path: Option<String>,
    pub selector: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct Login {
    pub path: Option<String>,
    pub submitpath: Option<String>,
    #[serde(default)]
    pub cookies: Vec<Scalar>,
    #[serde(default)]
    pub method: String,
    pub form: Option<String>,
    #[serde(default)]
    pub selectors: bool,
    #[serde(default)]
    pub inputs: Pairs<Scalar>,
    #[serde(default)]
    pub selectorinputs: Pairs<Selector>,
    #[serde(default)]
    pub getselectorinputs: Pairs<Selector>,
    #[serde(default)]
    pub error: Vec<ErrorBlock>,
    pub test: Option<PageTest>,
    pub captcha: Option<Captcha>,
    #[serde(default)]
    pub headers: Pairs<Vec<Scalar>>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct Response {
    #[serde(default, rename = "type")]
    pub kind: String,
    #[serde(default, alias = "noResultsMessage")]
    pub noresultsmessage: Option<Scalar>,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct SearchPath {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub method: String,
    #[serde(default)]
    pub inputs: Pairs<Scalar>,
    pub queryseparator: Option<String>,
    #[serde(default)]
    pub categories: Vec<Scalar>,
    #[serde(default = "yes")]
    pub inheritinputs: bool,
    #[serde(default)]
    pub followredirect: bool,
    pub response: Option<Response>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct Search {
    pub path: Option<String>,
    #[serde(default)]
    pub paths: Vec<SearchPath>,
    #[serde(default)]
    pub headers: Pairs<Vec<Scalar>>,
    #[serde(default)]
    pub keywordsfilters: Vec<Filter>,
    #[serde(default, alias = "allowEmptyInputs")]
    pub allowemptyinputs: bool,
    #[serde(default)]
    pub inputs: Pairs<Scalar>,
    #[serde(default)]
    pub error: Vec<ErrorBlock>,
    #[serde(default)]
    pub preprocessingfilters: Vec<Filter>,
    #[serde(default)]
    pub rows: Rows,
    #[serde(default)]
    pub fields: Pairs<Selector>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct SelectorField {
    pub selector: Option<String>,
    pub attribute: Option<String>,
    #[serde(default)]
    pub usebeforeresponse: bool,
    #[serde(default)]
    pub filters: Vec<Filter>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct Before {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub method: String,
    #[serde(default)]
    pub inputs: Pairs<Scalar>,
    pub queryseparator: Option<String>,
    pub pathselector: Option<SelectorField>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct InfoHash {
    pub hash: Option<SelectorField>,
    pub title: Option<SelectorField>,
    #[serde(default)]
    pub usebeforeresponse: bool,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct Download {
    #[serde(default)]
    pub selectors: Vec<SelectorField>,
    #[serde(default)]
    pub method: String,
    pub before: Option<Before>,
    pub infohash: Option<InfoHash>,
    #[serde(default)]
    pub headers: Pairs<Vec<Scalar>>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Definition {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub language: String,
    /// "public", "semi-private" or "private".
    #[serde(default, rename = "type")]
    pub privacy: String,
    #[serde(default)]
    pub encoding: Option<String>,
    #[serde(default, alias = "requestDelay")]
    pub requestdelay: Option<f64>,
    #[serde(default)]
    pub links: Vec<String>,
    #[serde(default)]
    pub legacylinks: Vec<String>,
    #[serde(default)]
    pub followredirect: bool,
    #[serde(default = "yes", alias = "testLinkTorrent")]
    pub testlinktorrent: bool,
    #[serde(default)]
    pub caps: Caps,
    #[serde(default)]
    pub settings: Option<Vec<SettingField>>,
    pub login: Option<Login>,
    #[serde(default)]
    pub search: Search,
    pub download: Option<Download>,
    #[serde(skip)]
    pub unused: BTreeMap<String, ()>,
}

impl Definition {
    pub fn from_yaml(text: &str) -> Result<Self, String> {
        let text = text.trim_start_matches('\u{feff}');
        // One definition starts "--- id: x" on a single line.
        let fixed;
        let text = match text.strip_prefix("--- ") {
            Some(rest) => {
                fixed = format!("---\n{rest}");
                &fixed
            }
            None => text,
        };
        serde_yaml_ng::from_str(text).map_err(|e| e.to_string())
    }
}
