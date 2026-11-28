// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use crate::error::HostError;

pub fn launch_agent_plist(
    binary: &Path,
    orb: &Path,
    runner_binary: &Path,
) -> Result<String, HostError> {
    let bin = absolute_text(binary)?;
    let orb = absolute_text(orb)?;
    let runner_binary = absolute_text(runner_binary)?;
    Ok(format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>com.tailrocks.velnor.host</string>
<key>ProgramArguments</key><array>
<string>{bin}</string>
<string>daemon</string>
<string>run</string>
</array>
<key>RunAtLoad</key><true/>
<key>EnvironmentVariables</key><dict>
<key>VELNOR_ORB</key><string>{orb}</string>
<key>VELNOR_RUNNER_BINARY</key><string>{runner_binary}</string>
</dict>
</dict></plist>
"#
    ))
}

fn absolute_text(path: &Path) -> Result<String, HostError> {
    path.to_str()
        .filter(|text| text.starts_with('/') && !text.chars().any(xml_forbidden))
        .map(xml_text)
        .ok_or(HostError::Path)
}

fn xml_forbidden(ch: char) -> bool {
    matches!(ch, '\0' | '\n' | '\r')
}

fn xml_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
    }
    out
}

#[must_use]
pub fn keychain_import_argv(service: &str) -> Vec<String> {
    vec![
        "security".to_owned(),
        "add-generic-password".to_owned(),
        "-s".to_owned(),
        service.to_owned(),
        "-a".to_owned(),
        "velnor-host".to_owned(),
        "-U".to_owned(),
        "-w".to_owned(),
    ]
}
