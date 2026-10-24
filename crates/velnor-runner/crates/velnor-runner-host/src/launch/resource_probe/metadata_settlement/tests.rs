use super::verify_startup_evidence;
use crate::launch::resource_probe::execute::ProbeStartupEvidence;

#[test]
fn accepts_only_cleanup_and_image_evidence_for_the_claimed_guest() {
    let root = "a".repeat(64);
    let evidence = ProbeStartupEvidence::test_fixture("operation", "engine", &root);
    assert_eq!(
        verify_startup_evidence(Some(&evidence), "operation", "engine", &root),
        Ok(())
    );
    assert!(verify_startup_evidence(Some(&evidence), "other", "engine", &root).is_err());
    assert!(verify_startup_evidence(Some(&evidence), "operation", "foreign", &root).is_err());
    assert!(
        verify_startup_evidence(Some(&evidence), "operation", "engine", &"b".repeat(64)).is_err()
    );
    assert!(verify_startup_evidence(None, "operation", "engine", &root).is_err());
}
