//! Text, number and date helpers the definitions' filters and fields rely on.
//! Ported from Prowlarr's `ParseUtil` and `DateTimeUtil` (GPL-3.0).

use std::sync::LazyLock;

use chrono::{
    DateTime, Datelike, Duration, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc,
    Weekday,
};
use fancy_regex::Regex;

macro_rules! re {
    ($pat:expr) => {{
        static R: LazyLock<Regex> = LazyLock::new(|| Regex::new($pat).expect("a fixed pattern"));
        &*R
    }};
}

// ---- numbers --------------------------------------------------------------

fn normalize_number(s: &str, int: bool) -> String {
    let mut v: String = s
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.' || *c == ',')
        .collect();
    v = v.trim().replace('-', "0");
    if int {
        if v.contains(',') && v.contains('.') {
            return v;
        }
        return if v.is_empty() {
            "0".into()
        } else {
            v.replace('.', ",")
        };
    }
    v = if v.is_empty() {
        "0".into()
    } else {
        v.replace(',', ".")
    };
    if v.matches('.').count() > 1
        && let Some(last) = v.rfind('.')
    {
        v = format!("{}{}", v[..last].replace('.', ""), &v[last..]);
    }
    v
}

pub fn coerce_double(s: &str) -> f64 {
    normalize_number(s, false).parse().unwrap_or(0.0)
}

/// Whole numbers; a thousands comma or dot is dropped ("1,234" is 1234).
pub fn coerce_long(s: &str) -> i64 {
    let n = normalize_number(s, true);
    n.replace([',', '.'], "").parse().unwrap_or(0)
}

pub fn coerce_int(s: &str) -> i32 {
    coerce_long(s).clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

/// The first run of digits in the text.
pub fn long_from_string(s: &str) -> Option<i64> {
    if s.trim().is_empty() {
        return None;
    }
    let mut digits = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() {
            digits.push(c);
        } else if !digits.is_empty() {
            break;
        }
    }
    Some(coerce_long(&digits))
}

/// "1.5 GB", "700 MiB", "42": bytes.
pub fn parse_bytes(s: &str) -> u64 {
    let unit: String = s
        .chars()
        .filter(|c| c.is_alphabetic())
        .collect::<String>()
        .replace('i', "")
        .to_lowercase();
    let value = coerce_double(s);
    let mult = if unit.contains("kb") {
        1024.0
    } else if unit.contains("mb") {
        1024.0 * 1024.0
    } else if unit.contains("gb") {
        1024.0 * 1024.0 * 1024.0
    } else if unit.contains("tb") {
        1024.0 * 1024.0 * 1024.0 * 1024.0
    } else {
        1.0
    };
    (value * mult).max(0.0) as u64
}

// ---- URLs and escaping ----------------------------------------------------

/// Query-string encoding the way .NET's `WebUtility.UrlEncode` does it: a
/// space is `+`, and `- _ . ! * ( )` stay as they are.
pub fn url_encode(s: &str, encoding: &'static encoding_rs::Encoding) -> String {
    let (bytes, _, _) = encoding.encode(s);
    let mut out = String::with_capacity(bytes.len());
    for b in bytes.iter() {
        match *b {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'*'
            | b'('
            | b')' => out.push(*b as char),
            b' ' => out.push('+'),
            b => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub fn url_decode(s: &str, encoding: &'static encoding_rs::Encoding) -> String {
    let bytes = s.as_bytes();
    let mut raw = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len()
                && bytes[i + 1].is_ascii_hexdigit()
                && bytes[i + 2].is_ascii_hexdigit() =>
            {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("00");
                raw.push(u8::from_str_radix(hex, 16).unwrap_or(0));
                i += 3;
            }
            b'+' => {
                raw.push(b' ');
                i += 1;
            }
            b => {
                raw.push(b);
                i += 1;
            }
        }
    }
    encoding.decode(&raw).0.into_owned()
}

pub fn html_decode(s: &str) -> String {
    html_escape::decode_html_entities(s).into_owned()
}

pub fn html_encode(s: &str) -> String {
    html_escape::encode_text(s).into_owned()
}

/// The value of a query-string argument in a URL.
pub fn query_argument(url: &str, name: &str) -> Option<String> {
    let query = url.split_once('?')?.1;
    let query = query.split('#').next()?;
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        (url_decode(k, encoding_rs::UTF_8) == name).then(|| url_decode(v, encoding_rs::UTF_8))
    })
}

