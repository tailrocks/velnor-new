//! Runtime checks for the exact emitted release-verifier step boundary.

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use velnor_actions_contract::{StepKind, StepRole};

use crate::{VERIFY_PATH, VERIFY_VERIFIER_SHELL, toolchain_env::CREDENTIAL_UNSET_VARS, yaml::Yaml};

use super::render_typed_step;

const VERIFY_RUN_PREFIX: &str = r#"function_env_unset=(); function_env_scan_complete=0; while IFS= read -r -d '' env_entry; do case "$env_entry" in BASH_FUNC_*=*) function_env_unset+=(-u "${env_entry%%=*}");; VELNOR_VERIFIER_ENV_SCAN_COMPLETE) function_env_scan_complete=$((function_env_scan_complete + 1));; esac; done < <({ /usr/bin/env -0 && printf '%s\000' 'VELNOR_VERIFIER_ENV_SCAN_COMPLETE'; }); [ "$function_env_scan_complete" -eq 1 ] || exit 1; exec /usr/bin/env "${function_env_unset[@]}" -u BASH_ENV -u ENV -u ACTIONS_ID_TOKEN_REQUEST_TOKEN -u ACTIONS_ID_TOKEN_REQUEST_URL -u ACTIONS_RUNTIME_TOKEN -u GITHUB_TOKEN -u MISE_GITHUB_TOKEN -u GH_TOKEN -u GH_HOST -u GH_CONFIG_DIR /bin/bash --noprofile --norc -p -e -o pipefail -c 'unset ACTIONS_ID_TOKEN_REQUEST_TOKEN ACTIONS_ID_TOKEN_REQUEST_URL ACTIONS_RUNTIME_TOKEN GITHUB_TOKEN MISE_GITHUB_TOKEN GH_TOKEN GH_HOST GH_CONFIG_DIR; "#;

pub(crate) struct Scratch(PathBuf);

impl Scratch {
    pub(crate) fn new() -> Result<Self, Box<dyn Error>> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = std::env::temp_dir().join(format!(
            "velnor-verifier-shell-{}-{nonce}",
            std::process::id()
        ));
        fs::DirBuilder::new().mode(0o700).create(&path)?;
        Ok(Self(path))
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.0));
    }
}

fn yaml_field<'a>(entries: &'a [(String, Yaml)], key: &str) -> &'a Yaml {
    entries
        .iter()
        .find_map(|(name, value)| (name == key).then_some(value))
        .unwrap_or_else(|| panic!("missing {key}"))
}

pub(crate) fn str_field<'a>(entries: &'a [(String, Yaml)], key: &str) -> &'a str {
    match yaml_field(entries, key) {
        Yaml::Str(value) | Yaml::Annotated { value, .. } => value,
        other => panic!("{key} is not a string: {other:?}"),
    }
}

fn write_executable(path: &std::path::Path, contents: &str) -> std::io::Result<()> {
    fs::write(path, contents)?;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)
}

fn rendered_probe(role: StepRole, script: &str) -> Yaml {
    let step = if role == StepRole::VerifyJactionlint {
        crate::steps::jactionlint_verifier_step(
            "Verify jactionlint",
            vec!["sh".to_owned(), "-c".to_owned(), script.to_owned()],
            BTreeMap::new(),
        )
        .expect("typed Jactionlint verifier step")
    } else {
        let mut step = crate::steps::shell_step(
            crate::VERIFY_RUST_TOOLCHAIN_STEP,
            vec!["sh".to_owned(), "-c".to_owned(), script.to_owned()],
            BTreeMap::new(),
        )
        .expect("typed Rust verifier step");
        step.role = Some(role);
        step
    };
    let StepKind::Shell { run, env } = &step.kind else {
        panic!("shell step");
    };
    render_typed_step(&step, run, env, &BTreeMap::new(), false)
        .expect("verifier render")
        .expect("recognized verifier")
}

pub(crate) fn step_entries(step: &Yaml) -> &[(String, Yaml)] {
    let Yaml::Map(entries) = step else {
        panic!("step mapping");
    };
    entries
}

fn run_emitted_step(
    root: &std::path::Path,
    step: &Yaml,
    extra_env: &[(&str, &str)],
) -> Result<Output, Box<dyn Error>> {
    run_emitted_step_in(root, root, step, extra_env)
}

