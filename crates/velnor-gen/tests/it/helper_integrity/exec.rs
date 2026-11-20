// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

use crate::io::Scratch;

use super::DEFAULT_SHELL;

const GOOD: &str = "#!/bin/sh\nprintf 'released-helper\\n'\n";
pub(super) const BAD: &str = "#!/bin/sh\nprintf 'replaced-helper\\n'\n";

pub(super) fn execute(
    step: &Value,
    root: &Path,
    shadows: &Path,
    digest: &str,
) -> Result<Output, Box<dyn Error>> {
    execute_environment(step, root, shadows, digest, &[])
}

pub(crate) fn execute_environment(
    step: &Value,
    root: &Path,
    shadows: &Path,
    digest: &str,
    environment: &[(&str, &str)],
) -> Result<Output, Box<dyn Error>> {
    let script = root.join("invocation.sh");
    fs::write(&script, step["run"].as_str().ok_or("no run")?)?;
    let env_file = root.join("helper-tools.env");
    let mut bindings: BTreeMap<String, String> = fs::read_to_string(&env_file)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect();
    if step["run"]
        .as_str()
        .is_some_and(|run| run.contains("$VELNOR_EXEC"))
    {
        bindings.insert("VELNOR_HELPER_SHA256".to_owned(), digest.to_owned());
    }
    bindings.extend(
        environment
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned())),
    );
    let shell = step["shell"].as_str().unwrap_or(DEFAULT_SHELL);
    let mut words = shell.split_whitespace();
    let program = words.next().ok_or("no shell executable")?;
    let mut command = Command::new(if program == "bash" {
        "/bin/bash"
    } else {
        program
    });
    for word in words {
        command.arg(if word == "{0}" {
            script.as_os_str()
        } else {
            word.as_ref()
        });
    }
    let path = if step["env"]["VELNOR_HELPER_INSTALL_TOOLS"].is_string() {
        "/usr/bin:/bin".to_owned()
    } else {
        format!("{}:/usr/bin:/bin", shadows.display())
    };
    command
        .env_clear()
        .env("PATH", path)
        .env("GITHUB_ENV", env_file)
        .env("RUNNER_TEMP", root)
        .env("GITHUB_RUN_ID", "42")
        .env("GITHUB_RUN_ATTEMPT", "1")
        .env("GITHUB_JOB", "format")
        .envs(&bindings);
    if let Some(env) = step["env"].as_object() {
        for (key, value) in env {
            let text = value.as_str().ok_or("non-string environment")?;
            let text = if key == "VELNOR_HELPER_SHA256" {
                digest.to_owned()
            } else if let Some(name) = text
                .strip_prefix("${{ env.")
                .and_then(|text| text.strip_suffix(" }}"))
            {
                bindings
                    .get(name)
                    .ok_or("missing installed tool binding")?
                    .clone()
            } else {
                text.replace("${{ runner.temp }}", root.to_str().ok_or("non-UTF8 root")?)
                    .replace("${{ github.run_id }}", "42")
                    .replace("${{ github.run_attempt }}", "1")
                    .replace("${{ github.job }}", "format")
            };
            command.env(key, text);
        }
    }
    Ok(command.output()?)
}

pub(crate) fn fixture() -> Result<(Scratch, String), Box<dyn Error>> {
    let scratch = Scratch::new("helper-integrity")?;
    let root = scratch.outside();
    let source = root.join("velnor-ci-bin/velnor-ci");
    fs::create_dir_all(source.parent().ok_or("no source parent")?)?;
    fs::write(&source, GOOD)?;
    fs::set_permissions(&source, fs::Permissions::from_mode(0o700))?;
    let hash = Command::new("/usr/bin/shasum")
        .args(["-a", "256"])
        .arg(&source)
        .output()?;
    assert!(hash.status.success());
    let digest = String::from_utf8(hash.stdout)?
        .split_whitespace()
        .next()
        .ok_or("no digest")?
        .to_owned();
    let shadows = root.join("shadows");
    fs::create_dir(&shadows)?;
    for program in ["velnor-ci", "sha256sum", "shasum"] {
        let file = shadows.join(program);
        fs::write(&file, BAD)?;
        fs::set_permissions(&file, fs::Permissions::from_mode(0o700))?;
    }
    Ok((scratch, digest))
}
