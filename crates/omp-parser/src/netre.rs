//! A small backtracking regex engine with .NET semantics.
//!
//! Sonarr's title parser leans on a .NET feature the usual Rust engines do not
//! have: a group that sits in a loop remembers *every* iteration
//! (`Group.Captures`), and the same group name may appear several times in one
//! pattern. This engine keeps that capture history, so the patterns can be
//! used almost verbatim.
//!
//! Supported syntax (what the *arr patterns use): literals, `.`, classes with
//! ranges and `\d \w \s \D \W \S`, `^ $ \b \B`, groups (capturing, named,
//! non-capturing), alternation, greedy and lazy quantifiers (`* + ? {n} {n,}
//! {n,m}`), look-ahead and look-behind (positive and negative, any length),
//! back-references (`\1`, `\k<name>`), `\p{..}` for a few common categories,
//! and a leading `(?i)`. Matching is leftmost, first-alternative-wins, like
//! .NET.
//!
//! A step budget stops runaway backtracking; hitting it means "no match".

use std::ops::Range;

/// Steps one search may take before giving up.
const STEP_LIMIT: u64 = 2_000_000;

#[derive(Debug, Clone)]
enum ClassItem {
    Range(char, char),
    Digit(bool),
    Word(bool),
    Space(bool),
    Prop(Prop, bool),
}

#[derive(Debug, Clone, Copy)]
enum Prop {
    Letter,
    Upper,
    Lower,
    Number,
    Punct,
}

#[derive(Debug, Clone)]
struct Class {
    negated: bool,
    items: Vec<ClassItem>,
}

#[derive(Debug, Clone)]
enum Node {
    Empty,
    Char(char),
    Any,
    Class(Class),
    Bol,
    Eol,
    WordBoundary(bool),
    Seq(Vec<Node>),
    Alt(Vec<Node>),
    Cap(usize, Box<Node>),
    Rep {
        node: Box<Node>,
        min: u32,
        max: u32,
        greedy: bool,
    },
    Look {
        behind: bool,
        negative: bool,
        node: Box<Node>,
    },
    /// `(?>...)`: matches like the group would on its own and never gives back.
    Atomic(Box<Node>),
    Backref(usize),
    BackrefName(String),
}

pub struct Regex {
    root: Node,
    groups: usize,
    names: Vec<(String, usize)>,
    ignore_case: bool,
}

// ---- parsing ------------------------------------------------------------

struct Parser {
    chars: Vec<char>,
    pos: usize,
    groups: usize,
    names: Vec<(String, usize)>,
    ignore_case: bool,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn starts_with(&self, s: &str) -> bool {
        s.chars()
            .enumerate()
            .all(|(i, c)| self.chars.get(self.pos + i) == Some(&c))
    }

    fn alternation(&mut self) -> Result<Node, String> {
        let mut branches = vec![self.sequence()?];
        while self.eat('|') {
            branches.push(self.sequence()?);
        }
        Ok(if branches.len() == 1 {
            branches.pop().unwrap()
        } else {
            Node::Alt(branches)
        })
    }

    fn sequence(&mut self) -> Result<Node, String> {
        let mut items = Vec::new();
        while let Some(c) = self.peek() {
            if c == '|' || c == ')' {
                break;
            }
            let atom = self.atom()?;
            items.push(self.quantified(atom)?);
        }
        Ok(match items.len() {
            0 => Node::Empty,
            1 => items.pop().unwrap(),
            _ => Node::Seq(items),
        })
    }

