//! NZB files: a list of files, each a list of article segments.

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum NzbError {
    #[error("not a valid NZB: {0}")]
    Invalid(String),
    #[error("the NZB contains no files")]
    Empty,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NzbSegment {
    /// 1-based position in the file.
    pub number: u32,
    /// Encoded size in bytes, as the NZB says.
    pub bytes: u64,
    /// Message-id without the angle brackets.
    pub message_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NzbFile {
    pub poster: String,
    pub subject: String,
    /// The file name taken from the subject.
    pub name: String,
    pub groups: Vec<String>,
    pub segments: Vec<NzbSegment>,
}

impl NzbFile {
    /// Encoded size of all segments; the real file is a little smaller.
    pub fn encoded_bytes(&self) -> u64 {
        self.segments.iter().map(|s| s.bytes).sum()
    }

    pub fn is_par2(&self) -> bool {
        self.name.to_ascii_lowercase().ends_with(".par2")
    }

    pub fn is_par2_recovery_volume(&self) -> bool {
        self.is_par2() && self.name.to_ascii_lowercase().contains(".vol")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nzb {
    pub files: Vec<NzbFile>,
}

impl Nzb {
    pub fn total_bytes(&self) -> u64 {
        self.files.iter().map(NzbFile::encoded_bytes).sum()
    }

    pub fn segment_count(&self) -> usize {
        self.files.iter().map(|f| f.segments.len()).sum()
    }

    pub fn parse(xml: &str) -> Result<Nzb, NzbError> {
        // Some NZBs carry a DOCTYPE, which roxmltree refuses by default.
        let options = roxmltree::ParsingOptions {
            allow_dtd: true,
            ..Default::default()
        };
        let doc = roxmltree::Document::parse_with_options(xml, options)
            .map_err(|e| NzbError::Invalid(e.to_string()))?;
        let root = doc.root_element();
        if root.tag_name().name() != "nzb" {
            return Err(NzbError::Invalid(format!(
                "root element is <{}>",
                root.tag_name().name()
            )));
        }

        let mut files = Vec::new();
        for file in root.children().filter(|n| n.has_tag_name("file")) {
            let subject = file.attribute("subject").unwrap_or_default().to_owned();
            let groups = file
                .children()
                .find(|n| n.has_tag_name("groups"))
                .map(|g| {
                    g.children()
                        .filter(|n| n.has_tag_name("group"))
                        .filter_map(|n| n.text())
                        .map(|t| t.trim().to_owned())
                        .collect()
                })
                .unwrap_or_default();

            let mut segments: Vec<NzbSegment> = file
                .children()
                .find(|n| n.has_tag_name("segments"))
                .map(|s| {
                    s.children()
                        .filter(|n| n.has_tag_name("segment"))
                        .filter_map(|n| {
                            let id = n.text()?.trim().trim_matches(['<', '>']).to_owned();
                            if id.is_empty() {
                                return None;
                            }
                            Some(NzbSegment {
                                number: n.attribute("number")?.parse().ok()?,
                                bytes: n
                                    .attribute("bytes")
                                    .and_then(|b| b.parse().ok())
                                    .unwrap_or(0),
                                message_id: id,
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            if segments.is_empty() {
                continue;
            }
            segments.sort_by_key(|s| s.number);

            files.push(NzbFile {
                poster: file.attribute("poster").unwrap_or_default().to_owned(),
                name: file_name_from_subject(&subject),
                subject,
                groups,
                segments,
            });
        }
        if files.is_empty() {
            return Err(NzbError::Empty);
        }
        Ok(Nzb { files })
    }

    /// A release name for the NZB: the `<meta type="name">` if present.
    pub fn title_from(xml: &str) -> Option<String> {
        let doc = roxmltree::Document::parse_with_options(
            xml,
            roxmltree::ParsingOptions {
                allow_dtd: true,
                ..Default::default()
            },
        )
        .ok()?;
        doc.descendants()
            .find(|n| n.has_tag_name("meta") && n.attribute("type") == Some("name"))
            .and_then(|n| n.text())
            .map(|t| t.trim().to_owned())
            .filter(|t| !t.is_empty())
    }
}

/// Posters put the file name in quotes: `Some Release [01/10] - "file.rar" yEnc (1/50)`.
/// Without quotes, falls back to the token that looks like a file name.
pub fn file_name_from_subject(subject: &str) -> String {
    if let Some(start) = subject.find('"')
        && let Some(len) = subject[start + 1..].find('"')
    {
        let name = &subject[start + 1..start + 1 + len];
        if !name.trim().is_empty() {
            return name.trim().to_owned();
        }
    }
    let looks_like_file = |t: &&str| {
        t.rsplit_once('.').is_some_and(|(stem, ext)| {
            !stem.is_empty()
                && (2..=5).contains(&ext.len())
                && ext.chars().all(|c| c.is_ascii_alphanumeric())
        })
    };
    subject
        .split_whitespace()
        .rfind(looks_like_file)
        .map(str::to_owned)
        .unwrap_or_else(|| subject.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE nzb PUBLIC "-//newzBin//DTD NZB 1.1//EN" "http://www.newzbin.com/DTD/nzb/nzb-1.1.dtd">
<nzb xmlns="http://www.newzbin.com/DTD/2003/nzb">
  <head><meta type="name">Some.Movie.2020.1080p</meta></head>
  <file poster="poster@example" date="1700000000" subject="[1/3] - &quot;movie.part2.rar&quot; yEnc (1/2)">
    <groups><group>alt.binaries.movies</group><group>alt.binaries.x</group></groups>
    <segments>
      <segment bytes="700000" number="2">second@news</segment>
      <segment bytes="710000" number="1">&lt;first@news&gt;</segment>
    </segments>
  </file>
  <file poster="p" date="1" subject="[2/3] - &quot;movie.par2&quot; yEnc (1/1)">
    <groups><group>alt.binaries.movies</group></groups>
    <segments><segment bytes="5000" number="1">par@news</segment></segments>
  </file>
  <file poster="p" date="1" subject="no segments here"><segments></segments></file>
</nzb>"#;

    #[test]
    fn parses_files_segments_and_groups() {
        let nzb = Nzb::parse(SAMPLE).unwrap();
        assert_eq!(nzb.files.len(), 2, "the file without segments is dropped");
        let f = &nzb.files[0];
        assert_eq!(f.name, "movie.part2.rar");
        assert_eq!(f.groups, ["alt.binaries.movies", "alt.binaries.x"]);
        assert_eq!(
            f.segments.iter().map(|s| s.number).collect::<Vec<_>>(),
            [1, 2],
            "sorted"
        );
        assert_eq!(
            f.segments[0].message_id, "first@news",
            "angle brackets stripped"
        );
        assert_eq!(f.encoded_bytes(), 1_410_000);
        assert_eq!(nzb.total_bytes(), 1_415_000);
        assert_eq!(nzb.segment_count(), 3);
        assert!(nzb.files[1].is_par2() && !nzb.files[1].is_par2_recovery_volume());
        assert_eq!(
            Nzb::title_from(SAMPLE).as_deref(),
            Some("Some.Movie.2020.1080p")
        );
    }

    #[test]
    fn rejects_garbage_and_empty() {
        assert!(matches!(Nzb::parse("hello"), Err(NzbError::Invalid(_))));
        assert!(matches!(Nzb::parse("<html/>"), Err(NzbError::Invalid(_))));
        assert_eq!(Nzb::parse("<nzb></nzb>"), Err(NzbError::Empty));
    }

    #[test]
    fn file_names_from_subjects() {
        for (subject, name) in [
            (r#"[01/10] - "My File.mkv" yEnc (1/50)"#, "My File.mkv"),
            (r#"Release "a.b.vol01+02.par2" yEnc"#, "a.b.vol01+02.par2"),
            ("Release.Name.2020.mkv yEnc (1/5)", "Release.Name.2020.mkv"),
            ("just words", "just words"),
        ] {
            assert_eq!(file_name_from_subject(subject), name, "{subject}");
        }
        let vol = NzbFile {
            poster: String::new(),
            subject: String::new(),
            name: "x.vol01+02.PAR2".into(),
            groups: vec![],
            segments: vec![],
        };
        assert!(vol.is_par2_recovery_volume());
    }
}