/// Turns a .NET pattern into one this engine reads: a `[` inside a character
/// class is a plain character there, `&&` and `~~` are not operators, and
/// `\p{IsCyrillic}` names a script.
pub fn net_pattern(p: &str) -> String {
    let mut out = String::with_capacity(p.len());
    let c: Vec<char> = p.chars().collect();
    let (mut i, mut in_class) = (0, false);
    while i < c.len() {
        match c[i] {
            '\\' if i + 1 < c.len() => {
                // \p{IsX} and \P{IsX}
                if matches!(c[i + 1], 'p' | 'P')
                    && c.get(i + 2) == Some(&'{')
                    && let Some(end) = c[i + 3..].iter().position(|x| *x == '}')
                {
                    let name: String = c[i + 3..i + 3 + end].iter().collect();
                    let mapped = match name.as_str() {
                        "IsCJKUnifiedIdeographs" => "Han".to_owned(),
                        n if n.starts_with("Is") && n.len() > 2 => n[2..].to_owned(),
                        n => n.to_owned(),
                    };
                    out.push('\\');
                    out.push(c[i + 1]);
                    out.push('{');
                    out.push_str(&mapped);
                    out.push('}');
                    i += 4 + end;
                    continue;
                }
                out.push(c[i]);
                out.push(c[i + 1]);
                i += 2;
                continue;
            }
            '[' if in_class => {
                // A POSIX class like [:alpha:] stays; any other `[` is a letter.
                if c.get(i + 1) == Some(&':') {
                    out.push('[');
                } else {
                    out.push_str("\\[");
                }
            }
            '[' => {
                in_class = true;
                out.push('[');
                if c.get(i + 1) == Some(&'^') {
                    out.push('^');
                    i += 1;
                }
                // A `]` first in a class is a letter.
                if c.get(i + 1) == Some(&']') {
                    out.push_str("\\]");
                    i += 1;
                }
            }
            ']' if in_class => {
                in_class = false;
                out.push(']');
            }
            '&' | '~' if in_class => {
                out.push('\\');
                out.push(c[i]);
            }
            ch => out.push(ch),
        }
        i += 1;
    }
    out
}

/// Compiles a pattern written for .NET.
pub fn compile(pattern: &str) -> Result<Regex, String> {
    Regex::new(&net_pattern(pattern)).map_err(|e| format!("bad pattern '{pattern}': {e}"))
}

/// Expands a .NET replacement string: `$1`, `${name}` and `$$`.
fn expand_net(repl: &str, caps: &fancy_regex::Captures<'_>, out: &mut String) {
    let c: Vec<char> = repl.chars().collect();
    let mut i = 0;
    while i < c.len() {
        if c[i] != '$' || i + 1 >= c.len() {
            out.push(c[i]);
            i += 1;
            continue;
        }
        match c[i + 1] {
            '$' => {
                out.push('$');
                i += 2;
            }
            '{' => {
                if let Some(end) = c[i + 2..].iter().position(|x| *x == '}') {
                    let name: String = c[i + 2..i + 2 + end].iter().collect();
                    let m = name
                        .parse::<usize>()
                        .ok()
                        .map_or_else(|| caps.name(&name), |n| caps.get(n));
                    out.push_str(m.map_or("", |m| m.as_str()));
                    i += 3 + end;
                } else {
                    out.push('$');
                    i += 1;
                }
            }
            d if d.is_ascii_digit() => {
                let mut j = i + 1;
                let mut n = String::new();
                while j < c.len() && c[j].is_ascii_digit() {
                    n.push(c[j]);
                    j += 1;
                }
                out.push_str(
                    caps.get(n.parse().unwrap_or(usize::MAX))
                        .map_or("", |m| m.as_str()),
                );
                i = j;
            }
            _ => {
                out.push('$');
                i += 1;
            }
        }
    }
}

/// `Regex.Replace` with .NET's replacement syntax.
pub fn net_replace(re: &Regex, input: &str, replacement: &str) -> String {
    let mut out = String::new();
    let mut last = 0;
    for caps in re.captures_iter(input).flatten() {
        let Some(m) = caps.get(0) else { continue };
        out.push_str(&input[last..m.start()]);
        expand_net(replacement, &caps, &mut out);
        last = m.end();
    }
    out.push_str(&input[last..]);
    out
}

