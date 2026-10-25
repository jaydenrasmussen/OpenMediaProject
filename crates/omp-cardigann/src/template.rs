//! The small template language definitions use. It is not Go's `text/template`:
//! Prowlarr reads it with a fixed order of regular expressions, and the
//! definitions are written for that, so this does the same steps in the same
//! order (re_replace, join, and/or/eq/ne, if-else, range, variables).

use std::collections::HashMap;
use std::sync::LazyLock;

use fancy_regex::Regex;

/// What a variable holds.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Val {
    #[default]
    Null,
    Str(String),
    List(Vec<String>),
}

impl Val {
    pub fn str(s: impl Into<String>) -> Self {
        Val::Str(s.into())
    }

    pub fn opt(s: Option<String>) -> Self {
        s.map_or(Val::Null, Val::Str)
    }

    fn as_str(&self) -> Option<&str> {
        match self {
            Val::Str(s) => Some(s),
            _ => None,
        }
    }

    fn truthy(&self) -> bool {
        match self {
            Val::Null => false,
            Val::Str(s) => !s.trim().is_empty(),
            Val::List(l) => !l.is_empty(),
        }
    }
}

pub type Vars = HashMap<String, Val>;

pub type Modifier<'a> = &'a dyn Fn(&str) -> String;

macro_rules! re {
    ($name:ident, $pat:expr) => {
        static $name: LazyLock<Regex> =
            LazyLock::new(|| Regex::new($pat).expect("a fixed pattern"));
    };
}

