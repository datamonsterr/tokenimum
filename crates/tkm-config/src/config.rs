//! Layered configuration and file editing.
use std::{collections::BTreeMap, path::PathBuf};
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
        unimplemented!()
    }
}
/// Resolved values and diagnostics.
#[derive(Debug, Clone, Default)]
pub struct EffectiveConfig {
    values: BTreeMap<String, (Value, Layer)>,
    /// Ignored untrusted entries.
    pub ignored: Vec<Ignored>,
    /// Non-fatal validation errors.
    pub errors: Vec<String>,
}
impl EffectiveConfig {
    /// Look up a typed value.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Value> {
        unimplemented!()
    }
    /// Look up a boolean.
    #[must_use]
    pub fn get_bool(&self, key: &str) -> Option<bool> {
        unimplemented!()
    }
    /// Look up an integer.
    #[must_use]
    pub fn get_int(&self, key: &str) -> Option<i64> {
        unimplemented!()
    }
    /// Look up a floating-point value.
    #[must_use]
    pub fn get_float(&self, key: &str) -> Option<f64> {
        unimplemented!()
    }
    /// Look up text.
    #[must_use]
    pub fn get_str(&self, key: &str) -> Option<&str> {
        unimplemented!()
    }
    /// Look up a list.
    #[must_use]
    pub fn get_list(&self, key: &str) -> Option<&[String]> {
        unimplemented!()
    }
    /// Identify the winning source.
    #[must_use]
    pub fn source(&self, key: &str) -> Option<Layer> {
        unimplemented!()
    }
}
/// Load and validate all layers. Malformed files return an error with their path.
pub fn load(inputs: &LoadInputs) -> Result<EffectiveConfig, String> {
    unimplemented!()
}
/// Check whether a process environment disables tokenimum.
#[must_use]
pub fn is_disabled(env: &[(String, String)]) -> bool {
    unimplemented!()
}
/// A TOML layer preserving document formatting.
pub struct ConfigFile {
    path: PathBuf,
    layer: Layer,
    document: toml_edit::DocumentMut,
}
impl ConfigFile {
    /// Load a file or start an empty document when missing.
    pub fn load(path: PathBuf, layer: Layer) -> Result<Self, String> {
        unimplemented!()
    }
    /// Retrieve a value as TOML text.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<String> {
        unimplemented!()
    }
    /// Validate and set a dotted key.
    pub fn set(&mut self, key: &str, value: &str) -> Result<(), String> {
        unimplemented!()
    }
    /// Remove a dotted key.
    pub fn unset(&mut self, key: &str) -> Result<(), String> {
        unimplemented!()
    }
    /// Save the document to its layer file.
    pub fn save(&self) -> Result<(), String> {
        unimplemented!()
    }
}
/// Describe a registered key and its effective source.
#[must_use]
pub fn explain(config: &EffectiveConfig, key: &str) -> Option<String> {
    unimplemented!()
}
/// Return keys whose values differ from the active preset.
#[must_use]
pub fn diff_from_preset(config: &EffectiveConfig) -> Vec<String> {
    unimplemented!()
}
