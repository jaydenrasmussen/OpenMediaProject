//! Delay profiles: wait a while for a better release, per protocol, per tag set.

use omp_core::release::DownloadProtocol;

#[derive(Debug, Clone, PartialEq)]
pub struct DelayProfile {
    pub id: i32,
    pub enable_usenet: bool,
    pub enable_torrent: bool,
    pub preferred_protocol: DownloadProtocol,
    /// Minutes.
    pub usenet_delay: i32,
    /// Minutes.
    pub torrent_delay: i32,
    /// Lower is matched first.
    pub order: i32,
    pub bypass_if_highest_quality: bool,
    pub bypass_if_above_custom_format_score: bool,
    pub minimum_custom_format_score: i32,
    /// Empty means "applies to everything".
    pub tags: Vec<i32>,
}

impl DelayProfile {
    /// The catch-all profile every install has: both protocols, no delay.
    pub fn default_profile() -> Self {
        Self {
            id: 1,
            enable_usenet: true,
            enable_torrent: true,
            preferred_protocol: DownloadProtocol::Usenet,
            usenet_delay: 0,
            torrent_delay: 0,
            order: i32::MAX,
            bypass_if_highest_quality: false,
            bypass_if_above_custom_format_score: false,
            minimum_custom_format_score: 0,
            tags: Vec::new(),
        }
    }

    /// Delay in minutes for a protocol.
    pub fn protocol_delay(&self, protocol: DownloadProtocol) -> i32 {
        if protocol == DownloadProtocol::Torrent {
            self.torrent_delay
        } else {
            self.usenet_delay
        }
    }
}

/// The first profile, by `order`, that has no tags or shares a tag with `tags`.
pub fn best_for_tags<'a>(profiles: &'a [DelayProfile], tags: &[i32]) -> Option<&'a DelayProfile> {
    profiles
        .iter()
        .filter(|p| p.tags.is_empty() || p.tags.iter().any(|t| tags.contains(t)))
        .min_by_key(|p| p.order)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(id: i32, order: i32, tags: &[i32]) -> DelayProfile {
        DelayProfile {
            id,
            order,
            tags: tags.to_vec(),
            ..DelayProfile::default_profile()
        }
    }

    #[test]
    fn picks_lowest_order_among_matching() {
        let all = vec![
            profile(1, i32::MAX, &[]),
            profile(2, 2, &[7]),
            profile(3, 1, &[9]),
        ];
        assert_eq!(best_for_tags(&all, &[7]).unwrap().id, 2);
        assert_eq!(best_for_tags(&all, &[9, 7]).unwrap().id, 3);
        assert_eq!(
            best_for_tags(&all, &[]).unwrap().id,
            1,
            "untagged movie gets the catch-all"
        );
        assert_eq!(best_for_tags(&all, &[42]).unwrap().id, 1);
    }

    #[test]
    fn no_profiles_is_none() {
        assert!(best_for_tags(&[], &[1]).is_none());
    }

    #[test]
    fn protocol_delay() {
        let mut p = DelayProfile::default_profile();
        p.usenet_delay = 10;
        p.torrent_delay = 60;
        assert_eq!(p.protocol_delay(DownloadProtocol::Usenet), 10);
        assert_eq!(p.protocol_delay(DownloadProtocol::Torrent), 60);
    }
}
