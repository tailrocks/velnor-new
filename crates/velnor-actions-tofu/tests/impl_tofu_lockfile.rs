//! Root-lock slot, lockfile inspection, and lock snapshot cases.
use velnor_actions_tofu::TofuTaskKind;
use velnor_actions_tofu::lockfile::{
    LOCKFILE_CORRUPT, TofuLockSnapshot, inspect_lockfile, lock_digest_at_root, lock_slot_for_kind,
};
use velnor_actions_tofu::task_identity::DigestSlot;

use crate::support::{Outcome, TempDir, fixture_dir};

/// Lock bytes of one named `tofu-lockfile` fixture root.
fn fixture_lock(name: &str) -> Option<String> {
    std::fs::read_to_string(
        fixture_dir("tofu-lockfile")
            .join(name)
            .join(".terraform.lock.hcl"),
    )
    .ok()
}

#[test]
fn root_lock_slot_binds_content_absence_and_ignorance() -> Outcome {
    let dir = TempDir::create("tofu-lock-slot")?;
    let absent = lock_digest_at_root(dir.path(), ".");
    assert!(matches!(absent, DigestSlot::AbsentProven(_)));
    assert_eq!(absent.state(), velnor_actions_tofu::SlotState::AbsentProven);
    dir.write(".terraform.lock.hcl", "lock-bytes\n")?;
    let known = lock_digest_at_root(dir.path(), ".");
    assert!(matches!(known, DigestSlot::Known(_)));
    assert!(
        known
            .as_known()
            .is_some_and(|digest| digest.starts_with("b3-"))
    );
    assert_ne!(known, absent, "content differs from absence");
    let nested = lock_digest_at_root(dir.path(), "stacks/a");
    assert!(matches!(nested, DigestSlot::AbsentProven(_)));
    assert!(nested.as_known().is_none());
    Ok(())
}

#[test]
fn root_lock_slot_never_collapses_absence_and_ignorance() -> Outcome {
    let dir = TempDir::create("tofu-lock-unknown")?;
    // A directory where the lock belongs: `read` fails with a
    // non-`NotFound` IO error on every platform, even for root.
    std::fs::create_dir(dir.path().join(".terraform.lock.hcl"))?;
    let unknown = lock_digest_at_root(dir.path(), ".");
    assert!(matches!(unknown, DigestSlot::Unknown(_)));
    assert!(unknown.is_unknown());
    assert_eq!(unknown.state(), velnor_actions_tofu::SlotState::Unknown);
    std::fs::remove_dir(dir.path().join(".terraform.lock.hcl"))?;
    let absent = lock_digest_at_root(dir.path(), ".");
    assert!(!absent.is_unknown(), "absence is proven, never unknown");
    Ok(())
}

#[test]
fn fmt_kind_keeps_the_lockfile_exclusion() -> Outcome {
    let dir = TempDir::create("tofu-lock-fmt")?;
    dir.write(".terraform.lock.hcl", "lock-bytes\n")?;
    for unit in [".", "stacks/a"] {
        let slot = lock_slot_for_kind(dir.path(), unit, TofuTaskKind::Fmt);
        assert!(
            matches!(slot, DigestSlot::AbsentProven(ref evidence)
                if evidence == "excluded:kind_does_not_read_lockfile"),
            "{unit}: {slot:?}"
        );
    }
    Ok(())
}

#[test]
fn init_and_validate_bind_the_root_lock() -> Outcome {
    let dir = TempDir::create("tofu-lock-kinds")?;
    dir.write("stacks/a/.terraform.lock.hcl", "lock-bytes\n")?;
    for kind in [TofuTaskKind::InitForValidate, TofuTaskKind::Validate] {
        let slot = lock_slot_for_kind(dir.path(), "stacks/a", kind);
        assert!(matches!(slot, DigestSlot::Known(_)), "{kind:?}: {slot:?}");
        let missing = lock_slot_for_kind(dir.path(), "stacks/b", kind);
        assert!(
            matches!(missing, DigestSlot::AbsentProven(_)),
            "{kind:?}: {missing:?}"
        );
    }
    Ok(())
}

#[test]
fn inspect_rejects_files_it_does_not_own() {
    assert!(inspect_lockfile("main.tf", Some("")).is_err());
    assert!(inspect_lockfile("mise.toml", None).is_err());
    assert!(inspect_lockfile("stacks/a/Cargo.lock", Some("")).is_err());
    assert!(inspect_lockfile(".terraform.lock.hcl", None).is_ok());
    assert!(inspect_lockfile("stacks/a/.terraform.lock.hcl", Some("")).is_ok());
}

#[test]
fn inspect_missing_lock_carries_no_claim() {
    let inspection = inspect_lockfile(".terraform.lock.hcl", None).expect("missing");
    assert_eq!(inspection.file, ".terraform.lock.hcl");
    assert!(inspection.spec.is_none());
    assert!(
        inspection.findings.is_empty(),
        "absence is a slot, not a finding"
    );
}

#[test]
fn inspect_empty_lock_is_neutral() {
    for content in ["", "\n", "  \n\t\n"] {
        let inspection = inspect_lockfile(".terraform.lock.hcl", Some(content)).expect("empty");
        let spec = inspection.spec.expect("spec");
        assert!(spec.providers.is_empty());
        assert!(inspection.findings.is_empty(), "neutral lock stays silent");
    }
}

