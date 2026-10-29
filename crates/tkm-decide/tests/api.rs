#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use tkm_decide::api::{self, Answer, DecisionsRequest, DecisionsResponse};

fn request() -> DecisionsRequest {
    serde_json::from_str(include_str!("fixtures/api-request-doc.json")).unwrap()
}
fn response() -> DecisionsResponse {
    serde_json::from_str(include_str!("fixtures/api-response-doc.json")).unwrap()
}

#[test]
fn documented_response_deserializes_and_validates() {
    let response = response();
    assert_eq!(response.model, "typesafe/jev-1.13-20260917");
    assert!(api::validate(&request().questions, &response).is_ok());
}
#[test]
fn documented_request_serializes_with_documented_field_names() {
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/api-request-doc.json")).unwrap();
    assert_eq!(serde_json::to_value(request()).unwrap(), expected);
}
#[test]
fn invalid_missing_key_probability_choice_score_and_unknown_type_are_rejected() {
    for fixture in [
        include_str!("fixtures/api-invalid-missing-key.json"),
        include_str!("fixtures/api-invalid-probability.json"),
        include_str!("fixtures/api-invalid-choice.json"),
        include_str!("fixtures/api-invalid-score.json"),
        include_str!("fixtures/api-invalid-unknown-type.json"),
    ] {
        let response: DecisionsResponse = serde_json::from_str(fixture).unwrap();
        assert!(api::validate(&request().questions, &response).is_err());
    }
}
#[test]
fn unknown_answer_does_not_discard_other_answers() {
    let response: DecisionsResponse =
        serde_json::from_str(include_str!("fixtures/api-invalid-unknown-type.json")).unwrap();
    assert_eq!(response.answers["is_bug"], Answer::Unknown);
    assert!(matches!(response.answers["team"], Answer::Choice { .. }));
}
#[test]
fn nonfinite_and_out_of_range_probability_metadata_is_rejected() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.1, 1.3] {
        let mut response = response();
        response
            .answers
            .insert("is_bug".into(), Answer::Noul { noul: value });
        assert!(api::validate(&request().questions, &response).is_err());
        let mut response = self::response();
        if let Some(Answer::Choice { confidence, .. }) = response.answers.get_mut("team") {
            *confidence = Some(value);
        }
        assert!(api::validate(&request().questions, &response).is_err());
        let mut response = self::response();
        if let Some(Answer::Score { probabilities, .. }) = response.answers.get_mut("urgency") {
            *probabilities = Some(std::collections::BTreeMap::from([("0".into(), value)]));
        }
        assert!(api::validate(&request().questions, &response).is_err());
    }
}
#[test]
fn mismatched_type_and_empty_score_criteria_are_rejected() {
    let mut response = response();
    response.answers.insert(
        "is_bug".into(),
        Answer::Choice {
            choice: "yes".into(),
            confidence: None,
            probabilities: None,
        },
    );
    assert!(api::validate(&request().questions, &response).is_err());
    let mut request = request();
    if let Some(api::Question::Score { criteria, .. }) = request.questions.get_mut("urgency") {
        criteria.clear();
    }
    assert!(api::validate(&request.questions, &self::response()).is_err());
}
#[test]
fn cost_and_probability_metadata_are_optional() {
    let mut value = serde_json::to_value(response()).unwrap();
    value["usage"].as_object_mut().unwrap().remove("cost");
    value["answers"]["team"]
        .as_object_mut()
        .unwrap()
        .remove("confidence");
    value["answers"]["team"]
        .as_object_mut()
        .unwrap()
        .remove("probabilities");
    let response: DecisionsResponse = serde_json::from_value(value).unwrap();
    assert!(api::validate(&request().questions, &response).is_ok());
    assert!(response.usage.cost.is_none());
}
#[test]
fn session_id_truncation_preserves_unicode_boundaries() {
    let mut request = request();
    request.session_id = Some("界".repeat(300));
    let value = serde_json::to_value(request).unwrap();
    assert_eq!(value["session_id"].as_str().unwrap().chars().count(), 256);
}

#[test]
fn extra_unasked_answers_are_ignored_even_when_unknown_or_invalid() {
    for extra in [
        Answer::Unknown,
        Answer::Noul { noul: f64::NAN },
        Answer::Choice {
            choice: "unasked".into(),
            confidence: Some(2.0),
            probabilities: None,
        },
        Answer::Score {
            score: -1.0,
            confidence: Some(f64::INFINITY),
            legend: None,
            probabilities: None,
        },
    ] {
        let mut response = response();
        response.answers.insert("extra".into(), extra);
        assert!(api::validate(&request().questions, &response).is_ok());
    }
}
