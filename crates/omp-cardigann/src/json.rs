//! JSON paths for definitions that read JSON (Newtonsoft's `SelectToken` subset
//! plus the `:has()`, `:not()` and `:contains()` row filters Cardigann adds).

use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
enum Step {
    Key(String),
    Index(i64),
    Any,
    Deep(String),
}

fn parse_path(path: &str) -> Option<Vec<Step>> {
    let chars: Vec<char> = path.chars().collect();
    let mut i = 0;
    let mut steps = vec![];
    if chars.first() == Some(&'$') {
        i = 1;
    }
    while i < chars.len() {
        match chars[i] {
            '.' if chars.get(i + 1) == Some(&'.') => {
                i += 2;
                let start = i;
                while i < chars.len() && chars[i] != '.' && chars[i] != '[' {
                    i += 1;
                }
                steps.push(Step::Deep(chars[start..i].iter().collect()));
            }
            '.' => {
                i += 1;
                let start = i;
                while i < chars.len() && chars[i] != '.' && chars[i] != '[' {
                    i += 1;
                }
                let key: String = chars[start..i].iter().collect();
                if key == "*" {
                    steps.push(Step::Any);
                } else if !key.is_empty() {
                    steps.push(Step::Key(key));
                }
            }
            '[' => {
                let end = chars[i..].iter().position(|c| *c == ']')? + i;
                let inner: String = chars[i + 1..end].iter().collect();
                let inner = inner.trim();
                if inner == "*" {
                    steps.push(Step::Any);
                } else if let Ok(n) = inner.parse::<i64>() {
                    steps.push(Step::Index(n));
                } else {
                    steps.push(Step::Key(inner.trim_matches(['\'', '"']).to_owned()));
                }
                i = end + 1;
            }
            _ => {
                // A path may start with a bare name.
                let start = i;
                while i < chars.len() && chars[i] != '.' && chars[i] != '[' {
                    i += 1;
                }
                steps.push(Step::Key(chars[start..i].iter().collect()));
            }
        }
    }
    Some(steps)
}

fn deep<'a>(v: &'a Value, key: &str, out: &mut Vec<&'a Value>) {
    match v {
        Value::Object(m) => {
            if let Some(x) = m.get(key) {
                out.push(x);
            }
            m.values().for_each(|c| deep(c, key, out));
        }
        Value::Array(a) => a.iter().for_each(|c| deep(c, key, out)),
        _ => {}
    }
}

/// The first token a path leads to.
pub fn select_token<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    let steps = parse_path(path)?;
    let mut current = vec![root];
    for step in steps {
        let mut next = vec![];
        for v in current {
            match &step {
                Step::Key(k) => {
                    if let Some(x) = v.get(k.as_str()) {
                        next.push(x);
                    }
                }
                Step::Index(n) => {
                    if let Value::Array(a) = v {
                        let idx = if *n < 0 { a.len() as i64 + n } else { *n };
                        if let Some(x) = usize::try_from(idx).ok().and_then(|i| a.get(i)) {
                            next.push(x);
                        }
                    }
                }
                Step::Any => match v {
                    Value::Array(a) => next.extend(a.iter()),
                    Value::Object(m) => next.extend(m.values()),
                    _ => {}
                },
                Step::Deep(k) => deep(v, k, &mut next),
            }
        }
        if next.is_empty() {
            return None;
        }
        current = next;
    }
    current.into_iter().next()
}

/// A token as text, the way the engine reads a value: arrays joined by commas,
/// whole numbers without a decimal point, null as nothing.
pub fn token_text(v: &Value) -> Option<String> {
    Some(match v {
        Value::Null => return None,
        Value::String(s) => s.clone(),
        Value::Bool(b) => if *b { "True" } else { "False" }.to_owned(),
        Value::Number(n) => match n.as_f64() {
            Some(f) if n.is_f64() && f.fract() == 0.0 && f.abs() < 1e15 => format!("{}", f as i64),
            _ => n.to_string(),
        },
        Value::Array(a) => a
            .iter()
            .filter_map(token_text)
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(_) => return None,
    })
}

