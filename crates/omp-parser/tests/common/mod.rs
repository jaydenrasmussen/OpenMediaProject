#![allow(dead_code)]
//! Helpers that read the cases extracted from Radarr's NUnit fixtures.

use serde_json::Value;

/// Cases of one test method: each case is the list of its arguments.
pub fn cases(fixture: &str, method: &str) -> Vec<Vec<Value>> {
    let path = format!(
        "{}/tests/data/radarr_parser_cases.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let all: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let methods = all[fixture]
        .as_array()
        .unwrap_or_else(|| panic!("no fixture {fixture}"));
    let cases: Vec<Vec<Value>> = methods
        .iter()
        .filter(|m| m["method"] == method)
        .flat_map(|m| m["cases"].as_array().unwrap().clone())
        .map(|c| c["args"].as_array().cloned().unwrap_or_default())
        .collect();
    assert!(!cases.is_empty(), "no cases for {fixture}.{method}");
    cases
}

pub fn s(v: &Value) -> &str {
    v.as_str().unwrap_or_else(|| panic!("not a string: {v}"))
}

/// Runs `f` for every case and reports all failures together.
pub fn check_all(
    name: &str,
    cases: Vec<Vec<Value>>,
    mut f: impl FnMut(&[Value]) -> Result<(), String>,
) {
    let total = cases.len();
    let failures: Vec<String> = cases
        .iter()
        .filter_map(|c| {
            f(c).err().map(|e| {
                format!(
                    "{:?}: {e}",
                    c.first().map(|v| v.to_string()).unwrap_or_default()
                )
            })
        })
        .collect();
    assert!(
        failures.is_empty(),
        "{name}: {} of {total} failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
