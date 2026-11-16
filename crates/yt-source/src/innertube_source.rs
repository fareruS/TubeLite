//! InnerTubeSource — Phase 3 `playable()`.
//!
//! The network call is a plain `POST /youtubei/v1/player` via the shared
//! reqwest client; the response parsing is a pure function
//! (`parse_player_response`) tested offline against fixtures in
//! `tests/fixtures/player*.json` (synthetic shapes, `example.invalid`
//! URLs — never real signed URLs, never committed).
//!
//! Selection rules (documented, not guessed — they encode the spec):
//! - DASH pair: video-only + audio-only adaptive formats; progressive
//!   `formats` are kept as candidates only (used when no adaptive pair
//!   exists, e.g. low-quality fallback).
//! - Video: highest height at or below the requested height; at equal
//!   height prefer VP9 over AVC (better quality per bitrate at 720p+),
//!   then highest bitrate. AV1 is excluded (spec: AV1 off).
//! - Audio: itag 251 (Opus) preferred, else highest bitrate audio.
//! - Expiry: `streamingData.expiresInSeconds` (string or number) added
//!   to the request time; the earliest expiry among chosen URLs wins.
//!   No expiry recorded => `PlayableMedia::is_expired` treats it as
//!   expired, forcing a re-resolve.
//! - Cipher: a format with `signatureCipher` and no plain `url` cannot
//!   be played without the player-script transform. If the track we need
//!   (video or audio) has no plain URL, return `SourceError::Cipher` —
//!   the caller then tries the next client in the fallback chain, which
//!   historically returns plain URLs (ANDROID/IOS). The transform itself
//!   lives in the embedded JS runtime (not chosen yet); it is NOT
//!   implemented here.
//! - Playability: `LOGIN_REQUIRED` => Forbidden (needs sign-in, which
//!   this mode does not do); `UNPLAYABLE`/`ERROR` => NotFound with the
//!   reason; missing/unknown shape => UnexpectedShape.
//!
//! NOT implemented yet in this file: search / video / channel / playlist
//! parsing (separate Phase 3 work, same pattern: pure parse + fixture).

use async_trait::async_trait;
use serde_json::Value;
use yt_model::{
    Capabilities, ChannelId, ChannelPage, Cursor, Format, Page, PlayableMedia, PlaylistId,
    QualityPref, VideoCard, VideoDetails, VideoId,
};

use crate::clients::ClientRegistry;
use crate::error::{Result, SourceError};
use crate::innertube::{
    browse_request_body, endpoint_url, player_request_body, search_request_body, FailureMemory,
    FallbackChain,
};
use crate::innertube_parse::{
    parse_channel_response, parse_playlist_response, parse_search_response, parse_video_response,
};
use crate::traits::{SearchQuery, StreamSource};

