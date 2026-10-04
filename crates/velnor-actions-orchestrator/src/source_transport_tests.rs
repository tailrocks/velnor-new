use super::*;

fn fixture(source: Option<&str>) -> tempfile::TempDir {
    let temp = tempfile::tempdir().expect("fixture");
    let source = source.map_or_else(String::new, |source| {
        format!("source = '{source}'\nchecksum = '{}'\n", "a".repeat(64))
    });
    std::fs::write(
        temp.path().join("Cargo.lock"),
        format!("version = 4\n[[package]]\nname='demo'\nversion='0.1.0'\n{source}"),
    )
    .expect("lock");
    temp
}

#[test]
fn public_registry_and_local_packages_are_qualified() {
    for source in [None, Some(PUBLIC_REGISTRY), Some(PUBLIC_SPARSE_REGISTRY)] {
        let temp = fixture(source);
        assert!(SourceTransportAdmission::new(temp.path(), &[String::new()]).is_some());
    }
}

#[test]
fn private_unknown_and_token_bearing_sources_are_not_qualified() {
    for source in [
        "git+https://github.com/company/private#0123456",
        "registry+https://private.example/index",
        "registry+https://token@github.com/rust-lang/crates.io-index",
    ] {
        let temp = fixture(Some(source));
        assert!(SourceTransportAdmission::new(temp.path(), &[String::new()]).is_none());
    }
}

#[test]
fn custom_source_auth_and_unknown_config_disable_transport() {
    for config in [
        "[source.crates-io]\nreplace-with='private'\n",
        "[registries.private]\nindex='https://private.example/index'\n",
        "[registry]\nglobal-credential-providers=['cargo:token']\n",
        "[env]\nCARGO_REGISTRIES_PRIVATE_TOKEN='private'\n",
        "include=['private-config.toml']\n",
        "[credential-alias]\nprivate=['custom-provider']\n",
        "[unknown]\nsetting=true\n",
        "this is not valid toml",
    ] {
        let temp = fixture(Some(PUBLIC_REGISTRY));
        std::fs::create_dir(temp.path().join(".cargo")).expect("config directory");
        std::fs::write(temp.path().join(".cargo/config.toml"), config).expect("config");
        assert!(SourceTransportAdmission::new(temp.path(), &[String::new()]).is_none());
    }
    let temp = fixture(Some(PUBLIC_REGISTRY));
    assert!(SourceTransportAdmission::new(temp.path(), &["unknown".to_owned(),]).is_none());
}

#[test]
fn every_selected_workspace_must_have_public_source_closure() {
    let temp = fixture(Some(PUBLIC_REGISTRY));
    let nested = temp.path().join("nested");
    std::fs::create_dir(&nested).expect("nested");
    std::fs::write(
        nested.join("Cargo.lock"),
        "version = 4\n[[package]]\nname='private'\nversion='0.1.0'\n\
         source='git+https://private.example/project#0123456'\n",
    )
    .expect("nested lock");
    assert!(
        SourceTransportAdmission::new(temp.path(), &[String::new(), "nested".to_owned()]).is_none()
    );
    assert!(SourceTransportAdmission::new(temp.path(), &[String::new()]).is_some());
}

#[test]
fn compiler_only_config_is_allowed_but_legacy_private_config_is_not() {
    let temp = fixture(Some(PUBLIC_REGISTRY));
    std::fs::create_dir(temp.path().join(".cargo")).expect("config directory");
    std::fs::write(
        temp.path().join(".cargo/config.toml"),
        "[build]\nrustc-wrapper='mbx'\n",
    )
    .expect("compiler config");
    assert!(SourceTransportAdmission::new(temp.path(), &[String::new()]).is_some());
    std::fs::write(
        temp.path().join(".cargo/config"),
        "[registries.private]\nindex='https://private.example/index'\n",
    )
    .expect("legacy config");
    assert!(SourceTransportAdmission::new(temp.path(), &[String::new()]).is_none());
}

#[test]
fn absent_empty_malformed_and_unreadable_locks_fail_closed() {
    for lock in [
        "",
        "version = 4\n",
        "version = 4\npackage = []\n",
        "version = 4\npackage = ['unknown']\n",
        "version = 4\n[[package]]\nsource = 123\n",
        "not valid toml",
    ] {
        let temp = tempfile::tempdir().expect("fixture");
        std::fs::write(temp.path().join("Cargo.lock"), lock).expect("lock");
        assert!(SourceTransportAdmission::new(temp.path(), &[String::new()]).is_none());
    }
    let temp = tempfile::tempdir().expect("fixture");
    assert!(SourceTransportAdmission::new(temp.path(), &[String::new()]).is_none());
    std::fs::create_dir(temp.path().join("Cargo.lock")).expect("unreadable lock");
    assert!(SourceTransportAdmission::new(temp.path(), &[String::new()]).is_none());
    assert!(SourceTransportAdmission::new(temp.path(), &[]).is_none());
}

