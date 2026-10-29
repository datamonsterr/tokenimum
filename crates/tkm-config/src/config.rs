//! Layered configuration and file editing.
use std::{collections::BTreeMap, path::PathBuf};
/// A configuration file or value could not be read or validated.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// File access failed.
    #[error("{path}: {source}", path = .path.display())]
    Io {
        /// File involved in the operation.
        path: PathBuf,
        /// Underlying I/O failure.
        #[source]
        source: std::io::Error,
    },
    /// TOML syntax was invalid.
    #[error("{path}: {source}", path = .path.display())]
    Parse {
        /// File containing invalid TOML.
        path: PathBuf,
        /// Underlying parse failure.
        #[source]
        source: toml_edit::TomlError,
    },
    /// The requested key is not registered.
    #[error("unknown key: {0}")]
    UnknownKey(String),
    /// A registered key has an invalid value.
    #[error("invalid value for {key}: {value}: {reason}")]
    InvalidValue {
        /// Registered key.
        key: String,
        /// Supplied value.
        value: String,
        /// Why validation failed.
        reason: String,
    },
    /// An untrusted repository tried to set a restricted value.
    #[error("repository cannot set {key}")]
    RestrictedInRepo {
        /// Restricted key.
        key: String,
    },
}
/// Configuration source, ordered from lowest to highest priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Layer {
    /// Built-in default.
    Default,
    /// Selected preset.
    Preset,
    /// Global configuration file.
    Global,
    /// Repository configuration file.
    Repo,
    /// Repository-local configuration file.
    RepoLocal,
    /// Environment variable.
    Env,
    /// Invocation flag.
    Flags,
}
/// One typed configuration value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// Boolean.
    Bool(bool),
    /// Integer.
    Int(i64),
    /// Floating-point number.
    Float(f64),
    /// Text.
    String(String),
    /// List of text values.
    List(Vec<String>),
    /// Unset optional value.
    Unset,
}
/// Entry ignored because a repository source is untrusted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ignored {
    /// Origin layer.
    pub layer: Layer,
    /// Dotted key.
    pub key: String,
    /// Reason for rejection.
    pub reason: String,
}
/// Explicit inputs to the configuration loader.
#[derive(Debug, Clone, Default)]
pub struct LoadInputs {
    /// Global configuration path.
    pub global_path: PathBuf,
    /// Optional repository root.
    pub repo_root: Option<PathBuf>,
    /// Explicit environment variables.
    pub env: Vec<(String, String)>,
    /// Dotted `key=value` flags.
    pub flags: Vec<String>,
}
impl LoadInputs {
    /// Capture process environment with the supplied global path and repository.
    #[must_use]
    pub fn from_env(global_path: PathBuf, repo_root: Option<PathBuf>, flags: Vec<String>) -> Self {
        Self {
            global_path,
            repo_root,
            flags,
            env: std::env::vars().collect(),
        }
    }
}
/// Resolved values and diagnostics.
#[derive(Debug, Default)]
pub struct EffectiveConfig {
    values: BTreeMap<String, (Value, Layer)>,
    /// Ignored untrusted entries.
    pub ignored: Vec<Ignored>,
    /// Non-fatal validation errors.
    pub errors: Vec<ConfigError>,
}
impl EffectiveConfig {
    /// Look up a typed value.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.values.get(key).map(|v| &v.0)
    }
    /// Look up a boolean.
    #[must_use]
    pub fn get_bool(&self, key: &str) -> Option<bool> {
        match self.get(key)? {
            Value::Bool(v) => Some(*v),
            _ => None,
        }
    }
    /// Look up an integer.
    #[must_use]
    pub fn get_int(&self, key: &str) -> Option<i64> {
        match self.get(key)? {
            Value::Int(v) => Some(*v),
            _ => None,
        }
    }
    /// Look up a floating-point value.
    #[must_use]
    pub fn get_float(&self, key: &str) -> Option<f64> {
        match self.get(key)? {
            Value::Float(v) => Some(*v),
            _ => None,
        }
    }
    /// Look up text.
    #[must_use]
    pub fn get_str(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            Value::String(v) => Some(v),
            _ => None,
        }
    }
    /// Look up a list.
    #[must_use]
    pub fn get_list(&self, key: &str) -> Option<&[String]> {
        match self.get(key)? {
            Value::List(v) => Some(v),
            _ => None,
        }
    }
    /// Identify the winning source.
    #[must_use]
    pub fn source(&self, key: &str) -> Option<Layer> {
        self.values.get(key).map(|v| v.1)
    }
}
#[derive(Clone, Copy)]
enum Kind {
    Bool,
    Int,
    Float(f64, f64),
    String,
    Enum(&'static [&'static str]),
    List,
}
struct Key {
    name: &'static str,
    kind: Kind,
    default: &'static str,
    description: &'static str,
    restricted: bool,
}
macro_rules! key {
    ($name:literal, $kind:expr, $default:literal, $description:literal) => {
        Key {
            name: $name,
            kind: $kind,
            default: $default,
            description: $description,
            restricted: false,
        }
    };
    ($name:literal, $kind:expr, $default:literal, $description:literal, restricted) => {
        Key {
            name: $name,
            kind: $kind,
            default: $default,
            description: $description,
            restricted: true,
        }
    };
}
fn registry() -> Vec<Key> {
    let mut keys = vec![
        key!(
            "preset",
            Kind::Enum(&["observe", "balanced", "aggressive", "offline"]),
            "balanced",
            "Select the configuration preset."
        ),
        key!(
            "facts.enabled",
            Kind::Bool,
            "true",
            "Enable repository fact learning."
        ),
        key!(
            "facts.evidence",
            Kind::Enum(&["code", "decide"]),
            "decide",
            "Choose the fact evidence mechanism."
        ),
        key!(
            "facts.promote_confirmations",
            Kind::Int,
            "3",
            "Sessions confirming a fact before promotion."
        ),
        key!(
            "facts.recency_decay",
            Kind::Float(0.0, 1.0),
            "0.8",
            "Per-session fact score decay."
        ),
        key!(
            "facts.contradiction_weight",
            Kind::Float(0.0, f64::MAX),
            "5.0",
            "Weight of a contradiction."
        ),
        key!(
            "facts.fire_gate",
            Kind::Float(0.0, 1.0),
            "0.40",
            "Minimum exercised-session failure fraction."
        ),
        key!(
            "facts.fire_min_sessions",
            Kind::Int,
            "4",
            "Minimum sessions for the fact fire gate."
        ),
        key!("facts.max_facts", Kind::Int, "4", "Maximum facts injected."),
        key!(
            "facts.max_tokens",
            Kind::Int,
            "150",
            "Maximum tokens in the fact block."
        ),
        key!(
            "prune.evidence_prompts",
            Kind::Int,
            "10",
            "Unused prompts before pruning evidence."
        ),
        key!(
            "prune.min_sessions",
            Kind::Int,
            "2",
            "Minimum sessions for pruning evidence."
        ),
        key!(
            "prune.sessions_any",
            Kind::Int,
            "5",
            "Alternative session count for pruning."
        ),
        key!(
            "prune.quiet_minutes",
            Kind::Int,
            "60",
            "Quiet period before writing a deny list."
        ),
        key!(
            "prune.mode",
            Kind::Enum(&["off", "report", "propose", "apply"]),
            "propose",
            "Pruning action mode.",
            restricted
        ),
        key!(
            "loop.min_turns",
            Kind::Int,
            "20",
            "Minimum calls before judging a turn."
        ),
        key!(
            "loop.gate_min_turns",
            Kind::Int,
            "18",
            "Minimum calls before the gate opens."
        ),
        key!(
            "loop.gate_error_outputs",
            Kind::Int,
            "8",
            "Error outputs that open the gate."
        ),
        key!(
            "loop.edit_churn",
            Kind::Int,
            "4",
            "Edits per file counted as churn."
        ),
        key!(
            "loop.stagnation",
            Kind::Float(0.0, 1.0),
            "0.20",
            "Repeated failure fraction."
        ),
        key!(
            "loop.envelope_pct",
            Kind::Int,
            "90",
            "Percentile of good turn lengths."
        ),
        key!(
            "loop.envelope_margin",
            Kind::Float(0.0, f64::MAX),
            "1.25",
            "Good turn envelope multiplier."
        ),
        key!(
            "loop.envelope_min_samples",
            Kind::Int,
            "5",
            "Good turns needed for an envelope."
        ),
        key!(
            "loop.backstop_mult",
            Kind::Float(0.0, f64::MAX),
            "2.5",
            "Rule backstop envelope multiplier."
        ),
        key!(
            "loop.redirects_per_turn",
            Kind::Int,
            "1",
            "Redirect budget per turn."
        ),
        key!(
            "loop.level",
            Kind::Enum(&["conservative", "default", "aggressive"]),
            "default",
            "Loop guard sensitivity."
        ),
        key!(
            "loop.mode",
            Kind::Enum(&["off", "shadow", "redirect"]),
            "redirect",
            "Loop guard action mode."
        ),
        key!(
            "loop.escalate_command",
            Kind::String,
            "",
            "Optional escalation command.",
            restricted
        ),
        key!(
            "inject.enabled",
            Kind::Bool,
            "true",
            "Enable context injection."
        ),
        key!(
            "inject.max_tokens",
            Kind::Int,
            "600",
            "Maximum injection tokens."
        ),
        key!(
            "inject.memory",
            Kind::Bool,
            "true",
            "Include memory pointers."
        ),
        key!(
            "inject.instructions_file",
            Kind::Bool,
            "false",
            "Include instruction files."
        ),
        key!(
            "inject.instructions_file_tracked",
            Kind::Bool,
            "false",
            "Allow tracked instruction files."
        ),
        key!("ledger.enabled", Kind::Bool, "true", "Enable usage ledger."),
        key!(
            "decide.enabled",
            Kind::Bool,
            "true",
            "Enable decision requests."
        ),
        key!(
            "decide.chain",
            Kind::List,
            "[\"inception/mercury-decide-20260930\",\"perplexity/pplx-decider-flash-v1.2\",\"typesafe/jev-1.14\"]",
            "Decision model fallback chain."
        ),
        key!(
            "decide.agreement",
            Kind::Bool,
            "false",
            "Require agreement between models."
        ),
        key!(
            "decide.shadow",
            Kind::Bool,
            "false",
            "Record decisions without acting."
        ),
        key!(
            "decide.budget_daily_usd",
            Kind::Float(0.0, f64::MAX),
            "0.50",
            "Daily decision budget in dollars.",
            restricted
        ),
        key!(
            "decide.budget_session_usd",
            Kind::Float(0.0, f64::MAX),
            "0.02",
            "Session decision budget in dollars.",
            restricted
        ),
        key!(
            "decide.api_key_path",
            Kind::String,
            "~/.config/tokenimum/openrouter.env",
            "Path to the decision API key.",
            restricted
        ),
        key!(
            "calibration.t_stuck",
            Kind::Float(0.0, 1.0),
            "0.85",
            "Stuck probability threshold."
        ),
        key!(
            "calibration.t_progress",
            Kind::Float(0.0, 1.0),
            "0.30",
            "Progress probability threshold."
        ),
        key!(
            "calibration.t_low",
            Kind::Float(0.0, 1.0),
            "0.30",
            "Low stuck threshold."
        ),
        key!(
            "calibration.t_high",
            Kind::Float(0.0, 1.0),
            "0.80",
            "High stuck threshold."
        ),
        key!(
            "calibration.shadow_days",
            Kind::Int,
            "14",
            "Minimum shadow period in days."
        ),
        key!(
            "calibration.shadow_turns",
            Kind::Int,
            "200",
            "Minimum judged turns in shadow."
        ),
        key!(
            "calibration.min_precision",
            Kind::Float(0.0, 1.0),
            "0.9",
            "Minimum calibration precision."
        ),
        key!(
            "integrations.superorchestrator",
            Kind::Enum(&["auto", "on", "off"]),
            "auto",
            "Superorchestrator integration mode."
        ),
        key!(
            "integrations.otherskill",
            Kind::Enum(&["auto", "on", "off"]),
            "auto",
            "Otherskill integration mode."
        ),
        key!(
            "ui.web.enabled",
            Kind::Bool,
            "false",
            "Enable the web dashboard."
        ),
        key!(
            "ui.web.bind",
            Kind::String,
            "127.0.0.1:7878",
            "Web dashboard bind address."
        ),
    ];
    for (enabled, level) in [
        ("hosts.claude.enabled", "hosts.claude.level"),
        ("hosts.codex.enabled", "hosts.codex.level"),
        ("hosts.agy.enabled", "hosts.agy.level"),
        ("hosts.cursor.enabled", "hosts.cursor.level"),
        ("hosts.kimi.enabled", "hosts.kimi.level"),
        ("hosts.deepseek.enabled", "hosts.deepseek.level"),
        ("hosts.opencode.enabled", "hosts.opencode.level"),
        ("hosts.pi.enabled", "hosts.pi.level"),
    ] {
        keys.push(Key {
            name: enabled,
            kind: Kind::Bool,
            default: "true",
            description: "Enable this host adapter.",
            restricted: false,
        });
        keys.push(Key {
            name: level,
            kind: Kind::Enum(&["auto", "hooks", "shim", "fold-only"]),
            default: "auto",
            description: "Select this host's integration level.",
            restricted: false,
        });
    }
    keys
}
fn parse_text(key: &Key, raw: &str) -> Result<Value, ConfigError> {
    unimplemented!()
}
fn parse_item(key: &Key, item: &toml_edit::Item) -> Result<Value, ConfigError> {
    unimplemented!()
}
fn preset(name: &str) -> &'static [(&'static str, &'static str)] {
    match name {
        "observe" => &[
            ("inject.enabled", "false"),
            ("loop.mode", "shadow"),
            ("prune.mode", "report"),
            ("decide.shadow", "true"),
        ],
        "aggressive" => &[
            ("facts.promote_confirmations", "2"),
            ("loop.level", "aggressive"),
            ("prune.mode", "apply"),
            ("decide.agreement", "false"),
            ("inject.max_tokens", "900"),
        ],
        "offline" => &[
            ("facts.evidence", "code"),
            ("decide.enabled", "false"),
            ("prune.mode", "propose"),
            ("inject.memory", "false"),
        ],
        _ => &[],
    }
}
fn restricted(key: &Key, value: &Value) -> bool {
    key.restricted && (key.name != "prune.mode" || *value == Value::String("apply".to_owned()))
}
fn apply(
    config: &mut EffectiveConfig,
    key: &Key,
    value: Value,
    layer: Layer,
    repo_selected: Option<Layer>,
) {
    if (matches!(layer, Layer::Repo | Layer::RepoLocal)
        || (layer == Layer::Preset && repo_selected.is_some()))
        && restricted(key, &value)
    {
        config.ignored.push(Ignored {
            layer: repo_selected.unwrap_or(layer),
            key: key.name.to_owned(),
            reason: "repository configuration cannot set this value".into(),
        });
    } else {
        config.values.insert(key.name.to_owned(), (value, layer));
    }
}
fn flatten(
    table: &dyn toml_edit::TableLike,
    prefix: &str,
    output: &mut Vec<(String, toml_edit::Item)>,
) {
    for (name, item) in table.iter() {
        let key = if prefix.is_empty() {
            name.to_owned()
        } else {
            format!("{prefix}.{name}")
        };
        if let Some(child) = item.as_table_like() {
            flatten(child, &key, output);
        } else {
            output.push((key, item.clone()));
        }
    }
}
fn read_file(path: &std::path::Path) -> Result<Vec<(String, toml_edit::Item)>, ConfigError> {
    unimplemented!()
}
/// Load and validate all layers. Malformed files return an error with their path.
///
/// # Errors
/// Returns an error for unreadable or malformed files.
pub fn load(inputs: &LoadInputs) -> Result<EffectiveConfig, ConfigError> {
    unimplemented!()
}
/// Check whether a process environment disables tokenimum.
#[must_use]
pub fn is_disabled(env: &[(String, String)]) -> bool {
    env.iter()
        .rev()
        .find(|(key, _)| key == "TKM_DISABLE")
        .is_some_and(|(_, v)| v == "1" || v.eq_ignore_ascii_case("true"))
}
/// A TOML layer preserving document formatting.
pub struct ConfigFile {
    path: PathBuf,
    layer: Layer,
    document: toml_edit::DocumentMut,
}
impl ConfigFile {
    /// Load a file or start an empty document when missing.
    ///
    /// # Errors
    /// Returns an error if the file cannot be read or parsed.
    pub fn load(path: PathBuf, layer: Layer) -> Result<Self, ConfigError> {
        unimplemented!()
    }
    /// Retrieve a value as TOML text.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<String> {
        let mut current = self.document.as_item();
        for part in key.split('.') {
            current = current.as_table_like()?.get(part)?;
        }
        Some(current.to_string())
    }
    /// Validate and set a dotted key.
    ///
    /// # Errors
    /// Returns an error for unknown, invalid, or disallowed values.
    pub fn set(&mut self, key: &str, raw: &str) -> Result<(), ConfigError> {
        unimplemented!()
    }
    /// Remove a dotted key.
    ///
    /// # Errors
    /// Returns an error for an unknown key.
    pub fn unset(&mut self, key: &str) -> Result<(), ConfigError> {
        unimplemented!()
    }
    /// Save the document to its layer file.
    ///
    /// # Errors
    /// Returns an error if the destination cannot be written.
    pub fn save(&self) -> Result<(), ConfigError> {
        unimplemented!()
    }
}
/// Describe a registered key and its effective source.
#[must_use]
pub fn explain(config: &EffectiveConfig, key: &str) -> Option<String> {
    let keys = registry();
    let k = keys.iter().find(|k| k.name == key)?;
    let allowed = match k.kind {
        Kind::Bool => "true | false".to_owned(),
        Kind::Int => "non-negative integer".to_owned(),
        Kind::Float(min, max) => format!("{min}..={max}"),
        Kind::String => "string".to_owned(),
        Kind::Enum(values) => values.join(" | "),
        Kind::List => "string list".to_owned(),
    };
    Some(format!(
        "{} Default: {}. Allowed: {}. Source: {:?}.",
        k.description,
        k.default,
        allowed,
        config.source(key)?
    ))
}
/// Return keys whose values differ from the active preset.
#[must_use]
pub fn diff_from_preset(config: &EffectiveConfig) -> Vec<String> {
    let mut baseline = EffectiveConfig::default();
    let keys = registry();
    for key in &keys {
        let value = if key.name == "loop.escalate_command" {
            Value::Unset
        } else if let Ok(v) = parse_text(key, key.default) {
            v
        } else {
            continue;
        };
        baseline
            .values
            .insert(key.name.into(), (value, Layer::Default));
    }
    let name = config.get_str("preset").unwrap_or("balanced");
    for (name, raw) in preset(name) {
        if let Some(key) = keys.iter().find(|k| k.name == *name) {
            if let Ok(value) = parse_text(key, raw) {
                baseline
                    .values
                    .insert((*name).into(), (value, Layer::Preset));
            }
        }
    }
    config
        .values
        .iter()
        .filter(|(name, (value, _))| name.as_str() != "preset" && baseline.get(name) != Some(value))
        .map(|(name, _)| name.clone())
        .collect()
}