/// Parse a `/player` JSON response into a `PlayableMedia`.
///
/// Pure — no I/O, no clock beyond the caller-supplied `now_unix`.
pub fn parse_player_response(
    json: &Value,
    video_id: &VideoId,
    pref: QualityPref,
    now_unix: i64,
) -> Result<PlayableMedia> {
    let status = json
        .get("playabilityStatus")
        .and_then(|ps| ps.get("status"))
        .and_then(|s| s.as_str())
        .unwrap_or("");
    match status {
        "OK" => {}
        "LOGIN_REQUIRED" => return Err(SourceError::Forbidden),
        "UNPLAYABLE" | "ERROR" => {
            let reason = json
                .get("playabilityStatus")
                .and_then(|ps| ps.get("reason"))
                .and_then(|r| r.as_str())
                .unwrap_or("unplayable");
            return Err(SourceError::NotFound(format!("{}: {reason}", video_id.0)));
        }
        other => {
            return Err(SourceError::UnexpectedShape(format!(
                "playabilityStatus.status = {other:?}"
            )))
        }
    }

    let sd = json
        .get("streamingData")
        .ok_or_else(|| SourceError::UnexpectedShape("missing streamingData".into()))?;

    let mut candidates: Vec<Format> = Vec::new();
    let mut saw_cipher_video = false;
    let mut saw_cipher_audio = false;

    let mut collect = |list: &Value| {
        if let Some(arr) = list.as_array() {
            for f in arr {
                let itag = f.get("itag").and_then(|i| i.as_u64()).unwrap_or(0) as u32;
                let mime = f
                    .get("mimeType")
                    .and_then(|m| m.as_str())
                    .unwrap_or("")
                    .to_string();
                let is_audio = mime.starts_with("audio/");
                if f.get("url").and_then(|u| u.as_str()).is_none() {
                    // Ciphered: no plain URL. Record it, do not invent one.
                    if f.get("signatureCipher").is_some() {
                        if is_audio {
                            saw_cipher_audio = true;
                        } else {
                            saw_cipher_video = true;
                        }
                    }
                    continue;
                }
                candidates.push(Format {
                    itag,
                    mime_type: mime,
                    height: f.get("height").and_then(|h| h.as_u64()).map(|h| h as u32),
                    bitrate: f.get("bitrate").and_then(|b| b.as_u64()),
                    url: f
                        .get("url")
                        .and_then(|u| u.as_str())
                        .unwrap_or("")
                        .to_string(),
                    expires_at_unix: None,
                });
            }
        }
    };
    // Adaptive (DASH) first, then progressive — candidates keep both.
    collect(sd.get("adaptiveFormats").unwrap_or(&Value::Null));
    collect(sd.get("formats").unwrap_or(&Value::Null));

    let expires_in: Option<i64> = sd.get("expiresInSeconds").and_then(|e| {
        e.as_str()
            .and_then(|s| s.parse().ok())
            .or_else(|| e.as_i64())
    });
    let expires_at_unix = expires_in.map(|s| now_unix + s);
    for c in &mut candidates {
        c.expires_at_unix = expires_at_unix;
    }

    let wanted_height = pref.height();
    let video = match wanted_height {
        Some(h) => choose_video(&candidates, h),
        None => None, // AudioOnly
    };
    let audio = choose_audio(&candidates);

    if video.is_none() && wanted_height.is_some() {
        if saw_cipher_video {
            return Err(SourceError::Cipher);
        }
        return Err(SourceError::UnexpectedShape(
            "no playable video format in streamingData".into(),
        ));
    }
    if audio.is_none() {
        if saw_cipher_audio {
            return Err(SourceError::Cipher);
        }
        return Err(SourceError::UnexpectedShape(
            "no playable audio format in streamingData".into(),
        ));
    }

    let earliest = [video.as_ref(), audio.as_ref()]
        .into_iter()
        .flatten()
        .filter_map(|f| f.expires_at_unix)
        .min();
    Ok(PlayableMedia {
        video_id: video_id.clone(),
        video,
        audio,
        candidates,
        expires_at_unix: earliest,
    })
}

fn is_vp9(mime: &str) -> bool {
    mime.contains("vp9")
}

fn is_av1(mime: &str) -> bool {
    mime.contains("av01")
}

/// Highest height at or below `wanted`; at equal height prefer VP9, then
/// highest bitrate. AV1 is excluded by spec. Video-only formats only
/// (height present); progressive formats carry a height too and are
/// eligible as a fallback when no adaptive pair exists.
pub(crate) fn choose_video(candidates: &[Format], wanted: u32) -> Option<Format> {
    let mut best: Option<&Format> = None;
    for f in candidates {
        let Some(h) = f.height else { continue };
        if h > wanted || is_av1(&f.mime_type) {
            continue;
        }
        let better = match best {
            None => true,
            Some(b) => {
                let bh = b.height.unwrap_or(0);
                h > bh
                    || (h == bh && is_vp9(&f.mime_type) && !is_vp9(&b.mime_type))
                    || (h == bh
                        && is_vp9(&f.mime_type) == is_vp9(&b.mime_type)
                        && f.bitrate.unwrap_or(0) > b.bitrate.unwrap_or(0))
            }
        };
        if better {
            best = Some(f);
        }
    }
    best.cloned()
}

/// itag 251 (Opus) preferred, else highest bitrate audio-only format.
pub(crate) fn choose_audio(candidates: &[Format]) -> Option<Format> {
    let mut best: Option<&Format> = None;
    for f in candidates {
        if f.height.is_some() {
            continue; // video-only or progressive (handled by the video choice)
        }
        let better = match best {
            None => true,
            Some(b) => {
                (f.itag == 251 && b.itag != 251)
                    || ((f.itag == 251) == (b.itag == 251)
                        && f.bitrate.unwrap_or(0) > b.bitrate.unwrap_or(0))
            }
        };
        if better {
            best = Some(f);
        }
    }
    best.cloned()
}

/// InnerTube source: POSTs to youtubei/v1 with the fallback chain from
/// `clients.json` and per-video failure memory. All `StreamSource`
/// methods are implemented; live calls are still unverified on this VM
/// (egress hangs on the POST — see KNOWN-ISSUES.md).
pub struct InnerTubeSource {
    http: reqwest::Client,
    registry: ClientRegistry,
    chain: FallbackChain,
    failures: std::sync::Mutex<FailureMemory>,
}

impl InnerTubeSource {
    pub fn new(registry: ClientRegistry) -> Result<Self> {
        let chain = FallbackChain::new(registry.player_fallback.clone());
        let http = reqwest::Client::builder()
            .user_agent("TubeLite/0.1.0")
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .map_err(|e| SourceError::Network(e.to_string()))?;
        Ok(Self {
            http,
            registry,
            chain,
            failures: std::sync::Mutex::new(FailureMemory::default()),
        })
    }
}

