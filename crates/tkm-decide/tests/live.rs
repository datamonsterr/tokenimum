#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Opt-in probes only: default CI never contacts the network.
use std::collections::BTreeMap;
use tkm_decide::{
    api::{self, DecisionsRequest, DecisionsResponse, ProviderPrefs, Question},
    client::{HttpTransport, Transport},
    key::load_key,
};
async fn probe(model: &str) {
    if std::env::var("TKM_LIVE").ok().as_deref() != Some("1") {
        return;
    }
    let Ok(value) = std::env::var("OPENROUTER_API_KEY") else {
        return;
    };
    if value.trim().is_empty() {
        return;
    }
    let key = load_key(Some(value), std::path::Path::new("unused"), None).unwrap();
    let request = DecisionsRequest {
        model: model.into(),
        state: serde_json::json!("Synthetic probe: a green circle."),
        questions: BTreeMap::from([(
            "green".into(),
            Question::Noul {
                instructions: "Is the described circle green?".into(),
                criteria: None,
            },
        )]),
        provider: Some(ProviderPrefs::default()),
        session_id: Some("tkm-live-probe".into()),
        user: None,
    };
    let transport = HttpTransport::new().unwrap();
    let reply = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        transport.post_decision(&key, serde_json::to_string(&request).unwrap()),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(
        reply.status, 200,
        "live model {model} returned status {}",
        reply.status
    );
    let response: DecisionsResponse = serde_json::from_str(&reply.body).unwrap();
    api::validate(&request.questions, &response).unwrap();
}
#[tokio::test]
#[ignore = "live: TKM_LIVE=1"]
async fn live_mercury_free() {
    probe("inception/mercury-decide:free").await;
}
#[tokio::test]
#[ignore = "live: TKM_LIVE=1"]
async fn live_mercury_dated() {
    probe("inception/mercury-decide-20260930").await;
}
#[tokio::test]
#[ignore = "live: TKM_LIVE=1"]
async fn live_pplx_dated() {
    probe("perplexity/pplx-decider-flash-v1.2-20261016").await;
}
#[tokio::test]
#[ignore = "live: TKM_LIVE=1"]
async fn live_jev_dated() {
    probe("typesafe/jev-1.14-20261014").await;
}
