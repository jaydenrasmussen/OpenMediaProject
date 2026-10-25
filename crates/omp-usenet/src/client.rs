//! The built-in usenet client: a [`DownloadClient`] that takes NZBs, downloads
//! and repairs them, and leaves the finished files in a category folder.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, atomic::Ordering},
};

use async_trait::async_trait;
use omp_core::release::DownloadProtocol;
use omp_downloads::{
    AddRequest, ClientError, ClientItem, DownloadClient, DownloadItemStatus, QueuePriority,
};
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;

use crate::{
    download::{Control, Limiter, download, safe_file_name},
    nntp::ServerConfig,
    nzb::Nzb,
    post::{Par2Outcome, Tools, extract_archives, par2_repair, remove_par2},
};

#[derive(Debug, Clone)]
pub struct UsenetSettings {
    pub servers: Vec<ServerConfig>,
    /// Work in progress goes here.
    pub incomplete_dir: PathBuf,
    /// Finished downloads go to `<complete_dir>/<category>/<name>/`.
    pub complete_dir: PathBuf,
    /// Queue state and stored NZBs.
    pub state_dir: PathBuf,
    pub tools: Tools,
    /// Total download speed limit in bytes per second; 0 is unlimited.
    pub speed_limit_bps: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum JobState {
    Queued,
    Downloading,
    PostProcessing,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct JobRecord {
    id: String,
    name: String,
    category: Option<String>,
    state: JobState,
    message: Option<String>,
    output: Option<PathBuf>,
    total_bytes: u64,
    #[serde(default)]
    priority: QueuePriority,
    #[serde(default)]
    paused: bool,
}

struct Job {
    record: JobRecord,
    control: Arc<Control>,
    /// Speed sampling for the queue: when, bytes done then, smoothed bytes per second.
    sample: Mutex<(std::time::Instant, u64, f64)>,
}

fn new_job(record: JobRecord, control: Arc<Control>) -> Job {
    Job {
        record,
        control,
        sample: Mutex::new((std::time::Instant::now(), 0, 0.0)),
    }
}

struct Inner {
    settings: UsenetSettings,
    jobs: Mutex<Vec<Job>>,
    wake: Notify,
    limiter: Arc<Limiter>,
    http: reqwest::Client,
    next: std::sync::atomic::AtomicU64,
}

pub struct UsenetClient {
    inner: Arc<Inner>,
}

fn other(e: impl std::fmt::Display) -> ClientError {
    ClientError::Other(e.to_string())
}

impl Inner {
    fn nzb_path(&self, id: &str) -> PathBuf {
        self.settings.state_dir.join(format!("{id}.nzb"))
    }

    fn save(&self) {
        let records: Vec<JobRecord> = self
            .jobs
            .lock()
            .unwrap()
            .iter()
            .map(|j| j.record.clone())
            .collect();
        let path = self.settings.state_dir.join("jobs.json");
        if let Err(e) = std::fs::write(
            &path,
            serde_json::to_vec_pretty(&records).unwrap_or_default(),
        ) {
            tracing::warn!("could not save the usenet queue: {e}");
        }
    }

    fn update(&self, id: &str, f: impl FnOnce(&mut JobRecord)) {
        if let Some(job) = self
            .jobs
            .lock()
            .unwrap()
            .iter_mut()
            .find(|j| j.record.id == id)
        {
            f(&mut job.record);
        }
        self.save();
    }

    fn exists(&self, id: &str) -> bool {
        self.jobs.lock().unwrap().iter().any(|j| j.record.id == id)
    }
}

impl UsenetClient {
    /// Starts the client and its worker, resuming whatever was queued.
    pub async fn start(settings: UsenetSettings) -> Result<Self, ClientError> {
        for dir in [
            &settings.incomplete_dir,
            &settings.complete_dir,
            &settings.state_dir,
        ] {
            tokio::fs::create_dir_all(dir).await.map_err(other)?;
        }
        let limiter = Arc::new(Limiter::new(settings.speed_limit_bps));
        let mut jobs = Vec::new();
        if let Ok(bytes) = tokio::fs::read(settings.state_dir.join("jobs.json")).await
            && let Ok(records) = serde_json::from_slice::<Vec<JobRecord>>(&bytes)
        {
            for mut record in records {
                // Work that was interrupted starts over.
                if matches!(
                    record.state,
                    JobState::Downloading | JobState::PostProcessing
                ) {
                    record.state = JobState::Queued;
                }
                let control = Control {
                    limiter: Some(limiter.clone()),
                    paused: record.paused.into(),
                    ..Default::default()
                };
                jobs.push(new_job(record, Arc::new(control)));
            }
        }
        let inner = Arc::new(Inner {
            limiter,
            settings,
            jobs: Mutex::new(jobs),
            wake: Notify::new(),
            http: omp_net::client_builder()
                .build()
                .expect("static client config"),
            next: std::sync::atomic::AtomicU64::new(0),
        });
        tokio::spawn(run(inner.clone()));
        inner.wake.notify_one();
        Ok(Self { inner })
    }

    /// Changes one job and saves the queue; an unknown id is an error.
    fn control(&self, id: &str, f: impl FnOnce(&mut Job)) -> Result<(), ClientError> {
        {
            let mut jobs = self.inner.jobs.lock().unwrap();
            let job = jobs
                .iter_mut()
                .find(|j| j.record.id == id)
                .ok_or_else(|| ClientError::Other("no such download".into()))?;
            f(job);
        }
        self.inner.save();
        Ok(())
    }

    /// Changes the total speed limit (bytes per second, 0 for none) at once.
    pub fn set_speed_limit(&self, bytes_per_second: u64) {
        self.inner.limiter.set(bytes_per_second);
    }

    fn new_id(&self) -> String {
        let n = self.inner.next.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        format!("omp_nzo_{:x}{:04x}", nanos, n)
    }
}

async fn run(inner: Arc<Inner>) {
    loop {
        let next = {
            let mut jobs = inner.jobs.lock().unwrap();
            // The most important waiting job first; among equals, the oldest.
            let best = jobs
                .iter()
                .enumerate()
                .filter(|(_, j)| j.record.state == JobState::Queued && !j.record.paused)
                .max_by_key(|(i, j)| (j.record.priority, std::cmp::Reverse(*i)))
                .map(|(i, _)| i);
            best.and_then(|i| jobs.get_mut(i)).map(|j| {
                j.record.state = JobState::Downloading;
                (j.record.clone(), j.control.clone())
            })
        };
        let Some((record, control)) = next else {
            inner.wake.notified().await;
            continue;
        };
        inner.save();
        let (state, message, output) = match process(&inner, &record, &control).await {
            Ok(output) => (JobState::Completed, None, Some(output)),
            Err(message) => (JobState::Failed, Some(message), None),
        };
        // A removed job has nothing to record.
        if inner.exists(&record.id) {
            inner.update(&record.id, |r| {
                r.state = state;
                r.message = message;
                r.output = output;
            });
        }
        let _ = tokio::fs::remove_file(inner.nzb_path(&record.id)).await;
    }
}

async fn process(
    inner: &Inner,
    record: &JobRecord,
    control: &Arc<Control>,
) -> Result<PathBuf, String> {
    let xml = tokio::fs::read_to_string(inner.nzb_path(&record.id))
        .await
        .map_err(|e| format!("stored NZB is missing: {e}"))?;
    let nzb = Nzb::parse(&xml).map_err(|e| e.to_string())?;
    let work = inner.settings.incomplete_dir.join(&record.id);
    let _ = tokio::fs::remove_dir_all(&work).await;

    let report = download(&nzb, inner.settings.servers.clone(), &work, control.clone())
        .await
        .map_err(|e| format!("download failed: {e}"))?;
    if report.cancelled || !inner.exists(&record.id) {
        let _ = tokio::fs::remove_dir_all(&work).await;
        return Err("cancelled".into());
    }

    inner.update(&record.id, |r| r.state = JobState::PostProcessing);
    let failed = report.failed_segments();
    if failed > 0 {
        tracing::info!(job = %record.name, failed, "articles missing, trying par2");
        match par2_repair(&work, &inner.settings.tools)
            .await
            .map_err(|e| e.to_string())?
        {
            Par2Outcome::Verified | Par2Outcome::Repaired => {}
            Par2Outcome::NoPar2 => {
                return Err(format!(
                    "{failed} articles were missing and there is no par2 data to repair them"
                ));
            }
            Par2Outcome::Failed(why) => return Err(format!("Repair failed: {why}")),
        }
    }
    let extracted = extract_archives(&work, &inner.settings.tools)
        .await
        .map_err(|e| e.to_string())?;
    if let Some((name, why)) = extracted.failed.first() {
        return Err(format!("Unpacking {name} failed: {why}"));
    }
    remove_par2(&work).await.map_err(|e| e.to_string())?;

    let mut destination = inner.settings.complete_dir.clone();
    if let Some(c) = record.category.as_deref().filter(|c| !c.is_empty()) {
        destination.push(safe_file_name(c));
    }
    destination.push(safe_file_name(&record.name));
    let _ = tokio::fs::remove_dir_all(&destination).await;
    tokio::fs::create_dir_all(destination.parent().unwrap_or(Path::new(".")))
        .await
        .map_err(|e| e.to_string())?;
    if tokio::fs::rename(&work, &destination).await.is_err() {
        copy_dir(&work, &destination)
            .await
            .map_err(|e| e.to_string())?;
        let _ = tokio::fs::remove_dir_all(&work).await;
    }
    Ok(destination)
}

async fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    tokio::fs::create_dir_all(to).await?;
    let mut stack = vec![(from.to_path_buf(), to.to_path_buf())];
    while let Some((src, dst)) = stack.pop() {
        let mut entries = tokio::fs::read_dir(&src).await?;
        while let Some(e) = entries.next_entry().await? {
            let target = dst.join(e.file_name());
            if e.file_type().await?.is_dir() {
                tokio::fs::create_dir_all(&target).await?;
                stack.push((e.path(), target));
            } else {
                tokio::fs::copy(e.path(), target).await?;
            }
        }
    }
    Ok(())
}

#[async_trait]
impl DownloadClient for UsenetClient {
    fn protocol(&self) -> DownloadProtocol {
        DownloadProtocol::Usenet
    }

