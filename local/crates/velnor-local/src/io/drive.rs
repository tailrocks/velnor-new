// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use tokio::fs;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use super::checkout::{changes, copy_checkout, git};
use super::env::{Addresses, DockerCli, Environment};
use super::proxy::Proxy;
use crate::cli::{EnvironmentArgs, RunArgs};
use crate::event::{NO_COMMIT, Push, Repository};
use crate::pairs::{DECISIONS, decisions};
use crate::scenario::{
    Outcome, Scenario, Step, distinct, inside, keeps_environment, overlapping, plan, summary,
};
use crate::side::{fork_heads, push_event, refuse_fork_branches};
use crate::verdict::{Check, conclude, verdict};

mod outcome;
mod pulls;
mod refusal;
mod send;
mod steps;

#[derive(Debug)]
struct Driver<'a> {
    args: &'a RunArgs,
    env: &'a Environment<DockerCli>,
    addresses: Addresses,
    authority: Proxy,
    repo: PathBuf,
    pushed: BTreeMap<String, String>,
    pulls: Vec<pulls::Pull>,
    events: usize,
    outcomes: Vec<Outcome>,
    tag: &'a str,
    fork_heads: BTreeSet<String>,
}

pub(super) async fn run(args: RunArgs) -> io::Result<()> {
    let mut planned = plan(&args.scenario, &args.environment.dir).map_err(io::Error::other)?;
    for (_, _, dir) in &mut planned {
        fs::create_dir_all(&*dir).await?;
        *dir = fs::canonicalize(&*dir).await?;
    }
    distinct(&planned).map_err(io::Error::other)?;
    let args = Arc::new(args);
    let slots = Arc::new(Semaphore::new(usize::from(args.parallel)));
    let closer = Arc::clone(&slots);
    let interrupt = tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            closer.close();
        }
    });
    let mut running = JoinSet::new();
    let mut spawned = BTreeMap::new();
    for (name, stem, dir) in planned {
        let (args, slots) = (Arc::clone(&args), Arc::clone(&slots));
        let task = (stem.clone(), dir.clone());
        let handle = running.spawn(async move {
            let Ok(_slot) = slots.acquire().await else {
                return (stem, Err("interrupted before it started".to_owned()));
            };
            let result = run_one(&args, &name, &stem, &dir, &slots).await;
            (stem, result.map_err(|error| error.to_string()))
        });
        spawned.insert(handle.id(), task);
    }
    let mut results = Vec::new();
    while let Some(joined) = running.join_next_with_id().await {
        match joined {
            Ok((_, result)) => results.push(result),
            Err(error) => {
                let (stem, dir) = spawned.remove(&error.id()).unwrap_or_default();
                let teardown = match Environment::at(&dir, DockerCli).await {
                    Ok(env) => env.down().await,
                    Err(error) => Err(error),
                };
                let teardown =
                    teardown.map_or_else(|e| format!("; teardown failed: {e}"), |_| String::new());
                results.push((stem, Err(format!("stopped: {error}{teardown}"))));
            }
        }
    }
    interrupt.abort();
    summary(&results).map_err(io::Error::other)
}

