//! MBX export ownership and cache-acceptance regressions.

use std::error::Error;
use std::fs;

use super::{ATTEMPT, Sandbox, assert_unavailable, export_result, init_store};

#[test]
fn successful_export_keeps_private_store_and_reports_runner_temp_cleanup()
-> Result<(), Box<dyn Error>> {
    let sandbox = Sandbox::create()?;
    let root = init_store(&sandbox, ATTEMPT)?;
    let (output, outputs, summary) = export_result(&sandbox, &root, "success")?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(outputs.contains("ready=true"), "{outputs}");
    assert!(outputs.contains("acceptance=accepted"), "{outputs}");
    assert!(outputs.contains("cleanup=runner-temp"), "{outputs}");
    assert!(summary.contains("accepted"), "{summary}");
    assert_eq!(
        fs::read_to_string(root.join("actions/sentinel"))?,
        "store stays owned\n"
    );
    assert!(root.join(".velnor-mbx-owner").is_file());
    assert_eq!(
        fs::read_to_string(sandbox.path().join("mbx-single-bundle/payload"))?,
        "bundle payload\n"
    );
    Ok(())
}

#[test]
fn ownership_and_export_failures_preserve_the_store_and_report_unavailable()
-> Result<(), Box<dyn Error>> {
    for failure in [
        "missing-marker",
        "forged-marker",
        "wrong-store",
        "gc-fail",
        "export-fail",
        "partial-export",
    ] {
        let sandbox = Sandbox::create()?;
        let root = init_store(&sandbox, ATTEMPT)?;
        match failure {
            "missing-marker" => fs::remove_file(root.join(".velnor-mbx-owner"))?,
            "forged-marker" => fs::write(root.join(".velnor-mbx-owner"), "another job\n")?,
            _ => {}
        }
        let (output, outputs, summary) = export_result(&sandbox, &root, failure)?;
        assert_unavailable(output, &outputs, &summary);
        assert_eq!(
            fs::read_to_string(root.join("actions/sentinel"))?,
            "store stays owned\n",
            "failure {failure} must preserve the store"
        );
        if failure == "partial-export" {
            assert!(sandbox.path().join("mbx-single-bundle/payload").is_file());
        }
    }
    Ok(())
}

#[test]
fn empty_export_is_no_entry_and_readonly_hardlinks_are_unchanged() -> Result<(), Box<dyn Error>> {
    let sandbox = Sandbox::create()?;
    let root = init_store(&sandbox, ATTEMPT)?;
    let (output, outputs, summary) = export_result(&sandbox, &root, "no-entry")?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(outputs.contains("ready=false"), "{outputs}");
    assert!(outputs.contains("acceptance=no_entry"), "{outputs}");
    assert!(!outputs.contains("cache_unavailable"), "{outputs}");
    assert!(summary.contains("no_entry"), "{summary}");
    assert_eq!(
        fs::read_to_string(root.join("actions/sentinel"))?,
        "store stays owned\n"
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let sandbox = Sandbox::create()?;
        let root = init_store(&sandbox, ATTEMPT)?;
        let path = root.join("actions/sentinel");
        let alias = sandbox.path().join("outside-hardlink");
        fs::hard_link(&path, &alias)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o444))?;
        fs::set_permissions(root.join("actions"), fs::Permissions::from_mode(0o555))?;
        let original = fs::metadata(&path)?;
        let (output, outputs, _) = export_result(&sandbox, &root, "success")?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(outputs.contains("acceptance=accepted"), "{outputs}");
        let after = fs::metadata(&path)?;
        let outside = fs::metadata(&alias)?;
        assert_eq!(after.ino(), original.ino());
        assert_eq!(outside.ino(), original.ino());
        assert_eq!(after.permissions().mode() & 0o777, 0o444);
        assert_eq!(fs::read_to_string(alias)?, "store stays owned\n");
        fs::set_permissions(root.join("actions"), fs::Permissions::from_mode(0o700))?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}
