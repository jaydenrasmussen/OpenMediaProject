//! Keeps scene numbering and alternate scene titles up to date. Ported from
//! Sonarr's `XemService` and `SceneMappingService` (GPL-3.0).

use omp_db::{Db, repo::SceneTitleRow};
use omp_metadata::scene::SceneSource;

use crate::evaluate::ServiceError;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct SceneRefresh {
    pub titles: usize,
    /// Series that now have scene numbers.
    pub renumbered_series: usize,
    /// Episodes given scene numbers.
    pub renumbered_episodes: usize,
}

/// Replaces the stored scene titles with the source's current list.
pub async fn refresh_scene_titles(
    db: &Db,
    source: &dyn SceneSource,
) -> Result<usize, ServiceError> {
    let titles = source.scene_titles().await?;
    let rows: Vec<SceneTitleRow> = titles
        .into_iter()
        .map(|t| SceneTitleRow {
            tvdb_id: t.tvdb_id,
            title: t.title,
            search_title: t.search_title,
            season: t.season,
        })
        .collect();
    db.scene_titles().replace_all(&rows).await?;
    Ok(rows.len())
}

/// Reads TheXEM's numbering for the series that have a TVDB id, or just
/// `only`. Series the XEM does not know lose scene numbers they had.
///
/// The XEM maps TVDB numbering; our episodes carry TMDB numbering, which is
/// the same for most series. Where it differs the mapping simply finds no
/// episode and nothing is changed for it.
pub async fn refresh_scene_numbering(
    db: &Db,
    source: &dyn SceneSource,
    only: Option<i64>,
) -> Result<(usize, usize), ServiceError> {
    let known = source.xem_known_ids().await?;
    let series = match only {
        Some(id) => vec![db.series().get(id).await?],
        None => db.series().list().await?,
    };
    let (mut renumbered_series, mut renumbered_episodes) = (0, 0);
    for s in series {
        let Some(tvdb) = s.info.tvdb_id else { continue };
        let episodes = db.episodes().for_series(s.id).await?;
        let mapped = if known.contains(&tvdb) {
            source.xem_map(tvdb).await?
        } else {
            Vec::new()
        };
        let mut count = 0;
        for e in &episodes {
            let hit = mapped
                .iter()
                .find(|m| m.tvdb.season == e.season_number && m.tvdb.episode == e.episode_number);
            let want = hit.map(|m| (m.scene.season, m.scene.episode, m.scene.absolute));
            let have = match (e.scene_season_number, e.scene_episode_number) {
                (Some(a), Some(b)) => Some((a, b, e.scene_absolute_episode_number)),
                _ => None,
            };
            if want != have {
                let (a, b, c) = want.map_or((None, None, None), |(a, b, c)| (Some(a), Some(b), c));
                db.episodes().set_scene_numbers(e.id, a, b, c).await?;
            }
            count += usize::from(want.is_some());
        }
        if count > 0 {
            renumbered_series += 1;
            renumbered_episodes += count;
        }
    }
    Ok((renumbered_series, renumbered_episodes))
}

/// Both refreshes.
pub async fn refresh_scene_data(
    db: &Db,
    source: &dyn SceneSource,
) -> Result<SceneRefresh, ServiceError> {
    let titles = refresh_scene_titles(db, source).await?;
    let (renumbered_series, renumbered_episodes) =
        refresh_scene_numbering(db, source, None).await?;
    Ok(SceneRefresh {
        titles,
        renumbered_series,
        renumbered_episodes,
    })
}
