//! Leaf admission and failure cleanup, using only synthetic executable headers.

use super::*;
use std::{fs, os::unix::fs::symlink, time::Duration};

#[path = "preview_payload_test_support.rs"]
mod support;
use support::{
    ARCHIVE, MANIFEST, SBOM, hash_file, mutate_manifest, verify, write_archive, write_manifest,
};

#[test]
fn admits_one_pair_with_full_request_and_exact_original_archive_sbom_bytes() {
    let fixture = support::fixture();
    let verified = verify(&fixture).expect("valid leaf");
    assert_eq!(verified.request, fixture.request);
    assert_eq!(
        verified.request.plan.products[0].targets.len(),
        2,
        "full plan retained"
    );
    assert_eq!(verified.archive.name, ARCHIVE);
    assert_eq!(verified.sbom.name, SBOM);
    assert_eq!(
        verified.archive.sha256,
        hash_file(&fixture.producer.join(ARCHIVE))
    );
    assert_eq!(
        verified.sbom.sha256,
        hash_file(&fixture.producer.join(SBOM))
    );
    assert_eq!(
        fs::read(&verified.archive.path).expect("staged"),
        fs::read(fixture.producer.join(ARCHIVE)).expect("original")
    );
    assert_eq!(
        fs::read(&verified.sbom.path).expect("staged"),
        fs::read(fixture.producer.join(SBOM)).expect("original")
    );
    assert_eq!(fs::read_dir(&fixture.staging).expect("staging").count(), 2);
    assert!(
        !fixture.staging.join("cli").exists(),
        "members never extracted"
    );
    assert!(
        !fixture.staging.join(MANIFEST).exists(),
        "handoff is not an asset"
    );
}

#[test]
fn admits_stable_leaf_without_changing_shared_archive_inventory() {
    let mut fixture = support::fixture();
    fixture.request.plan.channel =
        velnor_actions_contract::package_release::PackageReleaseChannel::Stable;
    fixture.request.plan_sha256 =
        velnor_actions_contract::preview_production::release_plan_sha256(&fixture.request.plan)
            .expect("stable plan digest");
    fixture.request.context.commit_count = 0;
    fixture.request.context.version = fixture.request.plan.base_version.clone();
    write_manifest(&fixture);
    let verified = verify(&fixture).expect("shared stable tar.gz leaf");
    assert_eq!(verified.request, fixture.request);
    assert_eq!(verified.archive.name, ARCHIVE);
    assert_eq!(verified.sbom.name, SBOM);
    assert_eq!(
        verified.archive.sha256,
        hash_file(&fixture.producer.join(ARCHIVE))
    );
}

#[test]
fn rejects_assembly_stage_unknown_pair_and_changed_full_plan_digest() {
    for mutate in [
        |request: &mut ReleaseProductionRequest| {
            request.stage = ReleaseProductionStage::Assemble { inputs: Vec::new() }
        },
        |request: &mut ReleaseProductionRequest| {
            request.stage = ReleaseProductionStage::Payload {
                pair: velnor_actions_contract::preview_production::ReleasePayloadPair {
                    product_id: "other".to_owned(),
                    target: "x86_64-unknown-linux-gnu".to_owned(),
                },
            };
        },
        |request: &mut ReleaseProductionRequest| {
            request.plan.products[0].targets.remove(0);
        },
    ] {
        let mut fixture = support::fixture();
        mutate(&mut fixture.request);
        assert!(verify(&fixture).is_err());
        assert!(!fixture.staging.exists());
    }
}

#[test]
fn rejects_missing_extra_serial_inventory_and_any_companion() {
    for mutate in [
        |value: &mut serde_json::Value| {
            value["payloads"] = serde_json::json!([]);
        },
        |value: &mut serde_json::Value| {
            value["sboms"] = serde_json::json!([]);
        },
        |value: &mut serde_json::Value| {
            let extra = value["payloads"][0].clone();
            value["payloads"].as_array_mut().expect("rows").push(extra);
        },
        |value: &mut serde_json::Value| {
            value["companions"] = serde_json::json!([{"id":"license", "path":"LICENSE", "name":"LICENSE", "sha256":"a".repeat(64)}]);
        },
    ] {
        let fixture = support::fixture();
        mutate_manifest(&fixture, mutate);
        assert!(verify(&fixture).is_err());
        assert!(!fixture.staging.exists());
    }
    for name in [ARCHIVE, SBOM] {
        let fixture = support::fixture();
        fs::remove_file(fixture.producer.join(name)).expect("remove required file");
        assert!(verify(&fixture).is_err());
    }
    let fixture = support::fixture();
    fs::write(fixture.producer.join("extra"), b"forbidden").expect("extra");
    assert!(verify(&fixture).is_err());
}