pub(crate) fn run_emitted_step_in(
    root: &std::path::Path,
    working_directory: &std::path::Path,
    step: &Yaml,
    extra_env: &[(&str, &str)],
) -> Result<Output, Box<dyn Error>> {
    let entries = step_entries(step);
    let script = str_field(entries, "run");
    let script_path = root.join("run.sh");
    fs::write(&script_path, script)?;

    let mut shell = str_field(entries, "shell").split_whitespace();
    let executable = shell.next().ok_or("missing verifier shell executable")?;
    let arguments = shell
        .map(|argument| {
            if argument == "{0}" {
                script_path.as_os_str().to_owned()
            } else {
                argument.into()
            }
        })
        .collect::<Vec<_>>();
    let mut command = Command::new(executable);
    command.args(arguments);
    command.current_dir(working_directory);
    command.env("PATH", root.join("poison-bin"));
    if let Yaml::Map(env) = yaml_field(entries, "env") {
        for (key, value) in env {
            let Yaml::Str(value) = value else {
                panic!("env {key} is not a string");
            };
            command.env(key, value);
        }
    } else {
        panic!("step env mapping");
    }
    for (key, value) in extra_env {
        command.env(key, value);
    }
    command.env("RUNNER_TEMP", root);
    Ok(command.output()?)
}

pub(crate) fn hostile_startup_fixture(
    root: &std::path::Path,
) -> Result<(std::path::PathBuf, std::path::PathBuf), Box<dyn Error>> {
    let marker = root.join("startup-ran");
    let bash_env = root.join("hostile-bash-env");
    fs::write(
        &bash_env,
        format!(
            "printf '%s\\n' startup >> '{marker}'\nenv() {{ printf '%s\\n' env-function >> '{marker}'; }}\nsh() {{ printf '%s\\n' sh-function >> '{marker}'; }}\nexport -f env sh\n",
            marker = marker.display()
        ),
    )?;
    let positive_control = Command::new("/bin/bash")
        .args(["--noprofile", "--norc", "-c", "env; sh -c ':'"])
        .env("BASH_ENV", &bash_env)
        .output()?;
    assert!(positive_control.status.success());
    assert_eq!(
        fs::read_to_string(&marker)?,
        "startup\nenv-function\nsh-function\n",
        "fixture must demonstrate startup code and inherited functions without privileged mode"
    );
    fs::remove_file(&marker)?;
    assert_exported_functions_are_imported(root)?;
    Ok((marker, bash_env))
}

fn assert_exported_functions_are_imported(root: &Path) -> Result<(), Box<dyn Error>> {
    let marker = root.join("functions-imported");
    let function_env = r#"() { printf '%s\n' env-function >> "$VERIFIER_MARKER"; }"#;
    let function_sh = r#"() { printf '%s\n' sh-function >> "$VERIFIER_MARKER"; }"#;
    let output = Command::new("/bin/bash")
        .args(["--noprofile", "--norc", "-c", "env; sh -c ':'"])
        .env("PATH", VERIFY_PATH)
        .env("VERIFIER_MARKER", &marker)
        .env("BASH_FUNC_env%%", function_env)
        .env("BASH_FUNC_sh%%", function_sh)
        .env_remove("BASH_ENV")
        .output()?;
    assert!(output.status.success());
    assert_eq!(
        fs::read_to_string(marker)?,
        "env-function\nsh-function\n",
        "positive control must import the exact hostile BASH_FUNC environment variables"
    );
    Ok(())
}

pub(crate) fn poison_path(
    root: &std::path::Path,
    marker: &std::path::Path,
) -> Result<std::path::PathBuf, Box<dyn Error>> {
    let poison_bin = root.join("poison-bin");
    fs::create_dir(&poison_bin)?;
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
        let poisoned_command = format!(
            "#!/bin/sh\nprintf '%s\\n' 'poison-{name}' >> '{}'\nexit 91\n",
            marker.display()
        );
        write_executable(&poison_bin.join(name), &poisoned_command)?;
    }
    Ok(poison_bin)
}

pub(crate) fn assert_emitted_boundary(step: &Yaml) {
    assert_emitted_boundary_with_extra_env(step, &[]);
}

pub(crate) fn assert_emitted_boundary_with_extra_env(step: &Yaml, extras: &[(&str, &str)]) {
    let entries = step_entries(step);
    assert_eq!(VERIFY_PATH, "/usr/bin:/bin:/usr/sbin:/sbin");
    assert_eq!(
        str_field(entries, "shell"),
        "/bin/bash --noprofile --norc -p -e -o pipefail {0}"
    );
    assert_eq!(str_field(entries, "shell"), VERIFY_VERIFIER_SHELL);
    assert!(str_field(entries, "run").starts_with(VERIFY_RUN_PREFIX));
    let Yaml::Map(env) = yaml_field(entries, "env") else {
        panic!("env mapping");
    };
    let mut expected = crate::toolchain_env::credential_scrub();
    expected.insert("PATH".to_owned(), VERIFY_PATH.to_owned());
    for (key, value) in extras {
        assert!(
            expected
                .insert((*key).to_owned(), (*value).to_owned())
                .is_none()
        );
    }
    assert_eq!(env.len(), expected.len(), "exact verifier environment");
    for (key, value) in expected {
        assert_eq!(
            env.iter()
                .find(|(name, _)| name == &key)
                .map(|(_, value)| value),
            Some(&Yaml::str(value)),
            "environment key {key}"
        );
    }
    assert_eq!(crate::toolchain_env::credential_scrub().len(), 11);
}

