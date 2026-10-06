use super::*;

#[test]
fn source_candidate_does_not_require_its_future_installed_receipt() -> Result<(), MiseError> {
    let candidate = RootRustCompilerCandidate::root_linux("0.1.0")?;
    candidate.verify_fresh()?;
    assert!(
        crate::catalog::rust_compiler_authority::RustCompilerArtifactAuthority::require_root_linux(
        )
        .is_err()
    );
    assert_eq!(candidate.environment().len(), 6);
    assert_eq!(candidate.environment()["RUSTUP_AUTO_INSTALL"], "0");
    assert_eq!(
        candidate.projection()["purpose"],
        "root-linux-compiler-candidate"
    );
    assert_eq!(candidate.projection()["host"], "x86_64-unknown-linux-gnu");
    assert_eq!(
        candidate
            .stages()
            .iter()
            .map(|stage| stage.name())
            .collect::<Vec<_>>(),
        ["clear", "acquire", "install"]
    );
    Ok(())
}

#[test]
fn fixed_install_admits_all_archives_before_native_manager() -> Result<(), MiseError> {
    let candidate = RootRustCompilerCandidate::root_linux("0.1.0")?;
    let source = candidate.stages()[2].source();
    let guard = source.find("candidate_verify_mirror()");
    let install = source.find("toolchain install");
    assert!(guard.is_some_and(|guard| install.is_some_and(|install| guard < install)));
    assert!(source.contains("export PATH=\"$root/manager-bin\""));
    assert!(source.contains("unset RUSTUP_PERMIT_COPY_RENAME"));
    assert!(
        source
            .contains("--profile minimal --component clippy --component rustfmt --no-self-update")
    );
    assert!(!source.contains("--force"));
    assert!(
        candidate.stages()[1]
            .source()
            .contains("curl --disable --fail")
    );
    assert!(
        !candidate.stages()[1]
            .source()
            .contains("--default-toolchain")
    );
    assert!(
        !candidate.stages()[1]
            .source()
            .contains("set auto-self-update")
    );
    assert!(source.contains("--default-toolchain none"));
    for stage in candidate.stages() {
        assert!(stage.arguments().is_empty());
        assert!(stage.source().contains("/usr/bin/python3 -I -S -B"));
        assert_eq!(
            stage.executable_search_paths(),
            ["/usr/bin", "/bin", "/usr/sbin", "/sbin"]
        );
    }
    Ok(())
}

#[test]
fn reconstruction_rejects_source_and_environment_mutation() -> Result<(), MiseError> {
    let mut candidate = RootRustCompilerCandidate::root_linux("0.1.0")?;
    candidate.stages[2].source.push_str("\ntrue\n");
    assert!(candidate.verify_fresh().is_err());
    let mut candidate = RootRustCompilerCandidate::root_linux("0.1.0")?;
    candidate
        .environment
        .insert("HOME".to_owned(), "/tmp/foreign".to_owned());
    assert!(candidate.verify_fresh().is_err());
    Ok(())
}
