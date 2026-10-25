//! Newznab's standard categories, and how a site's own categories map onto them.

use std::collections::{BTreeMap, BTreeSet};

/// Newznab's categories: number and name.
pub const STANDARD: &[(i32, &str)] = &[
    (0, "Other"),
    (10, "Other/Misc"),
    (20, "Other/Hashed"),
    (1000, "Console"),
    (1010, "Console/NDS"),
    (1020, "Console/PSP"),
    (1030, "Console/Wii"),
    (1040, "Console/XBox"),
    (1050, "Console/XBox 360"),
    (1060, "Console/Wiiware"),
    (1070, "Console/XBox 360 DLC"),
    (1080, "Console/PS3"),
    (1090, "Console/Other"),
    (1110, "Console/3DS"),
    (1120, "Console/PS Vita"),
    (1130, "Console/WiiU"),
    (1140, "Console/XBox One"),
    (1180, "Console/PS4"),
    (2000, "Movies"),
    (2010, "Movies/Foreign"),
    (2020, "Movies/Other"),
    (2030, "Movies/SD"),
    (2040, "Movies/HD"),
    (2045, "Movies/UHD"),
    (2050, "Movies/BluRay"),
    (2060, "Movies/3D"),
    (2070, "Movies/DVD"),
    (2080, "Movies/WEB-DL"),
    (2090, "Movies/x265"),
    (3000, "Audio"),
    (3010, "Audio/MP3"),
    (3020, "Audio/Video"),
    (3030, "Audio/Audiobook"),
    (3040, "Audio/Lossless"),
    (3050, "Audio/Other"),
    (3060, "Audio/Foreign"),
    (4000, "PC"),
    (4010, "PC/0day"),
    (4020, "PC/ISO"),
    (4030, "PC/Mac"),
    (4040, "PC/Mobile-Other"),
    (4050, "PC/Games"),
    (4060, "PC/Mobile-iOS"),
    (4070, "PC/Mobile-Android"),
    (5000, "TV"),
    (5010, "TV/WEB-DL"),
    (5020, "TV/Foreign"),
    (5030, "TV/SD"),
    (5040, "TV/HD"),
    (5045, "TV/UHD"),
    (5050, "TV/Other"),
    (5060, "TV/Sport"),
    (5070, "TV/Anime"),
    (5080, "TV/Documentary"),
    (5090, "TV/x265"),
    (6000, "XXX"),
    (6010, "XXX/DVD"),
    (6020, "XXX/WMV"),
    (6030, "XXX/XviD"),
    (6040, "XXX/x264"),
    (6045, "XXX/UHD"),
    (6050, "XXX/Pack"),
    (6060, "XXX/ImageSet"),
    (6070, "XXX/Other"),
    (6080, "XXX/SD"),
    (6090, "XXX/WEB-DL"),
    (7000, "Books"),
    (7010, "Books/Mags"),
    (7020, "Books/EBook"),
    (7030, "Books/Comics"),
    (7040, "Books/Technical"),
    (7050, "Books/Other"),
    (7060, "Books/Foreign"),
    (8000, "Other"),
    (8010, "Other/Misc"),
    (8020, "Other/Hashed"),
];

pub fn by_name(name: &str) -> Option<i32> {
    STANDARD.iter().find(|(_, n)| *n == name).map(|(id, _)| *id)
}

pub fn name_of(id: i32) -> Option<&'static str> {
    STANDARD.iter().find(|(i, _)| *i == id).map(|(_, n)| *n)
}

/// The group a category belongs to: 5040 is under 5000.
pub fn parent_of(id: i32) -> i32 {
    if id >= 100_000 { id } else { id - id % 1000 }
}

#[derive(Debug, Clone, PartialEq)]
struct Mapping {
    tracker: String,
    desc: Option<String>,
    newznab: i32,
}

/// One site's categories.
#[derive(Debug, Clone, Default)]
pub struct Categories {
    mappings: Vec<Mapping>,
    /// Group to the categories of it this site has.
    tree: BTreeMap<i32, BTreeSet<i32>>,
}

