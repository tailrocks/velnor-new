//! Real-shell execution of emitted tools-cache identity resolvers.

#[cfg(unix)]
use std::{fs, process::Command};

#[cfg(unix)]
use super::{TestResult, base_lock, base_toml, generated};

#[cfg(unix)]
struct ResolverRuntime {
    home: std::path::PathBuf,
    workspace: std::path::PathBuf,
    runner_temp: std::path::PathBuf,
    poison: std::path::PathBuf,
    audit: std::path::PathBuf,
    poison_log: std::path::PathBuf,
}

#[cfg(unix)]
fn resolver_runtime(root: &std::path::Path) -> Result<ResolverRuntime, Box<dyn std::error::Error>> {
    let home = root.join("home");
    let mise_dir = home.join(".local/share/mise/bin");
    let workspace = root.join("workspace");
    let runner_temp = root.join("runner-temp");
    let poison = root.join("poison");
    fs::create_dir_all(&mise_dir)?;
    fs::create_dir_all(&workspace)?;
    fs::create_dir_all(&runner_temp)?;
    fs::create_dir_all(&poison)?;
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
    Ok(ResolverRuntime {
        home,
        workspace,
        runner_temp,
        poison,
        audit,
        poison_log,
    })
}

#[cfg(unix)]
fn run_resolver_script(
    runtime: &ResolverRuntime,
    index: usize,
    script: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let github_env = runtime.workspace.join(format!("github-env-{index}"));
    let output = Command::new("/bin/bash")
        .args([
            "--noprofile",
            "--norc",
            "-e",
            "-o",
            "pipefail",
            "-c",
            script,
        ])
        .current_dir(&runtime.workspace)
        .env("PATH", &runtime.poison)
        .env("HOME", &runtime.home)
        .env("GITHUB_ENV", &github_env)
        .env("RUNNER_TEMP", &runtime.runner_temp)
        .env("RUNNER_OS", "Linux")
        .env("RUNNER_ARCH", "X64")
        .env("CI", "true")
        .env("RESOLVER_AUDIT", &runtime.audit)
        .envs(
            velnor_actions_workflow_renderer::toolchain_env::CREDENTIAL_UNSET_VARS
                .iter()
                .map(|name| (*name, "should-never-reach-mise")),
        )
        .output()?;
    assert!(
        output.status.success(),
        "resolver {index} failed: {}\n{script}",
        String::from_utf8_lossy(&output.stderr)
    );
    let resolved = fs::read_to_string(github_env)?;
    assert!(resolved.contains("VELNOR_TOOL_"), "{resolved}");
    Ok(())
}

#[cfg(unix)]
fn assert_resolver_credentials_were_scrubbed(
    runtime: &ResolverRuntime,
) -> Result<(), Box<dyn std::error::Error>> {
    let audit = fs::read_to_string(&runtime.audit)?;
    for name in velnor_actions_workflow_renderer::toolchain_env::CREDENTIAL_UNSET_VARS {
        assert!(
            audit.lines().any(|line| line == format!("{name}=unset")),
            "Mise observed {name} before unset: {audit}"
        );
    }
    assert!(audit.contains(&format!(
        "MISE_DATA_DIR={}",
        runtime.home.join(".local/share/mise").display()
    )));
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
    let scripts = generated()?.0;
    assert!(!scripts.is_empty(), "at least one resolver is emitted");
    for (index, script) in scripts.iter().enumerate() {
        run_resolver_script(&runtime, index, script)?;
    }
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

#[cfg(unix)]
pub(super) fn mise_delegate_wrapper(mise: &std::path::Path) -> String {
    format!(
        "#!/bin/sh\nexec {} \"$@\"\n",
        shell_quote(&mise.display().to_string())
    )
}
