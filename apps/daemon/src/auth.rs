use std::{collections::HashMap, sync::Arc, time::Duration};

use axum::{
    body::Body,
    extract::{Request, State},
    http::{
        HeaderMap, HeaderValue, Method, StatusCode,
        header::{
            ACCESS_CONTROL_ALLOW_CREDENTIALS, ACCESS_CONTROL_ALLOW_HEADERS,
            ACCESS_CONTROL_ALLOW_METHODS, ACCESS_CONTROL_ALLOW_ORIGIN, CACHE_CONTROL, COOKIE, HOST,
            ORIGIN, VARY,
        },
    },
    middleware::Next,
    response::{IntoResponse, Response},
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use subtle::ConstantTimeEq;
use tokio::sync::RwLock;

use crate::error::ApiError;

const SESSION_COOKIE: &str = "openchatcut_session";
pub const CSRF_HEADER: &str = "x-openchatcut-csrf";

#[derive(Debug, Clone)]
pub struct AuthState {
    daemon_token: Arc<str>,
    allowed_origins: Arc<std::collections::HashSet<String>>,
    sessions: Arc<RwLock<HashMap<String, BrowserSession>>>,
    session_ttl: Duration,
    secure_cookie: bool,
}

#[derive(Debug, Clone)]
struct BrowserSession {
    csrf_token: String,
    expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub enum RequestIdentity {
    DaemonToken,
    Browser,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserBootstrap {
    pub csrf_token: String,
    pub expires_at: DateTime<Utc>,
}

pub struct IssuedBrowserSession {
    pub bootstrap: BrowserBootstrap,
    pub set_cookie: HeaderValue,
}

impl AuthState {
    pub fn new(
        daemon_token: String,
        allowed_origins: std::collections::HashSet<String>,
        session_ttl: Duration,
        secure_cookie: bool,
    ) -> Self {
        Self {
            daemon_token: daemon_token.into(),
            allowed_origins: Arc::new(allowed_origins),
            sessions: Arc::new(RwLock::new(HashMap::new())),
            session_ttl,
            secure_cookie,
        }
    }

    pub async fn issue_browser_session(&self) -> IssuedBrowserSession {
        let session_token = random_secret();
        let csrf_token = random_secret();
        let expires_at = Utc::now()
            + chrono::Duration::from_std(self.session_ttl)
                .unwrap_or_else(|_| chrono::Duration::minutes(15));
        let now = Utc::now();
        let mut sessions = self.sessions.write().await;
        sessions.retain(|_, session| session.expires_at > now);
        if sessions.len() >= 1_024
            && let Some(oldest) = sessions
                .iter()
                .min_by_key(|(_, session)| session.expires_at)
                .map(|(token, _)| token.clone())
        {
            sessions.remove(&oldest);
        }
        sessions.insert(
            session_token.clone(),
            BrowserSession {
                csrf_token: csrf_token.clone(),
                expires_at,
            },
        );

        let secure = if self.secure_cookie { "; Secure" } else { "" };
        let cookie = format!(
            "{SESSION_COOKIE}={session_token}; HttpOnly; SameSite=Strict; Path=/api/v1; Max-Age={}{}",
            self.session_ttl.as_secs(),
            secure
        );
        IssuedBrowserSession {
            bootstrap: BrowserBootstrap {
                csrf_token,
                expires_at,
            },
            set_cookie: HeaderValue::from_str(&cookie).expect("random cookie is a valid header"),
        }
    }

    fn origin_allowed(&self, origin: &str) -> bool {
        self.allowed_origins.contains(origin)
    }

    async fn authenticate(&self, headers: &HeaderMap) -> Option<(RequestIdentity, Option<String>)> {
        if let Some(provided) = headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split_once(' '))
            .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("bearer"))
            .map(|(_, credentials)| credentials)
            && constant_time_equal(provided, &self.daemon_token)
        {
            return Some((RequestIdentity::DaemonToken, None));
        }

        let token = cookie_value(headers, SESSION_COOKIE)?;
        let now = Utc::now();
        let session = self.sessions.read().await.get(token).cloned();
        match session {
            Some(session) if session.expires_at > now => {
                Some((RequestIdentity::Browser, Some(session.csrf_token)))
            }
            Some(_) => {
                self.sessions.write().await.remove(token);
                None
            }
            None => None,
        }
    }
}

pub async fn security_middleware(
    State(auth): State<AuthState>,
    mut request: Request,
    next: Next,
) -> Response {
    if !valid_loopback_host(request.headers()) {
        return ApiError::new(
            StatusCode::FORBIDDEN,
            "invalid_host",
            "the HTTP Host must resolve to the loopback interface",
        )
        .into_response();
    }

    let origin = match request
        .headers()
        .get(ORIGIN)
        .and_then(|value| value.to_str().ok())
    {
        Some(origin) if auth.origin_allowed(origin) => Some(origin.to_owned()),
        Some(_) => {
            return ApiError::new(
                StatusCode::FORBIDDEN,
                "origin_not_allowed",
                "browser origin is not allowed",
            )
            .into_response();
        }
        None => None,
    };

    if request.method() == Method::OPTIONS {
        let Some(origin) = origin else {
            return ApiError::new(
                StatusCode::FORBIDDEN,
                "origin_required",
                "CORS preflight requires an allowed Origin",
            )
            .into_response();
        };
        return add_cors(Response::new(Body::empty()), &origin);
    }

    let path = request.uri().path();
    let public_health = path == "/health";
    let browser_bootstrap = path == "/api/v1/session/bootstrap";
    if browser_bootstrap && origin.is_none() {
        return ApiError::new(
            StatusCode::FORBIDDEN,
            "origin_required",
            "browser session bootstrap requires an allowed Origin",
        )
        .into_response();
    }

    if !public_health && !browser_bootstrap {
        let Some((identity, csrf)) = auth.authenticate(request.headers()).await else {
            return cors_error(
                ApiError::new(
                    StatusCode::UNAUTHORIZED,
                    "authentication_required",
                    "provide the daemon bearer token or a valid browser session",
                ),
                origin.as_deref(),
            );
        };

        let is_write = !matches!(*request.method(), Method::GET | Method::HEAD);
        if is_write && matches!(identity, RequestIdentity::Browser) {
            if origin.is_none() {
                return cors_error(
                    ApiError::new(
                        StatusCode::FORBIDDEN,
                        "origin_required",
                        "browser writes require an allowed Origin",
                    ),
                    origin.as_deref(),
                );
            }
            let provided = request
                .headers()
                .get(CSRF_HEADER)
                .and_then(|value| value.to_str().ok());
            if !provided.is_some_and(|value| {
                csrf.as_deref()
                    .is_some_and(|expected| constant_time_equal(value, expected))
            }) {
                return cors_error(
                    ApiError::new(
                        StatusCode::FORBIDDEN,
                        "csrf_failed",
                        "browser writes require a valid X-OpenChatCut-CSRF header",
                    ),
                    origin.as_deref(),
                );
            }
        }
        request.extensions_mut().insert(identity);
    }

    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        axum::http::header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    if let Some(origin) = origin {
        response = add_cors(response, &origin);
    }
    response
}

fn cors_error(error: ApiError, origin: Option<&str>) -> Response {
    let mut response = error.into_response();
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        axum::http::header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    match origin {
        Some(origin) => add_cors(response, origin),
        None => response,
    }
}

fn add_cors(mut response: Response, origin: &str) -> Response {
    let headers = response.headers_mut();
    if let Ok(value) = HeaderValue::from_str(origin) {
        headers.insert(ACCESS_CONTROL_ALLOW_ORIGIN, value);
    }
    headers.insert(
        ACCESS_CONTROL_ALLOW_CREDENTIALS,
        HeaderValue::from_static("true"),
    );
    headers.insert(
        ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET, POST, DELETE, OPTIONS"),
    );
    headers.insert(
        ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static(
            "authorization, content-type, idempotency-key, x-openchatcut-csrf, x-openchatcut-expected-revision, x-openchatcut-protocol-version",
        ),
    );
    headers.insert(VARY, HeaderValue::from_static("Origin"));
    response
}

fn valid_loopback_host(headers: &HeaderMap) -> bool {
    let Some(host) = headers.get(HOST).and_then(|value| value.to_str().ok()) else {
        return false;
    };
    let Ok(url) = url::Url::parse(&format!("http://{host}")) else {
        return false;
    };
    let Some(hostname) = url.host_str() else {
        return false;
    };
    let ip_hostname = hostname
        .strip_prefix('[')
        .and_then(|hostname| hostname.strip_suffix(']'))
        .unwrap_or(hostname);
    hostname.eq_ignore_ascii_case("localhost")
        || ip_hostname
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

fn cookie_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|part| part.trim().split_once('='))
        .find_map(|(candidate, value)| (candidate == name).then_some(value))
}

