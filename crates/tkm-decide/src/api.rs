//! Typed `OpenRouter` Decisions wire format and answer validation.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Provider routing and data collection preferences.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderPrefs {
    /// Whether providers may collect request data (`deny` by default).
    pub data_collection: String,
    /// Whether `OpenRouter` may try another provider.
    pub allow_fallbacks: bool,
}
impl Default for ProviderPrefs {
    fn default() -> Self {
        Self {
            data_collection: "deny".into(),
            allow_fallbacks: true,
        }
    }
}
/// Guidance for the two outcomes of a noul question.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoulCriteria {
    /// Guidance for a true answer.
    #[serde(rename = "true")]
    pub when_true: String,
    /// Guidance for a false answer.
    #[serde(rename = "false")]
    pub when_false: String,
}
/// One typed question in a purpose's batch.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Question {
    /// Probability that a condition holds.
    Noul {
        /// Complete judgment instructions.
        instructions: String,
        /// Optional guidance for each boolean outcome.
        #[serde(skip_serializing_if = "Option::is_none")]
        criteria: Option<NoulCriteria>,
    },
    /// Select one named option.
    Choice {
        /// Complete judgment instructions.
        instructions: String,
        /// Option names and their guidance.
        criteria: BTreeMap<String, String>,
    },
    /// Expected index among ordered levels.
    Score {
        /// Complete judgment instructions.
        instructions: String,
        /// Ordered guidance, from index zero upwards.
        criteria: Vec<String>,
    },
}
/// A single Decisions API request.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DecisionsRequest {
    /// Requested, pinned model slug.
    pub model: String,
    /// Already built and redacted state (string, object, or array).
    pub state: serde_json::Value,
    /// All questions for one purpose.
    pub questions: BTreeMap<String, Question>,
    /// Optional provider preferences.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<ProviderPrefs>,
    /// Session identifier, serialized as at most 256 Unicode characters.
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_session_id"
    )]
    pub session_id: Option<String>,
    /// Optional end-user identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
}
fn serialize_session_id<S: serde::Serializer>(
    value: &Option<String>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    value
        .as_deref()
        .map(truncate_session_id)
        .serialize(serializer)
}
/// Truncate a session identifier at a Unicode character boundary.
#[must_use]
pub fn truncate_session_id(value: &str) -> String {
    value.chars().take(256).collect()
}
/// Usage returned by the Decisions API.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    /// Input tokens billed by the provider.
    pub input_tokens: u64,
    /// Output tokens returned by the provider.
    pub output_tokens: u64,
    /// Actual request cost in USD, when supplied by the API.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<f64>,
}
/// Typed answer; unsupported types deserialize as `Unknown`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Answer {
    /// Probability of the true outcome.
    Noul {
        /// Probability in the inclusive range zero to one.
        noul: f64,
    },
    /// Chosen option and optional probability metadata.
    Choice {
        /// Name of the chosen criteria key.
        choice: String,
        /// Confidence in the selected option.
        #[serde(skip_serializing_if = "Option::is_none")]
        confidence: Option<f64>,
        /// Probabilities by option name.
        #[serde(skip_serializing_if = "Option::is_none")]
        probabilities: Option<BTreeMap<String, f64>>,
    },
    /// Probability-weighted index among ordered criteria.
    Score {
        /// Expected index, from zero to the last criterion.
        score: f64,
        /// Confidence in the expected index.
        #[serde(skip_serializing_if = "Option::is_none")]
        confidence: Option<f64>,
        /// API-provided index descriptions, possibly structured.
        #[serde(skip_serializing_if = "Option::is_none")]
        legend: Option<BTreeMap<String, serde_json::Value>>,
        /// Probabilities by criterion index.
        #[serde(skip_serializing_if = "Option::is_none")]
        probabilities: Option<BTreeMap<String, f64>>,
    },
    /// An unsupported answer type, rejected by validation when asked.
    #[serde(other)]
    Unknown,
}
/// A Decisions API response.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DecisionsResponse {
    /// Optional API generation identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Actual model version that answered.
    pub model: String,
    /// Provider that served this request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Answers indexed by the question name.
    pub answers: BTreeMap<String, Answer>,
    /// Token counts and optional actual cost.
    pub usage: Usage,
}
/// An answer that cannot safely be consumed by the caller.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("invalid answer for {question}: {reason}")]
pub struct InvalidAnswer {
    /// Question whose answer failed validation.
    pub question: String,
    /// Safe, static reason, containing no request state.
    pub reason: &'static str,
}
/// Validate every requested answer against its question.
///
/// # Errors
/// Returns an error for missing, mismatched, or out-of-range answers.
pub fn validate(
    questions: &BTreeMap<String, Question>,
    response: &DecisionsResponse,
) -> Result<(), InvalidAnswer> {
    for (name, question) in questions {
        let invalid = |reason| InvalidAnswer {
            question: name.clone(),
            reason,
        };
        let answer = response
            .answers
            .get(name)
            .ok_or_else(|| invalid("missing answer"))?;
        match (question, answer) {
            (Question::Noul { .. }, Answer::Noul { .. }) => {}
            (Question::Choice { criteria, .. }, Answer::Choice { choice, .. }) => {
                if !criteria.contains_key(choice) {
                    return Err(invalid("choice is not a criteria key"));
                }
            }
            (Question::Score { criteria, .. }, Answer::Score { score, .. }) => {
                // Bound the conversion so it is exact, even on 64-bit platforms.
                let last = criteria
                    .len()
                    .checked_sub(1)
                    .and_then(|n| u32::try_from(n).ok());
                if !score.is_finite() || last.is_none_or(|n| !(0.0..=f64::from(n)).contains(score))
                {
                    return Err(invalid("score is outside the criteria range"));
                }
            }
            _ => return Err(invalid("answer type does not match question")),
        }
    }
    for (name, answer) in &response.answers {
        if !probabilities_valid(answer) {
            return Err(InvalidAnswer {
                question: name.clone(),
                reason: "unsupported answer or probability outside zero to one",
            });
        }
    }
    Ok(())
}
fn probability_valid(value: f64) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}
fn probabilities_valid(answer: &Answer) -> bool {
    match answer {
        Answer::Noul { noul } => probability_valid(*noul),
        Answer::Choice {
            confidence,
            probabilities,
            ..
        }
        | Answer::Score {
            confidence,
            probabilities,
            ..
        } => {
            confidence.is_none_or(probability_valid)
                && probabilities
                    .as_ref()
                    .is_none_or(|values| values.values().copied().all(probability_valid))
        }
        Answer::Unknown => false,
    }
}
