// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, OpenOptions};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use crate::exec_support::{Ran, Scratch, fixture_path};

pub(crate) const REPOSITORY_ID: u64 = 4242;

pub(crate) struct Store {
    scratch: Scratch,
}

pub(crate) struct Entry {
    pub(crate) key: String,
    pub(crate) dir: PathBuf,
}

pub(crate) fn seed(content: &str) -> String {
    format!("mkdir -p data && printf {content} > data/a.txt")
}

const REWRITE_EVENT: &str = r#"printf '{"repository":{"default_branch":"evil"},"pull_request":{"number":9,"head":{"repo":{"full_name":"tailrocks/velnor-new"}}}}' > "$GITHUB_EVENT_PATH""#;

pub(crate) fn seed_and_rewrite_event() -> String {
    format!("{} && {REWRITE_EVENT}", seed("pr"))
}

impl Store {
    pub(crate) fn new() -> Result<Self> {
        Ok(Self {
            scratch: Scratch::new()?,
        })
    }

    pub(crate) fn root(&self) -> PathBuf {
        self.scratch.workspace().join("store")
    }

    pub(crate) fn entries(&self) -> Result<Vec<Entry>> {
        let mut found = Vec::new();
        let versions = self.root().join("cache").join("v1");
        if !versions.exists() {
            return Ok(found);
        }
        for owner in fs::read_dir(&versions)? {
            for scope in fs::read_dir(owner?.path())? {
                for entry in fs::read_dir(scope?.path())? {
                    let dir = entry?.path();
                    let key = fs::read_to_string(dir.join("key")).unwrap_or_default();
                    found.push(Entry { key, dir });
                }
            }
        }
        Ok(found)
    }

    pub(crate) fn entry(&self, key: &str) -> Result<Entry> {
        self.entries()?
            .into_iter()
            .find(|entry| entry.key == key)
            .with_context(|| format!("no entry for {key}"))
    }

    pub(crate) fn age(&self, key: &str, seconds: u64) -> Result<()> {
        let marker = self.entry(key)?.dir.join("complete");
        fs::set_permissions(&marker, fs::Permissions::from_mode(0o644))?;
        OpenOptions::new()
            .write(true)
            .open(marker)?
            .set_modified(SystemTime::now() - Duration::from_secs(seconds))?;
        Ok(())
    }
}

pub(crate) struct Run {
    pub(crate) ran: Ran,
    pub(crate) work: Scratch,
}

impl Run {
    pub(crate) fn workspace_file(&self, relative: &str) -> PathBuf {
        self.work.workspace().join(relative)
    }

    pub(crate) fn succeeded(&self) -> bool {
        self.ran.code == Some(0) && self.ran.has_line("job succeeded")
    }
}

pub(crate) fn patched(fixture: &str, work: &Scratch, edits: &[(&str, &str)]) -> Result<PathBuf> {
    let body = fs::read_to_string(fixture_path(fixture))?;
    let mut job: Value = serde_json::from_str(&body)?;
    let steps = job["Steps"].as_array_mut().context("fixture steps")?;
    for step in steps {
        let Some(inputs) = step["Inputs"]["map"].as_array_mut() else {
            continue;
        };
        for input in inputs {
            let name = input["key"]["lit"].as_str().unwrap_or_default().to_owned();
            if input["value"]["type"] != 0 {
                continue;
            }
            for (edited, value) in edits {
                if name == *edited {
                    input["value"] = json!({"type": 0, "lit": value});
                }
            }
        }
    }
    let path = work.workspace().with_file_name("job.json");
    fs::write(&path, serde_json::to_string(&job)?)?;
    Ok(path)
}

pub(crate) fn home_of(work: &Scratch) -> PathBuf {
    work.workspace().with_file_name("home")
}

pub(crate) fn bytes_under(path: &Path) -> u64 {
    let Ok(items) = fs::read_dir(path) else {
        return 0;
    };
    items
        .filter_map(Result::ok)
        .map(|item| match item.metadata() {
            Ok(meta) if meta.is_dir() => bytes_under(&item.path()),
            Ok(meta) => meta.len(),
            Err(_) => 0,
        })
        .sum()
}

