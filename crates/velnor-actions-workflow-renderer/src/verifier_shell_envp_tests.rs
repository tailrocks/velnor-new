use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use velnor_actions_contract::StepRole;

use crate::yaml::Yaml;

use super::{
    Scratch, assert_emitted_boundary, assert_verifier_output, hostile_startup_fixture, poison_path,
    rendered_probe, run_emitted_step, step_entries, str_field,
};

type TestResult = Result<(), Box<dyn Error>>;
const RAW_EXECVE_HELPER: &str = r#"
import ctypes, os, sys
args = sys.argv[1:]
separator = args.index("--env")
argv = [os.fsencode(value) for value in args[:separator]]
envp = [os.fsencode(value) for value in args[separator + 1:]]
argv_array = (ctypes.c_char_p * (len(argv) + 1))(*argv, None)
envp_array = (ctypes.c_char_p * (len(envp) + 1))(*envp, None)
libc = ctypes.CDLL(None, use_errno=True)
libc.execve.argtypes = [ctypes.c_char_p, ctypes.POINTER(ctypes.c_char_p), ctypes.POINTER(ctypes.c_char_p)]
libc.execve(argv[0], argv_array, envp_array)
error = ctypes.get_errno()
raise OSError(error, os.strerror(error))
"#;

fn available_bash_paths() -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let candidates = [
        "/bin/bash",
        "/usr/bin/bash",
        "/opt/homebrew/bin/bash",
        "/usr/local/bin/bash",
    ];
    let mut paths = Vec::new();
    for candidate in candidates {
        if !Path::new(candidate).is_file() {
            continue;
        }
        let candidate = fs::canonicalize(candidate)?;
        if paths.contains(&candidate) {
            continue;
        }
        if Command::new(&candidate)
            .arg("--version")
            .output()?
            .status
            .success()
        {
            paths.push(candidate);
        }
    }
    assert!(!paths.is_empty(), "a supported Bash executable is required");
    Ok(paths)
}

fn step_with_outer_bash(step: &Yaml, executable: &Path) -> Yaml {
    let mut step = step.clone();
    let Yaml::Map(fields) = &mut step else {
        panic!("verifier step mapping");
    };
    let shell = fields
        .iter_mut()
        .find_map(|(name, value)| (name == "shell").then_some(value))
        .expect("verifier shell field");
    *shell = Yaml::str(format!(
        "{} --noprofile --norc -p -e -o pipefail {{0}}",
        executable.display()
    ));
    step
}

fn emitted_argv(root: &Path, step: &Yaml) -> Vec<String> {
    let mut arguments = str_field(step_entries(step), "shell")
        .split_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    for argument in &mut arguments {
        if argument == "{0}" {
            *argument = root.join("run.sh").to_string_lossy().into_owned();
        }
    }
    arguments
}

fn emitted_env(root: &Path, step: &Yaml, additions: &[(&str, &str)]) -> BTreeMap<String, String> {
    let mut environment = BTreeMap::new();
    let Yaml::Map(entries) = step_entries(step)
        .iter()
        .find_map(|(name, value)| (name == "env").then_some(value))
        .expect("verifier env field")
    else {
        panic!("verifier env mapping");
    };
    for (key, value) in entries {
        let Yaml::Str(value) = value else {
            panic!("env {key} is not a string");
        };
        environment.insert(key.clone(), value.clone());
    }
    environment.insert(
        "RUNNER_TEMP".to_owned(),
        root.to_string_lossy().into_owned(),
    );
    for (key, value) in additions {
        environment.insert((*key).to_owned(), (*value).to_owned());
    }
    environment
}

fn run_with_raw_envp(
    root: &Path,
    step: &Yaml,
    additions: &[(&str, &str)],
    bare_entries: &[&str],
) -> Result<Output, Box<dyn Error>> {
    fs::write(root.join("run.sh"), str_field(step_entries(step), "run"))?;
    let arguments = emitted_argv(root, step);
    let environment = emitted_env(root, step, additions);
    let mut command = Command::new("python3");
    command.args(["-c", RAW_EXECVE_HELPER]);
    command.args(arguments);
    command.arg("--env");
    command.args(
        environment
            .iter()
            .map(|(key, value)| format!("{key}={value}")),
    );
    command.args(bare_entries);
    command.env_clear();
    command.env("PATH", "/usr/bin:/bin:/opt/homebrew/bin:/usr/local/bin");
    command.current_dir(root);
    Ok(command.output()?)
}

