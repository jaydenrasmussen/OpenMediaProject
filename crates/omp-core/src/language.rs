//! Languages. Ported from Radarr `NzbDrone.Core/Languages/Language.cs` (GPL-3.0).
//!
//! Ids and names match Radarr. `ANY` (-1) and `ORIGINAL` (-2) are special:
//! "any language" in profiles and "the movie's original language" in releases.

/// A language. Equality and hashing use the id only.
#[derive(Debug, Clone, Copy)]
pub struct Language {
    pub id: i32,
    pub name: &'static str,
}

impl PartialEq for Language {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
impl Eq for Language {}
impl std::hash::Hash for Language {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}
impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name)
    }
}

impl Language {
    pub const UNKNOWN: Language = Language {
        id: 0,
        name: "Unknown",
    };
    pub const ENGLISH: Language = Language {
        id: 1,
        name: "English",
    };
    pub const FRENCH: Language = Language {
        id: 2,
        name: "French",
    };
    pub const SPANISH: Language = Language {
        id: 3,
        name: "Spanish",
    };
    pub const GERMAN: Language = Language {
        id: 4,
        name: "German",
    };
    pub const ITALIAN: Language = Language {
        id: 5,
        name: "Italian",
    };
    pub const DANISH: Language = Language {
        id: 6,
        name: "Danish",
    };
    pub const DUTCH: Language = Language {
        id: 7,
        name: "Dutch",
    };
    pub const JAPANESE: Language = Language {
        id: 8,
        name: "Japanese",
    };
    pub const ICELANDIC: Language = Language {
        id: 9,
        name: "Icelandic",
    };
    pub const CHINESE: Language = Language {
        id: 10,
        name: "Chinese",
    };
    pub const RUSSIAN: Language = Language {
        id: 11,
        name: "Russian",
    };
    pub const POLISH: Language = Language {
        id: 12,
        name: "Polish",
    };
    pub const VIETNAMESE: Language = Language {
        id: 13,
        name: "Vietnamese",
    };
    pub const SWEDISH: Language = Language {
        id: 14,
        name: "Swedish",
    };
    pub const NORWEGIAN: Language = Language {
        id: 15,
        name: "Norwegian",
    };
    pub const FINNISH: Language = Language {
        id: 16,
        name: "Finnish",
    };
    pub const TURKISH: Language = Language {
        id: 17,
        name: "Turkish",
    };
    pub const PORTUGUESE: Language = Language {
        id: 18,
        name: "Portuguese",
    };
    pub const FLEMISH: Language = Language {
        id: 19,
        name: "Flemish",
    };
    pub const GREEK: Language = Language {
        id: 20,
        name: "Greek",
    };
    pub const KOREAN: Language = Language {
        id: 21,
        name: "Korean",
    };
    pub const HUNGARIAN: Language = Language {
        id: 22,
        name: "Hungarian",
    };
    pub const HEBREW: Language = Language {
        id: 23,
        name: "Hebrew",
    };
    pub const LITHUANIAN: Language = Language {
        id: 24,
        name: "Lithuanian",
    };
    pub const CZECH: Language = Language {
        id: 25,
        name: "Czech",
    };
    pub const HINDI: Language = Language {
        id: 26,
        name: "Hindi",
    };
    pub const ROMANIAN: Language = Language {
        id: 27,
        name: "Romanian",
    };
    pub const THAI: Language = Language {
        id: 28,
        name: "Thai",
    };
    pub const BULGARIAN: Language = Language {
        id: 29,
        name: "Bulgarian",
    };
    pub const PORTUGUESE_BR: Language = Language {
        id: 30,
        name: "Portuguese (Brazil)",
    };
    pub const ARABIC: Language = Language {
        id: 31,
        name: "Arabic",
    };
    pub const UKRAINIAN: Language = Language {
        id: 32,
        name: "Ukrainian",
    };
    pub const PERSIAN: Language = Language {
        id: 33,
        name: "Persian",
    };
    pub const BENGALI: Language = Language {
        id: 34,
        name: "Bengali",
    };
    pub const SLOVAK: Language = Language {
        id: 35,
        name: "Slovak",
    };
    pub const LATVIAN: Language = Language {
        id: 36,
        name: "Latvian",
    };
    pub const SPANISH_LATINO: Language = Language {
        id: 37,
        name: "Spanish (Latino)",
    };
    pub const CATALAN: Language = Language {
        id: 38,
        name: "Catalan",
    };
    pub const CROATIAN: Language = Language {
        id: 39,
        name: "Croatian",
    };
    pub const SERBIAN: Language = Language {
        id: 40,
        name: "Serbian",
    };
    pub const BOSNIAN: Language = Language {
        id: 41,
        name: "Bosnian",
    };
    pub const ESTONIAN: Language = Language {
        id: 42,
        name: "Estonian",
    };
    pub const TAMIL: Language = Language {
        id: 43,
        name: "Tamil",
    };
    pub const INDONESIAN: Language = Language {
        id: 44,
        name: "Indonesian",
    };
    pub const TELUGU: Language = Language {
        id: 45,
        name: "Telugu",
    };
    pub const MACEDONIAN: Language = Language {
        id: 46,
        name: "Macedonian",
    };
    pub const SLOVENIAN: Language = Language {
        id: 47,
        name: "Slovenian",
    };
    pub const MALAYALAM: Language = Language {
        id: 48,
        name: "Malayalam",
    };
    pub const KANNADA: Language = Language {
        id: 49,
        name: "Kannada",
    };
    pub const ALBANIAN: Language = Language {
        id: 50,
        name: "Albanian",
    };
    pub const AFRIKAANS: Language = Language {
        id: 51,
        name: "Afrikaans",
    };
    pub const MARATHI: Language = Language {
        id: 52,
        name: "Marathi",
    };
    pub const TAGALOG: Language = Language {
        id: 53,
        name: "Tagalog",
    };
    pub const URDU: Language = Language {
        id: 54,
        name: "Urdu",
    };
    pub const ROMANSH: Language = Language {
        id: 55,
        name: "Romansh",
    };
    pub const MONGOLIAN: Language = Language {
        id: 56,
        name: "Mongolian",
    };
    pub const GEORGIAN: Language = Language {
        id: 57,
        name: "Georgian",
    };
    pub const ANY: Language = Language {
        id: -1,
        name: "Any",
    };
    pub const ORIGINAL: Language = Language {
        id: -2,
        name: "Original",
    };

