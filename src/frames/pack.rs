//! How the archive is stored. Format 1: zstd of `{"gameMetadata": …, "frames": [ … ]}`, the
//! frames as the feed sent them, oldest first. A new format gets a new number: old rows keep
//! theirs and stay readable

use chrono::{DateTime, Utc};
use color_eyre::{
    Result,
    eyre::{WrapErr, eyre},
};
use serde_json::value::RawValue;

use super::{RawFrame, parse_frames};

pub const FORMAT: i16 = 1;
/// Slow to write (~1 s a game), small to keep. Reading doesn't depend on it
pub const LEVEL: i32 = 19;
/// No real game unpacks to this (a 33 min game is 56 MB of JSON): a bigger one is a broken row
const MAX_RAW: u64 = 1 << 30;

#[derive(Debug)]
pub struct Packed {
    pub data: Vec<u8>,
    /// JSON size before compression
    pub raw_bytes: i32,
    pub frame_count: i32,
    pub first: DateTime<Utc>,
    pub last: DateTime<Utc>,
}

pub fn pack(metadata: Option<&RawValue>, frames: &[RawFrame]) -> Result<Packed> {
    let (Some(first), Some(last)) = (frames.first(), frames.last()) else {
        return Err(eyre!("no frames to pack"));
    };
    let mut json =
        String::with_capacity(frames.iter().map(|f| f.raw.get().len() + 1).sum::<usize>() + 64);
    json.push_str(r#"{"gameMetadata":"#);
    json.push_str(metadata.map_or("null", RawValue::get));
    json.push_str(r#","frames":["#);
    for (i, f) in frames.iter().enumerate() {
        if i > 0 {
            json.push(',');
        }
        json.push_str(f.raw.get());
    }
    json.push_str("]}");
    let data = zstd::encode_all(json.as_bytes(), LEVEL).wrap_err("zstd")?;
    Ok(Packed {
        data,
        raw_bytes: i32::try_from(json.len()).wrap_err("archive over 2 GB")?,
        frame_count: i32::try_from(frames.len()).wrap_err("frame count")?,
        first: first.ts,
        last: last.ts,
    })
}

pub fn unpack(format: i16, data: &[u8]) -> Result<(Option<Box<RawValue>>, Vec<RawFrame>)> {
    if format != FORMAT {
        return Err(eyre!(
            "archive format {format} unknown (this build reads {FORMAT})"
        ));
    }
    use std::io::Read;
    let mut json = Vec::new();
    zstd::stream::read::Decoder::new(data)
        .wrap_err("archive: zstd")?
        .take(MAX_RAW + 1)
        .read_to_end(&mut json)
        .wrap_err("archive: zstd")?;
    if json.len() as u64 > MAX_RAW {
        return Err(eyre!("archive over {MAX_RAW} bytes once unpacked"));
    }
    let json = String::from_utf8(json).wrap_err("archive: not UTF-8")?;
    parse_frames(&json).wrap_err("archive")
}

/// Postgres `bytea` as text both ways (Canyon only sends text): `decode($1, 'hex')`,
/// `encode(data, 'hex')`
pub fn to_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(DIGITS[usize::from(b >> 4)] as char);
        s.push(DIGITS[usize::from(b & 0xf)] as char);
    }
    s
}

pub fn from_hex(s: &str) -> Result<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return Err(eyre!("hex of odd length {}", s.len()));
    }
    let digit = |c: u8| match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        b'A'..=b'F' => Ok(c - b'A' + 10),
        _ => Err(eyre!("bad hex digit {:?}", c as char)),
    };
    s.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|[hi, lo]| Ok(digit(*hi)? << 4 | digit(*lo)?))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frames::hand;

    fn frames() -> Vec<RawFrame> {
        let (w, _) = hand::game(30, true);
        w.iter().map(hand::raw).collect()
    }

    #[test]
    fn round_trip_keeps_every_byte() {
        let mut f = frames();
        // a field we don't know and odd spacing survive
        f[3] = RawFrame::new(
            RawValue::from_string(
                r#"{"rfc460Timestamp":"2026-10-31T23:05:10.091Z","gameState":"in_game","x":  {"new":true}}"#
                    .into(),
            )
            .unwrap(),
        )
        .unwrap();
        let meta = RawValue::from_string(hand::metadata().to_string()).unwrap();
        let p = pack(Some(&meta), &f).unwrap();
        assert_eq!(p.frame_count, 31);
        assert_eq!(p.first, f[0].ts);
        assert_eq!(p.last, f[30].ts);
        assert!(p.data.len() < p.raw_bytes as usize);
        let (m, back) = unpack(FORMAT, &p.data).unwrap();
        assert_eq!(m.unwrap().get(), meta.get());
        assert_eq!(back, f);
        assert!(back[3].raw.get().contains(r#""x":  {"new":true}"#));
        // details rows have no metadata
        let (m, _) = unpack(FORMAT, &pack(None, &f).unwrap().data).unwrap();
        assert!(m.is_none());
    }

    #[test]
    fn unknown_format_and_bad_data_are_errors() {
        let p = pack(None, &frames()).unwrap();
        let e = unpack(2, &p.data).unwrap_err();
        assert!(e.to_string().contains("format 2"), "{e}");
        assert!(unpack(FORMAT, b"not zstd").is_err());
        let mut cut = p.data.clone();
        cut.truncate(cut.len() / 2);
        assert!(unpack(FORMAT, &cut).is_err());
        assert!(pack(None, &[]).is_err());
    }

    #[test]
    fn hex_both_ways() {
        let bytes: Vec<u8> = (0..=255).collect();
        assert_eq!(from_hex(&to_hex(&bytes)).unwrap(), bytes);
        assert_eq!(to_hex(&[0x0f, 0xa0]), "0fa0");
        assert_eq!(from_hex("0FA0").unwrap(), vec![0x0f, 0xa0]);
        assert!(from_hex("abc").is_err());
        assert!(from_hex("zz").is_err());
    }
}
