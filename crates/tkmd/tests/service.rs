//! E4.7 service unit templates.
#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]

use std::path::Path;

use tkm_config::paths::Paths;
use tkmd::service::{render_launchd_plist, render_systemd_unit};

fn paths() -> Paths {
    Paths::from_dirs(
        Path::new("/home/u"),
        Some(Path::new("/home/u/.local/share")),
        Some(Path::new("/home/u/.config")),
        Some(Path::new("/run/user/1000")),
    )
}

#[test]
fn systemd_user_unit_renders_exec_and_environment() {
    let unit = render_systemd_unit(Path::new("/home/u/.cargo/bin/tkmd"), &paths());
    assert!(unit.contains("[Unit]") && unit.contains("[Service]") && unit.contains("[Install]"));
    assert!(unit.contains("ExecStart=/home/u/.cargo/bin/tkmd"));
    assert!(unit.contains("Restart=on-failure"));
    assert!(unit.contains("WantedBy=default.target"));
    assert!(unit.contains("StandardError=append:/home/u/.local/share/tokenimum/errors.log"));
    assert!(!unit.contains("{{"), "no unrendered placeholder: {unit}");
}

#[test]
fn launchd_agent_renders_label_program_and_log() {
    let plist = render_launchd_plist(Path::new("/usr/local/bin/tkmd"), &paths());
    assert!(plist.starts_with("<?xml"));
    assert!(plist.contains("<key>Label</key>\n    <string>com.tokenimum.tkmd</string>"));
    assert!(plist.contains("<string>/usr/local/bin/tkmd</string>"));
    assert!(plist.contains("<key>StandardErrorPath</key>\n    <string>/home/u/.local/share/tokenimum/errors.log</string>"));
    assert!(plist.contains("<key>RunAtLoad</key>"));
    assert!(!plist.contains("{{"));
    assert_eq!(
        plist.matches("<dict>").count(),
        plist.matches("</dict>").count()
    );
}

#[test]
fn paths_with_xml_specials_are_escaped_in_the_plist() {
    let plist = render_launchd_plist(Path::new("/opt/a&b/tkmd"), &paths());
    assert!(plist.contains("/opt/a&amp;b/tkmd"));
}
