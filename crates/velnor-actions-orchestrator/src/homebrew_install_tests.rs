use super::super::inspect::validate_receipt;
use super::super::inventory::{validate_brew_sbom, verify_keg_file_inventory};
use super::super::request::write_formula_file;
use super::super::support::{
    elf_facts, epoch_seconds, evidence_digest, is_sha256, is_supported_host_macho, safe_component,
    safe_relative_path, sha256,
};
use super::super::{
    ElfFacts, HOMEBREW_CELLAR, HOMEBREW_PREFIX, HOMEBREW_VERSION, HomebrewInstallEvidence,
    HomebrewInstalledFileEvidence,
};
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};
use velnor_actions_contract::homebrew_spec::HomebrewAuxiliaryTest;
use velnor_actions_mise::CheckDeadline;

#[test]
fn receipt_binds_exact_formula_source_version_and_stable_spec() {
    let path = Path::new("/private/runner/staging/example-preview.rb");
    let install_started = epoch_seconds().expect("current time");
    let receipt = serde_json::json!({
        "time": install_started,
        "homebrew_version": HOMEBREW_VERSION.trim_start_matches("Homebrew "),
        "installed_on_request": true,
        "poured_from_bottle": false,
        "changed_files": ["bin/example", "INSTALL_RECEIPT.json"],
        "source": {
            "path": path,
            "tap": null,
            "spec": "stable",
            "versions": { "stable": "1.2.3-preview.0123456789ab" }
        }
    });
    let bytes = serde_json::to_vec(&receipt).expect("receipt JSON");
    assert!(validate_receipt(&bytes, path, "1.2.3-preview.0123456789ab", install_started).is_ok());
    assert!(validate_receipt(&bytes, path, "1.2.4-preview.0123456789ab", install_started).is_err());
    assert!(
        validate_receipt(
            &bytes,
            Path::new("/private/runner/staging/other.rb"),
            "1.2.3-preview.0123456789ab",
            install_started,
        )
        .is_err()
    );
}

#[test]
fn receipt_accepts_null_relocation_inventory_but_rejects_invalid_inventory() {
    let path = Path::new("/private/runner/staging/example-preview.rb");
    let install_started = epoch_seconds().expect("current time");
    let mut receipt = serde_json::json!({
        "time": install_started,
        "homebrew_version": HOMEBREW_VERSION.trim_start_matches("Homebrew "),
        "installed_on_request": true,
        "poured_from_bottle": false,
        "changed_files": null,
        "source": {
            "path": path,
            "tap": null,
            "spec": "stable",
            "versions": { "stable": "1.2.3-preview.0123456789ab" }
        }
    });
    let bytes = serde_json::to_vec(&receipt).expect("null inventory receipt");
    assert!(validate_receipt(&bytes, path, "1.2.3-preview.0123456789ab", install_started).is_ok());

    receipt["changed_files"] = serde_json::json!(["bin/other"]);
    let bytes = serde_json::to_vec(&receipt).expect("mismatched inventory receipt");
    assert!(validate_receipt(&bytes, path, "1.2.3-preview.0123456789ab", install_started).is_ok());

    receipt["changed_files"] = serde_json::json!(["../escape"]);
    let bytes = serde_json::to_vec(&receipt).expect("unsafe inventory receipt");
    assert!(validate_receipt(&bytes, path, "1.2.3-preview.0123456789ab", install_started).is_err());
}

#[test]
fn keg_inventory_rejects_extra_files_and_links() {
    let root = physical_tempdir();
    let keg = root.path();
    std::fs::create_dir(keg.join(".brew")).expect("brew metadata directory");
    std::fs::create_dir(keg.join("bin")).expect("bin directory");
    std::fs::write(keg.join("INSTALL_RECEIPT.json"), b"{}").expect("receipt");
    std::fs::write(keg.join("sbom.spdx.json"), b"{}").expect("Brew SBOM");
    std::fs::write(keg.join(".brew/example-preview.rb"), b"formula").expect("recipe");
    std::fs::write(keg.join("bin/example"), b"binary").expect("payload");
    let payload = [PathBuf::from("bin/example")].into_iter().collect();
    assert!(verify_keg_file_inventory(keg, "example-preview", &payload).is_ok());

    std::fs::write(keg.join("unexpected"), b"extra").expect("unexpected file");
    assert!(verify_keg_file_inventory(keg, "example-preview", &payload).is_err());
    std::fs::remove_file(keg.join("unexpected")).expect("remove extra");
    std::os::unix::fs::symlink("bin/example", keg.join("linked")).expect("symlink");
    assert!(verify_keg_file_inventory(keg, "example-preview", &payload).is_err());
}