    fn number(&mut self) -> Option<u32> {
        let start = self.pos;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.pos += 1;
        }
        (self.pos > start).then(|| {
            self.chars[start..self.pos]
                .iter()
                .collect::<String>()
                .parse()
                .unwrap_or(u32::MAX)
        })
    }

    /// `{n}`, `{n,}` or `{n,m}` at the cursor, if that is what is there.
    fn braces(&mut self) -> Option<(u32, u32)> {
        let save = self.pos;
        if !self.eat('{') {
            return None;
        }
        let parsed = (|| {
            let min = self.number()?;
            let max = if self.eat(',') {
                self.number().unwrap_or(u32::MAX)
            } else {
                min
            };
            self.eat('}').then_some((min, max))
        })();
        if parsed.is_none() {
            self.pos = save;
        }
        parsed
    }

    fn quantified(&mut self, atom: Node) -> Result<Node, String> {
        let mut node = atom;
        loop {
            let (min, max) = match self.peek() {
                Some('*') => {
                    self.pos += 1;
                    (0, u32::MAX)
                }
                Some('+') => {
                    self.pos += 1;
                    (1, u32::MAX)
                }
                Some('?') => {
                    self.pos += 1;
                    (0, 1)
                }
                Some('{') => match self.braces() {
                    Some(r) => r,
                    None => return Ok(node),
                },
                _ => return Ok(node),
            };
            if max < min {
                return Err("quantifier range is backwards".into());
            }
            let greedy = !self.eat('?');
            node = Node::Rep {
                node: Box::new(node),
                min,
                max,
                greedy,
            };
        }
    }

    fn name_until(&mut self, close: char) -> Result<String, String> {
        let start = self.pos;
        while self.peek().is_some_and(|c| c != close) {
            self.pos += 1;
        }
        if self.peek() != Some(close) {
            return Err("unterminated group name".into());
        }
        let name: String = self.chars[start..self.pos].iter().collect();
        self.pos += 1;
        Ok(name)
    }

    fn group_index(&mut self, name: Option<String>) -> usize {
        if let Some(name) = &name
            && let Some((_, idx)) = self.names.iter().find(|(n, _)| n == name)
        {
            return *idx;
        }
        self.groups += 1;
        let idx = self.groups;
        if let Some(name) = name {
            self.names.push((name, idx));
        }
        idx
    }

    fn close(&mut self) -> Result<(), String> {
        if self.eat(')') {
            Ok(())
        } else {
            Err("missing )".into())
        }
    }

    fn atom(&mut self) -> Result<Node, String> {
        let c = self.peek().ok_or("unexpected end")?;
        self.pos += 1;
        Ok(match c {
            '.' => Node::Any,
            '^' => Node::Bol,
            '$' => Node::Eol,
            '[' => Node::Class(self.class()?),
            '(' => self.group()?,
            '\\' => self.escape()?,
            '*' | '+' | '?' => return Err(format!("nothing to repeat before '{c}'")),
            c => Node::Char(c),
        })
    }

    fn group(&mut self) -> Result<Node, String> {
        if self.eat('?') {
            if self.eat(':') {
                let inner = self.alternation()?;
                self.close()?;
                return Ok(inner);
            }
            if self.eat('=') {
                return self.look(false, false);
            }
            if self.eat('>') {
                let inner = self.alternation()?;
                self.close()?;
                return Ok(Node::Atomic(Box::new(inner)));
            }
            if self.eat('!') {
                return self.look(false, true);
            }
            if self.starts_with("<=") {
                self.pos += 2;
                return self.look(true, false);
            }
            if self.starts_with("<!") {
                self.pos += 2;
                return self.look(true, true);
            }
            if self.eat('<') || self.eat('\'') {
                let close = if self.chars[self.pos - 1] == '<' {
                    '>'
                } else {
                    '\''
                };
                let name = self.name_until(close)?;
                let idx = self.group_index(Some(name));
                let inner = self.alternation()?;
                self.close()?;
                return Ok(Node::Cap(idx, Box::new(inner)));
            }
            if self.eat('i') && self.eat(')') {
                self.ignore_case = true;
                return Ok(Node::Empty);
            }
            return Err("unsupported group syntax".into());
        }
        let idx = self.group_index(None);
        let inner = self.alternation()?;
        self.close()?;
        Ok(Node::Cap(idx, Box::new(inner)))
    }

    fn look(&mut self, behind: bool, negative: bool) -> Result<Node, String> {
        let inner = self.alternation()?;
        self.close()?;
        Ok(Node::Look {
            behind,
            negative,
            node: Box::new(inner),
        })
    }

    fn prop(&mut self) -> Result<Prop, String> {
        if !self.eat('{') {
            return Err("expected { after \\p".into());
        }
        let name = self.name_until('}')?;
        Ok(match name.as_str() {
            "L" | "Letter" => Prop::Letter,
            "Lu" => Prop::Upper,
            "Ll" => Prop::Lower,
            "N" | "Nd" => Prop::Number,
            "P" => Prop::Punct,
            other => return Err(format!("unsupported property {other}")),
        })
    }

    fn escape(&mut self) -> Result<Node, String> {
        let c = self.peek().ok_or("dangling backslash")?;
        self.pos += 1;
        let class = |item| {
            Node::Class(Class {
                negated: false,
                items: vec![item],
            })
        };
        Ok(match c {
            'd' => class(ClassItem::Digit(true)),
            'D' => class(ClassItem::Digit(false)),
            'w' => class(ClassItem::Word(true)),
            'W' => class(ClassItem::Word(false)),
            's' => class(ClassItem::Space(true)),
            'S' => class(ClassItem::Space(false)),
            'b' => Node::WordBoundary(true),
            'B' => Node::WordBoundary(false),
            'p' => class(ClassItem::Prop(self.prop()?, true)),
            'P' => class(ClassItem::Prop(self.prop()?, false)),
            'k' => {
                let close = match self.peek() {
                    Some('<') => '>',
                    Some('\'') => '\'',
                    _ => return Err("expected < after \\k".into()),
                };
                self.pos += 1;
                Node::BackrefName(self.name_until(close)?)
            }
            '1'..='9' => {
                self.pos -= 1;
                Node::Backref(self.number().unwrap_or(0) as usize)
            }
            other => Node::Char(self.escaped_char(other)?),
        })
    }

    /// The character an escape such as `\n`, `\.`, `\u4E00` or `\x41` stands
    /// for. `c` is the character after the backslash.
    fn escaped_char(&mut self, c: char) -> Result<char, String> {
        let digits = match c {
            'u' => 4,
            'x' => 2,
            other => return Ok(simple_escape(other)),
        };
        let hex: String = self.chars[self.pos..].iter().take(digits).collect();
        if hex.len() != digits {
            return Err("incomplete \\u or \\x escape".into());
        }
        self.pos += digits;
        u32::from_str_radix(&hex, 16)
            .ok()
            .and_then(char::from_u32)
            .ok_or_else(|| "bad \\u or \\x escape".to_owned())
    }

    fn class(&mut self) -> Result<Class, String> {
        let negated = self.eat('^');
        let mut items = Vec::new();
        let mut first = true;
        loop {
            let c = self.peek().ok_or("unterminated character class")?;
            self.pos += 1;
            if c == ']' && !first {
                break;
            }
            first = false;
            let lo = if c == '\\' {
                let e = self.peek().ok_or("dangling backslash")?;
                self.pos += 1;
                match e {
                    'd' => {
                        items.push(ClassItem::Digit(true));
                        continue;
                    }
                    'D' => {
                        items.push(ClassItem::Digit(false));
                        continue;
                    }
                    'w' => {
                        items.push(ClassItem::Word(true));
                        continue;
                    }
                    'W' => {
                        items.push(ClassItem::Word(false));
                        continue;
                    }
                    's' => {
                        items.push(ClassItem::Space(true));
                        continue;
                    }
                    'S' => {
                        items.push(ClassItem::Space(false));
                        continue;
                    }
                    'p' => {
                        items.push(ClassItem::Prop(self.prop()?, true));
                        continue;
                    }
                    'P' => {
                        items.push(ClassItem::Prop(self.prop()?, false));
                        continue;
                    }
                    'b' => '\u{8}',
                    other => self.escaped_char(other)?,
                }
            } else {
                c
            };
            // A range, unless the dash is the last thing in the class.
            if self.peek() == Some('-') && self.chars.get(self.pos + 1).is_some_and(|n| *n != ']') {
                self.pos += 1;
                let mut hi = self.peek().ok_or("unterminated range")?;
                self.pos += 1;
                if hi == '\\' {
                    let e = self.peek().ok_or("dangling backslash")?;
                    self.pos += 1;
                    hi = self.escaped_char(e)?;
                }
                if hi < lo {
                    return Err("character range is backwards".into());
                }
                items.push(ClassItem::Range(lo, hi));
            } else {
                items.push(ClassItem::Range(lo, lo));
            }
        }
        Ok(Class { negated, items })
    }
}

