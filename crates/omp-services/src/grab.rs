//! Sending decided releases to download clients. Ported from Radarr
//! `ProcessDownloadDecisions` and `DownloadService` (GPL-3.0).

use std::collections::{BTreeMap, HashSet};

use chrono::{DateTime, Duration, Utc};
use omp_core::{history::HistoryEvent, release::DownloadProtocol};
use omp_db::{
    Db,
    repo::{HistoryRecord, PendingReason, ProviderKind},
};
use omp_downloads::{AddRequest, ClientError, ClientRegistry};

use crate::evaluate::{EvaluatedRelease, Evaluation, ServiceError};

/// What happened to the releases of one batch, as indices into
/// [`Evaluation::items`].
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ProcessedDecisions {
    pub grabbed: Vec<GrabbedRelease>,
    /// Held for later: waiting out a delay, or no client could take them.
    pub pending: Vec<usize>,
    /// Permanently rejected, or found to be gone when we tried.
    pub rejected: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrabbedRelease {
    pub index: usize,
    pub download_id: String,
    pub client_id: i64,
    pub client_name: String,
}

pub struct GrabContext {
    pub now: DateTime<Utc>,
    /// When this process started, for back-off's startup grace.
    pub process_start: DateTime<Utc>,
    /// Category or label to file downloads under.
    pub category: Option<String>,
}

enum GrabResult {
    Grabbed(GrabbedRelease),
    Rejected,
    /// The client could not take it; try later.
    Failed,
    Skipped,
}

fn protocol_of(item: &EvaluatedRelease) -> DownloadProtocol {
    item.release.protocol
}

fn movie_id(items: &[EvaluatedRelease], index: usize) -> Option<i64> {
    items[index].movie.as_ref().map(|m| m.movie.id)
}

fn processed(items: &[EvaluatedRelease], done: impl Iterator<Item = usize>, index: usize) -> bool {
    let id = movie_id(items, index);
    done.into_iter().any(|d| movie_id(items, d) == id)
}

/// Grabs the best release for each movie.
///
/// Releases are tried best first. A movie is done once one release is
/// grabbed. Temporarily rejected releases (delays) are queued as pending, as
/// are releases that could not be sent because no client was available; a
/// failed protocol is not retried for the rest of the batch. Later releases
/// for a movie that already has a grab or a pending release are stored as
/// fallbacks.
pub async fn process_decisions(
    db: &Db,
    evaluation: &Evaluation,
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
    let mut pending_queue: Vec<(usize, PendingReason)> = Vec::new();
    let (mut usenet_failed, mut torrent_failed) = (false, false);

    let prepare_pending = |queue: &mut Vec<(usize, PendingReason)>,
                           grabbed: &[GrabbedRelease],
                           pending: &mut Vec<usize>,
                           index: usize,
                           mut reason: PendingReason| {
        if processed(items, grabbed.iter().map(|g| g.index), index)
            || processed(items, pending.iter().copied(), index)
        {
            reason = PendingReason::Fallback;
        }
        queue.push((index, reason));
        pending.push(index);
    };

    for index in evaluation.qualified_best_first() {
        let protocol = protocol_of(&items[index]);
        if processed(items, out.grabbed.iter().map(|g| g.index), index) {
            continue;
        }
        if items[index].decision.temporarily_rejected() {
            prepare_pending(
                &mut pending_queue,
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
            prepare_pending(
                &mut pending_queue,
                &out.grabbed,
                &mut out.pending,
                index,
                PendingReason::DownloadClientUnavailable,
            );
            continue;
        }

        match grab_one(db, evaluation, registry, ctx, index).await? {
            GrabResult::Grabbed(g) => out.grabbed.push(g),
            GrabResult::Rejected => out.rejected.push(index),
            GrabResult::Skipped => {}
            GrabResult::Failed => {
                prepare_pending(
                    &mut pending_queue,
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

    for (index, reason) in pending_queue {
        if let Some(id) = movie_id(items, index) {
            db.pending_releases()
                .add_or_update(id, &items[index].release, reason)
                .await?;
        }
    }
    Ok(out)
}

async fn grab_one(
    db: &Db,
    evaluation: &Evaluation,
    registry: &ClientRegistry,
    ctx: &GrabContext,
    index: usize,
) -> Result<GrabResult, ServiceError> {
    let item = &evaluation.items[index];
    let Some(movie) = item.movie.as_ref() else {
        return Ok(GrabResult::Skipped);
    };

    let blocked: HashSet<i64> = db
        .providers()
        .blocked(ProviderKind::DownloadClient, ctx.now)
        .await?
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    let client = match registry.select(item.release.protocol, &movie.movie.tags, false, &blocked) {
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
                        retry_after.map(|d| Duration::from_std(d).unwrap_or_default()),
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

/// Writes the "grabbed" history event, with the details later decisions need
/// (size and indexer flags) and the ones the UI shows.
async fn record_grab(
    db: &Db,
    item: &EvaluatedRelease,
    download_id: &str,
    client_name: &str,
    ctx: &GrabContext,
) -> Result<(), ServiceError> {
    let (Some(movie), Some(parsed)) = (item.movie.as_ref(), item.parsed.as_ref()) else {
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
    put("tmdb_id", r.tmdb_id.to_string());
    if let Some(group) = &parsed.release_group {
        put("release_group", group.clone());
    }
    if !parsed.release_hash.is_empty() {
        put("release_hash", parsed.release_hash.clone());
    }
    if let Some(hash) = r.torrent.as_ref().and_then(|t| t.info_hash.as_ref()) {
        put("torrent_info_hash", hash.clone());
    }

    db.history()
        .insert(&HistoryRecord {
            id: 0,
            movie_id: movie.movie.id,
            event: HistoryEvent::Grabbed,
            date: ctx.now,
            source_title: r.title.clone(),
            quality: parsed.quality,
            languages: item.languages.clone(),
            download_id: Some(download_id.to_owned()),
            data,
        })
        .await?;
    Ok(())
}

/// Once a release is grabbed, waiting releases for the same movie that are no
/// better have no further use. Better ones stay: they may still be worth an
/// upgrade when their delay ends.
async fn remove_grabbed_pending(db: &Db, grabbed: &EvaluatedRelease) -> Result<(), ServiceError> {
    let (Some(movie), Some(parsed), Some(profile)) = (
        grabbed.movie.as_ref(),
        grabbed.parsed.as_ref(),
        grabbed.profile.as_ref(),
    ) else {
        return Ok(());
    };
    for p in db.pending_releases().for_movie(movie.movie.id).await? {
        let Some(pending) = omp_parser::title::parse_movie_title(&p.release.title, false) else {
            continue;
        };
        if profile
            .comparer()
            .compare_model(&parsed.quality, &pending.quality)
            != std::cmp::Ordering::Less
        {
            db.pending_releases().remove(p.id).await?;
        }
    }
    Ok(())
}

/// Sends one chosen release to a download client regardless of what the
/// decision engine said: the person picked it. Records history and back-off
/// like an automatic grab. The error is a message fit to show.
pub async fn grab_selected(
    db: &Db,
    evaluation: &Evaluation,
    registry: &ClientRegistry,
    ctx: &GrabContext,
    index: usize,
) -> Result<GrabbedRelease, String> {
    let item = &evaluation.items[index];
    let Some(movie) = item.movie.as_ref() else {
        return Err("this release could not be matched to a movie in the library".into());
    };
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
        .select(item.release.protocol, &movie.movie.tags, false, &blocked)
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
