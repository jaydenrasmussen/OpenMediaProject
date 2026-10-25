//! A [`MediaProbe`] that asks `ffprobe`.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use async_trait::async_trait;

use crate::import::{MediaProbe, ProbeInfo};

pub struct Ffprobe {
    binary: PathBuf,
    timeout: Duration,
}

impl Ffprobe {
    pub fn new(binary: impl Into<PathBuf>) -> Self {
        Self {
            binary: binary.into(),
            timeout: Duration::from_secs(60),
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

impl Default for Ffprobe {
    /// Finds `ffprobe` on the PATH.
    fn default() -> Self {
        Self::new("ffprobe")
    }
}

/// Reads the runtime and audio stream count out of ffprobe's JSON.
pub fn parse_probe_output(json: &str) -> Option<ProbeInfo> {
    let raw: serde_json::Value = serde_json::from_str(json).ok()?;
    let runtime_seconds = raw
        .pointer("/format/duration")
        .and_then(|d| {
            d.as_str()
                .and_then(|s| s.parse::<f64>().ok())
                .or_else(|| d.as_f64())
        })
        .unwrap_or(0.0);
    let streams = raw.get("streams")?.as_array()?;
    let audio_streams = streams
        .iter()
        .filter(|s| s.get("codec_type").and_then(|t| t.as_str()) == Some("audio"))
        .count() as u32;
    // A file with no streams at all is not media.
    if streams.is_empty() {
        return None;
    }
    Some(ProbeInfo {
        runtime_seconds,
        audio_streams,
        raw,
    })
}

#[async_trait]
impl MediaProbe for Ffprobe {
    async fn probe(&self, path: &Path) -> Option<ProbeInfo> {
        let run = tokio::process::Command::new(&self.binary)
            .args([
                "-v",
                "quiet",
                "-print_format",
                "json",
                "-show_format",
                "-show_streams",
            ])
            .arg(path)
            .kill_on_drop(true)
            .output();
        let output = match tokio::time::timeout(self.timeout, run).await {
            Ok(Ok(o)) if o.status.success() => o,
            Ok(Ok(o)) => {
                tracing::warn!(file = %path.display(), "ffprobe failed: {}", o.status);
                return None;
            }
            Ok(Err(e)) => {
                tracing::warn!("could not run ffprobe: {e}");
                return None;
            }
            Err(_) => {
                tracing::warn!(file = %path.display(), "ffprobe timed out");
                return None;
            }
        };
        parse_probe_output(&String::from_utf8_lossy(&output.stdout))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_runtime_and_audio_streams() {
        let json = r#"{"streams":[{"codec_type":"video"},{"codec_type":"audio"},{"codec_type":"audio"},{"codec_type":"subtitle"}],"format":{"duration":"7265.120000"}}"#;
        let p = parse_probe_output(json).unwrap();
        assert_eq!(p.audio_streams, 2);
        assert!((p.runtime_seconds - 7265.12).abs() < 1e-6);
    }

    #[test]
    fn missing_duration_is_zero_and_garbage_is_none() {
        let p = parse_probe_output(r#"{"streams":[{"codec_type":"video"}],"format":{}}"#).unwrap();
        assert_eq!((p.runtime_seconds, p.audio_streams), (0.0, 0));
        assert!(parse_probe_output("not json").is_none());
        assert!(parse_probe_output(r#"{"streams":[],"format":{}}"#).is_none());
    }
}