    async fn add(&self, request: &AddRequest<'_>) -> Result<String, ClientError> {
        let release = request.release;
        if release.download_url.is_empty() {
            return Err(ClientError::ReleaseUnavailable(
                "release has no download link".into(),
            ));
        }
        let response = self
            .inner
            .http
            .get(&release.download_url)
            .send()
            .await
            .map_err(|e| ClientError::ReleaseDownloadFailed {
                message: e.to_string(),
                retry_after: None,
            })?;
        if !response.status().is_success() {
            let status = response.status();
            return Err(
                if status == reqwest::StatusCode::NOT_FOUND || status == reqwest::StatusCode::GONE {
                    ClientError::ReleaseUnavailable(format!("indexer answered {status}"))
                } else {
                    ClientError::ReleaseDownloadFailed {
                        message: format!("indexer answered {status}"),
                        retry_after: None,
                    }
                },
            );
        }
        let xml = response
            .text()
            .await
            .map_err(|e| ClientError::ReleaseDownloadFailed {
                message: e.to_string(),
                retry_after: None,
            })?;
        // Indexers answer errors with an XML or HTML body and a 200.
        let nzb = Nzb::parse(&xml).map_err(|e| ClientError::ReleaseUnavailable(e.to_string()))?;

        let id = self.new_id();
        tokio::fs::write(self.inner.nzb_path(&id), &xml)
            .await
            .map_err(other)?;
        let name = Nzb::title_from(&xml).unwrap_or_else(|| release.title.clone());
        {
            let control = Arc::new(Control {
                limiter: Some(self.inner.limiter.clone()),
                ..Default::default()
            });
            control
                .total_segments
                .store(nzb.segment_count() as u64, Ordering::Relaxed);
            self.inner.jobs.lock().unwrap().push(new_job(
                JobRecord {
                    id: id.clone(),
                    name: if release.title.is_empty() {
                        name
                    } else {
                        release.title.clone()
                    },
                    category: request.category.map(str::to_owned),
                    state: JobState::Queued,
                    message: None,
                    output: None,
                    total_bytes: nzb.total_bytes(),
                    priority: QueuePriority::Normal,
                    paused: false,
                },
                control,
            ));
        }
        self.inner.save();
        self.inner.wake.notify_one();
        Ok(id)
    }

