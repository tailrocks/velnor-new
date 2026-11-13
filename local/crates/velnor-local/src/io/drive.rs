// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tokio::fs;
use tokio::process::Command;

use super::env::{Addresses, Environment, STANDIN_IMAGE, program, request, words};
use crate::cli::RunArgs;
use crate::event::{NO_COMMIT, PullRequest, Push, Repository};
use crate::scenario::{Outcome, Scenario, Step, verdict};
use crate::standin;

const GIT_CONFIG: &str =
    "-c user.name=velnor-local -c user.email=velnor-local@localhost -c commit.gpgsign=false";

#[derive(Debug)]
struct Driver<'a> {
    args: &'a RunArgs,
    env: &'a Environment,
    addresses: Addresses,
    repo: PathBuf,
    pushed: BTreeMap<String, String>,
    pulls: Vec<Value>,
    events: usize,
    outcomes: Vec<Outcome>,
}

pub(super) async fn run(args: RunArgs) -> io::Result<()> {
    let path = if Path::new(&args.scenario)
        .extension()
        .is_some_and(|ext| ext == "json")
    {
        PathBuf::from(&args.scenario)
    } else {
        args.scenarios.join(format!("{}.json", args.scenario))
    };
    let scenario = Scenario::parse(&fs::read(&path).await?).map_err(io::Error::other)?;
    let env = Environment::at(&args.environment.dir).await?;
    let repo = env.dir.join("repo");
    copy_checkout(&args.repo, &repo).await?;
    for directory in ["cache", "api", "events", "logs", "metrics"] {
        let _ = fs::remove_dir_all(env.dir.join(directory)).await;
        fs::create_dir_all(env.dir.join(directory)).await?;
    }
    let up_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let clock = Instant::now();
    let addresses = env.up(&repo, &args.environment).await?;
    let up_seconds = clock.elapsed().as_secs_f64();
    println!("velnor-local: {} is up in {up_seconds:.1} s", env.name);
    let mut driver = Driver {
        args: &args,
        env: &env,
        addresses,
        repo,
        pushed: BTreeMap::new(),
        pulls: Vec::new(),
        events: 0,
        outcomes: Vec::new(),
    };
    let replayed = driver.replay(&scenario.steps).await;
    let stopped = env.down().await;
    replayed?;
    stopped?;
    let outcomes = driver.outcomes;
    print!("{}", verdict(&args.scenario, &outcomes, up_at, up_seconds));
    if outcomes.is_empty() || !outcomes.iter().all(Outcome::passed) {
        return Err(io::Error::other("a run did not succeed"));
    }
    Ok(())
}

async fn git(repo: &Path, args: &[&str], input: Option<&[u8]>) -> io::Result<Vec<u8>> {
    let repo = repo.to_string_lossy();
    let head = [&["-C", repo.as_ref()], &words(GIT_CONFIG)[..]].concat();
    program("git", &[&head[..], args].concat(), input).await
}

