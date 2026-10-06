//! `LaunchAgent` plist and Keychain argv. The secret is not an argument.

use std::path::Path;

use crate::{HostError, keychain_import_argv, launch_agent_plist};

fn program_arguments(plist: &str) -> Vec<&str> {
    let Some(start) = plist.find("<key>ProgramArguments</key>") else {
        return Vec::new();
    };
    let Some(end) = plist[start..].find("</array>") else {
        return Vec::new();
    };
    xml_strings(&plist[start..start + end])
}

fn xml_strings(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("<string>") {
        let after = &rest[start + "<string>".len()..];
        let Some(end) = after.find("</string>") else {
            break;
        };
        out.push(&after[..end]);
        rest = &after[end + "</string>".len()..];
    }
    out
}

#[test]
fn launch_agent_runs_daemon_in_the_foreground() -> Result<(), HostError> {
    let binary = "/usr/local/bin/velnor-host";
    let plist = launch_agent_plist(Path::new(binary))?;
    assert!(plist.contains("<key>Label</key><string>com.tailrocks.velnor.host</string>"));
    assert!(plist.contains("<key>RunAtLoad</key><true/>"));
    assert!(!plist.contains("<false/>"));
    assert_eq!(plist.matches("<array>").count(), 1);
    assert!(!plist.contains("<key>Program</key>"));
    assert!(!plist.contains("/bin/sh"));
    assert!(!plist.contains("/bin/bash"));
    assert!(!plist.contains("nohup"));
    assert!(!plist.contains("daemonize"));
    assert!(!plist.contains("AbandonProcessGroup"));
    assert_eq!(program_arguments(&plist), vec![binary, "daemon", "run"]);
    assert_eq!(
        launch_agent_plist(Path::new("velnor-host")),
        Err(HostError::Path)
    );
    assert_eq!(
        launch_agent_plist(Path::new("usr/bin/velnor-host")),
        Err(HostError::Path)
    );
    Ok(())
}

#[test]
fn plist_escapes_the_binary_path() -> Result<(), HostError> {
    let plist = launch_agent_plist(Path::new("/opt/velnor&host"))?;
    assert_eq!(
        program_arguments(&plist),
        vec!["/opt/velnor&amp;host", "daemon", "run"]
    );
    assert!(!plist.contains("<string>/opt/velnor&host</string>"));
    Ok(())
}

#[test]
fn keychain_import_does_not_take_the_secret() {
    let secret = "super-secret-value";
    let argv = keychain_import_argv("com.tailrocks.velnor.host");
    assert_eq!(
        argv,
        vec![
            "security".to_owned(),
            "add-generic-password".to_owned(),
            "-s".to_owned(),
            "com.tailrocks.velnor.host".to_owned(),
            "-a".to_owned(),
            "velnor-host".to_owned(),
            "-U".to_owned(),
            "-w".to_owned(),
        ]
    );
    assert_eq!(argv.iter().filter(|arg| *arg == "-w").count(), 1);
    assert_eq!(argv.last().map(String::as_str), Some("-w"));
    assert!(argv.iter().all(|arg| arg != secret));
}
