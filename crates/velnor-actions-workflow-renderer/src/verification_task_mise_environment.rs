use super::*;

pub(super) fn base_environment(root: &str) -> Vec<String> {
    vec![
        "set -euo pipefail".to_owned(),
        // Credential unsets come from the renderer's injected prelude, which
        // must stay the only token-naming text for the token-hygiene gate.
        "for name in ${!MISE_@}; do unset \"$name\"; done".to_owned(),
        "umask 077".to_owned(),
        format!("task_root=\"{root}\""),
        "test -n \"$RUNNER_TEMP\" && test -n \"$GITHUB_WORKSPACE\" && test -n \"$GITHUB_RUN_ID\" && test -n \"$GITHUB_RUN_ATTEMPT\"".to_owned(),
    ]
}

pub(super) fn private_environment() -> Vec<String> {
    [
        "HOME=\"$task_root/home\"",
        "XDG_CONFIG_HOME=\"$task_root/home/config\"",
        "XDG_DATA_HOME=\"$task_root/home/data\"",
        "XDG_CACHE_HOME=\"$task_root/home/cache\"",
        "XDG_STATE_HOME=\"$task_root/home/state\"",
        "MISE_CONFIG_DIR=\"$task_root/config\"",
        "MISE_DATA_DIR=\"$task_root/data\"",
        "MISE_CACHE_DIR=\"$task_root/cache\"",
        "MISE_STATE_DIR=\"$task_root/state\"",
        "CARGO_HOME=\"$task_root/cargo\"",
        "RUSTUP_HOME=\"$task_root/rustup\"",
        "MISE_GLOBAL_CONFIG_FILE=\"$task_root/global.toml\"",
        "MISE_SYSTEM_CONFIG_FILE=\"$task_root/system.toml\"",
        "MISE_AUTO_INSTALL=false",
        "MISE_EXEC_AUTO_INSTALL=false",
        "MISE_TASK_RUN_AUTO_INSTALL=false",
        "MISE_AUTO_ENV=false",
        "MISE_NO_ENV=1",
    ]
    .iter()
    .map(|assignment| format!("export {assignment}"))
    .collect()
}

pub(super) fn source_hash_check(
    path: &str,
    expected: Option<&str>,
    policy: &VerificationTaskPolicy,
) -> String {
    let (hash_bin, hash_args) = match policy.task.runner {
        velnor_actions_contract::VerificationRunner::LinuxX64 => ("/usr/bin/sha256sum", ""),
        velnor_actions_contract::VerificationRunner::MacosArm64
        | velnor_actions_contract::VerificationRunner::Macos26Arm64 => {
            ("/usr/bin/shasum", "-a 256")
        }
    };
    let mut statements = repository_path_guards(path, false);
    statements.push(match expected {
        Some(expected) => format!(
            "test -f \"$workspace_root/{path}\"; test ! -L \"$workspace_root/{path}\"; {hash_bin} {hash_args} \"$workspace_root/{path}\" > \"$task_root/sha256.txt\"; read actual rest < \"$task_root/sha256.txt\"; test \"$actual\" = \"{expected}\""
        ),
        None => format!(
            "test ! -e \"$workspace_root/{path}\" && test ! -L \"$workspace_root/{path}\""
        ),
    });
    statements.join("; ")
}

pub(super) fn repository_path_guards(path: &str, directory: bool) -> Vec<String> {
    let components = path.split('/').collect::<Vec<_>>();
    let prefix_len = if directory {
        components.len()
    } else {
        components.len().saturating_sub(1)
    };
    (1..=prefix_len)
        .map(|length| {
            let prefix = components[..length].join("/");
            format!("test -d \"$workspace_root/{prefix}\"; test ! -L \"$workspace_root/{prefix}\"")
        })
        .collect()
}

pub(super) fn private_config_chain_check(expected: &str) -> String {
    format!(
        "mise --no-env --no-hooks config ls --json | \"$jq_path\" -r '.[].path' | LC_ALL=C /usr/bin/sort > \"$task_root/actual_mise_configs.txt\"; printf '%s\\n' \"{expected}\" \"$task_root/global.toml\" \"$task_root/system.toml\" | LC_ALL=C /usr/bin/sort | /usr/bin/cmp -s \"$task_root/actual_mise_configs.txt\" -"
    )
}

pub(super) fn workspace_config_chain_check(policy: &VerificationTaskPolicy) -> String {
    let config_path = format!("$workspace_root/{}", policy.task.source.mise_config);
    let rust_path = format!(
        "$workspace_root/{}",
        policy.task.source.rust_toolchain_path()
    );
    let mut checks = vec![
        "mise --no-env --no-hooks config ls --json | \"$jq_path\" -r '.[].path' | LC_ALL=C /usr/bin/sort > \"$task_root/mise_configs.txt\"".to_owned(),
        "workspace_config_found=false".to_owned(),
        format!("while IFS= read -r path; do case \"$path\" in \"{config_path}\") workspace_config_found=true ;; \"{rust_path}\"|\"$task_root/global.toml\"|\"$task_root/system.toml\") ;; *) exit 1 ;; esac; done < \"$task_root/mise_configs.txt\""),
    ];
    checks.push(if policy.mise_config_sha256.is_some() {
        "[[ \"$workspace_config_found\" == true ]]".to_owned()
    } else {
        "[[ \"$workspace_config_found\" == false ]]".to_owned()
    });
    checks.join("; ")
}

pub(super) fn task_root(policy: &VerificationTaskPolicy) -> String {
    format!(
        "${{RUNNER_TEMP}}/velnor-verification-${{GITHUB_RUN_ID}}-${{GITHUB_RUN_ATTEMPT}}-{}",
        policy.task.id
    )
}

/// Inline-shell argv so the script is single-quoted whole at render time.
///
/// Inner-shell variables must survive the outer shell; the script itself
/// starts with `set -euo pipefail`.
pub(super) fn bash_script(script: &str) -> Vec<String> {
    vec!["bash".to_owned(), "-c".to_owned(), script.to_owned()]
}

pub(super) fn platform(
    runner: velnor_actions_contract::VerificationRunner,
) -> (&'static str, &'static str) {
    match runner {
        velnor_actions_contract::VerificationRunner::LinuxX64 => ("linux", "linux-x64"),
        velnor_actions_contract::VerificationRunner::MacosArm64
        | velnor_actions_contract::VerificationRunner::Macos26Arm64 => ("macos", "macos-arm64"),
    }
}

pub(super) fn safe_option(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 512
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b',' | b'.' | b'_' | b'-'))
}
