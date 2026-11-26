// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/mise.rs

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use futures_util::FutureExt;
use ring::digest;
use tokio::io::AsyncReadExt;
use uuid::Uuid;

use super::composite::SetupVelnor;
use super::mise_lock::{enforce_locked_mise_policy, is_valid_mise_version, require_sole_config};
use crate::exec::{
    AdapterRegistry, ExecError, ProcessOutcome, StepAdapter, StepContext, StepFuture, StepResult,
};
use crate::expr::hex_digest;
use crate::store::store_root;

const ACTION: &str = "jdx/mise-action";
const PREFIX: &str = "jdx/mise-action@";
const NOT_IN_STORE: &str = "tool is not installed in the tools store";
const CACHE_MUST_BE_FALSE: &str = "jdx/mise-action input 'cache' must be false";
const BOOLEAN_INPUTS: [&str; 11] = [
    "auto_update",
    "fetch_from_github",
    "reshim",
    "install",
    "bootstrap",
    "env",
    "wings_enabled",
    "export_path",
    "experimental",
    "add_shims_to_path",
    "monorepo",
];
const VERSION_POLICY: &str = include_str!("../../../../.velnor/version-policy.toml");

pub(super) fn register(registry: &mut AdapterRegistry, setup: SetupVelnor) {
    registry.register(Box::new(MiseAction));
    registry.register(Box::new(setup));
}

#[derive(Debug)]
pub(crate) struct MiseInputs {
    version: String,
    sha256: String,
    install_args: String,
}

impl MiseInputs {
    pub(crate) fn parse(inputs: &BTreeMap<String, String>) -> Result<Self, ExecError> {
        let mut parsed = Self {
            version: String::new(),
            sha256: String::new(),
            install_args: String::new(),
        };
        let mut cache_disabled = false;
        for (name, value) in inputs {
            match name.to_ascii_lowercase().as_str() {
                "version" => value.trim().clone_into(&mut parsed.version),
                "sha256" => value.trim().clone_into(&mut parsed.sha256),
                "install_args" => value.trim().clone_into(&mut parsed.install_args),
                "cache" if matches!(value.trim(), "false" | "False" | "FALSE") => {
                    cache_disabled = true;
                }
                "cache" => return Err(ExecError::Unsupported(CACHE_MUST_BE_FALSE.to_owned())),
                other if value.trim().is_empty() && BOOLEAN_INPUTS.contains(&other) => {
                    return Err(ExecError::Invalid(format!(
                        "Input does not meet YAML 1.2 \"Core Schema\" specification: {other}\nSupport boolean input list: `true | True | TRUE | false | False | FALSE`"
                    )));
                }
                _ if value.trim().is_empty() => {}
                other => {
                    return Err(ExecError::Unsupported(format!(
                        "jdx/mise-action input '{other}' is not supported by velnor-runner"
                    )));
                }
            }
        }
        if !cache_disabled {
            return Err(ExecError::Unsupported(CACHE_MUST_BE_FALSE.to_owned()));
        }
        if !is_valid_mise_version(&parsed.version) {
            return Err(ExecError::Invalid(format!(
                "mise version '{}' is not an exact YYYY.M.D version",
                parsed.version
            )));
        }
        Ok(parsed)
    }
}

async fn sha256_hex(path: &Path) -> Result<String, ExecError> {
    let io = |source| ExecError::io(format!("read {}", path.display()), source);
    let mut file = tokio::fs::File::open(path).await.map_err(io)?;
    let mut context = digest::Context::new(&digest::SHA256);
    let mut chunk = vec![0_u8; 1 << 16];
    loop {
        let read = file.read(&mut chunk).await.map_err(io)?;
        if read == 0 {
            break;
        }
        context.update(&chunk[..read]);
    }
    Ok(hex_digest(context.finish().as_ref()))
}

async fn locate_binary(inputs: &MiseInputs, tools: &Path) -> Result<PathBuf, ExecError> {
    let binary = tools.join("mise").join(&inputs.version).join("mise");
    if !tokio::fs::try_exists(&binary).await.unwrap_or(false) {
        return Err(ExecError::Invalid(format!(
            "mise {} is not in the tools store at {}; run the tools store task first",
            inputs.version,
            binary.display()
        )));
    }
    if !inputs.sha256.is_empty() {
        let actual = sha256_hex(&binary).await?;
        if !actual.eq_ignore_ascii_case(&inputs.sha256) {
            return Err(ExecError::Invalid(format!(
                "mise {} sha256 mismatch",
                inputs.version
            )));
        }
    }
    Ok(binary)
}