fn simple_escape(c: char) -> char {
    match c {
        'n' => '\n',
        'r' => '\r',
        't' => '\t',
        'f' => '\u{c}',
        'v' => '\u{b}',
        '0' => '\0',
        other => other,
    }
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || is_connector(c)
}

fn is_connector(c: char) -> bool {
    matches!(
        c,
        '\u{203f}' | '\u{2040}' | '\u{2054}' | '\u{fe33}' | '\u{fe34}' | '\u{fe4d}'
            ..='\u{fe4f}' | '\u{ff3f}'
    )
}

impl Prop {
    fn test(self, c: char) -> bool {
        match self {
            Prop::Letter => c.is_alphabetic(),
            Prop::Upper => c.is_uppercase(),
            Prop::Lower => c.is_lowercase(),
            Prop::Number => c.is_numeric(),
            Prop::Punct => {
                c.is_ascii_punctuation()
                    || (!c.is_alphanumeric()
                        && !c.is_whitespace()
                        && !c.is_control()
                        && !c.is_ascii())
            }
        }
    }
}

fn lower(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

fn upper(c: char) -> char {
    c.to_uppercase().next().unwrap_or(c)
}

impl ClassItem {
    fn test(&self, c: char) -> bool {
        match self {
            ClassItem::Range(a, b) => *a <= c && c <= *b,
            ClassItem::Digit(yes) => is_decimal(c) == *yes,
            ClassItem::Word(yes) => is_word(c) == *yes,
            ClassItem::Space(yes) => c.is_whitespace() == *yes,
            ClassItem::Prop(p, yes) => p.test(c) == *yes,
        }
    }
}

/// Unicode decimal digits (category Nd), which is what `\d` means in .NET.
fn is_decimal(c: char) -> bool {
    c.is_ascii_digit()
        || matches!(c, '\u{660}'..='\u{669}' | '\u{6f0}'..='\u{6f9}' | '\u{966}'..='\u{96f}' | '\u{ff10}'..='\u{ff19}')
}

impl Class {
    fn matches(&self, c: char, ignore_case: bool) -> bool {
        let hit = |c: char| self.items.iter().any(|i| i.test(c));
        let found = hit(c) || (ignore_case && (hit(lower(c)) || hit(upper(c))));
        found != self.negated
    }
}

// ---- matching -----------------------------------------------------------

/// Per group, every capture as char index ranges, in the order they closed.
type Captures = Vec<Vec<(usize, usize)>>;

struct State {
    /// Per group, every capture so far as char index ranges, in the order they closed.
    caps: Captures,
    steps: u64,
    aborted: bool,
}

struct Matcher<'a> {
    re: &'a Regex,
    text: &'a [char],
}