// ---- dates ----------------------------------------------------------------

#[derive(Default)]
struct Fields {
    year: Option<i32>,
    month: Option<u32>,
    day: Option<u32>,
    hour: Option<u32>,
    minute: Option<u32>,
    second: Option<u32>,
    nanos: u32,
    pm: Option<bool>,
    offset_minutes: Option<i32>,
}

const MONTHS: [&str; 12] = [
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];
const DAYS: [&str; 7] = [
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
];

fn digits(input: &[char], at: &mut usize, min: usize, max: usize) -> Option<u32> {
    let mut n = 0u32;
    let mut count = 0;
    while count < max && *at < input.len() && input[*at].is_ascii_digit() {
        n = n * 10 + input[*at].to_digit(10)?;
        *at += 1;
        count += 1;
    }
    (count >= min).then_some(n)
}

fn name(input: &[char], at: &mut usize, names: &[&str], short: bool) -> Option<usize> {
    let rest: String = input[*at..].iter().collect::<String>().to_lowercase();
    // Longest first, so "March" is not read as "Mar" + "ch".
    let mut best: Option<(usize, usize)> = None;
    for (i, n) in names.iter().enumerate() {
        let candidates: Vec<&str> = if short { vec![&n[..3]] } else { vec![n] };
        for cand in candidates {
            if rest.starts_with(cand) && best.is_none_or(|(_, l)| cand.len() > l) {
                best = Some((i, cand.len()));
            }
        }
    }
    let (i, len) = best?;
    *at += len;
    Some(i)
}

/// Reads `input` by a .NET custom date format ("yyyy-MM-dd HH:mm:ss zzz").
fn parse_net(input: &str, format: &str) -> Option<DateTime<Utc>> {
    let inp: Vec<char> = input.chars().collect();
    let fmt: Vec<char> = format.chars().collect();
    let (mut i, mut f) = (0usize, 0usize);
    let mut v = Fields::default();
    while f < fmt.len() {
        let c = fmt[f];
        let run = fmt[f..].iter().take_while(|x| **x == c).count();
        match c {
            'y' => {
                if run >= 3 {
                    v.year = Some(digits(&inp, &mut i, 4, 4)? as i32);
                } else {
                    let y = digits(&inp, &mut i, 2, 2)? as i32;
                    v.year = Some(if y > 49 { 1900 + y } else { 2000 + y });
                }
            }
            'M' => match run {
                1 => v.month = Some(digits(&inp, &mut i, 1, 2)?),
                2 => v.month = Some(digits(&inp, &mut i, 2, 2)?),
                3 => v.month = Some(name(&inp, &mut i, &MONTHS, true)? as u32 + 1),
                _ => v.month = Some(name(&inp, &mut i, &MONTHS, false)? as u32 + 1),
            },
            'd' => match run {
                1 => v.day = Some(digits(&inp, &mut i, 1, 2)?),
                2 => v.day = Some(digits(&inp, &mut i, 2, 2)?),
                3 => {
                    name(&inp, &mut i, &DAYS, true)?;
                }
                _ => {
                    name(&inp, &mut i, &DAYS, false)?;
                }
            },
            'H' | 'h' => {
                let h = if run == 1 {
                    digits(&inp, &mut i, 1, 2)?
                } else {
                    digits(&inp, &mut i, 2, 2)?
                };
                v.hour = Some(h);
            }
            'm' => {
                v.minute = Some(if run == 1 {
                    digits(&inp, &mut i, 1, 2)?
                } else {
                    digits(&inp, &mut i, 2, 2)?
                })
            }
            's' => {
                v.second = Some(if run == 1 {
                    digits(&inp, &mut i, 1, 2)?
                } else {
                    digits(&inp, &mut i, 2, 2)?
                })
            }
            'f' | 'F' => {
                let start = i;
                let n = digits(&inp, &mut i, if c == 'f' { run } else { 0 }, run);
                let taken = i - start;
                if let Some(n) = n.filter(|_| taken > 0) {
                    v.nanos = n * 10u32.pow(9 - taken as u32);
                }
            }
            't' => {
                let rest: String = inp[i..].iter().collect::<String>().to_lowercase();
                let len = if run == 1 { 1 } else { 2 };
                if rest.starts_with('p') {
                    v.pm = Some(true);
                } else if rest.starts_with('a') {
                    v.pm = Some(false);
                } else {
                    return None;
                }
                i += len.min(inp.len() - i);
            }
            'z' | 'K' => {
                if c == 'K' && inp.get(i) == Some(&'Z') {
                    v.offset_minutes = Some(0);
                    i += 1;
                } else {
                    let sign = match inp.get(i) {
                        Some('+') => 1,
                        Some('-') => -1,
                        _ => return None,
                    };
                    i += 1;
                    let h = digits(&inp, &mut i, 1, 2)? as i32;
                    let mut m = 0;
                    if (run >= 3 || c == 'K') && inp.get(i) == Some(&':') {
                        i += 1;
                        m = digits(&inp, &mut i, 2, 2)? as i32;
                    }
                    v.offset_minutes = Some(sign * (h * 60 + m));
                }
            }
            '\'' | '"' => {
                let mut lit = String::new();
                f += 1;
                while f < fmt.len() && fmt[f] != c {
                    lit.push(fmt[f]);
                    f += 1;
                }
                for l in lit.chars() {
                    if inp.get(i) != Some(&l) {
                        return None;
                    }
                    i += 1;
                }
                f += 1;
                continue;
            }
            '\\' => {
                f += 1;
                if inp.get(i) != fmt.get(f) {
                    return None;
                }
                i += 1;
                f += 1;
                continue;
            }
            ' ' => {
                if inp.get(i) != Some(&' ') {
                    return None;
                }
                while inp.get(i) == Some(&' ') {
                    i += 1;
                }
                f += run;
                continue;
            }
            other => {
                if inp.get(i) != Some(&other) {
                    return None;
                }
                i += 1;
                f += 1;
                continue;
            }
        }
        f += run;
    }
    if i != inp.len() {
        return None;
    }
    let mut hour = v.hour.unwrap_or(0);
    if let Some(pm) = v.pm {
        if hour == 12 {
            hour = if pm { 12 } else { 0 };
        } else if pm {
            hour += 12;
        }
    }
    let today = Utc::now().date_naive();
    let date = NaiveDate::from_ymd_opt(
        v.year.unwrap_or(today.year()),
        v.month.unwrap_or(today.month()),
        v.day.unwrap_or(1),
    )?;
    let naive = date.and_time(NaiveTime::from_hms_nano_opt(
        hour,
        v.minute.unwrap_or(0),
        v.second.unwrap_or(0),
        v.nanos,
    )?);
    Some(match v.offset_minutes {
        Some(m) => FixedOffset::east_opt(m * 60)?
            .from_local_datetime(&naive)
            .single()?
            .with_timezone(&Utc),
        None => Utc.from_utc_datetime(&naive),
    })
}

