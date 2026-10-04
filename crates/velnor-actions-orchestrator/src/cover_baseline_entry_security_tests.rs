//! Focused producer and evidence gate cases.

use super::*;

/// Attempt and artifact pins: a claim of attempt 1 never loads when the
/// run succeeded on attempt 3, and a foreign artifact id never loads.
#[test]
fn baseline_entry_pins_attempt_and_artifact() {
    let base = "a".repeat(40);
    let name = format!("velnor-baseline-{base}-{}", digest_b3(b"c"));
    let numeric = crate::cover_compat::baseline_artifact_numeric_id(&name);
    let tmp = tempfile::tempdir().expect("tempdir");
    let entry = tmp.path().join(&name);
    std::fs::create_dir(&entry).expect("entry");
    let manifest = manifest_json(&base, &name);
    std::fs::write(entry.join("baseline.json"), manifest.to_string()).expect("manifest");
    assert!(
        baseline_entry_for(&entry, &base, 7, 3, numeric).is_none(),
        "claims attempt 1, success on 3"
    );
    assert!(
        baseline_entry_for(&entry, &base, 7, 1, numeric.wrapping_add(1)).is_none(),
        "foreign artifact id"
    );
    assert!(baseline_entry_for(&entry, &base, 7, 1, numeric).is_some());
}

/// Symlink, traversal, and size gates: a linked payload, a linked entry
/// dir, a directory payload, and an oversize payload never load, even
/// when every name and claim is otherwise valid.
#[test]
fn baseline_entry_rejects_links_and_oversize() {
    let base = "a".repeat(40);
    let name = format!("velnor-baseline-{base}-{}", digest_b3(b"c"));
    let numeric = crate::cover_compat::baseline_artifact_numeric_id(&name);
    #[cfg(unix)]
    {
        let manifest = manifest_json(&base, &name).to_string();
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(tmp.path().join("real.json"), &manifest).expect("real");
        let linked = tmp.path().join(&name);
        std::fs::create_dir(&linked).expect("linked");
        std::os::unix::fs::symlink(tmp.path().join("real.json"), linked.join("baseline.json"))
            .expect("link");
        assert!(
            baseline_entry_for(&linked, &base, 7, 1, numeric).is_none(),
            "symlinked payload rejects even at a live target"
        );
        let tmp = tempfile::tempdir().expect("tempdir");
        let target = tmp.path().join("target");
        std::fs::create_dir(&target).expect("target");
        std::fs::write(target.join("baseline.json"), &manifest).expect("manifest");
        let via = tmp.path().join(&name);
        std::os::unix::fs::symlink(&target, &via).expect("dir link");
        assert!(
            baseline_entry_for(&via, &base, 7, 1, numeric).is_none(),
            "symlinked entry dir rejects"
        );
    }
    let tmp = tempfile::tempdir().expect("tempdir");
    let entry = tmp.path().join(&name);
    std::fs::create_dir(&entry).expect("entry");
    std::fs::create_dir(entry.join("baseline.json")).expect("dir payload");
    assert!(
        baseline_entry_for(&entry, &base, 7, 1, numeric).is_none(),
        "directory payload rejects"
    );
    std::fs::remove_dir(entry.join("baseline.json")).expect("rmdir");
    let big = format!(
        r#"{{"schema":2,"pad":"{}"}}"#,
        "p".repeat(MAX_BASELINE_MANIFEST_BYTES)
    );
    std::fs::write(entry.join("baseline.json"), big).expect("big");
    assert!(
        baseline_entry_for(&entry, &base, 7, 1, numeric).is_none(),
        "oversize payload rejects"
    );
}