#[test]
fn brew_sbom_is_bounded_and_bound_to_formula_version() {
    let sbom = serde_json::json!({
        "SPDXID": "SPDXRef-DOCUMENT", "spdxVersion": "SPDX-2.3",
        "name": "SBOM-SPDX-jackin-preview-0.6.4-preview.1362+41bad14",
        "creationInfo": {"created":"1970-01-01T00:00:00Z", "creators":["Tool: https://github.com/Homebrew/brew"]},
        "dataLicense":"CC0-1.0",
        "documentNamespace":"https://formulae.brew.sh/spdx/jackin-preview-0.6.4-preview.1362+41bad14.json",
        "documentDescribes":[],
        "files":[{"SPDXID":"SPDXRef-File-jackin-preview","fileName":"jackin-aarch64-apple-darwin.tar.gz","checksums":[{"algorithm":"SHA256","checksumValue":"a".repeat(64)}]}],
        "packages":[{"name":"jackin-preview","versionInfo":"0.6.4-preview.1362+41bad14","downloadLocation":"https://github.com/jackin-project/jackin/releases/download/preview-41bad147b5ba9db09be99b07248531ce1bd867be/jackin-aarch64-apple-darwin.tar.gz","checksums":[{"algorithm":"SHA256","checksumValue":"a".repeat(64)}]},{"name":"clang"}],
        "relationships":[]
    });
    let bytes = serde_json::to_vec(&sbom).expect("SBOM JSON");
    let url = "https://github.com/jackin-project/jackin/releases/download/preview-41bad147b5ba9db09be99b07248531ce1bd867be/jackin-aarch64-apple-darwin.tar.gz";
    assert!(
        validate_brew_sbom(
            &bytes,
            "jackin-preview",
            "0.6.4-preview.1362+41bad14",
            "jackin-aarch64-apple-darwin.tar.gz",
            &"a".repeat(64),
            url
        )
        .is_ok()
    );
    assert!(
        validate_brew_sbom(
            &bytes,
            "jackin-preview",
            "0.6.4-preview.1370+5094139",
            "jackin-aarch64-apple-darwin.tar.gz",
            &"a".repeat(64),
            url
        )
        .is_err()
    );
    let mut extra = sbom;
    extra["unexpected"] = serde_json::Value::Bool(true);
    assert!(
        validate_brew_sbom(
            &serde_json::to_vec(&extra).expect("SBOM JSON"),
            "jackin-preview",
            "0.6.4-preview.1362+41bad14",
            "jackin-aarch64-apple-darwin.tar.gz",
            &"a".repeat(64),
            url,
        )
        .is_err()
    );
}

#[test]
fn elf_header_is_decoded_offline_and_rejects_truncation() {
    let mut header = vec![0_u8; 64];
    header[..4].copy_from_slice(b"\x7fELF");
    header[4] = 2;
    header[5] = 1;
    header[18..20].copy_from_slice(&183_u16.to_le_bytes());
    assert_eq!(
        elf_facts(&header).expect("valid ELF header"),
        ElfFacts {
            class: 2,
            endian: 1,
            machine: 183,
        }
    );
    assert!(elf_facts(b"\x7fELF").is_err());
    header[5] = 9;
    assert!(elf_facts(&header).is_err());
}

#[test]
fn host_macho_check_requires_a_64_bit_arm_header() {
    let mut header = vec![0_u8; 8];
    header[..4].copy_from_slice(&[0xcf, 0xfa, 0xed, 0xfe]);
    header[4..8].copy_from_slice(&0x0100_000c_u32.to_le_bytes());
    assert!(is_supported_host_macho(&header));
    header[4..8].copy_from_slice(&0x0100_0007_u32.to_le_bytes());
    assert!(!is_supported_host_macho(&header));
    header[..4].copy_from_slice(&[0xfe, 0xed, 0xfa, 0xcf]);
    assert!(!is_supported_host_macho(&header));
}

#[test]
fn path_and_digest_grammars_reject_escape_and_noncanonical_hex() {
    assert!(safe_component("example-preview"));
    assert!(!safe_component("../example"));
    assert!(safe_relative_path(Path::new("libexec/example/bin")));
    assert!(!safe_relative_path(Path::new("../outside")));
    assert!(is_sha256(&"a".repeat(64)));
    assert!(!is_sha256(&"A".repeat(64)));
}

