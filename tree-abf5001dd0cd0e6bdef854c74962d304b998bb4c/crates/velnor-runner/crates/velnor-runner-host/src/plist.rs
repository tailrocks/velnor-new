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
        .filter(|text| text.starts_with('/'))
        .ok_or(HostError::Path)?;
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

/// Argv for a Keychain import that reads the secret on stdin, not on argv.
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
    ]
}