/// Turns a Go time layout ("2006-01-02 15:04:05") into the .NET format with
/// the same meaning, by the same chain of replacements Prowlarr uses.
fn go_to_net(layout: &str) -> String {
    let mut s = layout.to_owned();
    for (from, to) in [
        ("2006", "yyyy"),
        ("06", "yy"),
        ("January", "MMMM"),
        ("Jan", "MMM"),
        ("01", "MM"),
        ("Monday", "dddd"),
        ("Mon", "ddd"),
        ("02", "dd"),
        ("2", "d"),
        ("05", "ss"),
        ("15", "HH"),
        ("03", "hh"),
        ("3", "h"),
        ("04", "mm"),
        ("4", "m"),
        ("5", "s"),
        ("1", "M"),
        (".0000", "ffff"),
        (".000", "fff"),
        (".00", "ff"),
        (".0", "f"),
        (".9999", "FFFF"),
        (".999", "FFF"),
        (".99", "FF"),
        (".9", "F"),
        ("PM", "tt"),
        ("pm", "tt"),
        ("Z07:00", "'Z'zzz"),
        ("Z07", "'Z'zz"),
        ("-07:00", "zzz"),
        ("-07", "zz"),
    ] {
        s = s.replace(from, to);
    }
    s
}

/// A date by a layout that is either .NET's or Go's.
pub fn parse_with_layout(date: &str, layout: &str) -> Result<DateTime<Utc>, String> {
    let date = date.trim();
    let lower = layout.to_lowercase();
    if ["y", "h", "d"].iter().any(|c| lower.contains(c))
        && let Some(d) = parse_net(date, layout)
    {
        return Ok(d);
    }
    let net = go_to_net(layout);
    parse_net(date, &net).ok_or_else(|| format!("\"{date}\" does not fit the layout \"{layout}\""))
}

