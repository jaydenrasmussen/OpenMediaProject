//! Runs the real ffprobe/ffmpeg when they are installed.

use std::process::Command;

use omp_services::{Ffprobe, MediaProbe};

fn have(tool: &str) -> bool {
    Command::new(tool)
        .arg("-version")
        .output()
        .is_ok_and(|o| o.status.success())
}

#[tokio::test]
async fn probes_a_real_file_and_survives_a_bad_one() {
    if !have("ffmpeg") || !have("ffprobe") {
        eprintln!("ffmpeg/ffprobe not installed; skipping");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let video = dir.path().join("clip.mkv");
    let made = Command::new("ffmpeg")
        .args([
            "-v",
            "quiet",
            "-f",
            "lavfi",
            "-i",
            "testsrc=duration=2:size=64x48:rate=10",
            "-f",
            "lavfi",
            "-i",
            "sine=duration=2",
            "-shortest",
        ])
        .arg(&video)
        .status()
        .unwrap();
    assert!(made.success());

    let probe = Ffprobe::default();
    let info = probe.probe(&video).await.expect("probe");
    assert_eq!(info.audio_streams, 1);
    assert!(
        (1.5..2.5).contains(&info.runtime_seconds),
        "{}",
        info.runtime_seconds
    );

    let junk = dir.path().join("junk.mkv");
    std::fs::write(&junk, b"definitely not a video").unwrap();
    assert!(probe.probe(&junk).await.is_none());
    assert!(probe.probe(&dir.path().join("missing.mkv")).await.is_none());
    assert!(
        Ffprobe::new("/no/such/ffprobe")
            .probe(&video)
            .await
            .is_none()
    );
}
