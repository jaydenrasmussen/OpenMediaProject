//! Custom formats: named sets of conditions on a release, scored by profiles.
//!
//! A format matches when, for each *kind* of condition it contains, no
//! `required` condition of that kind failed and at least one condition of that
//! kind matched. A condition's `negate` flag inverts its own result.

use std::fmt;

use serde::{Deserialize, Serialize};

use omp_core::{language::Language, quality::Quality, release::IndexerFlags};
use omp_parser::re::Re;

/// A user-supplied, case-insensitive regex.
pub struct Pattern {
    raw: String,
    re: Re,
}

impl Pattern {
    pub fn new(raw: &str) -> Result<Self, String> {
        if raw.trim().is_empty() {
            return Err("Regex Pattern must not be empty".to_owned());
        }
        Ok(Self {
            raw: raw.to_owned(),
            re: Re::try_ci(raw)?,
        })
    }

    pub fn as_str(&self) -> &str {
        &self.raw
    }

    fn is_match(&self, text: Option<&str>) -> bool {
        text.is_some_and(|t| self.re.is_match(t))
    }
}

impl fmt::Debug for Pattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Pattern({:?})", self.raw)
    }
}

/// Everything a condition can look at.
#[derive(Debug, Clone, Default)]
pub struct CustomFormatInput {
    pub quality: Option<Quality>,
    pub edition: Option<String>,
    pub release_group: Option<String>,
    pub simple_release_title: Option<String>,
    pub filename: Option<String>,
    /// Year parsed from the release; 0 when none.
    pub parsed_year: i32,
    /// Library movie's year, used when the release has none.
    pub movie_year: Option<i32>,
    /// The library movie's original language, for `Language::ORIGINAL` conditions.
    pub movie_original_language: Option<Language>,
    pub size: u64,
    pub indexer_flags: IndexerFlags,
    pub languages: Vec<Language>,
}

#[derive(Debug)]
pub enum Condition {
    ReleaseTitle(Pattern),
    Edition(Pattern),
    ReleaseGroup(Pattern),
    Language {
        value: i32,
        except_language: bool,
    },
    IndexerFlag(IndexerFlags),
    Source(i32),
    Resolution(i32),
    QualityModifier(i32),
    /// Gigabytes; matches `min < size <= max`.
    Size {
        min: f64,
        max: f64,
    },
    Year {
        min: i32,
        max: i32,
    },
}

impl Condition {
    /// Conditions are grouped by this when combining them.
    fn kind(&self) -> u8 {
        match self {
            Condition::ReleaseTitle(_) => 1,
            Condition::Edition(_) => 2,
            Condition::Language { .. } => 3,
            Condition::IndexerFlag(_) => 4,
            Condition::Source(_) => 5,
            Condition::Resolution(_) => 6,
            Condition::QualityModifier(_) => 7,
            Condition::Size { .. } => 8,
            Condition::ReleaseGroup(_) => 9,
            Condition::Year { .. } => 10,
        }
    }
}

#[derive(Debug)]
pub struct Specification {
    pub name: String,
    pub negate: bool,
    pub required: bool,
    pub condition: Condition,
}

const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

impl Specification {
    pub fn is_satisfied_by(&self, input: &CustomFormatInput) -> bool {
        if let Condition::Language {
            value,
            except_language,
        } = &self.condition
        {
            return self.language_matches(input, *value, *except_language);
        }
        let matched = self.matches_without_negate(input);
        if self.negate { !matched } else { matched }
    }

    fn matches_without_negate(&self, input: &CustomFormatInput) -> bool {
        match &self.condition {
            Condition::ReleaseTitle(p) => {
                p.is_match(input.simple_release_title.as_deref())
                    || p.is_match(input.filename.as_deref())
            }
            Condition::Edition(p) => p.is_match(input.edition.as_deref()),
            Condition::ReleaseGroup(p) => p.is_match(input.release_group.as_deref()),
            Condition::IndexerFlag(flag) => input.indexer_flags.contains(*flag),
            Condition::Source(v) => input.quality.map_or(0, |q| q.source as i32) == *v,
            Condition::Resolution(v) => input.quality.map_or(0, |q| i32::from(q.resolution)) == *v,
            Condition::QualityModifier(v) => input.quality.map_or(0, |q| q.modifier as i32) == *v,
            Condition::Size { min, max } => {
                let size = input.size as f64;
                size > (*min * GIB).trunc() && size <= (*max * GIB).trunc()
            }
            Condition::Year { min, max } => {
                let year = if input.parsed_year > 0 {
                    Some(input.parsed_year)
                } else {
                    input.movie_year
                };
                year.is_some_and(|y| y >= *min && y <= *max)
            }
            Condition::Language { .. } => unreachable!("handled in is_satisfied_by"),
        }
    }