/// "2 hours 1 day ago", "now".
pub fn from_time_ago(s: &str) -> Result<DateTime<Utc>, String> {
    let s = s.to_lowercase();
    if s.contains("now") {
        return Ok(Utc::now());
    }
    let s = s.replace([','], "").replace("ago", "").replace("and", "");
    let mut ago = Duration::zero();
    let re = re!(r"\s*?([\d\.]+)\s*?([^\d\s\.]+)\s*?");
    for caps in re.captures_iter(&s).flatten() {
        let val = coerce_double(&caps[1]);
        let unit = &caps[2];
        let secs = if unit.contains("sec") || unit == "s" {
            1.0
        } else if unit.contains("min") || unit == "m" {
            60.0
        } else if unit.contains("hour") || unit.contains("hr") || unit == "h" {
            3600.0
        } else if unit.contains("day") || unit == "d" {
            86400.0
        } else if unit.contains("week") || unit.contains("wk") || unit == "w" {
            86400.0 * 7.0
        } else if unit.contains("month") || unit == "mo" {
            86400.0 * 30.0
        } else if unit.contains("year") || unit == "y" {
            86400.0 * 365.0
        } else {
            return Err(format!("unknown time unit '{unit}'"));
        };
        ago += Duration::milliseconds((val * secs * 1000.0) as i64);
    }
    Ok(Utc::now() - ago)
}

fn time_of_day(s: &str) -> Result<Duration, String> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(Duration::zero());
    }
    for fmt in ["%H:%M:%S", "%H:%M", "%I:%M:%S %p", "%I:%M %p", "%I:%M%p"] {
        if let Ok(t) = NaiveTime::parse_from_str(s, fmt) {
            return Ok(t.signed_duration_since(NaiveTime::MIN));
        }
    }
    Err(format!("cannot read the time '{s}'"))
}

/// A date written in any of the usual ways.
pub fn from_fuzzy(s: &str) -> Result<DateTime<Utc>, String> {
    let s = s.trim();
    if let Ok(d) = DateTime::parse_from_rfc3339(s).or_else(|_| DateTime::parse_from_rfc2822(s)) {
        return Ok(d.with_timezone(&Utc));
    }
    let with_time = [
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y/%m/%d %H:%M:%S",
        "%Y/%m/%d %H:%M",
        "%m/%d/%Y %H:%M:%S",
        "%m/%d/%Y %H:%M",
        "%m/%d/%Y %I:%M:%S %p",
        "%m/%d/%Y %I:%M %p",
        "%d/%m/%Y %H:%M:%S",
        "%d/%m/%Y %H:%M",
        "%d-%m-%Y %H:%M:%S",
        "%d-%m-%Y %H:%M",
        "%m-%d-%Y %H:%M:%S",
        "%d.%m.%Y %H:%M:%S",
        "%d.%m.%Y %H:%M",
        "%d.%m.%Y, %H:%M",
        "%b %d, %Y %H:%M:%S",
        "%b %d, %Y %H:%M",
        "%b %d, %Y %I:%M %p",
        "%B %d, %Y %H:%M",
        "%B %d, %Y %I:%M %p",
        "%d %b %Y %H:%M:%S",
        "%d %b %Y %H:%M",
        "%d %B %Y %H:%M",
        "%d %b %Y, %H:%M",
        "%d %B %Y, %H:%M",
        "%a, %d %b %Y %H:%M:%S",
        "%d-%b-%Y %H:%M",
        "%d-%b-%Y %H:%M:%S",
        "%b %d %Y %H:%M",
        "%b %d %Y %I:%M %p",
        "%Y-%m-%d %I:%M:%S %p",
        "%Y-%m-%d %I:%M %p",
    ];
    for fmt in with_time {
        if let Ok(n) = NaiveDateTime::parse_from_str(s, fmt) {
            return Ok(Utc.from_utc_datetime(&n));
        }
    }
    let date_only = [
        "%Y-%m-%d",
        "%Y/%m/%d",
        "%m/%d/%Y",
        "%d/%m/%Y",
        "%d-%m-%Y",
        "%d.%m.%Y",
        "%b %d, %Y",
        "%B %d, %Y",
        "%d %b %Y",
        "%d %B %Y",
        "%d-%b-%Y",
        "%b %d %Y",
        "%Y.%m.%d",
        "%d/%m/%y",
        "%m/%d/%y",
        "%d.%m.%y",
    ];
    for fmt in date_only {
        if let Ok(d) = NaiveDate::parse_from_str(s, fmt) {
            return Ok(Utc.from_utc_datetime(&d.and_time(NaiveTime::MIN)));
        }
    }
    Err(format!("cannot read the date '{s}'"))
}