type Cont<'k> = &'k mut dyn FnMut(usize, &mut State) -> bool;

impl Matcher<'_> {
    fn eq(&self, a: char, b: char) -> bool {
        a == b || (self.re.ignore_case && lower(a) == lower(b))
    }

    fn word_at(&self, i: usize) -> bool {
        self.text.get(i).is_some_and(|c| is_word(*c))
    }

    fn seq(&self, nodes: &[Node], i: usize, st: &mut State, k: Cont) -> bool {
        match nodes.split_first() {
            None => k(i, st),
            Some((first, rest)) => self.m(first, i, st, &mut |j, st| self.seq(rest, j, st, k)),
        }
    }

    fn rep(
        &self,
        node: &Node,
        (min, max, greedy): (u32, u32, bool),
        count: u32,
        i: usize,
        st: &mut State,
        k: Cont,
    ) -> bool {
        let more = |this: &Self, st: &mut State, k: Cont| -> bool {
            if count >= max {
                return false;
            }
            this.m(node, i, st, &mut |j, st| {
                // An iteration that goes nowhere cannot help once the minimum is met.
                if j == i && count >= min {
                    return false;
                }
                this.rep(node, (min, max, greedy), count + 1, j, st, k)
            })
        };
        if greedy {
            if more(self, st, k) {
                return true;
            }
            count >= min && k(i, st)
        } else {
            if count >= min && k(i, st) {
                return true;
            }
            more(self, st, k)
        }
    }

    fn lens(st: &State) -> Vec<usize> {
        st.caps.iter().map(Vec::len).collect()
    }

    fn restore(st: &mut State, lens: &[usize]) {
        for (c, n) in st.caps.iter_mut().zip(lens) {
            c.truncate(*n);
        }
    }

    fn m(&self, node: &Node, i: usize, st: &mut State, k: Cont) -> bool {
        st.steps += 1;
        if st.aborted || st.steps > STEP_LIMIT {
            st.aborted = true;
            return false;
        }
        match node {
            Node::Empty => k(i, st),
            Node::Char(c) => self.text.get(i).is_some_and(|t| self.eq(*t, *c)) && k(i + 1, st),
            Node::Any => self.text.get(i).is_some_and(|t| *t != '\n') && k(i + 1, st),
            Node::Class(class) => {
                self.text
                    .get(i)
                    .is_some_and(|t| class.matches(*t, self.re.ignore_case))
                    && k(i + 1, st)
            }
            Node::Bol => i == 0 && k(i, st),
            Node::Eol => {
                (i == self.text.len() || (i + 1 == self.text.len() && self.text[i] == '\n'))
                    && k(i, st)
            }
            Node::WordBoundary(want) => {
                let at = (i > 0 && self.word_at(i - 1)) != self.word_at(i);
                at == *want && k(i, st)
            }
            Node::Seq(nodes) => self.seq(nodes, i, st, k),
            Node::Alt(branches) => {
                for b in branches {
                    if self.m(b, i, st, k) {
                        return true;
                    }
                    if st.aborted {
                        return false;
                    }
                }
                false
            }
            Node::Cap(idx, inner) => self.m(inner, i, st, &mut |j, st| {
                st.caps[*idx].push((i, j));
                if k(j, st) {
                    true
                } else {
                    st.caps[*idx].pop();
                    false
                }
            }),
            Node::Rep {
                node,
                min,
                max,
                greedy,
            } => self.rep(node, (*min, *max, *greedy), 0, i, st, k),
            Node::Look {
                behind,
                negative,
                node,
            } => {
                let before = Self::lens(st);
                let found = if *behind {
                    (0..=i)
                        .rev()
                        .any(|start| self.m(node, start, st, &mut |end, _| end == i))
                } else {
                    self.m(node, i, st, &mut |_, _| true)
                };
                if found == *negative {
                    Self::restore(st, &before);
                    return false;
                }
                if *negative {
                    Self::restore(st, &before);
                }
                if k(i, st) {
                    true
                } else {
                    Self::restore(st, &before);
                    false
                }
            }
            Node::Atomic(inner) => {
                let mut end = None;
                if !self.m(inner, i, st, &mut |j, _| {
                    end = Some(j);
                    true
                }) {
                    return false;
                }
                end.is_some_and(|j| k(j, st))
            }
            Node::Backref(idx) => {
                self.backref(st.caps.get(*idx).and_then(|c| c.last().copied()), i, st, k)
            }
            Node::BackrefName(name) => {
                let idx = self
                    .re
                    .names
                    .iter()
                    .find(|(n, _)| n == name)
                    .map(|(_, i)| *i);
                let last = idx.and_then(|idx| st.caps[idx].last().copied());
                self.backref(last, i, st, k)
            }
        }
    }

    fn backref(&self, captured: Option<(usize, usize)>, i: usize, st: &mut State, k: Cont) -> bool {
        let Some((s, e)) = captured else { return false };
        let len = e - s;
        if i + len > self.text.len() {
            return false;
        }
        (0..len).all(|n| self.eq(self.text[s + n], self.text[i + n])) && k(i + len, st)
    }
}

