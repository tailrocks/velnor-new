//! Composed native ZIP -> reconstructed input -> actual pre-sign verifier.
use super::*;
use std::{fs, os::unix::fs::symlink};
use velnor_actions_contract::package_release::PackageReleaseChannel;

#[path = "preview_aggregate_admission_tests/rendered.rs"]
mod rendered;
#[path = "preview_aggregate_admission_tests/support.rs"]
mod support;
use support::Fixture;

#[test]
fn rendered_both_channel_aggregate_reaches_real_signer_input_verification() {
    for channel in [
        PackageReleaseChannel::Preview,
        PackageReleaseChannel::Stable,
    ] {
        let fixture = support::Fixture::new(channel);
        fixture.assert_rendered_sequence();
        let (downloaded, proof, manifest) = fixture.extract().expect("assembler-format ZIP");
        stage_signer_input(
            &fixture.root,
            &downloaded,
            &proof,
            &manifest,
            &proof.request.plan,
            &proof.request.context,
            Fixture::deadline(),
        )
        .expect("admit aggregate");
        let verified = crate::package_release_signer::verify_signer_input(
            &proof.request.plan,
            &fixture.identity(),
            &fixture.root,
        )
        .expect("same fixed directory consumed by real signer before crypto");
        let input = fixture
            .root
            .join(crate::package_release_signer::SIGNER_INPUT_DIR);
        assert!(input.join("metadata/license.txt").is_file());
        assert!(
            !input.join("LICENSE").exists(),
            "restore source path, not public alias"
        );
        assert!(!input.join(handoff::AGGREGATE_MANIFEST).exists());
        assert_eq!(verified.artifacts.len(), 3);
        assert!(
            verified
                .artifacts
                .iter()
                .any(|asset| asset.name == "LICENSE")
        );
    }
}

#[test]
fn aggregate_to_signer_rejects_changed_input_and_preexisting_or_linked_directory() {
    for mutation in [
        "missing",
        "changed",
        "extra",
        "manifest",
        "symlink",
        "preexisting",
    ] {
        let fixture = support::Fixture::new(PackageReleaseChannel::Preview);
        let (downloaded, proof, manifest) = fixture.extract().expect("ZIP");
        let input = fixture
            .root
            .join(crate::package_release_signer::SIGNER_INPUT_DIR);
        if mutation == "preexisting" {
            fs::create_dir(&input).expect("hostile existing path");
        }
        if mutation == "symlink" {
            symlink(&downloaded, &input).expect("hostile link");
        }
        let admitted = stage_signer_input(
            &fixture.root,
            &downloaded,
            &proof,
            &manifest,
            &proof.request.plan,
            &proof.request.context,
            Fixture::deadline(),
        );
        if matches!(mutation, "preexisting" | "symlink") {
            assert!(admitted.is_err(), "accepted {mutation}");
            continue;
        }
        admitted.expect("admit before mutation");
        let companion = input.join("metadata/license.txt");
        match mutation {
            "missing" => fs::remove_file(companion).expect("remove"),
            "changed" => fs::write(companion, b"changed").expect("replace"),
            "extra" => fs::write(input.join("extra"), b"extra").expect("extra"),
            "manifest" => fs::write(input.join("package-output.json"), b"{}").expect("manifest"),
            _ => unreachable!("closed cases"),
        }
        assert!(
            crate::package_release_signer::verify_signer_input(
                &proof.request.plan,
                &fixture.identity(),
                &fixture.root
            )
            .is_err(),
            "accepted {mutation}"
        );
    }
}

#[test]
fn aggregate_zip_remains_bound_to_policy_source_attempt_and_exact_inventory() {
    for mutation in ["policy", "source", "attempt", "extra", "plan", "digest"] {
        let mut fixture = support::Fixture::new(PackageReleaseChannel::Preview);
        let mut plan = fixture.proof.request.plan.clone();
        let mut context = fixture.proof.request.context.clone();
        let policy = if mutation == "policy" {
            "e".repeat(64)
        } else {
            "d".repeat(64)
        };
        if mutation == "source" {
            context.source_sha = "b".repeat(40);
        }
        if mutation == "attempt" {
            context.run_attempt += 1;
        }
        if mutation == "plan" {
            plan.base_version = "2.0.0".into();
        }
        if mutation == "digest" {
            fixture.files[0].1[0] ^= 1;
        }
        let archive = if mutation == "extra" {
            fixture.zip(true)
        } else {
            fixture.zip(false)
        };
        assert!(
            extract_aggregate(
                &mut fs::File::open(archive).expect("archive"),
                &fixture.root,
                &policy,
                &plan,
                &context,
                Fixture::deadline()
            )
            .is_err(),
            "accepted {mutation}"
        );
    }
}

#[test]
fn restored_destinations_reject_collisions_before_any_staging_write() {
    for path in [
        "package-output.json",
        "package-output.json/child",
        "cli-x86_64-unknown-linux-gnu.tar.gz.sbom.json",
        "cli-x86_64-unknown-linux-gnu.tar.gz.sbom.json/child",
    ] {
        let fixture = support::Fixture::new(PackageReleaseChannel::Stable);
        let (downloaded, proof, manifest) = fixture.extract().expect("ZIP");
        let mut plan = proof.request.plan.clone();
        plan.companions[0].path = path.into();
        assert!(
            stage_signer_input(
                &fixture.root,
                &downloaded,
                &proof,
                &manifest,
                &plan,
                &proof.request.context,
                Fixture::deadline()
            )
            .is_err()
        );
        assert!(
            !fixture
                .root
                .join(crate::package_release_signer::SIGNER_INPUT_DIR)
                .exists()
        );
    }
}

#[test]
fn restored_paths_refuse_planted_ancestor_final_link_and_existing_leaf() {
    for mutation in ["ancestor", "final", "existing"] {
        let fixture = support::Fixture::new(PackageReleaseChannel::Stable);
        let (downloaded, proof, manifest) = fixture.extract().expect("ZIP");
        let input = fixture.root.join("hostile-input");
        fs::create_dir(&input).expect("input");
        let outside = fixture.root.join("outside");
        fs::create_dir(&outside).expect("outside");
        let victim = outside.join("victim");
        fs::write(&victim, b"untouched").expect("victim");
        let metadata = input.join("metadata");
        if mutation == "ancestor" {
            symlink(&outside, &metadata).expect("ancestor link");
        } else {
            fs::create_dir(&metadata).expect("metadata");
            if mutation == "final" {
                symlink(&victim, metadata.join("license.txt")).expect("final link");
            } else {
                fs::write(metadata.join("license.txt"), b"untouched").expect("existing");
            }
        }
        assert!(
            copy_full_input(
                &downloaded,
                &input,
                &proof,
                &manifest,
                &proof.request.plan,
                Fixture::deadline()
            )
            .is_err(),
            "accepted {mutation}"
        );
        assert_eq!(fs::read(victim).expect("victim"), b"untouched");
        if mutation == "existing" {
            assert_eq!(
                fs::read(metadata.join("license.txt")).expect("leaf"),
                b"untouched"
            );
        }
    }
}