async fn copy_checkout(checkout: &Path, copy: &Path) -> io::Result<()> {
    if fs::metadata(copy).await.is_ok() {
        fs::remove_dir_all(copy).await?;
    }
    git(
        checkout,
        &["clone", "-q", ".", &copy.to_string_lossy()],
        None,
    )
    .await?;
    let changes = git(checkout, &["diff", "HEAD", "--binary"], None).await?;
    if !changes.is_empty() {
        git(copy, &["apply", "--index"], Some(&changes)).await?;
        git(copy, &["commit", "-q", "-m", "Uncommitted changes"], None).await?;
    }
    Ok(())
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
                Step::Push(branch) => self.push(branch).await?,
                Step::OpenPullRequest { head, base } => self.open(head, base).await?,
            }
        }
        Ok(())
    }

    fn repository(&self) -> Repository<'_> {
        Repository {
            full_name: &self.args.repository,
            default_branch: &self.args.environment.default_branch,
        }
    }

    async fn git(&self, args: &str) -> io::Result<String> {
        let stdout = git(&self.repo, &words(args), None).await?;
        Ok(String::from_utf8_lossy(&stdout).trim().to_owned())
    }

    async fn commit(&self, branch: &str, append: &str, line: &str) -> io::Result<()> {
        if self.git(&format!("switch -q {branch}")).await.is_err() {
            let base = &self.args.environment.default_branch;
            self.git(&format!("switch -q -c {branch} {base}")).await?;
        }
        let file = self.repo.join(append);
        let text = fs::read_to_string(&file).await.unwrap_or_default();
        fs::write(&file, format!("{text}{line}\n")).await?;
        let message = format!("Append to {append}");
        git(&self.repo, &["commit", "-q", "-a", "-m", &message], None).await?;
        Ok(())
    }

    async fn push(&mut self, branch: &str) -> io::Result<()> {
        self.git(&format!("switch -q {branch}")).await?;
        let after = self.git("rev-parse HEAD").await?;
        let before = self.pushed.get(branch).map_or(NO_COMMIT, String::as_str);
        let payload = self.repository().push(&Push {
            branch,
            before,
            after: &after,
        });
        self.pushed.insert(branch.to_owned(), after.clone());
        self.send("push", &payload, &format!("refs/heads/{branch}"), &after)
            .await
    }

    async fn open(&mut self, head: &str, base: &str) -> io::Result<()> {
        let number = self.pulls.len() as u64 + 1;
        let base_sha = self.git(&format!("rev-parse {base}")).await?;
        let head_sha = self.git(&format!("rev-parse {head}")).await?;
        let changed = self
            .git(&format!("diff --name-only {base_sha}...{head_sha}"))
            .await?;
        self.git(&format!("switch -q --detach {base_sha}")).await?;
        self.git(&format!("merge -q --no-ff --no-edit {head_sha}"))
            .await?;
        let merge = self.git("rev-parse HEAD").await?;
        self.pulls.push(json!({
            "number": number,
            "state": "open",
            "base": {"ref": base, "sha": base_sha},
            "head": {"ref": head, "sha": head_sha},
        }));
        let pulls = Value::Array(self.pulls.clone()).to_string();
        fs::write(self.env.dir.join("api/pulls.json"), pulls).await?;
        let registration = json!({"base_branch": base}).to_string();
        let path = format!("/_velnor/pull-requests/{number}");
        request(&self.addresses.cache, "PUT", &path, &registration)
            .await
            .ok_or_else(|| io::Error::other(format!("cannot register pull request {number}")))?;
        let payload = self.repository().pull_request(&PullRequest {
            number,
            base: (base, &base_sha),
            head: (head, &head_sha),
            changed_files: changed.lines().count(),
        });
        self.send(
            "pull_request",
            &payload,
            &format!("refs/pull/{number}/merge"),
            &merge,
        )
        .await
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
        fs::write(format!("{dir}/events/{name}.json"), payload.to_string()).await?;
        let runs = "/_apis/v1/Message/workflow/runs";
        let known: Vec<standin::Run> = self.get(runs).await?;
        let mut client = format!(
            "run --rm --network {} -v {}:/repo -v {dir}/events:/events:ro -w /repo \
             --entrypoint dotnet {STANDIN_IMAGE} /opt/runner.server/Runner.Client.dll \
             --server http://runner-server --event {event} --payload /events/{name}.json \
             --ref {git_ref} --sha {sha} --repository {}",
            self.env.name,
            self.repo.display(),
            self.args.repository
        );
        if let Some(workflow) = &self.args.workflow {
            client = format!("{client} -W {workflow}");
        }
        let log = std::fs::File::create(format!("{dir}/logs/{name}.log"))?;
        Command::new("docker")
            .args(words(&client))
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log)
            .status()
            .await?;
        for run in self.get::<Vec<standin::Run>>(runs).await? {
            if known.iter().all(|old| old.id != run.id) {
                let mut outcome = self.outcome(run).await?;
                outcome.metrics = self.collect(outcome.id).await;
                println!("velnor-local: {}", outcome.line());
                self.outcomes.push(outcome);
            }
        }
        Ok(())
    }

    async fn collect(&self, id: u64) -> Result<PathBuf, String> {
        let metrics = self.env.dir.join(format!("metrics/{id}.json"));
        let collect = format!("bench collect {id} --repo {} -o", self.args.repository);
        let collected = Command::new(&self.args.velnor_ci)
            .args(words(&collect))
            .arg(&metrics)
            .env("GITHUB_API_URL", &self.addresses.stub_url)
            .env("GITHUB_TOKEN", "velnor-local")
            .output()
            .await
            .map_err(|error| format!("{}: {error}", self.args.velnor_ci.display()))?;
        if collected.status.success() {
            return Ok(metrics);
        }
        Err(String::from_utf8_lossy(&collected.stderr).trim().to_owned())
    }

    async fn outcome(&self, run: standin::Run) -> io::Result<Outcome> {
        let jobs: Vec<standin::Job> = self
            .get(&format!("/_apis/v1/Message?runid={}", run.id))
            .await?;
        let mut started = None;
        for timeline in jobs.iter().filter_map(|job| job.time_line_id.as_ref()) {
            let records: Vec<standin::Record> =
                self.get(&format!("/_apis/v1/Timeline/{timeline}")).await?;
            started = started
                .into_iter()
                .chain(standin::job_started(&records))
                .min();
        }
        Ok(Outcome {
            id: run.id,
            workflow: jobs
                .first()
                .map_or(run.display_name, |job| job.workflowname.clone()),
            event: run.event_name.unwrap_or_default(),
            conclusion: standin::conclusion(run.result.as_deref()),
            started,
            metrics: Err("not collected".to_owned()),
        })
    }
}
