//! Release-group cases extracted from Radarr's ReleaseGroupParserFixture.

use omp_parser::release_group::parse_release_group;
use serde_json::Value;

fn cases(fixture: &str) -> Vec<(String, Vec<Value>)> {
    let path = format!(
        "{}/tests/data/radarr_parser_cases.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let all: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    all[fixture]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|m| {
            let method = m["method"].as_str().unwrap().to_owned();
            m["cases"]
                .as_array()
                .unwrap()
                .iter()
                .map(move |c| (method.clone(), c["args"].as_array().unwrap().clone()))
        })
        .collect()
}

#[test]
fn release_groups() {
    let mut failures = Vec::new();
    let mut total = 0;
    for (method, args) in cases("ReleaseGroupParserFixture") {
        total += 1;
        let title = args[0].as_str().unwrap();
        // The "should_not_parse_anime_hash..." test has a single argument and expects null.
        let expected = args.get(1).and_then(Value::as_str);
        let got = parse_release_group(title);
        if got.as_deref() != expected {
            failures.push(format!(
                "[{method}] {title:?}: got {got:?}, expected {expected:?}"
            ));
        }
    }
    assert!(total >= 150, "corpus too small: {total}");
    assert!(
        failures.is_empty(),
        "{} of {total} failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
