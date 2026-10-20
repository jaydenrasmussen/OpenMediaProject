//! The calendar as an iCalendar feed, for calendar apps that subscribe to a URL.

use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc};
use omp_db::repo::MovieRecord;

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace(';', "\\;")
        .replace(',', "\\,")
        .replace("\r\n", "\\n")
        .replace('\n', "\\n")
}

/// RFC 5545 asks for lines of at most 75 octets, continued with a space.
fn fold(line: &str) -> String {
    let mut out = String::new();
    let mut len = 0;
    for c in line.chars() {
        let width = c.len_utf8();
        if len + width > 74 {
            out.push_str("\r\n ");
            len = 1;
        }
        out.push(c);
        len += width;
    }
    out
}

fn date(d: NaiveDate) -> String {
    format!("{:04}{:02}{:02}", d.year(), d.month(), d.day())
}

/// Release events from `from` to `to` (inclusive) for the given movies.
pub fn feed(movies: &[MovieRecord], from: NaiveDate, to: NaiveDate, now: DateTime<Utc>) -> String {
    let mut lines = vec![
        "BEGIN:VCALENDAR".to_owned(),
        "VERSION:2.0".to_owned(),
        "PRODID:-//OpenMediaProject//Calendar//EN".to_owned(),
        "CALSCALE:GREGORIAN".to_owned(),
        "X-WR-CALNAME:OpenMediaProject".to_owned(),
    ];
    let stamp = now.format("%Y%m%dT%H%M%SZ").to_string();
    for m in movies {
        let kinds = [
            ("cinemas", "In cinemas", m.metadata.in_cinemas),
            ("digital", "Digital release", m.metadata.digital_release),
            ("physical", "Physical release", m.metadata.physical_release),
        ];
        for (key, label, when) in kinds {
            let Some(when) = when else { continue };
            let day = when.date_naive();
            if day < from || day > to {
                continue;
            }
            lines.push("BEGIN:VEVENT".into());
            lines.push(format!(
                "UID:omp-movie-{}-{key}@openmediaproject",
                m.movie.id
            ));
            lines.push(format!("DTSTAMP:{stamp}"));
            lines.push(format!("DTSTART;VALUE=DATE:{}", date(day)));
            lines.push(format!(
                "DTEND;VALUE=DATE:{}",
                date(day + Duration::days(1))
            ));
            lines.push(format!(
                "SUMMARY:{}",
                escape(&format!("{} ({}) - {label}", m.movie.title, m.movie.year))
            ));
            if let Some(overview) = m.metadata.overview.as_deref().filter(|o| !o.is_empty()) {
                lines.push(format!("DESCRIPTION:{}", escape(overview)));
            }
            lines.push("END:VEVENT".into());
        }
    }
    lines.push("END:VCALENDAR".into());
    let mut out: String = lines.iter().map(|l| fold(l) + "\r\n").collect();
    out.shrink_to_fit();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_escaped_and_long_lines_fold() {
        assert_eq!(escape("a, b; c\nd\\"), "a\\, b\\; c\\nd\\\\");
        let folded = fold(&format!("SUMMARY:{}", "é".repeat(60)));
        for line in folded.split("\r\n") {
            assert!(line.len() <= 75, "{} octets", line.len());
        }
        assert_eq!(
            folded.replace("\r\n ", ""),
            format!("SUMMARY:{}", "é".repeat(60))
        );
    }
}
