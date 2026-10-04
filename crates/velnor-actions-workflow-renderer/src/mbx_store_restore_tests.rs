//! MBX bundle restore and export path regressions.

use std::error::Error;
use std::fs;

use super::{ATTEMPT, Sandbox, assert_unavailable, export_result, import_result, init_store};

#[test]
fn warm_prefix_import_and_next_export_use_distinct_bundle_paths() -> Result<(), Box<dyn Error>> {
    let sandbox = Sandbox::create()?;
    let root = init_store(&sandbox, ATTEMPT)?;
    let restored = sandbox.path().join("mbx-single-bundle-restore");
    fs::create_dir(&restored)?;
    fs::write(restored.join("payload"), "restored compatible objects\n")?;

    let (imported, outputs, _, _) =
        import_result(&sandbox, &root, "compatible-prefix-key", "success")?;
    assert!(
        imported.status.success(),
        "{}",
        String::from_utf8_lossy(&imported.stderr)
    );
    assert!(outputs.is_empty(), "{outputs}");
    assert!(restored.join("payload").is_file());

    let (exported, outputs, _) = export_result(&sandbox, &root, "success")?;
    assert!(
        exported.status.success(),
        "{}",
        String::from_utf8_lossy(&exported.stderr)
    );
    assert!(outputs.contains("acceptance=accepted"), "{outputs}");
    assert_eq!(
        fs::read_to_string(restored.join("payload"))?,
        "restored compatible objects\n"
    );
    assert!(
        sandbox
            .path()
            .join("mbx-single-bundle-export/payload")
            .is_file()
    );
    Ok(())
}

#[test]
fn uncertain_import_preserves_archive_and_blocks_cache_export() -> Result<(), Box<dyn Error>> {
    let sandbox = Sandbox::create()?;
    let root = init_store(&sandbox, ATTEMPT)?;
    let restored = sandbox.path().join("mbx-single-bundle-restore");
    fs::create_dir(&restored)?;
    fs::write(restored.join("payload"), "restored compatible objects\n")?;

    let (imported, outputs, summary, github_env) =
        import_result(&sandbox, &root, "compatible-prefix-key", "import-fail")?;
    assert!(imported.status.success());
    assert!(outputs.contains("acceptance=cache_corrupt"), "{outputs}");
    assert!(summary.contains("cache_corrupt"), "{summary}");
    assert!(
        github_env.contains("MBX_CACHE_IMPORT_UNAVAILABLE=true"),
        "{github_env}"
    );
    assert_eq!(
        fs::read_to_string(restored.join("payload"))?,
        "restored compatible objects\n"
    );

    let (exported, export_outputs, export_summary) =
        export_result(&sandbox, &root, "import-uncertain")?;
    assert!(exported.status.success());
    assert_unavailable(exported, &export_outputs, &export_summary);
    assert!(
        !sandbox.path().join("mbx-single-bundle-export").exists(),
        "uncertain restored cache must not publish a new bundle"
    );
    assert_eq!(
        fs::read_to_string(root.join("actions/sentinel"))?,
        "store stays owned\n"
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
    assert!(summary.contains("cache_corrupt"), "{summary}");
    assert!(github_env.contains("MBX_CACHE_IMPORT_UNAVAILABLE=true"));
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
    let restored = sandbox.path().join("mbx-single-bundle-restore");
    symlink(&target, &restored)?;

    let (output, outputs, summary, github_env) =
        import_result(&sandbox, &root, "matched-key", "success")?;
    assert!(output.status.success());
    assert!(
        outputs.contains("acceptance=cache_unavailable"),
        "{outputs}"
    );
    assert!(summary.contains("cache_unavailable"), "{summary}");
    assert!(github_env.contains("MBX_CACHE_IMPORT_UNAVAILABLE=true"));
    assert!(restored.is_symlink());
    assert_eq!(
        fs::read_to_string(target.join("payload"))?,
        "foreign cache payload\n"
    );
    Ok(())
}
