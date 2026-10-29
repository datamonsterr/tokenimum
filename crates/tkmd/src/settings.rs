//! Effective settings the daemon needs, derived from the layered configuration (SPEC 10.2).
//!
//! Settings are resolved when a session starts (not on `tool_done`) and stored with the session.

use std::path::Path;
use std::sync::Arc;

use tkm_config::config::{EffectiveConfig, Layer, LoadInputs, load};
use tkm_config::paths::Paths;
use tkm_core::facts::FactsConfig;
use tkm_core::inject;
use tkm_core::loopguard::LoopConfig;
use tkm_core::prune::{PruneConfig, PruneMode};
use tkm_decide::config::{ChainModel, DecideConfig};

/// `loop.mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopMode {
    /// Loop guard disabled.
    Off,
    /// Decisions are recorded, nothing is injected.
    Shadow,
    /// Redirects are injected (once the answering model is calibrated).
    Redirect,
}

/// Configured integration level for one host.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum HostLevel {
    /// Select the host's standard integration.
    #[default]
    Auto,
    /// Use native hooks.
    Hooks,
    /// Use the host shim.
    Shim,
    /// Read session logs without live integration.
    FoldOnly,
}

/// Per-host integration settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostSettings {
    /// Whether this host is enabled.
    pub enabled: bool,
    /// Requested integration level.
    pub level: HostLevel,
}

impl Default for HostSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            level: HostLevel::Auto,
        }
    }
}

/// Settings read by the daemon.
#[derive(Debug, Clone)]
pub struct Settings {
    /// Host-specific settings, keyed by host wire name.
    pub hosts: std::collections::HashMap<String, HostSettings>,
    /// `loop.mode`.
    pub loop_mode: LoopMode,
    /// Loop guard thresholds (`loop.*` over the `loop.level` preset).
    pub loop_cfg: LoopConfig,
    /// `loop.escalate_command`.
    pub escalate_command: Option<String>,
    /// `inject.enabled`.
    pub inject_enabled: bool,
    /// Inject budget (`inject.max_tokens`, `inject.memory`, `facts.max_*`).
    pub inject: inject::Config,
    /// Whether repository facts are collected and injected.
    pub facts_enabled: bool,
    /// Whether session usage is recorded in the ledger.
    pub ledger_enabled: bool,
    /// Fact lifecycle settings.
    pub facts: FactsConfig,
    /// Pruning evidence settings.
    pub prune: PruneConfig,
    /// Session integration modes.
    pub otherskill_mode: tkm_integrations::detect::Mode,
    /// Session orchestrator integration mode.
    pub superorchestrator_mode: tkm_integrations::detect::Mode,
    /// Local cache-aware model prices.
    pub pricing: tkm_core::ledger::PriceTable,
    /// Minimum elapsed days in the initial shadow window.
    pub shadow_days: u64,
    /// Minimum distinct judged turns in the initial shadow window.
    pub shadow_turns: u64,
    /// Decision client settings.
    pub decide: DecideConfig,
    /// How long `prompt` waits for relevance scoring before using the cached default block.
    pub relevance_timeout_ms: u64,
    /// Problems found while loading configuration, for `errors.log`.
    pub config_errors: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        let facts = FactsConfig::default();
        Self {
            hosts: std::collections::HashMap::new(),
            loop_mode: LoopMode::Redirect,
            loop_cfg: LoopConfig::default(),
            escalate_command: None,
            inject_enabled: true,
            inject: inject::Config {
                max_tokens: 600,
                max_facts: facts.max_facts,
                max_fact_tokens: facts.max_tokens,
                memory: true,
            },
            facts_enabled: true,
            ledger_enabled: true,
            facts,
            prune: PruneConfig::default(),
            otherskill_mode: tkm_integrations::detect::Mode::Auto,
            superorchestrator_mode: tkm_integrations::detect::Mode::Auto,
            shadow_days: 14,
            shadow_turns: 200,
            decide: DecideConfig::default(),
            pricing: tkm_core::ledger::PriceTable::default(),
            relevance_timeout_ms: 1_200,
            config_errors: Vec::new(),
        }
    }
}

fn explicit(config: &EffectiveConfig, key: &str) -> bool {
    config
        .source(key)
        .is_some_and(|layer| layer != Layer::Default)
}

fn int(config: &EffectiveConfig, key: &str) -> Option<i64> {
    config.get_int(key)
}

fn unsigned<T: TryFrom<i64>>(config: &EffectiveConfig, key: &str) -> Option<T> {
    int(config, key).and_then(|v| T::try_from(v).ok())
}

