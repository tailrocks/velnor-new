// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Instant;

use super::*;

#[test]
fn fixture_teardown_does_not_wait_for_a_silent_client() -> TestResult<()> {
    let api = DockerApiFixture::new(ApiPlan::new("0000000000000000000000000000000000000000"))?;
    let _silent = UnixStream::connect(api.socket())?;
    let mut probe = UnixStream::connect(api.socket())?;
    probe.write_all(b"GET /probe HTTP/1.1\r\nHost: fixture\r\n\r\n")?;
    let mut reply = Vec::new();
    probe.read_to_end(&mut reply)?;
    assert_ne!(reply.len(), 0);
    let started = Instant::now();
    drop(api);
    let teardown = started.elapsed();
    assert!(
        teardown < Duration::from_secs(2),
        "fixture teardown waited {teardown:?} for a silent client"
    );
    Ok(())
}

fn bounded(fault: ApiFault) -> TestResult<()> {
    let git = GitFixture::new()?;
    let mut plan = ApiPlan::new(git.source_sha());
    plan.fault = Some(fault);
    let started = Instant::now();
    let (output, state, elapsed) =
        qualify_with_timeout_timed(&git, git.source_sha(), plan, Duration::from_secs(24))?;
    let teardown = started.elapsed().saturating_sub(elapsed);
    assert!(
        teardown < Duration::from_secs(2),
        "fixture teardown outlasted the qualifier by {teardown:?}"
    );
    assert_failed_and_cleaned(&output, &state);
    let readiness = matches!(fault, ApiFault::StallExecCreate | ApiFault::StallExecStream);
    assert!(elapsed < Duration::from_secs(if readiness { 22 } else { 9 }));
    assert!(elapsed >= Duration::from_secs(if readiness { 11 } else { 4 }));
    Ok(())
}

macro_rules! stall_cases {
    ($($name:ident => $fault:ident,)*) => {
        $(
            #[test]
            fn $name() -> TestResult<()> {
                bounded(ApiFault::$fault)
            }
        )*
    };
}

stall_cases! {
    stall_image_inspect => StallImageInspect,
    stall_container_create => StallContainerCreate,
    stall_container_start => StallContainerStart,
    stall_container_inspect => StallContainerInspect,
    stall_wait => StallWait,
    stall_logs => StallLogs,
    stall_exec_create => StallExecCreate,
    stall_exec_stream => StallExecStream,
}
