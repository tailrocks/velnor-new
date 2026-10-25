//! Execute the generated project-tool resolver workflow step under hostile
//! runner startup, PATH, and credential inputs.

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use velnor_actions_mise::RUST_VERSION;
use velnor_actions_workflow_renderer::RenderedTree;
use velnor_actions_workflow_renderer::{VERIFY_PATH, VERIFY_VERIFIER_SHELL};

use super::{ResolverStep, rendered_tree, resolver_steps};
use crate::impl_common::TestResult;

#[path = "impl_tools_identity_security_generated.rs"]
mod generated;
#[path = "impl_tools_identity_security_shape.rs"]
mod shape;

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Result<Self, Box<dyn Error>> {
        let path = tempfile::tempdir()?.keep();
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.0));
    }
}

pub(super) fn generated_resolvers(tree: &RenderedTree) -> Vec<(String, ResolverStep)> {
    tree.files
        .iter()
        .flat_map(|file| {
            resolver_steps(&file.bytes)
                .into_iter()
                .map(|step| (file.path.clone(), step))
                .collect::<Vec<_>>()
        })
        .collect()
}

fn write_executable(path: &Path, body: &str) -> Result<(), Box<dyn Error>> {
    fs::write(path, body)?;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

fn poison_path(root: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let bin = root.join("poison-bin");
    fs::create_dir(&bin)?;
    for name in [
        "awk",
        "bash",
        "cp",
        "cut",
        "env",
        "grep",
        "mkdir",
        "mise",
        "rm",
        "sha256sum",
        "sh",
        "shasum",
        "tr",
    ] {
        write_executable(
            &bin.join(name),
            &format!("#!/bin/sh\nprintf '%s\\n' '{name}' >> \"$POISON_LOG\"\nexit 97\n"),
        )?;
    }
    Ok(bin)
}

fn run_step(
    root: &Path,
    checkout: &Path,
    step: &ResolverStep,
    poison_bin: &Path,
    extra_env: &[(&str, &Path)],
) -> Result<Output, Box<dyn Error>> {
    assert_eq!(step.shell, VERIFY_VERIFIER_SHELL);
    let script = root.join("emitted-run.sh");
    fs::write(&script, &step.run)?;
    let mut words = step.shell.split_whitespace();
    let executable = words.next().ok_or("missing shell executable")?;
    let mut command = Command::new(executable);
    let mut has_script = false;
    for word in words {
        if word == "{0}" {
            command.arg(&script);
            has_script = true;
        } else {
            command.arg(word);
        }
    }
    assert!(has_script, "the emitted shell lost its script placeholder");
    let env_file = root.join("github-env");
    let temp = root.join("runner-temp");
    fs::create_dir_all(&temp)?;
    fs::write(&env_file, "")?;
    command
        .env_clear()
        .env("PATH", poison_bin)
        .current_dir(checkout)
        .env("GITHUB_ENV", &env_file)
        .env("RUNNER_TEMP", &temp)
        .env("RUNNER_OS", "Linux")
        .env("RUNNER_ARCH", "X64")
        .env("CI", "true")
        .env("POISON_LOG", root.join("poison.log"))
        .env("AUDIT", root.join("mise-audit"));
    for (key, value) in &step.env {
        command.env(key, value);
    }
    for (key, value) in extra_env {
        command.env(key, value);
    }
    Ok(command.output()?)
}

fn hostile_controls(root: &Path) -> Result<(PathBuf, PathBuf), Box<dyn Error>> {
    let marker = root.join("startup-marker");
    let bash_env = root.join("hostile-bash-env");
    fs::write(
        &bash_env,
        format!(
            "printf 'startup\\n' >> '{marker}'\nenv() {{ printf 'env-function\\n' >> '{marker}'; }}\nsh() {{ printf 'sh-function\\n' >> '{marker}'; }}\nexport -f env sh\n",
            marker = marker.display()
        ),
    )?;
    let output = Command::new("/bin/bash")
        .args(["--noprofile", "--norc", "-c", "env; sh -c ':'"])
        .env("PATH", VERIFY_PATH)
        .env("BASH_ENV", &bash_env)
        .output()?;
    assert!(output.status.success());
    assert_eq!(
        fs::read_to_string(&marker)?,
        "startup\nenv-function\nsh-function\n",
        "the BASH_ENV control must prove startup and function execution"
    );
    fs::remove_file(&marker)?;

    let function_marker = root.join("function-import-marker");
    let output = Command::new("/bin/bash")
        .args(["--noprofile", "--norc", "-c", "env; sh -c ':'"])
        .env("PATH", VERIFY_PATH)
        .env("VERIFIER_MARKER", &function_marker)
        .env(
            "BASH_FUNC_env%%",
            r#"() { printf '%s\n' env-function >> "$VERIFIER_MARKER"; }"#,
        )
        .env(
            "BASH_FUNC_sh%%",
            r#"() { printf '%s\n' sh-function >> "$VERIFIER_MARKER"; }"#,
        )
        .output()?;
    assert!(output.status.success());
    assert_eq!(
        fs::read_to_string(function_marker)?,
        "env-function\nsh-function\n",
        "the exact exported-function variables must be imported by the control"
    );
    Ok((marker, bash_env))
}

fn assert_outputs(root: &Path, expected: &[(&str, &str)]) -> Result<(), Box<dyn Error>> {
    let contents = fs::read_to_string(root.join("github-env"))?;
    let mut outputs = BTreeMap::new();
    for line in contents.lines() {
        let (name, value) = line.split_once('=').ok_or("malformed GITHUB_ENV entry")?;
        assert!(outputs.insert(name.to_owned(), value.to_owned()).is_none());
    }
    let expected = expected
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(
        outputs, expected,
        "GITHUB_ENV must contain exactly the fixture's tool and lock bindings"
    );
    Ok(())
}

fn assert_audit(root: &Path, selected: &Path) -> Result<(), Box<dyn Error>> {
    let audit = fs::read_to_string(root.join("mise-audit"))?;
    let selected = selected.canonicalize()?;
    let lines = audit.lines().collect::<Vec<_>>();
    let expected_mise = format!("mise={}/bin/mise", selected.display());
    let expected_data = format!("MISE_DATA_DIR={}", selected.display());
    let expected_path = format!("PATH={VERIFY_PATH}");
    let calls = lines
        .iter()
        .filter(|line| line.starts_with("mise="))
        .collect::<Vec<_>>();
    assert!(!calls.is_empty(), "the resolver did not invoke Mise");
    assert!(calls.iter().all(|line| **line == expected_mise));
    assert!(
        lines
            .iter()
            .filter(|line| line.starts_with("MISE_DATA_DIR="))
            .all(|line| **line == expected_data)
    );
    assert!(
        lines
            .iter()
            .filter(|line| line.starts_with("PATH="))
            .all(|line| **line == expected_path)
    );
    for name in [
        "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
        "ACTIONS_ID_TOKEN_REQUEST_URL",
        "ACTIONS_RUNTIME_TOKEN",
        "GITHUB_TOKEN",
        "MISE_GITHUB_TOKEN",
        "GH_TOKEN",
        "GH_HOST",
        "GH_CONFIG_DIR",
    ] {
        assert!(
            lines.contains(&format!("{name}=<unset>").as_str()),
            "{name}: {audit}"
        );
    }
    for name in ["CARGO_REGISTRY_TOKEN", "NPM_TOKEN", "NODE_AUTH_TOKEN"] {
        assert!(
            lines.contains(&format!("{name}=").as_str()),
            "{name}: {audit}"
        );
    }
    assert!(
        lines
            .iter()
            .filter(|line| line.starts_with("args="))
            .all(|line| line.starts_with("args=--no-config --no-env --no-hooks config get "))
    );
    Ok(())
}

pub(super) fn assert_step_envelope(
    tree: &RenderedTree,
    path: &str,
    step: &ResolverStep,
) -> Result<(), Box<dyn Error>> {
    let yaml = tree
        .get(path)
        .ok_or("generated resolver file disappeared")?;
    shape::assert_direct_resolver_step(yaml)?;
    assert_eq!(step.shell, VERIFY_VERIFIER_SHELL);
    assert_eq!(step.env.get("PATH").map(String::as_str), Some(VERIFY_PATH));
    assert_eq!(step.env.len(), 12, "trusted PATH plus eleven scrub values");
    assert_eq!(
        step.env.values().filter(|value| value.is_empty()).count(),
        11
    );
    Ok(())
}

#[test]
fn emitted_job_resolver_uses_trusted_mise_and_scrubs_credentials() -> TestResult {
    generated::assert_generated_workflow_resolver()?;
    Ok(())
}

#[test]
fn emitted_shared_lane_composite_resolver_uses_trusted_mise_and_scrubs_credentials() -> TestResult {
    generated::assert_generated_shared_lane_composite_resolver()?;
    Ok(())
}
