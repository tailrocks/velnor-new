//! MBX bundle restore and export path regressions.

use std::error::Error;
use std::fs;

use super::{
    ATTEMPT, Sandbox, assert_unavailable, export_result, import_result,
    import_result_with_handoff_failure, init_store,
};

#[test]
fn warm_import_moves_the_restored_bundle_before_exporting_to_same_path()
-> Result<(), Box<dyn Error>> {
    let sandbox = Sandbox::create()?;
    let root = init_store(&sandbox, ATTEMPT)?;
    let restored = sandbox.path().join("mbx-single-bundle");
    fs::create_dir(&restored)?;
    fs::write(restored.join("payload"), "restored compatible objects\n")?;

    let (imported, outputs, _, github_env) =
        import_result(&sandbox, &root, "compatible-prefix-key", "success")?;
    assert!(
        imported.status.success(),
        "{}",
        String::from_utf8_lossy(&imported.stderr)
    );
    assert!(outputs.contains("cache-state=imported"), "{outputs}");
    assert!(github_env.contains("MBX_CACHE_IMPORT_STATE=imported"));
    assert!(!restored.exists(), "the shared transport path must be free");
    let staging = sandbox.path().join("mbx-single-bundle-imported");
    assert_eq!(
        fs::read_to_string(staging.join("payload"))?,
        "restored compatible objects\n"
    );

    let (exported, outputs, _) = export_result(&sandbox, &root, "imported")?;
    assert!(
        exported.status.success(),
        "{}",
        String::from_utf8_lossy(&exported.stderr)
    );
    assert!(outputs.contains("acceptance=accepted"), "{outputs}");
    assert_eq!(
        fs::read_to_string(restored.join("payload"))?,
        "bundle payload\n"
    );
    assert!(staging.join("payload").is_file());
    Ok(())
}

#[test]
fn uncertain_import_preserves_archive_and_blocks_export() -> Result<(), Box<dyn Error>> {
    let sandbox = Sandbox::create()?;
    let root = init_store(&sandbox, ATTEMPT)?;
    let restored = sandbox.path().join("mbx-single-bundle");
    fs::create_dir(&restored)?;
    fs::write(restored.join("payload"), "restored compatible objects\n")?;

    let (imported, outputs, summary, github_env) =
        import_result(&sandbox, &root, "compatible-prefix-key", "import-fail")?;
    assert!(imported.status.success());
    assert!(outputs.contains("acceptance=cache_corrupt"), "{outputs}");
    assert!(outputs.contains("cache-state=unavailable"), "{outputs}");
    assert!(summary.contains("cache_corrupt"), "{summary}");
    assert!(github_env.contains("MBX_CACHE_IMPORT_STATE=unavailable"));
    assert_eq!(
        fs::read_to_string(restored.join("payload"))?,
        "restored compatible objects\n"
    );

    let (exported, export_outputs, export_summary) =
        export_result(&sandbox, &root, "import-uncertain")?;
    assert_unavailable(exported, &export_outputs, &export_summary);
    assert_eq!(
        fs::read_to_string(restored.join("payload"))?,
        "restored compatible objects\n",
        "uncertain restore must not be replaced by an export"
    );
    assert_eq!(
        fs::read_to_string(root.join("actions/sentinel"))?,
        "store stays owned\n"
    );
    Ok(())
}

#[test]
fn failed_environment_handoff_marks_cache_unavailable_and_keeps_archive()
-> Result<(), Box<dyn Error>> {
    let sandbox = Sandbox::create()?;
    let root = init_store(&sandbox, ATTEMPT)?;
    let restored = sandbox.path().join("mbx-single-bundle");
    fs::create_dir(&restored)?;
    fs::write(restored.join("payload"), "restored compatible objects\n")?;

    let (imported, outputs, _, _) = import_result_with_handoff_failure(
        &sandbox,
        &root,
        "compatible-prefix-key",
        "success",
        Some("env"),
    )?;
    assert!(imported.status.success());
    assert!(outputs.contains("ready=false"), "{outputs}");
    assert!(outputs.contains("cache-state=unavailable"), "{outputs}");
    assert!(
        outputs.contains("acceptance=cache_unavailable"),
        "{outputs}"
    );
    assert!(
        sandbox
            .path()
            .join("mbx-single-bundle-imported/payload")
            .is_file(),
        "a successful import keeps the transport content in private staging"
    );
    Ok(())
}

#[test]
fn failed_output_handoff_fails_step_and_preserves_imported_archive() -> Result<(), Box<dyn Error>> {
    let sandbox = Sandbox::create()?;
    let root = init_store(&sandbox, ATTEMPT)?;
    let restored = sandbox.path().join("mbx-single-bundle");
    fs::create_dir(&restored)?;
    fs::write(restored.join("payload"), "restored compatible objects\n")?;

    let (imported, outputs, _, github_env) = import_result_with_handoff_failure(
        &sandbox,
        &root,
        "compatible-prefix-key",
        "success",
        Some("output"),
    )?;
    assert!(
        !imported.status.success(),
        "output handoff must fail closed"
    );
    assert!(outputs.is_empty(), "{outputs}");
    assert!(github_env.contains("MBX_CACHE_IMPORT_STATE=imported"));
    assert!(
        sandbox
            .path()
            .join("mbx-single-bundle-imported/payload")
            .is_file(),
        "failed output handoff must not delete imported cache content"
    );
    Ok(())
}

#[test]
fn matched_but_missing_restore_marks_cache_corrupt() -> Result<(), Box<dyn Error>> {
    let sandbox = Sandbox::create()?;
    let root = init_store(&sandbox, ATTEMPT)?;
    let (output, outputs, summary, github_env) =
        import_result(&sandbox, &root, "matched-key", "success")?;
    assert!(output.status.success());
    assert!(outputs.contains("acceptance=cache_corrupt"), "{outputs}");
    assert!(outputs.contains("cache-state=unavailable"), "{outputs}");
    assert!(summary.contains("cache_corrupt"), "{summary}");
    assert!(github_env.contains("MBX_CACHE_IMPORT_STATE=unavailable"));
    assert_eq!(
        fs::read_to_string(root.join("actions/sentinel"))?,
        "store stays owned\n"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn symlinked_restore_is_preserved_and_marks_cache_unavailable() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    let sandbox = Sandbox::create()?;
    let root = init_store(&sandbox, ATTEMPT)?;
    let target = sandbox.path().join("restore-target");
    fs::create_dir(&target)?;
    fs::write(target.join("payload"), "foreign cache payload\n")?;
    let restored = sandbox.path().join("mbx-single-bundle");
    symlink(&target, &restored)?;

    let (output, outputs, summary, github_env) =
        import_result(&sandbox, &root, "matched-key", "success")?;
    assert!(output.status.success());
    assert!(
        outputs.contains("acceptance=cache_unavailable"),
        "{outputs}"
    );
    assert!(summary.contains("cache_unavailable"), "{summary}");
    assert!(github_env.contains("MBX_CACHE_IMPORT_STATE=unavailable"));
    assert!(restored.is_symlink());
    assert_eq!(
        fs::read_to_string(target.join("payload"))?,
        "foreign cache payload\n"
    );
    Ok(())
}
