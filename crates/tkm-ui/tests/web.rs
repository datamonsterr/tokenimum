#![forbid(unsafe_code)]
#![cfg(feature = "web")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use std::sync::{
    Arc,
    atomic::{AtomicU64, AtomicUsize, Ordering},
};
use tkm_ui::{
    model::MonitorState,
    web::{self, Actions, WebConfig, WebState},
};
use tower::ServiceExt;

#[derive(Default)]
struct Fake(AtomicUsize);
impl Actions for Fake {
    fn forget_fact(&self, _: &str, _: &str) -> Result<String, String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok("forgot".into())
    }
    fn restore_tool(&self, _: &str, _: &str, _: &str) -> Result<String, String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok("restored".into())
    }
    fn switch_preset(&self, _: &str, _: &str) -> Result<String, String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok("switched".into())
    }
}
fn app(fake: Arc<Fake>, clock: Arc<AtomicU64>) -> axum::Router {
    let config = WebConfig {
        bind: "127.0.0.1:7878".parse().unwrap(),
        token: "secret".into(),
    };
    let mut state = WebState::new(
        config,
        MonitorState::default(),
        fake,
        std::path::PathBuf::new(),
    );
    state.set_clock(Arc::new(move || clock.load(Ordering::SeqCst)));
    web::router(state)
}
async fn request(
    app: axum::Router,
    method: &str,
    path: &str,
    host: &str,
    token: Option<&str>,
    body: &str,
) -> axum::response::Response {
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header("host", host)
        .header("content-type", "application/json");
    if let Some(token) = token {
        req = req.header("authorization", format!("Bearer {token}"));
    }
    app.oneshot(req.body(Body::from(body.to_owned())).unwrap())
        .await
        .unwrap()
}
#[tokio::test]
async fn auth_host_and_csp_apply_to_every_route() {
    let app = app(Arc::new(Fake::default()), Arc::new(AtomicU64::new(0)));
    for (method, path) in [
        ("GET", "/api/state"),
        ("GET", "/api/history"),
        ("GET", "/api/facts/r/f"),
        ("GET", "/api/prune"),
        ("POST", "/api/actions/forget_fact"),
        ("POST", "/api/actions/restore_tool"),
        ("POST", "/api/actions/switch_preset"),
        ("POST", "/api/confirm/id"),
        ("GET", "/api/unknown"),
    ] {
        for token in [None, Some("wrong")] {
            let res = request(app.clone(), method, path, "localhost:7878", token, "{}").await;
            assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "{path}");
            assert!(res.headers().contains_key("content-security-policy"));
        }
    }
    assert_eq!(
        request(app.clone(), "GET", "/", "evil:7878", None, "")
            .await
            .status(),
        StatusCode::MISDIRECTED_REQUEST
    );
    assert!(
        request(app, "GET", "/", "localhost:7878", None, "")
            .await
            .headers()
            .contains_key("content-security-policy")
    );
}
#[tokio::test]
async fn actions_require_one_unexpired_confirmation() {
    let fake = Arc::new(Fake::default());
    let clock = Arc::new(AtomicU64::new(10));
    let app = app(fake.clone(), clock.clone());
    let res = request(
        app.clone(),
        "POST",
        "/api/actions/forget_fact",
        "localhost:7878",
        Some("secret"),
        r#"{"repo":"repo","fact_id":"id"}"#,
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(fake.0.load(Ordering::SeqCst), 0);
    let body = to_bytes(res.into_body(), 4096).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let path = format!("/api/confirm/{}", json["confirm_id"].as_str().unwrap());
    assert_eq!(
        request(
            app.clone(),
            "POST",
            &path,
            "localhost:7878",
            Some("secret"),
            "{}"
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(fake.0.load(Ordering::SeqCst), 1);
    assert!(matches!(
        request(
            app.clone(),
            "POST",
            &path,
            "localhost:7878",
            Some("secret"),
            "{}"
        )
        .await
        .status(),
        StatusCode::NOT_FOUND | StatusCode::GONE
    ));
    let res = request(
        app.clone(),
        "POST",
        "/api/actions/forget_fact",
        "localhost:7878",
        Some("secret"),
        r#"{"repo":"repo","fact_id":"id"}"#,
    )
    .await;
    let body = to_bytes(res.into_body(), 4096).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    clock.store(70, Ordering::SeqCst);
    let path = format!("/api/confirm/{}", json["confirm_id"].as_str().unwrap());
    assert!(matches!(
        request(app, "POST", &path, "localhost:7878", Some("secret"), "{}")
            .await
            .status(),
        StatusCode::NOT_FOUND | StatusCode::GONE
    ));
    assert_eq!(fake.0.load(Ordering::SeqCst), 1);
}
#[test]
fn rejects_non_loopback_and_external_assets() {
    for bind in ["0.0.0.0:1", "192.168.1.2:1", "[::1]:1", "[::]:1"] {
        assert!(web::validate_bind(bind).is_err());
    }
    for asset in [
        include_str!("../assets/index.html"),
        include_str!("../assets/app.js"),
        include_str!("../assets/app.css"),
    ] {
        assert!(!asset.contains("http://") && !asset.contains("https://"));
        for bytes in asset.as_bytes().windows(3) {
            assert!(!(bytes[0] == b'/' && bytes[1] == b'/' && bytes[2].is_ascii_alphanumeric()));
        }
    }
}

#[tokio::test]
async fn each_action_is_inert_until_confirmed() {
    let fake = Arc::new(Fake::default());
    let app = app(fake.clone(), Arc::new(AtomicU64::new(0)));
    for (kind, body) in [
        (
            "restore_tool",
            r#"{"checkout":"/repo","host":"claude","tool":"tool"}"#,
        ),
        ("switch_preset", r#"{"repo":"repo","preset":"offline"}"#),
    ] {
        let before = fake.0.load(Ordering::SeqCst);
        let res = request(
            app.clone(),
            "POST",
            &format!("/api/actions/{kind}"),
            "localhost:7878",
            Some("secret"),
            body,
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(fake.0.load(Ordering::SeqCst), before);
        let data = to_bytes(res.into_body(), 4096).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&data).unwrap();
        let path = format!("/api/confirm/{}", value["confirm_id"].as_str().unwrap());
        assert_eq!(
            request(
                app.clone(),
                "POST",
                &path,
                "localhost:7878",
                Some("secret"),
                "{}"
            )
            .await
            .status(),
            StatusCode::OK
        );
        assert_eq!(fake.0.load(Ordering::SeqCst), before + 1);
    }
}
#[test]
fn token_has_expected_format_and_is_fresh() {
    let first = web::generate_token().unwrap();
    assert_eq!(first.len(), 64);
    assert!(
        first
            .bytes()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    );
    assert_ne!(first, web::generate_token().unwrap());
}

#[tokio::test]
async fn confirmations_and_action_input_are_bounded() {
    let app = app(Arc::new(Fake::default()), Arc::new(AtomicU64::new(0)));
    for _ in 0..32 {
        assert_eq!(
            request(
                app.clone(),
                "POST",
                "/api/actions/forget_fact",
                "localhost:7878",
                Some("secret"),
                r#"{"repo":"repo","fact_id":"id"}"#
            )
            .await
            .status(),
            StatusCode::OK
        );
    }
    assert_eq!(
        request(
            app.clone(),
            "POST",
            "/api/actions/forget_fact",
            "localhost:7878",
            Some("secret"),
            r#"{"repo":"repo","fact_id":"id"}"#
        )
        .await
        .status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    let oversized = serde_json::json!({"repo": "x".repeat(4097), "fact_id":"id"}).to_string();
    assert_eq!(
        request(
            app,
            "POST",
            "/api/actions/forget_fact",
            "localhost:7878",
            Some("secret"),
            &oversized
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn restore_rejects_unrecorded_checkout() {
    let app = app(Arc::new(Fake::default()), Arc::new(AtomicU64::new(0)));
    let res = request(
        app,
        "POST",
        "/api/actions/restore_tool",
        "localhost:7878",
        Some("secret"),
        r#"{"checkout":"/unknown","host":"claude","tool":"tool"}"#,
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}
