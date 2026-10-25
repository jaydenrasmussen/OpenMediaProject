//! The NNTP client and downloader against a real news server (INN 2.7 in
//! Docker). Slow, so it only runs when asked:
//! `cargo test -p omp-usenet --test real_server -- --ignored --nocapture`
//! Set `OMP_NNTP_HOST` / `OMP_NNTP_PORT` to use a server that is already running.

use std::{
    process::Command,
    sync::Arc,
    time::{Duration, Instant},
};

use omp_usenet::{Control, Nzb, ServerConfig, download, encode_part};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

fn docker(args: &[&str]) -> String {
    let out = Command::new("docker").args(args).output().expect("docker");
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// Starts INN with one group, readable and postable by anyone.
fn start_inn(port: u16) -> String {
    let name = "omp-real-inn";
    docker(&["rm", "-f", name]);
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/inn");
    let out = docker(&[
        "run",
        "-d",
        "--name",
        name,
        "-h",
        "news.example.com",
        "-p",
        &format!("{port}:119"),
        "-v",
        &format!("{}:/inn", dir.display()),
        "debian:bookworm-slim",
        "/inn/run.sh",
    ]);
    assert!(out.len() >= 12, "docker run failed: {out}");
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(180) {
        if let Ok(mut s) = std::net::TcpStream::connect(("127.0.0.1", port)) {
            use std::io::{Read, Write};
            s.set_read_timeout(Some(Duration::from_secs(3))).ok();
            let mut buf = [0u8; 256];
            let n = s.read(&mut buf).unwrap_or(0);
            if String::from_utf8_lossy(&buf[..n]).starts_with("200") {
                s.write_all(b"GROUP alt.binaries.test\r\n").ok();
                let n = s.read(&mut buf).unwrap_or(0);
                if String::from_utf8_lossy(&buf[..n]).starts_with("211") {
                    return name.into();
                }
            }
        }
        std::thread::sleep(Duration::from_secs(2));
    }
    panic!("INN never came up: {}", docker(&["logs", name]));
}

/// Posts one article with NNTP's POST and returns its message id.
async fn post(port: u16, message_id: &str, subject: &str, body: &[u8]) {
    let stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    let (r, mut w) = stream.into_split();
    let mut r = BufReader::new(r);
    let mut line = String::new();
    r.read_line(&mut line).await.unwrap();
    assert!(line.starts_with("200"), "{line}");
    w.write_all(b"POST\r\n").await.unwrap();
    line.clear();
    r.read_line(&mut line).await.unwrap();
    assert!(line.starts_with("340"), "POST refused: {line}");
    let head = format!(
        "From: omp@example.com\r\nNewsgroups: alt.binaries.test\r\nSubject: {subject}\r\nMessage-ID: <{message_id}>\r\n\r\n"
    );
    w.write_all(head.as_bytes()).await.unwrap();
    // Dot-stuff any line starting with a dot.
    let mut stuffed = Vec::new();
    let mut at_line_start = true;
    for &b in body {
        if at_line_start && b == b'.' {
            stuffed.push(b'.');
        }
        stuffed.push(b);
        at_line_start = b == b'\n';
    }
    w.write_all(&stuffed).await.unwrap();
    w.write_all(b"\r\n.\r\n").await.unwrap();
    line.clear();
    r.read_line(&mut line).await.unwrap();
    assert!(line.starts_with("240"), "article refused: {line}");
    w.write_all(b"QUIT\r\n").await.unwrap();
}

#[tokio::test]
#[ignore = "needs Docker; starts a real INN news server"]
async fn a_file_posted_to_a_real_server_downloads_and_matches() {
    let port: u16 = std::env::var("OMP_NNTP_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(18119);
    let host = std::env::var("OMP_NNTP_HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let container = std::env::var("OMP_NNTP_HOST")
        .is_err()
        .then(|| start_inn(port));

    // 300 KB of data that does not compress or repeat, in three parts.
    let mut data = Vec::new();
    let mut x: u32 = 12345;
    while data.len() < 300_000 {
        x = x.wrapping_mul(1664525).wrapping_add(1013904223);
        data.push((x >> 24) as u8);
    }
    let parts: Vec<&[u8]> = data.chunks(100_000).collect();
    let stamp = std::process::id();
    let mut segments = String::new();
    for (i, chunk) in parts.iter().enumerate() {
        let id = format!("real-{stamp}-{i}@omp.test");
        let encoded = encode_part(
            "real.bin",
            data.len() as u64,
            (i * 100_000) as u64,
            chunk,
            i as u32 + 1,
            parts.len() as u32,
        );
        post(
            port,
            &id,
            &format!("real.bin yEnc ({}/{})", i + 1, parts.len()),
            &encoded,
        )
        .await;
        segments += &format!(
            r#"<segment bytes="{}" number="{}">{id}</segment>"#,
            chunk.len(),
            i + 1
        );
    }
    // One article that was never posted: the download must report it missing, not hang.
    segments += &format!(r#"<segment bytes="10" number="4">missing-{stamp}@omp.test</segment>"#);
    let nzb = Nzb::parse(&format!(
        r#"<?xml version="1.0"?><nzb xmlns="http://www.newzbin.com/DTD/2003/nzb"><file poster="omp@example.com" date="1700000000" subject="real.bin yEnc (1/4)"><groups><group>alt.binaries.test</group></groups><segments>{segments}</segments></file></nzb>"#
    ))
    .unwrap();

    let dir = tempfile::tempdir().unwrap();
    let mut server = ServerConfig::new(host, port);
    server.timeout = Duration::from_secs(20);
    let report = download(&nzb, vec![server], dir.path(), Arc::new(Control::default()))
        .await
        .unwrap();
    assert_eq!(report.files.len(), 1);
    assert_eq!(
        report.failed_segments(),
        1,
        "only the unposted article is missing"
    );

    let got = std::fs::read(dir.path().join("real.bin")).unwrap();
    assert_eq!(
        got.len(),
        data.len(),
        "the file is its full size, with a hole where the lost part was"
    );
    assert_eq!(
        &got[..300_000],
        &data[..],
        "every posted byte arrived intact"
    );

    if let Some(c) = container {
        docker(&["rm", "-f", &c]);
    }
}
