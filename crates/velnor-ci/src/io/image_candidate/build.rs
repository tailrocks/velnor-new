// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::process::Command;
use std::time::Duration;

use super::output::{self, Staging};
use super::process::{Runner, script_environment};
use super::{DOCKERFILE_DIR, FileSource, docker, github_env, read_lock, scan_archive, source};
use crate::cli_args::BuildArgs;
use crate::error::CiError;
use crate::image_candidate::build::{Parts, record, refuse_secrets};
use crate::image_candidate::canonical;
use crate::image_candidate::identity::workflow;
use crate::image_candidate::oci;

const DONE: &str = "PASS: Candidate archive and record were written.\n";
const CHECK_TIMEOUT: Duration = Duration::from_secs(300);

pub(in crate::io) fn build(args: &BuildArgs) -> Result<String, CiError> {
    refuse_secrets(std::env::vars_os().map(|(name, _)| name))?;
    let runner = Runner::start()?;
    let root = fs::canonicalize(&args.root)
        .map_err(|error| CiError::new(format!("{}: {error}", args.root.display())))?;
    let env = github_env();
    let github_sha = env.get("GITHUB_SHA").map(String::as_str);
    let inputs = source::identity(&runner, &root, &args.source_commit, github_sha)?;
    let (workflow, reference) = workflow(&env)?;
    let lock = read_lock(&root)?;
    let outputs = output::plan(&root, &args.archive, &args.record)?;
    let parent = outputs
        .archive
        .parent()
        .ok_or_else(|| CiError::new("Candidate output directory must exist."))?;
    let scratch = Staging::create(&std::env::temp_dir())?;
    let mut check = Command::new("bash");
    check
        .arg(root.join(DOCKERFILE_DIR).join("build-test.sh"))
        .current_dir(&root);
    script_environment(&mut check, &scratch.home());
    runner.run(&mut check, CHECK_TIMEOUT)?;
    drop(scratch);
    if source::identity(&runner, &root, &args.source_commit, github_sha)? != inputs {
        return Err(CiError::new(
            "Build inputs changed while build-test.sh ran.",
        ));
    }
    if output::plan(&root, &args.archive, &args.record)? != outputs {
        return Err(CiError::new(
            "Candidate output paths changed while build-test.sh ran.",
        ));
    }
    let staging = Staging::create(parent)?;
    let plugin = docker::plugin_source(args.buildx.as_deref())?;
    let isolated = docker::isolate(&plugin, &lock, staging.path())?;
    let builder = docker::version(&runner, &args.docker, &isolated, &lock)?;
    let archive = staging.path().join("candidate.oci.tar");
    let manifest = staging.path().join("candidate.json");
    docker::build(
        &runner,
        &args.docker,
        &root,
        staging.path(),
        &isolated,
        &archive,
    )?;
    runner.checkpoint()?;
    let scan = scan_archive(&archive)?;
    let graph = oci::verify(
        &scan,
        &FileSource {
            path: archive.clone(),
        },
    )?;
    let value = record(&Parts {
        commit: &args.source_commit,
        reference: &reference,
        inputs,
        workflow,
        builder: &builder,
        archive_sha: &scan.archive_sha,
        archive_size: scan.archive_size,
        oci: graph,
    })?;
    output::write_private(&manifest, &canonical::encode(&value))?;
    runner.checkpoint()?;
    output::publish_pair([
        (archive.as_path(), outputs.archive.as_path()),
        (manifest.as_path(), outputs.record.as_path()),
    ])?;
    if let Err(error) = runner.checkpoint() {
        output::withdraw(&outputs);
        return Err(error);
    }
    Ok(DONE.to_owned())
}
