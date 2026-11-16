//! Contract tests: every StreamSource implementation must pass this suite
//! (against fixtures or a mock server). InnerTubeSource, ExtractorSource
//! and OfficialSource (Phases 3/6) must be wired into this file — do not
//! invent a second, weaker suite for them.

use yt_model::{ChannelId, PlaylistId, QualityPref, VideoId};
use yt_source::{
    traits::{SearchKind, SearchQuery},
    FixtureSource, StreamSource,
};

async fn check_contract<S: StreamSource>(src: &S) {
    // search returns at least one card with a non-empty title
    let page = src
        .search(&SearchQuery {
            text: "fixture".into(),
            kind: SearchKind::All,
        })
        .await
        .expect("search");
    assert!(!page.items.is_empty());
    assert!(!page.items[0].title.is_empty());

    // video round-trip for the fixture id
    let id = VideoId("dQw4w9WgXcQ".into());
    let details = src.video(&id).await.expect("video");
    assert_eq!(details.id, id);
    assert!(
        !details.chapters.is_empty(),
        "chapters must be present in fixture"
    );

    // unknown video id is NotFound, not an empty success
    assert!(src.video(&VideoId("no-such-video".into())).await.is_err());

    // channel
    let ch = src
        .channel(&ChannelId("UCuAXrQd1Upf2qQv8n2OQvQ".into()))
        .await
        .expect("channel");
    assert!(!ch.title.is_empty());

    // playlist
    let pl = src
        .playlist(&PlaylistId("PLfixture".into()), None)
        .await
        .expect("playlist");
    assert!(!pl.items.is_empty());

    // playable: DASH pair, audio present, video present at 720p, absent audio-only
    let playable = src
        .playable(&id, QualityPref::Height(720))
        .await
        .expect("playable");
    assert!(playable.audio.is_some());
    assert_eq!(playable.audio.as_ref().unwrap().itag, 251);
    assert!(playable.video.is_some());
    assert_eq!(playable.video.as_ref().unwrap().height, Some(720));
    assert!(!playable.is_expired(1_700_000_000));

    let audio_only = src
        .playable(&id, QualityPref::AudioOnly)
        .await
        .expect("audio-only");
    assert!(audio_only.video.is_none());
    assert!(audio_only.audio.is_some());

    // capabilities are honest
    let caps = src.capabilities();
    assert!(caps.search);
}

#[tokio::test]
async fn fixture_source_passes_contract() {
    check_contract(&FixtureSource).await;
}