    /// Language conditions apply `negate` themselves, as upstream does.
    fn language_matches(&self, input: &CustomFormatInput, value: i32, except: bool) -> bool {
        let compared = if value == Language::ORIGINAL.id {
            match input.movie_original_language {
                Some(l) if l != Language::UNKNOWN => Some(l),
                _ => Language::from_id(value),
            }
        } else {
            Language::from_id(value)
        };
        let Some(compared) = compared else {
            return false;
        };
        let any_other = input.languages.iter().any(|l| *l != compared);
        let has = input.languages.contains(&compared);
        match (self.negate, except) {
            (false, false) => has,
            (false, true) => any_other,
            (true, false) => !has,
            (true, true) => !any_other,
        }
    }
}

#[derive(Debug)]
pub struct CustomFormat {
    pub id: i32,
    pub name: String,
    pub include_when_renaming: bool,
    pub specifications: Vec<Specification>,
}

impl CustomFormat {
    pub fn matches(&self, input: &CustomFormatInput) -> bool {
        // Group conditions by kind, in first-seen order.
        let mut kinds: Vec<u8> = Vec::new();
        for s in &self.specifications {
            let k = s.condition.kind();
            if !kinds.contains(&k) {
                kinds.push(k);
            }
        }
        kinds.iter().all(|kind| {
            let results: Vec<(&Specification, bool)> = self
                .specifications
                .iter()
                .filter(|s| s.condition.kind() == *kind)
                .map(|s| (s, s.is_satisfied_by(input)))
                .collect();
            let required_failed = results.iter().any(|(s, ok)| s.required && !ok);
            let all_failed = results.iter().all(|(_, ok)| !ok);
            !(required_failed || all_failed)
        })
    }
}

