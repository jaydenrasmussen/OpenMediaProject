//! CSS selectors over [`Dom`]: what the definitions use, including the
//! jQuery-style `:contains()` that browsers do not have.

use crate::dom::{Dom, Id};

#[derive(Debug, Clone, PartialEq)]
pub struct SelectorList(Vec<Complex>);

#[derive(Debug, Clone, PartialEq)]
struct Complex {
    first: Compound,
    rest: Vec<(Combinator, Compound)>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Combinator {
    Descendant,
    Child,
    Next,
    Later,
}

#[derive(Debug, Clone, PartialEq, Default)]
struct Compound {
    tag: Option<String>,
    simples: Vec<Simple>,
}

#[derive(Debug, Clone, PartialEq)]
enum Simple {
    Id(String),
    Class(String),
    Attr {
        name: String,
        op: AttrOp,
        value: String,
        insensitive: bool,
    },
    Pseudo(Pseudo),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum AttrOp {
    Exists,
    Equals,
    Includes,
    Dash,
    Prefix,
    Suffix,
    Substring,
    NotEquals,
}

#[derive(Debug, Clone, PartialEq)]
enum Pseudo {
    FirstChild,
    LastChild,
    OnlyChild,
    FirstOfType,
    LastOfType,
    OnlyOfType,
    Nth {
        a: i64,
        b: i64,
        from_end: bool,
        of_type: bool,
    },
    Not(SelectorList),
    Is(SelectorList),
    Has(Vec<Complex>, Vec<Combinator>),
    Contains(String),
    Empty,
    Root,
    Checked,
    Disabled,
    Enabled,
    Scope,
}

const NO_SCOPE: Id = usize::MAX;

struct Parser<'a> {
    chars: Vec<char>,
    at: usize,
    source: &'a str,
}

type Parsed<T> = Result<T, String>;

impl SelectorList {
    pub fn parse(source: &str) -> Parsed<Self> {
        let mut p = Parser {
            chars: source.chars().collect(),
            at: 0,
            source,
        };
        let list = p.list(false)?;
        p.skip_ws();
        if p.at < p.chars.len() {
            return Err(format!(
                "unexpected '{}' in selector \"{source}\"",
                p.chars[p.at]
            ));
        }
        Ok(list)
    }

    /// Every element under `scope`, in document order, that matches.
    pub fn select_all(&self, dom: &Dom, scope: Id) -> Vec<Id> {
        dom.descendants(scope)
            .into_iter()
            .filter(|e| self.matches_in(dom, *e, scope))
            .collect()
    }

    pub fn select_first(&self, dom: &Dom, scope: Id) -> Option<Id> {
        dom.descendants(scope)
            .into_iter()
            .find(|e| self.matches_in(dom, *e, scope))
    }

    pub fn matches(&self, dom: &Dom, el: Id) -> bool {
        self.matches_in(dom, el, NO_SCOPE)
    }

