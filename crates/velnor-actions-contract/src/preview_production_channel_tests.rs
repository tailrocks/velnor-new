//! Channel-specific production identity controls.
use super::*;

#[test]
fn stable_uses_full_inventory_and_exact_base_version() {
    let mut candidate = request();
    candidate.plan.channel = crate::package_release::PackageReleaseChannel::Stable;
    candidate.plan_sha256 = release_plan_sha256(&candidate.plan).expect("stable digest");
    candidate.context.version = candidate.plan.base_version.clone();
    candidate.context.commit_count = 0;
    candidate.validate().expect("shared stable production");
    assert_eq!(
        expected_release_payload_pairs(&candidate.plan)
            .expect("pairs")
            .len(),
        6
    );
    for (index, pair) in expected_release_payload_pairs(&candidate.plan)
        .expect("pairs")
        .iter()
        .enumerate()
    {
        assert_eq!(
            release_payload_job_id(&candidate.plan, pair).expect("job"),
            format!("stable-payload-{index:03}")
        );
    }
    candidate.context.version.push_str("-preview.1+aaaaaaa");
    assert!(candidate.validate().is_err());
}