// ---- public API ---------------------------------------------------------

/// One successful match with every group's capture history.
pub struct Match<'t> {
    text: &'t str,
    start: usize,
    end: usize,
    caps: Vec<Vec<(usize, usize)>>,
    names: &'t [(String, usize)],
}

impl<'t> Match<'t> {
    /// The whole text the match was made on.
    pub fn source(&self) -> &'t str {
        self.text
    }

    pub fn as_str(&self) -> &'t str {
        &self.text[self.start..self.end]
    }

    pub fn range(&self) -> Range<usize> {
        self.start..self.end
    }

    fn index(&self, name: &str) -> Option<usize> {
        self.names.iter().find(|(n, _)| n == name).map(|(_, i)| *i)
    }

    /// Byte ranges of every capture of the named group, in the order they were made.
    pub fn capture_ranges(&self, name: &str) -> Vec<Range<usize>> {
        self.index(name)
            .map(|i| self.caps[i].iter().map(|(s, e)| *s..*e).collect())
            .unwrap_or_default()
    }

    /// Every capture of the named group (.NET `Group.Captures`).
    pub fn captures(&self, name: &str) -> Vec<&'t str> {
        self.capture_ranges(name)
            .into_iter()
            .map(|r| &self.text[r])
            .collect()
    }

    /// The last capture, which is what .NET calls the group's value.
    pub fn group(&self, name: &str) -> Option<&'t str> {
        self.capture_ranges(name).pop().map(|r| &self.text[r])
    }

    /// Where the last capture of the group ends, if it matched.
    pub fn group_end(&self, name: &str) -> Option<usize> {
        self.capture_ranges(name).pop().map(|r| r.end)
    }

    fn group_by_number(&self, n: usize) -> Option<&'t str> {
        self.caps
            .get(n)
            .and_then(|c| c.last())
            .map(|(s, e)| &self.text[*s..*e])
    }

    /// Fills in `$1`, `${name}` and `$$` in a replacement template.
    pub fn expand(&self, template: &str) -> String {
        let chars: Vec<char> = template.chars().collect();
        let mut out = String::new();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] != '$' || i + 1 >= chars.len() {
                out.push(chars[i]);
                i += 1;
                continue;
            }
            match chars[i + 1] {
                '$' => {
                    out.push('$');
                    i += 2;
                }
                '{' => {
                    let close = chars[i + 2..].iter().position(|c| *c == '}');
                    match close {
                        Some(len) => {
                            let name: String = chars[i + 2..i + 2 + len].iter().collect();
                            let value = match name.parse::<usize>() {
                                Ok(n) => self.group_by_number(n),
                                Err(_) => self.group(&name),
                            };
                            out.push_str(value.unwrap_or_default());
                            i += len + 3;
                        }
                        None => {
                            out.push('$');
                            i += 1;
                        }
                    }
                }
                d if d.is_ascii_digit() => {
                    out.push_str(
                        self.group_by_number(d as usize - '0' as usize)
                            .unwrap_or_default(),
                    );
                    i += 2;
                }
                _ => {
                    out.push('$');
                    i += 1;
                }
            }
        }
        out
    }

    /// Whether the group took part in the match (.NET `Group.Success`).
    pub fn success(&self, name: &str) -> bool {
        self.index(name).is_some_and(|i| !self.caps[i].is_empty())
    }
}