fn hostile_environment(root: &Path, bash_env: &Path, marker: &Path) -> Vec<(&'static str, String)> {
    let path = |value: &Path| value.to_string_lossy().into_owned();
    let env_function = r#"() { printf '%s\n' function-env >> "$POISON_LOG"; }"#;
    let sh_function = r#"() { printf '%s\n' function-sh >> "$POISON_LOG"; }"#;
    vec![
        ("BASH_ENV", path(bash_env)),
        ("ENV", path(bash_env)),
        ("BASH_FUNC_env%%", env_function.to_owned()),
        ("BASH_FUNC_sh%%", sh_function.to_owned()),
        ("POISON_LOG", path(marker)),
        (
            "VELNOR_VERIFIER_ENV_SCAN_COMPLETE",
            "VELNOR_VERIFIER_ENV_SCAN_COMPLETE".to_owned(),
        ),
        ("GITHUB_TOKEN", "outer-token-canary".to_owned()),
        (
            "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
            "outer-oidc-token-canary".to_owned(),
        ),
        (
            "ACTIONS_ID_TOKEN_REQUEST_URL",
            "outer-oidc-url-canary".to_owned(),
        ),
        (
            "ACTIONS_RUNTIME_TOKEN",
            "outer-runtime-token-canary".to_owned(),
        ),
        ("MISE_GITHUB_TOKEN", "outer-mise-token-canary".to_owned()),
        ("GH_TOKEN", "outer-gh-token-canary".to_owned()),
        ("GH_HOST", "outer-gh-host-canary".to_owned()),
        ("GH_CONFIG_DIR", "outer-gh-config-canary".to_owned()),
        ("GITHUB_ENV", path(&root.join("github-env"))),
    ]
}

#[test]
fn privileged_shell_normalizes_raw_envp_and_removes_hidden_functions() -> TestResult {
    let script = format!(
        r#"test "$PATH" = "{path}"; printf '%s\n' "$PATH" > "$RUNNER_TEMP/observed-path"; command -v sh > "$RUNNER_TEMP/observed-sh"; env > "$RUNNER_TEMP/observed-env"; sh -c 'printf "%s\n" nested-shell-ok > "$RUNNER_TEMP/nested-shell"'; printf '%s\n' verifier-ok"#,
        path = crate::VERIFY_PATH
    );
    let base_step = rendered_probe(StepRole::VerifyRustManifest, &script);
    assert_emitted_boundary(&base_step);
    for executable in available_bash_paths()? {
        let root = Scratch::new()?;
        let (marker, bash_env) = hostile_startup_fixture(root.path())?;
        poison_path(root.path(), &marker)?;
        let step = step_with_outer_bash(&base_step, &executable);
        let additions = hostile_environment(root.path(), &bash_env, &marker);
        let additions = additions
            .iter()
            .map(|(key, value)| (*key, value.as_str()))
            .collect::<Vec<_>>();
        let output = run_with_raw_envp(
            root.path(),
            &step,
            &additions,
            &["VELNOR_VERIFIER_ENV_SCAN_COMPLETE"],
        )?;
        assert_verifier_output(root.path(), &marker, output)?;
        let child_env = fs::read_to_string(root.path().join("observed-env"))?;
        assert!(child_env.lines().any(|line| {
            line == "VELNOR_VERIFIER_ENV_SCAN_COMPLETE=VELNOR_VERIFIER_ENV_SCAN_COMPLETE"
        }));
    }
    Ok(())
}

fn replace_run(step: &Yaml, from: &str, to: &str) -> Yaml {
    let mut step = step.clone();
    let Yaml::Map(fields) = &mut step else {
        panic!("verifier step mapping");
    };
    let run = fields
        .iter_mut()
        .find_map(|(name, value)| (name == "run").then_some(value))
        .expect("verifier run field");
    let Yaml::Str(run) = run else {
        panic!("verifier run scalar");
    };
    assert_eq!(run.matches(from).count(), 1, "unique scanner producer");
    *run = run.replace(from, to);
    step
}

#[test]
fn missing_env_scan_completion_stops_before_verifier_body() -> TestResult {
    let root = Scratch::new()?;
    let body_marker = root.path().join("body-ran");
    let script = r#"printf body > "$RUNNER_TEMP/body-ran""#;
    let step = rendered_probe(StepRole::VerifyRustManifest, script);
    assert_emitted_boundary(&step);
    let broken = replace_run(&step, "/usr/bin/env -0", "/usr/bin/false");
    let output = run_emitted_step(root.path(), &broken, &[])?;
    assert!(
        !output.status.success(),
        "a failed environment scan must fail closed"
    );
    assert!(
        !body_marker.exists(),
        "verifier body ran without a scan sentinel"
    );
    Ok(())
}
