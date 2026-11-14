// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::fs;
use tokio::process::Command;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use super::checkout::{copy_checkout, git};
use super::env::{Addresses, DockerCli, Environment, STANDIN_IMAGE, request, words};
use crate::cli::RunArgs;
use crate::event::{NO_COMMIT, Push, Repository, for_standin};
use crate::scenario::{Outcome, Scenario, Step, overlapping, verdict};
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
}

pub(super) async fn run(args: RunArgs) -> io::Result<()> {
    let args = Arc::new(args);
    let several = args.scenario.len() > 1;
    let slots = Arc::new(Semaphore::new(usize::from(args.parallel)));
    let mut running = JoinSet::new();
    for name in args.scenario.clone() {
        let (args, slots) = (Arc::clone(&args), Arc::clone(&slots));
        running.spawn(async move {
            let _slot = slots.acquire().await.map_err(io::Error::other);
            let stem = Path::new(&name)
                .file_stem()
                .map_or_else(|| name.clone(), |stem| stem.to_string_lossy().into_owned());
            let dir = if several {
                args.environment.dir.join(&stem)
            } else {
                args.environment.dir.clone()
            };
            let result = run_one(&args, &name, &stem, &dir).await;
            result.map_err(|error| format!("{stem}: {error}"))
        });
    }
    let mut failed = Vec::new();
    while let Some(joined) = running.join_next().await {
        if let Err(error) = joined
            .map_err(|error| error.to_string())
            .and_then(|result| result)
        {
            failed.push(error);
        }
    }
    if failed.is_empty() {
        return Ok(());
    }
    Err(io::Error::other(failed.join("; ")))
}

async fn run_one(args: &RunArgs, scenario: &str, tag: &str, dir: &Path) -> io::Result<()> {
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
    let env = Environment::at(dir, DockerCli).await?;
    let checkout = fs::canonicalize(&args.repo).await?;
    if overlapping(&env.dir, &checkout) {
        return Err(io::Error::other(
            "the state directory and the checkout overlap",
        ));
    }
    let repo = env.dir.join("repo");
    copy_checkout(&args.repo, &repo, &args.environment.default_branch).await?;
    for directory in ["cache", "api", "events", "logs", "metrics"] {
        match fs::remove_dir_all(env.dir.join(directory)).await {
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
        fs::create_dir_all(env.dir.join(directory)).await?;
    }
    let session = tokio::select! {
        session = session(args, &env, repo, &scenario, tag) => session,
        _ = tokio::signal::ctrl_c() => Err(io::Error::other("interrupted")),
    };
    let stopped = env.down().await;
    let (outcomes, up_at, up_seconds) = session?;
    stopped?;
    print!("{}", verdict(tag, &outcomes, up_at, up_seconds));
    if !outcomes.iter().all(Outcome::passed) {
        return Err(io::Error::other("a run did not succeed"));
    }
    Ok(())
}

async fn session<'a>(
    args: &'a RunArgs,
    env: &'a Environment<DockerCli>,
    repo: PathBuf,
    scenario: &Scenario,
    tag: &'a str,
) -> io::Result<(Vec<Outcome>, u64, f64)> {
    let up_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let clock = Instant::now();
    let addresses = env.up(&repo, &args.environment).await?;
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
    };
    let branches = driver
        .git(&["for-each-ref", "--format=%(refname:short)", "refs/heads"])
        .await?;
    for branch in branches.lines() {
        let parent = format!("{branch}^");
        if let Ok(before) = driver.git(&["rev-parse", "--verify", "-q", &parent]).await {
            driver.pushed.insert(branch.to_owned(), before);
        }
    }
    driver.replay(&scenario.steps).await?;
    Ok((driver.outcomes, up_at, up_seconds))
}

impl Driver<'_> {
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
                Step::OpenPullRequest { head, base } => self.open(head, base).await?,
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
        let parent = fs::canonicalize(file.parent().unwrap_or(&self.repo)).await?;
        if !parent.starts_with(fs::canonicalize(&self.repo).await?) {
            return Err(io::Error::other(format!("{append} leaves the repository")));
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
        let payload = self.repository().push(&Push {
            branch,
            before,
            after: &after,
        });
        self.pushed.insert(branch.to_owned(), after.clone());
        self.send("push", &payload, &format!("refs/heads/{branch}"), &after)
            .await?;
        self.synchronize(branch, &after).await
    }

    async fn get<T: DeserializeOwned>(&self, path: &str) -> io::Result<T> {
        let bytes = request(&self.addresses.cache, "GET", path, "")
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
        let known: Vec<standin::Run> = self.get(runs).await?;
        let label = format!("velnor-local={}", self.env.name);
        let (repo, events) = (
            format!("{}:/repo", self.repo.display()),
            format!("{dir}/events:/events:ro"),
        );
        let payload_path = format!("/events/{name}.json");
        let mut client = words("run --rm -w /repo --entrypoint dotnet");
        client.extend([
            "--label",
            &label,
            "--network",
            &self.env.name,
            "-v",
            &repo,
            "-v",
            &events,
        ]);
        client.push(STANDIN_IMAGE);
        client.extend(words(
            "/opt/runner.server/Runner.Client.dll --server http://runner-server",
        ));
        client.extend([
            "--event",
            event,
            "--payload",
            &payload_path,
            "--ref",
            git_ref,
            "--sha",
            sha,
        ]);
        client.extend(["--repository", &self.args.repository]);
        if let Some(workflow) = &self.args.workflow {
            client.extend(["-W", workflow]);
        }
        let log = std::fs::File::create(format!("{dir}/logs/{name}.log"))?;
        let status = Command::new("docker")
            .args(&client)
            .kill_on_drop(true)
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log)
            .status()
            .await?;
        let mut created = false;
        for run in self.get::<Vec<standin::Run>>(runs).await? {
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
            let outcome = self.outcome(run).await?;
            println!("velnor-local {}: {}", self.tag, outcome.line());
            self.outcomes.push(outcome);
        }
        if !status.success() && !created {
            return Err(io::Error::other(format!(
                "Runner.Client failed; see logs/{name}.log"
            )));
        }
        Ok(())
    }
}
