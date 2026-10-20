//! Turns ffprobe's JSON into the values the `{MediaInfo ...}` naming tokens
//! use. Radarr gets these from MediaInfo; ffprobe says the same things in
//! different words, so the codec names are mapped to the ones people expect
//! to see in file names.

use omp_naming::NamingMedia;
use serde_json::Value;

fn str_of<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn video_codec(stream: &Value, scene: &str) -> String {
    let scene = scene.to_lowercase();
    match str_of(stream, "codec_name") {
        "h264" => if scene.contains("h264") {
            "h264"
        } else {
            "x264"
        }
        .to_owned(),
        "hevc" => if scene.contains("h265") {
            "h265"
        } else {
            "x265"
        }
        .to_owned(),
        "mpeg2video" => "MPEG2".into(),
        "mpeg4" => if scene.contains("divx") {
            "DivX"
        } else {
            "XviD"
        }
        .to_owned(),
        "msmpeg4v3" | "msmpeg4v2" => "DivX".into(),
        "vc1" => "VC1".into(),
        "vp9" => "VP9".into(),
        "av1" => "AV1".into(),
        "" => String::new(),
        other => other.to_uppercase(),
    }
}

fn audio_codec(stream: &Value) -> String {
    let profile = str_of(stream, "profile");
    let atmos = profile.to_lowercase().contains("atmos");
    match str_of(stream, "codec_name") {
        "ac3" => "AC3".into(),
        "eac3" => if atmos { "EAC3 Atmos" } else { "EAC3" }.to_owned(),
        "truehd" => if atmos { "TrueHD Atmos" } else { "TrueHD" }.to_owned(),
        "dts" => {
            let p = profile.to_lowercase();
            if p.contains("dts:x") || p.contains("dts-x") {
                "DTS-X".into()
            } else if p.contains("ma") && p.contains("hd") {
                "DTS-HD MA".into()
            } else if p.contains("hra") || (p.contains("hd") && p.contains("high")) {
                "DTS-HD HRA".into()
            } else if p.contains("es") {
                "DTS-ES".into()
            } else {
                "DTS".into()
            }
        }
        "aac" => if profile.contains("HE") {
            "HE-AAC"
        } else {
            "AAC"
        }
        .to_owned(),
        "mp3" => "MP3".into(),
        "mp2" => "MP2".into(),
        "flac" => "FLAC".into(),
        "opus" => "Opus".into(),
        "vorbis" => "Vorbis".into(),
        c if c.starts_with("pcm") => "PCM".into(),
        "" => String::new(),
        other => other.to_uppercase(),
    }
}

/// Channel count as the usual "5.1": every channel after the second counts
/// as a full channel plus the subwoofer.
fn channels(count: i64) -> f64 {
    match count {
        n if n <= 0 => 0.0,
        1 | 2 => count as f64,
        n => (n - 1) as f64 + 0.1,
    }
}

fn bit_depth(stream: &Value) -> i32 {
    if let Some(bits) = str_of(stream, "bits_per_raw_sample")
        .parse::<i32>()
        .ok()
        .filter(|b| *b > 0)
    {
        return bits;
    }
    let fmt = str_of(stream, "pix_fmt");
    if fmt.contains("12") {
        12
    } else if fmt.contains("10") {
        10
    } else {
        8
    }
}

fn has_side_data(stream: &Value, needle: &str) -> bool {
    stream
        .get("side_data_list")
        .and_then(Value::as_array)
        .is_some_and(|l| {
            l.iter()
                .any(|d| str_of(d, "side_data_type").to_lowercase().contains(needle))
        })
}

/// "HDR" and what kind, or two empty strings for ordinary video.
fn dynamic_range(stream: &Value) -> (String, String) {
    let transfer = str_of(stream, "color_transfer");
    let dolby = has_side_data(stream, "dovi");
    let base = match transfer {
        "smpte2084"
            if has_side_data(stream, "smpte2094-40")
                || has_side_data(stream, "dynamic hdr plus") =>
        {
            "HDR10Plus"
        }
        "smpte2084" => "HDR10",
        "arib-std-b67" => "HLG",
        _ => "",
    };
    match (dolby, base) {
        (false, "") => (String::new(), String::new()),
        (true, "") => ("HDR".into(), "DV".into()),
        (true, b) => ("HDR".into(), format!("DV {b}")),
        (false, b) => ("HDR".into(), b.to_owned()),
    }
}

fn languages(streams: &[Value], kind: &str) -> Vec<String> {
    streams
        .iter()
        .filter(|s| str_of(s, "codec_type") == kind)
        .map(|s| {
            s.pointer("/tags/language")
                .and_then(Value::as_str)
                .unwrap_or("und")
                .to_owned()
        })
        .collect()
}

