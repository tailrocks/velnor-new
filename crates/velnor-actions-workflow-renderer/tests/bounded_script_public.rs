// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{fixed_python_authority::bound_workflow, yaml::Yaml};
use std::{error::Error, fs, process::Command};

type TestResult = Result<(), Box<dyn Error>>;

fn workflow(run: &str, runner: &str, shell: Option<&str>, extra: Vec<(String, Yaml)>) -> Yaml {
    let mut step = vec![
        ("name".into(), Yaml::str("Original")),
        ("run".into(), Yaml::str(run)),
    ];
    if let Some(shell) = shell {
        step.push(("shell".into(), Yaml::str(shell)));
    }
    step.extend(extra);
    Yaml::Map(vec![(
        "jobs".into(),
        Yaml::Map(vec![(
            "job".into(),
            Yaml::Map(vec![
                ("runs-on".into(), Yaml::str(runner)),
                ("steps".into(), Yaml::Seq(vec![Yaml::Map(step)])),
            ]),
        )]),
    )])
}

fn steps(document: &Yaml) -> Result<&[Yaml], Box<dyn Error>> {
    let Yaml::Map(root) = document else {
        return Err("root".into());
    };
    let Yaml::Map(jobs) = &root[0].1 else {
        return Err("jobs".into());
    };
    let Yaml::Map(job) = &jobs[0].1 else {
        return Err("job".into());
    };
    let Yaml::Seq(steps) = &job[1].1 else {
        return Err("steps".into());
    };
    Ok(steps)
}

fn field<'a>(step: &'a Yaml, key: &str) -> Option<&'a Yaml> {
    let Yaml::Map(fields) = step else {
        return None;
    };
    fields
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value)
}

fn run(step: &Yaml) -> Result<&str, Box<dyn Error>> {
    let Some(Yaml::Str(run)) = field(step, "run") else {
        return Err("run".into());
    };
    Ok(run)
}

#[test]
fn long_scripts_preserve_shell_exit_and_step_fields() -> TestResult {
    for shell in [None, Some("bash")] {
        let source = format!(
            "{}\nprintf '%s' \"$VALUE\"\nfalse | true\nprintf ':after'\n",
            " ".repeat(21_001)
        );
        let attributes = vec![
            ("id".into(), Yaml::str("keep")),
            ("if".into(), Yaml::str("always()")),
            ("working-directory".into(), Yaml::str("/tmp")),
            ("continue-on-error".into(), Yaml::Bool(true)),
            ("timeout-minutes".into(), Yaml::Int(4)),
            (
                "env".into(),
                Yaml::Map(vec![("VALUE".into(), Yaml::str("hello"))]),
            ),
        ];
        let mut document = workflow(&source, "ubuntu-26.04", shell, attributes.clone());
        bound_workflow(&mut document)?;
        let steps = steps(&document)?;
        let last = steps.last().ok_or("last")?;
        for (key, value) in attributes {
            assert_eq!(field(last, &key), Some(&value));
        }
        assert!(
            steps[..steps.len() - 1]
                .iter()
                .all(|step| field(step, "if") == Some(&Yaml::str("always()")))
        );
        let temp = tempfile::tempdir()?;
        let env_file = temp.path().join("env");
        fs::write(&env_file, "")?;
        let mut directory = String::new();
        for step in &steps[..steps.len() - 1] {
            let output = Command::new("/bin/bash")
                .args(["-e", "-o", "pipefail", "-c", run(step)?])
                .env("RUNNER_TEMP", temp.path())
                .env("GITHUB_ENV", &env_file)
                .env("VELNOR_FIXED_SCRIPT_DIR", &directory)
                .output()?;
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            if let Some(value) = fs::read_to_string(&env_file)?
                .lines()
                .find_map(|line| line.strip_prefix("VELNOR_FIXED_SCRIPT_DIR="))
            {
                directory = value.to_owned();
            }
        }
        assert_eq!(
            fs::read(format!("{directory}/script.sh"))?,
            source.as_bytes()
        );
        let invoke = |script: &str| -> Result<std::process::Output, std::io::Error> {
            let mut command = Command::new("/bin/bash");
            command.arg("-e");
            if shell.is_some() {
                command.args(["-o", "pipefail"]);
            }
            command
                .args(["-c", script])
                .env("VALUE", "hello")
                .env("VELNOR_FIXED_SCRIPT_DIR", &directory)
                .output()
        };
        let before = invoke(&source)?;
        let after = invoke(run(last)?)?;
        assert_eq!(before.status.code(), after.status.code());
        assert_eq!(before.stdout, after.stdout);
    }
    Ok(())
}

#[test]
fn unsupported_oversized_scripts_are_rejected() {
    for (source, runner, shell, expected) in [
        (
            format!("{}${{{{ github.sha }}}}", " ".repeat(21_001)),
            "ubuntu-26.04",
            None,
            "oversized_run_contains_expression",
        ),
        (
            " ".repeat(21_001),
            "ubuntu-26.04",
            Some("python"),
            "oversized_run_unsupported_shell",
        ),
        (
            " ".repeat(21_001),
            "windows-2025",
            None,
            "oversized_run_unsupported_runner",
        ),
    ] {
        let error =
            bound_workflow(&mut workflow(&source, runner, shell, vec![])).expect_err("must reject");
        assert!(error.to_string().contains(expected));
    }
}

#[path = "bounded_script_adversarial.rs"]
mod adversarial;
