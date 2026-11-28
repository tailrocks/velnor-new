// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Stdio};

fn tool(dir: &Path, name: &str, mode: u32) -> std::io::Result<std::path::PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(name);
    std::fs::write(&path, "#!/bin/sh\n")?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode))?;
    Ok(path)
}

#[test]
fn the_daemon_refuses_to_start_when_the_orb_or_runner_others_can_replace_is_configured()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().canonicalize()?;
    let loose_orb = tool(&root.join("a"), "orb", 0o775)?;
    let loose_runner = tool(&root.join("b"), "velnor-runner", 0o775)?;
    let safe_orb = tool(&root.join("c"), "orb", 0o755)?;
    let cases = [
        ("VELNOR_ORB", &loose_orb, None),
        ("VELNOR_RUNNER_BINARY", &loose_runner, Some(&safe_orb)),
    ];
    for (name, path, orb) in cases {
        let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-host"));
        command
            .arg("--state")
            .arg(root.join("state"))
            .args(["daemon", "run"])
            .env(name, path)
            .stdin(Stdio::null());
        if let Some(orb) = orb {
            command.env("VELNOR_ORB", orb);
        }
        let output = command.output()?;
        assert!(!output.status.success(), "{name}");
        let message = String::from_utf8(output.stderr)?;
        assert!(message.contains(path.to_str().ok_or("utf8")?), "{message}");
    }
    Ok(())
}