/// The formats from `all` that match `input`, ordered by name.
pub fn calculate<'a>(all: &'a [CustomFormat], input: &CustomFormatInput) -> Vec<&'a CustomFormat> {
    let mut matched: Vec<&CustomFormat> = all.iter().filter(|f| f.matches(input)).collect();
    matched.sort_by(|a, b| a.name.cmp(&b.name));
    matched
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(condition: Condition) -> Specification {
        Specification {
            name: "s".into(),
            negate: false,
            required: false,
            condition,
        }
    }

    fn title(p: &str) -> Condition {
        Condition::ReleaseTitle(Pattern::new(p).unwrap())
    }

    fn input(title: &str) -> CustomFormatInput {
        CustomFormatInput {
            simple_release_title: Some(title.to_owned()),
            quality: Some(Quality::BLURAY_1080P),
            ..Default::default()
        }
    }

    fn format(specs: Vec<Specification>) -> CustomFormat {
        CustomFormat {
            id: 1,
            name: "f".into(),
            include_when_renaming: false,
            specifications: specs,
        }
    }

    #[test]
    fn any_of_a_kind_is_enough() {
        let f = format(vec![spec(title("hdr")), spec(title("dv"))]);
        assert!(f.matches(&input("Movie 2160p DV")));
        assert!(!f.matches(&input("Movie 2160p")));
    }

    #[test]
    fn required_condition_must_match() {
        let mut must = spec(title("remux"));
        must.required = true;
        let f = format(vec![must, spec(title("hdr"))]);
        assert!(f.matches(&input("Movie Remux HDR")));
        assert!(!f.matches(&input("Movie HDR")), "required condition failed");
    }

    #[test]
    fn different_kinds_must_all_match() {
        let f = format(vec![
            spec(title("movie")),
            spec(Condition::Resolution(1080)),
        ]);
        assert!(f.matches(&input("Movie")));
        let mut other = input("Movie");
        other.quality = Some(Quality::BLURAY_720P);
        assert!(!f.matches(&other), "resolution kind has no match");
    }

    #[test]
    fn negate_inverts_a_condition() {
        let mut not_cam = spec(title("cam"));
        not_cam.negate = true;
        let f = format(vec![not_cam]);
        assert!(f.matches(&input("Movie BluRay")));
        assert!(!f.matches(&input("Movie CAM")));
    }

    #[test]
    fn size_is_exclusive_low_inclusive_high_in_gib() {
        let f = format(vec![spec(Condition::Size { min: 1.0, max: 2.0 })]);
        let mut i = input("x");
        i.size = 1024 * 1024 * 1024;
        assert!(!f.matches(&i), "min is exclusive");
        i.size += 1;
        assert!(f.matches(&i));
        i.size = 2 * 1024 * 1024 * 1024;
        assert!(f.matches(&i), "max is inclusive");
        i.size += 1;
        assert!(!f.matches(&i));
    }

    #[test]
    fn source_resolution_modifier_use_the_quality() {
        use omp_core::quality::{Modifier, QualitySource};
        let i = input("x");
        let src = format(vec![spec(Condition::Source(QualitySource::Bluray as i32))]);
        let res = format(vec![spec(Condition::Resolution(1080))]);
        let remux = format(vec![spec(Condition::QualityModifier(
            Modifier::Remux as i32,
        ))]);
        assert!(src.matches(&i));
        assert!(res.matches(&i));
        assert!(!remux.matches(&i));
    }

    #[test]
    fn language_conditions() {
        let mut i = input("x");
        i.languages = vec![Language::GERMAN, Language::ENGLISH];
        let has_german = format(vec![spec(Condition::Language {
            value: Language::GERMAN.id,
            except_language: false,
        })]);
        let except_german = format(vec![spec(Condition::Language {
            value: Language::GERMAN.id,
            except_language: true,
        })]);
        assert!(has_german.matches(&i));
        assert!(
            except_german.matches(&i),
            "English is a language other than German"
        );

        let mut negated = spec(Condition::Language {
            value: Language::GERMAN.id,
            except_language: false,
        });
        negated.negate = true;
        assert!(!format(vec![negated]).matches(&i));

        // "Original" resolves to the movie's original language when known.
        let original = format(vec![spec(Condition::Language {
            value: Language::ORIGINAL.id,
            except_language: false,
        })]);
        i.movie_original_language = Some(Language::GERMAN);
        assert!(original.matches(&i));
        i.movie_original_language = None;
        assert!(!original.matches(&i));
    }

    #[test]
    fn year_falls_back_to_the_movie() {
        let f = format(vec![spec(Condition::Year {
            min: 1990,
            max: 1999,
        })]);
        let mut i = input("x");
        i.movie_year = Some(1995);
        assert!(f.matches(&i));
        i.parsed_year = 2005;
        assert!(!f.matches(&i), "release year wins when present");
    }

    #[test]
    fn indexer_flag_and_group() {
        let flag = format(vec![spec(Condition::IndexerFlag(
            IndexerFlags::G_FREELEECH,
        ))]);
        let mut i = input("x");
        assert!(!flag.matches(&i));
        i.indexer_flags = IndexerFlags::G_FREELEECH | IndexerFlags::G_SCENE;
        assert!(flag.matches(&i));

        let grp = format(vec![spec(Condition::ReleaseGroup(
            Pattern::new("^SPARKS$").unwrap(),
        ))]);
        i.release_group = Some("sparks".into());
        assert!(grp.matches(&i), "case-insensitive");
    }

    #[test]
    fn release_title_also_checks_the_filename() {
        let f = format(vec![spec(title("special"))]);
        let mut i = input("Movie");
        i.filename = Some("Movie.Special.mkv".into());
        assert!(f.matches(&i));
    }

    #[test]
    fn bad_patterns_are_rejected() {
        assert!(Pattern::new("").is_err());
        assert!(Pattern::new("(oops").is_err());
    }

    #[test]
    fn results_are_sorted_by_name() {
        let mk = |id, name: &str| CustomFormat {
            id,
            name: name.into(),
            include_when_renaming: false,
            specifications: vec![spec(title("movie"))],
        };
        let all = vec![mk(1, "Zed"), mk(2, "Alpha")];
        let names: Vec<_> = calculate(&all, &input("Movie"))
            .iter()
            .map(|f| f.name.clone())
            .collect();
        assert_eq!(names, ["Alpha", "Zed"]);
    }
}

