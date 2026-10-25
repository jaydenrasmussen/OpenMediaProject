//! Sending decided TV releases to download clients. Ported from Sonarr
//! `ProcessDownloadDecisions` and `DownloadService` (GPL-3.0).

use std::{
    cmp::Ordering,
    collections::{BTreeMap, HashSet},
};

use omp_core::{history::HistoryEvent, release::DownloadProtocol};
use omp_db::{
    Db,
    repo::{EpisodeHistoryRecord, PendingReason, ProviderKind},
};
use omp_downloads::{AddRequest, ClientError, ClientRegistry};

use crate::{
    evaluate::ServiceError,
    grab::{GrabContext, GrabbedRelease, ProcessedDecisions},
    tv_evaluate::{EvaluatedTvRelease, TvEvaluation},
};

enum GrabResult {
    Grabbed(GrabbedRelease),
    Rejected,
    Failed,
    Skipped,
}

fn overlaps(items: &[EvaluatedTvRelease], done: impl Iterator<Item = usize>, index: usize) -> bool {
    let ids = items[index].episode_ids();
    done.into_iter()
        .any(|d| items[d].episodes.iter().any(|e| ids.contains(&e.id)))
}

/// Grabs the best release for each set of episodes.
///
/// Releases are tried best first, series by series. Once an episode is
/// grabbed, other releases containing it are skipped. Delayed releases and
/// those that no client could take become pending; later ones for episodes
/// that already have a grab or pending release are stored as fallbacks.
pub async fn process_tv_decisions(
    db: &Db,
    evaluation: &TvEvaluation,
    registry: &ClientRegistry,
    ctx: &GrabContext,
) -> Result<ProcessedDecisions, ServiceError> {
    let items = &evaluation.items;
    let mut out = ProcessedDecisions {
        rejected: items
            .iter()
            .enumerate()
            .filter(|(_, it)| it.decision.rejected())
            .map(|(i, _)| i)
            .collect(),
        ..Default::default()
    };
    let mut queue: Vec<(usize, PendingReason)> = Vec::new();
    let (mut usenet_failed, mut torrent_failed) = (false, false);

    fn prepare(
        items: &[EvaluatedTvRelease],
        queue: &mut Vec<(usize, PendingReason)>,
        grabbed: &[GrabbedRelease],
        pending: &mut Vec<usize>,
        index: usize,
        mut reason: PendingReason,
    ) {
        if overlaps(items, grabbed.iter().map(|g| g.index), index)
            || overlaps(items, pending.iter().copied(), index)
        {
            reason = PendingReason::Fallback;
        }
        queue.push((index, reason));
        pending.push(index);
    }

    for index in evaluation.qualified_best_first() {
        let protocol = items[index].release.protocol;
        if overlaps(items, out.grabbed.iter().map(|g| g.index), index) {
            continue;
        }
        if items[index].decision.temporarily_rejected() {
            prepare(
                items,
                &mut queue,
                &out.grabbed,
                &mut out.pending,
                index,
                PendingReason::Delay,
            );
            continue;
        }
        if (protocol == DownloadProtocol::Usenet && usenet_failed)
            || (protocol == DownloadProtocol::Torrent && torrent_failed)
        {
            prepare(
                items,
                &mut queue,
                &out.grabbed,
                &mut out.pending,
                index,
                PendingReason::DownloadClientUnavailable,
            );
            continue;
        }
        match grab_one(db, &items[index], registry, ctx, index).await? {
            GrabResult::Grabbed(g) => out.grabbed.push(g),
            GrabResult::Rejected => out.rejected.push(index),
            GrabResult::Skipped => {}
            GrabResult::Failed => {
                prepare(
                    items,
                    &mut queue,
                    &out.grabbed,
                    &mut out.pending,
                    index,
                    PendingReason::DownloadClientUnavailable,
                );
                match protocol {
                    DownloadProtocol::Usenet => usenet_failed = true,
                    DownloadProtocol::Torrent => torrent_failed = true,
                    DownloadProtocol::Unknown => {}
                }
            }
        }
    }

    for g in &out.grabbed {
        remove_grabbed_pending(db, &items[g.index]).await?;
    }
    for (index, reason) in queue {
        let item = &items[index];
        if let Some(series) = &item.series {
            db.pending_tv_releases()
                .add_or_update(series.id, &item.episode_ids(), &item.release, reason)
                .await?;
        }
    }
    Ok(out)
}

