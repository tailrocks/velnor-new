//! Exact Go compiler authority and identity mutation proofs.

use super::*;

#[test]
fn official_go_has_exact_native_host_rows() -> Result<(), crate::MiseError> {
    let cases = [
        (
            SourceBuildCompilerHost::LinuxAmd64,
            "x86_64-unknown-linux-gnu",
            "linux",
            "amd64",
            "https://go.dev/dl/go1.27.1.linux-amd64.tar.gz",
            "63d339f0da5ab53635a56f2490a7984dfe12dfcff22ad749f63edaf590168445",
            "30969f97169d7f43fe6a085873d75613adc21e30818a8c61d95bd27275df4624",
            "93d419aad923f0c45760b3cc25a64aff07c80306076501e84c9d40c4ab682fd4",
        ),
        (
            SourceBuildCompilerHost::MacosArm64,
            "aarch64-apple-darwin",
            "darwin",
            "arm64",
            "https://go.dev/dl/go1.27.1.darwin-arm64.tar.gz",
            "ee215d57e0ec269c60cc9ceca68e6bda321ba9ee5afe24f4b0988703c2d87d12",
            "132b69336a1f809932a8a20b0201dbbb980e86e3a323ae32e893639d83d71598",
            "94afd0a97d5fe086fedad187a308a92cfba24842292700f725d7af20d9478216",
        ),
        (
            SourceBuildCompilerHost::MacosAmd64,
            "x86_64-apple-darwin",
            "darwin",
            "amd64",
            "https://go.dev/dl/go1.27.1.darwin-amd64.tar.gz",
            "8f8f52c6649542cf027bbc9b9c68d1ec042f9f34808a40413f0b8b3f66f3caa4",
            "285418143831d996755c236ca0938ad317b22edeeb1d61bfa082f50550399fe3",
            "a591cfd5be3a4eb2af7d6408c93dd6c76a34c1b1678ac00f240a99a176d08e1b",
        ),
    ];
    for (
        host,
        target,
        goos,
        goarch,
        asset_url,
        archive_sha256,
        primary_binary_sha256,
        toolchain_tree_sha256,
    ) in cases
    {
        let record = official_go(host, GO_VERSION)?;
        assert_eq!(record.host(), host);
        assert_eq!(host.target_triple(), target);
        assert_eq!(record.target_triple(), target);
        assert_eq!(record.goos(), goos);
        assert_eq!(record.goarch(), goarch);
        assert_eq!(record.go_version(), GO_VERSION);
        assert_eq!(record.asset_url(), asset_url);
        assert_eq!(record.archive_sha256(), archive_sha256);
        assert_eq!(record.compiler_binary_sha256(), primary_binary_sha256);
        assert_eq!(record.toolchain_tree_sha256(), toolchain_tree_sha256);
    }
    Ok(())
}

#[test]
fn official_go_binds_source_archive_and_recipe_identity() -> Result<(), crate::MiseError> {
    let record = official_go(SourceBuildCompilerHost::LinuxAmd64, GO_VERSION)?;
    assert_eq!(record.source_repository(), SOURCE_REPOSITORY);
    assert_eq!(record.source_commit(), SOURCE_COMMIT);
    assert_eq!(record.source_tree(), SOURCE_TREE);
    assert_eq!(record.archive_root(), "go");
    assert_eq!(record.compiler_member(), "go/bin/go");
    assert_eq!(record.transform_abi(), TRANSFORM_ABI);
    assert_eq!(record.toolchain_file_count(), TOOLCHAIN_FILE_COUNT);
    assert!(record.native_only());
    assert_eq!(record.recipe_purpose(), RECIPE_PURPOSE);
    assert_eq!(record.qualification_digest().len(), 64);
    Ok(())
}

#[test]
fn official_go_rejects_non_exact_versions() {
    for version in ["1.27.1", "go1.27.0", "go1.27.2", "", "latest"] {
        assert!(matches!(
            official_go(SourceBuildCompilerHost::LinuxAmd64, version),
            Err(crate::MiseError::InvalidToolVersion { .. })
        ));
    }
}

#[test]
fn every_authority_field_changes_digest() -> Result<(), crate::MiseError> {
    let original = official_go(SourceBuildCompilerHost::LinuxAmd64, GO_VERSION)?;
    let mutations: [fn(&mut QualifiedBuildCompiler); 18] = [
        |record| record.host = SourceBuildCompilerHost::MacosArm64,
        |record| record.target = "changed-target",
        |record| record.goos = "changed-goos",
        |record| record.goarch = "changed-goarch",
        |record| record.go_version = "changed-version",
        |record| record.asset_url = "changed-url",
        |record| record.archive_sha256 = "changed-archive",
        |record| record.primary_binary_sha256 = "changed-binary",
        |record| record.toolchain_tree_sha256 = "changed-tree",
        |record| record.source_repository = "changed-repository",
        |record| record.source_commit = "changed-commit",
        |record| record.source_tree = "changed-source-tree",
        |record| record.archive_root = "changed-root",
        |record| record.compiler_member = "changed-member",
        |record| record.transform_abi = "changed-transform",
        |record| record.toolchain_file_count = TOOLCHAIN_FILE_COUNT - 1,
        |record| record.native_only = false,
        |record| record.recipe_purpose = "changed-purpose",
    ];
    for mutate in mutations {
        let mut changed = original;
        mutate(&mut changed);
        assert_ne!(
            original.qualification_digest(),
            changed.qualification_digest()
        );
    }
    Ok(())
}

#[test]
fn corrupted_records_fail_closed() -> Result<(), crate::MiseError> {
    let original = official_go(SourceBuildCompilerHost::LinuxAmd64, GO_VERSION)?;
    let mutations: [fn(&mut QualifiedBuildCompiler); 16] = [
        |record| record.target = "wrong-target",
        |record| record.goos = "wrong-os",
        |record| record.goarch = "wrong-arch",
        |record| record.go_version = "go1.27.2",
        |record| record.asset_url = "https://example.invalid/go.tar.gz",
        |record| record.archive_sha256 = "0",
        |record| record.primary_binary_sha256 = "not-a-digest",
        |record| record.toolchain_tree_sha256 = "0",
        |record| record.source_commit = "0",
        |record| record.source_tree = "0",
        |record| record.archive_root = "wrong-root",
        |record| record.compiler_member = "wrong-member",
        |record| record.transform_abi = "wrong-transform",
        |record| record.toolchain_file_count = 0,
        |record| record.native_only = false,
        |record| record.recipe_purpose = "runtime",
    ];
    for mutate in mutations {
        let mut changed = original;
        mutate(&mut changed);
        assert!(changed.validate().is_err());
    }
    Ok(())
}
