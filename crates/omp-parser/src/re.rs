//! A thin regex wrapper that lets us keep Radarr's patterns almost verbatim.
//!
//! The originals are .NET regexes. Most compile with `fancy-regex`, which is
//! Unicode-aware like .NET. A few use variable-length look-behind or
//! back-references inside look-behind; those fall back to `regress`, an
//! ECMAScript-style engine that supports both. Patterns choose the engine
//! automatically: fancy-regex first, regress if that refuses to compile.
//!
//! `regress` treats `\w`, `\W`, `\b` and `\d` as ASCII-only, unlike .NET. For
//! the fallback we rewrite those to Unicode property classes (`to_es_unicode`)
//! and compile with the `u` flag.

use std::ops::Range;

/// .NET's `\w`: letters, non-spacing marks, decimal digits, connector punctuation.
const WORD: &str = r"\p{L}\p{Mn}\p{Nd}\p{Pc}";

/// Characters that may follow a backslash as a plain escape in `u` mode.
const SYNTAX: &str = "^$\\.*+?()[]{}|/";

/// Rewrites a .NET pattern for `regress` in Unicode mode.
///
/// - `\b`, `\w`, `\W`, `\d`, `\D` become Unicode-aware forms.
/// - A positive class containing `\W` becomes an alternation, because
///   `regress` has no set operations.
/// - Identity escapes of ordinary punctuation (`\-`, `\_`, `\ `) lose the
///   backslash outside classes; `u` mode rejects them.
fn to_es_unicode(pattern: &str) -> String {
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::with_capacity(pattern.len() * 2);
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && i + 1 < chars.len() {
            let n = chars[i + 1];
            match n {
                'b' => out.push_str(&format!(
                    "(?:(?<=[{WORD}])(?![{WORD}])|(?<![{WORD}])(?=[{WORD}]))"
                )),
                'w' => out.push_str(&format!("[{WORD}]")),
                'W' => out.push_str(&format!("[^{WORD}]")),
                'd' => out.push_str(r"\p{Nd}"),
                'D' => out.push_str(r"\P{Nd}"),
                n if n.is_ascii_alphanumeric() || SYNTAX.contains(n) => {
                    out.push(c);
                    out.push(n);
                }
                n => out.push(n),
            }
            i += 2;
            continue;
        }
        if c == '[' {
            let (class, next) = translate_class(&chars, i);
            out.push_str(&class);
            i = next;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

/// Translates one character class starting at `chars[start] == '['`.
fn translate_class(chars: &[char], start: usize) -> (String, usize) {
    let mut i = start + 1;
    let negated = chars.get(i) == Some(&'^');
    if negated {
        i += 1;
    }
    let mut body = String::new();
    let mut has_non_word = false;
    while i < chars.len() && chars[i] != ']' {
        if chars[i] == '\\' && i + 1 < chars.len() {
            let n = chars[i + 1];
            match n {
                'w' => body.push_str(WORD),
                'd' => body.push_str(r"\p{Nd}"),
                'W' => has_non_word = true,
                n if n.is_ascii_alphanumeric() || SYNTAX.contains(n) || n == '-' => {
                    body.push('\\');
                    body.push(n);
                }
                n => body.push(n),
            }
            i += 2;
        } else {
            // A literal '[' inside a class must be escaped in `u` mode.
            if chars[i] == '[' {
                body.push('\\');
            }
            body.push(chars[i]);
            i += 1;
        }
    }
    let end = i + 1; // skip ']'
    let text = match (negated, has_non_word) {
        (false, false) => format!("[{body}]"),
        (true, false) => format!("[^{body}]"),
        (false, true) if body.is_empty() => format!("[^{WORD}]"),
        (false, true) => format!("(?:[{body}]|[^{WORD}])"),
        (true, true) => panic!("negated class containing \\W is not supported"),
    };
    (text, end)
}

enum Engine {
    Fancy(fancy_regex::Regex),
    Regress(regress::Regex),
}

pub struct Re {
    engine: Engine,
    names: Vec<String>,
}

/// One match with its named groups.
pub struct Caps<'t> {
    text: &'t str,
    whole: Range<usize>,
    groups: Vec<(String, Range<usize>)>,
}

impl<'t> Caps<'t> {
    pub fn whole(&self) -> Range<usize> {
        self.whole.clone()
    }

    pub fn whole_str(&self) -> &'t str {
        &self.text[self.whole.clone()]
    }

    pub fn span(&self, name: &str) -> Option<Range<usize>> {
        self.groups
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, r)| r.clone())
    }

    pub fn name(&self, name: &str) -> Option<&'t str> {
        self.span(name).map(|r| &self.text[r])
    }

    pub fn has(&self, name: &str) -> bool {
        self.span(name).is_some()
    }
}

