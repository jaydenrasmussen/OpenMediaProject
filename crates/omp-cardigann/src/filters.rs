//! The filters a definition can chain on a value.

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use encoding_rs::Encoding;
use fancy_regex::Regex;

use crate::def::Filter;
use crate::template::{self, Vars};
use crate::util;

pub struct Context<'a> {
    pub vars: &'a Vars,
    pub encoding: &'static Encoding,
}

fn arg(f: &Filter, i: usize) -> Result<&str, String> {
    f.args
        .nth(i)
        .ok_or_else(|| format!("the filter '{}' needs argument {}", f.name, i + 1))
}

fn regex(pattern: &str) -> Result<Regex, String> {
    util::compile(pattern)
}

/// Characters a file name may not hold.
fn valid_file_name(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_control() || "/\\:*?\"<>|".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect()
}

/// Runs the chain on a value.
pub fn apply(mut data: String, filters: &[Filter], ctx: &Context<'_>) -> Result<String, String> {
    for f in filters {
        data = one(data, f, ctx)?;
    }
    Ok(data)
}

fn one(data: String, f: &Filter, ctx: &Context<'_>) -> Result<String, String> {
    Ok(match f.name.as_str() {
        "querystring" => util::query_argument(&data, arg(f, 0)?)
            .ok_or_else(|| format!("no '{}' in the address", f.args.first().unwrap_or_default()))?,
        "timeparse" | "dateparse" => match util::parse_with_layout(&data, arg(f, 0)?) {
            Ok(d) => util::to_text(d),
            // The original logs a date it cannot read and carries on with the text.
            Err(_) => data,
        },
        "regexp" => {
            let re = regex(arg(f, 0)?)?;
            match re.captures(&data).map_err(|e| e.to_string())? {
                Some(c) => c.get(1).map_or(String::new(), |m| m.as_str().to_owned()),
                None => String::new(),
            }
        }
        "re_replace" => {
            let re = regex(arg(f, 0)?)?;
            let replacement = template::apply(arg(f, 1)?, ctx.vars, None)?;
            util::net_replace(&re, &data, &replacement)
        }
        "split" => {
            let sep = arg(f, 0)?.chars().next().ok_or("split needs a separator")?;
            let mut pos: i64 = arg(f, 1)?
                .trim()
                .parse()
                .map_err(|_| "split needs a number")?;
            let parts: Vec<&str> = data.split(sep).collect();
            if pos < 0 {
                pos += parts.len() as i64;
            }
            parts
                .get(usize::try_from(pos).map_err(|_| "split position is before the start")?)
                .ok_or("split position is past the end")?
                .to_string()
        }
        "replace" => {
            let to = template::apply(arg(f, 1)?, ctx.vars, None)?;
            let from = arg(f, 0)?;
            if from.is_empty() {
                data
            } else {
                data.replace(from, &to)
            }
        }
        "trim" => match f.args.first().and_then(|s| s.chars().next()) {
            Some(c) => data.trim_matches(c).to_owned(),
            None => data.trim().to_owned(),
        },
        "prepend" => format!("{}{data}", template::apply(arg(f, 0)?, ctx.vars, None)?),
        "append" => format!("{data}{}", template::apply(arg(f, 0)?, ctx.vars, None)?),
        "tolower" => data.to_lowercase(),
        "toupper" => data.to_uppercase(),
        "base64decode" => {
            let bytes = URL_SAFE_NO_PAD
                .decode(data.trim_end_matches('='))
                .or_else(|_| STANDARD.decode(&data))
                .map_err(|e| e.to_string())?;
            String::from_utf8_lossy(&bytes).into_owned()
        }
        "base64encode" => URL_SAFE_NO_PAD.encode(data.as_bytes()),
        "urldecode" => util::url_decode(&data, ctx.encoding),
        "urlencode" => util::url_encode(&data, ctx.encoding),
        "htmldecode" => util::html_decode(&data),
        "htmlencode" => util::html_encode(&data),
        "timeago" | "reltime" => util::to_text(util::from_time_ago(&data)?),
        "fuzzytime" => util::to_text(util::from_unknown(&data)?),
        "validfilename" => valid_file_name(&data),
        "diacritics" => match f.args.first() {
            Some("replace") => util::strip_diacritics(&data),
            _ => return Err("unsupported diacritics filter argument".into()),
        },
        "jsonjoinarray" => {
            let value: serde_json::Value =
                serde_json::from_str(&data).map_err(|e| e.to_string())?;
            let selected =
                crate::json::select_token(&value, arg(f, 0)?).ok_or("nothing at that path")?;
            let items: Vec<String> = match selected {
                serde_json::Value::Array(a) => {
                    a.iter().filter_map(crate::json::token_text).collect()
                }
                other => crate::json::token_text(other).into_iter().collect(),
            };
            items.join(arg(f, 1)?)
        }
        "validate" => {
            let delimiters: &[char] = &[',', ' ', '/', ')', '(', '.', ';', '[', ']', '"', '|', ':'];
            let allowed: Vec<String> = arg(f, 0)?
                .to_lowercase()
                .split(delimiters)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect();
            let lowered = data.to_lowercase();
            let present: Vec<&str> = lowered
                .split(delimiters)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect();
            let mut out: Vec<&str> = vec![];
            for a in &allowed {
                if present.contains(&a.as_str()) && !out.contains(&a.as_str()) {
                    out.push(a);
                }
            }
            out.join(", ")
        }
        // Debugging aids, and a row filter handled elsewhere.
        "hexdump" | "strdump" | "andmatch" => data,
        other => return Err(format!("unsupported filter '{other}'")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::def::{Args, Filter};
    use crate::template::Val;

    fn f(name: &str, args: &[&str]) -> Filter {
        Filter {
            name: name.into(),
            args: match args.len() {
                0 => Args::None,
                1 => Args::One(args[0].into()),
                _ => Args::Many(args.iter().map(|s| s.to_string()).collect()),
            },
        }
    }

    fn run(data: &str, filters: &[Filter]) -> Result<String, String> {
        let mut vars = Vars::new();
        vars.insert(".Config.sitelink".into(), Val::str("https://x/"));
        apply(
            data.into(),
            filters,
            &Context {
                vars: &vars,
                encoding: encoding_rs::UTF_8,
            },
        )
    }

    #[test]
    fn text_filters() {
        assert_eq!(
            run(
                " a-b ",
                &[f("trim", &[]), f("replace", &["-", " "]), f("toupper", &[])]
            )
            .unwrap(),
            "A B"
        );
        assert_eq!(
            run(
                "x",
                &[
                    f("prepend", &["{{ .Config.sitelink }}d/"]),
                    f("append", &[".torrent"])
                ]
            )
            .unwrap(),
            "https://x/d/x.torrent"
        );
        assert_eq!(
            run("S01E02 Show", &[f("regexp", &[r"S(\d+)E"])]).unwrap(),
            "01"
        );
        assert_eq!(run("none", &[f("regexp", &[r"S(\d+)E"])]).unwrap(), "");
        assert_eq!(run("a/b/c", &[f("split", &["/", "-1"])]).unwrap(), "c");
        assert_eq!(run("a/b/c", &[f("split", &["/", "1"])]).unwrap(), "b");
        assert_eq!(
            run("1999.1080p", &[f("re_replace", &[r"(\d{4})\.", "$1 "])]).unwrap(),
            "1999 1080p"
        );
        assert_eq!(run("a&amp;b", &[f("htmldecode", &[])]).unwrap(), "a&b");
        assert_eq!(run("a b", &[f("urlencode", &[])]).unwrap(), "a+b");
        assert_eq!(run("a/b:c", &[f("validfilename", &[])]).unwrap(), "a_b_c");
        assert_eq!(
            run("café", &[f("diacritics", &["replace"])]).unwrap(),
            "cafe"
        );
        assert_eq!(
            run("hello", &[f("base64encode", &[]), f("base64decode", &[])]).unwrap(),
            "hello"
        );
    }

    #[test]
    fn address_and_date_filters() {
        assert_eq!(
            run("https://x/d.php?id=42&k=1", &[f("querystring", &["id"])]).unwrap(),
            "42"
        );
        assert!(run("https://x/", &[f("querystring", &["id"])]).is_err());
        let d = run("2023-04-05 14:30", &[f("dateparse", &["yyyy-MM-dd HH:mm"])]).unwrap();
        assert_eq!(d, "Wed, 5 Apr 2023 14:30:00 +0000");
        // A date that does not fit is passed on as it was.
        assert_eq!(
            run("later", &[f("dateparse", &["yyyy-MM-dd"])]).unwrap(),
            "later"
        );
        assert_eq!(
            run("Comedy / Drama", &[f("validate", &["drama, action"])]).unwrap(),
            "drama"
        );
    }

    #[test]
    fn an_unknown_filter_is_named() {
        assert!(
            run("x", &[f("frobnicate", &[])])
                .unwrap_err()
                .contains("frobnicate")
        );
    }
}
