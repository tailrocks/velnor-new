use super::*;

#[test]
fn source_template_is_exact_reviewed_publication_owner_bytes() {
    assert_eq!(source_sha256(TEMPLATE.as_bytes()), TEMPLATE_SHA256);
    assert_eq!(TEMPLATE.matches(ACTION_MARKER).count(), 1);
}

#[test]
fn native_qualification_retains_first_action_and_exact_fixed_policy() {
    let published = source::fixture();
    let files = files_for_policy(WorkflowPolicy::VelnorRepositoryV1, &published)
        .expect("source-only render");
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, WORKFLOW_PATH);
    let body = files[0].bytes.split_once('\n').expect("marker").1;
    assert_eq!(
        body,
        TEMPLATE.replace(ACTION_MARKER, published.action_reference())
    );
    let first_step = body.find("      - name:").expect("first step");
    let second_step = body[first_step + 1..]
        .find("      - name:")
        .map(|index| index + first_step + 1)
        .expect("upload step");
    let first = &body[first_step..second_step];
    assert!(first.contains(published.action_reference()));
    assert!(!first.contains("run:"));
    assert!(!first.contains("checkout@"));
    assert!(!first.contains("cache@"));
    assert!(body.contains("runs-on: ubuntu-26.04"));
    assert!(body.contains("permissions:\n  contents: read\n"));
    assert!(body.contains("on:\n  workflow_dispatch:\n"));
    assert!(body.contains("actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a"));
    assert!(!body.contains(ACTION_MARKER));
}

#[test]
fn consumer_policy_never_registers_foundation_qualification() {
    assert!(
        files_for_policy(WorkflowPolicy::ConsumerV1, &source::fixture())
            .expect("consumer policy")
            .is_empty()
    );
}

#[test]
fn mutated_template_rejects_before_render() {
    for template in [
        TEMPLATE.replace("ubuntu-26.04", "ubuntu-latest"),
        TEMPLATE.replace(ACTION_MARKER, "caller/repository/action@main"),
        format!("{TEMPLATE}\nuses: {ACTION_MARKER}\n"),
    ] {
        assert!(render(&source::fixture(), &template).is_err());
    }
}

fn repository(policy: &str, origin: &str) -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("repository fixture");
    for directory in [".velnor", ".git/objects", ".git/refs", ".github/workflows"] {
        std::fs::create_dir_all(root.path().join(directory)).expect("fixture directories");
    }
    std::fs::write(root.path().join(".git/HEAD"), "ref: refs/heads/main\n")
        .expect("Git fixture HEAD");
    std::fs::write(
        root.path().join(".git/config"),
        format!("[core]\nrepositoryformatversion = 0\n[remote \"origin\"]\nurl = {origin}\n"),
    )
    .expect("Git fixture origin");
    std::fs::write(
        root.path().join(".velnor/config.toml"),
        format!("schema = 1\n[workflow]\npolicy = \"{policy}\"\ndefault_branch = \"main\"\n"),
    )
    .expect("generator config");
    root
}

#[test]
fn pure_source_preview_does_not_discover_or_replace_repository_content() {
    let root = repository(
        "velnor-repository-v1",
        "https://github.com/tailrocks/velnor-new.git",
    );
    for (path, bytes) in [
        ("Cargo.toml", "invalid Cargo syntax"),
        (".github/CODEOWNERS", "* @fixture-owner\n"),
        (".github/workflows/existing.yml", "existing bytes\n"),
    ] {
        std::fs::write(root.path().join(path), bytes).expect("existing content");
    }
    let destination = tempfile::tempdir().expect("empty external preview");
    let paths = preview_foundation_qualification(root.path(), destination.path())
        .expect("source preview needs no runtime Foundation or valid Cargo manifest");
    assert_eq!(paths, vec![WORKFLOW_PATH.to_owned()]);
    assert_eq!(
        std::fs::read_to_string(destination.path().join(WORKFLOW_PATH)).expect("preview bytes"),
        render(&source::fixture(), TEMPLATE).expect("same factory")[0].bytes
    );
    assert_eq!(
        std::fs::read_to_string(root.path().join(".github/CODEOWNERS")).expect("preserved owner"),
        "* @fixture-owner\n"
    );
    assert_eq!(
        std::fs::read_to_string(root.path().join(".github/workflows/existing.yml"))
            .expect("preserved workflow"),
        "existing bytes\n"
    );
    assert!(!root.path().join(WORKFLOW_PATH).exists());
}

#[test]
fn source_preview_rejects_policy_identity_and_unsafe_destinations() {
    let destination = tempfile::tempdir().expect("external preview");
    for (policy, origin, expected) in [
        (
            "consumer-v1",
            "https://github.com/tailrocks/velnor-new.git",
            "foundation_qualification_requires_velnor_repository_policy",
        ),
        (
            "velnor-repository-v1",
            "https://github.com/fixture/other.git",
            "velnor_policy_requires_tailrocks_velnor_new",
        ),
    ] {
        let root = repository(policy, origin);
        let error = preview_foundation_qualification(root.path(), destination.path())
            .expect_err("policy or identity rejection");
        assert!(matches!(
            error,
            OrchestratorError::IdentityRejected { problem } if problem == expected
        ));
        assert!(!destination.path().join(".github").exists());
    }
    let root = repository(
        "velnor-repository-v1",
        "https://github.com/tailrocks/velnor-new.git",
    );
    let inside = root.path().join("preview");
    std::fs::create_dir(&inside).expect("empty inside preview");
    let error = preview_foundation_qualification(root.path(), &inside)
        .expect_err("repository containment rejection");
    assert!(matches!(
        error,
        OrchestratorError::PreviewRefused { reason, .. } if reason == "inside_repository"
    ));
    assert!(!inside.join(".github").exists());
    std::fs::write(destination.path().join("existing"), "keep").expect("occupied preview");
    let error = preview_foundation_qualification(root.path(), destination.path())
        .expect_err("nonempty preview rejection");
    assert!(matches!(
        error,
        OrchestratorError::PreviewRefused { reason, .. } if reason == "non_empty"
    ));
    assert_eq!(
        std::fs::read_to_string(destination.path().join("existing")).expect("preserved preview"),
        "keep"
    );
    assert!(!destination.path().join(".github").exists());
}
