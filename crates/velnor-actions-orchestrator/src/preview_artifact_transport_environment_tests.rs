use super::*;

#[test]
fn native_consumer_environment_is_closed_and_role_specific() {
    for role in [ReleaseConsumer::Assembler, ReleaseConsumer::Signer] {
        validate_consumer_environment(
            role,
            velnor_actions_contract::package_release::PackageReleaseChannel::Preview,
            Some(role.job_name(
                velnor_actions_contract::package_release::PackageReleaseChannel::Preview,
            )),
        )
        .expect("exact native role");
        for actual in [
            None,
            Some("preview-producer"),
            Some("preview-payload-000"),
            Some("unknown"),
        ] {
            assert!(
                validate_consumer_environment(
                    role,
                    velnor_actions_contract::package_release::PackageReleaseChannel::Preview,
                    actual
                )
                .is_err()
            );
        }
    }
    assert!(
        validate_consumer_environment(
            ReleaseConsumer::Signer,
            velnor_actions_contract::package_release::PackageReleaseChannel::Preview,
            Some("preview-assemble")
        )
        .is_err()
    );
    assert!(
        validate_consumer_environment(
            ReleaseConsumer::Assembler,
            velnor_actions_contract::package_release::PackageReleaseChannel::Preview,
            Some("preview-signer")
        )
        .is_err()
    );
}