#[test]
fn inspect_stale_fixture_lists_the_provider() {
    let content = fixture_lock("stale").expect("stale lock");
    let inspection = inspect_lockfile("stale/.terraform.lock.hcl", Some(&content)).expect("stale");
    let spec = inspection.spec.expect("spec");
    assert_eq!(spec.providers, vec!["example.com/a/b".to_owned()]);
    assert!(inspection.findings.is_empty());
}

#[test]
fn inspect_weakened_fixture_is_a_corrupt_finding() {
    let content = fixture_lock("weakened").expect("weakened lock");
    let inspection =
        inspect_lockfile("weakened/.terraform.lock.hcl", Some(&content)).expect("weakened");
    assert!(inspection.spec.is_none());
    assert_eq!(inspection.findings.len(), 1);
    let finding = &inspection.findings[0];
    assert_eq!(finding.code, LOCKFILE_CORRUPT);
    assert_eq!(finding.code, "tofu_lockfile_corrupt");
    assert!(finding.validate().is_ok());
    assert!(
        finding
            .action
            .as_deref()
            .unwrap_or_default()
            .contains("manually"),
        "manual remediation: {finding:?}"
    );
    assert!(
        finding.reason.contains("validate"),
        "names the blocked gate: {finding:?}"
    );
}

#[test]
fn inspect_ignores_non_provider_blocks() {
    let content = "# comment only\nterraform {\n  required_version = \">= 1.6\"\n}\n";
    let inspection = inspect_lockfile(".terraform.lock.hcl", Some(content)).expect("blocks");
    let spec = inspection.spec.expect("spec");
    assert!(spec.providers.is_empty());
    assert!(inspection.findings.is_empty());
}

#[test]
fn inspect_sorts_and_dedupes_providers() {
    let content =
        "provider \"b.example/c\" {}\nprovider \"a.example/d\" {}\nprovider \"b.example/c\" {}\n";
    let inspection = inspect_lockfile(".terraform.lock.hcl", Some(content)).expect("dupes");
    let spec = inspection.spec.expect("spec");
    assert_eq!(
        spec.providers,
        vec!["a.example/d".to_owned(), "b.example/c".to_owned()]
    );
}

#[test]
fn snapshot_verifies_clean_and_detects_lock_drift() -> Outcome {
    let dir = TempDir::create("tofu-lock-snap")?;
    dir.write(".terraform.lock.hcl", "root-lock\n")?;
    dir.write("stacks/a/main.tf", "variable \"x\" {}\n")?;
    let roots = [String::new(), "stacks/a".to_owned()];
    let snap = TofuLockSnapshot::capture(dir.path(), &roots);
    assert!(snap.verify(dir.path()).is_ok(), "unchanged verifies");
    dir.write(".terraform.lock.hcl", "rotated\n")?;
    let err = snap.verify(dir.path()).expect_err("drift fails");
    assert!(
        err.contains("tofu_lock_changed:.terraform.lock.hcl"),
        "got {err}"
    );
    std::fs::remove_file(dir.path().join(".terraform.lock.hcl"))?;
    let err = snap.verify(dir.path()).expect_err("removal fails");
    assert!(
        err.contains("tofu_lock_changed:.terraform.lock.hcl"),
        "got {err}"
    );
    Ok(())
}

#[test]
fn snapshot_detects_terraform_dir_changes() -> Outcome {
    let dir = TempDir::create("tofu-lock-terraform")?;
    let roots = [String::new()];
    let snap = TofuLockSnapshot::capture(dir.path(), &roots);
    assert!(snap.verify(dir.path()).is_ok(), "no workdir verifies");
    std::fs::create_dir(dir.path().join(".terraform"))?;
    let err = snap.verify(dir.path()).expect_err("appearance fails");
    assert!(err.contains("tofu_terraform_dir_changed"), "got {err}");
    std::fs::remove_dir(dir.path().join(".terraform"))?;
    assert!(snap.verify(dir.path()).is_ok(), "restored verifies");
    Ok(())
}

#[test]
fn snapshot_unreadable_lock_fails_closed() -> Outcome {
    let dir = TempDir::create("tofu-lock-unreadable")?;
    dir.write(".terraform.lock.hcl", "root-lock\n")?;
    let roots = [String::new()];
    let snap = TofuLockSnapshot::capture(dir.path(), &roots);
    assert!(snap.verify(dir.path()).is_ok(), "readable verifies");
    std::fs::remove_file(dir.path().join(".terraform.lock.hcl"))?;
    std::fs::create_dir(dir.path().join(".terraform.lock.hcl"))?;
    let err = snap.verify(dir.path()).expect_err("unreadable fails");
    assert!(err.contains("tofu_lock_unreadable"), "got {err}");
    Ok(())
}

#[test]
fn lockfile_module_never_writes() {
    let src = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/lockfile.rs"),
    )
    .expect("read");
    for token in [
        "fs::write",
        "File::create",
        "OpenOptions",
        "Command::new",
        "std::process",
    ] {
        assert!(!src.contains(token), "write token {token}");
    }
}
