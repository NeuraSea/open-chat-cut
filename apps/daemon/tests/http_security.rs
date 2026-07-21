use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use chrono::Utc;
use http_body_util::BodyExt;
use openchatcut_daemon::{AppState, Config, build_app, runtime::RuntimeDescriptor};
use serde_json::Value;
use tempfile::TempDir;
use tower::ServiceExt;

async fn app() -> (axum::Router, TempDir) {
    let temp = tempfile::tempdir().unwrap();
    let config = Config::for_test(temp.path().to_owned());
    let runtime = RuntimeDescriptor {
        protocol_version: "1".to_owned(),
        instance_id: "test-instance".to_owned(),
        api_base_url: "http://127.0.0.1:3210/api/v1".to_owned(),
        token_path: config.token_path.clone(),
        pid: std::process::id(),
        started_at: Utc::now(),
    };
    let state = AppState::initialize(&config, runtime, "test-daemon-token".to_owned())
        .await
        .unwrap();
    (build_app(state), temp)
}

fn request(method: &str, uri: &str) -> axum::http::request::Builder {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::HOST, "127.0.0.1:3210")
}

#[tokio::test]
async fn health_is_public_but_status_is_not() {
    let (app, _temp) = app().await;
    let health = app
        .clone()
        .oneshot(request("GET", "/health").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(health.status(), StatusCode::OK);

    let status = app
        .oneshot(
            request("GET", "/api/v1/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(status.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn bearer_auth_works_and_external_host_is_rejected() {
    let (app, _temp) = app().await;
    let ok = app
        .clone()
        .oneshot(
            request("GET", "/api/v1/status")
                .header(header::AUTHORIZATION, "Bearer test-daemon-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(ok.status(), StatusCode::OK);

    let rejected = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/status")
                .header(header::HOST, "attacker.example:3210")
                .header(header::AUTHORIZATION, "Bearer test-daemon-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(rejected.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn browser_cookie_requires_csrf_for_post() {
    let (app, _temp) = app().await;
    let bootstrap = app
        .clone()
        .oneshot(
            request("POST", "/api/v1/session/bootstrap")
                .header(header::ORIGIN, "http://127.0.0.1:3100")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(bootstrap.status(), StatusCode::OK);
    let cookie = bootstrap
        .headers()
        .get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let body = bootstrap.into_body().collect().await.unwrap().to_bytes();
    let value: Value = serde_json::from_slice(&body).unwrap();
    let csrf = value["csrfToken"].as_str().unwrap();

    let missing = app
        .clone()
        .oneshot(
            request("POST", "/api/v1/tools/get_status")
                .header(header::ORIGIN, "http://127.0.0.1:3100")
                .header(header::COOKIE, &cookie)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{\"arguments\":{}}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::FORBIDDEN);

    let accepted = app
        .oneshot(
            request("POST", "/api/v1/tools/get_status")
                .header(header::ORIGIN, "http://127.0.0.1:3100")
                .header(header::COOKIE, cookie)
                .header(header::CONTENT_TYPE, "application/json")
                .header("x-openchatcut-csrf", csrf)
                .body(Body::from("{\"arguments\":{}}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(accepted.status(), StatusCode::OK);
    assert_eq!(
        accepted
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .unwrap(),
        "http://127.0.0.1:3100"
    );
}

#[tokio::test]
async fn generation_tool_rejects_an_incomplete_request_structurally() {
    let (app, _temp) = app().await;
    let response = app
        .oneshot(
            request("POST", "/api/v1/tools/generate_asset")
                .header(header::AUTHORIZATION, "Bearer test-daemon-token")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["error"]["code"], "missing_idempotency_key");
}

#[tokio::test]
async fn malformed_json_uses_the_structured_error_contract() {
    let (app, _temp) = app().await;
    let response = app
        .oneshot(
            request("POST", "/api/v1/tools/get_status")
                .header(header::AUTHORIZATION, "Bearer test-daemon-token")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["error"]["code"], "invalid_json");
}

#[tokio::test]
async fn transcription_is_not_advertised_without_a_worker() {
    let (app, _temp) = app().await;
    let status = app
        .clone()
        .oneshot(
            request("GET", "/api/v1/status")
                .header(header::AUTHORIZATION, "Bearer test-daemon-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = status.into_body().collect().await.unwrap().to_bytes();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["capabilities"]["transcription"], false);

    let response = app
        .oneshot(
            request("POST", "/api/v1/tools/start_transcription")
                .header(header::AUTHORIZATION, "Bearer test-daemon-token")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"arguments":{"projectId":"p","assetId":"a","expectedRevision":0},"idempotencyKey":"transcribe-1"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["error"]["code"], "capability_not_available");
}

#[tokio::test]
async fn generator_catalog_is_available_without_claiming_paid_providers_are_configured() {
    let (app, _temp) = app().await;
    let response = app
        .oneshot(
            request("POST", "/api/v1/tools/list_generators")
                .header(header::AUTHORIZATION, "Bearer test-daemon-token")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"arguments":{"kind":"video"}}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    let providers = value["data"]["providers"].as_array().unwrap();
    assert!(!providers.is_empty());
    assert!(providers.iter().all(|provider| {
        provider["availability"]["state"] == "needsConfiguration"
            || provider["availability"]["state"] == "unavailable"
    }));
    assert!(providers.iter().all(|provider| {
        provider["adapters"]
            .as_array()
            .unwrap()
            .iter()
            .all(|adapter| adapter["capability"] == "videoGeneration")
    }));
}

#[tokio::test]
async fn remote_import_requires_confirmation_and_blocks_loopback_ssrf() {
    let (app, _temp) = app().await;
    let created = app
        .clone()
        .oneshot(
            request("POST", "/api/v1/projects")
                .header(header::AUTHORIZATION, "Bearer test-daemon-token")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"name":"SSRF test","idempotencyKey":"create-ssrf-test"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = created.into_body().collect().await.unwrap().to_bytes();
    let created: Value = serde_json::from_slice(&bytes).unwrap();
    let project_id = created["envelope"]["document"]["id"].as_str().unwrap();
    let body = |confirm: bool| {
        serde_json::json!({
            "idempotencyKey": "remote-ssrf-test",
            "arguments": {
                "projectId": project_id,
                "expectedRevision": 0,
                "url": "http://127.0.0.1:3210/health",
                "confirm": confirm
            }
        })
        .to_string()
    };
    let unconfirmed = app
        .clone()
        .oneshot(
            request("POST", "/api/v1/tools/import_remote_media")
                .header(header::AUTHORIZATION, "Bearer test-daemon-token")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body(false)))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unconfirmed.status(), StatusCode::PRECONDITION_REQUIRED);

    let blocked = app
        .oneshot(
            request("POST", "/api/v1/tools/import_remote_media")
                .header(header::AUTHORIZATION, "Bearer test-daemon-token")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body(true)))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(blocked.status(), StatusCode::FORBIDDEN);
    let bytes = blocked.into_body().collect().await.unwrap().to_bytes();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["error"]["code"], "remote_url_blocked");
}
