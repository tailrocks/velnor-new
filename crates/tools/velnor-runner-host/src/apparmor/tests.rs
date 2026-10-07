use super::{
    AdmissionFailure, PROFILE_NAMES, ProfileRecord, RunnerProfileAdmission, approved_policy_sha256,
    parse_sha256, verify_observed_profiles,
};

const POLICY_SHA256: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn enforcing_profiles() -> String {
    let mut profiles = String::new();
    for name in PROFILE_NAMES {
        profiles.push_str(name);
        profiles.push_str(" (enforce)\n");
    }
    profiles
}

fn matching_records() -> Vec<ProfileRecord> {
    PROFILE_NAMES
        .iter()
        .enumerate()
        .map(|(index, name)| ProfileRecord {
            directory: format!("{name}.{}", index + 12),
            name: Some((*name).to_owned()),
            raw_sha256: Some(POLICY_SHA256.to_owned()),
        })
        .collect()
}

#[test]
fn admission_requires_an_approved_identity() {
    assert_eq!(approved_policy_sha256(), Err(AdmissionFailure::Unapproved));
}

#[test]
fn missing_profile_fails_closed() {
    assert_eq!(
        verify_observed_profiles(POLICY_SHA256, Some(""), Some(&matching_records())),
        Err(AdmissionFailure::Missing)
    );
    let profiles = "velnor-runner (enforce)\nvelnor-worker (enforce)\n";
    assert_eq!(
        verify_observed_profiles(POLICY_SHA256, Some(profiles), Some(&matching_records()),),
        Err(AdmissionFailure::Missing)
    );
}

#[test]
fn complain_profile_fails_closed() {
    let profiles =
        enforcing_profiles().replace("velnor-worker (enforce)", "velnor-worker (complain)");
    assert_eq!(
        verify_observed_profiles(POLICY_SHA256, Some(&profiles), Some(&matching_records())),
        Err(AdmissionFailure::NotEnforcing)
    );
}

#[test]
fn wrong_policy_hash_fails_closed() {
    let wrong = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";
    let mut records = matching_records();
    records[2].raw_sha256 = Some(wrong.to_owned());
    assert_eq!(
        verify_observed_profiles(POLICY_SHA256, Some(&enforcing_profiles()), Some(&records)),
        Err(AdmissionFailure::WrongHash)
    );
}

#[test]
fn malformed_or_unavailable_policy_hash_fails_closed() {
    assert_eq!(
        parse_sha256("not-readable"),
        Err(AdmissionFailure::MalformedHash)
    );
    assert_eq!(
        verify_observed_profiles(
            POLICY_SHA256,
            Some(&enforcing_profiles()),
            Some(&[
                ProfileRecord {
                    directory: "velnor-runner.1".to_owned(),
                    name: Some("velnor-runner".to_owned()),
                    raw_sha256: Some(String::new()),
                },
                ProfileRecord {
                    directory: "velnor-worker.2".to_owned(),
                    name: Some("velnor-worker".to_owned()),
                    raw_sha256: Some(POLICY_SHA256.to_owned()),
                },
                ProfileRecord {
                    directory: "velnor-job.3".to_owned(),
                    name: Some("velnor-job".to_owned()),
                    raw_sha256: Some(POLICY_SHA256.to_owned()),
                },
            ]),
        ),
        Err(AdmissionFailure::MalformedHash)
    );
    let mut unreadable = matching_records();
    unreadable[1].raw_sha256 = None;
    assert_eq!(
        verify_observed_profiles(
            POLICY_SHA256,
            Some(&enforcing_profiles()),
            Some(&unreadable),
        ),
        Err(AdmissionFailure::Unavailable)
    );
    assert_eq!(
        verify_observed_profiles(POLICY_SHA256, Some(&enforcing_profiles()), None),
        Err(AdmissionFailure::Unavailable)
    );
}

