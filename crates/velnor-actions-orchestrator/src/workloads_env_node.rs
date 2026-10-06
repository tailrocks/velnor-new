//! Credential-free npm v11 children with generator-owned configuration.
//!
//! npm v11 resolves environment configuration above project/user/global files.
//! User configuration explicitly selects `/dev/null`; global configuration is
//! an empty owned file (npm rejects loading one file as both sources). Public
//! source qualification separately rejects repository `.npmrc` configuration.
//! Source: https://docs.npmjs.com/cli/v11/configuring-npm/npmrc

use std::collections::BTreeMap;

use velnor_actions_contract::Step;

use crate::OrchestratorError;

const HOME_BASE: &str = "${{ runner.temp }}/velnor/native/node-home";
const CACHE: &str = "${{ runner.temp }}/velnor/native/npm";
const CONFIG: [(&str, &str); 2] = [
    ("NPM_CONFIG_USERCONFIG", "/dev/null"),
    ("NPM_CONFIG_REGISTRY", "https://registry.npmjs.org/"),
];

/// Recover the root only from the closed native Mise vector's directory flag.
pub(crate) fn env_for_argv(argv: &[String]) -> Result<BTreeMap<String, String>, OrchestratorError> {
    let root = match argv {
        [mise, cd, root, ..] if mise == "mise" && cd == "--cd" => root,
        _ => {
            return Err(crate::internal::internal(
                "node_isolation_fixed_argv_required",
            ));
        }
    };
    task_env(root)
}

/// One owned home per validated repository root; download bytes stay shared.
pub(crate) fn task_env(root: &str) -> Result<BTreeMap<String, String>, OrchestratorError> {
    crate::source_prep::validate_root(root)?;
    let slug = velnor_actions_contract::digest_b3(root.as_bytes());
    let home = format!("{HOME_BASE}/{slug}");
    let mut env = BTreeMap::from([
        ("HOME".to_owned(), home.clone()),
        ("XDG_CONFIG_HOME".to_owned(), format!("{home}/config")),
        ("XDG_DATA_HOME".to_owned(), format!("{home}/data")),
        ("XDG_CACHE_HOME".to_owned(), format!("{home}/cache")),
        ("NPM_CONFIG_CACHE".to_owned(), CACHE.to_owned()),
        (
            "NPM_CONFIG_GLOBALCONFIG".to_owned(),
            format!("{home}/global.npmrc"),
        ),
    ]);
    for (key, value) in CONFIG
        .iter()
        .chain(velnor_actions_mise::ISOLATION_ENV.iter())
        .chain(velnor_actions_mise::NO_AUTO_INSTALL_ENV.iter())
    {
        env.insert((*key).to_owned(), (*value).to_owned());
    }
    Ok(env)
}

/// Wrap only the fixed repository payload; report/cache helpers stay outside.
pub(crate) fn isolation_prefix() -> String {
    let mut prefix = String::from("env -i PATH=\"$PATH\"");
    for key in [
        "RUNNER_TEMP",
        "MISE_DATA_DIR",
        "HOME",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_CACHE_HOME",
        "NPM_CONFIG_CACHE",
    ] {
        prefix.push_str(&format!(" {key}=\"${key}\""));
    }
    prefix.push_str(" NPM_CONFIG_GLOBALCONFIG=\"$HOME/global.npmrc\"");
    // Fixed configuration never expands ambient config values, including a
    // differently cased npm_config_* selector or authentication variable.
    for (key, value) in CONFIG
        .iter()
        .chain(velnor_actions_mise::ISOLATION_ENV.iter())
        .chain(velnor_actions_mise::NO_AUTO_INSTALL_ENV.iter())
    {
        prefix.push_str(&format!(" {key}='{value}'"));
    }
    prefix.push(' ');
    prefix
}

