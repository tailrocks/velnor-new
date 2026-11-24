// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use super::output::{self, Staging};
use super::{DOCKERFILE_DIR, FileSource, docker, github_env, process, scan_archive, source};
use crate::cli_args::BuildArgs;
use crate::error::CiError;
use crate::image_candidate::build::{Parts, record};
use crate::image_candidate::canonical;
use crate::image_candidate::identity::workflow;
use crate::image_candidate::lock::parse_lock;
use crate::image_candidate::oci;

const DONE: &str = "PASS: Candidate archive and record were written.\n";
const LOCK: &str = ".velnor/runner.lock";
const CHECK_TIMEOUT: Duration = Duration::from_secs(300);

fn read(path: &Path) -> Result<String, CiError> {
    fs::read_to_string(path).map_err(|error| CiError::new(format!("{}: {error}", path.display())))
}

pub(in crate::io) fn build(args: &BuildArgs) -> Result<String, CiError> {
    let root = fs::canonicalize(&args.root)
        .map_err(|error| CiError::new(format!("{}: {error}", args.root.display())))?;
    let env = github_env();
    let github_sha = env.get("GITHUB_SHA").map(String::as_str);
    let inputs = source::identity(&root, &args.source_commit, github_sha)?;
    let (workflow, reference) = workflow(&env)?;
    let lock = parse_lock(&read(&root.join(LOCK))?)?;
    let buildx = docker::buildx(&args.docker, &lock)?;
    let outputs = output::plan(&root, &args.archive, &args.record)?;
    let mut check = Command::new("bash");
    check
        .arg(root.join(DOCKERFILE_DIR).join("build-test.sh"))
        .current_dir(&root);
    process::run(process::clean_environment(&mut check), CHECK_TIMEOUT)?;
    if source::identity(&root, &args.source_commit, github_sha)? != inputs {
        return Err(CiError::new(
            "Build inputs changed while build-test.sh ran.",
        ));
    }
    let parent = outputs
        .archive
        .parent()
        .ok_or_else(|| CiError::new("Candidate output directory must exist."))?;
    let staging = Staging::create(parent)?;
    let archive = staging.path().join("candidate.oci.tar");
    let manifest = staging.path().join("candidate.json");
    docker::build(
        &args.docker,
        &root,
        staging.path(),
        &archive,
        &buildx,
        &lock,
    )?;
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
        builder: &buildx.banner,
        archive_sha: &scan.archive_sha,
        archive_size: scan.archive_size,
        oci: graph,
    })?;
    output::write_private(&manifest, &canonical::encode(&value))?;
    output::publish_pair([
        (archive.as_path(), outputs.archive.as_path()),
        (manifest.as_path(), outputs.record.as_path()),
    ])?;
    Ok(DONE.to_owned())
}
