//! Caption fetching and conversion.
//!
//! YouTube timedtext comes in two shapes, both parsed here:
//! - **JSON3** (`fmt=json3`): `{"events": [{"tStartMs", "dDurationMs",
//!   "segs": [{"utf8": "..."}]}]}`. Events without `segs` are skipped.
//! - **srv3 XML** (the default `base_url` response): `<text start="0"
//!   dur="2">...</text>` with XML entities. Parsed by a minimal scanner
//!   for exactly that shape — not a general XML parser; a malformed
//!   document yields an error, never guessed cues.
//!
//! Cues are converted to WebVTT for mpv (`sub-add`). The track URL is
//! never logged (see `logging.rs` in the app crate).

use serde_json::Value;

use crate::error::{Result, SourceError};

#[derive(Debug, Clone, PartialEq)]
pub struct CaptionCue {
    pub start_secs: f64,
    pub end_secs: f64,
    pub text: String,
}

/// Parse a JSON3 timedtext document.
pub fn parse_json3(json: &Value) -> Result<Vec<CaptionCue>> {
    let events = json
        .get("events")
        .and_then(|e| e.as_array())
        .ok_or_else(|| SourceError::Parse("json3: missing events array".into()))?;
    let mut cues = Vec::new();
    for ev in events {
        let Some(segs) = ev.get("segs").and_then(|s| s.as_array()) else {
            continue;
        };
        let text: String = segs
            .iter()
            .filter_map(|s| s.get("utf8").and_then(|u| u.as_str()))
            .collect::<Vec<_>>()
            .join("");
        let text = text.trim().to_string();
        if text.is_empty() {
            continue;
        }
        let start_ms = ev.get("tStartMs").and_then(|v| v.as_u64()).unwrap_or(0);
        let dur_ms = ev.get("dDurationMs").and_then(|v| v.as_u64()).unwrap_or(0);
        cues.push(CaptionCue {
            start_secs: start_ms as f64 / 1000.0,
            end_secs: (start_ms + dur_ms) as f64 / 1000.0,
            text,
        });
    }
    cues.sort_by(|a, b| {
        a.start_secs
            .partial_cmp(&b.start_secs)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(cues)
}

fn decode_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
}

/// Parse an srv3 timedtext document. Minimal scanner for
/// `<text start=".." dur="..">...</text>` elements only.
pub fn parse_srv3(xml: &str) -> Result<Vec<CaptionCue>> {
    let mut cues = Vec::new();
    let mut rest = xml;
    while let Some(open) = rest.find("<text ") {
        let tag = &rest[open..];
        let Some(tag_end) = tag.find('>') else {
            return Err(SourceError::Parse("srv3: unterminated <text> tag".into()));
        };
        let tag = &tag[..tag_end];
        let attr = |name: &str| -> Option<f64> {
            let pat = format!("{name}=\"");
            let start = tag.find(&pat)? + pat.len();
            let end = tag[start..].find('"')? + start;
            tag[start..end].parse().ok()
        };
        let (Some(start), Some(dur)) = (attr("start"), attr("dur")) else {
            return Err(SourceError::Parse("srv3: <text> missing start/dur".into()));
        };
        let body_start = open + tag_end + 1;
        let Some(close) = rest[body_start..].find("</text>") else {
            return Err(SourceError::Parse("srv3: unterminated <text> body".into()));
        };
        let text = decode_entities(&rest[body_start..body_start + close]);
        let text = text.trim().to_string();
        if !text.is_empty() {
            cues.push(CaptionCue {
                start_secs: start,
                end_secs: start + dur,
                text,
            });
        }
        rest = &rest[body_start + close + "</text>".len()..];
    }
    Ok(cues)
}

fn vtt_timestamp(secs: f64) -> String {
    let total_ms = (secs * 1000.0).round() as u64;
    let (h, m, s, ms) = (
        total_ms / 3_600_000,
        (total_ms / 60_000) % 60,
        (total_ms / 1000) % 60,
        total_ms % 1000,
    );
    format!("{h:02}:{m:02}:{s:02}.{ms:03}")
}

/// Convert cues to a WebVTT document for mpv.
pub fn to_vtt(cues: &[CaptionCue]) -> String {
    let mut out = String::from("WEBVTT\n\n");
    for cue in cues {
        out.push_str(&format!(
            "{} --> {}\n{}\n\n",
            vtt_timestamp(cue.start_secs),
            vtt_timestamp(cue.end_secs),
            cue.text
        ));
    }
    out
}

/// Fetch a caption track. The track's `base_url` returns srv3 XML by
/// default; appending `fmt=json3` asks for JSON3. If the body is not
/// JSON, fall back to the srv3 parser — never fail on a format guess.
pub async fn fetch_captions(client: &reqwest::Client, base_url: &str) -> Result<Vec<CaptionCue>> {
    let url = if base_url.contains("fmt=") {
        base_url.to_string()
    } else if base_url.contains('?') {
        format!("{base_url}&fmt=json3")
    } else {
        format!("{base_url}?fmt=json3")
    };
    let body = client
        .get(&url)
        .send()
        .await
        .map_err(|e| SourceError::Network(e.to_string()))?
        .text()
        .await
        .map_err(|e| SourceError::Network(e.to_string()))?;
    let trimmed = body.trim_start();
    if trimmed.starts_with('{') {
        let json: Value =
            serde_json::from_str(&body).map_err(|e| SourceError::Parse(e.to_string()))?;
        parse_json3(&json)
    } else if trimmed.starts_with('<') {
        parse_srv3(&body)
    } else {
        Err(SourceError::Parse(
            "caption response is neither JSON nor XML".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> String {
        let p: PathBuf = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name);
        std::fs::read_to_string(p).unwrap()
    }

    #[test]
    fn json3_parses_and_skips_empty_events() {
        let json: Value = serde_json::from_str(&fixture("captions.json3")).unwrap();
        let cues = parse_json3(&json).unwrap();
        assert_eq!(cues.len(), 3);
        assert_eq!(cues[0].text, "Hello world");
        assert_eq!(cues[0].start_secs, 0.0);
        assert_eq!(cues[0].end_secs, 2.0);
        assert_eq!(cues[1].text, "Second line\nwith a break");
        assert_eq!(cues[2].start_secs, 65.0);
    }

    #[test]
    fn srv3_parses_and_decodes_entities() {
        let cues = parse_srv3(&fixture("captions.srv3")).unwrap();
        assert_eq!(cues.len(), 3);
        assert_eq!(cues[0].text, "Hello & welcome");
        assert_eq!(cues[1].text, "It's a test");
        assert_eq!(cues[1].start_secs, 2.5);
        assert_eq!(cues[1].end_secs, 4.0);
    }

    #[test]
    fn vtt_roundtrip() {
        let json: Value = serde_json::from_str(&fixture("captions.json3")).unwrap();
        let cues = parse_json3(&json).unwrap();
        let vtt = to_vtt(&cues);
        assert!(vtt.starts_with("WEBVTT\n\n"));
        assert!(vtt.contains("00:00:00.000 --> 00:00:02.000\nHello world"));
        assert!(vtt.contains("00:01:05.000 --> 00:01:08.000\nOne minute in"));
    }

    #[test]
    fn srv3_rejects_malformed() {
        assert!(parse_srv3("<text start=\"0\" dur=\"2\">no close").is_err());
        assert!(parse_srv3("<text dur=\"2\">x</text>").is_err());
    }
}
