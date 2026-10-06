use super::*;
use velnor_actions_contract::StepKind;

#[test]
fn ownership_is_closed_and_binds_literal_key_and_private_home() {
    for root in ["", "root", "literal'$`/space dir"] {
        let owner = record(root, env!("CARGO_PKG_VERSION")).expect("owner record");
        assert_eq!(
            owner.invocation().args(),
            &[velnor_actions_tofu::key_for_root(root)]
        );
        assert!(owner.invocation().installed_selectors().is_empty());
        assert_eq!(owner.environment().len(), 9);
        assert_eq!(
            owner.environment()["HOME"],
            producer_home(root).expect("home")
        );
        assert_eq!(owner.environment()["PATH"], "/usr/bin:/bin");
        assert_eq!(
            owner.invocation().descriptor().source_sha256(),
            velnor_actions_contract::compiled_source_sha256(owner.source().as_bytes())
        );
        assert_eq!(
            owner.invocation().descriptor().operation(),
            SourceBoundOperation::TofuRootOwnership
        );
        let emitted = step(&owner).expect("closed step");
        assert_eq!(
            emitted.id.expect("fixed id").as_str(),
            "velnor-tofu-root-ownership"
        );
        let StepKind::SourceBoundHelper { invocation, env } = emitted.kind else {
            panic!("compiled prerequisite");
        };
        assert_eq!(&invocation, owner.invocation());
        assert_eq!(&env, owner.environment());
    }
}

#[test]
fn owner_rejects_version_drift_and_keeps_repository_root_distinct() {
    assert!(record("", "different-version").is_err());
    assert_ne!(
        producer_home("").expect("repository"),
        producer_home("root").expect("literal")
    );
}
