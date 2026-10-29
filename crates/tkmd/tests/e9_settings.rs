#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]

use std::fs;
use tkm_config::paths::Paths;
use tkm_integrations::detect::Mode;
use tkmd::settings::Settings;

#[test]
fn repository_integration_modes_and_decisions_are_resolved() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(".tokenimum.toml"), "[integrations]\notherskill = 'off'\nsuperorchestrator = 'off'\n[decide]\nenabled = false\n").unwrap();
    let paths = Paths::from_dirs(
        dir.path(),
        Some(&dir.path().join("data")),
        Some(&dir.path().join("config")),
        Some(&dir.path().join("run")),
    );
    let settings = Settings::load(&paths, Some(dir.path()), Vec::new());
    assert_eq!(settings.otherskill_mode, Mode::Off);
    assert_eq!(settings.superorchestrator_mode, Mode::Off);
    assert!(!settings.decide.enabled);
}