impl Categories {
    fn place(&mut self, id: i32) {
        let parent = parent_of(id);
        let entry = self.tree.entry(parent).or_default();
        if id != parent {
            entry.insert(id);
        }
    }

    /// Adds the site's category `tracker`, which is Newznab category `cat`. A
    /// described category also gets its own number (100000 plus the site's).
    pub fn add(&mut self, tracker: &str, cat: Option<i32>, desc: Option<&str>) {
        if let Some(cat) = cat {
            self.mappings.push(Mapping {
                tracker: tracker.to_owned(),
                desc: desc.map(str::to_owned),
                newznab: cat,
            });
            self.place(cat);
        }
        if let Some(desc) = desc {
            let n = tracker.parse::<i32>().unwrap_or_else(|_| {
                // A stable number from the name, as Prowlarr does from its hash.
                tracker
                    .bytes()
                    .fold(5381u32, |h, b| h.wrapping_mul(33) ^ u32::from(b)) as i32
                    & 0xFFFF
            });
            let custom = 100_000 + n;
            self.mappings.push(Mapping {
                tracker: tracker.to_owned(),
                desc: Some(desc.to_owned()),
                newznab: custom,
            });
            self.tree.entry(custom).or_default();
        }
    }

    /// The query's categories with the groups opened into their members.
    pub fn expand(&self, query: &[i32]) -> Vec<i32> {
        let mut out = vec![];
        for q in query {
            out.push(*q);
            if *q < 100_000
                && let Some(children) = self.tree.get(q)
            {
                out.extend(children.iter().copied());
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// The site's categories that answer a query for these Newznab categories.
    pub fn to_tracker(&self, query: &[i32]) -> Vec<String> {
        let expanded = self.expand(query);
        let mut out: Vec<String> = vec![];
        for m in &self.mappings {
            if expanded.contains(&m.newznab) && !out.contains(&m.tracker) {
                out.push(m.tracker.clone());
            }
        }
        out
    }

    /// The Newznab categories of a site category, by its id.
    pub fn from_tracker(&self, tracker: &str) -> Vec<i32> {
        let mut out = vec![];
        for m in &self.mappings {
            if !m.tracker.is_empty()
                && m.tracker.eq_ignore_ascii_case(tracker)
                && !out.contains(&m.newznab)
            {
                out.push(m.newznab);
            }
        }
        out
    }

    /// The Newznab categories of a site category, by its description.
    pub fn from_description(&self, desc: &str) -> Vec<i32> {
        let mut out = vec![];
        for m in &self.mappings {
            if m.desc
                .as_deref()
                .is_some_and(|d| d.eq_ignore_ascii_case(desc))
                && !out.contains(&m.newznab)
            {
                out.push(m.newznab);
            }
        }
        out
    }

    /// The standard groups this site has anything in.
    pub fn groups(&self) -> Vec<i32> {
        self.tree.keys().copied().filter(|k| *k < 100_000).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_site_s_categories_map_both_ways() {
        let mut c = Categories::default();
        c.add("41", by_name("TV/HD"), Some("TV/HD"));
        c.add("5", by_name("TV/SD"), None);
        c.add("42", by_name("Movies/HD"), Some("Movies/HD"));
        // TV, the group, opens to what the site has under it.
        let mut tv = c.to_tracker(&[5000]);
        tv.sort();
        assert_eq!(tv, vec!["41", "5"]);
        assert_eq!(c.to_tracker(&[2040]), vec!["42"]);
        assert_eq!(c.from_tracker("41"), vec![5040, 100_041]);
        assert_eq!(c.from_description("movies/hd"), vec![2040, 100_042]);
        assert_eq!(c.groups(), vec![2000, 5000]);
        assert_eq!(by_name("Movies/UHD"), Some(2045));
        assert_eq!(parent_of(5040), 5000);
    }
}
