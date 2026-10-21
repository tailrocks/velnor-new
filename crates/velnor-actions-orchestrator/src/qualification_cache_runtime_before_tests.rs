//! Exercise authenticated BEFORE snapshots after artifact download.

use super::*;

use std::os::unix::fs::PermissionsExt;

use velnor_actions_contract::workflow::QualificationCacheBackendObservation;

#[test]
fn downloaded_before_snapshot_is_validated_without_a_private_mode_requirement() {
    let fixture = Fixture::cold();
    let context = fixture.plan.qualification.as_ref().expect("context");
    let snapshot = crate::qualification_cache_backend::complete_snapshot_for_test(context)
        .expect("complete snapshot");
    crate::qualification_cache_backend::stage_qualification_cache_before(
        fixture.runner_temp.path(),
        &snapshot,
    )
    .expect("stage BEFORE snapshot");
    let snapshot_path = fixture
        .runner_temp
        .path()
        .join("velnor")
        .join(&fixture.plan.run_key)
        .join(crate::qualification_cache_backend::QUALIFICATION_CACHE_BACKEND_BEFORE_FILENAME);
    fs::set_permissions(&snapshot_path, fs::Permissions::from_mode(0o644))
        .expect("simulate downloaded artifact permissions");

    let request = fixture.request();
    let key = bind(&request).restore_key.expect("bound restore key");
    assert_eq!(
        qualification_cache_before_observation(&request, &key).expect("validated observation"),
        QualificationCacheBackendObservation::Absent
    );
}