async fn run_one(
    args: &RunArgs,
    scenario: &str,
    tag: &str,
    dir: &Path,
    slots: &Semaphore,
) -> io::Result<()> {
    let path = if Path::new(scenario)
        .extension()
        .is_some_and(|ext| ext == "json")
    {
        PathBuf::from(scenario)
    } else {
        args.scenarios.join(format!("{scenario}.json"))
    };
    let scenario = Scenario::parse(&fs::read(&path).await?)
        .map_err(io::Error::other)?
        .on_default_branch(&args.environment.default_branch);
    let values = [args.repository.as_str(), &args.environment.default_branch];
    scenario
        .refuse_options(values.into_iter().chain(args.workflow.as_deref()))
        .map_err(io::Error::other)?;
    refuse_fork_branches(&scenario.steps, &args.environment.default_branch)
        .map_err(io::Error::other)?;
    let env = Environment::at(dir, DockerCli).await?;
    if !inside(&fs::canonicalize(&args.environment.dir).await?, &env.dir) {
        return Err(io::Error::other(format!(
            "{} leads outside the state directory",
            dir.display()
        )));
    }
    let checkout = fs::canonicalize(&args.repo).await?;
    if overlapping(&env.dir, &checkout) {
        return Err(io::Error::other(
            "the state directory and the checkout overlap",
        ));
    }
    let repo = env.dir.join("repo");
    copy_checkout(&args.repo, &repo, &args.environment.default_branch, tag).await?;
    super::local_ci::pin_workflows(&args.environment, &repo, tag).await?;
    for directory in ["cache", "api", "events", "logs", "metrics"] {
        match fs::remove_dir_all(env.dir.join(directory)).await {
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
        fs::create_dir_all(env.dir.join(directory)).await?;
    }
    let mut environment = args.environment.clone();
    if scenario.has_fork() {
        environment.fork_runners = environment.fork_runners.max(1);
    }
    let pairs = super::supervisor::pairs(&environment).await?;
    if slots.is_closed() {
        return Err(io::Error::other("interrupted before it started"));
    }
    let (outcomes, up_at, up_seconds, replayed) = tokio::select! {
        session = session(args, &environment, &env, repo, &scenario, tag, pairs.as_ref()) => session,
        _ = tokio::signal::ctrl_c() => (
            Vec::new(), 0, 0.0, Err(io::Error::new(io::ErrorKind::Interrupted, "interrupted")),
        ),
    };
    let interrupted = replayed
        .as_ref()
        .is_err_and(|error| error.kind() == io::ErrorKind::Interrupted);
    let kept = keeps_environment(args.keep, interrupted, replayed.is_err(), outcomes.len());
    let stopped = if kept { Ok([0; 3]) } else { env.down().await };
    let check = job_start_check(&env.dir, pairs.is_some()).await?;
    let warning =
        crate::authority::warning(pairs.as_ref().is_some_and(|pairs| pairs.policy.is_some()));
    print!(
        "{}",
        verdict(tag, &outcomes, (up_at, up_seconds), &check, warning)
    );
    if kept {
        println!(
            "velnor-local {tag}: {} is kept up; `velnor-local down --dir {}` removes it",
            env.name,
            env.dir.display()
        );
    }
    replayed?;
    stopped?;
    conclude(&check, &outcomes).map_err(io::Error::other)
}

async fn job_start_check(dir: &Path, pairs: bool) -> io::Result<Check> {
    if !pairs {
        return Ok(Check::Plain);
    }
    let text = fs::read_to_string(dir.join("runner").join(DECISIONS)).await;
    let text = match text {
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        read => read?,
    };
    decisions(&text).map(Check::Pairs).map_err(io::Error::other)
}

async fn session<'a>(
    args: &'a RunArgs,
    environment: &EnvironmentArgs,
    env: &'a Environment<DockerCli>,
    repo: PathBuf,
    scenario: &Scenario,
    tag: &'a str,
    pairs: Option<&super::supervisor::Pairs>,
) -> (Vec<Outcome>, u64, f64, io::Result<()>) {
    println!("velnor-local {tag}: {}", super::run::runner_mode(pairs));
    let up_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let clock = Instant::now();
    let addresses = match env.up(&repo, environment).await {
        Ok(addresses) => addresses,
        Err(error) => return (Vec::new(), up_at, clock.elapsed().as_secs_f64(), Err(error)),
    };
    let authority = match super::supervisor::start_with(&env.dir, pairs, &repo).await {
        Ok(authority) => {
            for pair in &authority.pairs {
                println!("velnor-local {tag}: pair {pair} listening");
            }
            Proxy::authorized(&authority.address, &authority.token)
        }
        Err(error) => {
            let _ = env.down().await;
            return (Vec::new(), up_at, clock.elapsed().as_secs_f64(), Err(error));
        }
    };
    let up_seconds = clock.elapsed().as_secs_f64();
    println!(
        "velnor-local {tag}: {} is up in {up_seconds:.1} s",
        env.name
    );
    let mut driver = Driver {
        args,
        env,
        addresses,
        authority,
        repo,
        pushed: BTreeMap::new(),
        pulls: Vec::new(),
        events: 0,
        outcomes: Vec::new(),
        tag,
        fork_heads: fork_heads(&scenario.steps),
    };
    let replayed = driver.start(&scenario.steps).await;
    (driver.outcomes, up_at, up_seconds, replayed)
}

