use super::*;

#[test]
fn release_role_mapping_is_channel_closed_and_payload_names_come_from_full_plan() {
    let request = crate::package_release_publish::preview_artifacts::fixture::request();
    for channel in [
        PackageReleaseChannel::Preview,
        PackageReleaseChannel::Stable,
    ] {
        let mut plan = request.plan.clone();
        plan.channel = channel;
        let prefix = if channel == PackageReleaseChannel::Preview {
            "preview"
        } else {
            "stable"
        };
        for pair in expected_release_payload_pairs(&plan).expect("valid fixture authority") {
            let id = release_payload_job_id(&plan, &pair).expect("valid fixture authority");
            let expected = format!("Build {} / {}", pair.product_id, pair.target);
            if channel == PackageReleaseChannel::Preview {
                assert_eq!(id, "preview-payload");
                assert_eq!(
                    preview_payload_job_name(&plan, &pair.product_id, &pair.target)
                        .expect("validated preview matrix row"),
                    expected
                );
            } else {
                assert_eq!(
                    release_job_name(&plan, &id).expect("valid fixture authority"),
                    expected
                );
            }
        }
        assert_eq!(
            release_job_name(&plan, &format!("{prefix}-publisher"))
                .expect("valid fixture authority"),
            "Publish authenticated channel release"
        );
        for bad in [
            "other-publisher",
            "preview-payload-999",
            "stable-payload-999",
            "preview-tap-selection",
        ] {
            release_job_name(&plan, bad).expect_err("untrusted authority must fail");
        }
        let other = if prefix == "preview" {
            "stable"
        } else {
            "preview"
        };
        release_job_name(&plan, &format!("{other}-publisher"))
            .expect_err("untrusted authority must fail");
        if channel == PackageReleaseChannel::Preview {
            assert!(preview_payload_job_name(&plan, "foreign", "aarch64-apple-darwin").is_err());
        }
        continuation_job_name(channel, &format!("{other}-tap-selection"))
            .expect_err("untrusted authority must fail");
    }
}