#[test]
fn registry_spelling_is_an_exact_public_allowlist() {
    for source in [
        "registry+https://index.crates.io/",
        "sparse+https://index.crates.io",
        "sparse+https://token@index.crates.io/",
        "sparse+https://index.crates.io/private/",
        "registry+https://github.com/rust-lang/crates.io-index?token=secret",
        "git+https://github.com/rust-lang/crates.io-index#0123456",
    ] {
        let temp = fixture(Some(source));
        assert!(SourceTransportAdmission::new(temp.path(), &[String::new()]).is_none());
    }
}

#[test]
fn checkout_config_scope_matches_cargo_manifest_path_discovery() {
    let temp = fixture(Some(PUBLIC_REGISTRY));
    let nested = temp.path().join("nested");
    std::fs::create_dir_all(nested.join(".cargo")).expect("nested config directory");
    std::fs::copy(temp.path().join("Cargo.lock"), nested.join("Cargo.lock")).expect("nested lock");
    std::fs::write(
        nested.join(".cargo/config.toml"),
        "[source.crates-io]\nreplace-with='private'\n",
    )
    .expect("nested private config");
    assert!(SourceTransportAdmission::new(temp.path(), &["nested".to_owned()]).is_some());
    std::fs::create_dir(temp.path().join(".cargo")).expect("checkout config directory");
    std::fs::write(
        temp.path().join(".cargo/config.toml"),
        "[source.crates-io]\nreplace-with='private'\n",
    )
    .expect("checkout private config");
    assert!(SourceTransportAdmission::new(temp.path(), &["nested".to_owned()]).is_none());
}

#[test]
fn unreadable_checkout_config_is_not_treated_as_absent() {
    let temp = fixture(Some(PUBLIC_REGISTRY));
    std::fs::create_dir_all(temp.path().join(".cargo/config.toml")).expect("unreadable config");
    assert!(SourceTransportAdmission::new(temp.path(), &[String::new()]).is_none());
}

#[test]
fn admission_captures_sorted_unique_selected_evidence() {
    let temp = fixture(Some(PUBLIC_REGISTRY));
    std::fs::create_dir(temp.path().join("nested")).expect("nested");
    std::fs::copy(
        temp.path().join("Cargo.lock"),
        temp.path().join("nested/Cargo.lock"),
    )
    .expect("nested lock");
    let admission = SourceTransportAdmission::new(
        temp.path(),
        &["nested".to_owned(), String::new(), "nested".to_owned()],
    )
    .expect("admission");
    assert_eq!(admission.roots(), &[String::new(), "nested".to_owned()]);
    let before = admission.locks()[0].1.clone();
    std::fs::write(temp.path().join("Cargo.lock"), "invalid").expect("changed lock");
    assert_eq!(admission.locks()[0].1, before);
    assert!(admission.configs().is_empty());
}

#[test]
fn malformed_package_identity_checksum_and_lock_version_deny_admission() {
    for lock in [
        "version=3\n[[package]]\nname='demo'\nversion='1.0.0'",
        "version=5\n[[package]]\nname='demo'\nversion='1.0.0'",
        "[[package]]\nname='demo'\nversion='1.0.0'",
        "version=4\n[[package]]\nversion='1.0.0'",
        "version=4\n[[package]]\nname='demo'",
        "version=4\n[[package]]\nname='demo'\nversion='not-semver'",
        "version=4\n[[package]]\nname='../demo'\nversion='1.0.0'",
        "version=4\n[[package]]\nname='demo'\nversion='1.0.0'\nchecksum='unexpected'",
        "version=4\n[[package]]\nname='demo'\nversion='1.0.0'\n\
         [[package]]\nname='demo'\nversion='1.0.0'",
    ] {
        let temp = fixture(None);
        std::fs::write(temp.path().join("Cargo.lock"), lock).expect("lock");
        assert!(SourceTransportAdmission::new(temp.path(), &[String::new()]).is_none());
    }
    for checksum in [
        "a".repeat(63),
        "a".repeat(65),
        "A".repeat(64),
        "g".repeat(64),
    ] {
        let temp = fixture(Some(PUBLIC_REGISTRY));
        let lock = std::fs::read_to_string(temp.path().join("Cargo.lock")).expect("lock");
        let lock = lock.replace(&"a".repeat(64), &checksum);
        std::fs::write(temp.path().join("Cargo.lock"), lock).expect("lock");
        assert!(SourceTransportAdmission::new(temp.path(), &[String::new()]).is_none());
    }
}