    async fn items(&self) -> Result<Vec<ClientItem>, ClientError> {
        let jobs = self.inner.jobs.lock().unwrap();
        Ok(jobs
            .iter()
            .map(|j| {
                let r = &j.record;
                let total = j.control.total_segments.load(Ordering::Relaxed);
                let done = j.control.done_segments.load(Ordering::Relaxed);
                let remaining = match r.state {
                    JobState::Completed => 0,
                    _ if total == 0 => r.total_bytes,
                    _ => r
                        .total_bytes
                        .saturating_sub(r.total_bytes * done.min(total) / total),
                };
                let (status, can_finish) = match r.state {
                    JobState::Queued if r.paused => (DownloadItemStatus::Paused, false),
                    JobState::Queued => (DownloadItemStatus::Queued, false),
                    JobState::Downloading if r.paused => (DownloadItemStatus::Paused, false),
                    JobState::Downloading | JobState::PostProcessing => {
                        (DownloadItemStatus::Downloading, false)
                    }
                    JobState::Completed => (DownloadItemStatus::Completed, true),
                    JobState::Failed => (DownloadItemStatus::Failed, false),
                };
                // Speed from the bytes fetched since the last look, smoothed.
                let speed = if r.state == JobState::Downloading && !r.paused {
                    let now = std::time::Instant::now();
                    let bytes = j.control.bytes_done.load(Ordering::Relaxed);
                    let mut s = j.sample.lock().unwrap();
                    let secs = now.duration_since(s.0).as_secs_f64();
                    if secs >= 1.0 {
                        let instant = bytes.saturating_sub(s.1) as f64 / secs;
                        s.2 = if s.2 == 0.0 {
                            instant
                        } else {
                            s.2 * 0.6 + instant * 0.4
                        };
                        s.0 = now;
                        s.1 = bytes;
                    }
                    s.2 as u64
                } else {
                    0
                };
                ClientItem {
                    eta_seconds: (speed > 0 && remaining > 0).then(|| remaining / speed),
                    download_speed: speed,
                    priority: r.priority,
                    download_id: r.id.clone(),
                    category: r.category.clone(),
                    title: r.name.clone(),
                    total_size: r.total_bytes,
                    remaining_size: remaining,
                    status,
                    output_path: r.output.as_ref().map(|p| p.to_string_lossy().into_owned()),
                    seed_ratio: None,
                    can_move_files: can_finish,
                    can_be_removed: can_finish,
                    message: r.message.clone(),
                }
            })
            .collect())
    }

