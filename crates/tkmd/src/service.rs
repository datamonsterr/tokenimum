//! E4.7 service unit templates, rendered by pure functions (SPEC 11.1).
//!
//! The template files live in `packaging/`; this module holds the same text as constants. `tkm init` (E7) writes the rendered text to
//! `~/.config/systemd/user/tkmd.service` or `~/Library/LaunchAgents/com.tokenimum.tkmd.plist`.
//! Both route the daemon's stderr to `errors.log`.

use std::path::Path;

use tkm_config::paths::Paths;

/// Same text as `packaging/systemd/tkmd.service.in` (a test keeps them equal). Kept as a constant
/// because `cargo geiger` rejects `include_str!` of non-Rust files.
const SYSTEMD: &str = r"[Unit]
Description=tokenimum daemon (tkmd)
Documentation=https://github.com/datamonsterr/tokenimum

[Service]
Type=simple
ExecStart={{EXE}}
Restart=on-failure
RestartSec=2
StandardOutput=null
StandardError=append:{{ERRORS_LOG}}

[Install]
WantedBy=default.target
";
/// Same text as `packaging/launchd/com.tokenimum.tkmd.plist.in`.
const LAUNCHD: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.tokenimum.tkmd</string>
    <key>ProgramArguments</key>
    <array>
        <string>{{EXE}}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <dict>
        <key>SuccessfulExit</key>
        <false/>
    </dict>
    <key>StandardOutPath</key>
    <string>/dev/null</string>
    <key>StandardErrorPath</key>
    <string>{{ERRORS_LOG}}</string>
</dict>
</plist>
"#;

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

#[cfg(test)]
mod tests {
    use super::*;

    fn packaging(name: &str) -> String {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../packaging")
            .join(name);
        std::fs::read_to_string(path).unwrap()
    }

    #[test]
    fn constants_match_the_packaging_files() {
        assert_eq!(SYSTEMD, packaging("systemd/tkmd.service.in"));
        assert_eq!(LAUNCHD, packaging("launchd/com.tokenimum.tkmd.plist.in"));
    }
}