async fn grab_one(
    db: &Db,
    item: &EvaluatedTvRelease,
    registry: &ClientRegistry,
    ctx: &GrabContext,
    index: usize,
) -> Result<GrabResult, ServiceError> {
    let Some(series) = item.series.as_ref() else {
        return Ok(GrabResult::Skipped);
    };
    let blocked: HashSet<i64> = db
        .providers()
        .blocked(ProviderKind::DownloadClient, ctx.now)
        .await?
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    let client = match registry.select(item.release.protocol, &series.tags, false, &blocked) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("no download client for {}: {e}", item.release.title);
            return Ok(GrabResult::Failed);
        }
    };
    let request = AddRequest {
        release: &item.release,
        category: ctx.category.as_deref(),
    };
    match client.client.add(&request).await {
        Ok(download_id) => {
            db.providers().record_success(client.id).await?;
            if item.release.indexer_id > 0 {
                db.providers()
                    .record_success(i64::from(item.release.indexer_id))
                    .await?;
            }
            record_grab(db, item, &download_id, &client.name, ctx).await?;
            Ok(GrabResult::Grabbed(GrabbedRelease {
                index,
                download_id,
                client_id: client.id,
                client_name: client.name.clone(),
            }))
        }
        Err(ClientError::ReleaseUnavailable(why)) => {
            tracing::warn!("release {} is not available: {why}", item.release.title);
            Ok(GrabResult::Rejected)
        }
        Err(ClientError::ReleaseDownloadFailed {
            message,
            retry_after,
        }) => {
            tracing::warn!("could not download {}: {message}", item.release.title);
            if item.release.indexer_id > 0 {
                db.providers()
                    .record_failure(
                        i64::from(item.release.indexer_id),
                        ctx.now,
                        ctx.process_start,
                        retry_after.map(|d| chrono::Duration::from_std(d).unwrap_or_default()),
                        true,
                    )
                    .await?;
            }
            Ok(GrabResult::Skipped)
        }
        Err(e) if e.is_client_failure() => {
            tracing::warn!("{} could not take {}: {e}", client.name, item.release.title);
            db.providers()
                .record_failure(client.id, ctx.now, ctx.process_start, None, true)
                .await?;
            Ok(GrabResult::Failed)
        }
        Err(e) => {
            tracing::warn!(
                "could not add {} to {}: {e}",
                item.release.title,
                client.name
            );
            Ok(GrabResult::Skipped)
        }
    }
}

/// One "grabbed" history row per episode in the release.
async fn record_grab(
    db: &Db,
    item: &EvaluatedTvRelease,
    download_id: &str,
    client_name: &str,
    ctx: &GrabContext,
) -> Result<(), ServiceError> {
    let (Some(series), Some(parsed)) = (item.series.as_ref(), item.parsed.as_ref()) else {
        return Ok(());
    };
    let r = &item.release;
    let mut data: BTreeMap<String, String> = BTreeMap::new();
    let mut put = |k: &str, v: String| {
        data.insert(k.to_owned(), v);
    };
    put("indexer", r.indexer.clone());
    put("indexer_id", r.indexer_id.to_string());
    put("size", r.size.to_string());
    put("indexer_flags", r.indexer_flags.bits().to_string());
    put("protocol", format!("{:?}", r.protocol));
    put("download_client", client_name.to_owned());
    put("download_url", r.download_url.clone());
    put("guid", r.guid.clone());
    put("published_date", r.publish_date.to_rfc3339());
    put("age_minutes", format!("{:.0}", r.age_minutes(ctx.now)));
    put("custom_format_score", item.custom_format_score.to_string());
    put("release_source", format!("{:?}", item.source));
    if parsed.full_season {
        put("release_type", "SeasonPack".into());
    } else if item.episodes.len() > 1 {
        put("release_type", "MultiEpisode".into());
    } else {
        put("release_type", "SingleEpisode".into());
    }
    if let Some(group) = &parsed.release_group {
        put("release_group", group.clone());
    }
    if let Some(hash) = r.torrent.as_ref().and_then(|t| t.info_hash.as_ref()) {
        put("torrent_info_hash", hash.clone());
    }
    for e in &item.episodes {
        db.episode_history()
            .insert(&EpisodeHistoryRecord {
                id: 0,
                series_id: series.id,
                episode_id: e.id,
                event: HistoryEvent::Grabbed,
                date: ctx.now,
                source_title: r.title.clone(),
                quality: parsed.quality,
                languages: parsed.languages.clone(),
                download_id: Some(download_id.to_owned()),
                data: data.clone(),
            })
            .await?;
    }
    Ok(())
}