fn weekday_of(name: &str) -> Weekday {
    match name.to_lowercase().as_str() {
        n if n.starts_with("mon") => Weekday::Mon,
        n if n.starts_with("tue") => Weekday::Tue,
        n if n.starts_with("wed") => Weekday::Wed,
        n if n.starts_with("thu") => Weekday::Thu,
        n if n.starts_with("fri") => Weekday::Fri,
        n if n.starts_with("sat") => Weekday::Sat,
        _ => Weekday::Sun,
    }
}

/// A date of unknown format: a Unix time, "3 hours ago", "Yesterday 14:22",
/// "Saturday at 14:22", or a written date.
pub fn from_unknown(s: &str) -> Result<DateTime<Utc>, String> {
    let s = s.trim();
    if let Ok(d) = DateTime::parse_from_rfc2822(s) {
        return Ok(d.with_timezone(&Utc));
    }
    if !s.is_empty()
        && s.chars().all(|c| c.is_ascii_digit())
        && let Ok(secs) = s.parse::<i64>()
    {
        // Milliseconds are 13 digits long.
        let secs = if s.len() >= 13 { secs / 1000 } else { secs };
        return DateTime::from_timestamp(secs, 0).ok_or_else(|| format!("'{s}' is not a time"));
    }
    if s.to_lowercase().contains("now") {
        return Ok(Utc::now());
    }
    if re!(r"(?i)\bago").is_match(s).unwrap_or(false) {
        return from_time_ago(s);
    }
    let midnight = Utc::now().date_naive().and_time(NaiveTime::MIN);
    let midnight = Utc.from_utc_datetime(&midnight);
    for (re, shift) in [
        (re!(r"(?i)\btoday(?:[\s,]+(?:at){0,1}\s*|[\s,]*|$)"), 0i64),
        (re!(r"(?i)\byesterday(?:[\s,]+(?:at){0,1}\s*|[\s,]*|$)"), -1),
        (re!(r"(?i)\btomorrow(?:[\s,]+(?:at){0,1}\s*|[\s,]*|$)"), 1),
    ] {
        if let Ok(Some(m)) = re.find(s) {
            let time = s.replace(m.as_str(), "");
            return Ok(midnight + time_of_day(&time)? + Duration::days(shift));
        }
    }
    if let Ok(Some(caps)) =
        re!(r"(?i)\b(monday|tuesday|wednesday|thursday|friday|saturday|sunday)\s+at\s+").captures(s)
    {
        let time = s.replace(&caps[0], "");
        let mut d = midnight + time_of_day(&time)?;
        let want = weekday_of(&caps[1]);
        while d.weekday() != want {
            d -= Duration::days(1);
        }
        return Ok(d);
    }
    let mut text = s.to_owned();
    if let Ok(Some(caps)) = re!(r"^(\d{1,2}-\d{1,2})(\s|$)").captures(s) {
        text = text.replacen(&caps[1], &format!("{}-{}", Utc::now().year(), &caps[1]), 1);
    }
    if let Ok(Some(caps)) = re!(r"^(\d{1,2}\s+\w{3})\s+(\d{1,2}\:\d{1,2}.*)$").captures(s) {
        text = format!("{} {} {}", &caps[1], Utc::now().year(), &caps[2]);
    }
    from_fuzzy(&text)
}

/// The form dates take between filters (RFC 2822), which `from_unknown` reads back.
pub fn to_text(d: DateTime<Utc>) -> String {
    d.to_rfc2822()
}

