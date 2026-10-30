use super::*;
use velnor_actions_contract::config::NativeHomebrewOperation;

const SOURCE: &str = "cccccccccccccccccccccccccccccccccccccccc";
const HOST_SHA: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn transition() -> HomebrewAdoptionTransition {
    velnor_actions_contract::homebrew_adoption::parse_homebrew_adoption_transition(include_bytes!(
        "../../../.velnor/qualification/homebrew/jackin/transition.json"
    ))
    .expect("protected seed transition")
}

fn manifest() -> Vec<u8> {
    format!(
        r#"{{"schema":1,"version":"0.1.55","repository":"tailrocks/velnor-new","commit":"{SOURCE}","targets":[{{"target":"x86_64-unknown-linux-gnu","artifact":"https://github.com/tailrocks/velnor-new/releases/download/v0.1.55/velnor-actions-0.1.55-x86_64-unknown-linux-gnu","sha256":"{HOST_SHA}"}},{{"target":"aarch64-apple-darwin","artifact":"https://github.com/tailrocks/velnor-new/releases/download/v0.1.55/velnor-actions-0.1.55-aarch64-apple-darwin","sha256":"{HOST_SHA}"}},{{"target":"x86_64-apple-darwin","artifact":"https://github.com/tailrocks/velnor-new/releases/download/v0.1.55/velnor-actions-0.1.55-x86_64-apple-darwin","sha256":"{HOST_SHA}"}}]}}"#
    )
    .into_bytes()
}

fn identity<'a>(
    manifest_bytes: &'a [u8],
    manifest_sha256: &'a str,
) -> HomebrewAdoptionSeedSdkIdentity<'a> {
    HomebrewAdoptionSeedSdkIdentity {
        source_sha: SOURCE,
        version: "0.1.55",
        manifest_bytes,
        manifest_sha256,
        host_target_sha256: HOST_SHA,
    }
}

#[test]
fn exact_authenticated_seed_bundle_is_deterministic_and_closed() {
    let transition = transition();
    let manifest = manifest();
    let digest = sha256(&manifest);
    let sdk = identity(&manifest, &digest);
    let pins = crate::schema2::product_release_test_pins::test_pins();
    let first = render_homebrew_adoption_seed(&transition, &sdk, &pins).expect("render");
    let second = render_homebrew_adoption_seed(&transition, &sdk, &pins).expect("render");
    assert_eq!(first, second);
    assert_eq!(
        first
            .iter()
            .find(|file| file.path == ".velnor/release-manifest.json")
            .expect("authenticated manifest output")
            .bytes
            .as_bytes(),
        manifest.as_slice()
    );
    assert_eq!(
        first
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>(),
        transition
            .generated_output_paths
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
    );

    let ci = &first[0].bytes;
    assert!(ci.contains(&format!("@{SOURCE}")));
    assert!(ci.contains(sdk.manifest_sha256));
    assert!(ci.contains("transition_id: jackin-homebrew-seed-921858f1"));
    assert!(!ci.contains("secrets: inherit"));
    assert!(!ci.contains("GITHUB_WORKFLOW_SHA"));

    let callee =
        crate::homebrew_adoption_workflow::render_homebrew_adoption_callee(sdk.version, &pins)
            .expect("callee");
    assert!(!callee.file.bytes.contains(SOURCE));
    assert!(!callee.file.bytes.contains(sdk.manifest_sha256));
    assert!(!callee.file.bytes.contains("jackin-homebrew-seed-921858f1"));

    let config: velnor_actions_contract::config::VelnorConfig =
        toml::from_str(&first[1].bytes).expect("typed generated config parses");
    config
        .validate(".velnor/config.toml")
        .expect("seed config validates");
    assert_eq!(config.native_checks.len(), 2);
    assert_eq!(
        config.native_checks[0].operation,
        NativeHomebrewOperation::TapTree
    );
    assert_eq!(
        config.native_checks[1].operation,
        NativeHomebrewOperation::AdoptionContract
    );
    let review = config
        .workflow
        .review_completion
        .expect("preserved exact-head review");
    assert_eq!(review.check_name, "Review completion");
    assert_eq!(review.app_id_variable, "REVIEW_COMPLETION_APP_ID");
    assert_eq!(
        review.app_private_key_secret,
        "REVIEW_COMPLETION_APP_PRIVATE_KEY"
    );
    assert_eq!(review.reviewer_app_id_variable, "REVIEWER_APP_ID");

    let mise: toml::Value = toml::from_str(&first[4].bytes).expect("mise config parses");
    assert_eq!(mise["settings"]["lockfile"].as_bool(), Some(true));
    let lock = velnor_actions_mise::toolfiles::lockfile::parse_mise_lockfile(&first[3].bytes)
        .expect("empty generated lock parses");
    assert!(lock.tools.is_empty());
}

#[test]
fn renderer_rejects_manifest_source_hash_version_and_transition_drift() {
    let transition = transition();
    let manifest = manifest();
    let digest = sha256(&manifest);
    let pins = crate::schema2::product_release_test_pins::test_pins();
    let mut sdk = identity(&manifest, &digest);
    sdk.source_sha = "dddddddddddddddddddddddddddddddddddddddd";
    assert!(render_homebrew_adoption_seed(&transition, &sdk, &pins).is_err());

    sdk = identity(&manifest, &digest);
    sdk.manifest_sha256 = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    assert!(render_homebrew_adoption_seed(&transition, &sdk, &pins).is_err());

    sdk = identity(&manifest, &digest);
    sdk.version = "0.1.54";
    assert!(render_homebrew_adoption_seed(&transition, &sdk, &pins).is_err());

    let mut altered = transition.clone();
    altered
        .generated_output_paths
        .push("scripts/attacker.sh".to_owned());
    assert!(render_homebrew_adoption_seed(&altered, &identity(&manifest, &digest), &pins).is_err());

    let mut legacy = transition.clone();
    legacy.template_version = 1;
    legacy
        .generated_output_paths
        .retain(|path| path != ".velnor/release-manifest.json");
    assert!(render_homebrew_adoption_seed(&legacy, &identity(&manifest, &digest), &pins).is_err());

    let tampered_manifest = String::from_utf8(manifest.clone())
        .expect("fixture manifest is UTF-8")
        .replace("0.1.55", "0.1.54")
        .into_bytes();
    let tampered_digest = sha256(&tampered_manifest);
    let tampered = identity(&tampered_manifest, &tampered_digest);
    assert!(render_homebrew_adoption_seed(&transition, &tampered, &pins).is_err());
}