/// `None` when the probe has no video or audio to describe.
pub fn naming_media(raw: &Value, scene_name: Option<&str>) -> Option<NamingMedia> {
    let streams = raw.get("streams")?.as_array()?;
    let video = streams.iter().find(|s| str_of(s, "codec_type") == "video");
    let audio = streams
        .iter()
        .filter(|s| str_of(s, "codec_type") == "audio")
        .max_by_key(|s| {
            s.pointer("/disposition/default")
                .and_then(Value::as_i64)
                .unwrap_or(0)
        });
    if video.is_none() && audio.is_none() {
        return None;
    }
    let scene = scene_name.unwrap_or_default();
    let (range, range_type) = video.map(dynamic_range).unwrap_or_default();
    Some(NamingMedia {
        video_codec: video.map(|v| video_codec(v, scene)).unwrap_or_default(),
        video_bit_depth: video.map(bit_depth).unwrap_or(0),
        audio_codec: audio.map(audio_codec).unwrap_or_default(),
        audio_channels: audio
            .and_then(|a| a.get("channels").and_then(Value::as_i64))
            .map(channels)
            .unwrap_or(0.0),
        audio_languages: languages(streams, "audio"),
        subtitles: languages(streams, "subtitle"),
        is_3d: false,
        dynamic_range: range,
        dynamic_range_type: range_type,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn probe() -> Value {
        json!({ "streams": [
            { "codec_type": "video", "codec_name": "hevc", "pix_fmt": "yuv420p10le",
              "color_transfer": "smpte2084",
              "side_data_list": [{ "side_data_type": "DOVI configuration record" }] },
            { "codec_type": "audio", "codec_name": "truehd", "profile": "Dolby TrueHD + Dolby Atmos",
              "channels": 8, "tags": { "language": "eng" }, "disposition": { "default": 1 } },
            { "codec_type": "audio", "codec_name": "ac3", "channels": 6, "tags": { "language": "ger" } },
            { "codec_type": "subtitle", "codec_name": "subrip", "tags": { "language": "eng" } },
            { "codec_type": "subtitle", "codec_name": "subrip" },
        ]})
    }

    #[test]
    fn describes_a_modern_remux() {
        let m = naming_media(&probe(), Some("Movie.2023.2160p.UHD.BluRay.REMUX.HEVC-GRP")).unwrap();
        assert_eq!(m.video_codec, "x265");
        assert_eq!(m.video_bit_depth, 10);
        assert_eq!(
            (m.audio_codec.as_str(), m.audio_channels),
            ("TrueHD Atmos", 7.1)
        );
        assert_eq!(m.audio_languages, ["eng", "ger"]);
        assert_eq!(m.subtitles, ["eng", "und"]);
        assert_eq!(
            (m.dynamic_range.as_str(), m.dynamic_range_type.as_str()),
            ("HDR", "DV HDR10")
        );
    }

    #[test]
    fn scene_names_keep_their_spelling_of_the_codec() {
        let p = json!({ "streams": [{ "codec_type": "video", "codec_name": "h264" }] });
        assert_eq!(
            naming_media(&p, Some("Movie.1080p.BluRay.x264-GRP"))
                .unwrap()
                .video_codec,
            "x264"
        );
        assert_eq!(
            naming_media(&p, Some("Movie.1080p.WEB.H264-GRP"))
                .unwrap()
                .video_codec,
            "h264"
        );
        assert_eq!(naming_media(&p, None).unwrap().video_codec, "x264");
    }

    #[test]
    fn channel_counts_read_like_layouts() {
        assert_eq!(
            [1, 2, 3, 6, 7, 8].map(channels),
            [1.0, 2.0, 2.1, 5.1, 6.1, 7.1]
        );
    }

    #[test]
    fn plain_video_has_no_dynamic_range_and_empty_probes_are_nothing() {
        let sdr = json!({ "streams": [{ "codec_type": "video", "codec_name": "h264", "pix_fmt": "yuv420p" }] });
        let m = naming_media(&sdr, None).unwrap();
        assert_eq!((m.dynamic_range.as_str(), m.video_bit_depth), ("", 8));
        assert!(naming_media(&json!({ "streams": [{ "codec_type": "data" }] }), None).is_none());
        assert!(naming_media(&json!({}), None).is_none());
    }

    #[test]
    fn audio_profiles_pick_the_dts_flavour() {
        let dts = |profile: &str| audio_codec(&json!({ "codec_name": "dts", "profile": profile }));
        assert_eq!(dts("DTS-HD MA"), "DTS-HD MA");
        assert_eq!(dts("DTS-HD HRA"), "DTS-HD HRA");
        assert_eq!(dts("DTS"), "DTS");
        assert_eq!(dts("DTS-ES"), "DTS-ES");
    }
}