/// Strips accents ("é" becomes "e") the way the `diacritics` filter does.
pub fn strip_diacritics(s: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    s.nfd()
        .filter(|c| !unicode_normalization::char::is_combining_mark(*c))
        .nfc()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_and_sizes() {
        assert_eq!(coerce_int("1,234"), 1234);
        assert_eq!(coerce_int("12 seeders"), 12);
        assert_eq!(coerce_int("-"), 0);
        assert_eq!(coerce_double("1.5 GB"), 1.5);
        assert_eq!(coerce_double("1,5"), 1.5);
        assert_eq!(parse_bytes("1.5 GB"), 1_610_612_736);
        assert_eq!(parse_bytes("700 MiB"), 734_003_200);
        assert_eq!(parse_bytes("2048"), 2048);
        assert_eq!(long_from_string("tt0133093"), Some(133093));
        assert_eq!(long_from_string("none"), Some(0));
        assert_eq!(long_from_string("  "), None);
    }

    #[test]
    fn urls() {
        let utf8 = encoding_rs::UTF_8;
        assert_eq!(
            url_encode("The Matrix (1999) & co", utf8),
            "The+Matrix+(1999)+%26+co"
        );
        assert_eq!(url_decode("The+Matrix%20%26", utf8), "The Matrix &");
        assert_eq!(
            query_argument("http://x/?a=1&xt=urn%3Abtih%3Aabc#f", "xt").as_deref(),
            Some("urn:btih:abc")
        );
        assert_eq!(url_encode("é", encoding_rs::WINDOWS_1252), "%E9");
    }

    #[test]
    fn net_replacements_follow_dotnet_syntax() {
        let re = Regex::new(r"(\d+)x(\d+)").unwrap();
        assert_eq!(net_replace(&re, "a 12x34 b", "S$1E$2"), "a S12E34 b");
        assert_eq!(net_replace(&re, "12x34", "${2}-${1}$$"), "34-12$");
        assert_eq!(net_replace(&re, "12x34", "$foo"), "$foo");
    }

    #[test]
    fn dotnet_patterns_are_made_readable() {
        let re = compile(r##"[\[!"#$%&'()*+,\-.\/:;<=>?@[\]^_`{|}~]"##).unwrap();
        assert!(re.is_match("a[b").unwrap());
        assert_eq!(net_replace(&re, "a.b", ""), "ab");
        let ru = compile(r"[\p{IsCyrillic}]+").unwrap();
        assert!(ru.is_match("привет").unwrap());
        assert!(!ru.is_match("hello").unwrap());
        assert!(
            compile(r"([\p{IsCJKUnifiedIdeographs}\W]+)")
                .unwrap()
                .is_match("漢字")
                .unwrap()
        );
        assert!(
            compile(r"(?<year>\d{4})")
                .unwrap()
                .is_match("1999")
                .unwrap()
        );
    }

    #[test]
    fn dates_by_layout() {
        let d = parse_with_layout("2023-04-05 14:30:00 +02:00", "yyyy-MM-dd HH:mm:ss zzz").unwrap();
        assert_eq!(d.to_rfc3339(), "2023-04-05T12:30:00+00:00");
        let g = parse_with_layout("2023-04-05 14:30:00", "2006-01-02 15:04:05").unwrap();
        assert_eq!(g.to_rfc3339(), "2023-04-05T14:30:00+00:00");
        let m = parse_with_layout("Mar 7, 2021 3:05 PM", "Jan 2, 2006 3:04 PM").unwrap();
        assert_eq!(m.to_rfc3339(), "2021-03-07T15:05:00+00:00");
        assert!(parse_with_layout("nonsense", "2006-01-02").is_err());
    }

    #[test]
    fn dates_of_unknown_shape() {
        assert_eq!(
            from_unknown("1700000000").unwrap().timestamp(),
            1_700_000_000
        );
        assert_eq!(
            from_unknown("1700000000000").unwrap().timestamp(),
            1_700_000_000
        );
        assert_eq!(
            from_unknown("2021-02-03 04:05:06").unwrap().to_rfc3339(),
            "2021-02-03T04:05:06+00:00"
        );
        assert!(
            (Utc::now() - from_unknown("2 hours 30 minutes ago").unwrap())
                .num_minutes()
                .abs()
                - 150
                < 2
        );
        let y = from_unknown("Yesterday 14:22").unwrap();
        assert_eq!(
            y.date_naive(),
            (Utc::now() - Duration::days(1)).date_naive()
        );
        assert_eq!(
            to_text(from_unknown("2021-02-03").unwrap()),
            "Wed, 3 Feb 2021 00:00:00 +0000"
        );
        assert!(from_unknown("whenever").is_err());
    }

    #[test]
    fn accents_come_off() {
        assert_eq!(strip_diacritics("Amélie à Paris"), "Amelie a Paris");
    }
}