#[test]
fn evidence_digest_binds_receipt_time_and_each_installed_member() {
    let mut evidence = HomebrewInstallEvidence {
        formula: "example-preview".to_owned(),
        version: "1.2.3-preview.0123456789ab".to_owned(),
        brew_version: HOMEBREW_VERSION.to_owned(),
        prefix: HOMEBREW_PREFIX.to_owned(),
        cellar: HOMEBREW_CELLAR.to_owned(),
        keg: "/opt/homebrew/Cellar/example-preview/1.2.3-preview.0123456789ab".to_owned(),
        install_time: 1,
        receipt_sha256: "a".repeat(64),
        recipe_sha256: "b".repeat(64),
        payload_sha256: "c".repeat(64),
        version_output_sha256: "d".repeat(64),
        installed_files: vec![HomebrewInstalledFileEvidence {
            asset: "example-aarch64-apple-darwin.tar.gz".to_owned(),
            member: "example".to_owned(),
            destination: PathBuf::from("bin/example"),
            sha256: "e".repeat(64),
            mode: 0o755,
            test: HomebrewAuxiliaryTest::None,
            elf: None,
        }],
        verification_sha256: String::new(),
    };
    let first = evidence_digest(&evidence);
    evidence.installed_files[0].member = "changed-member".to_owned();
    assert_ne!(first, evidence_digest(&evidence));
    evidence.installed_files[0].member = "example".to_owned();
    evidence.install_time += 1;
    assert_ne!(first, evidence_digest(&evidence));
}

#[test]
fn install_evidence_deserialization_rejects_unknown_fields() {
    let mut value = serde_json::json!({
        "formula": "example-preview",
        "version": "1.2.3-preview.0123456789ab",
        "brew_version": HOMEBREW_VERSION,
        "prefix": HOMEBREW_PREFIX,
        "cellar": HOMEBREW_CELLAR,
        "keg": "/opt/homebrew/Cellar/example-preview/1.2.3-preview.0123456789ab",
        "install_time": 1,
        "receipt_sha256": "a".repeat(64),
        "recipe_sha256": "b".repeat(64),
        "payload_sha256": "c".repeat(64),
        "installed_files": [{
            "asset": "example-aarch64-apple-darwin.tar.gz",
            "member": "example",
            "destination": "bin/example",
            "sha256": "d".repeat(64),
            "mode": 493,
            "test": "none",
            "elf": null
        }],
        "version_output_sha256": "e".repeat(64),
        "verification_sha256": "f".repeat(64)
    });
    assert!(serde_json::from_value::<HomebrewInstallEvidence>(value.clone()).is_ok());
    value["unexpected"] = serde_json::Value::Bool(true);
    assert!(serde_json::from_value::<HomebrewInstallEvidence>(value).is_err());

    let file = serde_json::json!({
        "asset": "example-aarch64-apple-darwin.tar.gz",
        "member": "example",
        "destination": "bin/example",
        "sha256": "d".repeat(64),
        "mode": 493,
        "test": "none",
        "elf": null,
        "unexpected": true
    });
    assert!(serde_json::from_value::<HomebrewInstalledFileEvidence>(file).is_err());
}

#[test]
fn candidate_formula_write_is_exclusive_and_symlink_safe() {
    let temp = physical_tempdir();
    let formula = b"class ExamplePreview < Formula\nend\n";
    let digest = sha256(formula);
    let deadline =
        CheckDeadline::from_start(std::time::Instant::now(), std::time::Duration::from_secs(5))
            .expect("bounded deadline");
    let destination = temp.path().join("candidate.rb");

    write_formula_file(&destination, formula, &digest, deadline).expect("exclusive write");
    assert_eq!(
        std::fs::read(&destination).expect("candidate bytes"),
        formula
    );
    assert_eq!(
        std::fs::symlink_metadata(&destination)
            .expect("candidate metadata")
            .permissions()
            .mode()
            & 0o777,
        0o644
    );
    assert!(
        write_formula_file(
            &destination,
            b"replacement",
            &sha256(b"replacement"),
            deadline
        )
        .is_err()
    );
    assert_eq!(
        std::fs::read(&destination).expect("preserved bytes"),
        formula
    );

    let target = temp.path().join("outside.rb");
    std::fs::write(&target, b"untouched").expect("target bytes");
    let planted = temp.path().join("planted.rb");
    std::os::unix::fs::symlink(&target, &planted).expect("plant symlink");
    assert!(write_formula_file(&planted, formula, &digest, deadline).is_err());
    assert_eq!(
        std::fs::read(&target).expect("target remains"),
        b"untouched"
    );
}

fn physical_tempdir() -> tempfile::TempDir {
    let root = std::env::temp_dir()
        .canonicalize()
        .expect("physical system temp directory");
    tempfile::tempdir_in(root).expect("private temporary fixture")
}
