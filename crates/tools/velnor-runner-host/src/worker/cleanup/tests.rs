mod dind;
mod fixtures;
mod lifecycle;
mod outer_network;
mod recovery;
mod start_observation;

#[test]
fn observed_job_keeps_event_and_rest_identifiers_separate() -> Result<(), crate::HostError> {
    use super::{ObservedJobIdentity, WorkerGenerationIdentity};

    let observed = ObservedJobIdentity::new(
        17,
        None,
        "opaque-scale-set-job".to_owned(),
        None,
        29,
        "velnor-job-41".to_owned(),
    )?;
    let identity = WorkerGenerationIdentity::new(
        41,
        "velnor-job-41".to_owned(),
        "w0123456789abcdef0123456789abcdef".to_owned(),
        "a123456789abcdef".to_owned(),
        "b123456789abcdef".to_owned(),
        Some(observed),
    )?;
    let proof_view = identity.observed_job().ok_or(crate::HostError::Identity)?;
    assert_eq!(proof_view.workflow_run_id(), 17);
    assert_eq!(proof_view.attempt(), None);
    assert_eq!(proof_view.scale_set_job_id(), "opaque-scale-set-job");
    assert_eq!(proof_view.actions_job_id(), None);
    assert_eq!(proof_view.runner_id(), 29);
    assert_eq!(proof_view.runner_name(), "velnor-job-41");
    Ok(())
}

#[test]
fn observed_numeric_ids_fit_the_journal_integer_domain() {
    use super::ObservedJobIdentity;

    let too_large = i64::MAX as u64 + 1;
    assert!(
        ObservedJobIdentity::new(
            too_large,
            None,
            "opaque-scale-set-job".to_owned(),
            None,
            29,
            "velnor-job-41".to_owned(),
        )
        .is_err()
    );
    assert!(
        ObservedJobIdentity::new(
            17,
            None,
            "opaque-scale-set-job".to_owned(),
            Some(too_large),
            29,
            "velnor-job-41".to_owned(),
        )
        .is_err()
    );
    assert!(
        ObservedJobIdentity::new(
            17,
            None,
            "opaque-scale-set-job".to_owned(),
            None,
            too_large,
            "velnor-job-41".to_owned(),
        )
        .is_err()
    );
}

#[test]
fn generation_outer_network_is_optional_but_exact_when_present() -> Result<(), crate::HostError> {
    use super::WorkerGenerationIdentity;

    let legacy = WorkerGenerationIdentity::new(
        41,
        "velnor-job-41".to_owned(),
        "w0123456789abcdef0123456789abcdef".to_owned(),
        "a123456789abcdef".to_owned(),
        "b123456789abcdef".to_owned(),
        None,
    )?;
    assert_eq!(legacy.outer_network_name(), None);
    assert_eq!(legacy.outer_network_id(), None);

    let linux = WorkerGenerationIdentity::new_with_network(
        41,
        "velnor-job-41".to_owned(),
        "w0123456789abcdef0123456789abcdef".to_owned(),
        "a123456789abcdef".to_owned(),
        "b123456789abcdef".to_owned(),
        Some("w0123456789abcdef0123456789abcdef-outer".to_owned()),
        Some("c123456789abcdef".to_owned()),
        None,
    )?;
    assert_eq!(
        linux.outer_network_name(),
        Some("w0123456789abcdef0123456789abcdef-outer")
    );
    assert_eq!(linux.outer_network_id(), Some("c123456789abcdef"));

    assert!(
        WorkerGenerationIdentity::new_with_network(
            41,
            "velnor-job-41".to_owned(),
            "w0123456789abcdef0123456789abcdef".to_owned(),
            "a123456789abcdef".to_owned(),
            "b123456789abcdef".to_owned(),
            Some("unrelated-bridge".to_owned()),
            Some("c123456789abcdef".to_owned()),
            None,
        )
        .is_err()
    );
    assert!(
        WorkerGenerationIdentity::new_with_network(
            41,
            "velnor-job-41".to_owned(),
            "w0123456789abcdef0123456789abcdef".to_owned(),
            "a123456789abcdef".to_owned(),
            "b123456789abcdef".to_owned(),
            Some("w0123456789abcdef0123456789abcdef-outer".to_owned()),
            None,
            None,
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn rest_attempt_and_numeric_job_id_must_be_observed_together() {
    use super::ObservedJobIdentity;

    for (attempt, actions_job_id) in [(Some(4_u32), None), (None, Some(8_765_u64))] {
        assert!(
            ObservedJobIdentity::new(
                17,
                attempt,
                "opaque-scale-set-job".to_owned(),
                actions_job_id,
                29,
                "velnor-job-41".to_owned(),
            )
            .is_err()
        );
    }
}
