use super::output;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    time::Duration,
};
use velnor_actions_contract::homebrew_adoption::HomebrewAdoptionFileIdentity;
use velnor_actions_mise::CheckDeadline;

use super::seed::{self, BaseTreeEntry};

#[path = "homebrew_adoption_prepare_tests.rs"]
mod prepare_tests;

#[test]
fn seed_agents_reader_uses_the_protected_source_file_ceiling() {
    assert_eq!(seed::MAX_SDK_MANIFEST_BYTES, 8 * 1024 * 1024);
    assert_eq!(
        seed::MAX_TAP_AGENTS_BYTES,
        velnor_actions_contract::package_release_policy::MAX_CARGO_VERSION_MANIFEST_BYTES
    );
    let transition = seed::load_transition("jackin-homebrew-seed-921858f1").expect("transition");
    let deadline = CheckDeadline::after(Duration::ZERO).expect("expired deadline");
    let error = crate::package_release_publish::read_protected_source_file(
        &transition.tap_repository,
        transition.tap_repository_id,
        "main",
        &transition.base_sha,
        &transition.claude_source_path,
        seed::MAX_TAP_AGENTS_BYTES,
        deadline,
    )
    .expect_err("the valid AGENTS limit reaches the deadline before any network request");
    assert_eq!(
        error.to_string(),
        "internal: package_public_verification_deadline"
    );

    let error = crate::package_release_publish::read_protected_source_file(
        &transition.tap_repository,
        transition.tap_repository_id,
        "main",
        &transition.base_sha,
        &transition.claude_source_path,
        seed::MAX_TAP_AGENTS_BYTES + 1,
        deadline,
    )
    .expect_err("the production reader must reject limits above its ceiling");
    assert_eq!(
        error.to_string(),
        "internal: package_public_source_file_selection_invalid"
    );
}

#[test]
fn output_is_data_only_exclusive_and_has_exact_deletion_manifest() {
    let root = tempfile::tempdir().expect("repo root");
    let parent = tempfile::tempdir().expect("output parent");
    let target = parent.path().join("adoption");
    let (canonical_parent, canonical_target) =
        output::fresh_output_path(&target, root.path()).expect("fresh path");
    let deleted = vec![HomebrewAdoptionFileIdentity {
        path: "scripts/old.py".to_owned(),
        mode: 0o100_644,
        blob_sha: "a".repeat(40),
    }];
    let report = output::write_overlay(
        &canonical_parent,
        &canonical_target,
        "initial-seed",
        &"b".repeat(40),
        &"c".repeat(40),
        &BTreeMap::from([(
            ".github/workflows/ci.yml".to_owned(),
            (0o100_644, b"workflow\n".to_vec()),
        )]),
        deleted.clone(),
    )
    .expect("overlay");
    assert_eq!(report.deleted_paths, vec!["scripts/old.py".to_owned()]);
    assert_eq!(
        fs::read(target.join(".github/workflows/ci.yml")).expect("workflow"),
        b"workflow\n"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&target)
                .expect("target mode")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(target.join(".github/workflows/ci.yml"))
                .expect("workflow mode")
                .permissions()
                .mode()
                & 0o777,
            0o644
        );
    }
    let manifest = fs::read(target.join(output::DELETION_MANIFEST_PATH)).expect("manifest");
    assert!(
        String::from_utf8(manifest)
            .expect("utf8")
            .contains("scripts/old.py")
    );
    assert!(
        output::write_overlay(
            &canonical_parent,
            &canonical_target,
            "initial-seed",
            &"b".repeat(40),
            &"c".repeat(40),
            &BTreeMap::from([(
                ".github/workflows/ci.yml".to_owned(),
                (0o100_644, b"replacement\n".to_vec()),
            )]),
            deleted,
        )
        .is_err()
    );
    assert_eq!(
        fs::read(target.join(".github/workflows/ci.yml")).expect("unchanged workflow"),
        b"workflow\n"
    );
    assert!(output::fresh_output_path(&target, root.path()).is_err());
}

#[test]
fn output_path_must_be_absolute_and_outside_repository() {
    let root = tempfile::tempdir().expect("repo root");
    assert!(output::fresh_output_path(std::path::Path::new("relative"), root.path()).is_err());
    assert!(output::fresh_output_path(&root.path().join("nested"), root.path()).is_err());
}

