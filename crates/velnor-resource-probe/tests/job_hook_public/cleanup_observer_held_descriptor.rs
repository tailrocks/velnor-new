// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    time::Duration,
};

use velnor_resource_probe::cleanup_observer::CleanupObserverRecord;

use super::{PairNamespace, Scenario, TestResult, scan};
use crate::scan_poll::{Observation, poll_observation};

const SCAN_POLL_LIMIT: Duration = Duration::from_secs(10);
const SCAN_POLL_INTERVAL: Duration = Duration::from_millis(50);
const INVENTORY_FAULTS: [&str; 7] = [
    "proc_",
    "task_",
    "inventory_",
    "descriptor_",
    "process_",
    "namespace_limit",
    "observer_",
];

struct Holder(Child);

impl Drop for Holder {
    fn drop(&mut self) {
        let _killed = self.0.kill();
        let _waited = self.0.wait();
    }
}

fn observation(record: &CleanupObserverRecord) -> Observation {
    if record.owned_reference_count >= 1 {
        Observation::Counted
    } else if record.issues.iter().any(|issue| {
        INVENTORY_FAULTS
            .iter()
            .any(|fault| issue.starts_with(fault))
    }) {
        Observation::Faulted
    } else {
        Observation::NotYetVisible
    }
}

fn scan_until_owned(runner_namespace: ([u32; 2], u64)) -> TestResult<CleanupObserverRecord> {
    poll_observation(
        SCAN_POLL_LIMIT,
        SCAN_POLL_INTERVAL,
        || {
            scan(Scenario {
                runner_namespace,
                volume_override: None,
            })
        },
        observation,
    )
}

#[test]
fn a_descriptor_held_outside_the_pair_namespace_is_an_owned_reference() -> TestResult<()> {
    let mut pair = PairNamespace::start("held-namespace")?;
    let mut holder = Holder(
        Command::new("sh")
            .args(["-c", "exec 3<\"$1\" && echo held && exec sleep 300", "sh"])
            .arg(format!("/proc/{}/ns/mnt", pair.pid()))
            .stdout(Stdio::piped())
            .spawn()?,
    );
    let mut stdout = BufReader::new(holder.0.stdout.take().ok_or("no holder stdout")?);
    let mut ready = String::new();
    stdout.read_line(&mut ready)?;
    assert_eq!(ready.trim(), "held");
    pair.child.kill()?;
    pair.child.wait()?;
    let record = scan_until_owned(pair.namespace)?;
    assert!(record.owned_reference_count >= 1, "{record:?}");
    Ok(())
}
