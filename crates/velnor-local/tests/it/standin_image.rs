// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! The stand-in image's patch of runner.server. runner.server arms a timer when it hands a job to a
//! runner, so that the job's cancel request fires at `timeout-minutes` (360 when unset), but the wait
//! that is meant to turn the request into a cancel message for the runner never throws: the runner is
//! never told, the job runs on, its token (`timeout-minutes + 10`) expires, its completion is lost
//! and the jobs that `need` it never start. The image waits with `Task.Delay`, which throws, so the
//! runner cancels the job and reports it as cancelled, as GitHub does. The token expiry stays as
//! runner.server signs it, which is what bounds the cache-admin grant.

use std::path::Path;

const DOCKERFILE: &str = "../../docker/runner-server/Dockerfile";
const SNIPPET: &str = "../../docker/runner-server/job-grant.cs.snippet";
const EXPIRY: &str = "AddMinutes(timeoutMinutes + 10)";
const BROKEN_WAIT: &str =
    "WaitAnyCancellationToken(jobRunningToken, ts.Token, job.CancelRequest.Token)";
const WORKING_WAIT: &str = "Task.Delay(-1, CancellationTokenSource.CreateLinkedTokenSource(jobRunningToken, ts.Token, job.CancelRequest.Token).Token)";

fn read(relative: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| unreachable!("{}: {e}", path.display()))
}

fn rewrite_line(dockerfile: &str) -> &str {
    dockerfile
        .lines()
        .find(|line| line.contains("sed -i") && line.contains(BROKEN_WAIT))
        .unwrap_or_else(|| unreachable!("the Dockerfile does not rewrite {BROKEN_WAIT}"))
}

#[test]
fn the_stand_in_delivers_the_cancel_at_timeout_minutes_to_the_runner() {
    let rewrite = rewrite_line(&read(DOCKERFILE)).to_owned();
    assert!(
        rewrite.contains(WORKING_WAIT),
        "the wait must throw when the job's cancel request fires: {rewrite}"
    );
}

#[test]
fn the_build_fails_when_the_wait_moves_or_the_rewrite_does_not_apply() {
    let dockerfile = read(DOCKERFILE);
    // The rewrite is one line of runner.server. The build counts it before, so a moved or
    // duplicated anchor stops the build instead of leaving the job uncancelled, and checks after
    // that the new wait is there and the old one is gone.
    let step = dockerfile
        .split("RUN ")
        .find(|step| step.contains("sed -i") && step.contains(BROKEN_WAIT))
        .unwrap_or_else(|| unreachable!("no RUN step rewrites {BROKEN_WAIT}"));
    assert!(
        step.contains(&format!("grep -c 'await Helper.{BROKEN_WAIT};'")) && step.contains("= 1 "),
        "the build does not count the one anchor: {step}"
    );
    assert!(
        step.contains(&format!("grep -q 'await {WORKING_WAIT};'")),
        "the build does not check that the new wait is in place: {step}"
    );
    assert!(
        step.contains(&format!("! grep -q '{BROKEN_WAIT}'")),
        "the build does not check that the old wait is gone: {step}"
    );
}

#[test]
fn the_job_token_expires_when_runner_server_says_not_a_day_later() {
    // A longer expiry would widen the cache-admin grant that the job-grant snippet signs, so
    // neither signing site is rewritten, and no pin stretches the expiry.
    let dockerfile = read(DOCKERFILE);
    assert!(
        !dockerfile.contains("AddHours") && !dockerfile.contains(EXPIRY),
        "the Dockerfile must not rewrite the token expiry"
    );
    let snippet = read(SNIPPET);
    assert!(
        snippet.contains(EXPIRY) && !snippet.contains("AddHours"),
        "the grant must expire timeout-minutes + 10 after queueing"
    );
}
