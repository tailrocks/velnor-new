use super::*;
use std::fs;
use std::os::unix::fs::PermissionsExt;

#[test]
fn safe_staged_root_mode_change_is_rejected_before_live_import() {
    let temp = tempfile::tempdir().expect("runner temp");
    let runner_temp = runner_temp(&temp, "root-mode");
    let roots = roots(&runner_temp);
    create_live_roots(&roots, "cold-state");
    fs::set_permissions(&roots.roots[3].path, fs::Permissions::from_mode(0o750))
        .expect("source root mode");
    let exported = export_payload(&binding(None, None), &roots, &runner_temp).expect("export");
    create_live_roots(&roots, "stale-state");
    let live_file = fs::read(&roots.roots[1].path).expect("live file before import");
    let stage = exported.save_stage_path.as_ref().expect("save stage");
    fs::set_permissions(
        stage.join("roots/mise_data"),
        fs::Permissions::from_mode(0o755),
    )
    .expect("changed safe root mode");

    let result = import_payload(
        &binding(Some(cache_entry(&runner_temp)), Some(exported.state_digest)),
        &roots,
        &runner_temp,
        true,
        Some(CACHE_KEY),
        &QualificationCacheBackendObservation::Found(cache_entry(&runner_temp)),
    );

    assert!(result.is_err(), "changed root mode was admitted");
    assert_eq!(
        fs::read(&roots.roots[1].path).expect("live file after import"),
        live_file
    );
}

#[test]
fn group_or_world_writable_cache_root_is_rejected_before_export() {
    let temp = tempfile::tempdir().expect("runner temp");
    let runner_temp = runner_temp(&temp, "unsafe-root-mode");
    let roots = roots(&runner_temp);
    create_live_roots(&roots, "cold-state");
    fs::set_permissions(&roots.roots[3].path, fs::Permissions::from_mode(0o777))
        .expect("unsafe root mode");

    assert!(export_payload(&binding(None, None), &roots, &runner_temp).is_err());
    assert!(!runner_temp.join("velnor/qualification-cache-v3").exists());
}
