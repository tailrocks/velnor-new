use super::*;

#[test]
fn observed_unit_hash_never_comes_from_labels_or_output_paths() {
    let args = [
        "--crate-name",
        "registry_package",
        "--out-dir",
        "/build/package-abcdef12/out",
    ];
    assert!(extra_filename(&args.map(OsString::from)).is_none());
    assert_eq!(
        extra_filename(&["-C", "extra-filename=-abcdef12"].map(OsString::from)),
        Some("abcdef12".into())
    );
    assert_eq!(
        extra_filename(&["--codegen=extra-filename=-abc123"].map(OsString::from)),
        Some("abc123".into())
    );
    assert!(
        extra_filename(&["-Cextra-filename=-abc", "-Cextra-filename=-def"].map(OsString::from))
            .is_none()
    );
}

#[test]
fn missing_native_path_remains_unknown() {
    assert!(observed_path(Path::new("/does-not-exist/native-source"), Path::new("/")).is_none());
}

#[test]
fn conflicting_manifest_context_remains_unknown() -> eyre::Result<()> {
    let root = tempfile::tempdir()?;
    let manifest = root.path().join("Cargo.toml");
    let foreign = root.path().join("foreign.toml");
    std::fs::write(&manifest, "")?;
    std::fs::write(&foreign, "")?;
    assert!(
        manifest_evidence(
            Some(root.path().as_os_str().into()),
            Some(foreign.into_os_string()),
            root.path()
        )
        .is_none()
    );
    assert!(manifest_evidence(Some("relative".into()), None, root.path()).is_none());
    assert_eq!(
        manifest_evidence(
            Some(root.path().as_os_str().into()),
            Some(manifest.clone().into_os_string()),
            root.path()
        ),
        Some(manifest.canonicalize()?)
    );
    Ok(())
}
