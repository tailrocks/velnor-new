use super::*;
use velnor_actions_contract::SourceProducerRole;

fn role_admission(role: Option<SourceProducerRole>) -> ProducerAdmission {
    let mut admission = admission();
    if let Some(role) = role {
        let source = serde_json::from_value(serde_json::json!({
            "role": role, "selection": {"tasks": [], "cargo_fallback": false,
                "unconditional": false}, "source_identity": "source-key",
            "verification_step": "verify", "restore_step": "restore",
            "save_step": "save", "publication_step": "lookup", "report_step": "report"
        }))
        .expect("source producer");
        admission.role = ProducerRole::Source { producer: source };
    }
    admission
}

#[test]
fn every_role_error_availability_and_conclusion_is_closed() {
    use ProducerTerminalError as Error;
    let errors = [
        Error::None,
        Error::PreparationFailed,
        Error::CacheNotPublished,
        Error::CacheTransportFailed,
        Error::CacheTransportUnavailable,
        Error::PrivateOrAuthRequired,
        Error::PublicAuthorityUnavailable,
        Error::UnsupportedRegistry,
        Error::SourceVerificationFailed,
    ];
    let roles = [
        None,
        Some(SourceProducerRole::Cargo),
        Some(SourceProducerRole::Npm),
        Some(SourceProducerRole::Bun),
        Some(SourceProducerRole::Gradle),
        Some(SourceProducerRole::Tofu),
    ];
    let conclusions = [
        JobConclusion::Success,
        JobConclusion::Failure,
        JobConclusion::Cancelled,
        JobConclusion::Skipped,
        JobConclusion::Neutral,
        JobConclusion::Missing,
    ];
    for role in roles {
        let admission = role_admission(role);
        for error in errors {
            for (verified, available) in
                [(false, false), (false, true), (true, false), (true, true)]
            {
                let expected = match error {
                    Error::None => verified && available,
                    Error::PreparationFailed => !verified && !available,
                    Error::CacheNotPublished | Error::CacheTransportUnavailable => {
                        verified && !available
                    }
                    Error::CacheTransportFailed => verified,
                    _ => role.is_some() && !verified && !available,
                };
                let identity = if role.is_some() {
                    "source-key"
                } else {
                    "tool-qualified-key"
                };
                let mut report = ProducerReport {
                    job_id: admission.job_id.clone(),
                    identity: identity.to_owned(),
                    verified,
                    cache_available: available,
                    error,
                };
                assert_eq!(report.matches(&admission), expected, "{role:?}/{error:?}");
                for conclusion in conclusions {
                    assert_eq!(
                        report.terminal_success(&admission, conclusion),
                        expected && error == Error::None && conclusion == JobConclusion::Success
                    );
                    assert_eq!(
                        advisory_failure(&admission, conclusion, Some(&report)),
                        expected && conclusion == JobConclusion::Failure
                    );
                }
                report.identity = "foreign".to_owned();
                assert!(!report.matches(&admission));
                assert!(!report.terminal_success(&admission, JobConclusion::Success));
            }
        }
    }
}

#[test]
fn mandatory_producer_retains_transport_failure_even_with_current_key_availability() {
    let mut plan = plan();
    plan.producers.entries[0].policy = ProducerPolicy::Mandatory;
    let mut request = request("success");
    request.producer_reports = vec![ProducerReport {
        verified: true,
        cache_available: true,
        error: ProducerTerminalError::CacheTransportFailed,
        ..terminal()
    }];
    let mut signals = Signals::default();
    required_evidence::fold_jobs(&plan, &request, &mut signals);
    assert!(signals.failed);
}
