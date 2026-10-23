//! Real-shell execution of emitted tools-cache identity resolvers.

#[cfg(unix)]
use std::{collections::BTreeSet, fs, process::Command};

#[cfg(unix)]
use super::{ResolverStep, TestResult, base_lock, base_toml, rendered_tree};

#[cfg(unix)]
struct ResolverRuntime {
    root: std::path::PathBuf,
    home: std::path::PathBuf,
    workspace: std::path::PathBuf,
    runner_temp: std::path::PathBuf,
    selected_data: std::path::PathBuf,
    selected_input: std::path::PathBuf,
    xdg_data_home: std::path::PathBuf,
    poison: std::path::PathBuf,
    audit: std::path::PathBuf,
    poison_log: std::path::PathBuf,
}

#[cfg(unix)]
fn resolver_runtime(root: &std::path::Path) -> Result<ResolverRuntime, Box<dyn std::error::Error>> {
    let root = root.to_path_buf();
    let home = root.join("home");
    let workspace = root.join("workspace");
    let runner_temp = root.join("runner-temp");
    let physical_parent = root.join("physical");
    let alias_parent = root.join("alias");
    let selected_data = physical_parent.join("mise-data");
    let selected_input = alias_parent.join("mise-data");
    let mise_dir = selected_data.join("bin");
    let xdg_data_home = root.join("xdg-decoy");
    let poison = root.join("poison");
    fs::create_dir_all(&mise_dir)?;
    fs::create_dir_all(&workspace)?;
    fs::create_dir_all(&runner_temp)?;
    fs::create_dir_all(&poison)?;
    fs::create_dir_all(&physical_parent)?;
    std::os::unix::fs::symlink(&physical_parent, &alias_parent)?;
    fs::write(workspace.join("mise.toml"), base_toml())?;
    fs::write(workspace.join("mise.lock"), base_lock())?;

    let real_mise = executable_on_path("mise")?;
    let audit = root.join("mise-audit.log");
    write_executable(
        &mise_dir.join("mise"),
        &mise_audit_wrapper(&real_mise, &audit),
    )?;
    let poison_log = root.join("poison.log");
    let helpers = [
        "awk",
        "cp",
        "cut",
        "grep",
        "mkdir",
        "rm",
        "sha256sum",
        "shasum",
        "tr",
        "mise",
    ];
    for name in helpers {
        let script = format!(
            "#!/bin/sh\nprintf '%s\\n' '{}' >> {}\nexit 91\n",
            name,
            shell_quote(&poison_log.display().to_string())
        );
        write_executable(&poison.join(name), &script)?;
    }
    let poisoned_mise = fs::read_to_string(poison.join("mise"))?;
    for decoy in [home.join(".local/share/mise"), xdg_data_home.join("mise")] {
        let bin = decoy.join("bin");
        fs::create_dir_all(&bin)?;
        write_executable(&bin.join("mise"), &poisoned_mise)?;
    }
    write_executable(
        &mise_dir.join("mise"),
        &mise_audit_wrapper(&real_mise, &audit),
    )?;
    Ok(ResolverRuntime {
        root,
        home,
        workspace,
        runner_temp,
        selected_data,
        selected_input,
        xdg_data_home,
        poison,
        audit,
        poison_log,
    })
}

#[cfg(unix)]
fn run_resolver_script(
    runtime: &ResolverRuntime,
    index: usize,
    step: &ResolverStep,
) -> Result<bool, Box<dyn std::error::Error>> {
    let github_env = runtime.workspace.join(format!("github-env-{index}"));
    fs::write(&github_env, "")?;
    let script = runtime.root.join(format!("resolver-{index}.sh"));
    fs::write(&script, &step.run)?;
    let mut shell = step.shell.split_whitespace();
    let executable = shell.next().ok_or("resolver shell is empty")?;
    let mut command = Command::new(executable);
    let mut has_script_placeholder = false;
    for argument in shell {
        if argument == "{0}" {
            command.arg(&script);
            has_script_placeholder = true;
        } else {
            command.arg(argument);
        }
    }
    assert!(
        has_script_placeholder,
        "emitted resolver shell has no script placeholder: {}",
        step.shell
    );
    let output = command
        .env_clear()
        .current_dir(&runtime.workspace)
        .env("PATH", &runtime.poison)
        .env("GITHUB_ENV", &github_env)
        .env("RUNNER_TEMP", &runtime.runner_temp)
        .env("RUNNER_OS", "Linux")
        .env("RUNNER_ARCH", "X64")
        .env("CI", "true")
        .env("POISON_LOG", &runtime.poison_log)
        .env("RESOLVER_AUDIT", &runtime.audit)
        .envs(step.env.iter())
        .env("MISE_DATA_DIR", &runtime.selected_input)
        .env("XDG_DATA_HOME", &runtime.xdg_data_home)
        .env("HOME", &runtime.home)
        .envs(
            velnor_actions_workflow_renderer::toolchain_env::CREDENTIAL_UNSET_VARS
                .iter()
                .map(|name| (*name, "should-never-reach-mise")),
        )
        .output()?;
    assert!(
        output.status.success(),
        "resolver {index} failed: {}\n{}\n{}",
        step.shell,
        step.run,
        String::from_utf8_lossy(&output.stderr)
    );
    assert_tool_outputs(&github_env, &step.run)
}

