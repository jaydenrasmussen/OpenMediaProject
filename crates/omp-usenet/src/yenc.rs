//! yEnc decoding of article bodies.
//!
//! An article body looks like
//! ```text
//! =ybegin part=1 total=3 line=128 size=1000 name=file.bin
//! =ypart begin=1 end=500
//! <encoded bytes>
//! =yend size=500 part=1 pcrc32=1a2b3c4d
//! ```
//! A single-part post has no `=ypart` line and a `crc32=` on `=yend`.

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum YencError {
    #[error("no =ybegin line")]
    NoBegin,
    #[error("malformed yEnc header: {0}")]
    Header(String),
    #[error("article is truncated (no =yend)")]
    NoEnd,
    #[error("decoded {got} bytes but the trailer says {expected}")]
    SizeMismatch { got: u64, expected: u64 },
    #[error("checksum mismatch: computed {got:08x}, trailer says {expected:08x}")]
    CrcMismatch { got: u32, expected: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YencPart {
    pub name: String,
    /// Size of the whole file.
    pub file_size: u64,
    /// 0-based offset of this part in the file.
    pub offset: u64,
    pub data: Vec<u8>,
}

fn attr<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    // `name=` runs to the end of the line and may contain spaces; the rest are single tokens.
    let marker = format!(" {key}=");
    let start = line.find(&marker)? + marker.len();
    let rest = &line[start..];
    if key == "name" {
        Some(rest.trim_end())
    } else {
        Some(rest.split_whitespace().next().unwrap_or(""))
    }
}

fn number(line: &str, key: &str) -> Result<Option<u64>, YencError> {
    attr(line, key)
        .map(|v| {
            v.parse::<u64>()
                .map_err(|_| YencError::Header(format!("{key}={v}")))
        })
        .transpose()
}

fn hex(line: &str, key: &str) -> Option<u32> {
    attr(line, key).and_then(|v| u32::from_str_radix(v.trim(), 16).ok())
}

/// Decodes one article body (already free of NNTP dot-stuffing).
pub fn decode_part(body: &[u8]) -> Result<YencPart, YencError> {
    let mut lines = body
        .split(|&b| b == b'\n')
        .map(|l| l.strip_suffix(b"\r").unwrap_or(l));

    let begin = loop {
        let line = lines.next().ok_or(YencError::NoBegin)?;
        if line.starts_with(b"=ybegin ") {
            break String::from_utf8_lossy(line).into_owned();
        }
    };
    let file_size =
        number(&begin, "size")?.ok_or_else(|| YencError::Header("missing size".into()))?;
    let name = attr(&begin, "name").unwrap_or_default().to_owned();
    let multipart = attr(&begin, "part").is_some();

    let mut offset = 0;
    let mut data = Vec::new();
    let mut trailer = None;
    let mut first = true;
    for line in lines {
        if first && line.starts_with(b"=ypart ") {
            let text = String::from_utf8_lossy(line);
            let begin = number(&text, "begin")?
                .ok_or_else(|| YencError::Header("ypart without begin".into()))?;
            offset = begin.saturating_sub(1);
            first = false;
            continue;
        }
        first = false;
        if line.starts_with(b"=yend") {
            trailer = Some(String::from_utf8_lossy(line).into_owned());
            break;
        }
        let mut iter = line.iter();
        while let Some(&b) = iter.next() {
            let byte = if b == b'=' {
                match iter.next() {
                    Some(&n) => n.wrapping_sub(64),
                    None => break,
                }
            } else {
                b
            };
            data.push(byte.wrapping_sub(42));
        }
    }
    let trailer = trailer.ok_or(YencError::NoEnd)?;

    // A trailer without a size was cut off mid-line.
    let Some(expected) = number(&trailer, "size")? else {
        return Err(YencError::NoEnd);
    };
    if expected != data.len() as u64 {
        return Err(YencError::SizeMismatch {
            got: data.len() as u64,
            expected,
        });
    }
    // Multi-part posts carry the part's crc as pcrc32; single-part ones as crc32.
    let expected_crc = if multipart {
        hex(&trailer, "pcrc32")
    } else {
        hex(&trailer, "crc32").or_else(|| hex(&trailer, "pcrc32"))
    };
    if let Some(expected) = expected_crc {
        let got = crc32fast::hash(&data);
        if got != expected {
            return Err(YencError::CrcMismatch { got, expected });
        }
    }
    Ok(YencPart {
        name,
        file_size,
        offset,
        data,
    })
}

/// Encodes one part. Used by tests and to build mock servers.
pub fn encode_part(
    name: &str,
    file_size: u64,
    offset: u64,
    data: &[u8],
    part: u32,
    total: u32,
) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend(
        format!("=ybegin part={part} total={total} line=128 size={file_size} name={name}\r\n")
            .bytes(),
    );
    out.extend(
        format!(
            "=ypart begin={} end={}\r\n",
            offset + 1,
            offset + data.len() as u64
        )
        .bytes(),
    );
    let mut column = 0;
    for &b in data {
        let enc = b.wrapping_add(42);
        let critical = matches!(enc, 0 | b'\n' | b'\r' | b'=')
            || (column == 0 && matches!(enc, b'.' | b' ' | b'\t'));
        if critical {
            out.push(b'=');
            out.push(enc.wrapping_add(64));
            column += 2;
        } else {
            out.push(enc);
            column += 1;
        }
        if column >= 128 {
            out.extend(b"\r\n");
            column = 0;
        }
    }
    if column > 0 {
        out.extend(b"\r\n");
    }
    out.extend(
        format!(
            "=yend size={} part={part} pcrc32={:08x}\r\n",
            data.len(),
            crc32fast::hash(data)
        )
        .bytes(),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_every_byte_value_across_line_breaks() {
        let data: Vec<u8> = (0..=255u8).cycle().take(5000).collect();
        let encoded = encode_part("all bytes.bin", 12_000, 4000, &data, 2, 3);
        let part = decode_part(&encoded).unwrap();
        assert_eq!(part.data, data);
        assert_eq!(part.name, "all bytes.bin", "names may contain spaces");
        assert_eq!((part.file_size, part.offset), (12_000, 4000));
    }

    #[test]
    fn single_part_posts_use_crc32() {
        let data = b"hello world";
        let enc: Vec<u8> = data.iter().map(|b| b.wrapping_add(42)).collect();
        let mut body = b"=ybegin line=128 size=11 name=h.txt\r\n".to_vec();
        body.extend(&enc);
        body.extend(format!("\r\n=yend size=11 crc32={:08x}\r\n", crc32fast::hash(data)).bytes());
        let p = decode_part(&body).unwrap();
        assert_eq!(
            (p.data.as_slice(), p.offset, p.file_size),
            (&data[..], 0, 11)
        );
    }

    #[test]
    fn corruption_is_detected() {
        let mut encoded = encode_part("a.bin", 100, 0, &[1, 2, 3, 4, 5, 6, 7, 8], 1, 1);
        // Flip a data byte (after the two header lines).
        let first_data = encoded.iter().position(|&b| b == b'\n').unwrap();
        let second = first_data
            + 1
            + encoded[first_data + 1..]
                .iter()
                .position(|&b| b == b'\n')
                .unwrap()
            + 1;
        encoded[second] ^= 0x10;
        assert!(matches!(
            decode_part(&encoded),
            Err(YencError::CrcMismatch { .. })
        ));

        let full = encode_part("a.bin", 100, 0, &[1, 2, 3], 1, 1);
        let cut = &full[..full.len() - 30];
        assert_eq!(decode_part(cut), Err(YencError::NoEnd));
        assert_eq!(decode_part(b"just some text\r\n"), Err(YencError::NoBegin));
    }

    #[test]
    fn size_mismatch_is_detected() {
        let body = b"=ybegin part=1 total=1 line=128 size=10 name=x\r\n=ypart begin=1 end=10\r\nabc\r\n=yend size=10 part=1\r\n";
        assert!(matches!(
            decode_part(body),
            Err(YencError::SizeMismatch {
                got: 3,
                expected: 10
            })
        ));
    }
}