#[test]
fn rejects_wrong_source_ref_version_target_format_and_sbom_binding() {
    for (field, value) in [
        ("schema", "velnor.package-output.v1".to_owned()),
        ("source_commit", "b".repeat(40)),
        ("source_ref", "refs/heads/other".to_owned()),
        ("version", "1.2.3-preview.8+aaaaaaa".to_owned()),
    ] {
        let fixture = support::fixture();
        mutate_manifest(&fixture, |manifest| {
            manifest[field] = serde_json::json!(value);
        });
        assert!(verify(&fixture).is_err());
    }
    for (field, value) in [
        ("target", "aarch64-apple-darwin"),
        ("format", "raw-binary"),
        ("product_id", "other"),
    ] {
        let fixture = support::fixture();
        mutate_manifest(&fixture, |manifest| {
            manifest["payloads"][0][field] = serde_json::json!(value);
        });
        assert!(verify(&fixture).is_err());
    }
    let fixture = support::fixture();
    let sbom = fixture.producer.join(SBOM);
    let mut document: serde_json::Value =
        serde_json::from_slice(&fs::read(&sbom).expect("SBOM")).expect("JSON");
    document["metadata"]["component"]["hashes"][0]["content"] = serde_json::json!("0".repeat(64));
    fs::write(sbom, serde_json::to_vec(&document).expect("JSON")).expect("wrong binding");
    write_manifest(&fixture);
    assert!(verify(&fixture).is_err());
    assert!(
        !fixture.staging.exists(),
        "failure after staging cleans owned bytes"
    );
}

#[test]
fn rejects_wrong_archive_bins_or_architecture_even_with_matching_claimed_hashes() {
    for (member, machine) in [("unexpected", 62), ("cli", 183)] {
        let fixture = support::fixture();
        write_archive(&fixture.producer.join(ARCHIVE), member, machine);
        write_manifest(&fixture);
        let error = verify(&fixture).expect_err("wrong executable rejected");
        let expected = if member == "cli" {
            "preview_tar_executable_format_mismatch"
        } else {
            "preview_tar_member_inventory_invalid"
        };
        assert!(error.to_string().contains(expected), "{error}");
        assert!(!fixture.staging.exists());
    }
}

#[test]
fn rejects_symlinks_path_escape_and_preexisting_staging_without_removing_foreign_files() {
    for name in [ARCHIVE, SBOM, MANIFEST] {
        let fixture = support::fixture();
        let target = fixture.root.path().join("outside");
        fs::copy(fixture.producer.join(name), &target).expect("outside copy");
        fs::remove_file(fixture.producer.join(name)).expect("remove");
        symlink(&target, fixture.producer.join(name)).expect("planted link");
        assert!(verify(&fixture).is_err());
        assert!(target.exists());
    }
    let fixture = support::fixture();
    fs::create_dir(&fixture.staging).expect("foreign existing directory");
    fs::write(fixture.staging.join("keep"), b"foreign").expect("foreign bytes");
    assert!(verify(&fixture).is_err());
    assert_eq!(
        fs::read(fixture.staging.join("keep")).expect("preserved"),
        b"foreign"
    );
    assert!(
        verify_payload_output(
            &fixture.producer,
            "../escape",
            &fixture.staging,
            &fixture.request,
            CheckDeadline::after(Duration::from_secs(1)).expect("deadline")
        )
        .is_err()
    );
}

#[test]
fn rejects_claimed_digest_mismatch_and_mutation_of_staged_inspected_bytes() {
    let fixture = support::fixture();
    fs::write(fixture.producer.join(ARCHIVE), b"changed producer bytes").expect("mutation");
    assert!(verify(&fixture).is_err());
    assert!(!fixture.staging.exists());
    let fixture = support::fixture();
    let result = verify_with_hook(
        &fixture.producer,
        MANIFEST,
        &fixture.staging,
        &fixture.request,
        CheckDeadline::after(Duration::from_secs(10)).expect("deadline"),
        |stage| {
            let path = stage.join(SBOM);
            // Whitespace preserves valid JSON and the actual archive binding,
            // so only the final byte identity check rejects this mutation.
            let mut bytes = fs::read(&path).expect("copied SBOM");
            bytes.push(b' ');
            fs::write(path, bytes).expect("staged mutation");
        },
    );
    let error = result.expect_err("valid JSON with changed staged bytes must be rejected");
    assert!(
        error
            .to_string()
            .contains("preview_payload_staged_mutation"),
        "{error}"
    );
    assert!(!fixture.staging.exists());
}

#[test]
fn deadline_rejects_entry_without_creating_or_reusing_staging() {
    let fixture = support::fixture();
    let expired = CheckDeadline::from_start(Instant::now(), Duration::ZERO).expect("expired");
    assert!(
        verify_payload_output(
            &fixture.producer,
            MANIFEST,
            &fixture.staging,
            &fixture.request,
            expired
        )
        .is_err()
    );
    assert!(!fixture.staging.exists());
}

#[test]
fn cleanup_preserves_a_replacement_foreign_staging_directory() {
    let fixture = support::fixture();
    let retained = fixture.root.path().join("retained-original");
    let result = verify_with_hook(
        &fixture.producer,
        MANIFEST,
        &fixture.staging,
        &fixture.request,
        CheckDeadline::after(Duration::from_secs(10)).expect("deadline"),
        |stage| {
            fs::rename(stage, &retained).expect("move original owned inode");
            fs::create_dir(stage).expect("foreign replacement");
            fs::write(stage.join("keep"), b"foreign").expect("replacement bytes");
        },
    );
    assert!(result.is_err());
    assert_eq!(
        fs::read(fixture.staging.join("keep")).expect("foreign preserved"),
        b"foreign"
    );
    assert!(
        retained.join(ARCHIVE).exists(),
        "never delete a renamed path by guessing its location"
    );
}
