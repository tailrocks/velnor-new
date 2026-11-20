// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn bounded(fault: ApiFault) -> TestResult<()> {
    let git = GitFixture::new()?;
    let mut plan = ApiPlan::new(git.source_sha());
    plan.fault = Some(fault);
    let (output, state, elapsed) =
        qualify_with_timeout_timed(&git, git.source_sha(), plan, Duration::from_secs(24))?;
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