fn random_secret() -> String {
    hex::encode(rand::random::<[u8; 32]>())
}

fn constant_time_equal(left: &str, right: &str) -> bool {
    bool::from(left.as_bytes().ct_eq(right.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_cookie_without_prefix_confusion() {
        let mut headers = HeaderMap::new();
        headers.insert(
            COOKIE,
            "other=1; openchatcut_session=secret".parse().unwrap(),
        );
        assert_eq!(cookie_value(&headers, SESSION_COOKIE), Some("secret"));
    }

    #[test]
    fn host_must_be_loopback() {
        for valid in ["localhost:3210", "127.0.0.1:3210", "[::1]:3210"] {
            let mut headers = HeaderMap::new();
            headers.insert(HOST, valid.parse().unwrap());
            assert!(valid_loopback_host(&headers), "{valid}");
        }
        let mut headers = HeaderMap::new();
        headers.insert(HOST, "example.com:3210".parse().unwrap());
        assert!(!valid_loopback_host(&headers));
    }

    #[tokio::test]
    async fn hosted_sessions_use_secure_cookies() {
        let hosted = AuthState::new(
            "token".to_owned(),
            std::collections::HashSet::new(),
            Duration::from_secs(60),
            true,
        )
        .issue_browser_session()
        .await;
        let cookie = hosted.set_cookie.to_str().unwrap();
        assert!(cookie.contains("; Secure"));
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("SameSite=Strict"));
    }
}