impl Regex {
    pub fn new(pattern: &str) -> Result<Self, String> {
        Self::build(pattern, false)
    }

    /// Case-insensitive, like `RegexOptions.IgnoreCase`.
    pub fn ci(pattern: &str) -> Result<Self, String> {
        Self::build(pattern, true)
    }

    fn build(pattern: &str, ignore_case: bool) -> Result<Self, String> {
        let mut p = Parser {
            chars: pattern.chars().collect(),
            pos: 0,
            groups: 0,
            names: Vec::new(),
            ignore_case,
        };
        let root = p.alternation().map_err(|e| format!("{e} in /{pattern}/"))?;
        if p.pos != p.chars.len() {
            return Err(format!("unbalanced ) in /{pattern}/"));
        }
        Ok(Self {
            root,
            groups: p.groups,
            names: p.names,
            ignore_case: p.ignore_case,
        })
    }

    /// Tries to match starting exactly at char index `at`.
    fn match_at(&self, text: &[char], at: usize) -> Option<(usize, Captures)> {
        let mut st = State {
            caps: vec![Vec::new(); self.groups + 1],
            steps: 0,
            aborted: false,
        };
        let matcher = Matcher { re: self, text };
        let mut end = None;
        let ok = matcher.m(&self.root, at, &mut st, &mut |j, _| {
            end = Some(j);
            true
        });
        if ok { end.map(|e| (e, st.caps)) } else { None }
    }

    /// The first match at or after byte offset `from`.
    pub fn find_from<'t>(&'t self, text: &'t str, from: usize) -> Option<Match<'t>> {
        let chars: Vec<char> = text.chars().collect();
        let mut offsets: Vec<usize> = text.char_indices().map(|(b, _)| b).collect();
        offsets.push(text.len());
        let first = offsets.iter().position(|b| *b >= from)?;
        for at in first..=chars.len() {
            if let Some((end, caps)) = self.match_at(&chars, at) {
                let to_byte = |(s, e): (usize, usize)| (offsets[s], offsets[e]);
                return Some(Match {
                    text,
                    start: offsets[at],
                    end: offsets[end],
                    caps: caps
                        .into_iter()
                        .map(|c| c.into_iter().map(to_byte).collect())
                        .collect(),
                    names: &self.names,
                });
            }
        }
        None
    }

    pub fn find<'t>(&'t self, text: &'t str) -> Option<Match<'t>> {
        self.find_from(text, 0)
    }

    pub fn is_match(&self, text: &str) -> bool {
        self.find(text).is_some()
    }

    /// Every non-overlapping match, like .NET `Regex.Matches`.
    pub fn find_all<'t>(&'t self, text: &'t str) -> Vec<Match<'t>> {
        let mut out = Vec::new();
        let mut from = 0;
        while let Some(m) = self.find_from(text, from) {
            from = if m.end > m.start {
                m.end
            } else {
                // An empty match: step over one character.
                text[m.end..]
                    .chars()
                    .next()
                    .map_or(text.len() + 1, |c| m.end + c.len_utf8())
            };
            out.push(m);
            if from > text.len() {
                break;
            }
        }
        out
    }

