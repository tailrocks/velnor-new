// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::path::Path;
use std::process::{Command, Output};

fn host(cwd: &Path, home: Option<&str>, args: &[&str]) -> std::io::Result<Output> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-host"));
    command.current_dir(cwd).args(args).env_remove("HOME");
    if let Some(home) = home {
        command.env("HOME", home);
    }
    command.output()
}

fn entries(dir: &Path) -> std::io::Result<Vec<String>> {
    std::fs::read_dir(dir)?
        .map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().into_owned()))
        .collect()
}

#[test]
fn unresolvable_home_never_places_state_or_service_files_under_the_cwd()
-> Result<(), Box<dyn Error>> {
    for home in [None, Some(""), Some("relative/home")] {
        let cwd = tempfile::tempdir()?;
        let absolute_state = tempfile::tempdir()?;
        let state = absolute_state
            .path()
            .to_str()
            .ok_or("state path is not UTF-8")?;
        for args in [
            vec!["drain"],
            vec!["service", "install"],
            vec!["--state", state, "service", "install"],
            vec!["--state", state, "service", "start"],
            vec!["logs"],
        ] {
            let output = host(cwd.path(), home, &args)?;
            assert!(
                !output.status.success(),
                "{args:?} with HOME={home:?} exited 0"
            );
            assert_eq!(
                entries(cwd.path())?,
                Vec::<String>::new(),
                "{args:?} with HOME={home:?} wrote into the cwd"
            );
        }
        assert_eq!(
            entries(absolute_state.path())?,
            Vec::<String>::new(),
            "service install with HOME={home:?} wrote into the explicit state"
        );
    }
    Ok(())
}

#[test]
fn absolute_home_places_state_under_application_support() -> Result<(), Box<dyn Error>> {
    let cwd = tempfile::tempdir()?;
    let home = tempfile::tempdir()?;
    let home_text = home.path().to_str().ok_or("home path is not UTF-8")?;
    let output = host(cwd.path(), Some(home_text), &["drain"])?;
    assert!(output.status.success());
    assert!(
        home.path()
            .join("Library/Application Support/Velnor/drain")
            .exists()
    );
    assert_eq!(entries(cwd.path())?, Vec::<String>::new());
    Ok(())
}