impl Driver<'_> {
    async fn start(&mut self, steps: &[Step]) -> io::Result<()> {
        let branches = self
            .git(&["for-each-ref", "--format=%(refname:short)", "refs/heads"])
            .await?;
        for branch in branches.lines() {
            let parent = format!("{branch}^");
            if let Ok(before) = self.git(&["rev-parse", "--verify", "-q", &parent]).await {
                self.pushed.insert(branch.to_owned(), before);
            }
        }
        self.replay(steps).await
    }

    async fn replay(&mut self, steps: &[Step]) -> io::Result<()> {
        for step in steps {
            match step {
                Step::Commit {
                    branch,
                    append,
                    line,
                } => self.commit(branch, append, line).await?,
                Step::UpdateLock {
                    branch,
                    package,
                    workspace,
                } => self.update_lock(branch, package, workspace).await?,
                Step::Push(branch) => self.push(branch).await?,
                Step::OpenPullRequest { head, base, fork } => self.open(head, base, *fork).await?,
                Step::MergePullRequest(head) => self.merge(head).await?,
                Step::ClosePullRequest(head) => self.close(head).await?,
                Step::MergeGroup(head) => self.merge_group(head).await?,
                Step::Schedule(cron) => self.schedule(cron).await?,
                Step::WorkflowDispatch { branch, inputs } => self.dispatch(branch, inputs).await?,
            }
        }
        Ok(())
    }

    async fn on_branch(&self, branch: &str) -> io::Result<()> {
        if self.git(&["switch", "-q", branch]).await.is_err() {
            let base = &self.args.environment.default_branch;
            self.git(&["switch", "-q", "-c", branch, base]).await?;
        }
        Ok(())
    }

    fn repository(&self) -> Repository<'_> {
        Repository {
            full_name: &self.args.repository,
            default_branch: &self.args.environment.default_branch,
            id: self.args.repository_id,
        }
    }

    async fn git(&self, args: &[&str]) -> io::Result<String> {
        let stdout = git(&self.repo, args, None).await?;
        Ok(String::from_utf8_lossy(&stdout).trim().to_owned())
    }

    async fn commit(&self, branch: &str, append: &str, line: &str) -> io::Result<()> {
        self.on_branch(branch).await?;
        let file = self.repo.join(append);
        let root = fs::canonicalize(&self.repo).await?;
        let mut dir = self.repo.clone();
        for part in Path::new(append)
            .parent()
            .into_iter()
            .flat_map(Path::components)
        {
            dir.push(part);
            if fs::symlink_metadata(&dir).await.is_err() {
                fs::create_dir(&dir).await?;
            }
            if !fs::canonicalize(&dir).await?.starts_with(&root) {
                return Err(io::Error::other(format!("{append} leaves the repository")));
            }
        }
        let mut bytes = match fs::read(&file).await {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
            read => read?,
        };
        bytes.extend_from_slice(format!("{line}\n").as_bytes());
        fs::write(&file, bytes).await?;
        let message = format!("Append to {append}");
        self.git(&["add", "-A"]).await?;
        self.git(&["commit", "-q", "-m", &message]).await?;
        Ok(())
    }

    async fn push(&mut self, branch: &str) -> io::Result<()> {
        self.git(&["switch", "-q", branch]).await?;
        let after = self.git(&["rev-parse", "HEAD"]).await?;
        let before = self.pushed.get(branch).map_or(NO_COMMIT, String::as_str);
        let commits = changes(&self.repo, branch, before, &after).await?;
        let push = Push {
            branch,
            before,
            after: &after,
            commits: &commits,
        };
        let payload = push_event(&self.repository(), &push, &self.fork_heads);
        self.pushed.insert(branch.to_owned(), after.clone());
        let moved = self.retarget(branch, &after).await?;
        if let Some(payload) = payload {
            self.send("push", &payload, &format!("refs/heads/{branch}"), &after)
                .await?;
        } else {
            println!(
                "velnor-local {}: {branch} is a fork's branch, so the push reaches only its pull request",
                self.tag
            );
        }
        self.synchronize(moved).await
    }
}
