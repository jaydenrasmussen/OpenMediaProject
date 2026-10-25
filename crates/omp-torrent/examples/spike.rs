//! Phase 0 spike: download a torrent with librqbit, stop, restart, and confirm
//! progress survives. Not part of the test suite (needs the network).
//!
//! cargo run --release -p omp-torrent --example spike -- <file.torrent> <out-dir> [phase1-secs] [phase2-secs]

use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::Result;
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, Session, SessionOptions,
    SessionPersistenceConfig,
};

async fn run_phase(
    name: &str,
    torrent: Vec<u8>,
    out: PathBuf,
    limit: Duration,
    stop_at_pct: Option<u64>,
) -> Result<bool> {
    let session: Arc<Session> = Session::new_with_opts(
        out.join("data"),
        SessionOptions {
            fastresume: true,
            persistence: Some(SessionPersistenceConfig::Json {
                folder: Some(out.join("session")),
            }),
            ..Default::default()
        },
    )
    .await?;

    let resp = session
        .add_torrent(
            AddTorrent::from_bytes(torrent),
            Some(AddTorrentOptions {
                overwrite: true,
                ..Default::default()
            }),
        )
        .await?;
    let handle = match resp {
        AddTorrentResponse::Added(_, h) | AddTorrentResponse::AlreadyManaged(_, h) => h,
        AddTorrentResponse::ListOnly(_) => unreachable!(),
    };

    let started = Instant::now();
    let mut finished = false;
    let mut first = true;
    while started.elapsed() < limit {
        // Sample before sleeping so the first reading is the state right after (re)start.
        let s = handle.stats();
        let pct = (s.progress_bytes * 100)
            .checked_div(s.total_bytes)
            .unwrap_or(0);
        if first {
            println!(
                "[{name}] first stats: {pct}% ({} of {} bytes) state={}",
                s.progress_bytes, s.total_bytes, s.state
            );
            first = false;
        }
        let (down, peers) = s
            .live
            .as_ref()
            .map(|l| (l.download_speed.to_string(), l.snapshot.peer_stats.live))
            .unwrap_or_default();
        println!(
            "[{name}] {pct}% down={down} live_peers={peers} state={}",
            s.state
        );
        if s.finished {
            finished = true;
            break;
        }
        if stop_at_pct.is_some_and(|p| pct >= p) {
            break;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    session.stop().await;
    Ok(finished)
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let torrent = std::fs::read(args.next().expect("torrent file"))?;
    let out = PathBuf::from(args.next().expect("output dir"));
    let p1: u64 = args.next().map(|s| s.parse()).transpose()?.unwrap_or(60);
    let p2: u64 = args.next().map(|s| s.parse()).transpose()?.unwrap_or(180);

    let done = run_phase(
        "run1",
        torrent.clone(),
        out.clone(),
        Duration::from_secs(p1),
        Some(30),
    )
    .await?;
    if !done {
        println!("--- restarting session ---");
        let done = run_phase("run2", torrent, out, Duration::from_secs(p2), None).await?;
        println!("completed after restart: {done}");
    }
    Ok(())
}
