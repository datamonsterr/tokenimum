#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use tkm_config::config::{Layer, LoadInputs, load};
#[test]
fn entries_exposes_values_and_sources_in_key_order() {
    let dir = std::env::temp_dir().join(format!("tkm-entries-missing-{}", std::process::id()));
    let config = load(&LoadInputs {
        global_path: dir.join("missing.toml"),
        ..LoadInputs::default()
    })
    .unwrap();
    let entries: Vec<_> = config.entries().collect();
    assert!(entries.windows(2).all(|pair| pair[0].0 < pair[1].0));
    assert!(
        entries
            .iter()
            .any(|(key, _, layer)| *key == "preset" && *layer == Layer::Default)
    );
}