impl Settings {
    /// Maps an effective configuration onto daemon settings.
    pub fn from_config(config: &EffectiveConfig) -> Self {
        let mut out = Self {
            config_errors: config.errors.iter().map(ToString::to_string).collect(),
            ..Self::default()
        };

        for host in [
            "claude", "codex", "agy", "cursor", "kimi", "deepseek", "opencode", "pi",
        ] {
            let enabled = config
                .get_bool(&format!("hosts.{host}.enabled"))
                .unwrap_or(true);
            let level = match config.get_str(&format!("hosts.{host}.level")) {
                Some("hooks") => HostLevel::Hooks,
                Some("shim") => HostLevel::Shim,
                Some("fold-only") => HostLevel::FoldOnly,
                _ => HostLevel::Auto,
            };
            out.hosts
                .insert(host.to_owned(), HostSettings { enabled, level });
        }
        out.otherskill_mode = config
            .get_str("integrations.otherskill")
            .and_then(|s| s.parse().ok())
            .unwrap_or_default();
        out.superorchestrator_mode = config
            .get_str("integrations.superorchestrator")
            .and_then(|s| s.parse().ok())
            .unwrap_or_default();
        out.loop_mode = match config.get_str("loop.mode") {
            Some("off") => LoopMode::Off,
            Some("shadow") => LoopMode::Shadow,
            _ => LoopMode::Redirect,
        };
        let mut lc = LoopConfig::preset(config.get_str("loop.level").unwrap_or("default"));
        macro_rules! over_int {
            ($key:literal, $field:ident) => {
                if explicit(config, $key)
                    && let Some(v) = unsigned(config, $key)
                {
                    lc.$field = v;
                }
            };
        }
        macro_rules! over_float {
            ($key:literal, $field:ident) => {
                if explicit(config, $key)
                    && let Some(v) = config.get_float($key)
                {
                    lc.$field = v;
                }
            };
        }
        over_int!("loop.min_turns", min_turns);
        over_int!("loop.gate_min_turns", gate_min_turns);
        over_int!("loop.gate_error_outputs", gate_error_outputs);
        over_int!("loop.edit_churn", edit_churn);
        over_int!("loop.envelope_min_samples", envelope_min_samples);
        over_int!("loop.redirects_per_turn", redirects_per_turn);
        over_float!("loop.stagnation", stagnation);
        over_float!("loop.envelope_margin", envelope_margin);
        over_float!("loop.backstop_mult", backstop_mult);
        if explicit(config, "loop.envelope_pct")
            && let Some(v) = config
                .get_int("loop.envelope_pct")
                .and_then(|v| u32::try_from(v).ok())
        {
            lc.envelope_pct = f64::from(v);
        }
        out.loop_cfg = lc;
        out.escalate_command = config
            .get_str("loop.escalate_command")
            .filter(|c| !c.trim().is_empty())
            .map(str::to_owned);

        if let Some(v) = config.get_bool("inject.enabled") {
            out.inject_enabled = v;
        }
        if let Some(v) = unsigned(config, "inject.max_tokens") {
            out.inject.max_tokens = v;
        }
        if let Some(v) = config.get_bool("inject.memory") {
            out.inject.memory = v;
        }

        out.facts_enabled = config.get_bool("facts.enabled").unwrap_or(true);
        out.ledger_enabled = config.get_bool("ledger.enabled").unwrap_or(true);
        let facts = &mut out.facts;
        if let Some(v) = unsigned(config, "facts.promote_confirmations") {
            facts.promote_confirmations = v;
        }
        if let Some(v) = config.get_float("facts.recency_decay") {
            facts.recency_decay = v;
        }
        if let Some(v) = config.get_float("facts.contradiction_weight") {
            facts.contradiction_weight = v;
        }
        if let Some(v) = config.get_float("facts.fire_gate") {
            facts.fire_gate = v;
        }
        if let Some(v) = unsigned(config, "facts.fire_min_sessions") {
            facts.fire_min_sessions = v;
        }
        if let Some(v) = unsigned(config, "facts.max_facts") {
            facts.max_facts = v;
        }
        if let Some(v) = unsigned(config, "facts.max_tokens") {
            facts.max_tokens = v;
        }
        out.inject.max_facts = out.facts.max_facts;
        out.inject.max_fact_tokens = out.facts.max_tokens;

        if let Some(v) = unsigned(config, "prune.evidence_prompts") {
            out.prune.evidence_prompts = v;
        }
        if let Some(v) = unsigned(config, "prune.min_sessions") {
            out.prune.min_sessions = v;
        }
        if let Some(v) = unsigned(config, "prune.sessions_any") {
            out.prune.sessions_any = v;
        }
        if let Some(v) = unsigned(config, "prune.quiet_minutes") {
            out.prune.quiet_minutes = v;
        }
        if let Some(protect) = config.get_list("prune.protect") {
            out.prune.protect = protect.to_vec();
        }
        out.prune.mode = match config.get_str("prune.mode") {
            Some("off") => PruneMode::Off,
            Some("report") => PruneMode::Report,
            Some("apply") => PruneMode::Apply,
            _ => PruneMode::Propose,
        };

        if let Some(v) = unsigned(config, "calibration.shadow_days") {
            out.shadow_days = v;
        }
        if let Some(v) = unsigned(config, "calibration.shadow_turns") {
            out.shadow_turns = v;
        }
        if let Some(v) = config.get_bool("decide.enabled") {
            out.decide.enabled = v;
        }
        if let Some(v) = config.get_bool("decide.agreement") {
            out.decide.agreement = v;
        }
        if let Some(v) = config.get_float("decide.budget_daily_usd") {
            out.decide.daily_cap_usd = v;
        }
        if let Some(v) = config.get_float("decide.budget_session_usd") {
            out.decide.session_cap_usd = v;
        }
        // The registry default names undated slugs that the client rejects; only an explicit chain
        // replaces the client's own pinned default.
        if explicit(config, "decide.chain")
            && let Some(list) = config.get_list("decide.chain").filter(|l| !l.is_empty())
        {
            out.decide.chain = list
                .iter()
                .map(|slug| ChainModel {
                    slug: slug.clone(),
                    free_probe: slug.ends_with(":free"),
                })
                .collect();
        }
        out
    }

