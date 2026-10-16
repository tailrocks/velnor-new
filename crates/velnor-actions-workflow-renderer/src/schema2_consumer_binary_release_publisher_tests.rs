#[path = "schema2_consumer_binary_release_publisher_stub.rs"]
mod stub;
#[path = "schema2_consumer_binary_release_publisher_support.rs"]
mod support;

macro_rules! publish_test {
    ($name:ident, $scenario:literal) => {
        #[test]
        fn $name() {
            support::run_publish_scenario($scenario);
        }
    };
}

publish_test!(publisher_scenario_success, "success");
publish_test!(publisher_scenario_immutable_disabled, "immutable-disabled");
publish_test!(
    publisher_scenario_environment_unprotected,
    "environment-unprotected"
);
publish_test!(
    publisher_scenario_environment_missing,
    "environment-missing"
);
publish_test!(publisher_scenario_missing_token, "missing-token");
publish_test!(publisher_scenario_stale_before_tag, "stale-before-tag");
publish_test!(
    publisher_scenario_stale_before_publish,
    "stale-before-publish"
);
publish_test!(publisher_scenario_tag_collision, "tag-collision");
publish_test!(publisher_scenario_partial_collision, "partial-collision");
publish_test!(publisher_scenario_non_404, "non-404");
publish_test!(publisher_scenario_mutable_release, "mutable-release");
publish_test!(
    publisher_scenario_extra_release_asset,
    "extra-release-asset"
);
publish_test!(
    publisher_scenario_download_extra_file,
    "download-extra-file"
);
publish_test!(
    publisher_scenario_download_corrupt_bytes,
    "download-corrupt-bytes"
);
publish_test!(
    publisher_scenario_download_extra_checksum,
    "download-extra-checksum"
);
publish_test!(
    publisher_scenario_final_attestation_failed,
    "final-attestation-failed"
);