/// Finds named groups and rewrites `(?<name>` to `(?P<name>` for fancy-regex.
fn scan_names(pattern: &str) -> (String, Vec<String>) {
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::with_capacity(pattern.len() + 8);
    let mut names = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && i + 1 < chars.len() {
            out.push(c);
            out.push(chars[i + 1]);
            i += 2;
            continue;
        }
        if c == '(' && chars.get(i + 1) == Some(&'?') {
            let mut j = i + 2;
            if chars.get(j) == Some(&'P') {
                j += 1;
            }
            if chars.get(j) == Some(&'<')
                && chars
                    .get(j + 1)
                    .is_some_and(|c| c.is_ascii_alphabetic() || *c == '_')
            {
                let start = j + 1;
                let mut k = start;
                while k < chars.len() && chars[k] != '>' {
                    k += 1;
                }
                let name: String = chars[start..k].iter().collect();
                names.push(name.clone());
                out.push_str(&format!("(?P<{name}>"));
                i = k + 1;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    (out, names)
}

impl Re {
    /// Case-sensitive.
    pub fn new(pattern: &str) -> Self {
        Self::build(pattern, false)
    }

    /// Case-insensitive (`RegexOptions.IgnoreCase`).
    pub fn ci(pattern: &str) -> Self {
        Self::build(pattern, true)
    }

    fn build(pattern: &str, ignore_case: bool) -> Self {
        let (fancy_pattern, names) = scan_names(pattern);
        let prefixed = if ignore_case {
            format!("(?i){fancy_pattern}")
        } else {
            fancy_pattern
        };
        let engine = match fancy_regex::Regex::new(&prefixed) {
            Ok(r) => Engine::Fancy(r),
            Err(fancy_err) => {
                let flags = if ignore_case { "iu" } else { "u" };
                match regress::Regex::with_flags(&to_es_unicode(pattern), flags) {
                    Ok(r) => Engine::Regress(r),
                    Err(e) => panic!(
                        "pattern failed in both engines: {pattern}\nfancy: {fancy_err}\nregress: {e}"
                    ),
                }
            }
        };
        Self { engine, names }
    }

    /// Which engine compiled the pattern (for tests and diagnostics).
    pub fn engine_name(&self) -> &'static str {
        match self.engine {
            Engine::Fancy(_) => "fancy-regex",
            Engine::Regress(_) => "regress",
        }
    }

    /// All non-overlapping matches, like .NET `Regex.Matches`.
    pub fn captures_all<'t>(&self, text: &'t str) -> Vec<Caps<'t>> {
        match &self.engine {
            Engine::Fancy(r) => r
                .captures_iter(text)
                .filter_map(Result::ok)
                .map(|c| {
                    let m0 = c.get(0).expect("group 0");
                    Caps {
                        text,
                        whole: m0.start()..m0.end(),
                        groups: self
                            .names
                            .iter()
                            .filter_map(|n| c.name(n).map(|m| (n.clone(), m.start()..m.end())))
                            .collect(),
                    }
                })
                .collect(),
            Engine::Regress(r) => r
                .find_iter(text)
                .map(|m| Caps {
                    text,
                    whole: m.range(),
                    groups: self
                        .names
                        .iter()
                        .filter_map(|n| m.named_group(n).map(|g| (n.clone(), g)))
                        .collect(),
                })
                .collect(),
        }
    }

    pub fn captures<'t>(&self, text: &'t str) -> Option<Caps<'t>> {
        self.captures_all(text).into_iter().next()
    }

    pub fn is_match(&self, text: &str) -> bool {
        match &self.engine {
            Engine::Fancy(r) => r.is_match(text).unwrap_or(false),
            Engine::Regress(r) => r.find(text).is_some(),
        }
    }

    /// Replaces every match with a fixed string (no group references).
    pub fn replace_all(&self, text: &str, replacement: &str) -> String {
        self.replace_with(text, |_| replacement.to_owned())
    }

    /// Replaces every match with the closure's result (`MatchEvaluator`).
    pub fn replace_with(&self, text: &str, mut f: impl FnMut(&Caps<'_>) -> String) -> String {
        let mut out = String::with_capacity(text.len());
        let mut last = 0;
        for c in self.captures_all(text) {
            let w = c.whole();
            out.push_str(&text[last..w.start]);
            out.push_str(&f(&c));
            last = w.end;
        }
        out.push_str(&text[last..]);
        out
    }

    /// Splits around matches, like .NET `Regex.Split` for patterns without
    /// capture groups.
    pub fn split<'t>(&self, text: &'t str) -> Vec<&'t str> {
        let mut parts = Vec::new();
        let mut last = 0;
        for c in self.captures_all(text) {
            let w = c.whole();
            parts.push(&text[last..w.start]);
            last = w.end;
        }
        parts.push(&text[last..]);
        parts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_groups_and_engines() {
        let r = Re::ci(r"(?<a>x+)-(?<b>y+)");
        assert_eq!(r.engine_name(), "fancy-regex");
        let c = r.captures("..XX-yy..").unwrap();
        assert_eq!(c.name("a"), Some("XX"));
        assert_eq!(c.name("b"), Some("yy"));
    }

    #[test]
    fn falls_back_for_variable_lookbehind() {
        let r = Re::new(r"(?<!a.*)b");
        assert_eq!(r.engine_name(), "regress");
        assert!(r.is_match("b"));
        assert!(!r.is_match("ab"));
    }

    #[test]
    fn regress_fallback_is_unicode_aware() {
        // Variable look-behind forces regress. \b must not split "Ossetië".
        let r = Re::ci(r"(?<!a.*)-(?<g>[a-z0-9]+)\b");
        assert_eq!(r.engine_name(), "regress");
        assert!(r.captures("Zuid-Ossetië").is_none());
        assert!(r.captures("x-abc ").is_some());
        // [-_\W] with a Cyrillic char: it is non-ASCII but is a word char in .NET.
        let r = Re::ci(r"(?<!a.*)(?<t>.+?)[-_\W]+(?<y>\d{4})");
        let c = r.captures("Енола 2020").unwrap();
        assert_eq!(c.name("t"), Some("Енола"));
    }

    #[test]
    fn replace_and_split() {
        let r = Re::new(r"[ ]+(?:AKA|/)[ ]+");
        assert_eq!(r.split("A AKA B / C"), vec!["A", "B", "C"]);
        assert_eq!(Re::new("o").replace_all("foo", "0"), "f00");
    }
}