#[test]
fn missing_duplicate_and_unknown_policy_hashes_fail_closed() {
    let mut missing = matching_records();
    missing.pop();
    assert_eq!(
        verify_observed_profiles(POLICY_SHA256, Some(&enforcing_profiles()), Some(&missing),),
        Err(AdmissionFailure::Missing)
    );

    let mut duplicate = matching_records();
    duplicate.push(ProfileRecord {
        directory: "velnor-job.999".to_owned(),
        name: Some("velnor-job".to_owned()),
        raw_sha256: Some(POLICY_SHA256.to_owned()),
    });
    assert_eq!(
        verify_observed_profiles(POLICY_SHA256, Some(&enforcing_profiles()), Some(&duplicate),),
        Err(AdmissionFailure::Ambiguous)
    );

    let mut unknown = matching_records();
    unknown.push(ProfileRecord {
        directory: "velnor-job..extra.1".to_owned(),
        name: Some("velnor-job//extra".to_owned()),
        raw_sha256: Some(POLICY_SHA256.to_owned()),
    });
    assert_eq!(
        verify_observed_profiles(POLICY_SHA256, Some(&enforcing_profiles()), Some(&unknown),),
        Err(AdmissionFailure::UnknownProfile)
    );
}

#[test]
fn unavailable_kernel_policy_state_fails_closed() {
    assert_eq!(
        verify_observed_profiles(POLICY_SHA256, None, None),
        Err(AdmissionFailure::Unavailable)
    );
}

#[test]
fn duplicate_profile_names_fail_closed() {
    assert_eq!(
        verify_observed_profiles(
            POLICY_SHA256,
            Some(&format!(
                "{}velnor-runner (enforce)\n",
                enforcing_profiles()
            )),
            Some(&matching_records()),
        ),
        Err(AdmissionFailure::Ambiguous)
    );
}

#[test]
fn unknown_and_child_profiles_fail_closed() {
    assert_eq!(
        verify_observed_profiles(
            POLICY_SHA256,
            Some(&format!("{}velnor-extra (enforce)\n", enforcing_profiles())),
            Some(&matching_records()),
        ),
        Err(AdmissionFailure::UnknownProfile)
    );
    assert_eq!(
        verify_observed_profiles(
            POLICY_SHA256,
            Some(&format!(
                "{}velnor-worker//unexpected (enforce)\n",
                enforcing_profiles()
            )),
            Some(&matching_records()),
        ),
        Err(AdmissionFailure::UnknownProfile)
    );
}

#[test]
fn exact_enforcing_profile_hash_admits() -> Result<(), AdmissionFailure> {
    let admission = verify_observed_profiles(
        POLICY_SHA256,
        Some(&enforcing_profiles()),
        Some(&matching_records()),
    )?;
    assert_eq!(admission, RunnerProfileAdmission { _seal: () });
    Ok(())
}

#[test]
fn numbered_profile_directories_resolve_by_authoritative_names() -> Result<(), AdmissionFailure> {
    verify_observed_profiles(
        POLICY_SHA256,
        Some(&enforcing_profiles()),
        Some(&matching_records()),
    )?;
    Ok(())
}

#[test]
fn mismatched_or_unqualified_profile_directory_fails_closed() {
    let mut mismatched = matching_records();
    mismatched[0].directory = "velnor-worker.12".to_owned();
    assert_eq!(
        verify_observed_profiles(
            POLICY_SHA256,
            Some(&enforcing_profiles()),
            Some(&mismatched),
        ),
        Err(AdmissionFailure::MismatchedDirectory)
    );

    let mut unnumbered = matching_records();
    unnumbered[1].directory = "velnor-worker".to_owned();
    assert_eq!(
        verify_observed_profiles(
            POLICY_SHA256,
            Some(&enforcing_profiles()),
            Some(&unnumbered),
        ),
        Err(AdmissionFailure::MismatchedDirectory)
    );
}

#[test]
fn mismatched_and_unknown_authoritative_names_fail_closed() {
    let mut mismatched = matching_records();
    mismatched[0].name = Some("velnor-worker".to_owned());
    assert_eq!(
        verify_observed_profiles(
            POLICY_SHA256,
            Some(&enforcing_profiles()),
            Some(&mismatched),
        ),
        Err(AdmissionFailure::MismatchedDirectory)
    );

    let mut unknown = matching_records();
    unknown.push(ProfileRecord {
        directory: "velnor-other.87".to_owned(),
        name: Some("velnor-other".to_owned()),
        raw_sha256: Some(POLICY_SHA256.to_owned()),
    });
    assert_eq!(
        verify_observed_profiles(POLICY_SHA256, Some(&enforcing_profiles()), Some(&unknown),),
        Err(AdmissionFailure::UnknownProfile)
    );
}
