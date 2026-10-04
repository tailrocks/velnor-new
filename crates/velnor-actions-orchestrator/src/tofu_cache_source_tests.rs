//! Provider source qualification and immutable export descriptor tests.

use super::*;

fn lock(source: &str) -> String {
    format!("provider \"{source}\" {{\n version = \"1.0.0\"\n hashes = [\"h1:checksum\"]\n}}")
}

fn tofu_task() -> ProposedTask {
    velnor_actions_tofu::propose_task(&velnor_actions_tofu::TofuTaskGroup {
        root: "stacks/a".to_owned(),
        kind: velnor_actions_tofu::TofuTaskKind::Validate,
        configuration: "default".to_owned(),
        no_targets: false,
    })
    .expect("tofu task")
}

#[test]
fn public_hosts_authorize_transport_for_any_published_namespace() {
    for source in [
        "registry.opentofu.org/1password/onepassword",
        "registry.opentofu.org/integrations/github",
        "registry.terraform.io/hashicorp/aws",
    ] {
        assert!(public_provider_sources(Some(&lock(source))), "{source}");
    }
}

#[test]
fn unknown_private_or_ambiguous_sources_never_authorize_transport() {
    for source in [
        "private.example.com/org/provider",
        "registry.opentofu.org.evil.example/org/provider",
        "registry.opentofu.org/org/provider/extra",
        "registry.opentofu.org//provider",
        "hashicorp/aws",
    ] {
        assert!(!public_provider_sources(Some(&lock(source))), "{source}");
    }
    let mixed = format!(
        "{}\n{}",
        lock("registry.opentofu.org/hashicorp/aws"),
        lock("private.example.com/org/provider")
    );
    assert!(!public_provider_sources(Some(&mixed)));
    for content in [
        None,
        Some(""),
        Some("invalid {"),
        Some("provider \"registry.opentofu.org/hashicorp/aws\" {}"),
    ] {
        assert!(!public_provider_sources(content));
    }
}

#[test]
fn source_proof_reads_the_exact_root_lock_and_refuses_escape() {
    let repository = tempfile::tempdir().expect("repository");
    let public = repository.path().join("public");
    let private = repository.path().join("private");
    std::fs::create_dir(&public).expect("public root");
    std::fs::create_dir(&private).expect("private root");
    std::fs::write(
        public.join(".terraform.lock.hcl"),
        lock("registry.opentofu.org/cloudflare/cloudflare"),
    )
    .expect("public lock");
    std::fs::write(
        private.join(".terraform.lock.hcl"),
        lock("private.example.com/org/provider"),
    )
    .expect("private lock");
    let mut reads = FileCache::new();
    assert!(public_provider_sources_at_root(
        repository.path(),
        "public",
        &mut reads
    ));
    assert!(!public_provider_sources_at_root(
        repository.path(),
        "private",
        &mut reads
    ));
    assert!(!public_provider_sources_at_root(
        repository.path(),
        "absent",
        &mut reads
    ));
    assert!(!public_provider_sources_at_root(
        repository.path(),
        "../public",
        &mut reads
    ));
}

#[test]
fn descriptor_captures_public_lock_and_exact_selection() {
    let repository = tempfile::tempdir().expect("repository");
    let root = repository.path().join("stacks/a");
    std::fs::create_dir_all(&root).expect("root");
    let content = lock("registry.opentofu.org/hashicorp/aws");
    std::fs::write(root.join(".terraform.lock.hcl"), &content).expect("lock");
    let descriptor =
        provider_export_descriptor_at_root(repository.path(), "stacks/a", &mut FileCache::new())
            .expect("descriptor");
    assert_eq!(descriptor.lock_content, content);
    assert_eq!(
        descriptor.selections,
        vec![(
            "registry.opentofu.org/hashicorp/aws".to_owned(),
            "1.0.0".to_owned()
        )]
    );
}

#[test]
fn descriptor_rejects_private_mixed_and_unpinned_locks() {
    let private = lock("private.example.com/org/provider");
    let mixed = format!(
        "{}\n{}",
        lock("registry.opentofu.org/hashicorp/aws"),
        private
    );
    for content in [
        private,
        mixed,
        lock("registry.opentofu.org/hashicorp/aws").replace("version = \"1.0.0\"\n", ""),
    ] {
        assert!(descriptor_from_lock("", &content).is_none(), "{content}");
    }
}

#[test]
fn descriptor_rejects_unsafe_or_inexact_versions() {
    for version in [
        "v1.2.3",
        "1.2",
        "1.2.3.4",
        "1.2.3-",
        "1.2.3+",
        "1.2.3-Alpha",
    ] {
        assert!(!safe_version(version), "{version}");
        assert!(descriptor_from_lock("", &lock_with_version(version)).is_none());
    }
    assert!(safe_version("1.2.3-alpha+meta"));
    assert!(!safe_version(&format!("1.2.3-{}", "a".repeat(128))));
}

#[test]
fn descriptor_for_tasks_rejects_metadata_mutation_and_disagreement() {
    let content = lock("registry.opentofu.org/hashicorp/aws");
    let descriptor = descriptor_from_lock("", &content).expect("descriptor");
    let encoded = serde_json::to_string(&descriptor).expect("descriptor json");
    let mut first = tofu_task();
    first.identity.environment.insert(
        crate::select_tofu::PUBLIC_PROVIDER_TRANSPORT.to_owned(),
        "true".to_owned(),
    );
    first
        .identity
        .environment
        .insert(PROVIDER_EXPORT_ENV.to_owned(), encoded);
    let mut second = first.clone();
    assert!(descriptor_for_tasks(&[&first, &second]).is_some());

    second
        .identity
        .environment
        .insert(PROVIDER_EXPORT_ENV.to_owned(), "{}".to_owned());
    assert!(descriptor_for_tasks(&[&first, &second]).is_none());

    let mut changed = descriptor;
    changed.lock_content.push('\n');
    first.identity.environment.insert(
        PROVIDER_EXPORT_ENV.to_owned(),
        serde_json::to_string(&changed).expect("changed descriptor json"),
    );
    assert!(descriptor_for_tasks(&[&first]).is_none());
}

fn lock_with_version(version: &str) -> String {
    format!(
        "provider \"registry.opentofu.org/hashicorp/aws\" {{\n version = \"{version}\"\n hashes = [\"h1:checksum\"]\n}}"
    )
}

#[cfg(unix)]
#[test]
fn public_source_bytes_behind_escaping_symlink_do_not_authorize_cache() {
    let repository = tempfile::tempdir().expect("repository");
    let outside = tempfile::tempdir().expect("outside");
    std::fs::write(
        outside.path().join(".terraform.lock.hcl"),
        lock("registry.opentofu.org/hashicorp/aws"),
    )
    .expect("outside lock");
    std::os::unix::fs::symlink(outside.path(), repository.path().join("escape"))
        .expect("escaping symlink");
    assert!(!public_provider_sources_at_root(
        repository.path(),
        "escape",
        &mut FileCache::new()
    ));
}
