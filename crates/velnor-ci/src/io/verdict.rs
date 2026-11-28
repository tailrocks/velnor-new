// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use super::artifact_api::ArtifactApi;
use super::{EVENT_VARIABLE, api_url, append_outputs, http, read_limited};
use crate::cli::VerdictCommand;
use crate::error::CiError;
use crate::github::Repo;
use crate::verdict::{ARTIFACT_NAME, FILE_NAME, Reusable, Runner, VerdictMatch};
use crate::verdict_gather::{Artifacts, Context, evaluate};

const LOOKUP_TIMEOUT: Duration = Duration::from_secs(30);
const MEMBER_LIMIT: u64 = 1 << 20;
const DIRECTORY: &str = "velnor-verdict";
const DEFAULT_SERVER: &str = "https://github.com";

fn variable(name: &str) -> String {
    std::env::var(name).unwrap_or_default()
}

fn required(name: &str) -> Result<String, CiError> {
    Some(variable(name))
        .filter(|value| !value.is_empty())
        .ok_or_else(|| CiError::new(format!("{name} is not set")))
}

fn own() -> Result<VerdictMatch, CiError> {
    let output = Command::new("git")
        .args(["rev-parse", "--verify", "HEAD^{tree}"])
        .output()
        .map_err(|e| CiError::new(format!("cannot run git: {e}")))?;
    if !output.status.success() {
        return Err(CiError::new(format!(
            "git rev-parse HEAD^{{tree}} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(VerdictMatch::new(
        String::from_utf8_lossy(&output.stdout).trim().to_owned(),
        VerdictMatch::parse_plan(&required("PLAN_JSON")?)?,
        Runner {
            os: required("RUNNER_OS")?,
            arch: required("RUNNER_ARCH")?,
            environment: required("RUNNER_ENVIRONMENT")?,
        },
    ))
}

pub(super) fn run(command: &VerdictCommand) -> Result<String, CiError> {
    match command {
        VerdictCommand::Record => record(),
        VerdictCommand::Reuse => Ok(reuse()),
    }
}

fn record() -> Result<String, CiError> {
    let directory = PathBuf::from(required("RUNNER_TEMP")?).join(DIRECTORY);
    fs::create_dir_all(&directory)
        .map_err(|e| CiError::new(format!("cannot create {}: {e}", directory.display())))?;
    let staging = directory.join(format!(".{FILE_NAME}.partial"));
    let target = directory.join(FILE_NAME);
    fs::write(&staging, own()?.text()?)
        .and_then(|()| fs::rename(&staging, &target))
        .map_err(|e| CiError::new(format!("cannot write {}: {e}", target.display())))?;
    Ok(format!(
        "velnor: recorded the verdict in {}\n",
        target.display()
    ))
}

struct Downloads {
    api: ArtifactApi,
    scratch: PathBuf,
}

impl Artifacts for Downloads {
    fn verdict_text(&self, repo: &Repo, run: u64) -> Result<String, CiError> {
        let listed: Vec<_> = self
            .api
            .list(repo, run)?
            .into_iter()
            .filter(|artifact| artifact.name == ARTIFACT_NAME && !artifact.expired)
            .collect();
        let [artifact] = listed.as_slice() else {
            return Err(CiError::new(format!(
                "the run has {} live artifacts named {ARTIFACT_NAME}, not one",
                listed.len()
            )));
        };
        let archive = self.api.download(repo, artifact.id, &self.scratch)?;
        let mut zip = zip::ZipArchive::new(archive)
            .map_err(|e| CiError::new(format!("not a zip archive: {e}")))?;
        let member = zip
            .by_name(FILE_NAME)
            .map_err(|e| CiError::new(format!("no {FILE_NAME} in the artifact: {e}")))?;
        let bytes = read_limited(member.take(MEMBER_LIMIT + 1), MEMBER_LIMIT)?;
        String::from_utf8(bytes).map_err(|e| CiError::new(format!("{FILE_NAME}: {e}")))
    }
}

fn decide() -> Result<(String, Vec<String>), String> {
    let event = variable(EVENT_VARIABLE);
    if event != "merge_group" {
        return Err(format!("the event is {event}, not merge_group"));
    }
    let fail = |error: CiError| error.to_string();
    let own = own().map_err(fail)?;
    let reusable =
        Reusable::parse_list(&required("VELNOR_REUSABLE").map_err(fail)?).map_err(fail)?;
    let token = required("GITHUB_TOKEN").map_err(fail)?;
    let server = Some(variable("GITHUB_SERVER_URL"))
        .filter(|url| !url.is_empty())
        .unwrap_or_else(|| DEFAULT_SERVER.to_owned());
    let context = Context {
        repository: required("GITHUB_REPOSITORY").map_err(fail)?,
        workflow_ref: required("GITHUB_WORKFLOW_REF").map_err(fail)?,
        head_ref: required("VELNOR_MERGE_GROUP_HEAD_REF").map_err(fail)?,
        base_sha: required("VELNOR_MERGE_GROUP_BASE_SHA").map_err(fail)?,
        default_branch: required("VELNOR_DEFAULT_BRANCH").map_err(fail)?,
        required: required("VELNOR_REQUIRED_CHECK").map_err(fail)?,
        server_url: server,
    };
    let github = http::HttpGitHub::new(api_url(), token.clone(), LOOKUP_TIMEOUT);
    let temp = Some(variable("RUNNER_TEMP"))
        .filter(|path| !path.is_empty())
        .map_or_else(std::env::temp_dir, PathBuf::from);
    let downloads = Downloads {
        api: ArtifactApi::new(api_url(), token),
        scratch: temp.join(format!("velnor-verdict-{}.zip", std::process::id())),
    };
    let reuse = evaluate(&github, &downloads, &context, &own, &reusable)
        .map_err(|refusal| refusal.to_string())?;
    Ok((reuse.run, reuse.reused.into_iter().collect()))
}

fn reuse() -> String {
    let (outputs, note) = match decide() {
        Ok((run, reused)) => (
            format!("reuse=true\nreuse_run={run}\nreused={}\n", reused.join(" ")),
            format!("velnor: reusing verdict of {run}\n"),
        ),
        Err(reason) => (
            "reuse=false\nreuse_run=\nreused=\n".to_owned(),
            format!("velnor: no verdict for this key ({reason})\n"),
        ),
    };
    match append_outputs(&outputs) {
        Ok(()) => note,
        Err(error) => format!("{note}(cannot write the output: {error})\n"),
    }
}