#[cfg(unix)]
#[test]
fn escaped_and_symlinked_external_evidence_deny_admission() {
    use std::os::unix::fs::symlink;
    let temp = fixture(None);
    let outside = fixture(None);
    for root in ["../outside", "/outside", "nested/../"] {
        assert!(SourceTransportAdmission::new(temp.path(), &[root.to_owned()]).is_none());
    }
    symlink(outside.path(), temp.path().join("linked")).expect("root symlink");
    assert!(SourceTransportAdmission::new(temp.path(), &["linked".to_owned()]).is_none());
    std::fs::remove_file(temp.path().join("Cargo.lock")).expect("remove lock");
    symlink(
        outside.path().join("Cargo.lock"),
        temp.path().join("Cargo.lock"),
    )
    .expect("lock symlink");
    assert!(SourceTransportAdmission::new(temp.path(), &[String::new()]).is_none());
}

#[test]
fn captured_admission_matches_filesystem_policy() {
    for source in [None, Some(PUBLIC_REGISTRY), Some(PUBLIC_SPARSE_REGISTRY)] {
        let temp = fixture(source);
        let admitted = SourceTransportAdmission::new(temp.path(), &[String::new()])
            .expect("filesystem admission");
        let captured = SourceTransportAdmission::from_captured(
            admitted.roots(),
            admitted.locks(),
            admitted.configs(),
        )
        .expect("captured admission");
        assert_eq!(captured.roots(), admitted.roots());
        assert_eq!(captured.locks(), admitted.locks());
        assert_eq!(captured.configs(), admitted.configs());
    }
}

#[test]
fn captured_admission_requires_exact_sorted_unique_lock_scope() {
    let temp = fixture(None);
    let lock = std::fs::read_to_string(temp.path().join("Cargo.lock")).expect("lock");
    let root = String::new();
    let nested = "nested".to_owned();
    for (roots, locks) in [
        (vec![], vec![]),
        (vec![root.clone()], vec![]),
        (vec![root.clone()], vec![(nested.clone(), lock.clone())]),
        (
            vec![root.clone()],
            vec![(root.clone(), lock.clone()), (nested.clone(), lock.clone())],
        ),
        (
            vec![root.clone(), root.clone()],
            vec![(root.clone(), lock.clone()), (root.clone(), lock.clone())],
        ),
        (
            vec![nested.clone(), root.clone()],
            vec![(nested.clone(), lock.clone()), (root.clone(), lock.clone())],
        ),
        (
            vec![root.clone(), nested.clone()],
            vec![(nested.clone(), lock.clone()), (root.clone(), lock.clone())],
        ),
    ] {
        assert!(SourceTransportAdmission::from_captured(&roots, &locks, &[]).is_none());
    }
    for root in ["../outside", "/outside", "nested/../"] {
        assert!(
            SourceTransportAdmission::from_captured(
                &[root.to_owned()],
                &[(root.to_owned(), lock.clone())],
                &[],
            )
            .is_none()
        );
    }
}

#[test]
fn captured_admission_denies_private_sources_and_configuration() {
    for source in [
        "registry+https://private.example/index",
        "git+https://github.com/company/private#0123456",
        "sparse+https://token@index.crates.io/",
    ] {
        let temp = fixture(Some(source));
        let lock = std::fs::read_to_string(temp.path().join("Cargo.lock")).expect("lock");
        assert!(
            SourceTransportAdmission::from_captured(
                &[String::new()],
                &[(String::new(), lock)],
                &[],
            )
            .is_none()
        );
    }
    let temp = fixture(None);
    let locks = vec![(
        String::new(),
        std::fs::read_to_string(temp.path().join("Cargo.lock")).expect("lock"),
    )];
    for (name, config) in [
        (
            ".cargo/config.toml",
            "[registry]\nglobal-credential-providers=['cargo:token']",
        ),
        (
            ".cargo/config",
            "[source.crates-io]\nreplace-with='private'",
        ),
        ("nested/.cargo/config.toml", "[build]\njobs=2"),
        (".cargo/config.toml", "invalid toml"),
    ] {
        assert!(
            SourceTransportAdmission::from_captured(
                &[String::new()],
                &locks,
                &[(name.to_owned(), config.to_owned())],
            )
            .is_none()
        );
    }
    let config = (
        ".cargo/config.toml".to_owned(),
        "[build]\njobs=2".to_owned(),
    );
    assert!(
        SourceTransportAdmission::from_captured(
            &[String::new()],
            &locks,
            &[config.clone(), config],
        )
        .is_none()
    );
}
