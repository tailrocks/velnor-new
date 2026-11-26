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
            match name.as_str() {
                "version" => value.trim().clone_into(&mut parsed.version),
                "sha256" => value.trim().clone_into(&mut parsed.sha256),
                "install_args" => value.trim().clone_into(&mut parsed.install_args),
                "cache" if value.trim() == "false" => cache_disabled = true,
                "cache" => return Err(ExecError::Unsupported(CACHE_MUST_BE_FALSE.to_owned())),
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
) -> Result<String, ExecError> {
    let words: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
    let program = install.binary.to_string_lossy().into_owned();
    let env = ctx.env_with(&install.process_env(ctx));
    ctx.capture((&program, &words, ctx.workspace, &env)).await
}

async fn mise_json(
    ctx: &mut StepContext<'_>,
    install: &Install,
    args: &[&str],
) -> Result<BTreeMap<String, String>, ExecError> {
    let output = mise_capture(ctx, install, args).await?;
    let values: BTreeMap<String, serde_json::Value> = serde_json::from_str(&output)
        .map_err(|error| ExecError::Invalid(format!("mise {} output: {error}", args.join(" "))))?;
    Ok(values
        .into_iter()
        .filter_map(|(name, value)| Some((name, value.as_str()?.to_owned())))
        .collect())
}

fn path_additions(computed: &str, current: &str, binary_dir: &str) -> Vec<String> {
    let known: Vec<&str> = current.split(':').chain([binary_dir]).collect();
    computed
        .split(':')
        .filter(|entry| !entry.is_empty() && !known.contains(entry))
        .map(str::to_owned)
        .collect()
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

impl Install {
    fn new(ctx: &StepContext<'_>, tools: &Path, binary: PathBuf) -> Self {
        let dir = binary
            .parent()
            .map_or_else(String::new, |dir| dir.to_string_lossy().into_owned());
        let data = tools.join("data").to_string_lossy().into_owned();
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
}

async fn export(ctx: &mut StepContext<'_>, install: &Install) -> Result<(), ExecError> {
    let redacted = mise_json(ctx, install, &["env", "--redacted", "--json"]).await?;
    for value in redacted.values() {
        ctx.add_mask(value);
    }
    let computed = mise_json(ctx, install, &["env", "--json"]).await?;
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
            let mut added = path_additions(value, &current_path(ctx), &install.dir);
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
    enforce_locked_mise_policy(ctx, &inputs.install_args).await?;
    let install = Install::new(ctx, &tools, binary);
    let loaded = mise_capture(ctx, &install, &["config", "ls", "--json"]).await?;
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
