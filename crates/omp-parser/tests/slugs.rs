//! Slug cases extracted from Radarr's SlugParserFixture.

mod common;

use common::{cases, check_all, s};
use omp_parser::normalize::{to_url_slug, to_url_slug_with};

fn eq(got: String, want: &str) -> Result<(), String> {
    if got == want {
        Ok(())
    } else {
        Err(format!("got {got:?}, want {want:?}"))
    }
}

#[test]
fn slugs() {
    let f = "SlugParserFixture";
    check_all("accents", cases(f, "should_replace_accents"), |a| {
        eq(to_url_slug(s(&a[0]), false), s(&a[1]))
    });
    check_all(
        "special",
        cases(f, "should_replace_special_characters"),
        |a| eq(to_url_slug(s(&a[0]), false), "testresult"),
    );
    check_all("lowercase", cases(f, "should_lowercase_capitals"), |a| {
        eq(
            to_url_slug(s(&a[0]), false),
            "this-is-a-mixed-case-sensitive-value",
        )
    });
    check_all(
        "trim",
        cases(f, "should_trim_trailing_dashes_and_underscores"),
        |a| eq(to_url_slug(s(&a[0]), false), "test"),
    );
    check_all("spaces", cases(f, "should_replace_spaces_with_dash"), |a| {
        eq(to_url_slug(s(&a[0]), false), "test-result")
    });
    check_all("double", cases(f, "should_replace_double_occurence"), |a| {
        eq(to_url_slug(s(&a[0]), false), s(&a[1]))
    });
    check_all(
        "dash on",
        cases(
            f,
            "should_replace_special_characters_with_dash_when_enabled",
        ),
        |a| eq(to_url_slug(s(&a[0]), true), "test-result"),
    );
    check_all(
        "dash off",
        cases(
            f,
            "should__not_replace_special_characters_with_dash_when_disabled",
        ),
        |a| eq(to_url_slug(s(&a[0]), false), "testresult"),
    );
    check_all(
        "trim list",
        cases(
            f,
            "should_trim_trailing_dashes_and_underscores_based_on_list",
        ),
        |a| eq(to_url_slug_with(s(&a[0]), false, s(&a[1]), ""), s(&a[2])),
    );
    check_all(
        "dedupe list",
        cases(f, "should_replace_duplicate_characters_based_on_list"),
        |a| eq(to_url_slug_with(s(&a[0]), false, "", s(&a[1])), s(&a[2])),
    );
    assert_eq!(to_url_slug_with("test", false, "", "-_"), "test");
    assert_eq!(to_url_slug_with("test", false, "-_", ""), "test");
}