async fn append(path: &str, text: &str) -> Result<(), ExecError> {
    use tokio::io::AsyncWriteExt;
    let io = |source| ExecError::io(format!("write {path}"), source);
    let mut file = tokio::fs::OpenOptions::new()
        .append(true)
        .open(path)
        .await
        .map_err(io)?;
    file.write_all(text.as_bytes()).await.map_err(io)
}

fn env_command(name: &str, value: &str) -> String {
    let delimiter = format!("velnor_{}", Uuid::new_v4().simple());
    format!("{name}<<{delimiter}\n{value}\n{delimiter}\n")
}

async fn mise_capture(
    ctx: &mut StepContext<'_>,
    install: &Install,
    args: &[&str],
    process_env: &[(String, String)],
) -> Result<String, ExecError> {
    let words: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
    let program = install.binary.to_string_lossy().into_owned();
    let env = ctx.env_with(process_env);
    ctx.capture((&program, &words, ctx.workspace, &env)).await
}

async fn mise_json(
    ctx: &mut StepContext<'_>,
    install: &Install,
    args: &[&str],
) -> Result<BTreeMap<String, String>, ExecError> {
    let process_env = install.process_env(ctx);
    let output = mise_capture(ctx, install, args, &process_env).await?;
    let values: BTreeMap<String, serde_json::Value> = serde_json::from_str(&output)
        .map_err(|error| ExecError::Invalid(format!("mise {} output: {error}", args.join(" "))))?;
    Ok(values
        .into_iter()
        .filter_map(|(name, value)| Some((name, value.as_str()?.to_owned())))
        .collect())
}

fn path_additions(computed: &str, current: &str, binary_dir: &str) -> Vec<String> {
    let entries = |path: &str| -> Vec<String> {
        path.split(':')
            .filter(|entry| !entry.is_empty())
            .map(str::to_owned)
            .collect()
    };
    let computed = entries(computed);
    let current = entries(current);
    let prefix = (!current.is_empty())
        .then(|| {
            computed
                .windows(current.len())
                .position(|window| window == current)
        })
        .flatten();
    let additions: Vec<String> = match prefix {
        Some(index) => computed[..index].to_vec(),
        None => computed
            .into_iter()
            .filter(|entry| !current.contains(entry))
            .collect(),
    };
    additions
        .into_iter()
        .filter(|entry| entry != binary_dir)
        .collect()
}

async fn managed_paths(ctx: &mut StepContext<'_>, install: &Install) -> Option<Vec<String>> {
    let process_env = install.process_env(ctx);
    let output = mise_capture(ctx, install, &["env", "--paths"], &process_env)
        .await
        .ok()?;
    Some(
        output
            .lines()
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect(),
    )
}

struct Install {
    binary: PathBuf,
    dir: String,
    data: String,
    env: Vec<(String, String)>,
}

fn current_path(ctx: &StepContext<'_>) -> String {
    ctx.env
        .get("PATH")
        .cloned()
        .or_else(|| std::env::var("PATH").ok())
        .unwrap_or_default()
}

fn data_dir(tools: &Path, _lock: &[u8]) -> PathBuf {
    tools.join("data")
}

impl Install {
    fn new(ctx: &StepContext<'_>, tools: &Path, lock: &str, binary: PathBuf) -> Self {
        let dir = binary
            .parent()
            .map_or_else(String::new, |dir| dir.to_string_lossy().into_owned());
        let data = data_dir(tools, lock.as_bytes())
            .to_string_lossy()
            .into_owned();
        let mut env = vec![
            ("MISE_DATA_DIR".to_owned(), data.clone()),
            (
                "MISE_CACHE_DIR".to_owned(),
                ctx.temp.join("mise-cache").to_string_lossy().into_owned(),
            ),
            ("MISE_AUTO_INSTALL".to_owned(), "false".to_owned()),
        ];
        for (name, value) in [
            (
                "MISE_TRUSTED_CONFIG_PATHS",
                ctx.workspace.to_string_lossy().into_owned(),
            ),
            ("MISE_YES", "1".to_owned()),
        ] {
            if !ctx.env.contains_key(name) && std::env::var_os(name).is_none() {
                env.push((name.to_owned(), value));
            }
        }
        Self {
            binary,
            dir,
            data,
            env,
        }
    }

    fn process_env(&self, ctx: &StepContext<'_>) -> Vec<(String, String)> {
        let mut env = self.env.clone();
        env.push((
            "PATH".to_owned(),
            format!("{}:{}", self.dir, current_path(ctx)),
        ));
        env
    }