    /// As [`Self::matches`], where `:scope` is the element the search began at.
    pub fn matches_in(&self, dom: &Dom, el: Id, scope: Id) -> bool {
        self.0.iter().any(|c| complex_matches(dom, el, c, scope))
    }
}

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.at).copied()
    }

    fn skip_ws(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.at += 1;
        }
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn ident(&mut self) -> Parsed<String> {
        let mut out = String::new();
        while let Some(c) = self.peek() {
            if c == '\\' {
                self.at += 1;
                if let Some(n) = self.peek() {
                    out.push(n);
                    self.at += 1;
                }
            } else if c.is_alphanumeric() || c == '-' || c == '_' || !c.is_ascii() {
                out.push(c);
                self.at += 1;
            } else {
                break;
            }
        }
        if out.is_empty() {
            Err(format!(
                "expected a name at position {} of \"{}\"",
                self.at, self.source
            ))
        } else {
            Ok(out)
        }
    }

    fn list(&mut self, in_parens: bool) -> Parsed<SelectorList> {
        let mut out = vec![];
        loop {
            self.skip_ws();
            out.push(self.complex(in_parens)?);
            self.skip_ws();
            if !self.eat(',') {
                break;
            }
        }
        Ok(SelectorList(out))
    }

    fn complex(&mut self, in_parens: bool) -> Parsed<Complex> {
        let first = self.compound()?;
        let mut rest = vec![];
        loop {
            let had_space = self.peek().is_some_and(char::is_whitespace);
            self.skip_ws();
            let comb = match self.peek() {
                Some('>') => {
                    self.at += 1;
                    Combinator::Child
                }
                Some('+') => {
                    self.at += 1;
                    Combinator::Next
                }
                Some('~') => {
                    self.at += 1;
                    Combinator::Later
                }
                Some(',') | None => break,
                Some(')') if in_parens => break,
                Some(_) if had_space => Combinator::Descendant,
                // `a:not(x)a:not(y)` is read as the second part being inside the first.
                Some(c) if c.is_alphabetic() || c == '*' => Combinator::Descendant,
                Some(_) => break,
            };
            self.skip_ws();
            rest.push((comb, self.compound()?));
        }
        Ok(Complex { first, rest })
    }

    fn compound(&mut self) -> Parsed<Compound> {
        let mut c = Compound::default();
        let mut universal = false;
        if self.eat('*') {
            universal = true;
            if self.eat('|') && !self.eat('*') {
                c.tag = Some(self.ident()?.to_ascii_lowercase());
            }
        } else if self.peek().is_some_and(|ch| {
            ch.is_alphanumeric() || ch == '-' || ch == '_' || ch == '\\' || !ch.is_ascii()
        }) {
            let mut name = self.ident()?;
            // `ns|tag`: the namespace plays no part.
            if self.peek() == Some('|') && self.chars.get(self.at + 1) != Some(&'=') {
                self.at += 1;
                name = self.ident()?;
            }
            c.tag = Some(name.to_ascii_lowercase());
        }
        loop {
            match self.peek() {
                Some('#') => {
                    self.at += 1;
                    c.simples.push(Simple::Id(self.ident()?));
                }
                Some('.') => {
                    self.at += 1;
                    c.simples.push(Simple::Class(self.ident()?));
                }
                Some('[') => {
                    self.at += 1;
                    c.simples.push(self.attribute()?);
                }
                Some(':') => {
                    self.at += 1;
                    self.eat(':');
                    c.simples.push(Simple::Pseudo(self.pseudo()?));
                }
                _ => break,
            }
        }
        if c.tag.is_none() && c.simples.is_empty() && !universal {
            return Err(format!(
                "expected a selector at position {} of \"{}\"",
                self.at, self.source
            ));
        }
        Ok(c)
    }

    fn string_or_ident(&mut self) -> Parsed<String> {
        self.skip_ws();
        match self.peek() {
            Some(q @ ('"' | '\'')) => {
                self.at += 1;
                let mut out = String::new();
                while let Some(c) = self.peek() {
                    self.at += 1;
                    if c == q {
                        return Ok(out);
                    }
                    if c == '\\' {
                        if let Some(n) = self.peek() {
                            out.push(n);
                            self.at += 1;
                        }
                    } else {
                        out.push(c);
                    }
                }
                Err(format!("unterminated string in \"{}\"", self.source))
            }
            _ => {
                let mut out = String::new();
                while let Some(c) = self.peek() {
                    if c == ']' || c == ')' || c.is_whitespace() {
                        break;
                    }
                    out.push(c);
                    self.at += 1;
                }
                Ok(out)
            }
        }
    }

    /// The text of `:contains(...)`: quoted, or everything up to the `)`.
    fn contains_arg(&mut self) -> Parsed<String> {
        self.skip_ws();
        if matches!(self.peek(), Some('"' | '\'')) {
            return self.string_or_ident();
        }
        let mut out = String::new();
        while let Some(c) = self.peek() {
            if c == ')' {
                break;
            }
            out.push(c);
            self.at += 1;
        }
        Ok(out.trim().to_owned())
    }

    fn attribute(&mut self) -> Parsed<Simple> {
        self.skip_ws();
        let mut name = self.ident()?;
        if self.peek() == Some('|') && self.chars.get(self.at + 1) != Some(&'=') {
            self.at += 1;
            name = self.ident()?;
        }
        self.skip_ws();
        let op = match self.peek() {
            Some(']') => {
                self.at += 1;
                return Ok(Simple::Attr {
                    name: name.to_ascii_lowercase(),
                    op: AttrOp::Exists,
                    value: String::new(),
                    insensitive: false,
                });
            }
            Some('=') => {
                self.at += 1;
                AttrOp::Equals
            }
            Some(c @ ('~' | '|' | '^' | '$' | '*' | '!')) => {
                self.at += 1;
                if !self.eat('=') {
                    return Err(format!("expected '=' after '{c}' in \"{}\"", self.source));
                }
                match c {
                    '~' => AttrOp::Includes,
                    '|' => AttrOp::Dash,
                    '^' => AttrOp::Prefix,
                    '$' => AttrOp::Suffix,
                    '*' => AttrOp::Substring,
                    _ => AttrOp::NotEquals,
                }
            }
            _ => return Err(format!("bad attribute selector in \"{}\"", self.source)),
        };
        let value = self.string_or_ident()?;
        self.skip_ws();
        let insensitive = matches!(self.peek(), Some('i' | 'I'));
        if insensitive || matches!(self.peek(), Some('s' | 'S')) {
            self.at += 1;
            self.skip_ws();
        }
        if !self.eat(']') {
            return Err(format!("expected ']' in \"{}\"", self.source));
        }
        Ok(Simple::Attr {
            name: name.to_ascii_lowercase(),
            op,
            value,
            insensitive,
        })
    }

    fn pseudo(&mut self) -> Parsed<Pseudo> {
        let name = self.ident()?.to_ascii_lowercase();
        let arg = self.eat('(');
        let p = match (name.as_str(), arg) {
            ("first-child", false) => Pseudo::FirstChild,
            ("last-child", false) => Pseudo::LastChild,
            ("only-child", false) => Pseudo::OnlyChild,
            ("first-of-type", false) => Pseudo::FirstOfType,
            ("last-of-type", false) => Pseudo::LastOfType,
            ("only-of-type", false) => Pseudo::OnlyOfType,
            ("empty", false) => Pseudo::Empty,
            ("root", false) => Pseudo::Root,
            ("checked", false) => Pseudo::Checked,
            ("disabled", false) => Pseudo::Disabled,
            ("enabled", false) => Pseudo::Enabled,
            ("scope", false) => Pseudo::Scope,
            ("nth-child" | "nth-last-child" | "nth-of-type" | "nth-last-of-type", true) => {
                let (a, b) = self.nth()?;
                Pseudo::Nth {
                    a,
                    b,
                    from_end: name.contains("last"),
                    of_type: name.ends_with("of-type"),
                }
            }
            ("not", true) => Pseudo::Not(self.list(true)?),
            ("is" | "where" | "matches" | "any", true) => Pseudo::Is(self.list(true)?),
            ("has", true) => {
                let mut complexes = vec![];
                let mut combs = vec![];
                loop {
                    self.skip_ws();
                    let comb = match self.peek() {
                        Some('>') => {
                            self.at += 1;
                            Combinator::Child
                        }
                        Some('+') => {
                            self.at += 1;
                            Combinator::Next
                        }
                        Some('~') => {
                            self.at += 1;
                            Combinator::Later
                        }
                        _ => Combinator::Descendant,
                    };
                    self.skip_ws();
                    complexes.push(self.complex(true)?);
                    combs.push(comb);
                    self.skip_ws();
                    if !self.eat(',') {
                        break;
                    }
                }
                Pseudo::Has(complexes, combs)
            }
            ("contains" | "-soup-contains", true) => Pseudo::Contains(self.contains_arg()?),
            (other, _) => {
                return Err(format!(
                    "unsupported selector :{other} in \"{}\"",
                    self.source
                ));
            }
        };
        if arg {
            self.skip_ws();
            if !self.eat(')') {
                return Err(format!("expected ')' in \"{}\"", self.source));
            }
        }
        Ok(p)
    }

    /// `an+b`, `odd`, `even`, or a plain number.
    fn nth(&mut self) -> Parsed<(i64, i64)> {
        self.skip_ws();
        let mut text = String::new();
        while let Some(c) = self.peek() {
            if c == ')' {
                break;
            }
            text.push(c);
            self.at += 1;
        }
        let t: String = text
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            .to_ascii_lowercase();
        match t.as_str() {
            "odd" => return Ok((2, 1)),
            "even" => return Ok((2, 0)),
            _ => {}
        }
        let num = |s: &str| -> Parsed<i64> {
            match s {
                "" | "+" => Ok(1),
                "-" => Ok(-1),
                s => s
                    .parse::<i64>()
                    .map_err(|_| format!("bad number '{s}' in \"{}\"", self.source)),
            }
        };
        if let Some(n) = t.find('n') {
            let a = num(&t[..n])?;
            let rest = &t[n + 1..];
            let b = if rest.is_empty() { 0 } else { num(rest)? };
            Ok((a, b))
        } else {
            Ok((0, num(&t)?))
        }
    }
}

