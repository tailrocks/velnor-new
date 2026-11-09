// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::args::RunnerFamilyPublicationArgs;
use std::{cell::Cell, rc::Rc, time::Instant};

fn arguments() -> RunnerFamilyPublicationArgs {
    RunnerFamilyPublicationArgs {
        source_commit: "a".repeat(40),
        producer_run_id: 1,
        producer_run_attempt: 1,
        artifact_id: 1,
        assets_dir: "/tmp/assets".into(),
        qualification_proof: "/tmp/proof".into(),
        private_scratch: "/tmp/scratch".into(),
        manifest_output: "/tmp/manifest".into(),
    }
}

#[test]
fn family_publication_commands_allow_longer_work_within_job_budget()
-> Result<(), Box<dyn std::error::Error>> {
    let commands = [
        ImageMaintenanceCommand::PublishRunnerImages(arguments()),
        ImageMaintenanceCommand::VerifyRunnerPublication(arguments()),
    ];
    assert_eq!(commands.each_ref().map(deadline_minutes), [40, 40]);
    for command in commands {
        let minutes = deadline_minutes(&command);
        let start = Instant::now();
        let now = Rc::new(Cell::new(start));
        let clock = Rc::clone(&now);
        CheckDeadline::with_clock(
            move || clock.get(),
            || {
                let deadline = CheckDeadline::after(Duration::from_mins(u64::from(minutes)))?;
                now.set(start + Duration::from_mins(11));
                assert!(deadline.remaining().is_ok());
                now.set(start + Duration::from_mins(41));
                assert!(deadline.remaining().is_err());
                Ok::<(), Box<dyn std::error::Error>>(())
            },
        )?;
    }
    Ok(())
}
