use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use chrono::Utc;
use http_body_util::BodyExt;
use openchatcut_daemon::{AppState, Config, build_app, runtime::RuntimeDescriptor};
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

async fn app() -> (axum::Router, TempDir) {
    let temp = tempfile::tempdir().unwrap();
    let config = Config::for_test(temp.path().to_owned());
    let runtime = RuntimeDescriptor {
        protocol_version: "1".to_owned(),
        instance_id: "proposal-security-test".to_owned(),
        api_base_url: "http://127.0.0.1:3210/api/v1".to_owned(),
        token_path: config.token_path.clone(),
        pid: std::process::id(),
        started_at: Utc::now(),
    };
    let state = AppState::initialize(&config, runtime, "test-token".to_owned())
        .await
        .unwrap();
    (build_app(state), temp)
}

async fn json_request(
    app: &axum::Router,
    method: &str,
    uri: &str,
    body: Value,
) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header(header::HOST, "127.0.0.1:3210")
                .header(header::AUTHORIZATION, "Bearer test-token")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap())
}

async fn create_project(app: &axum::Router) -> (String, Value) {
    let (status, body) = json_request(
        app,
        "POST",
        "/api/v1/projects",
        json!({ "name": "Proposal security", "idempotencyKey": "create-proposal-security" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    (
        body["envelope"]["document"]["id"]
            .as_str()
            .unwrap()
            .to_owned(),
        body["envelope"]["document"].clone(),
    )
}

#[tokio::test]
async fn apply_requires_confirmation_and_exact_server_side_proposal() {
    let (app, _temp) = app().await;
    let (project_id, _) = create_project(&app).await;
    let operations = json!([{ "type": "setProjectName", "name": "Reviewed" }]);
    let (status, validated) = json_request(
        &app,
        "POST",
        "/api/v1/tools/validate_timeline_edit",
        json!({
            "arguments": {
                "projectId": project_id,
                "expectedRevision": 0,
                "operations": operations,
            },
            "idempotencyKey": "validate-reviewed-name"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{validated}");
    let proposal_id = validated["proposal"]["proposalId"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(validated["proposal"]["payload"], operations);
    assert!(
        !validated["proposal"]["diffs"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    let (status, missing_confirmation) = json_request(
        &app,
        "POST",
        "/api/v1/tools/apply_timeline_edit",
        json!({
            "arguments": {
                "projectId": project_id,
                "expectedRevision": 0,
                "proposalId": proposal_id,
                "operations": operations,
            },
            "idempotencyKey": "apply-reviewed-name"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::PRECONDITION_REQUIRED);
    assert_eq!(
        missing_confirmation["error"]["code"],
        "confirmation_required"
    );

    let (status, payload_swap) = json_request(
        &app,
        "POST",
        "/api/v1/tools/apply_timeline_edit",
        json!({
            "arguments": {
                "projectId": project_id,
                "expectedRevision": 0,
                "proposalId": proposal_id,
                "confirm": true,
                "operations": [{ "type": "setProjectName", "name": "Injected" }],
            },
            "idempotencyKey": "apply-payload-swap"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{payload_swap}");
    assert_eq!(payload_swap["error"]["code"], "proposal_payload_mismatch");

    let (status, applied) = json_request(
        &app,
        "POST",
        "/api/v1/tools/apply_timeline_edit",
        json!({
            "arguments": {
                "projectId": project_id,
                "expectedRevision": 0,
                "proposalId": proposal_id,
                "confirm": true,
            },
            "idempotencyKey": "apply-reviewed-name"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{applied}");
    assert_eq!(applied["revision"], 1);
    assert_eq!(applied["data"]["envelope"]["document"]["name"], "Reviewed");
}

#[tokio::test]
async fn tools_reject_actor_spoofing_and_whole_document_operations() {
    let (app, _temp) = app().await;
    let (project_id, document) = create_project(&app).await;
    let (status, spoofed) = json_request(
        &app,
        "POST",
        "/api/v1/tools/validate_timeline_edit",
        json!({
            "arguments": {
                "projectId": project_id,
                "expectedRevision": 0,
                "transaction": {
                    "transactionId": "spoofed-actor",
                    "projectId": project_id,
                    "baseRevision": 0,
                    "idempotencyKey": "spoofed-actor",
                    "actor": { "kind": "user", "id": "admin" },
                    "operations": [{ "type": "setProjectName", "name": "Spoofed" }]
                }
            },
            "idempotencyKey": "spoofed-actor"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{spoofed}");
    assert_eq!(spoofed["error"]["code"], "agent_actor_spoofing");

    for (key, operation) in [
        (
            "replace-document",
            json!({ "type": "replaceDocument", "document": document }),
        ),
        (
            "replace-scene-graph",
            json!({ "type": "replaceSceneGraph", "scenes": [], "currentSceneId": null }),
        ),
    ] {
        let (status, rejected) = json_request(
            &app,
            "POST",
            "/api/v1/tools/validate_timeline_edit",
            json!({
                "arguments": {
                    "projectId": project_id,
                    "expectedRevision": 0,
                    "operations": [operation],
                },
                "idempotencyKey": key
            }),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{rejected}");
        assert_eq!(rejected["error"]["code"], "privileged_operation_forbidden");
    }
}