pub(crate) fn command(job: &Path, work: &Scratch, store: &Store) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-runner"));
    command
        .arg("exec")
        .arg("--job")
        .arg(job)
        .arg("--workspace")
        .arg(work.workspace())
        .arg("--temp")
        .arg(work.options().temp)
        .env("HOME", home_of(work))
        .env("VELNOR_STORE_ROOT", store.root());
    command
}

pub(crate) fn ran_of(output: &std::process::Output, started: Instant) -> Ran {
    Ran {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        code: output.status.code(),
        elapsed: started.elapsed(),
    }
}

pub(crate) fn run(fixture: &str, store: &Store, edits: &[(&str, &str)]) -> Result<Run> {
    run_in(Scratch::new()?, fixture, store, edits)
}

pub(crate) fn run_in(
    work: Scratch,
    fixture: &str,
    store: &Store,
    edits: &[(&str, &str)],
) -> Result<Run> {
    run_as(work, fixture, store, edits, &Message::default())
}

#[derive(Default)]
pub(crate) struct Message<'a> {
    pub(crate) github: &'a [(&'a str, &'a str)],
    pub(crate) numbers: &'a [(&'a str, u64)],
    pub(crate) variables: &'a [(&'a str, &'a str)],
    pub(crate) renamed: &'a [(&'a str, &'a str)],
    pub(crate) trusted: Option<&'a Path>,
    pub(crate) open_files: Option<u32>,
    pub(crate) umask: Option<u32>,
    pub(crate) revision: Option<&'a str>,
}

fn patch_message(job: &Path, message: &Message<'_>) -> Result<()> {
    let mut value: Value = serde_json::from_str(&fs::read_to_string(job)?)?;
    let entries = value["ContextData"]["github"]["d"]
        .as_array_mut()
        .context("github context")?;
    for (name, text) in message.github {
        let value = json!({"t": 0, "s": text});
        match entries.iter_mut().find(|entry| entry["k"] == *name) {
            Some(entry) => entry["v"] = value,
            None => entries.push(json!({"k": name, "v": value})),
        }
    }
    for (name, number) in message.numbers {
        let value = json!({"t": 4, "n": number});
        match entries.iter_mut().find(|entry| entry["k"] == *name) {
            Some(entry) => entry["v"] = value,
            None => entries.push(json!({"k": name, "v": value})),
        }
    }
    for (name, text) in message.variables {
        value["Variables"][name] = json!({"Value": text, "IsSecret": false});
    }
    for step in value["Steps"].as_array_mut().context("steps")? {
        if let Some(rev) = message
            .revision
            .filter(|_| step["Reference"]["Name"] == "actions/cache")
        {
            step["Reference"]["Ref"] = json!(rev);
        }
        for input in step["Inputs"]["map"].as_array_mut().into_iter().flatten() {
            for (from, to) in message.renamed {
                if input["key"]["lit"] == *from {
                    input["key"]["lit"] = json!(to);
                }
            }
        }
    }
    fs::write(job, serde_json::to_string(&value)?)?;
    Ok(())
}

fn limited(inner: &Command, prelude: &str) -> Command {
    let mut outer = Command::new("sh");
    outer
        .arg("-c")
        .arg(format!("{prelude} exec \"$0\" \"$@\""))
        .arg(inner.get_program())
        .args(inner.get_args());
    for (name, value) in inner.get_envs() {
        match value {
            Some(value) => outer.env(name, value),
            None => outer.env_remove(name),
        };
    }
    outer
}

pub(crate) fn spawn(
    fixture: &str,
    work: &Scratch,
    store: &Store,
    edits: &[(&str, &str)],
    message: &Message<'_>,
) -> Result<Child> {
    let job = patched(fixture, work, edits)?;
    patch_message(&job, message)?;
    let mut command = command(&job, work, store);
    if let Some(facts) = message.trusted {
        command.arg("--trusted").arg(facts);
    }
    let prelude: Vec<String> = message
        .open_files
        .map(|limit| format!("ulimit -n {limit} &&"))
        .into_iter()
        .chain(message.umask.map(|mask| format!("umask {mask:03o} &&")))
        .collect();
    if !prelude.is_empty() {
        command = limited(&command, &prelude.join(" "));
    }
    command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("start velnor-runner exec")
}