/// The storable form of a condition. Radarr stores the implementation name
/// and a few fields; this keeps that shape so imports are straightforward.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "implementation", rename_all = "snake_case")]
pub enum ConditionDef {
    ReleaseTitle { value: String },
    Edition { value: String },
    ReleaseGroup { value: String },
    Language { value: i32, except_language: bool },
    IndexerFlag { value: u32 },
    Source { value: i32 },
    Resolution { value: i32 },
    QualityModifier { value: i32 },
    Size { min: f64, max: f64 },
    Year { min: i32, max: i32 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpecificationDef {
    pub name: String,
    #[serde(default)]
    pub negate: bool,
    #[serde(default)]
    pub required: bool,
    #[serde(flatten)]
    pub condition: ConditionDef,
}

impl SpecificationDef {
    /// Compiles the condition. Fails when a regex is empty or invalid.
    pub fn build(&self) -> Result<Specification, String> {
        let condition = match &self.condition {
            ConditionDef::ReleaseTitle { value } => Condition::ReleaseTitle(Pattern::new(value)?),
            ConditionDef::Edition { value } => Condition::Edition(Pattern::new(value)?),
            ConditionDef::ReleaseGroup { value } => Condition::ReleaseGroup(Pattern::new(value)?),
            ConditionDef::Language {
                value,
                except_language,
            } => Condition::Language {
                value: *value,
                except_language: *except_language,
            },
            ConditionDef::IndexerFlag { value } => Condition::IndexerFlag(IndexerFlags(*value)),
            ConditionDef::Source { value } => Condition::Source(*value),
            ConditionDef::Resolution { value } => Condition::Resolution(*value),
            ConditionDef::QualityModifier { value } => Condition::QualityModifier(*value),
            ConditionDef::Size { min, max } => {
                if *min < 0.0 || max <= min {
                    return Err("Size: max must be greater than min, and min at least 0".to_owned());
                }
                Condition::Size {
                    min: *min,
                    max: *max,
                }
            }
            ConditionDef::Year { min, max } => {
                if *min <= 0 || max < min {
                    return Err("Year: min must be positive and max at least min".to_owned());
                }
                Condition::Year {
                    min: *min,
                    max: *max,
                }
            }
        };
        Ok(Specification {
            name: self.name.clone(),
            negate: self.negate,
            required: self.required,
            condition,
        })
    }
}

/// The storable form of a custom format.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomFormatDef {
    /// Assigned on creation; clients may leave it out.
    #[serde(default)]
    pub id: i32,
    pub name: String,
    #[serde(default)]
    pub include_when_renaming: bool,
    pub specifications: Vec<SpecificationDef>,
}

impl CustomFormatDef {
    pub fn build(&self) -> Result<CustomFormat, String> {
        let specifications = self
            .specifications
            .iter()
            .map(|s| s.build().map_err(|e| format!("{}: {e}", s.name)))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(CustomFormat {
            id: self.id,
            name: self.name.clone(),
            include_when_renaming: self.include_when_renaming,
            specifications,
        })
    }
}

#[cfg(test)]
mod def_tests {
    use super::*;

    #[test]
    fn definitions_round_trip_and_build() {
        let def = CustomFormatDef {
            id: 7,
            name: "Remux".into(),
            include_when_renaming: true,
            specifications: vec![
                SpecificationDef {
                    name: "title".into(),
                    negate: false,
                    required: true,
                    condition: ConditionDef::ReleaseTitle {
                        value: "remux".into(),
                    },
                },
                SpecificationDef {
                    name: "1080".into(),
                    negate: false,
                    required: false,
                    condition: ConditionDef::Resolution { value: 1080 },
                },
            ],
        };
        let json = serde_json::to_string(&def).unwrap();
        assert!(
            json.contains(r#""implementation":"release_title""#),
            "{json}"
        );
        let back: CustomFormatDef = serde_json::from_str(&json).unwrap();
        assert_eq!(back, def);

        let format = back.build().unwrap();
        let input = CustomFormatInput {
            simple_release_title: Some("Movie 2020 REMUX".into()),
            quality: Some(omp_core::quality::Quality::REMUX_1080P),
            ..Default::default()
        };
        assert!(format.matches(&input));
    }

    #[test]
    fn invalid_definitions_name_the_failing_condition() {
        let mut def = CustomFormatDef {
            id: 1,
            name: "bad".into(),
            include_when_renaming: false,
            specifications: vec![SpecificationDef {
                name: "broken regex".into(),
                negate: false,
                required: false,
                condition: ConditionDef::ReleaseTitle {
                    value: "(oops".into(),
                },
            }],
        };
        let err = def.build().unwrap_err();
        assert!(err.starts_with("broken regex:"), "{err}");

        def.specifications[0].condition = ConditionDef::Size { min: 5.0, max: 1.0 };
        assert!(def.build().is_err());
        def.specifications[0].condition = ConditionDef::Year { min: 0, max: 2000 };
        assert!(def.build().is_err());
    }
}
