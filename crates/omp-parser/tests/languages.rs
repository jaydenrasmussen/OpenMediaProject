//! Language cases extracted from Radarr's LanguageParserFixture.

mod common;

use common::{cases, check_all, s};
use omp_core::language::Language;
use omp_parser::{language::parse_languages, title::parse_movie_title};
use serde_json::Value;

fn lang(name: &str) -> Language {
    // Names in the C# enum accessors differ slightly from display names.
    let display = match name {
        "PortugueseBR" => "Portuguese (Brazil)",
        "SpanishLatino" => "Spanish (Latino)",
        n => n,
    };
    Language::from_name(display).unwrap_or_else(|| panic!("unknown language {name}"))
}

#[test]
fn language_cases() {
    let path = format!(
        "{}/tests/data/language_expectations.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let exp: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let mut methods = 0;
    for (method, e) in exp.as_object().unwrap() {
        methods += 1;
        let direct = e["mode"] == "direct";
        let is_dir = e["is_dir"].as_bool().unwrap_or(false);
        check_all(method, cases("LanguageParserFixture", method), |a| {
            let title = s(&a[0]);
            let got = if direct {
                parse_languages(title)
            } else {
                parse_movie_title(title, is_dir)
                    .ok_or("did not parse")?
                    .languages
            };
            for want in e["contains"].as_array().unwrap() {
                let want = lang(want.as_str().unwrap());
                if !got.contains(&want) {
                    return Err(format!("{got:?} lacks {want}"));
                }
            }
            for want in e["first"].as_array().unwrap() {
                let want = lang(want.as_str().unwrap());
                if got.first() != Some(&want) {
                    return Err(format!("first of {got:?} is not {want}"));
                }
            }
            if let Some(n) = e["count"].as_u64()
                && got.len() as u64 != n
            {
                return Err(format!("{got:?} has {} entries, want {n}", got.len()));
            }
            Ok(())
        });
    }
    assert!(methods >= 55, "only {methods} methods");
}