re!(
    RE_REPLACE,
    r#"\{\{\s*re_replace\s+(\..+?)\s+"(.*?)"\s+"(.*?)"\s*\}\}"#
);
re!(JOIN, r#"\{\{\s*join\s+(\..+?)\s+"(.*?)"\s*\}\}"#);
re!(
    LOGIC,
    r#"\b(and|or|eq|ne)(?:\s+(?:\(?\.[^\)\s]+\)?|"[^"]+")){2,}"#
);
re!(LOGIC_TOKEN, r#"\(?\.[^\)\s]+\)?|"[^"]+""#);
re!(
    IF_ELSE,
    r#"\{\{\s*if\s*(.+?)\s*\}\}(.*?)\{\{\s*else\s*\}\}(.*?)\{\{\s*end\s*\}\}"#
);
re!(
    RANGE,
    r#"\{\{\s*range\s*(?:(?:(?P<index>\$.+?),)(?:\s*(?P<element>.+?)\s*(?::=)\s*))?(?P<variable>.+?)\s*\}\}(?P<prefix>.*?)\{\{\.\}\}(?P<postfix>.*?)\{\{end\}\}"#
);
re!(VARIABLE, r#"\{\{\s*(\..+?)\s*\}\}"#);

fn lookup<'a>(vars: &'a Vars, name: &str) -> &'a Val {
    vars.get(name).unwrap_or(&Val::Null)
}

/// Replaces each distinct match of `re` in `template` using `f`, like the
/// original's `template.Replace(all, expanded)` for every match.
fn replace_matches(
    template: &mut String,
    re: &Regex,
    mut f: impl FnMut(&fancy_regex::Captures<'_>) -> Result<String, String>,
) -> Result<(), String> {
    let source = template.clone();
    let mut pairs = vec![];
    for caps in re.captures_iter(&source) {
        let caps = caps.map_err(|e| e.to_string())?;
        let all = caps
            .get(0)
            .map(|m| m.as_str().to_owned())
            .unwrap_or_default();
        pairs.push((all, f(&caps)?));
    }
    for (all, with) in pairs {
        *template = template.replace(&all, &with);
    }
    Ok(())
}

/// Expands a template. `modifier` is applied to each value put into the text
/// (URL-encoding, for search paths).
pub fn apply(
    template: &str,
    vars: &Vars,
    modifier: Option<Modifier<'_>>,
) -> Result<String, String> {
    if template.trim().is_empty() || !template.contains("{{") {
        return Ok(template.to_owned());
    }
    let modify = |s: String| match modifier {
        Some(m) => m(&s),
        None => s,
    };
    let mut t = template.to_owned();

    replace_matches(&mut t, &RE_REPLACE, |c| {
        let input = lookup(vars, &c[1]).as_str().unwrap_or_default().to_owned();
        let re = crate::util::compile(&c[2])?;
        Ok(modify(crate::util::net_replace(&re, &input, &c[3])))
    })?;

    replace_matches(&mut t, &JOIN, |c| {
        let joined = match lookup(vars, &c[1]) {
            Val::List(l) => l.join(&c[2]),
            Val::Str(s) => s.clone(),
            Val::Null => String::new(),
        };
        Ok(modify(joined))
    })?;

    // and / or / eq / ne, innermost last, re-matching after each so they nest.
    while let Some(m) = LOGIC.find(&t).map_err(|e| e.to_string())? {
        let (start, end) = (m.start(), m.end());
        let whole = m.as_str().to_owned();
        let name_len = whole.find(char::is_whitespace).unwrap_or(whole.len());
        let name = &whole[..name_len];
        let rest = &whole[name_len..];
        let tokens: Vec<(usize, &str)> = LOGIC_TOKEN
            .find_iter(rest)
            .filter_map(Result::ok)
            .map(|m| (m.start(), m.as_str()))
            .collect();
        let mut params: Vec<String> = tokens
            .iter()
            .map(|(_, p)| p.trim_matches(['(', ')']).to_owned())
            .collect();
        let mut length = end - start;
        let result = match name {
            "and" | "or" => {
                // Only variables take part; string literals are dropped.
                params.retain(|p| !p.starts_with('"'));
                let want_blank = name == "and";
                let mut picked = String::new();
                for p in &params {
                    picked = p.clone();
                    if !lookup(vars, p).truthy() == want_blank {
                        break;
                    }
                }
                picked
            }
            _ => {
                if tokens.len() > 2 {
                    length = name_len + tokens[2].0;
                }
                let value = |p: &String| -> Option<String> {
                    if p.starts_with('"') {
                        Some(p.trim_matches('"').to_owned())
                    } else {
                        lookup(vars, p).as_str().map(str::to_owned)
                    }
                };
                let equal = params.len() >= 2 && value(&params[0]) == value(&params[1]);
                if equal == (name == "eq") {
                    ".True"
                } else {
                    ".False"
                }
                .to_owned()
            }
        };
        t.replace_range(start..start + length, &result);
    }

    replace_matches(&mut t, &IF_ELSE, |c| {
        let condition = &c[1];
        if !condition.starts_with('.') {
            return Err(format!("the condition '{condition}' is not supported"));
        }
        Ok(if lookup(vars, condition).truthy() {
            c[2].to_owned()
        } else {
            c[3].to_owned()
        })
    })?;

    replace_matches(&mut t, &RANGE, |c| {
        let index = c.name("index").map_or("", |m| m.as_str());
        let prefix = &c["prefix"];
        let postfix = &c["postfix"];
        let items = match lookup(vars, &c["variable"]) {
            Val::List(l) => l.clone(),
            Val::Str(s) => vec![s.clone()],
            Val::Null => vec![],
        };
        let marker = format!("{{{{{index}}}}}");
        let mut out = String::new();
        for (i, item) in items.into_iter().enumerate() {
            let item = modify(item);
            if index.is_empty() {
                out.push_str(prefix);
                out.push_str(&item);
                out.push_str(postfix);
            } else {
                out.push_str(&prefix.replace(&marker, &i.to_string()));
                out.push_str(&item);
                out.push_str(&postfix.replace(&marker, &i.to_string()));
            }
        }
        Ok(out)
    })?;

    replace_matches(&mut t, &VARIABLE, |c| {
        let value = match lookup(vars, &c[1]) {
            Val::Str(s) => s.clone(),
            Val::List(l) => l.join(","),
            Val::Null => String::new(),
        };
        Ok(modify(value))
    })?;
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars() -> Vars {
        let mut v = Vars::new();
        v.insert(".Config.user".into(), Val::str("sam"));
        v.insert(".Config.empty".into(), Val::str("  "));
        v.insert(".Config.mode".into(), Val::str("hd"));
        v.insert(".Query.Keywords".into(), Val::str("The Matrix 1999"));
        v.insert(
            ".Categories".into(),
            Val::List(vec!["1".into(), "2".into(), "3".into()]),
        );
        v.insert(".True".into(), Val::str("True"));
        v
    }

    fn run(t: &str) -> String {
        apply(t, &vars(), None).unwrap()
    }

    #[test]
    fn plain_text_and_variables() {
        assert_eq!(run("no braces"), "no braces");
        assert_eq!(
            run("u={{ .Config.user }}&k={{.Query.Keywords}}"),
            "u=sam&k=The Matrix 1999"
        );
        assert_eq!(run("[{{ .Config.missing }}]"), "[]");
    }

    #[test]
    fn if_else_on_whether_a_value_is_set() {
        assert_eq!(run("{{ if .Config.user }}yes{{ else }}no{{ end }}"), "yes");
        assert_eq!(run("{{ if .Config.empty }}yes{{ else }}no{{ end }}"), "no");
        assert_eq!(
            run("{{ if .Config.nothing }}yes{{ else }}no{{ end }}"),
            "no"
        );
        assert_eq!(
            run("a{{if .Categories}}[{{ join .Categories \",\" }}]{{else}}none{{end}}b"),
            "a[1,2,3]b"
        );
    }

    #[test]
    fn and_or_eq_ne() {
        assert_eq!(
            run("{{ if or .Config.nothing .Config.user }}y{{ else }}n{{ end }}"),
            "y"
        );
        assert_eq!(
            run("{{ if and .Config.user .Config.nothing }}y{{ else }}n{{ end }}"),
            "n"
        );
        assert_eq!(
            run("{{ if eq .Config.mode \"hd\" }}y{{ else }}n{{ end }}"),
            "y"
        );
        assert_eq!(
            run("{{ if ne .Config.mode \"hd\" }}y{{ else }}n{{ end }}"),
            "n"
        );
        assert_eq!(
            run("{{ if and eq .Config.mode \"hd\" eq .Config.user \"sam\" }}y{{ else }}n{{ end }}"),
            "y"
        );
    }

    #[test]
    fn re_replace_join_and_range() {
        assert_eq!(
            run(r#"{{ re_replace .Query.Keywords "[^a-zA-Z0-9]+" "%" }}"#),
            "The%Matrix%1999"
        );
        assert_eq!(run(r#"{{ join .Categories "," }}"#), "1,2,3");
        assert_eq!(
            run("{{ range .Categories }}&c={{.}}{{end}}"),
            "&c=1&c=2&c=3"
        );
        assert_eq!(
            run("{{ range $i, $c := .Categories }}&c{{$i}}={{.}}{{end}}"),
            "&c0=1&c1=2&c2=3"
        );
    }

    #[test]
    fn the_modifier_encodes_what_is_put_in() {
        let m: Modifier = &|s| s.replace(' ', "+");
        assert_eq!(
            apply("/s/{{ .Query.Keywords }}/1", &vars(), Some(m)).unwrap(),
            "/s/The+Matrix+1999/1"
        );
    }

    #[test]
    fn an_unsupported_condition_is_an_error() {
        assert!(apply("{{ if foo }}a{{ else }}b{{ end }}", &vars(), None).is_err());
    }
}