/// Splits "a.b:has(c):contains(x)" into the path and its filters.
fn split_filters(selector: &str) -> (&str, Vec<(String, String)>) {
    let path_end = selector.find(':').unwrap_or(selector.len());
    let (path, mut rest) = (&selector[..path_end], &selector[path_end..]);
    let mut filters = vec![];
    while let Some(stripped) = rest.strip_prefix(':') {
        let Some(open) = stripped.find('(') else {
            break;
        };
        let name = stripped[..open].to_owned();
        // The filter ends at the ")" that is followed by ":" or the end.
        let body = &stripped[open + 1..];
        let mut end = None;
        for (i, c) in body.char_indices() {
            if c == ')' && (body[i + 1..].is_empty() || body[i + 1..].starts_with(':')) {
                end = Some(i);
                break;
            }
        }
        let Some(end) = end else { break };
        filters.push((name, body[..end].to_owned()));
        rest = &body[end + 1..];
    }
    (path, filters)
}

/// Applies the filters to what the path selects. `None` means the row or field
/// does not exist; `Some(path)` is the path to read.
pub fn field_selector<'a>(parent: &Value, selector: &'a str) -> Option<&'a str> {
    let (path, filters) = split_filters(selector);
    let target = if path.trim().is_empty() {
        parent
    } else {
        select_token(parent, path)?
    };
    for (name, key) in filters {
        match name.as_str() {
            "has" => {
                let found = if key.contains(':') {
                    field_selector(target, &key).is_some()
                } else {
                    select_token(target, &key).is_some()
                };
                if !found {
                    return None;
                }
            }
            "not" => {
                let found = if key.contains(':') {
                    field_selector(target, &key).is_some()
                } else {
                    select_token(target, &key).is_some()
                };
                if found {
                    return None;
                }
            }
            "contains" if !target.to_string().contains(&key) => return None,
            _ => {}
        }
    }
    Some(path)
}

/// The rows a row selector picks: the array at its path, minus rows its
/// filters rule out. `None` when the path does not lead to an array.
pub fn rows<'a>(root: &'a Value, selector: &str) -> Option<Vec<&'a Value>> {
    let (path, _) = split_filters(selector);
    let array = match select_token(root, path)? {
        Value::Array(a) => a,
        _ => return None,
    };
    let filters = &selector[path.len()..];
    Some(
        array
            .iter()
            .filter(|row| filters.is_empty() || field_selector(row, filters).is_some())
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn paths_reach_values() {
        let v = json!({"data": {"movies": [{"id": 1, "name": "A", "t": {"x": 5}}, {"id": 2}], "n": 2.0}});
        assert_eq!(select_token(&v, "data.movies[0].name"), Some(&json!("A")));
        assert_eq!(select_token(&v, "$.data.n"), Some(&json!(2.0)));
        assert_eq!(select_token(&v, "data.movies[-1].id"), Some(&json!(2)));
        assert_eq!(select_token(&v, "..x"), Some(&json!(5)));
        assert_eq!(select_token(&v, "data['n']"), Some(&json!(2.0)));
        assert_eq!(select_token(&v, "data.nothing"), None);
        assert_eq!(token_text(&json!(2.0)).as_deref(), Some("2"));
        assert_eq!(token_text(&json!([1, "b"])).as_deref(), Some("1,b"));
        assert_eq!(token_text(&json!(null)), None);
    }

    #[test]
    fn row_filters_pick_rows() {
        let v = json!({"list": [{"a": 1, "tags": {"x": 1}}, {"a": 2}, {"a": 3, "tags": {}}]});
        assert_eq!(rows(&v, "list").unwrap().len(), 3);
        assert_eq!(rows(&v, "list:has(tags)").unwrap().len(), 2);
        assert_eq!(rows(&v, "list:not(tags)").unwrap().len(), 1);
        assert_eq!(rows(&v, "list:contains(\"a\":2)").unwrap().len(), 1);
        assert!(rows(&v, "nothing").is_none());
    }
}