/// Prepare owned mutable directories without installing tools or dependencies.
pub(crate) fn materialization_step(root: &str) -> Result<Step, OrchestratorError> {
    let script = r#"set -eu
umask 077
for path in "$RUNNER_TEMP/velnor" "$RUNNER_TEMP/velnor/native" "$RUNNER_TEMP/velnor/native/node-home" "$HOME" "$XDG_CONFIG_HOME" "$XDG_DATA_HOME" "$XDG_CACHE_HOME" "$NPM_CONFIG_CACHE" "$HOME/global.npmrc"; do
  test ! -L "$path" || { echo 'npm_isolation_symlink' >&2; exit 1; }
done
mkdir -p "$HOME" "$XDG_CONFIG_HOME" "$XDG_DATA_HOME" "$XDG_CACHE_HOME" "$NPM_CONFIG_CACHE"
printf '' > "$HOME/global.npmrc"
"#;
    Ok(velnor_actions_workflow_renderer::shell_step(
        "Prepare isolated npm configuration",
        vec!["sh".to_owned(), "-c".to_owned(), script.to_owned()],
        task_env(root)?,
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use velnor_actions_contract::StepKind;

    #[test]
    fn configuration_paths_are_owned_and_renderable() {
        let env = task_env("web app").expect("root");
        velnor_actions_workflow_renderer::validate_env(&env).expect("env");
        assert!(env["HOME"].starts_with(HOME_BASE));
        assert!(!env["HOME"].contains("web app"));
        assert_ne!(env["HOME"], task_env("other").expect("root")["HOME"]);
        assert_eq!(env["NPM_CONFIG_CACHE"], CACHE);
        assert_eq!(env["NPM_CONFIG_USERCONFIG"], "/dev/null");
        assert_eq!(
            env["NPM_CONFIG_GLOBALCONFIG"],
            format!("{}/global.npmrc", env["HOME"])
        );
        assert_eq!(env["NPM_CONFIG_REGISTRY"], "https://registry.npmjs.org/");
        assert!(task_env("../outside").is_err());
        let StepKind::Shell { run, .. } = materialization_step(".").expect("step").kind else {
            panic!("shell step");
        };
        assert!(run[2].contains("test ! -L"));
        assert!(!run[2].contains("npm ci"));
    }

    #[test]
    fn environment_root_comes_only_from_fixed_mise_directory_flag() {
        let argv = ["mise", "--cd", "web app", "--no-config", "exec"].map(str::to_owned);
        assert_eq!(
            env_for_argv(&argv).expect("fixed argv"),
            task_env("web app").expect("root")
        );
        for argv in [
            vec![],
            vec!["npm".to_owned()],
            vec![
                "mise".to_owned(),
                "--cd".to_owned(),
                "../outside".to_owned(),
            ],
        ] {
            assert!(env_for_argv(&argv).is_err());
        }
    }

    #[test]
    fn actual_child_drops_credentials_and_ambient_npm_selectors() {
        let env = task_env(".").expect("root");
        let script = format!("{}env", isolation_prefix());
        let mut command = Command::new("sh");
        command.args(["-c", &script]).env_clear().envs(&env);
        command.env("PATH", "/usr/bin:/bin");
        command.env("RUNNER_TEMP", "/tmp/runner");
        command.env("MISE_DATA_DIR", "/tmp/runner/velnor/mise");
        for key in [
            "NPM_TOKEN",
            "NODE_AUTH_TOKEN",
            "AWS_ACCESS_KEY_ID",
            "AWS_SECRET_ACCESS_KEY",
            "GITHUB_TOKEN",
            "GH_TOKEN",
            "ACTIONS_RUNTIME_TOKEN",
            "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
            "npm_config_userconfig",
            "npm_config_globalconfig",
            "npm_config_registry",
            "NODE_OPTIONS",
            "HTTPS_PROXY",
            "VELNOR_NPM_PUBLIC_INTEGRITIES",
        ] {
            command.env(key, "hostile-token");
        }
        command.env("NPM_CONFIG_USERCONFIG", "$(touch /tmp/never-execute)");
        command.env("NPM_CONFIG_GLOBALCONFIG", "hostile-token");
        command.env("NPM_CONFIG_REGISTRY", "https://private.invalid/");
        command.env("MISE_NO_CONFIG", "0");
        let output = command.output().expect("child");
        assert!(output.status.success());
        let text = String::from_utf8(output.stdout).expect("utf8");
        let actual: BTreeMap<_, _> = text
            .lines()
            .map(|line| {
                let (key, value) = line.split_once('=').expect("entry");
                (key.to_owned(), value.to_owned())
            })
            .collect();
        let mut expected = env;
        expected.insert("PATH".to_owned(), "/usr/bin:/bin".to_owned());
        expected.insert("RUNNER_TEMP".to_owned(), "/tmp/runner".to_owned());
        expected.insert(
            "MISE_DATA_DIR".to_owned(),
            "/tmp/runner/velnor/mise".to_owned(),
        );
        assert_eq!(actual, expected);
        assert!(!text.contains("hostile-token"));
    }

    #[test]
    fn fixed_argv_keeps_root_as_one_literal_argument() {
        let argv = vec![
            "printf".to_owned(),
            "%s".to_owned(),
            "web app; credential".to_owned(),
        ];
        let command = velnor_actions_workflow_renderer::join_argv_for_run(&argv).expect("argv");
        let output = Command::new("sh")
            .args(["-c", &format!("{}{command}", isolation_prefix())])
            .env("PATH", "/usr/bin:/bin")
            .output()
            .expect("child");
        assert!(output.status.success());
        assert_eq!(output.stdout, b"web app; credential");
    }
}
