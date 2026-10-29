//! Entry point for `tkm top`.
use tkm_config::paths::Paths;
/// Run the monitor.
///
/// # Errors
/// Returns an error when the snapshot or terminal cannot be used.
pub fn run_top(paths: &Paths) -> Result<(), crate::Error> {
    crate::tui::run(paths)
}
