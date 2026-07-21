use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use chrono::Utc;
use http_body_util::BodyExt;
use openchatcut_daemon::{AppState, Config, build_app, runtime::RuntimeDescriptor};
use openchatcut_domain::{
    Asset, AssetId, AssetKind, AssetProvenance, LinkGroupId, ProjectDocument, ProjectId,
    Sha256Digest, StoryClip, StoryClipId, StorySequence, StorySequenceId, TranscriptDocument,
    TranscriptId, TranscriptWord, WordId,
};
use serde_json::{Value, json};
use tower::ServiceExt;

#[tokio::test]
async fn broll_search_prefers_managed_local_assets_and_resolves_story_word_anchor() {
    let temp = tempfile::tempdir().unwrap();
    let config = Config::for_test(temp.path().to_owned());
    let state = AppState::initialize(
        &config,
        RuntimeDescriptor {
            protocol_version: "1".to_owned(),
            instance_id: "broll-search-test".to_owned(),
            api_base_url: "http://127.0.0.1:3210/api/v1".to_owned(),
            token_path: config.token_path.clone(),
            pid: std::process::id(),
            started_at: Utc::now(),
        },
        "token".to_owned(),
    )
    .await
    .unwrap();
    let mut document =
        ProjectDocument::new(ProjectId::new("broll-project").unwrap(), "B-roll search");
    let mut mountain = Asset::new(
        AssetId::new("asset:mountain").unwrap(),
        "Mountain sunrise.mp4",
        AssetKind::Video,
    );
    mountain.content_hash = Some(Sha256Digest::new("a".repeat(64)).unwrap());
    mountain.provenance = AssetProvenance::Imported {
        source_name: Some("alpine-sunrise-camera-a.mp4".to_owned()),
    };
    let mut office = Asset::new(
        AssetId::new("asset:office").unwrap(),
        "Office team.png",
        AssetKind::Image,
    );
    office.content_hash = Some(Sha256Digest::new("b".repeat(64)).unwrap());
    office.provenance = AssetProvenance::Generated {
        provider: "codex-image".to_owned(),
        model: "gpt-image-2".to_owned(),
        prompt: "A collaborative office team".to_owned(),
        seed: None,
    };
    document.assets.extend([office, mountain]);
    let transcript_id = TranscriptId::new("transcript-1").unwrap();
    let word_id = WordId::new("word-sunrise").unwrap();
    let mut transcript = TranscriptDocument::new(transcript_id.clone(), "en");
    transcript.words.push(TranscriptWord {
        id: word_id.clone(),
        spoken_text: "sunrise".to_owned(),
        display_text: "sunrise".to_owned(),
        start_ticks: 1_000,
        end_ticks: 3_000,
        speaker_id: None,
        deleted: false,
        confidence: Some(0.99),
        extensions: Default::default(),
    });
    document.transcripts.push(transcript);
    document.story_sequences.push(StorySequence {
        id: StorySequenceId::new("story-1").unwrap(),
        transcript_id: transcript_id.clone(),
        clips: vec![StoryClip {
            id: StoryClipId::new("story-clip-1").unwrap(),
            word_ids: vec![word_id.clone()],
            timeline_start_ticks: 20_000,
            source_start_ticks: 500,
            source_end_ticks: 4_000,
            link_group_id: LinkGroupId::new("link-1").unwrap(),
            extensions: Default::default(),
        }],
        extensions: Default::default(),
    });
    state
        .database
        .create_project(
            document,
            "create-broll-project",
            &json!({ "fixture": true }),
        )
        .await
        .unwrap();

    let response = build_app(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/tools/search_broll")
                .header(header::HOST, "127.0.0.1:3210")
                .header(header::AUTHORIZATION, "Bearer token")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "arguments": {
                            "projectId": "broll-project",
                            "query": "mountain sunrise",
                            "transcriptId": transcript_id,
                            "wordId": word_id,
                            "edge": "start",
                            "bias": "after"
                        }
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(body["data"]["recommendation"], "useLocal");
    assert_eq!(body["data"]["localMatches"][0]["assetId"], "asset:mountain");
    assert_eq!(
        body["data"]["anchor"]["timelineAnchor"]["wordId"],
        "word-sunrise"
    );
    assert_eq!(
        body["data"]["anchor"]["timelineAnchor"]["fallbackTicks"],
        20_500
    );
    assert_eq!(body["data"]["anchor"]["resolvedTicks"], 20_500);
    assert_eq!(body["data"]["stockSearch"]["configured"], false);
    assert!(body["data"]["fallbackProviders"].as_array().unwrap().len() >= 3);
    state.native_jobs.shutdown().await;
}