    fn probe_env(&self, ctx: &StepContext<'_>) -> Vec<(String, String)> {
        let mut env = self.process_env(ctx);
        env.push(("MISE_NO_ENV".to_owned(), "1".to_owned()));
        env
    }
}

async fn export(ctx: &mut StepContext<'_>, install: &Install) -> Result<(), ExecError> {
    let redacted = mise_json(ctx, install, &["env", "--redacted", "--json"]).await?;
    for value in redacted.values() {
        ctx.add_mask(value);
    }
    let computed = mise_json(ctx, install, &["env", "--json"]).await?;
    let managed = managed_paths(ctx, install).await;
    let github_path = ctx.env.get("GITHUB_PATH").cloned().unwrap_or_default();
    let github_env = ctx.env.get("GITHUB_ENV").cloned().unwrap_or_default();
    let mut paths = vec![install.dir.clone(), format!("{}/shims", install.data)];
    let mut variables: String = install
        .env
        .iter()
        .map(|(name, value)| env_command(name, value))
        .collect();
    for (name, value) in &computed {
        if name.eq_ignore_ascii_case("PATH") {
            let mut added = managed
                .clone()
                .unwrap_or_else(|| path_additions(value, &current_path(ctx), &install.dir));
            added.reverse();
            paths.extend(added);
        } else {
            variables.push_str(&env_command(name, value));
        }
    }
    append(&github_path, &(paths.join("\n") + "\n")).await?;
    append(&github_env, &variables).await
}

pub(crate) async fn mise_action(
    inputs: &MiseInputs,
    ctx: &mut StepContext<'_>,
) -> Result<StepResult, ExecError> {
    let tools = store_root().join("tools");
    let binary = locate_binary(inputs, &tools).await?;
    let lock = enforce_locked_mise_policy(ctx, &inputs.install_args).await?;
    let install = Install::new(ctx, &tools, &lock, binary);
    let probe_env = install.probe_env(ctx);
    let loaded = mise_capture(ctx, &install, &["config", "ls", "--json"], &probe_env).await?;
    require_sole_config(ctx.workspace, &loaded).await?;
    let mut args = vec!["install".to_owned(), "--locked".to_owned()];
    args.extend(inputs.install_args.split_whitespace().map(str::to_owned));
    let program = install.binary.to_string_lossy().into_owned();
    let env = ctx.env_with(&install.process_env(ctx));
    let dir = ctx.workspace.to_path_buf();
    match ctx
        .run_process((&program, &args, &dir, &env), false)
        .await?
    {
        ProcessOutcome::Succeeded => {}
        ProcessOutcome::Cancelled => return Ok(StepResult::Cancelled),
        ProcessOutcome::Failed => {
            ctx.log(NOT_IN_STORE);
            return Ok(StepResult::Failed);
        }
    }
    export(ctx, &install).await?;
    ctx.set_output("cache-hit", "false");
    Ok(StepResult::Succeeded)
}

fn pinned_revision() -> Result<String, ExecError> {
    let policy: toml::Table = VERSION_POLICY.parse().map_err(|error| {
        ExecError::Invalid(format!(
            ".velnor/version-policy.toml is not valid TOML: {error}"
        ))
    })?;
    policy
        .get("actions")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(toml::Value::as_table)
        .find(|action| action.get("name").and_then(toml::Value::as_str) == Some(ACTION))
        .and_then(|action| action.get("sha")?.as_str())
        .map(str::to_owned)
        .ok_or_else(|| {
            ExecError::Invalid(format!(
                ".velnor/version-policy.toml pins no revision of {ACTION}"
            ))
        })
}

fn require_pinned_revision(reference: &str) -> Result<(), ExecError> {
    let pinned = pinned_revision()?;
    if reference.strip_prefix(PREFIX) == Some(pinned.as_str()) {
        return Ok(());
    }
    Err(ExecError::Unsupported(format!(
        "{reference} is not the {ACTION} revision {pinned} pinned in .velnor/version-policy.toml"
    )))
}

#[derive(Debug)]
struct MiseAction;

impl StepAdapter for MiseAction {
    fn handles(&self, reference: &str) -> bool {
        reference.starts_with(PREFIX)
    }

    fn run<'a>(&'a self, ctx: &'a mut StepContext<'_>) -> StepFuture<'a> {
        async move {
            require_pinned_revision(&ctx.reference)?;
            let inputs = MiseInputs::parse(&ctx.inputs)?;
            mise_action(&inputs, ctx).await
        }
        .boxed()
    }
}