    /// Every language, in Radarr's order.
    pub const ALL: [Language; 60] = [
        Self::UNKNOWN,
        Self::ENGLISH,
        Self::FRENCH,
        Self::SPANISH,
        Self::GERMAN,
        Self::ITALIAN,
        Self::DANISH,
        Self::DUTCH,
        Self::JAPANESE,
        Self::ICELANDIC,
        Self::CHINESE,
        Self::RUSSIAN,
        Self::POLISH,
        Self::VIETNAMESE,
        Self::SWEDISH,
        Self::NORWEGIAN,
        Self::FINNISH,
        Self::TURKISH,
        Self::PORTUGUESE,
        Self::FLEMISH,
        Self::GREEK,
        Self::KOREAN,
        Self::HUNGARIAN,
        Self::HEBREW,
        Self::LITHUANIAN,
        Self::CZECH,
        Self::ROMANIAN,
        Self::HINDI,
        Self::THAI,
        Self::BULGARIAN,
        Self::PORTUGUESE_BR,
        Self::ARABIC,
        Self::UKRAINIAN,
        Self::PERSIAN,
        Self::BENGALI,
        Self::SLOVAK,
        Self::LATVIAN,
        Self::SPANISH_LATINO,
        Self::CATALAN,
        Self::CROATIAN,
        Self::SERBIAN,
        Self::BOSNIAN,
        Self::ESTONIAN,
        Self::TAMIL,
        Self::INDONESIAN,
        Self::TELUGU,
        Self::MACEDONIAN,
        Self::SLOVENIAN,
        Self::MALAYALAM,
        Self::KANNADA,
        Self::ALBANIAN,
        Self::AFRIKAANS,
        Self::MARATHI,
        Self::TAGALOG,
        Self::URDU,
        Self::ROMANSH,
        Self::MONGOLIAN,
        Self::GEORGIAN,
        Self::ANY,
        Self::ORIGINAL,
    ];

    pub fn from_id(id: i32) -> Option<Language> {
        Self::ALL.iter().copied().find(|l| l.id == id)
    }

    /// Case-insensitive lookup by display name ("Spanish (Latino)").
    pub fn from_name(name: &str) -> Option<Language> {
        Self::ALL
            .iter()
            .copied()
            .find(|l| l.name.eq_ignore_ascii_case(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_unique_and_lookup_works() {
        let mut ids: Vec<_> = Language::ALL.iter().map(|l| l.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), Language::ALL.len());
        assert_eq!(Language::from_id(37), Some(Language::SPANISH_LATINO));
        assert_eq!(
            Language::from_name("portuguese (brazil)"),
            Some(Language::PORTUGUESE_BR)
        );
        assert_eq!(Language::from_name("Klingon"), None);
    }
}