fn complex_matches(dom: &Dom, el: Id, c: &Complex, scope: Id) -> bool {
    let parts: Vec<(&Compound, Option<Combinator>)> = {
        let mut v = vec![(&c.first, None)];
        v.extend(c.rest.iter().map(|(comb, comp)| (comp, Some(*comb))));
        v
    };
    matches_from(dom, el, &parts, parts.len() - 1, scope)
}

/// Matches `parts[at]` against `el`, then the parts to its left against `el`'s surroundings.
fn matches_from(
    dom: &Dom,
    el: Id,
    parts: &[(&Compound, Option<Combinator>)],
    at: usize,
    scope: Id,
) -> bool {
    let (compound, comb) = parts[at];
    if !compound_matches(dom, el, compound, scope) {
        return false;
    }
    let Some(comb) = comb else { return true };
    match comb {
        Combinator::Child => dom
            .parent_element(el)
            .is_some_and(|p| matches_from(dom, p, parts, at - 1, scope)),
        Combinator::Descendant => {
            let mut cur = dom.parent_element(el);
            while let Some(p) = cur {
                if matches_from(dom, p, parts, at - 1, scope) {
                    return true;
                }
                cur = dom.parent_element(p);
            }
            false
        }
        Combinator::Next => dom
            .prev_element_sibling(el)
            .is_some_and(|p| matches_from(dom, p, parts, at - 1, scope)),
        Combinator::Later => {
            let mut cur = dom.prev_element_sibling(el);
            while let Some(p) = cur {
                if matches_from(dom, p, parts, at - 1, scope) {
                    return true;
                }
                cur = dom.prev_element_sibling(p);
            }
            false
        }
    }
}

