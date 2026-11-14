// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::fs;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use super::checkout::{copy_checkout, git};
use super::env::{Addresses, DockerCli, Environment, STANDIN_IMAGE, request, words};
use crate::cli::{EnvironmentArgs, RunArgs};
use crate::event::{NO_COMMIT, Push, Repository, for_standin};
use crate::scenario::{
    Outcome, Scenario, Step, distinct, inside, overlapping, plan, summary, verdict,
};
use crate::side::{Side, fork_heads, push_event, refuse_fork_branches};
use crate::standin;

mod outcome;
mod pulls;
mod steps;

#[derive(Debug)]
struct Driver<'a> {
    args: &'a RunArgs,
    env: &'a Environment<DockerCli>,
    addresses: Addresses,
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
                    teardown.map_or_else(|e| format!("; teardown failed: {e}"), |()| String::new());
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
    if slots.is_closed() {
        return Err(io::Error::other("interrupted before it started"));
    }
    let (outcomes, up_at, up_seconds, replayed) = tokio::select! {
        session = session(args, &environment, &env, repo, &scenario, tag) => session,
        _ = tokio::signal::ctrl_c() => (Vec::new(), 0, 0.0, Err(io::Error::other("interrupted"))),
    };
    let stopped = env.down().await;
    print!("{}", verdict(tag, &outcomes, up_at, up_seconds));
    replayed?;
    stopped?;
    if !outcomes.iter().all(Outcome::passed) {
        return Err(io::Error::other("a run did not succeed"));
    }
    Ok(())
}

async fn session<'a>(
    args: &'a RunArgs,
    environment: &EnvironmentArgs,
    env: &'a Environment<DockerCli>,
    repo: PathBuf,
    scenario: &Scenario,
    tag: &'a str,
) -> (Vec<Outcome>, u64, f64, io::Result<()>) {
    let up_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let clock = Instant::now();
    let addresses = match env.up(&repo, environment).await {
        Ok(addresses) => addresses,
        Err(error) => return (Vec::new(), up_at, clock.elapsed().as_secs_f64(), Err(error)),
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
        let push = Push {
            branch,
            before,
            after: &after,
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

    async fn get<T: DeserializeOwned>(&self, side: Side, path: &str) -> io::Result<T> {
        let proxy = self.addresses.standin(side)?;
        let bytes = request(proxy, "GET", path, "")
            .await
            .ok_or_else(|| io::Error::other(format!("runner.server does not answer {path}")))?;
        serde_json::from_slice(&bytes).map_err(io::Error::other)
    }

    async fn send(
        &mut self,
        event: &str,
        payload: &Value,
        git_ref: &str,
        sha: &str,
    ) -> io::Result<()> {
        self.events += 1;
        let name = format!("{}-{event}", self.events);
        let dir = self.env.dir.display();
        fs::write(
            format!("{dir}/events/{name}.json"),
            for_standin(payload.clone()).to_string(),
        )
        .await?;
        let runs = "/_apis/v1/Message/workflow/runs";
        let side = Side::of(payload);
        let known: Vec<standin::Run> = self.get(side, runs).await?;
        let (repo, events) = (
            format!("{}:/repo", self.repo.display()),
            format!("{dir}/events:/events:ro"),
        );
        let payload_path = format!("/events/{name}.json");
        let mut client = words("-w /repo --entrypoint dotnet");
        client.extend(["-v", &repo, "-v", &events]);
        client.push(STANDIN_IMAGE);
        let call = standin::ClientCall {
            event,
            payload: &payload_path,
            git_ref,
            sha,
            repository: &self.args.repository,
            workflow: self.args.workflow.as_deref(),
        };
        client.extend(call.args());
        let log = self.env.dir.join(format!("logs/{name}.log"));
        let succeeded = self.env.client(payload, &client, &log).await?;
        let mut created = false;
        for run in self.get::<Vec<standin::Run>>(side, runs).await? {
            if known.iter().any(|old| old.id == run.id) {
                continue;
            }
            created = true;
            if standin::conclusion(run.result.as_deref()) == Some("skipped") {
                println!(
                    "velnor-local {}: {event} does not trigger {}",
                    self.tag, run.file_name
                );
                continue;
            }
            let outcome = self.outcome(run, side).await?;
            println!("velnor-local {}: {}", self.tag, outcome.line());
            self.outcomes.push(outcome);
        }
        if !succeeded && !created {
            return Err(io::Error::other(format!(
                "Runner.Client failed; see logs/{name}.log"
            )));
        }
        Ok(())
    }
}
