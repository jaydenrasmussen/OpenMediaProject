use serde::{Deserialize, Serialize};

/// Newznab (usenet) or Torznab (torrent). Same protocol, different payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flavor {
    Newznab,
    Torznab,
}

/// The JSON stored in a provider row's `settings` column.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct IndexerSettings {
    pub base_url: String,
    pub api_path: String,
    pub api_key: String,
    /// Newznab category ids; 2000 is Movies and 2010..2060 its sub-categories.
    pub categories: Vec<i32>,
    /// Categories for TV searches; 5030 is TV/SD and 5040 TV/HD.
    pub tv_categories: Vec<i32>,
    /// Categories for anime searches (5070 is TV/Anime). Empty skips anime.
    pub anime_categories: Vec<i32>,
    /// Also search anime by season and episode, not only absolute number.
    pub anime_standard_format_search: bool,
    /// TV: in a season search, single episodes of a season whose last episode
    /// aired more than this many days ago are refused (the pack is wanted).
    pub season_search_maximum_single_episode_age: u32,
    /// Extra query parameters, each starting with `&`.
    pub additional_parameters: String,
    /// Leave the year out of text searches.
    pub remove_year: bool,
    /// Torznab only: skip releases with fewer seeders.
    pub minimum_seeders: i32,
    /// Torznab only: a release needs at least one of these flag bits.
    pub required_flags: Vec<u32>,
}

impl Default for IndexerSettings {
    fn default() -> Self {
        Self {
            base_url: String::new(),
            api_path: "/api".into(),
            api_key: String::new(),
            categories: vec![2000, 2010, 2020, 2030, 2040, 2045, 2050, 2060],
            tv_categories: vec![5030, 5040],
            anime_categories: vec![],
            anime_standard_format_search: false,
            season_search_maximum_single_episode_age: 0,
            additional_parameters: String::new(),
            remove_year: false,
            minimum_seeders: 1,
            required_flags: vec![],
        }
    }
}

impl IndexerSettings {
    /// Checks what Radarr checks before saving.
    pub fn validate(&self) -> Result<(), String> {
        if self.categories.is_empty()
            && self.tv_categories.is_empty()
            && self.anime_categories.is_empty()
        {
            return Err("'Categories' must be provided".into());
        }
        let url = url::Url::parse(&self.base_url).map_err(|e| format!("invalid base URL: {e}"))?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err("base URL must be http or https".into());
        }
        if !self.api_path.starts_with('/') {
            return Err("API path must start with '/'".into());
        }
        if !self.additional_parameters.is_empty() && !self.additional_parameters.starts_with('&') {
            return Err("additional parameters must look like &key=value".into());
        }
        // Radarr insists on a key for a few well-known public indexers.
        const NEEDS_KEY: [&str; 7] = [
            "nzbs.org",
            "nzb.su",
            "dognzb.cr",
            "nzbplanet.net",
            "nzbid.org",
            "nzbndx.com",
            "nzbindex.in",
        ];
        let lower = self.base_url.to_lowercase();
        if self.api_key.is_empty() && NEEDS_KEY.iter().any(|h| lower.contains(h)) {
            return Err("this indexer requires an API key".into());
        }
        Ok(())
    }

    /// `base_url` + `api_path` without trailing slashes.
    pub fn api_url(&self) -> String {
        format!(
            "{}{}",
            self.base_url.trim_end_matches('/'),
            self.api_path.trim_end_matches('/')
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid() -> IndexerSettings {
        IndexerSettings {
            base_url: "https://idx.example".into(),
            ..Default::default()
        }
    }

    #[test]
    fn defaults_and_url() {
        let s = valid();
        assert_eq!(s.api_url(), "https://idx.example/api");
        let s = IndexerSettings {
            base_url: "https://idx.example/".into(),
            api_path: "/api/".into(),
            ..Default::default()
        };
        assert_eq!(s.api_url(), "https://idx.example/api");
        assert!(valid().validate().is_ok());
    }

    #[test]
    fn validation() {
        let mut s = valid();
        s.categories.clear();
        assert!(s.validate().is_ok(), "TV categories are enough");
        s.tv_categories.clear();
        s.anime_categories.clear();
        assert!(s.validate().is_err());
        assert!(
            IndexerSettings::default().validate().is_err(),
            "no base url"
        );
        let mut s = valid();
        s.base_url = "ftp://idx.example".into();
        assert!(s.validate().is_err());
        let mut s = valid();
        s.additional_parameters = "x=1".into();
        assert!(s.validate().is_err());
        s.additional_parameters = "&x=1".into();
        assert!(s.validate().is_ok());
        let mut s = valid();
        s.base_url = "https://api.nzb.su".into();
        assert!(s.validate().is_err(), "key required for nzb.su");
        s.api_key = "k".into();
        assert!(s.validate().is_ok());
    }

    #[test]
    fn settings_json_fills_in_defaults() {
        let s: IndexerSettings = serde_json::from_str(r#"{"base_url":"https://x"}"#).unwrap();
        assert_eq!(s.api_path, "/api");
        assert_eq!(s.minimum_seeders, 1);
    }
}
