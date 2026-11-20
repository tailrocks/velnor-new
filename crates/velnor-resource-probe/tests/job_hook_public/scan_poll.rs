// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    error::Error,
    thread,
    time::{Duration, Instant},
};

use velnor_resource_probe::cleanup_observer_wire::CleanupObserverRecord;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Observation {
    Counted,
    NotYetVisible,
}

pub(crate) fn owned_reference_observation(record: &CleanupObserverRecord) -> Observation {
    if record.owned_reference_count >= 1 {
        Observation::Counted
    } else {
        Observation::NotYetVisible
    }
}

pub(crate) fn poll_observation<T>(
    limit: Duration,
    interval: Duration,
    mut observe: impl FnMut() -> Result<T, Box<dyn Error>>,
    classify: impl Fn(&T) -> Observation,
) -> Result<T, Box<dyn Error>> {
    let deadline = Instant::now() + limit;
    loop {
        let value = observe()?;
        match classify(&value) {
            Observation::Counted => return Ok(value),
            Observation::NotYetVisible if Instant::now() >= deadline => return Ok(value),
            Observation::NotYetVisible => thread::sleep(interval),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMIT: Duration = Duration::from_secs(5);
    const INTERVAL: Duration = Duration::from_millis(1);

    fn run(
        sequence: &[(u32, Observation)],
        limit: Duration,
    ) -> Result<(u32, usize), Box<dyn Error>> {
        let mut calls = 0;
        let value = poll_observation(
            limit,
            INTERVAL,
            || {
                let index = calls.min(sequence.len() - 1);
                calls += 1;
                Ok(sequence[index])
            },
            |(_, observation)| *observation,
        )?;
        Ok((value.0, calls))
    }

    fn loaded_ci_host_record() -> CleanupObserverRecord {
        CleanupObserverRecord {
            schema_version: 1,
            operation_id: "0123456789abcdef0123456789abcdef".to_owned(),
            binding_sha256: "1d4362f29562fb1668677fd4d9bdc54c982b3a402a9fe4b1fd357d7beb1c9fd5"
                .to_owned(),
            guest_boot_id: Some("35d64c2c-ee1f-42a2-8d26-96f68eeb1853".to_owned()),
            docker_root: None,
            complete: false,
            owned_references_absent: false,
            issues: [
                "initial_cgroup_or_root_check_incomplete",
                "daemon_identity_unreadable",
                "descriptor_limit",
                "final_cgroup_or_root_check_incomplete",
                "docker_root_changed",
                "daemon_identity_changed",
            ]
            .map(str::to_owned)
            .to_vec(),
            task_count: 5037,
            descriptor_count: 68493,
            namespace_count: 1,
            owned_reference_count: 0,
            absent_volume_mountpoints: Vec::new(),
            absent_tmpfs_devices: Vec::new(),
            absent_mount_namespaces: Vec::new(),
            absent_cgroup_paths: None,
        }
    }

    #[test]
    fn a_scan_that_hit_the_descriptor_limit_on_a_loaded_host_is_retried() {
        assert_eq!(
            owned_reference_observation(&loaded_ci_host_record()),
            Observation::NotYetVisible
        );
    }

    #[test]
    fn a_not_yet_visible_observation_retries_until_it_is_counted() -> Result<(), Box<dyn Error>> {
        let sequence = [
            (1, Observation::NotYetVisible),
            (2, Observation::NotYetVisible),
            (3, Observation::Counted),
        ];
        assert_eq!(run(&sequence, LIMIT)?, (3, 3));
        Ok(())
    }

    #[test]
    fn a_never_visible_observation_returns_at_a_zero_deadline_without_retrying()
    -> Result<(), Box<dyn Error>> {
        let sequence = [
            (1, Observation::NotYetVisible),
            (2, Observation::NotYetVisible),
        ];
        let (last, calls) = run(&sequence, Duration::ZERO)?;
        assert_eq!((last, calls), (1, 1));
        Ok(())
    }

    #[test]
    fn a_failed_observation_is_returned_without_retrying() {
        let mut calls = 0;
        let result: Result<u32, _> = poll_observation(
            LIMIT,
            INTERVAL,
            || {
                calls += 1;
                Err::<u32, Box<dyn Error>>("scan failed".into())
            },
            |_| Observation::NotYetVisible,
        );
        assert!(result.is_err());
        assert_eq!(calls, 1);
    }
}
