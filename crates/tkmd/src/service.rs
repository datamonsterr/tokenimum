//! E4.7 service unit templates, rendered by pure functions (SPEC 11.1).
//!
//! The templates live in `crates/tkmd/service/`. `tkm init` (E7) writes the rendered text to
//! `~/.config/systemd/user/tkmd.service` or `~/Library/LaunchAgents/com.tokenimum.tkmd.plist`.
//! Both route the daemon's stderr to `errors.log`.

use std::path::Path;

use tkm_config::paths::Paths;

const SYSTEMD: &str = include_str!("../service/tkmd.service.in");
const LAUNCHD: &str = include_str!("../service/com.tokenimum.tkmd.plist.in");

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Renders the systemd user unit for the daemon at `exe`.
pub fn render_systemd_unit(exe: &Path, paths: &Paths) -> String {
    SYSTEMD
        .replace("{{EXE}}", &exe.to_string_lossy())
        .replace("{{ERRORS_LOG}}", &paths.errors_log.to_string_lossy())
}

/// Renders the launchd agent property list for the daemon at `exe`.
pub fn render_launchd_plist(exe: &Path, paths: &Paths) -> String {
    LAUNCHD
        .replace("{{EXE}}", &xml_escape(&exe.to_string_lossy()))
        .replace(
            "{{ERRORS_LOG}}",
            &xml_escape(&paths.errors_log.to_string_lossy()),
        )
}
