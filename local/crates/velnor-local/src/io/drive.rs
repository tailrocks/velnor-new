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

use super::env::{Addresses, DockerCli, Environment, STANDIN_IMAGE, program, request, words};
use crate::cli::RunArgs;
use crate::event::{NO_COMMIT, PullRequest, Push, Repository};
use crate::scenario::{Outcome, Scenario, Step, overlapping, verdict};
use crate::standin;

const GIT_CONFIG: &str =
    "-c user.name=velnor-local -c user.email=velnor-local@localhost -c commit.gpgsign=false";

#[derive(Debug)]
struct Driver<'a> {
    args: &'a RunArgs,
    env: &'a Environment<DockerCli>,
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
    let scenario = Scenario::parse(&fs::read(&path).await?)
        .map_err(io::Error::other)?
        .on_default_branch(&args.environment.default_branch);
    let values = [args.repository.as_str(), &args.environment.default_branch];
    scenario
        .refuse_options(values.into_iter().chain(args.workflow.as_deref()))
        .map_err(io::Error::other)?;
    let env = Environment::at(&args.environment.dir, DockerCli).await?;
    let checkout = fs::canonicalize(&args.repo).await?;
    if overlapping(&env.dir, &checkout) {
        return Err(io::Error::other(
            "the state directory and the checkout overlap",
        ));
    }
    let repo = env.dir.join("repo");
    copy_checkout(&args.repo, &repo, &args.environment.default_branch).await?;
    for directory in ["cache", "api", "events", "logs"] {
        match fs::remove_dir_all(env.dir.join(directory)).await {
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
        fs::create_dir_all(env.dir.join(directory)).await?;
    }
    let session = tokio::select! {
        session = session(&args, &env, repo, &scenario) => session,
        _ = tokio::signal::ctrl_c() => Err(io::Error::other("interrupted")),
    };
    let stopped = env.down().await;
    let (outcomes, up_at, up_seconds) = session?;
    stopped?;
    print!("{}", verdict(&args.scenario, &outcomes, up_at, up_seconds));
    if outcomes.is_empty() || !outcomes.iter().all(Outcome::passed) {
        return Err(io::Error::other("a run did not succeed"));
    }
    Ok(())
}

async fn session(
    args: &RunArgs,
    env: &Environment<DockerCli>,
    repo: PathBuf,
    scenario: &Scenario,
) -> io::Result<(Vec<Outcome>, u64, f64)> {
    let up_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let clock = Instant::now();
    let addresses = env.up(&repo, &args.environment).await?;
    let up_seconds = clock.elapsed().as_secs_f64();
    println!("velnor-local: {} is up in {up_seconds:.1} s", env.name);
    let mut driver = Driver {
        args,
        env,
        addresses,
        repo,
        pushed: BTreeMap::new(),
        pulls: Vec::new(),
        events: 0,
        outcomes: Vec::new(),
    };
    driver.replay(&scenario.steps).await?;
    Ok((driver.outcomes, up_at, up_seconds))
}

async fn git(repo: &Path, args: &[&str], input: Option<&[u8]>) -> io::Result<Vec<u8>> {
    let repo = repo.to_string_lossy();
    let head = [&["-C", repo.as_ref()], &words(GIT_CONFIG)[..]].concat();
    program("git", &[&head[..], args].concat(), input).await
}

async fn copy_checkout(checkout: &Path, copy: &Path, default: &str) -> io::Result<()> {
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
    git(copy, &["checkout", "-q", "-B", default], None).await?;
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

    async fn git(&self, args: &[&str]) -> io::Result<String> {
        let stdout = git(&self.repo, args, None).await?;
        Ok(String::from_utf8_lossy(&stdout).trim().to_owned())
    }

    async fn commit(&self, branch: &str, append: &str, line: &str) -> io::Result<()> {
        if self.git(&["switch", "-q", branch]).await.is_err() {
            let base = &self.args.environment.default_branch;
            self.git(&["switch", "-q", "-c", branch, base]).await?;
        }
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
            .await
    }

    async fn open(&mut self, head: &str, base: &str) -> io::Result<()> {
        let number = self.pulls.len() as u64 + 1;
        let base_sha = self.git(&["rev-parse", base]).await?;
        let head_sha = self.git(&["rev-parse", head]).await?;
        let range = format!("{base_sha}...{head_sha}");
        let changed = self.git(&["diff", "--name-only", &range]).await?;
        self.git(&["switch", "-q", "--detach", &base_sha]).await?;
        self.git(&["merge", "-q", "--no-ff", "--no-edit", &head_sha])
            .await?;
        let merge = self.git(&["rev-parse", "HEAD"]).await?;
        let pull = format!("refs/pull/{number}");
        self.git(&["update-ref", &format!("{pull}/merge"), &merge])
            .await?;
        self.git(&["update-ref", &format!("{pull}/head"), &head_sha])
            .await?;
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
        let before = self.outcomes.len();
        for run in self.get::<Vec<standin::Run>>(runs).await? {
            if known.iter().all(|old| old.id != run.id) {
                let outcome = self.outcome(run).await?;
                println!("velnor-local: {}", outcome.line());
                self.outcomes.push(outcome);
            }
        }
        if !status.success() && self.outcomes.len() == before {
            return Err(io::Error::other(format!(
                "Runner.Client failed; see logs/{name}.log"
            )));
        }
        Ok(())
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
        })
    }
}