fn compound_matches(dom: &Dom, el: Id, c: &Compound, scope: Id) -> bool {
    if let Some(tag) = &c.tag
        && dom.name(el) != Some(tag.as_str())
    {
        return false;
    }
    c.simples.iter().all(|s| simple_matches(dom, el, s, scope))
}

fn simple_matches(dom: &Dom, el: Id, s: &Simple, scope: Id) -> bool {
    match s {
        Simple::Id(id) => dom.attr(el, "id") == Some(id.as_str()),
        Simple::Class(class) => dom
            .attr(el, "class")
            .is_some_and(|v| v.split_whitespace().any(|c| c == class)),
        Simple::Attr {
            name,
            op,
            value,
            insensitive,
        } => {
            let actual = dom.attr(el, name);
            let Some(actual) = actual else {
                return *op == AttrOp::NotEquals;
            };
            let (a, v) = if *insensitive {
                (actual.to_lowercase(), value.to_lowercase())
            } else {
                (actual.to_owned(), value.clone())
            };
            match op {
                AttrOp::Exists => true,
                AttrOp::Equals => a == v,
                AttrOp::NotEquals => a != v,
                AttrOp::Includes => a.split_whitespace().any(|w| w == v),
                AttrOp::Dash => a == v || a.starts_with(&format!("{v}-")),
                AttrOp::Prefix => !v.is_empty() && a.starts_with(&v),
                AttrOp::Suffix => !v.is_empty() && a.ends_with(&v),
                AttrOp::Substring => !v.is_empty() && a.contains(&v),
            }
        }
        Simple::Pseudo(p) => pseudo_matches(dom, el, p, scope),
    }
}

