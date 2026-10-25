//! Kodi-style `.nfo` files: small XML descriptions that sit beside the video
//! and let media centres show details without looking them up again.

use omp_core::{movie::CreditKind, tv::SeriesStatus};
use omp_db::repo::{EpisodeRecord, MovieRecord, SeriesRecord};

fn esc(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '&' => "&amp;".to_owned(),
            '<' => "&lt;".to_owned(),
            '>' => "&gt;".to_owned(),
            '"' => "&quot;".to_owned(),
            '\'' => "&apos;".to_owned(),
            // XML 1.0 cannot carry most control characters at all.
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => String::new(),
            c => c.to_string(),
        })
        .collect()
}

struct Xml {
    out: String,
}

impl Xml {
    fn new() -> Self {
        Self {
            out: "<?xml version=\"1.0\" encoding=\"utf-8\" standalone=\"yes\"?>\n".into(),
        }
    }

    fn tag(&mut self, name: &str, value: impl AsRef<str>) {
        let v = value.as_ref();
        if !v.trim().is_empty() {
            self.out
                .push_str(&format!("  <{name}>{}</{name}>\n", esc(v)));
        }
    }

    fn raw(&mut self, text: &str) {
        self.out.push_str(text);
    }
}

fn wrap(root: &str, body: &str) -> String {
    let mut x = Xml::new();
    x.raw(&format!("<{root}>\n{body}</{root}>\n"));
    x.out
}

fn body(f: impl FnOnce(&mut Xml)) -> String {
    let mut x = Xml { out: String::new() };
    f(&mut x);
    x.out
}

fn unique(x: &mut Xml, kind: &str, id: &str, default: bool) {
    if !id.is_empty() {
        x.raw(&format!(
            "  <uniqueid type=\"{kind}\"{}>{}</uniqueid>\n",
            if default { " default=\"true\"" } else { "" },
            esc(id)
        ));
    }
}

pub fn movie_nfo(m: &MovieRecord) -> String {
    let meta = &m.metadata;
    wrap(
        "movie",
        &body(|x| {
            x.tag("title", &meta.title);
            x.tag("originaltitle", &meta.original_title);
            if meta.year > 0 {
                x.tag("year", meta.year.to_string());
            }
            x.tag("plot", meta.overview.clone().unwrap_or_default());
            if meta.runtime > 0 {
                x.tag("runtime", meta.runtime.to_string());
            }
            x.tag("mpaa", meta.certification.clone().unwrap_or_default());
            for g in &meta.genres {
                x.tag("genre", g);
            }
            x.tag("studio", meta.studio.clone().unwrap_or_default());
            if let Some(d) = meta
                .digital_release
                .or(meta.physical_release)
                .or(meta.in_cinemas)
            {
                x.tag("premiered", d.format("%Y-%m-%d").to_string());
            }
            if let Some(r) = &meta.ratings {
                x.raw(&format!(
                    "  <ratings>\n    <rating name=\"tmdb\" default=\"true\"><value>{:.1}</value><votes>{}</votes></rating>\n  </ratings>\n",
                    r.value, r.votes
                ));
            }
            x.tag("set", meta.collection_title.clone().unwrap_or_default());
            unique(x, "tmdb", &meta.tmdb_id.to_string(), true);
            unique(
                x,
                "imdb",
                meta.imdb_id.as_deref().unwrap_or_default(),
                false,
            );
            for c in meta
                .credits
                .iter()
                .filter(|c| c.kind == CreditKind::Cast)
                .take(30)
            {
                x.raw(&format!(
                    "  <actor>\n    <name>{}</name>\n    <role>{}</role>\n    <order>{}</order>\n  </actor>\n",
                    esc(&c.name),
                    esc(&c.role),
                    c.order
                ));
            }
        }),
    )
}

pub fn series_nfo(s: &SeriesRecord) -> String {
    let i = &s.info;
    wrap(
        "tvshow",
        &body(|x| {
            x.tag("title", &i.title);
            x.tag("originaltitle", &i.original_title);
            if i.year > 0 {
                x.tag("year", i.year.to_string());
            }
            x.tag("plot", i.overview.clone().unwrap_or_default());
            if i.runtime > 0 {
                x.tag("runtime", i.runtime.to_string());
            }
            x.tag("mpaa", i.certification.clone().unwrap_or_default());
            for g in &i.genres {
                x.tag("genre", g);
            }
            x.tag("studio", i.network.clone().unwrap_or_default());
            if let Some(d) = i.first_aired {
                x.tag("premiered", d.to_string());
            }
            x.tag(
                "status",
                match i.status {
                    SeriesStatus::Ended => "Ended",
                    SeriesStatus::Continuing => "Continuing",
                    SeriesStatus::Upcoming => "Upcoming",
                    SeriesStatus::Deleted => "Deleted",
                },
            );
            if let Some(r) = &i.ratings {
                x.raw(&format!(
                    "  <ratings>\n    <rating name=\"tmdb\" default=\"true\"><value>{:.1}</value><votes>{}</votes></rating>\n  </ratings>\n",
                    r.value, r.votes
                ));
            }
            unique(x, "tmdb", &i.tmdb_id.to_string(), true);
            unique(
                x,
                "tvdb",
                &i.tvdb_id.map(|t| t.to_string()).unwrap_or_default(),
                false,
            );
            unique(x, "imdb", i.imdb_id.as_deref().unwrap_or_default(), false);
        }),
    )
}

/// One file's NFO. A file holding several episodes gets one `episodedetails`
/// block per episode, as Kodi expects.
pub fn episode_nfo(s: &SeriesRecord, episodes: &[EpisodeRecord]) -> String {
    let mut x = Xml::new();
    for e in episodes {
        let block = body(|b| {
            b.tag("title", &e.title);
            b.tag("showtitle", &s.info.title);
            b.tag("season", e.season_number.to_string());
            b.tag("episode", e.episode_number.to_string());
            if let Some(d) = e.air_date {
                b.tag("aired", d.to_string());
            }
            b.tag("plot", e.overview.clone().unwrap_or_default());
            if e.runtime > 0 {
                b.tag("runtime", e.runtime.to_string());
            }
            if e.tmdb_id > 0 {
                unique(b, "tvdb", &e.tmdb_id.to_string(), true);
            }
        });
        x.raw(&format!("<episodedetails>\n{block}</episodedetails>\n"));
    }
    x.out
}

/// Writes `text` unless the file already holds exactly it. Returns whether
/// the file changed.
pub async fn write_if_changed(path: &std::path::Path, text: &str) -> std::io::Result<bool> {
    if tokio::fs::read_to_string(path)
        .await
        .is_ok_and(|old| old == text)
    {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(path, text).await?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_escaped_and_empty_fields_left_out() {
        assert_eq!(
            esc("Tom & Jerry <\"1\">\u{1}"),
            "Tom &amp; Jerry &lt;&quot;1&quot;&gt;"
        );
        let mut x = Xml { out: String::new() };
        x.tag("a", "  ");
        x.tag("b", "x");
        assert_eq!(x.out, "  <b>x</b>\n");
    }
}
