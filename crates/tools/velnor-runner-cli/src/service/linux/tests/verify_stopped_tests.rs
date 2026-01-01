use super::*;

fn stopped_unit() -> Vec<u8> {
    unit_snapshot(UnitSnapshot {
        active_state: "inactive",
        sub_state: "dead",
        main_pid: 0,
        control_pid: 0,
        result: "success",
        stop_code: "(null)",
        stop_status: "0/0",
        timeout: "1860s",
    })
}

fn verification_manager(unit: Vec<u8>, jobs: ManagerOutput) -> FakeManager {
    verification_manager_with_final(unit.clone(), unit, jobs)
}

fn verification_manager_with_final(
    initial_unit: Vec<u8>,
    final_unit: Vec<u8>,
    jobs: ManagerOutput,
) -> FakeManager {
    FakeManager::with_outputs([
        manager_output(true, initial_unit),
        credential_property(),
        identity_unit_snapshot(),
        jobs,
        manager_output(true, final_unit),
    ])
}

#[test]
fn verify_stopped_requires_the_exact_idle_package_contract_and_no_jobs() {
    let mut manager = verification_manager(stopped_unit(), empty_jobs());

    assert_eq!(
        perform(
            ServiceAction::VerifyStopped,
            &mut manager,
            DRAIN_TIMEOUT_SECS
        ),
        Ok(())
    );
    assert_eq!(
        manager.calls.first(),
        Some(&vec![
            "show".to_owned(),
            "--no-pager".to_owned(),
            format!("--property={}", super::super::SHOW_PROPERTIES),
            UNIT.to_owned(),
        ])
    );
    assert_eq!(
        manager
            .calls
            .get(manager.calls.len() - 2)
            .map(|call| call[0].as_str()),
        Some("list-jobs")
    );
    assert_eq!(
        manager.calls.last().map(|call| call[0].as_str()),
        Some("show")
    );
    assert!(manager.calls.iter().all(|call| {
        !matches!(
            call.first().map(String::as_str),
            Some("start" | "stop" | "restart" | "kill")
        )
    }));
}

#[test]
fn verify_stopped_rejects_running_failed_and_unknown_job_state() {
    let active = unit_snapshot(UnitSnapshot {
        active_state: "active",
        sub_state: "running",
        main_pid: 42,
        control_pid: 0,
        result: "success",
        stop_code: "(null)",
        stop_status: "0/0",
        timeout: "1860s",
    });
    let mut manager = verification_manager(active, empty_jobs());
    assert_eq!(
        perform(
            ServiceAction::VerifyStopped,
            &mut manager,
            DRAIN_TIMEOUT_SECS
        ),
        Err(ServiceFault::StopNotVerified)
    );

    let failed = String::from_utf8_lossy(&stopped_unit())
        .replace("Result=success", "Result=exit-code")
        .into_bytes();
    let mut manager = verification_manager(failed, empty_jobs());
    assert_eq!(
        perform(
            ServiceAction::VerifyStopped,
            &mut manager,
            DRAIN_TIMEOUT_SECS
        ),
        Err(ServiceFault::StopNotVerified)
    );

    let missing_job = String::from_utf8_lossy(&stopped_unit())
        .replace("Job=\n", "")
        .into_bytes();
    let mut manager = verification_manager(missing_job, empty_jobs());
    assert_eq!(
        perform(
            ServiceAction::VerifyStopped,
            &mut manager,
            DRAIN_TIMEOUT_SECS
        ),
        Err(ServiceFault::UnknownState)
    );
}

#[test]
fn verify_stopped_rechecks_the_unit_after_ancillary_reads() {
    let active = unit_snapshot(UnitSnapshot {
        active_state: "active",
        sub_state: "running",
        main_pid: 73,
        control_pid: 0,
        result: "success",
        stop_code: "(null)",
        stop_status: "0/0",
        timeout: "1860s",
    });
    let mut manager = verification_manager_with_final(stopped_unit(), active, empty_jobs());

    assert_eq!(
        perform(
            ServiceAction::VerifyStopped,
            &mut manager,
            DRAIN_TIMEOUT_SECS
        ),
        Err(ServiceFault::StopNotVerified)
    );
    assert_eq!(
        manager
            .calls
            .iter()
            .filter(|call| call.last().is_some_and(|unit| unit == UNIT))
            .count(),
        2
    );
    assert_eq!(
        manager
            .calls
            .get(manager.calls.len() - 2)
            .map(|call| call[0].as_str()),
        Some("list-jobs")
    );
}

#[test]
fn verify_stopped_rejects_unit_and_global_pending_jobs() {
    let unit_job = String::from_utf8_lossy(&stopped_unit())
        .replace("Job=\n", "Job=42 /org/freedesktop/systemd1/job/42\n")
        .into_bytes();
    let mut manager = verification_manager(unit_job, empty_jobs());
    assert_eq!(
        perform(
            ServiceAction::VerifyStopped,
            &mut manager,
            DRAIN_TIMEOUT_SECS
        ),
        Err(ServiceFault::PendingJob)
    );
    assert!(
        !manager
            .calls
            .iter()
            .any(|call| call.first().is_some_and(|verb| verb == "list-jobs"))
    );

    let global_job = manager_output(true, b"42 velnor-host.service stop waiting\n".to_vec());
    let mut manager = verification_manager(stopped_unit(), global_job);
    assert_eq!(
        perform(
            ServiceAction::VerifyStopped,
            &mut manager,
            DRAIN_TIMEOUT_SECS
        ),
        Err(ServiceFault::PendingJob)
    );
}

#[test]
fn verify_stopped_ignores_pending_jobs_for_other_units() {
    let unrelated_job = manager_output(true, b"43 apt-daily.service start waiting\n".to_vec());
    let mut manager = verification_manager(stopped_unit(), unrelated_job);

    assert_eq!(
        perform(
            ServiceAction::VerifyStopped,
            &mut manager,
            DRAIN_TIMEOUT_SECS
        ),
        Ok(())
    );
}

#[test]
fn verify_stopped_rejects_an_alias_or_missing_unit_identity() {
    let alias = String::from_utf8_lossy(&stopped_unit())
        .replace("Id=velnor-host.service", "Id=other.service")
        .into_bytes();
    let mut manager = verification_manager(alias, empty_jobs());
    assert_eq!(
        perform(
            ServiceAction::VerifyStopped,
            &mut manager,
            DRAIN_TIMEOUT_SECS
        ),
        Err(ServiceFault::ServiceContract)
    );

    let missing_id = String::from_utf8_lossy(&stopped_unit())
        .replace("Id=velnor-host.service\n", "")
        .into_bytes();
    let mut manager = verification_manager(missing_id, empty_jobs());
    assert_eq!(
        perform(
            ServiceAction::VerifyStopped,
            &mut manager,
            DRAIN_TIMEOUT_SECS
        ),
        Err(ServiceFault::ServiceContract)
    );
}