fn assert_verifier_output(
    root: &std::path::Path,
    marker: &std::path::Path,
    output: Output,
) -> Result<(), Box<dyn Error>> {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout)?, "verifier-ok\n");
    assert!(!marker.exists(), "hostile shell startup or function ran");
    assert_eq!(
        fs::read_to_string(root.join("nested-shell"))?,
        "nested-shell-ok\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("observed-path"))?,
        format!("{VERIFY_PATH}\n")
    );
    let resolved_sh = fs::read_to_string(root.join("observed-sh"))?;
    assert!(resolved_sh.starts_with("/usr/bin/") || resolved_sh.starts_with("/bin/"));
    let child_env = fs::read_to_string(root.join("observed-env"))?;
    for name in CREDENTIAL_UNSET_VARS {
        assert!(
            !child_env
                .lines()
                .any(|line| line.starts_with(&format!("{name}="))),
            "{name} crossed into the verifier body"
        );
    }
    for name in ["CARGO_REGISTRY_TOKEN", "NPM_TOKEN", "NODE_AUTH_TOKEN"] {
        assert!(
            child_env.lines().any(|line| line == format!("{name}=")),
            "the remaining scrub keys must be present only as empty values"
        );
    }
    Ok(())
}

#[test]
fn release_verifier_command_uses_the_same_startup_safe_boundary() {
    let script = crate::selected_mise_verifier::test_script("2.3.4");
    let step = crate::verifier_shell::render_command(
        "Verify jactionlint",
        &["sh".to_owned(), "-c".to_owned(), script],
    )
    .expect("release verifier renders");
    assert_emitted_boundary(&step);
}

#[test]
fn emitted_verifier_roles_ignore_hostile_startup_and_search_path_inputs()
-> Result<(), Box<dyn Error>> {
    let root = Scratch::new()?;
    let (marker, bash_env) = hostile_startup_fixture(root.path())?;
    poison_path(root.path(), &marker)?;
    let mise_data = root.path().join("selected-mise");
    fs::create_dir_all(mise_data.join("bin"))?;
    write_executable(&mise_data.join("bin/mise"), "#!/bin/sh\nexit 0\n")?;
    let mise_data = mise_data.to_str().expect("Mise data path").to_owned();
    let script = format!(
        "test \"$PATH\" = \"{VERIFY_PATH}\"; printf '%s\\n' \"$PATH\" > \"$RUNNER_TEMP/observed-path\"; command -v sh > \"$RUNNER_TEMP/observed-sh\"; env > \"$RUNNER_TEMP/observed-env\"; sh -c 'printf \"%s\\n\" nested-shell-ok > \"$RUNNER_TEMP/nested-shell\"'; printf '%s\\n' verifier-ok"
    );
    let hostile_function = format!("() {{ touch '{}'; }}", marker.display());
    let hostile_env = [
        ("BASH_ENV", bash_env.to_str().expect("BASH_ENV path")),
        ("BASH_FUNC_env%%", &hostile_function),
        ("BASH_FUNC_sh%%", &hostile_function),
        ("GITHUB_TOKEN", "runner-overrode-empty-step-env"),
        (
            "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
            "runner-overrode-empty-step-env",
        ),
        (
            "ACTIONS_ID_TOKEN_REQUEST_URL",
            "runner-overrode-empty-step-env",
        ),
        ("ACTIONS_RUNTIME_TOKEN", "runner-overrode-empty-step-env"),
        ("MISE_GITHUB_TOKEN", "runner-overrode-empty-step-env"),
        ("GH_TOKEN", "runner-overrode-empty-step-env"),
        ("GH_HOST", "runner-overrode-empty-step-env"),
        ("GH_CONFIG_DIR", "runner-overrode-empty-step-env"),
        ("MISE_DATA_DIR", &mise_data),
    ];
    let step = rendered_probe(StepRole::VerifyRustManifest, &script);
    assert_emitted_boundary(&step);
    let output = run_emitted_step(root.path(), &step, &hostile_env)?;
    assert_verifier_output(root.path(), &marker, output)?;
    Ok(())
}

#[path = "verifier_shell_jactionlint_runtime_tests.rs"]
mod jactionlint_runtime;

#[path = "verifier_shell_envp_tests.rs"]
mod envp;
