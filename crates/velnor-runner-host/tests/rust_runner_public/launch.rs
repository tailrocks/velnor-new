// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_runner_host::{HostError, RustRunnerFailure, RustRunnerLauncher};

use super::support::{Fixture, SECRET_JIT, jit, reservation, runner_lines};

type Check = Result<(), Box<dyn Error>>;

#[test]
fn rust_runner_refuses_to_start_a_job_until_the_sandbox_confines_it() -> Check {
    let fixture = Fixture::ready()?;
    fixture.launcher().ensure_machine()?;
    let before = fixture.log()?;
    let refused = Some(HostError::RustRunner(RustRunnerFailure::LaunchClosed));
    assert_eq!(
        fixture
            .production_launcher()
            .start_job(reservation(61)?, &jit(SECRET_JIT))
            .err(),
        refused
    );
    assert_eq!(
        RustRunnerLauncher::new(
            fixture.root.join("orb"),
            fixture.store.clone(),
            fixture.root.join("host-runner"),
        )
        .start_job(reservation(62)?, &jit(SECRET_JIT))
        .err(),
        refused
    );
    let after = fixture.log()?;
    assert_eq!(before, after, "a refused start reached the machine");
    assert_eq!(runner_lines(&after), Vec::<&String>::new());
    assert!(!fixture.seen("61.jit").exists());
    assert_eq!(std::fs::read_dir(&fixture.store)?.count(), 0);
    Ok(())
}