/// Once a release is grabbed, waiting releases for the same episodes that are
/// no better have no further use.
async fn remove_grabbed_pending(db: &Db, grabbed: &EvaluatedTvRelease) -> Result<(), ServiceError> {
    let (Some(series), Some(parsed), Some(profile)) = (
        grabbed.series.as_ref(),
        grabbed.parsed.as_ref(),
        grabbed.profile.as_ref(),
    ) else {
        return Ok(());
    };
    let ids = grabbed.episode_ids();
    for p in db.pending_tv_releases().for_series(series.id).await? {
        if !p.episode_ids.iter().any(|id| ids.contains(id)) {
            continue;
        }
        let Some(pending) = omp_parser::tv::parse_title(&p.release.title) else {
            continue;
        };
        if profile
            .comparer()
            .compare_model(&parsed.quality, &pending.quality)
            != Ordering::Less
        {
            db.pending_tv_releases().remove(p.id).await?;
        }
    }
    Ok(())
}

/// Sends one chosen release regardless of the decision: the person picked it.
pub async fn grab_selected_tv(
    db: &Db,
    evaluation: &TvEvaluation,
    registry: &ClientRegistry,
    ctx: &GrabContext,
    index: usize,
) -> Result<GrabbedRelease, String> {
    let item = &evaluation.items[index];
    let Some(series) = item.series.as_ref() else {
        return Err("this release could not be matched to a series in the library".into());
    };
    if item.episodes.is_empty() {
        return Err("this release could not be matched to any episodes".into());
    }
    let db_err = |e: omp_db::DbError| e.to_string();
    let blocked: HashSet<i64> = db
        .providers()
        .blocked(ProviderKind::DownloadClient, ctx.now)
        .await
        .map_err(db_err)?
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    let client = registry
        .select(item.release.protocol, &series.tags, false, &blocked)
        .map_err(|e| e.to_string())?;
    let request = AddRequest {
        release: &item.release,
        category: ctx.category.as_deref(),
    };
    match client.client.add(&request).await {
        Ok(download_id) => {
            db.providers()
                .record_success(client.id)
                .await
                .map_err(db_err)?;
            if item.release.indexer_id > 0 {
                db.providers()
                    .record_success(i64::from(item.release.indexer_id))
                    .await
                    .map_err(db_err)?;
            }
            record_grab(db, item, &download_id, &client.name, ctx)
                .await
                .map_err(|e| e.to_string())?;
            remove_grabbed_pending(db, item)
                .await
                .map_err(|e| e.to_string())?;
            Ok(GrabbedRelease {
                index,
                download_id,
                client_id: client.id,
                client_name: client.name.clone(),
            })
        }
        Err(e) => {
            if e.is_client_failure() {
                db.providers()
                    .record_failure(client.id, ctx.now, ctx.process_start, None, true)
                    .await
                    .map_err(db_err)?;
            }
            Err(e.to_string())
        }
    }
}