    /// Replaces every match with a template: `$1`, `${name}` and `$$` are
    /// expanded (the last capture of a named group, empty if it did not match).
    pub fn replace_all_template(&self, text: &str, template: &str) -> String {
        let mut out = String::new();
        let mut last = 0;
        for m in self.find_all(text) {
            out.push_str(&text[last..m.start]);
            out.push_str(&m.expand(template));
            last = m.end;
        }
        out.push_str(&text[last..]);
        out
    }

    /// Replaces every match with `with`.
    pub fn replace_all(&self, text: &str, with: &str) -> String {
        let mut out = String::new();
        let mut last = 0;
        for m in self.find_all(text) {
            out.push_str(&text[last..m.start]);
            out.push_str(with);
            last = m.end;
        }
        out.push_str(&text[last..]);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn re(p: &str) -> Regex {
        Regex::ci(p).unwrap()
    }

    #[test]
    fn literals_classes_and_anchors() {
        assert!(re("^abc$").is_match("ABC"));
        assert!(!Regex::new("^abc$").unwrap().is_match("ABC"));
        assert!(re(r"^\d{4}-\d{2}$").is_match("2024-05"));
        assert!(!re(r"^\d{4}$").is_match("12345"));
        assert!(re("[a-c]+").is_match("xxbxx"));
        assert!(re("[^a-c]+").find("abcxyz").unwrap().as_str() == "xyz");
        assert!(re(r"[-_. ]+").is_match("a.b"));
        assert!(re(r"[\w-]+").find("--ab_c--").unwrap().as_str() == "--ab_c--");
        assert_eq!(re(r"a.c").find("a\nc a-c").unwrap().as_str(), "a-c");
    }

    #[test]
    fn greedy_and_lazy_quantifiers() {
        assert_eq!(re("a+").find("caaat").unwrap().as_str(), "aaa");
        assert_eq!(re("a+?").find("caaat").unwrap().as_str(), "a");
        assert_eq!(re("<.+>").find("<a><b>").unwrap().as_str(), "<a><b>");
        assert_eq!(re("<.+?>").find("<a><b>").unwrap().as_str(), "<a>");
        assert_eq!(re(r"\d{2,3}").find("12345").unwrap().as_str(), "123");
        assert_eq!(re(r"\d{2,3}?").find("12345").unwrap().as_str(), "12");
        assert_eq!(re(r"x{2}").find("xxx").unwrap().as_str(), "xx");
        // A brace that is not a quantifier is a literal.
        assert_eq!(re(r"a{,2}").find("a{,2}").unwrap().as_str(), "a{,2}");
    }

    #[test]
    fn alternation_prefers_the_first_branch() {
        assert_eq!(re("a|ab").find("ab").unwrap().as_str(), "a");
        assert_eq!(re("(?:ab|a)c").find("ac").unwrap().as_str(), "ac");
        assert_eq!(re("x(?:a|b)+y").find("xabbay").unwrap().as_str(), "xabbay");
    }

    #[test]
    fn named_groups_keep_every_iteration() {
        let r = re(r"^S(?<season>\d+)(?:E(?<episode>\d+))+");
        let m = r.find("S01E02E03E04").unwrap();
        assert_eq!(m.group("season"), Some("01"));
        assert_eq!(m.captures("episode"), ["02", "03", "04"]);
        assert_eq!(
            m.group("episode"),
            Some("04"),
            "the value is the last capture"
        );
        assert!(m.success("episode") && !m.success("nope"));
    }

    #[test]
    fn the_same_name_may_appear_twice() {
        let r = re(r"(?<n>\d+)-(?<n>\d+)");
        let m = r.find("12-34").unwrap();
        assert_eq!(m.captures("n"), ["12", "34"]);
        let r = re(r"(?:S(?<season>\d+)(?:-S?(?<season>\d+))+)");
        assert_eq!(
            r.find("S01-S02-03").unwrap().captures("season"),
            ["01", "02", "03"]
        );
    }

    #[test]
    fn failed_iterations_leave_no_captures_behind() {
        let r = re(r"^(?:(?<a>x)y|(?<b>x)z)$");
        let m = r.find("xz").unwrap();
        assert!(!m.success("a") && m.success("b"));
        let r = re(r"(?<w>\d)+x");
        assert_eq!(r.find("123x").unwrap().captures("w"), ["1", "2", "3"]);
    }

    #[test]
    fn lookarounds() {
        assert_eq!(re(r"a(?=b)").find("ac ab").unwrap().range(), 3..4);
        assert_eq!(re(r"a(?!b)").find("ab ac").unwrap().range(), 3..4);
        assert_eq!(re(r"(?<=x)\d").find("1 y2 x3").unwrap().as_str(), "3");
        assert_eq!(re(r"(?<!x)\d").find("x1 y2").unwrap().as_str(), "2");
        // Variable-length look-behind.
        assert_eq!(
            re(r"(?<!\d+)\d").find("12 x").map(|m| m.range()),
            Some(0..1)
        );
        assert_eq!(re(r"(?<=\b0\d)\d").find("007").unwrap().as_str(), "7");
        // Captures inside a failed negative look-ahead do not leak.
        let r = re(r"(?!(?<c>b))a");
        let m = r.find("a").unwrap();
        assert!(!m.success("c"));
    }

    #[test]
    fn back_references() {
        let r = re(r"^(?<y>\d{4})(?<sep>[-_]?)(?<m>\d{2})\k<sep>(?<d>\d{2})$");
        assert!(r.is_match("2018-10-12"));
        assert!(r.is_match("20181012"));
        assert!(!r.is_match("2018-1012"));
        assert!(re(r"(a)\1").is_match("aA"));
    }

    #[test]
    fn word_boundaries_and_unicode() {
        assert!(re(r"\bfoo\b").is_match("a foo b"));
        assert!(!re(r"\bfoo\b").is_match("afoob"));
        assert!(re(r"\w+").find("héllo wörld").unwrap().as_str() == "héllo");
        assert!(re(r"\bné\b").is_match("un né un"));
        assert_eq!(re(r"é+").find("caféé!").unwrap().as_str(), "éé");
        assert!(re(r"\p{L}+").is_match("日本語"));
    }

    #[test]
    fn matches_returns_every_non_overlapping_hit() {
        let r = re(r"S(?<s>\d+)E(?<e>\d+)");
        let all = r.find_all("S01E02 and S03E04");
        let found: Vec<_> = all
            .iter()
            .map(|m| (m.group("s").unwrap(), m.group("e").unwrap()))
            .collect();
        assert_eq!(found, [("01", "02"), ("03", "04")]);
        assert_eq!(re("x*").find_all("axb").len(), 4);
    }

    #[test]
    fn replace_all_works_on_byte_offsets() {
        assert_eq!(re(r"\s+").replace_all("a  b\tc", " "), "a b c");
        assert_eq!(re("é").replace_all("café é", "e"), "cafe e");
    }

    #[test]
    fn atomic_groups_do_not_give_back() {
        assert!(re(r"^(?>a+)b").is_match("aaab"));
        assert!(!re(r"^(?>a+)ab").is_match("aaab"));
        assert!(re(r"^(?:a+)ab").is_match("aaab"));
    }

    #[test]
    fn unicode_and_hex_escapes() {
        assert!(re(r"^[\u4E00-\u9FCC]+$").is_match("日本語"));
        assert!(!re(r"^[\u4E00-\u9FCC]+$").is_match("abc"));
        assert!(re(r"\x41\u0042").is_match("AB"));
        assert!(Regex::new(r"\u12").is_err());
    }

    #[test]
    fn templates_expand_named_and_numbered_groups() {
        let r = re(r"^\.E(\d{2,4})\.\d{6}\.(.*-(F1RST|NEXT))$");
        assert_eq!(
            r.replace_all_template(".E0102.230101.Show-NEXT", ".S01E$1.$2"),
            ".S01E0102.Show-NEXT"
        );
        let r = re(r"(?<a>x)(?<b>y)");
        assert_eq!(
            r.replace_all_template("xy", "${b}${a}$$${missing}!"),
            "yx$!"
        );
    }

    #[test]
    fn leading_inline_flag_and_errors() {
        assert!(Regex::new("(?i)abc").unwrap().is_match("ABC"));
        for bad in ["(", "a)", "[a", "*a", "a{2,1}", r"\k<x", "(?<n"] {
            assert!(Regex::new(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn runaway_patterns_give_up_instead_of_hanging() {
        let r = re(r"^(a+)+$");
        assert!(!r.is_match(&format!("{}b", "a".repeat(40))));
    }

    #[test]
    fn empty_loops_terminate() {
        assert!(re(r"^(?:a*)*$").is_match("aaa"));
        assert!(re(r"^(?:)+x").is_match("x"));
    }
}
