//! Downloads an NZB: many workers pull segments from a shared queue, fetch
//! them (falling back through the servers in priority order), decode, and
//! write each part at its offset in the target file.

use std::{
    collections::{HashMap, VecDeque},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use tokio::{
    fs::File,
    io::{AsyncSeekExt, AsyncWriteExt},
    sync::Mutex as AsyncMutex,
};

use crate::{
    nntp::{NntpConnection, NntpError, ServerConfig},
    nzb::Nzb,
    yenc::{self, YencPart},
};

/// A shared download speed limit. Every worker of every job reports the
/// bytes it fetched and is slowed when the total runs ahead of the limit.
pub struct Limiter {
    bytes_per_second: AtomicU64,
    state: AsyncMutex<(f64, std::time::Instant)>,
}

impl Limiter {
    /// 0 means no limit.
    pub fn new(bytes_per_second: u64) -> Self {
        Self {
            bytes_per_second: AtomicU64::new(bytes_per_second),
            state: AsyncMutex::new((0.0, std::time::Instant::now())),
        }
    }

    pub fn set(&self, bytes_per_second: u64) {
        self.bytes_per_second
            .store(bytes_per_second, Ordering::Relaxed);
    }

    /// Accounts for `n` bytes, waiting if that puts us over the limit. A
    /// debt model: the bytes are spent first and the wait pays them back.
    pub async fn take(&self, n: u64) {
        let rate = self.bytes_per_second.load(Ordering::Relaxed);
        if rate == 0 {
            return;
        }
        let wait = {
            let mut s = self.state.lock().await;
            let now = std::time::Instant::now();
            let refill = now.duration_since(s.1).as_secs_f64() * rate as f64;
            // A second's worth of credit at most, so idle time is not banked.
            s.0 = (s.0 + refill).min(rate as f64) - n as f64;
            s.1 = now;
            (s.0 < 0.0).then(|| Duration::from_secs_f64(-s.0 / rate as f64))
        };
        if let Some(wait) = wait {
            tokio::time::sleep(wait).await;
        }
    }
}

/// Shared between the downloader and whoever shows progress or controls it.
#[derive(Default)]
pub struct Control {
    /// The speed limit shared by every job, if the client has one.
    pub limiter: Option<Arc<Limiter>>,
    pub total_segments: AtomicU64,
    pub done_segments: AtomicU64,
    pub failed_segments: AtomicU64,
    /// Decoded bytes written so far.
    pub bytes_done: AtomicU64,
    pub paused: AtomicBool,
    pub cancelled: AtomicBool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileReport {
    /// Index of the file in the NZB.
    pub index: usize,
    /// The name the poster's yEnc header gave it (else the NZB's).
    pub name: String,
    /// `None` when not a single part could be fetched.
    pub path: Option<PathBuf>,
    pub segments_total: usize,
    pub segments_failed: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadReport {
    pub files: Vec<FileReport>,
    pub cancelled: bool,
}

impl DownloadReport {
    pub fn failed_segments(&self) -> usize {
        self.files.iter().map(|f| f.segments_failed).sum()
    }
}

struct Task {
    file: usize,
    message_id: String,
}

struct OpenFile {
    name: String,
    path: PathBuf,
    file: File,
}

struct Shared {
    queue: Mutex<VecDeque<Task>>,
    servers: Vec<ServerConfig>,
    dir: PathBuf,
    files: Vec<AsyncMutex<Option<OpenFile>>>,
    failed: Vec<AtomicU64>,
    control: Arc<Control>,
}

/// A file name from a poster is untrusted: keep only the final component.
pub fn safe_file_name(name: &str) -> String {
    let last = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(name)
        .trim()
        .trim_matches('.');
    if last.is_empty() {
        "unnamed".to_owned()
    } else {
        last.to_owned()
    }
}

pub async fn download(
    nzb: &Nzb,
    mut servers: Vec<ServerConfig>,
    dir: &Path,
    control: Arc<Control>,
) -> std::io::Result<DownloadReport> {
    tokio::fs::create_dir_all(dir).await?;
    servers.sort_by_key(|s| s.priority);

    let mut queue = VecDeque::new();
    for (i, f) in nzb.files.iter().enumerate() {
        for s in &f.segments {
            queue.push_back(Task {
                file: i,
                message_id: s.message_id.clone(),
            });
        }
    }
    control
        .total_segments
        .store(queue.len() as u64, Ordering::Relaxed);

    let best = servers.first().map_or(0, |s| s.priority);
    let workers: Vec<usize> = servers
        .iter()
        .enumerate()
        .filter(|(_, s)| s.priority == best)
        .flat_map(|(i, s)| std::iter::repeat_n(i, s.max_connections.max(1)))
        .collect();

    let shared = Arc::new(Shared {
        queue: Mutex::new(queue),
        servers,
        dir: dir.to_path_buf(),
        files: nzb.files.iter().map(|_| AsyncMutex::new(None)).collect(),
        failed: nzb.files.iter().map(|_| AtomicU64::new(0)).collect(),
        control: control.clone(),
    });

    let mut handles = Vec::new();
    for home in workers {
        handles.push(tokio::spawn(worker(shared.clone(), home)));
    }
    for h in handles {
        let _ = h.await;
    }

    let mut files = Vec::new();
    for (i, f) in nzb.files.iter().enumerate() {
        let mut slot = shared.files[i].lock().await;
        let (name, path) = match slot.as_mut() {
            Some(open) => {
                let _ = open.file.flush().await;
                (open.name.clone(), Some(open.path.clone()))
            }
            None => (safe_file_name(&f.name), None),
        };
        files.push(FileReport {
            index: i,
            name,
            path,
            segments_total: f.segments.len(),
            segments_failed: shared.failed[i].load(Ordering::Relaxed) as usize,
        });
    }
    Ok(DownloadReport {
        files,
        cancelled: control.cancelled.load(Ordering::Relaxed),
    })
}

async fn worker(shared: Arc<Shared>, home: usize) {
    let mut connections: HashMap<usize, NntpConnection> = HashMap::new();
    loop {
        if shared.control.cancelled.load(Ordering::Relaxed) {
            break;
        }
        if shared.control.paused.load(Ordering::Relaxed) {
            tokio::time::sleep(Duration::from_millis(100)).await;
            continue;
        }
        let Some(task) = shared.queue.lock().unwrap().pop_front() else {
            break;
        };
        match fetch(&shared, &mut connections, home, &task.message_id).await {
            Some(part) => {
                let len = part.data.len() as u64;
                if let Err(e) = write_part(&shared, task.file, part).await {
                    tracing::warn!("could not write part: {e}");
                    shared.failed[task.file].fetch_add(1, Ordering::Relaxed);
                    shared
                        .control
                        .failed_segments
                        .fetch_add(1, Ordering::Relaxed);
                } else {
                    shared.control.bytes_done.fetch_add(len, Ordering::Relaxed);
                    if let Some(limiter) = &shared.control.limiter {
                        limiter.take(len).await;
                    }
                }
            }
            None => {
                shared.failed[task.file].fetch_add(1, Ordering::Relaxed);
                shared
                    .control
                    .failed_segments
                    .fetch_add(1, Ordering::Relaxed);
            }
        }
        shared.control.done_segments.fetch_add(1, Ordering::Relaxed);
    }
    for (_, c) in connections {
        c.quit().await;
    }
}

/// Tries the worker's own server first, then the rest in priority order.
async fn fetch(
    shared: &Shared,
    connections: &mut HashMap<usize, NntpConnection>,
    home: usize,
    message_id: &str,
) -> Option<YencPart> {
    let order: Vec<usize> = std::iter::once(home)
        .chain((0..shared.servers.len()).filter(|&i| i != home))
        .collect();
    for server in order {
        for attempt in 0..3 {
            if let std::collections::hash_map::Entry::Vacant(slot) = connections.entry(server) {
                match NntpConnection::connect(&shared.servers[server]).await {
                    Ok(c) => {
                        slot.insert(c);
                    }
                    Err(e) => {
                        tracing::debug!(host = %shared.servers[server].host, "connect failed: {e}");
                        if attempt == 2 || !e.is_transient() {
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(200 * (attempt + 1))).await;
                        continue;
                    }
                }
            }
            let conn = connections.get_mut(&server).expect("just inserted");
            match conn.body(message_id).await {
                Ok(body) => match yenc::decode_part(&body) {
                    Ok(part) => return Some(part),
                    Err(e) => {
                        tracing::debug!(
                            message_id,
                            "bad article from {}: {e}",
                            shared.servers[server].host
                        );
                        break; // another server may have an intact copy
                    }
                },
                Err(NntpError::ArticleNotFound) => break,
                Err(e) => {
                    connections.remove(&server);
                    if !e.is_transient() {
                        tracing::warn!(host = %shared.servers[server].host, "server error: {e}");
                        break;
                    }
                }
            }
        }
    }
    None
}

async fn write_part(shared: &Shared, file: usize, part: YencPart) -> std::io::Result<()> {
    let mut slot = shared.files[file].lock().await;
    if slot.is_none() {
        let name = safe_file_name(&part.name);
        let path = unique_path(&shared.dir, &name);
        let f = File::create(&path).await?;
        f.set_len(part.file_size).await?;
        *slot = Some(OpenFile {
            name: path.file_name().unwrap().to_string_lossy().into_owned(),
            path,
            file: f,
        });
    }
    let open = slot.as_mut().expect("opened above");
    open.file
        .seek(std::io::SeekFrom::Start(part.offset))
        .await?;
    open.file.write_all(&part.data).await
}

/// Two files with the same name in one NZB must not overwrite each other.
fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let mut path = dir.join(name);
    let mut n = 1;
    while path.exists() {
        path = dir.join(format!("{n}-{name}"));
        n += 1;
    }
    path
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex as StdMutex;

    use super::*;
    use crate::{
        nntp::mock::{MockState, spawn},
        nzb::{NzbFile, NzbSegment},
        yenc::encode_part,
    };

    fn nzb_for(files: &[(&str, Vec<String>)]) -> Nzb {
        Nzb {
            files: files
                .iter()
                .map(|(name, ids)| NzbFile {
                    poster: "p".into(),
                    subject: format!("\"{name}\""),
                    name: (*name).into(),
                    groups: vec!["alt.test".into()],
                    segments: ids
                        .iter()
                        .enumerate()
                        .map(|(i, id)| NzbSegment {
                            number: i as u32 + 1,
                            bytes: 100,
                            message_id: id.clone(),
                        })
                        .collect(),
                })
                .collect(),
        }
    }

    /// Splits `data` into `parts` articles named `{prefix}-{n}@t` on the server.
    fn post(
        state: &StdMutex<MockState>,
        prefix: &str,
        name: &str,
        data: &[u8],
        parts: usize,
    ) -> Vec<String> {
        let size = data.len().div_ceil(parts);
        let mut ids = Vec::new();
        for (i, chunk) in data.chunks(size).enumerate() {
            let id = format!("{prefix}-{i}@t");
            state.lock().unwrap().articles.insert(
                id.clone(),
                encode_part(
                    name,
                    data.len() as u64,
                    (i * size) as u64,
                    chunk,
                    i as u32 + 1,
                    parts as u32,
                ),
            );
            ids.push(id);
        }
        ids
    }

    fn pattern(len: usize, seed: u8) -> Vec<u8> {
        (0..len)
            .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
            .collect()
    }

    fn server(port: u16, priority: u32, connections: usize) -> ServerConfig {
        let mut s = ServerConfig::new("127.0.0.1", port);
        s.priority = priority;
        s.max_connections = connections;
        s.timeout = Duration::from_secs(5);
        s
    }

    #[tokio::test]
    async fn assembles_files_from_out_of_order_segments_over_several_connections() {
        let state = Arc::new(StdMutex::new(MockState::default()));
        let a = pattern(50_000, 1);
        let b = pattern(7_001, 9);
        let ids_a = post(&state, "a", "movie.mkv", &a, 7);
        let ids_b = post(&state, "b", "movie.nfo", &b, 1);
        let port = spawn(state.clone()).await;
        let dir = tempfile::tempdir().unwrap();

        let nzb = nzb_for(&[("obfuscated-subject-name", ids_a), ("other", ids_b)]);
        let control = Arc::new(Control::default());
        let report = download(&nzb, vec![server(port, 0, 4)], dir.path(), control.clone())
            .await
            .unwrap();

        assert_eq!(report.failed_segments(), 0);
        assert_eq!(
            report.files[0].name, "movie.mkv",
            "yEnc header name beats the NZB's"
        );
        assert_eq!(
            std::fs::read(report.files[0].path.as_ref().unwrap()).unwrap(),
            a
        );
        assert_eq!(
            std::fs::read(report.files[1].path.as_ref().unwrap()).unwrap(),
            b
        );
        assert_eq!(control.total_segments.load(Ordering::Relaxed), 8);
        assert_eq!(control.done_segments.load(Ordering::Relaxed), 8);
        assert_eq!(
            control.bytes_done.load(Ordering::Relaxed),
            (a.len() + b.len()) as u64
        );
        assert!(
            state.lock().unwrap().connections <= 4,
            "no more connections than configured"
        );
    }

    #[tokio::test]
    async fn missing_articles_fall_back_to_the_next_server() {
        let primary = Arc::new(StdMutex::new(MockState::default()));
        let backup = Arc::new(StdMutex::new(MockState::default()));
        let data = pattern(30_000, 3);
        let ids = post(&backup, "f", "file.bin", &data, 5);
        // The primary only has two of the five.
        for id in &ids[..2] {
            let article = backup.lock().unwrap().articles[id].clone();
            primary.lock().unwrap().articles.insert(id.clone(), article);
        }
        let (p1, p2) = (spawn(primary.clone()).await, spawn(backup.clone()).await);
        let dir = tempfile::tempdir().unwrap();

        let report = download(
            &nzb_for(&[("file.bin", ids)]),
            vec![server(p1, 0, 2), server(p2, 1, 2)],
            dir.path(),
            Arc::default(),
        )
        .await
        .unwrap();
        assert_eq!(report.failed_segments(), 0);
        assert_eq!(
            std::fs::read(report.files[0].path.as_ref().unwrap()).unwrap(),
            data
        );
        let backup_bodies = backup
            .lock()
            .unwrap()
            .log
            .iter()
            .filter(|l| l.starts_with("BODY"))
            .count();
        assert_eq!(
            backup_bodies, 3,
            "backup asked only for what the primary lacked"
        );
    }

    #[tokio::test]
    async fn articles_missing_everywhere_are_counted_and_the_rest_still_arrive() {
        let state = Arc::new(StdMutex::new(MockState::default()));
        let data = pattern(20_000, 5);
        let mut ids = post(&state, "g", "g.bin", &data, 4);
        state.lock().unwrap().articles.remove(&ids[2]);
        ids.push("never-posted@t".into());
        let port = spawn(state).await;
        let dir = tempfile::tempdir().unwrap();

        let control = Arc::new(Control::default());
        let report = download(
            &nzb_for(&[("g.bin", ids)]),
            vec![server(port, 0, 2)],
            dir.path(),
            control.clone(),
        )
        .await
        .unwrap();
        assert_eq!(report.files[0].segments_failed, 2);
        assert_eq!(control.failed_segments.load(Ordering::Relaxed), 2);
        let written = std::fs::read(report.files[0].path.as_ref().unwrap()).unwrap();
        assert_eq!(
            written.len(),
            data.len(),
            "file is full size with holes for par2 to fill"
        );
        assert_eq!(&written[..5000], &data[..5000]);
    }

    #[tokio::test]
    async fn a_file_with_nothing_fetched_has_no_path_and_cancel_stops_early() {
        let state = Arc::new(StdMutex::new(MockState::default()));
        let port = spawn(state).await;
        let dir = tempfile::tempdir().unwrap();
        let report = download(
            &nzb_for(&[("x.bin", vec!["nope@t".into()])]),
            vec![server(port, 0, 1)],
            dir.path(),
            Arc::default(),
        )
        .await
        .unwrap();
        assert_eq!(
            (
                report.files[0].path.clone(),
                report.files[0].segments_failed
            ),
            (None, 1)
        );

        let control = Arc::new(Control::default());
        control.cancelled.store(true, Ordering::Relaxed);
        let report = download(
            &nzb_for(&[("x.bin", vec!["nope@t".into()])]),
            vec![server(port, 0, 1)],
            dir.path(),
            control,
        )
        .await
        .unwrap();
        assert!(report.cancelled);
        assert_eq!(report.files[0].segments_failed, 0, "nothing was attempted");
    }

    #[test]
    fn hostile_names_are_confined_to_the_download_folder() {
        assert_eq!(safe_file_name("../../etc/passwd"), "passwd");
        assert_eq!(safe_file_name("..\\..\\x.exe"), "x.exe");
        assert_eq!(safe_file_name("..."), "unnamed");
        assert_eq!(safe_file_name("plain.mkv"), "plain.mkv");
    }
}