impl InnerTubeSource {
    /// POST to an InnerTube endpoint, walking the fallback chain on
    /// network errors and 403s. Returns the parsed JSON of the first
    /// client that answers. Parse errors are the caller's problem —
    /// a client that returns the wrong shape is not retried here.
    async fn post_json(&self, endpoint: &str, body: Value) -> Result<Value> {
        let mut client_name = self.chain.first().unwrap_or("WEB").to_string();
        loop {
            let resp = self
                .http
                .post(endpoint_url(endpoint))
                .header("Origin", "https://www.youtube.com")
                .json(&body)
                .send()
                .await;
            let resp = match resp {
                Ok(r) => r,
                Err(e) if e.is_timeout() || e.is_connect() => {
                    match self.chain.next_after(&client_name) {
                        Some(next) => {
                            client_name = next.to_string();
                            continue;
                        }
                        None => return Err(SourceError::Network(e.to_string())),
                    }
                }
                Err(e) => return Err(SourceError::Network(e.to_string())),
            };
            if resp.status() == reqwest::StatusCode::FORBIDDEN {
                match self.chain.next_after(&client_name) {
                    Some(next) => {
                        client_name = next.to_string();
                        continue;
                    }
                    None => return Err(SourceError::Forbidden),
                }
            }
            return resp
                .json()
                .await
                .map_err(|e| SourceError::Parse(e.to_string()));
        }
    }
}

#[async_trait]
#[allow(clippy::double_must_use)]
impl StreamSource for InnerTubeSource {
    async fn search(&self, q: &SearchQuery) -> Result<Page<VideoCard>> {
        let client = self
            .registry
            .get(self.chain.first().unwrap_or("WEB"))
            .ok_or_else(|| SourceError::UnexpectedShape("unknown client WEB".into()))?;
        let body = search_request_body(client, &q.text);
        let json = self.post_json("search", body).await?;
        parse_search_response(&json)
    }

    async fn video(&self, id: &VideoId) -> Result<VideoDetails> {
        let client = self
            .registry
            .get(self.chain.first().unwrap_or("WEB"))
            .ok_or_else(|| SourceError::UnexpectedShape("unknown client WEB".into()))?;
        let body = player_request_body(client, &id.0);
        let json = self.post_json("player", body).await?;
        parse_video_response(&json, id)
    }

    async fn channel(&self, id: &ChannelId) -> Result<ChannelPage> {
        let client = self
            .registry
            .get(self.chain.first().unwrap_or("WEB"))
            .ok_or_else(|| SourceError::UnexpectedShape("unknown client WEB".into()))?;
        let body = browse_request_body(client, Some(&id.0), None);
        let json = self.post_json("browse", body).await?;
        parse_channel_response(&json, id)
    }

    async fn playlist(&self, id: &PlaylistId, cursor: Option<Cursor>) -> Result<Page<VideoCard>> {
        let client = self
            .registry
            .get(self.chain.first().unwrap_or("WEB"))
            .ok_or_else(|| SourceError::UnexpectedShape("unknown client WEB".into()))?;
        // Playlists are browsed as "VL<playlist id>"; a continuation token
        // replaces the browse id on later pages.
        let browse_id = format!("VL{}", id.0);
        let body = match &cursor {
            Some(c) => browse_request_body(client, None, Some(&c.0)),
            None => browse_request_body(client, Some(&browse_id), None),
        };
        let json = self.post_json("browse", body).await?;
        parse_playlist_response(&json)
    }

