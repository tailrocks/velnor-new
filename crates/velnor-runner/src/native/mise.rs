// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/mise.rs

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use futures_util::FutureExt;
use uuid::Uuid;

mod binary;
mod inputs;
mod isolation;
mod mounts;
mod paths;
mod pin;
mod readonly;
mod tools;
mod walk;

use self::binary::locate_binary;
use self::inputs::MiseInputs;
use self::isolation::{Isolation, StoreEnv};
use self::paths::require_confined;
use self::pin::{require_pinned_revision, require_pinned_version};
use self::readonly::require_read_only_store;
use super::composite::SetupVelnor;
use super::mise_lock::{enforce_locked_mise_policy, require_sole_config};
use crate::broker::TrustedFacts;
use crate::exec::{
    AdapterRegistry, ExecError, ProcessOutcome, StepAdapter, StepContext, StepFuture, StepResult,
};
use crate::store::store_root;

const ACTION: &str = "jdx/mise-action";
const PREFIX: &str = "jdx/mise-action@";
const NOT_IN_STORE: &str = "tool is not installed in the tools store";

pub(super) fn register(registry: &mut AdapterRegistry, facts: Result<TrustedFacts, String>) {
    registry.register(Box::new(MiseAction {
        facts: facts.clone(),
    }));
    registry.register(Box::new(SetupVelnor { facts }));
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

struct Mise {
    program: String,
    cwd: PathBuf,
    env: Vec<(String, String)>,
    inherited_path: String,
}

impl Mise {
    fn new(install: &Install, isolation: &Isolation) -> Self {
        let env = isolation.env(&install.store);
        let inherited_path = env
            .iter()
            .find(|(name, _)| name == "PATH")
            .map_or_default(|(_, value)| value.clone());
        Self {
            program: install.binary.to_string_lossy().into_owned(),
            cwd: isolation.config_dir(),
            env,
            inherited_path,
        }
    }

    async fn capture(&self, ctx: &mut StepContext<'_>, args: &[&str]) -> Result<String, ExecError> {
        let words: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        let bytes = ctx
            .capture_bytes((&self.program, &words, &self.cwd, &self.env), false)
            .await?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    async fn json(
        &self,
        ctx: &mut StepContext<'_>,
        isolation: &Isolation,
        args: &[&str],
    ) -> Result<BTreeMap<String, String>, ExecError> {
        let output = self.capture(ctx, args).await?;
        let values: BTreeMap<String, serde_json::Value> =
            serde_json::from_str(&output).map_err(|error| {
                ExecError::Invalid(format!("mise {} output: {error}", args.join(" ")))
            })?;
        Ok(values
            .into_iter()
            .filter_map(|(name, value)| {
                let value = value.as_str()?;
                Some((name, isolation.to_workspace(value)))
            })
            .collect())
    }
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

async fn managed_paths(
    ctx: &mut StepContext<'_>,
    mise: &Mise,
    isolation: &Isolation,
) -> Result<Option<Vec<String>>, ExecError> {
    match mise.capture(ctx, &["env", "--paths"]).await {
        Err(ExecError::Cancelled) => Err(ExecError::Cancelled),
        output => Ok(output.ok().map(|text| {
            text.lines()
                .filter(|line| !line.is_empty())
                .map(|line| isolation.to_workspace(line))
                .collect()
        })),
    }
}

struct Install {
    tools: PathBuf,
    binary: PathBuf,
    store: StoreEnv,
    exports: Vec<(String, String)>,
}

fn data_dir(tools: &Path, _lock: &[u8]) -> PathBuf {
    tools.join("data")
}

fn cargo_home(tools: &Path) -> PathBuf {
    tools.join("cargo")
}

fn rustup_home(tools: &Path) -> PathBuf {
    tools.join("rustup")
}

fn store_env(ctx: &StepContext<'_>, tools: &Path, lock: &str, binary: &Path) -> StoreEnv {
    StoreEnv {
        bin: binary.parent().map_or_default(Path::to_path_buf),
        data: data_dir(tools, lock.as_bytes()),
        shims: ctx.temp.join("mise-shims"),
        cargo_home: cargo_home(tools),
        rustup_home: rustup_home(tools),
    }
}

impl Install {
    fn new(ctx: &StepContext<'_>, tools: &Path, lock: &str, binary: PathBuf) -> Self {
        let store = store_env(ctx, tools, lock, &binary);
        let mut exports = vec![
            (
                "MISE_DATA_DIR".to_owned(),
                store.data.to_string_lossy().into_owned(),
            ),
            (
                "MISE_SHIMS_DIR".to_owned(),
                store.shims.to_string_lossy().into_owned(),
            ),
            (
                "MISE_CACHE_DIR".to_owned(),
                ctx.temp.join("mise-cache").to_string_lossy().into_owned(),
            ),
            ("MISE_AUTO_INSTALL".to_owned(), "false".to_owned()),
            (
                "MISE_CARGO_HOME".to_owned(),
                store.cargo_home.to_string_lossy().into_owned(),
            ),
            (
                "MISE_RUSTUP_HOME".to_owned(),
                store.rustup_home.to_string_lossy().into_owned(),
            ),
        ];
        for (name, value) in [
            (
                "MISE_TRUSTED_CONFIG_PATHS",
                ctx.workspace.to_string_lossy().into_owned(),
            ),
            ("MISE_YES", "1".to_owned()),
        ] {
            if !ctx.env.contains_key(name) && std::env::var_os(name).is_none() {
                exports.push((name.to_owned(), value));
            }
        }
        Self {
            tools: tools.to_path_buf(),
            binary,
            store,
            exports,
        }
    }

    async fn require_read_only(&self) -> Result<(), ExecError> {
        require_read_only_store(&self.tools, &self.store.directories()).await
    }
}

async fn export(
    ctx: &mut StepContext<'_>,
    mise: &Mise,
    isolation: &Isolation,
    install: &Install,
) -> Result<(), ExecError> {
    let redacted = mise
        .json(ctx, isolation, &["env", "--redacted", "--json"])
        .await?;
    for value in redacted.values() {
        ctx.add_mask(value);
    }
    let computed = mise.json(ctx, isolation, &["env", "--json"]).await?;
    let managed = managed_paths(ctx, mise, isolation).await?;
    let github_path = ctx.env.get("GITHUB_PATH").cloned().unwrap_or_default();
    let github_env = ctx.env.get("GITHUB_ENV").cloned().unwrap_or_default();
    let dir = install.store.bin.to_string_lossy().into_owned();
    let mut paths = vec![
        dir.clone(),
        install.store.shims.to_string_lossy().into_owned(),
    ];
    let mut variables: String = install
        .exports
        .iter()
        .map(|(name, value)| env_command(name, value))
        .collect();
    for (name, value) in &computed {
        if name.eq_ignore_ascii_case("PATH") {
            let mut added = managed
                .clone()
                .unwrap_or_else(|| path_additions(value, &mise.inherited_path, &dir));
            require_confined(&added, &install.store.directories()).await?;
            added.reverse();
            paths.extend(added);
        } else if name != "CARGO_HOME" || Path::new(value) != install.store.cargo_home {
            variables.push_str(&env_command(name, value));
        }
    }
    install.require_read_only().await?;
    append(&github_path, &(paths.join("\n") + "\n")).await?;
    append(&github_env, &variables).await
}

async fn mise_action(
    inputs: &MiseInputs,
    facts: &Result<TrustedFacts, String>,
    ctx: &mut StepContext<'_>,
) -> Result<StepResult, ExecError> {
    let tools = store_root().join("tools");
    let binary = locate_binary(inputs, &tools).await?;
    let facts = facts
        .as_ref()
        .map_err(|reason| ExecError::Invalid(reason.clone()))?;
    let accepted = enforce_locked_mise_policy(ctx, facts, &inputs.install_args).await?;
    let install = Install::new(ctx, &tools, &accepted.lock, binary);
    install.require_read_only().await?;
    let isolation = Isolation::create(ctx.temp, ctx.workspace, &accepted).await?;
    let mise = Mise::new(&install, &isolation);
    let loaded = mise.capture(ctx, &["config", "ls", "--json"]).await?;
    require_sole_config(&isolation.config_file(), &loaded)?;
    let mut args = vec!["install".to_owned(), "--locked".to_owned()];
    args.extend(inputs.install_args.split_whitespace().map(str::to_owned));
    let spec = (
        mise.program.as_str(),
        args.as_slice(),
        mise.cwd.as_path(),
        mise.env.as_slice(),
    );
    match ctx.run_exact(spec, false).await? {
        ProcessOutcome::Succeeded => {}
        ProcessOutcome::Cancelled => return Ok(StepResult::Cancelled),
        ProcessOutcome::Failed => {
            ctx.log(NOT_IN_STORE);
            return Ok(StepResult::Failed);
        }
    }
    install.require_read_only().await?;
    export(ctx, &mise, &isolation, &install).await?;
    isolation.remove().await?;
    Ok(StepResult::Succeeded)
}

#[derive(Debug)]
struct MiseAction {
    facts: Result<TrustedFacts, String>,
}

impl StepAdapter for MiseAction {
    fn handles(&self, reference: &str) -> bool {
        reference.starts_with(PREFIX)
    }

    fn run<'a>(&'a self, ctx: &'a mut StepContext<'_>) -> StepFuture<'a> {
        async move {
            require_pinned_revision(&ctx.reference)?;
            let given = ctx
                .inputs
                .iter()
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect();
            let inputs = MiseInputs::parse(&given)?;
            require_pinned_version(&inputs.version)?;
            ctx.set_output("cache-hit", "false");
            mise_action(&inputs, &self.facts, ctx).await
        }
        .boxed()
    }
}