#[cfg(unix)]
#[test]
fn output_rejects_group_or_world_writable_parent() {
    use std::os::unix::fs::PermissionsExt;
    let parent = tempfile::tempdir().expect("output parent");
    fs::set_permissions(parent.path(), fs::Permissions::from_mode(0o775))
        .expect("writable parent mode");
    let target = parent.path().join("adoption");
    let result = output::write_overlay(
        parent.path(),
        &target,
        "initial-seed",
        &"b".repeat(40),
        &"c".repeat(40),
        &BTreeMap::new(),
        Vec::new(),
    );
    fs::set_permissions(parent.path(), fs::Permissions::from_mode(0o700))
        .expect("restore parent mode");
    assert!(result.is_err());
    assert!(!target.exists());
}

#[test]
fn exact_protected_seed_base_admits_only_its_claude_symlink() {
    let transition = seed::load_transition("jackin-homebrew-seed-921858f1").expect("transition");
    let link_sha = crate::release_policy_admission::git_blob_sha1(b"AGENTS.md").expect("sha");
    assert_eq!(link_sha, "47dc3e3d863cfb5727b87d785d09abf9743c0a72");
    let (regular, symlink, directories) = protected_base_fixture(&transition, &link_sha);
    assert!(
        seed::validated_base_entries(
            &transition,
            &transition.base_tree,
            &regular,
            &symlink,
            &directories,
            &link_sha,
        )
        .is_ok()
    );

    for (path, mode, sha) in [
        ("other.md", "120000", link_sha.as_str()),
        ("CLAUDE.md", "100644", link_sha.as_str()),
        (
            "CLAUDE.md",
            "120000",
            "0000000000000000000000000000000000000000",
        ),
    ] {
        let mutated = vec![BaseTreeEntry {
            path,
            mode,
            sha,
            kind: "blob",
        }];
        assert!(
            seed::validated_base_entries(
                &transition,
                &transition.base_tree,
                &regular,
                &mutated,
                &directories,
                &link_sha,
            )
            .is_err()
        );
    }
    let mut extra = symlink.clone();
    extra.push(BaseTreeEntry {
        path: "other.md",
        mode: "120000",
        sha: &link_sha,
        kind: "blob",
    });
    assert!(
        seed::validated_base_entries(
            &transition,
            &transition.base_tree,
            &regular,
            &extra,
            &directories,
            &link_sha,
        )
        .is_err()
    );
}

fn protected_base_fixture<'a>(
    transition: &'a velnor_actions_contract::homebrew_adoption::HomebrewAdoptionTransition,
    link_sha: &'a str,
) -> (
    Vec<BaseTreeEntry<'a>>,
    Vec<BaseTreeEntry<'a>>,
    Vec<BaseTreeEntry<'a>>,
) {
    let mut regular = transition
        .deletions
        .iter()
        .chain(&transition.unchanged_inert_files)
        .map(|item| BaseTreeEntry {
            path: &item.path,
            mode: mode_string(item.mode),
            sha: &item.blob_sha,
            kind: "blob",
        })
        .collect::<Vec<_>>();
    regular.push(BaseTreeEntry {
        path: &transition.claude_source_path,
        mode: "100644",
        sha: &transition.claude_source_blob_sha,
        kind: "blob",
    });
    let symlink = vec![BaseTreeEntry {
        path: "CLAUDE.md",
        mode: "120000",
        sha: link_sha,
        kind: "blob",
    }];
    let dirs = regular
        .iter()
        .chain(&symlink)
        .flat_map(|entry| parent_paths(entry.path))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|path| BaseTreeEntry {
            path,
            mode: "040000",
            sha: "1111111111111111111111111111111111111111",
            kind: "tree",
        })
        .collect();
    (regular, symlink, dirs)
}

fn mode_string(mode: u32) -> &'static str {
    match mode {
        0o100_644 => "100644",
        0o100_755 => "100755",
        _ => "000000",
    }
}

fn parent_paths(path: &str) -> impl Iterator<Item = &str> {
    path.match_indices('/').map(|(index, _)| &path[..index])
}