    async fn pause(&self, download_id: &str) -> Result<(), ClientError> {
        self.control(download_id, |j| {
            j.record.paused = true;
            j.control.paused.store(true, Ordering::Relaxed);
        })
    }

    async fn resume(&self, download_id: &str) -> Result<(), ClientError> {
        self.control(download_id, |j| {
            j.record.paused = false;
            j.control.paused.store(false, Ordering::Relaxed);
        })?;
        self.inner.wake.notify_one();
        Ok(())
    }

    async fn set_priority(
        &self,
        download_id: &str,
        priority: QueuePriority,
    ) -> Result<(), ClientError> {
        self.control(download_id, |j| j.record.priority = priority)?;
        self.inner.wake.notify_one();
        Ok(())
    }

    async fn remove(&self, download_id: &str, delete_data: bool) -> Result<(), ClientError> {
        let removed = {
            let mut jobs = self.inner.jobs.lock().unwrap();
            jobs.iter()
                .position(|j| j.record.id == download_id)
                .map(|i| jobs.remove(i))
        };
        let Some(job) = removed else { return Ok(()) };
        job.control.cancelled.store(true, Ordering::Relaxed);
        self.inner.save();
        let _ = tokio::fs::remove_file(self.inner.nzb_path(download_id)).await;
        if job.record.state != JobState::Completed {
            let _ = tokio::fs::remove_dir_all(self.inner.settings.incomplete_dir.join(download_id))
                .await;
        }
        if delete_data && let Some(out) = &job.record.output {
            let _ = tokio::fs::remove_dir_all(out).await;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{process::Command as StdCommand, sync::Mutex as StdMutex, time::Duration};

    use axum::{Router, extract::State, http::StatusCode, response::IntoResponse, routing::get};
    use chrono::Utc;
    use omp_core::release::{IndexerFlags, ReleaseInfo};

    use super::*;
    use crate::{
        nntp::mock::{MockState, spawn},
        yenc::encode_part,
    };

    struct Posted {
        /// (file name, message ids)
        files: Vec<(String, Vec<String>)>,
    }

    fn pattern(len: usize, seed: u8) -> Vec<u8> {
        (0..len)
            .map(|i| (i as u8).wrapping_mul(13).wrapping_add(seed))
            .collect()
    }

    /// Posts `data` as `name` in `parts` articles; returns the message ids.
    fn post(
        state: &StdMutex<MockState>,
        name: &str,
        data: &[u8],
        parts: usize,
    ) -> (String, Vec<String>) {
        let size = data.len().div_ceil(parts.max(1)).max(1);
        let mut ids = Vec::new();
        for (i, chunk) in data.chunks(size).enumerate() {
            let id = format!("{name}-{i}@t");
            state.lock().unwrap().articles.insert(
                id.clone(),
                encode_part(
                    name,
                    data.len() as u64,
                    (i * size) as u64,
                    chunk,
                    i as u32 + 1,
                    data.len().div_ceil(size) as u32,
                ),
            );
            ids.push(id);
        }
        (name.to_owned(), ids)
    }

    fn nzb_xml(p: &Posted) -> String {
        let mut x = String::from(
            r#"<?xml version="1.0"?><nzb xmlns="http://www.newzbin.com/DTD/2003/nzb">"#,
        );
        for (name, ids) in &p.files {
            x.push_str(&format!(r#"<file poster="p" date="1" subject="&quot;{name}&quot; yEnc"><groups><group>alt.test</group></groups><segments>"#));
            for (i, id) in ids.iter().enumerate() {
                x.push_str(&format!(
                    r#"<segment bytes="1000" number="{}">{id}</segment>"#,
                    i + 1
                ));
            }
            x.push_str("</segments></file>");
        }
        x.push_str("</nzb>");
        x
    }

    async fn serve_nzb(body: String, status: u16) -> String {
        async fn h(State((body, status)): State<(String, u16)>) -> impl IntoResponse {
            (StatusCode::from_u16(status).unwrap(), body)
        }
        let app = Router::new()
            .route("/get.nzb", get(h))
            .with_state((body, status));
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/get.nzb", l.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        url
    }

    fn release(url: String) -> ReleaseInfo {
        ReleaseInfo {
            guid: "g".into(),
            title: "Some.Movie.2020.1080p.WEB-DL-GRP".into(),
            size: 0,
            download_url: url,
            indexer_id: 0,
            indexer: "Idx".into(),
            indexer_priority: 25,
            protocol: DownloadProtocol::Usenet,
            publish_date: Utc::now(),
            container: None,
            indexer_flags: IndexerFlags(0),
            imdb_id: None,
            tmdb_id: 0,
            torrent: None,
        }
    }

    fn settings(root: &Path, port: u16) -> UsenetSettings {
        let mut server = ServerConfig::new("127.0.0.1", port);
        server.max_connections = 3;
        server.timeout = Duration::from_secs(5);
        UsenetSettings {
            servers: vec![server],
            incomplete_dir: root.join("incomplete"),
            complete_dir: root.join("complete"),
            state_dir: root.join("state"),
            tools: Tools::default(),
            speed_limit_bps: 0,
        }
    }

    async fn wait_until(client: &UsenetClient, id: &str, want: DownloadItemStatus) -> ClientItem {
        for _ in 0..200 {
            if let Some(i) = client
                .items()
                .await
                .unwrap()
                .into_iter()
                .find(|i| i.download_id == id && i.status == want)
            {
                return i;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!(
            "job never reached {want:?}: {:?}",
            client.items().await.unwrap()
        );
    }

    fn have(tool: &str, arg: &str) -> bool {
        StdCommand::new(tool).arg(arg).output().is_ok()
    }

    async fn add(client: &UsenetClient, xml: String) -> String {
        let url = serve_nzb(xml, 200).await;
        client
            .add(&AddRequest {
                release: &release(url),
                category: Some("movies"),
            })
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn downloads_a_release_into_its_category_folder_and_survives_a_restart() {
        let state = Arc::new(StdMutex::new(MockState::default()));
        let movie = pattern(40_000, 1);
        let info = pattern(900, 2);
        let posted = Posted {
            files: vec![
                post(&state, "movie.mkv", &movie, 5),
                post(&state, "movie.nfo", &info, 1),
            ],
        };
        let port = spawn(state).await;
        let root = tempfile::tempdir().unwrap();

        let client = UsenetClient::start(settings(root.path(), port))
            .await
            .unwrap();
        let id = add(&client, nzb_xml(&posted)).await;
        let item = wait_until(&client, &id, DownloadItemStatus::Completed).await;

        assert_eq!(item.category.as_deref(), Some("movies"));
        assert_eq!(item.title, "Some.Movie.2020.1080p.WEB-DL-GRP");
        assert_eq!(item.remaining_size, 0);
        assert!(item.can_be_removed && item.can_move_files);
        let out = PathBuf::from(item.output_path.clone().unwrap());
        assert_eq!(
            out,
            root.path()
                .join("complete/movies/Some.Movie.2020.1080p.WEB-DL-GRP")
        );
        assert_eq!(std::fs::read(out.join("movie.mkv")).unwrap(), movie);
        assert_eq!(std::fs::read(out.join("movie.nfo")).unwrap(), info);
        assert!(
            !root.path().join("incomplete").join(&id).exists(),
            "work folder is gone"
        );

        // A new process sees the finished job.
        let again = UsenetClient::start(settings(root.path(), port))
            .await
            .unwrap();
        let items = again.items().await.unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(
            (items[0].download_id.as_str(), items[0].status),
            (id.as_str(), DownloadItemStatus::Completed)
        );

        again.remove(&id, true).await.unwrap();
        assert!(again.items().await.unwrap().is_empty());
        assert!(!out.exists(), "data deleted on request");
    }

    #[tokio::test]
    async fn a_paused_download_waits_and_higher_priority_goes_first() {
        let state = Arc::new(StdMutex::new(MockState::default()));
        let a = Posted {
            files: vec![post(&state, "a.mkv", &pattern(20_000, 3), 4)],
        };
        let b = Posted {
            files: vec![post(&state, "b.mkv", &pattern(20_000, 4), 4)],
        };
        let port = spawn(state).await;
        let root = tempfile::tempdir().unwrap();
        let client = UsenetClient::start(settings(root.path(), port))
            .await
            .unwrap();

        // Stop the worker from starting anything while the queue is arranged:
        // both are paused the moment they exist.
        let first = add(&client, nzb_xml(&a)).await;
        client.pause(&first).await.unwrap();
        let second = add(&client, nzb_xml(&b)).await;
        client.pause(&second).await.unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        let items = client.items().await.unwrap();
        assert!(
            items.iter().all(|i| i.status == DownloadItemStatus::Paused),
            "{items:?}"
        );

        // The later one is more important, so it finishes first.
        client
            .set_priority(&second, QueuePriority::High)
            .await
            .unwrap();
        client.resume(&second).await.unwrap();
        wait_until(&client, &second, DownloadItemStatus::Completed).await;
        let items = client.items().await.unwrap();
        let first_item = items.iter().find(|i| i.download_id == first).unwrap();
        assert_eq!(
            first_item.status,
            DownloadItemStatus::Paused,
            "still waiting"
        );
        assert_eq!(
            items
                .iter()
                .find(|i| i.download_id == second)
                .unwrap()
                .priority,
            QueuePriority::High
        );

        client.resume(&first).await.unwrap();
        wait_until(&client, &first, DownloadItemStatus::Completed).await;
        assert!(client.pause("nope").await.is_err());
    }

    #[tokio::test]
    async fn the_speed_limit_slows_a_download() {
        let state = Arc::new(StdMutex::new(MockState::default()));
        let posted = Posted {
            files: vec![post(&state, "m.mkv", &pattern(60_000, 5), 6)],
        };
        let port = spawn(state).await;
        let root = tempfile::tempdir().unwrap();
        let mut s = settings(root.path(), port);
        s.speed_limit_bps = 20_000; // 60 kB takes about two seconds
        let client = UsenetClient::start(s).await.unwrap();
        let started = std::time::Instant::now();
        let id = add(&client, nzb_xml(&posted)).await;
        wait_until(&client, &id, DownloadItemStatus::Completed).await;
        assert!(
            started.elapsed() >= Duration::from_millis(1500),
            "{:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn missing_articles_are_repaired_with_par2() {
        if !have("par2", "--help") {
            eprintln!("par2 not installed; skipping");
            return;
        }
        let state = Arc::new(StdMutex::new(MockState::default()));
        let movie = pattern(120_000, 7);
        let work = tempfile::tempdir().unwrap();
        std::fs::write(work.path().join("movie.mkv"), &movie).unwrap();
        assert!(
            StdCommand::new("par2")
                .current_dir(work.path())
                .args(["create", "-q", "-r25", "movie.par2", "movie.mkv"])
                .status()
                .unwrap()
                .success()
        );
        let mut files = vec![post(&state, "movie.mkv", &movie, 12)];
        for entry in std::fs::read_dir(work.path()).unwrap().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(".par2") {
                files.push(post(
                    &state,
                    &name,
                    &std::fs::read(entry.path()).unwrap(),
                    1,
                ));
            }
        }
        // Two of the twelve movie articles are gone from the server.
        for id in ["movie.mkv-3@t", "movie.mkv-8@t"] {
            state.lock().unwrap().articles.remove(id);
        }
        let port = spawn(state).await;
        let root = tempfile::tempdir().unwrap();

        let client = UsenetClient::start(settings(root.path(), port))
            .await
            .unwrap();
        let id = add(&client, nzb_xml(&Posted { files })).await;
        let item = wait_until(&client, &id, DownloadItemStatus::Completed).await;
        let out = PathBuf::from(item.output_path.unwrap());
        assert_eq!(
            std::fs::read(out.join("movie.mkv")).unwrap(),
            movie,
            "repaired byte for byte"
        );
        assert!(
            !std::fs::read_dir(&out)
                .unwrap()
                .flatten()
                .any(|e| e.file_name().to_string_lossy().ends_with(".par2")),
            "par2 files cleaned up"
        );
    }

    #[tokio::test]
    async fn unrepairable_downloads_fail_with_a_reason() {
        let state = Arc::new(StdMutex::new(MockState::default()));
        let movie = pattern(20_000, 4);
        let posted = Posted {
            files: vec![post(&state, "movie.mkv", &movie, 4)],
        };
        state.lock().unwrap().articles.remove("movie.mkv-1@t");
        let port = spawn(state).await;
        let root = tempfile::tempdir().unwrap();

        let client = UsenetClient::start(settings(root.path(), port))
            .await
            .unwrap();
        let id = add(&client, nzb_xml(&posted)).await;
        let item = wait_until(&client, &id, DownloadItemStatus::Failed).await;
        assert!(item.message.unwrap().contains("no par2"), "says why");
        assert!(!item.can_be_removed);
        assert!(item.output_path.is_none());
    }

    #[tokio::test]
    async fn archives_are_unpacked() {
        if !have("7zz", "--help") {
            eprintln!("7zz not installed; skipping");
            return;
        }
        let payload = pattern(250_000, 9);
        let work = tempfile::tempdir().unwrap();
        let src = tempfile::tempdir().unwrap();
        std::fs::write(src.path().join("movie.mkv"), &payload).unwrap();
        assert!(
            StdCommand::new("7zz")
                .current_dir(src.path())
                .args(["a", "-v90k", "-mx0", "-bso0", "-bsp0"])
                .arg(work.path().join("rel.7z"))
                .arg("movie.mkv")
                .status()
                .unwrap()
                .success()
        );

        let state = Arc::new(StdMutex::new(MockState::default()));
        let mut files = Vec::new();
        let mut volumes: Vec<_> = std::fs::read_dir(work.path()).unwrap().flatten().collect();
        volumes.sort_by_key(|e| e.file_name());
        for v in volumes {
            files.push(post(
                &state,
                &v.file_name().to_string_lossy(),
                &std::fs::read(v.path()).unwrap(),
                2,
            ));
        }
        let port = spawn(state).await;
        let root = tempfile::tempdir().unwrap();

        let client = UsenetClient::start(settings(root.path(), port))
            .await
            .unwrap();
        let id = add(&client, nzb_xml(&Posted { files })).await;
        let item = wait_until(&client, &id, DownloadItemStatus::Completed).await;
        let out = PathBuf::from(item.output_path.unwrap());
        let names: Vec<_> = std::fs::read_dir(&out)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["movie.mkv"], "volumes removed after extraction");
        assert_eq!(std::fs::read(out.join("movie.mkv")).unwrap(), payload);
    }

    #[tokio::test]
    async fn bad_nzbs_are_rejected_when_added() {
        let state = Arc::new(StdMutex::new(MockState::default()));
        let port = spawn(state).await;
        let root = tempfile::tempdir().unwrap();
        let client = UsenetClient::start(settings(root.path(), port))
            .await
            .unwrap();

        let html = serve_nzb("<html><body>API limit reached</body></html>".into(), 200).await;
        let r = client
            .add(&AddRequest {
                release: &release(html),
                category: None,
            })
            .await;
        assert!(
            matches!(r, Err(ClientError::ReleaseUnavailable(_))),
            "{r:?}"
        );

        let gone = serve_nzb(String::new(), 404).await;
        let r = client
            .add(&AddRequest {
                release: &release(gone),
                category: None,
            })
            .await;
        assert!(
            matches!(r, Err(ClientError::ReleaseUnavailable(_))),
            "{r:?}"
        );

        let busy = serve_nzb(String::new(), 503).await;
        let r = client
            .add(&AddRequest {
                release: &release(busy),
                category: None,
            })
            .await;
        assert!(
            matches!(r, Err(ClientError::ReleaseDownloadFailed { .. })),
            "{r:?}"
        );

        let r = client
            .add(&AddRequest {
                release: &release(String::new()),
                category: None,
            })
            .await;
        assert!(matches!(r, Err(ClientError::ReleaseUnavailable(_))));
        assert!(client.items().await.unwrap().is_empty());
    }
}
