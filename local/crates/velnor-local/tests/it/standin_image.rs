// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! The stand-in image's patches of runner.server. runner.server signs every job's token to expire
//! `timeout-minutes + 10` after the job is queued, but it never cancels a job at its timeout, as
//! GitHub does. A job that runs past that, as every lane of a slow laptop run does, finishes with a
//! token the server refuses: its completion never arrives, the jobs that need it are never
//! dispatched, and the run stays `running` for ever.

use std::path::Path;

const DOCKERFILE: &str = "../../docker/runner-server/Dockerfile";
const SNIPPET: &str = "../../docker/runner-server/job-grant.cs.snippet";
const EXPIRY: &str = "AddMinutes(timeoutMinutes + 10)";

fn read(relative: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| unreachable!("{}: {e}", path.display()))
}

#[test]
fn the_stand_in_keeps_a_job_token_valid_after_the_jobs_timeout() {
    let dockerfile = read(DOCKERFILE);
    let rewrite = dockerfile
        .lines()
        .find(|line| line.contains(EXPIRY) && line.contains("sed"))
        .unwrap_or_else(|| unreachable!("the Dockerfile leaves the token expiry at {EXPIRY}"));
    assert!(
        rewrite.contains("AddHours(24)"),
        "the token must outlive any job: {rewrite}"
    );
}

#[test]
fn the_build_fails_when_runner_server_signs_a_token_the_patch_does_not_cover() {
    let dockerfile = read(DOCKERFILE);
    // Runner.Server signs the job token in two places (GitHub and Azure jobs); the grant snippet
    // adds a third. The build counts them before the rewrite and finds none after it.
    assert!(
        dockerfile.contains("= 3 "),
        "the build does not count the three sites"
    );
    assert!(
        dockerfile.contains("! grep -q 'AddMinutes(timeoutMinutes + 10)'"),
        "the build does not check that none is left"
    );
    assert!(
        read(SNIPPET).contains(EXPIRY),
        "the snippet changed: update the count"
    );
}