#[cfg(unix)]
fn assert_tool_outputs(
    github_env: &std::path::Path,
    script: &str,
) -> Result<bool, Box<dyn std::error::Error>> {
    assert!(
        script.contains("velnor_tool VELNOR_TOOL_"),
        "resolver does not emit a project tool: {script}"
    );
    let resolved = fs::read_to_string(github_env)?;
    assert!(
        !resolved.trim().is_empty(),
        "resolver emitted no tool output"
    );
    let mut saw_tool = false;
    let mut outputs = Vec::new();
    for line in resolved.lines() {
        let (name, value) = line.split_once('=').ok_or("malformed GITHUB_ENV entry")?;
        assert!(!value.is_empty(), "empty tool output: {line}");
        assert!(
            name.starts_with("VELNOR_TOOL_") || name == "RUSTUP_TOOLCHAIN",
            "unexpected resolver output: {line}"
        );
        saw_tool |= name.starts_with("VELNOR_TOOL_");
        outputs.push((name, value));
    }
    assert!(saw_tool, "no VELNOR_TOOL_ output: {resolved}");
    let lock_names = script
        .lines()
        .filter_map(|line| line.strip_prefix("velnor_lock "))
        .filter_map(|arguments| arguments.split_whitespace().next())
        .collect::<Vec<_>>();
    for name in &lock_names {
        let digest = outputs
            .iter()
            .find_map(|(output, value)| (*output == *name).then_some(*value))
            .ok_or_else(|| format!("resolver omitted lock output {name}: {resolved}"))?;
        assert!(
            digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "resolver did not hash the checked-in lock entry for {name}: {digest}"
        );
    }
    Ok(!lock_names.is_empty())
}

#[cfg(unix)]
fn assert_resolver_credentials_were_scrubbed(
    runtime: &ResolverRuntime,
) -> Result<(), Box<dyn std::error::Error>> {
    let audit = fs::read_to_string(&runtime.audit)?;
    let calls = audit
        .lines()
        .filter_map(|line| line.strip_prefix("args="))
        .collect::<Vec<_>>();
    assert!(
        !calls.is_empty(),
        "the resolver did not invoke Mise: {audit}"
    );
    assert!(
        calls
            .iter()
            .all(|args| args.starts_with("--no-config --no-env --no-hooks config get ")),
        "Mise received an unexpected resolver command: {calls:?}"
    );
    let selected = runtime.selected_data.canonicalize()?;
    assert_eq!(runtime.selected_input.canonicalize()?, selected);
    let expected_data = format!("MISE_DATA_DIR={}", selected.display());
    let data_lines = audit
        .lines()
        .filter(|line| line.starts_with("MISE_DATA_DIR="))
        .collect::<Vec<_>>();
    assert_eq!(data_lines.len(), calls.len(), "{audit}");
    assert!(
        data_lines
            .iter()
            .all(|line| line.as_bytes() == expected_data.as_bytes())
    );
    for name in velnor_actions_workflow_renderer::toolchain_env::CREDENTIAL_UNSET_VARS {
        let expected = format!("{name}=unset");
        let credential_lines = audit
            .lines()
            .filter(|line| line.starts_with(&format!("{name}=")))
            .collect::<Vec<_>>();
        assert!(
            credential_lines.len() == calls.len()
                && credential_lines
                    .iter()
                    .all(|line| line.as_bytes() == expected.as_bytes()),
            "Mise observed {name} before unset: {audit}"
        );
    }
    assert!(
        !runtime.poison_log.exists(),
        "a helper resolved through poisoned PATH"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn emitted_resolvers_use_the_trusted_mise_and_scrub_credentials_before_helpers() -> TestResult {
    let root = tempfile::TempDir::new()?;
    let runtime = resolver_runtime(root.path())?;
    let tree = rendered_tree()?;
    let resolvers = super::security::generated_resolvers(&tree);
    let mut seen = BTreeSet::new();
    let mut count = 0;
    let mut saw_lock_present_resolver = false;
    for (path, step) in &resolvers {
        super::security::assert_step_envelope(&tree, path, step)?;
        let identity = (step.shell.clone(), step.run.clone(), step.env.clone());
        if !seen.insert(identity) {
            continue;
        }
        saw_lock_present_resolver |= run_resolver_script(&runtime, count, step)?;
        count += 1;
    }
    assert!(count > 0, "at least one distinct resolver is emitted");
    assert!(
        saw_lock_present_resolver,
        "a lock-present resolver was not executed"
    );
    assert_resolver_credentials_were_scrubbed(&runtime)?;
    Ok(())
}

#[cfg(unix)]
pub(super) fn executable_on_path(name: &str) -> std::io::Result<std::path::PathBuf> {
    use std::{env, io};

    let paths = env::var_os("PATH").ok_or_else(|| io::Error::other("PATH is unset"))?;
    for directory in env::split_paths(&paths) {
        let candidate = directory.join(name);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!("{name} was not found on the test PATH"),
    ))
}

#[cfg(unix)]
pub(super) fn write_executable(path: &std::path::Path, contents: &str) -> std::io::Result<()> {
    use std::{fs, os::unix::fs::PermissionsExt};

    fs::write(path, contents)?;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)
}

#[cfg(unix)]
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(unix)]
fn mise_audit_wrapper(mise: &std::path::Path, audit: &std::path::Path) -> String {
    let mut credential_lines = String::new();
    for name in velnor_actions_workflow_renderer::toolchain_env::CREDENTIAL_UNSET_VARS {
        credential_lines.push_str("printf '%s=%s\\n' '");
        credential_lines.push_str(name);
        credential_lines.push_str("' \"${");
        credential_lines.push_str(name);
        credential_lines.push_str("-unset}\" >> \"$RESOLVER_AUDIT\"\n");
    }
    format!(
        "#!/bin/sh\nprintf 'args=%s\\n' \"$*\" >> {}\n{credential_lines}printf 'MISE_DATA_DIR=%s\\n' \"${{MISE_DATA_DIR-unset}}\" >> \"$RESOLVER_AUDIT\"\nexec {} \"$@\"\n",
        shell_quote(&audit.display().to_string()),
        shell_quote(&mise.display().to_string())
    )
}
