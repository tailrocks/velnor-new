//! Offline proof that the rendered seed manifest passes ordinary preparation.

use std::{fs, path::Path};

use super::super::seed;

#[test]
fn current_seed_manifest_passes_ordinary_prepare_over_legacy_manifest_base() {
    let root = tap_repository();
    let (version, sdk_manifest, manifest, manifest_path) = install_current_seed(root.path());

    assert_legacy_manifest_rejects(root.path(), &manifest_path, &version);

    fs::write(&manifest_path, &sdk_manifest).expect("install exact authenticated seed manifest");
    let preparation = crate::prepare::prepare(root.path()).expect("ordinary prepare succeeds");
    assert_eq!(
        preparation.discovery.consumer_manifest_json.as_deref(),
        Some(std::str::from_utf8(&sdk_manifest).expect("manifest UTF-8"))
    );
    assert_acquire_uses_manifest(&preparation, &manifest);

    fs::write(&manifest_path, b"not json").expect("tampered manifest");
    let tampered_error = crate::prepare::prepare(root.path()).expect_err("tampering rejects");
    assert!(
        tampered_error.to_string().contains("malformed_json"),
        "unexpected tampered manifest failure: {tampered_error}"
    );
    assert!(
        crate::pins::consumer_acquire_step_with_manifest("ubuntu-26.04", &version, None).is_err(),
        "the normal consumer acquire gate rejects a missing manifest"
    );
}

fn tap_repository() -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("Tap repository");
    let git = crate::git_fixture::sterile_command()
        .args(["init", "-q", "-b", "main"])
        .current_dir(root.path())
        .output()
        .expect("initialize Tap repository");
    assert!(
        git.status.success(),
        "git init failed: {}",
        String::from_utf8_lossy(&git.stderr)
    );
    root
}

fn install_current_seed(
    root: &Path,
) -> (
    String,
    Vec<u8>,
    velnor_actions_contract::ReleaseManifest,
    std::path::PathBuf,
) {
    let transition = seed::load_transition("jackin-homebrew-seed-921858f1").expect("transition");
    let version = env!("CARGO_PKG_VERSION").to_owned();
    let source_sha = "c".repeat(40);
    let sdk_manifest = seed_test_manifest(&version, &source_sha);
    let sdk_manifest_sha = crate::generator::sha256_hex(&sdk_manifest);
    let manifest = velnor_actions_contract::ReleaseManifest::parse_json(
        std::str::from_utf8(&sdk_manifest).expect("manifest UTF-8"),
        "fixture release manifest",
    )
    .expect("fixture manifest parses");
    let host_sha = manifest
        .record_for_target("x86_64-unknown-linux-gnu")
        .expect("Linux x64 host record")
        .sha256
        .clone();
    let rendered = velnor_actions_workflow_renderer::render_homebrew_adoption_seed(
        &transition,
        &velnor_actions_workflow_renderer::HomebrewAdoptionSeedSdkIdentity {
            source_sha: &source_sha,
            version: &version,
            manifest_bytes: &sdk_manifest,
            manifest_sha256: &sdk_manifest_sha,
            host_target_sha256: &host_sha,
        },
        &crate::product_release_pins::homebrew_adoption_pins().expect("compiled seed pins"),
    )
    .expect("current SDK seed rendering");
    let rendered_manifest = rendered
        .iter()
        .find(|file| file.path == ".velnor/release-manifest.json")
        .expect("renderer includes the manifest output");
    assert_eq!(rendered_manifest.bytes.as_bytes(), sdk_manifest.as_slice());
    for file in &rendered {
        if file.path != ".velnor/release-manifest.json" {
            write_rendered_file(root, file);
        }
    }
    let manifest_path = root.join(".velnor/release-manifest.json");
    fs::write(&manifest_path, seed_test_manifest("0.1.48", &source_sha))
        .expect("legacy Tap manifest");
    (version, sdk_manifest, manifest, manifest_path)
}

fn write_rendered_file(root: &Path, file: &velnor_actions_workflow_renderer::RenderedFile) {
    let path = root.join(&file.path);
    fs::create_dir_all(path.parent().expect("rendered file parent")).expect("parent");
    fs::write(path, file.bytes.as_bytes()).expect("rendered seed file");
}

fn assert_legacy_manifest_rejects(root: &Path, path: &Path, version: &str) {
    let error = crate::prepare::prepare(root).expect_err("legacy version rejects");
    assert!(
        error.to_string().contains(&format!(
            "release_manifest_version_mismatch:0.1.48:{version}"
        )),
        "unexpected legacy seed failure: {error}"
    );
    assert!(path.is_file(), "legacy manifest remains installed");
}

fn assert_acquire_uses_manifest(
    preparation: &crate::prepare::GenerationPreparation,
    manifest: &velnor_actions_contract::ReleaseManifest,
) {
    let acquire = preparation
        .workflow
        .ir
        .jobs
        .values()
        .flat_map(|job| &job.steps)
        .find(|step| step.role == Some(velnor_actions_contract::StepRole::AcquireVelnor))
        .expect("ordinary plan includes SDK acquisition");
    let velnor_actions_contract::StepKind::Shell { run, env } = &acquire.kind else {
        panic!("Acquire Velnor must be a shell step");
    };
    let host_record = manifest
        .record_for_target("x86_64-unknown-linux-gnu")
        .expect("host record");
    assert_eq!(
        env.get(velnor_actions_workflow_renderer::ASSET_URL_ENV),
        Some(&host_record.artifact)
    );
    assert_eq!(
        env.get(velnor_actions_workflow_renderer::ASSET_SHA_ENV),
        Some(&host_record.sha256)
    );
    assert!(run.iter().any(|line| line.contains("$VELNOR_ASSET_URL")));
    assert!(run.iter().any(|line| line.contains("$VELNOR_ASSET_SHA256")));
}

fn seed_test_manifest(version: &str, source_sha: &str) -> Vec<u8> {
    let sha = "b".repeat(64);
    let targets = velnor_actions_contract::SUPPORTED_TARGETS
        .iter()
        .map(|target| {
            format!(
                "{{\"target\":\"{target}\",\"artifact\":\"https://github.com/tailrocks/velnor-new/releases/download/v{version}/velnor-actions-{version}-{target}\",\"sha256\":\"{sha}\"}}"
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"schema\":1,\"version\":\"{version}\",\"repository\":\"tailrocks/velnor-new\",\"commit\":\"{source_sha}\",\"targets\":[{targets}]}}"
    )
    .into_bytes()
}
