//! `LaunchAgent` program arguments and Keychain argv. The secret is not an argument.

use std::path::Path;

use crate::error::HostError;

/// Plist whose program is absolute `velnor-host daemon run`. No double-fork.
///
/// # Errors
///
/// Returns [`HostError::Path`] unless `binary` is absolute unicode.
pub fn launch_agent_plist(binary: &Path) -> Result<String, HostError> {
    let bin = binary
        .to_str()
        .filter(|text| text.starts_with('/') && !text.chars().any(xml_forbidden))
        .ok_or(HostError::Path)?;
    let bin = xml_text(bin);
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
</dict></plist>
"#
    ))
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

/// Argv for a Keychain import.
///
/// A trailing `-w` with no value makes `security` prompt. The password
/// is not an argument. `-w <password>` is the insecure form and is not used.
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