/// The position of `el` among its siblings, counted from 1.
fn position(dom: &Dom, el: Id, from_end: bool, of_type: bool) -> Option<i64> {
    let parent = dom.parent(el)?;
    let name = dom.name(el)?;
    let siblings: Vec<Id> = dom
        .element_children(parent)
        .filter(|s| !of_type || dom.name(*s) == Some(name))
        .collect();
    let idx = siblings.iter().position(|s| *s == el)? as i64;
    Some(if from_end {
        siblings.len() as i64 - idx
    } else {
        idx + 1
    })
}

fn pseudo_matches(dom: &Dom, el: Id, p: &Pseudo, scope: Id) -> bool {
    match p {
        Pseudo::FirstChild => position(dom, el, false, false) == Some(1),
        Pseudo::LastChild => position(dom, el, true, false) == Some(1),
        Pseudo::OnlyChild => {
            position(dom, el, false, false) == Some(1) && position(dom, el, true, false) == Some(1)
        }
        Pseudo::FirstOfType => position(dom, el, false, true) == Some(1),
        Pseudo::LastOfType => position(dom, el, true, true) == Some(1),
        Pseudo::OnlyOfType => {
            position(dom, el, false, true) == Some(1) && position(dom, el, true, true) == Some(1)
        }
        Pseudo::Nth {
            a,
            b,
            from_end,
            of_type,
        } => {
            let Some(pos) = position(dom, el, *from_end, *of_type) else {
                return false;
            };
            if *a == 0 {
                pos == *b
            } else {
                let diff = pos - b;
                diff % a == 0 && diff / a >= 0
            }
        }
        Pseudo::Not(list) => !list.matches_in(dom, el, scope),
        Pseudo::Is(list) => list.matches_in(dom, el, scope),
        Pseudo::Scope => el == scope,
        Pseudo::Has(complexes, combs) => complexes
            .iter()
            .zip(combs)
            .any(|(c, comb)| has_match(dom, el, c, *comb, scope)),
        Pseudo::Contains(text) => dom.text_content(el).contains(text.as_str()),
        Pseudo::Empty => dom.children(el).iter().all(|c| {
            matches!(dom.kind(*c), crate::dom::Kind::Comment)
                || matches!(dom.kind(*c), crate::dom::Kind::Text(t) if t.is_empty())
        }),
        Pseudo::Root => dom.parent(el) == Some(dom.root()),
        Pseudo::Checked => dom.attr(el, "checked").is_some() || dom.attr(el, "selected").is_some(),
        Pseudo::Disabled => dom.attr(el, "disabled").is_some(),
        Pseudo::Enabled => dom.attr(el, "disabled").is_none(),
    }
}