    /// Returns the effective host integration level; fallback policy can override this accessor.
    #[must_use]
    pub fn effective_host_level(&self, host: tkm_proto::Host) -> HostLevel {
        self.host(host).level
    }

    /// Returns the settings for a host.
    #[must_use]
    pub fn host(&self, host: tkm_proto::Host) -> HostSettings {
        self.hosts.get(host.as_str()).copied().unwrap_or_default()
    }

    /// Loads the layered configuration for a repository root and maps it. A configuration that
    /// cannot be loaded yields the defaults plus one entry in [`Settings::config_errors`].
    pub fn load(paths: &Paths, repo_root: Option<&Path>, env: Vec<(String, String)>) -> Self {
        let inputs = LoadInputs {
            global_path: paths.global_config.clone(),
            repo_root: repo_root.map(Path::to_path_buf),
            env,
            flags: Vec::new(),
        };
        let mut settings = match load(&inputs) {
            Ok(config) => Self::from_config(&config),
            Err(error) => Self {
                config_errors: vec![error.to_string()],
                ..Self::default()
            },
        };
        let pricing_path = paths.config_dir.join("pricing.toml");
        match std::fs::read_to_string(&pricing_path) {
            Ok(text) => match tkm_core::ledger::PriceTable::parse(&text) {
                Ok(pricing) => settings.pricing = pricing,
                Err(error) => settings
                    .config_errors
                    .push(format!("{}: {error}", pricing_path.display())),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => settings
                .config_errors
                .push(format!("{}: {error}", pricing_path.display())),
        }
        settings
    }
}

/// Supplies settings when a session starts.
pub trait SettingsSource: Send + Sync {
    /// Settings for a repository root (or none when the cwd is not in a checkout).
    fn settings_for(&self, repo_root: Option<&Path>) -> Arc<Settings>;
}

/// Reads the layered configuration files and the process environment.
pub struct FileSettings {
    paths: Paths,
    env: Vec<(String, String)>,
}

impl FileSettings {
    /// Settings source over `paths` and a captured environment.
    pub fn new(paths: Paths, env: Vec<(String, String)>) -> Self {
        Self { paths, env }
    }
}

impl SettingsSource for FileSettings {
    fn settings_for(&self, repo_root: Option<&Path>) -> Arc<Settings> {
        Arc::new(Settings::load(&self.paths, repo_root, self.env.clone()))
    }
}

/// Always returns the same settings (tests and benches).
pub struct FixedSettings(Arc<Settings>);

impl FixedSettings {
    /// Wraps fixed settings.
    pub fn new(settings: Settings) -> Self {
        Self(Arc::new(settings))
    }
}

impl SettingsSource for FixedSettings {
    fn settings_for(&self, _repo_root: Option<&Path>) -> Arc<Settings> {
        self.0.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_config_registry_defaults() {
        let dir = std::env::temp_dir().join("tkmd-settings-test-nonexistent");
        let paths = Paths::from_dirs(&dir, Some(&dir), Some(&dir), Some(&dir));
        let loaded = Settings::load(&paths, None, vec![]);
        let default = Settings::default();
        assert_eq!(loaded.loop_mode, default.loop_mode);
        assert_eq!(loaded.loop_cfg, default.loop_cfg);
        assert_eq!(loaded.inject, default.inject);
        assert!(
            loaded.config_errors.is_empty(),
            "{:?}",
            loaded.config_errors
        );
    }

    #[test]
    fn mechanism_switches_are_loaded_from_configuration() {
        let dir = std::env::temp_dir().join("tkmd-settings-switches-test-nonexistent");
        let paths = Paths::from_dirs(&dir, Some(&dir), Some(&dir), Some(&dir));
        let env = vec![
            ("TKM_FACTS_ENABLED".to_owned(), "false".to_owned()),
            ("TKM_LEDGER_ENABLED".to_owned(), "false".to_owned()),
        ];
        let settings = Settings::load(&paths, None, env);
        assert!(!settings.facts_enabled);
        assert!(!settings.ledger_enabled);
    }

    #[test]
    fn per_host_switches_and_levels_are_loaded() {
        let dir = std::env::temp_dir().join("tkmd-settings-hosts-test-nonexistent");
        let paths = Paths::from_dirs(&dir, Some(&dir), Some(&dir), Some(&dir));
        let settings = Settings::load(
            &paths,
            None,
            vec![
                ("TKM_HOSTS_CLAUDE_ENABLED".into(), "false".into()),
                ("TKM_HOSTS_CLAUDE_LEVEL".into(), "fold-only".into()),
                ("TKM_HOSTS_PI_LEVEL".into(), "shim".into()),
            ],
        );
        assert!(!settings.host(tkm_proto::Host::Claude).enabled);
        assert_eq!(
            settings.host(tkm_proto::Host::Claude).level,
            HostLevel::FoldOnly
        );
        assert_eq!(settings.host(tkm_proto::Host::Pi).level, HostLevel::Shim);
        assert_eq!(settings.host(tkm_proto::Host::Codex).level, HostLevel::Auto);
    }

    #[test]
    fn all_host_switches_and_levels_are_independent() {
        let dir = std::env::temp_dir().join("tkmd-settings-all-hosts-test-nonexistent");
        let paths = Paths::from_dirs(&dir, Some(&dir), Some(&dir), Some(&dir));
        let hosts = [
            tkm_proto::Host::Claude,
            tkm_proto::Host::Codex,
            tkm_proto::Host::Agy,
            tkm_proto::Host::Cursor,
            tkm_proto::Host::Kimi,
            tkm_proto::Host::Deepseek,
            tkm_proto::Host::Opencode,
            tkm_proto::Host::Pi,
        ];
        let levels = ["auto", "hooks", "shim", "fold-only"];
        for level in levels {
            let env = hosts
                .iter()
                .flat_map(|host| {
                    [
                        (
                            format!("TKM_HOSTS_{}_ENABLED", host.as_str().to_uppercase()),
                            "false".into(),
                        ),
                        (
                            format!("TKM_HOSTS_{}_LEVEL", host.as_str().to_uppercase()),
                            level.into(),
                        ),
                    ]
                })
                .collect();
            let settings = Settings::load(&paths, None, env);
            for host in hosts {
                assert!(!settings.host(host).enabled, "{}", host.as_str());
                let expected = match level {
                    "hooks" => HostLevel::Hooks,
                    "shim" => HostLevel::Shim,
                    "fold-only" => HostLevel::FoldOnly,
                    _ => HostLevel::Auto,
                };
                assert_eq!(settings.effective_host_level(host), expected);
            }
        }
    }

    #[test]
    fn agreement_defaults_off_and_can_be_enabled_by_configuration() {
        let dir = std::env::temp_dir().join("tkmd-agreement-settings-nonexistent");
        let paths = Paths::from_dirs(&dir, Some(&dir), Some(&dir), Some(&dir));
        assert!(!Settings::default().decide.agreement);
        let settings = Settings::load(
            &paths,
            None,
            vec![("TKM_DECIDE_AGREEMENT".into(), "true".into())],
        );
        assert!(settings.decide.agreement);
    }

    #[test]
    fn environment_overrides_loop_mode_and_decide_switch() {
        let dir = std::env::temp_dir().join("tkmd-settings-test-nonexistent");
        let paths = Paths::from_dirs(&dir, Some(&dir), Some(&dir), Some(&dir));
        let env = vec![
            ("TKM_LOOP_MODE".to_owned(), "shadow".to_owned()),
            ("TKM_DECIDE_ENABLED".to_owned(), "false".to_owned()),
            ("TKM_LOOP_GATE_MIN_TURNS".to_owned(), "9".to_owned()),
        ];
        let settings = Settings::load(&paths, None, env);
        assert_eq!(settings.loop_mode, LoopMode::Shadow);
        assert!(!settings.decide.enabled);
        assert_eq!(settings.loop_cfg.gate_min_turns, 9);
    }
}
