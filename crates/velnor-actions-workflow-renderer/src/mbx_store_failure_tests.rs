//! MBX export refusal regressions for unowned or redirected paths.

use std::error::Error;
use std::fs;

use super::{
    ATTEMPT, Sandbox, assert_unavailable, export_result, export_with_cache_root, init_store,
};

#[test]
fn shared_root_and_existing_bundle_are_preserved_and_rejected() -> Result<(), Box<dyn Error>> {
    let sandbox = Sandbox::create()?;
    let root = init_store(&sandbox, ATTEMPT)?;
    let shared = sandbox.path().join("shared-mbx-root");
    fs::create_dir_all(shared.join("actions"))?;
    fs::write(shared.join("actions/sentinel"), "shared stays owned\n")?;
    let (output, outputs, summary) = export_with_cache_root(&sandbox, &shared, "success")?;
    assert_unavailable(output, &outputs, &summary);
    assert_eq!(
        fs::read_to_string(shared.join("actions/sentinel"))?,
        "shared stays owned\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("actions/sentinel"))?,
        "store stays owned\n"
    );

    let bundle = sandbox.path().join("mbx-single-bundle");
    fs::create_dir(&bundle)?;
    fs::write(bundle.join("prior-payload"), "prior export stays owned\n")?;
    let (output, outputs, summary) = export_result(&sandbox, &root, "success")?;
    assert_unavailable(output, &outputs, &summary);
    assert_eq!(
        fs::read_to_string(bundle.join("prior-payload"))?,
        "prior export stays owned\n"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn symlinked_roots_and_action_stores_are_not_exported() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    let sandbox = Sandbox::create()?;
    let root = init_store(&sandbox, ATTEMPT)?;
    let target = sandbox.path().join("private-store-target");
    fs::rename(&root, &target)?;
    symlink(&target, &root)?;
    let (output, outputs, summary) = export_result(&sandbox, &root, "success")?;
    assert_unavailable(output, &outputs, &summary);
    assert_eq!(
        fs::read_to_string(target.join("actions/sentinel"))?,
        "store stays owned\n"
    );
    fs::remove_file(&root)?;
    fs::remove_dir_all(target)?;

    let sandbox = Sandbox::create()?;
    let root = init_store(&sandbox, ATTEMPT)?;
    let outside = sandbox.path().join("outside-actions");
    fs::create_dir(&outside)?;
    fs::write(outside.join("sentinel"), "outside stays owned\n")?;
    fs::remove_dir(root.join("actions"))?;
    symlink(&outside, root.join("actions"))?;
    let (output, outputs, summary) = export_result(&sandbox, &root, "success")?;
    assert_unavailable(output, &outputs, &summary);
    assert_eq!(
        fs::read_to_string(outside.join("sentinel"))?,
        "outside stays owned\n"
    );
    fs::remove_file(root.join("actions"))?;
    fs::remove_dir_all(outside)?;
    Ok(())
}