/// `:has()`: starting from `scope`, step by the leading combinator and then
/// through the rest of the chain; true when something is reached.
fn has_match(dom: &Dom, scope: Id, c: &Complex, lead: Combinator, outer: Id) -> bool {
    let step = |from: &[Id], comb: Combinator| -> Vec<Id> {
        let mut out = vec![];
        for n in from {
            match comb {
                Combinator::Descendant => out.extend(dom.descendants(*n)),
                Combinator::Child => out.extend(dom.element_children(*n)),
                Combinator::Next => {
                    if let Some(p) = dom.parent(*n) {
                        let sibs: Vec<Id> = dom.element_children(p).collect();
                        if let Some(i) = sibs.iter().position(|s| s == n)
                            && let Some(next) = sibs.get(i + 1)
                        {
                            out.push(*next);
                        }
                    }
                }
                Combinator::Later => {
                    if let Some(p) = dom.parent(*n) {
                        let sibs: Vec<Id> = dom.element_children(p).collect();
                        if let Some(i) = sibs.iter().position(|s| s == n) {
                            out.extend(sibs[i + 1..].iter().copied());
                        }
                    }
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    };
    let mut set: Vec<Id> = step(&[scope], lead)
        .into_iter()
        .filter(|n| compound_matches(dom, *n, &c.first, outer))
        .collect();
    for (comb, compound) in &c.rest {
        set = step(&set, *comb)
            .into_iter()
            .filter(|n| compound_matches(dom, *n, compound, outer))
            .collect();
    }
    !set.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count(html: &str, selector: &str) -> usize {
        let d = Dom::parse_html(html);
        SelectorList::parse(selector)
            .unwrap()
            .select_all(&d, 0)
            .len()
    }

    const TABLE: &str = "<table><tr class='odd'><td><a href='/t/1'>One</a></td><td>5</td></tr>\
        <tr><td><a href='/t/2'>Two</a></td><td>7</td></tr><tr class='odd'><td>Three</td><td>9</td></tr></table>";

    #[test]
    fn tags_classes_attributes_and_combinators() {
        assert_eq!(count(TABLE, "tr"), 3);
        assert_eq!(count(TABLE, "tr.odd"), 2);
        assert_eq!(count(TABLE, "table > tr"), 0); // the HTML parser puts a tbody between, as a browser does
        assert_eq!(count(TABLE, "table > tbody > tr"), 3);
        assert_eq!(count(TABLE, "tr > td > a[href^='/t/']"), 2);
        assert_eq!(count(TABLE, "a[href$='2']"), 1);
        assert_eq!(count(TABLE, "td + td"), 3);
        assert_eq!(count(TABLE, "tr, a"), 5);
    }

    #[test]
    fn positions() {
        assert_eq!(count(TABLE, "tr:nth-child(2)"), 1);
        assert_eq!(count(TABLE, "tr:nth-child(odd)"), 2);
        assert_eq!(count(TABLE, "tr:nth-child(2n+1)"), 2);
        assert_eq!(count(TABLE, "tr:nth-last-child(1)"), 1);
        assert_eq!(count(TABLE, "td:first-child"), 3);
        assert_eq!(count(TABLE, "td:nth-child(-n+1)"), 3);
        assert_eq!(count(TABLE, "tr:nth-of-type(3)"), 1);
    }

    #[test]
    fn contains_not_and_has() {
        assert_eq!(count(TABLE, "tr:contains('Two')"), 1);
        assert_eq!(count(TABLE, "td:contains(Three)"), 1);
        assert_eq!(count(TABLE, "tr:has(a)"), 2);
        assert_eq!(count(TABLE, "tr:not(:has(a))"), 1);
        assert_eq!(count(TABLE, "tr:has(> td > a[href='/t/1'])"), 1);
        assert_eq!(count(TABLE, "tr:not(.odd)"), 1);
        assert_eq!(count(TABLE, "tr.odd:not(:has(a)):contains(Three)"), 1);
    }

    #[test]
    fn bad_selectors_are_errors_not_panics() {
        assert!(SelectorList::parse("a[").is_err());
        assert!(SelectorList::parse(":bogus").is_err());
        assert!(SelectorList::parse("").is_err());
    }
}
