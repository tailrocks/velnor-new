use super::super::{AdmissionFailure, verify_observed_profiles};
use super::{POLICY_SHA256, enforcing_profiles, matching_records};

#[test]
fn missing_profile_fails_closed() {
    assert_eq!(
        verify_observed_profiles(POLICY_SHA256, Some(""), Some(&matching_records())),
        Err(AdmissionFailure::Missing)
    );
    let profiles = "velnor-runner (enforce)\n";
    assert_eq!(
        verify_observed_profiles(POLICY_SHA256, Some(profiles), Some(&[]),),
        Err(AdmissionFailure::Missing)
    );
}

#[test]
fn complain_profile_fails_closed() {
    let profiles =
        enforcing_profiles().replace("velnor-runner (enforce)", "velnor-runner (complain)");
    assert_eq!(
        verify_observed_profiles(POLICY_SHA256, Some(&profiles), Some(&matching_records())),
        Err(AdmissionFailure::NotEnforcing)
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
                "{}velnor-runner//unexpected (enforce)\n",
                enforcing_profiles()
            )),
            Some(&matching_records()),
        ),
        Err(AdmissionFailure::UnknownProfile)
    );
}