    async fn playable(&self, id: &VideoId, pref: QualityPref) -> Result<PlayableMedia> {
        let now_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let mut client_name = self.chain.first().unwrap_or("WEB").to_string();
        loop {
            let client = self.registry.get(&client_name).ok_or_else(|| {
                SourceError::UnexpectedShape(format!("unknown client {client_name}"))
            })?;
            {
                let failures = self.failures.lock().unwrap();
                if !failures.may_try(&id.0, &client_name) {
                    // Skip to the next client in the chain.
                    match self.chain.next_after(&client_name) {
                        Some(next) => {
                            client_name = next.to_string();
                            continue;
                        }
                        None => return Err(SourceError::Forbidden),
                    }
                }
            }
            let body = player_request_body(client, &id.0);
            let resp = self
                .http
                .post(endpoint_url("player"))
                .header("Origin", "https://www.youtube.com")
                .json(&body)
                .send()
                .await;
            let resp = match resp {
                Ok(r) => r,
                Err(e) => {
                    self.failures
                        .lock()
                        .unwrap()
                        .record_failure(&id.0, &client_name);
                    if e.is_timeout() || e.is_connect() {
                        match self.chain.next_after(&client_name) {
                            Some(next) => {
                                client_name = next.to_string();
                                continue;
                            }
                            None => return Err(SourceError::Network(e.to_string())),
                        }
                    }
                    return Err(SourceError::Network(e.to_string()));
                }
            };
            if resp.status() == reqwest::StatusCode::FORBIDDEN {
                self.failures
                    .lock()
                    .unwrap()
                    .record_failure(&id.0, &client_name);
                match self.chain.next_after(&client_name) {
                    Some(next) => {
                        client_name = next.to_string();
                        continue;
                    }
                    None => return Err(SourceError::Forbidden),
                }
            }
            let json: Value = resp
                .json()
                .await
                .map_err(|e| SourceError::Parse(e.to_string()))?;
            match parse_player_response(&json, id, pref, now_unix) {
                Ok(media) => return Ok(media),
                Err(SourceError::Cipher) | Err(SourceError::Forbidden) => {
                    self.failures
                        .lock()
                        .unwrap()
                        .record_failure(&id.0, &client_name);
                    match self.chain.next_after(&client_name) {
                        Some(next) => {
                            client_name = next.to_string();
                            continue;
                        }
                        None => return Err(SourceError::Cipher),
                    }
                }
                Err(e) => return Err(e),
            }
        }
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            limitation_note: Some(
                "InnerTube is unofficial and can break without notice; client versions are placeholders until verified live.".into(),
            ),
            ..Capabilities::default()
        }
    }

    fn name(&self) -> &'static str {
        "innertube"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn fixture(name: &str) -> Value {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
    }

    fn vid() -> VideoId {
        VideoId("dQw4w9WgXcQ".into())
    }

    #[test]
    fn player_fixture_picks_dash_pair_at_720() {
        let m = parse_player_response(
            &fixture("player.json"),
            &vid(),
            QualityPref::Auto,
            1_700_000_000,
        )
        .unwrap();
        // Auto = 720p: VP9 247 preferred over AVC 136 at equal height.
        assert_eq!(m.video.as_ref().unwrap().itag, 247);
        assert_eq!(m.video.as_ref().unwrap().height, Some(720));
        // Audio: Opus 251 preferred.
        assert_eq!(m.audio.as_ref().unwrap().itag, 251);
        // Expiry: now + 21600.
        assert_eq!(m.expires_at_unix, Some(1_700_021_600));
        assert!(!m.is_expired(1_700_000_000));
        assert!(m.is_expired(1_700_021_600));
        // Candidates include the progressive fallbacks too.
        assert!(m.candidates.iter().any(|f| f.itag == 22));
    }

    #[test]
    fn player_fixture_1080_picks_vp9_248() {
        let m = parse_player_response(
            &fixture("player.json"),
            &vid(),
            QualityPref::Height(1080),
            1_700_000_000,
        )
        .unwrap();
        assert_eq!(m.video.as_ref().unwrap().itag, 248);
    }

    #[test]
    fn player_fixture_audio_only_has_no_video() {
        let m = parse_player_response(
            &fixture("player.json"),
            &vid(),
            QualityPref::AudioOnly,
            1_700_000_000,
        )
        .unwrap();
        assert!(m.video.is_none());
        assert_eq!(m.audio.as_ref().unwrap().itag, 251);
    }

    #[test]
    fn player_fixture_360_falls_back_to_243() {
        let m = parse_player_response(
            &fixture("player.json"),
            &vid(),
            QualityPref::Height(360),
            1_700_000_000,
        )
        .unwrap();
        assert_eq!(m.video.as_ref().unwrap().itag, 243);
    }

    #[test]
    fn login_required_maps_to_forbidden() {
        let err = parse_player_response(
            &fixture("player_unplayable.json"),
            &vid(),
            QualityPref::Auto,
            1_700_000_000,
        )
        .unwrap_err();
        assert!(matches!(err, SourceError::Forbidden));
    }

    #[test]
    fn ciphered_video_maps_to_cipher_error() {
        // Only video track is ciphered; audio is plain. The video we need
        // has no plain URL, so the whole resolve is a Cipher error and the
        // caller must try the next client.
        let err = parse_player_response(
            &fixture("player_cipher.json"),
            &vid(),
            QualityPref::Auto,
            1_700_000_000,
        )
        .unwrap_err();
        assert!(matches!(err, SourceError::Cipher));
    }

    #[test]
    fn missing_streaming_data_is_unexpected_shape() {
        let err = parse_player_response(
            &serde_json::json!({"playabilityStatus": {"status": "OK"}}),
            &vid(),
            QualityPref::Auto,
            1_700_000_000,
        )
        .unwrap_err();
        assert!(matches!(err, SourceError::UnexpectedShape(_)));
    }
}