pub(crate) fn run_as(
    work: Scratch,
    fixture: &str,
    store: &Store,
    edits: &[(&str, &str)],
    message: &Message<'_>,
) -> Result<Run> {
    let started = Instant::now();
    let child = spawn(fixture, &work, store, edits, message)?;
    let output = child.wait_with_output()?;
    Ok(Run {
        ran: ran_of(&output, started),
        work,
    })
}

pub(crate) fn run_bounded(
    fixture: &str,
    store: &Store,
    edits: &[(&str, &str)],
    limit: Duration,
) -> Result<Run> {
    let work = Scratch::new()?;
    let started = Instant::now();
    let mut child = spawn(fixture, &work, store, edits, &Message::default())?;
    while child.try_wait()?.is_none() {
        if started.elapsed() > limit {
            let _ = child.kill();
            let _ = child.wait();
            bail!("the job did not finish within {limit:?}");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let output = child.wait_with_output()?;
    Ok(Run {
        ran: ran_of(&output, started),
        work,
    })
}

pub(crate) fn nothing_left_in_the_broker_dirs(store: &Store) -> Result<()> {
    for name in ["incoming", "aside"] {
        let left: Vec<_> = match fs::read_dir(store.root().join(name)) {
            Ok(items) => items
                .map(|item| item.map(|item| item.path()))
                .collect::<Result<_, _>>()?,
            Err(_) => Vec::new(),
        };
        assert_eq!(left, Vec::<PathBuf>::new());
    }
    Ok(())
}

pub(crate) fn trusted_facts(work: &Scratch, event: &Value) -> Result<PathBuf> {
    trusted_facts_of(work, event, REPOSITORY_ID)
}

pub(crate) fn trusted_facts_of(work: &Scratch, event: &Value, id: u64) -> Result<PathBuf> {
    trusted_facts_on(work, event, id, "https://github.com")
}

pub(crate) fn trusted_facts_on(
    work: &Scratch,
    event: &Value,
    id: u64,
    server: &str,
) -> Result<PathBuf> {
    let facts = json!({
        "repository_id": id,
        "repository": "tailrocks/velnor-new",
        "server_url": server,
        "default_branch": "main",
        "sha": "0123456789abcdef0123456789abcdef01234567",
        "run_id": 1,
        "run_attempt": 1,
        "event_name": if event["kind"] == "pull_request" { "pull_request" } else { "push" },
        "event": event,
    });
    let path = work.workspace().with_file_name("trusted.json");
    fs::write(&path, serde_json::to_string(&facts)?)?;
    Ok(path)
}

pub(crate) fn terminate_and_wait(mut child: Child) -> Result<Ran> {
    let started = Instant::now();
    let signalled = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .context("run kill")?;
    assert!(signalled.success());
    let deadline = Instant::now() + Duration::from_secs(60);
    while child.try_wait()?.is_none() {
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            bail!("the job did not stop after SIGTERM");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    Ok(ran_of(&child.wait_with_output()?, started))
}

pub(crate) fn wait_until(what: &str, mut ready: impl FnMut() -> bool) -> Result<()> {
    let started = Instant::now();
    while !ready() {
        if started.elapsed() > Duration::from_secs(60) {
            bail!("timed out waiting for {what}");
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    Ok(())
}

pub(crate) fn saved(store: &Store, key: &str, content: &str) -> Result<()> {
    let script = seed(content);
    let run = run("cache-save", store, &[("key", key), ("script", &script)])?;
    assert!(run.succeeded(), "{}", run.ran.stdout);
    assert!(run.ran.has_line(&format!("Cache saved with key: {key}")));
    Ok(())
}

pub(crate) use crate::native_cache_tar::{overwrite_entry, tar_member, unlock_entry};
