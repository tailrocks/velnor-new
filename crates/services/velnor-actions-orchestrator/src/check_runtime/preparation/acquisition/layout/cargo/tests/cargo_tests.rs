use super::*;

#[test]
fn verified_sources_move_into_an_owned_complete_vendor_tree() {
    let fixture = fixture();
    prepare(&fixture).expect("prepared offline source");
    assert!(!fixture.primary.exists());
    assert!(!fixture.dependency.exists());
    assert!(fixture.home.join("sources/Cargo.lock").is_file());
    let checksum = fs::read(
        fixture
            .home
            .join("vendor/dependency-1.2.3/.cargo-checksum.json"),
    )
    .expect("checksum receipt");
    let checksum: serde_json::Value = serde_json::from_slice(&checksum).expect("checksum JSON");
    assert_eq!(checksum["package"], "d".repeat(64));
    assert_eq!(
        checksum["files"]["src/lib.rs"],
        crate::cover_identity::generator::sha256_hex(b"pub fn dependency() {}\n")
    );
    let config = fs::read_to_string(fixture.home.join("cargo-source.toml")).expect("source config");
    assert!(config.contains("replace-with = \"velnor-vendor\""));
}

#[test]
fn lock_drift_and_missing_archive_closure_fail_before_source_moves() {
    let mut fixture = fixture();
    fs::write(fixture.primary.join("Cargo.lock"), "version = 4\n").expect("tamper lock");
    assert!(prepare(&fixture).is_err());
    assert!(fixture.primary.exists());
    let lock = fs::read(fixture.primary.join("Cargo.lock")).expect("tampered lock");
    if let QualifiedToolOptions::Cargo {
        installation: QualifiedCargoInstallation::Source { source_lock_sha256 },
        ..
    } = &mut fixture.tool.options
    {
        *source_lock_sha256 = crate::cover_identity::generator::sha256_hex(&lock);
    }
    assert!(prepare(&fixture).is_err());
    assert!(fixture.primary.exists());
}

#[test]
fn ancestor_and_source_cargo_configuration_are_rejected() {
    let fixture = fixture();
    fs::create_dir(fixture.home.join(".cargo")).expect("ancestor config dir");
    fs::write(
        fixture.home.join(".cargo/config.toml"),
        "[env]\nEVIL = \"true\"\n",
    )
    .expect("ancestor config");
    assert!(prepare(&fixture).is_err());
    fs::remove_file(fixture.home.join(".cargo/config.toml")).expect("remove ancestor config");
    fs::create_dir(fixture.primary.join(".cargo")).expect("source config dir");
    fs::write(
        fixture.primary.join(".cargo/config"),
        "[env]\nEVIL = \"true\"\n",
    )
    .expect("source config");
    assert!(prepare(&fixture).is_err());
    assert!(fixture.primary.exists());
}

#[test]
fn dependency_identity_and_forged_checksum_receipts_are_rejected() {
    let fixture = fixture();
    fs::write(
        fixture.dependency.join("Cargo.toml"),
        "[package]\nname = \"other\"\nversion = \"1.2.3\"\n",
    )
    .expect("wrong manifest");
    assert!(prepare(&fixture).is_err());
    fs::write(
        fixture.dependency.join("Cargo.toml"),
        "[package]\nname = \"dependency\"\nversion = \"1.2.3\"\n",
    )
    .expect("restore manifest");
    fs::write(fixture.dependency.join(".cargo-checksum.json"), "{}").expect("forged checksum");
    assert!(prepare(&fixture).is_err());
    assert!(fixture.primary.exists());
}
