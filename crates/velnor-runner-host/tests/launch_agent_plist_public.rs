// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use velnor_runner_host::{HostError, launch_agent_plist};

#[test]
fn plist_renders_environment_variables() -> Result<(), Box<dyn std::error::Error>> {
    let plist = launch_agent_plist(
        Path::new("/opt/velnor/velnor-host"),
        Path::new("/opt/homebrew/bin/o&rb"),
        Path::new("/opt/velnor/bin/<runner>"),
    )?;
    assert!(plist.contains(
        "<key>EnvironmentVariables</key><dict>\n<key>VELNOR_ORB</key><string>/opt/homebrew/bin/o&amp;rb</string>\n<key>VELNOR_RUNNER_BINARY</key><string>/opt/velnor/bin/&lt;runner&gt;</string>\n</dict>"
    ));
    assert!(plist.contains("<key>RunAtLoad</key><true/>"));
    let proof = Path::new(env!("CARGO_TARGET_TMPDIR")).join("launch-agent-proof.plist");
    std::fs::write(&proof, &plist)?;
    println!("{}", proof.display());
    Ok(())
}

#[test]
fn plist_refuses_relative_environment_paths() {
    let binary = Path::new("/opt/velnor/velnor-host");
    let absolute = Path::new("/opt/bin/x");
    for (orb, runner) in [
        (Path::new("orb"), absolute),
        (absolute, Path::new("runner")),
    ] {
        assert!(matches!(
            launch_agent_plist(binary, orb, runner),
            Err(HostError::Path)
        ));
    }
}

#[test]
fn plist_refuses_characters_that_xml_cannot_hold() {
    let binary = Path::new("/opt/velnor/velnor-host");
    let absolute = Path::new("/opt/bin/x");
    for bad in [
        "/opt/a\u{b}b",
        "/opt/a\u{1}b",
        "/opt/a\u{fffe}b",
        "/opt/a\tb",
    ] {
        assert!(matches!(
            launch_agent_plist(binary, Path::new(bad), absolute),
            Err(HostError::Path)
        ));
        assert!(matches!(
            launch_agent_plist(binary, absolute, Path::new(bad)),
            Err(HostError::Path)
        ));
    }
}
