use super::{assert_exact_empty_verifier_credentials, assert_release_workflow_verifier};

const VALID: &str = include_str!("snapshots/generator-release-qualify-linux.yml");
const TRUSTED_PATH: &str = velnor_actions_workflow_renderer::VERIFY_PATH;

#[test]
fn exact_empty_credentials_are_limited_to_both_verifier_steps() {
    assert!(
        assert_exact_empty_verifier_credentials(
            VALID,
            &["Verify Rust channel manifest", "Verify jactionlint"],
        )
        .is_ok()
    );
}

#[test]
fn verifier_path_is_required_and_trusted() {
    let path_entry = format!("        PATH: {TRUSTED_PATH}\n");
    let missing = VALID.replacen(&path_entry, "", 1);
    assert_ne!(
        missing, VALID,
        "missing-PATH tamper must change its fixture"
    );
    assert!(
        assert_exact_empty_verifier_credentials(
            &missing,
            &["Verify Rust channel manifest", "Verify jactionlint"],
        )
        .is_err()
    );

    let poisoned = VALID.replacen(TRUSTED_PATH, "/tmp/poison", 1);
    assert_ne!(poisoned, VALID, "wrong-PATH tamper must change its fixture");
    assert!(
        assert_exact_empty_verifier_credentials(
            &poisoned,
            &["Verify Rust channel manifest", "Verify jactionlint"],
        )
        .is_err()
    );
}

#[test]
fn rejects_live_or_expression_credentials() {
    for value in ["\"literal\"", "\"${{ github.token }}\""] {
        let tampered = VALID.replacen("GH_TOKEN: \"\"", &format!("GH_TOKEN: {value}"), 1);
        assert!(
            assert_exact_empty_verifier_credentials(
                &tampered,
                &["Verify Rust channel manifest", "Verify jactionlint"],
            )
            .is_err()
        );
    }
}

#[test]
fn rejects_duplicate_missing_and_alternate_token_bindings() {
    let duplicate = VALID.replacen(
        "        GH_TOKEN: \"\"\n",
        "        GH_TOKEN: \"\"\n        GH_TOKEN: \"\"\n",
        1,
    );
    assert!(
        assert_exact_empty_verifier_credentials(
            &duplicate,
            &["Verify Rust channel manifest", "Verify jactionlint"],
        )
        .is_err()
    );

    let missing = VALID.replacen("        GH_TOKEN: \"\"\n", "", 1);
    assert!(
        assert_exact_empty_verifier_credentials(
            &missing,
            &["Verify Rust channel manifest", "Verify jactionlint"],
        )
        .is_err()
    );

    let alternate = VALID.replacen(
        "        GH_TOKEN: \"\"\n",
        "        GH_TOKEN: \"\"\n        EXTRA_TOKEN: \"\"\n",
        1,
    );
    assert!(
        assert_exact_empty_verifier_credentials(
            &alternate,
            &["Verify Rust channel manifest", "Verify jactionlint"],
        )
        .is_err()
    );
}

#[test]
fn rejects_credentials_outside_verifier_steps() {
    let misplaced = VALID.replacen(
        "    - name: Setup Mise\n",
        "    - name: Setup Mise\n      env:\n        GH_TOKEN: \"\"\n",
        1,
    );
    assert!(
        assert_exact_empty_verifier_credentials(
            &misplaced,
            &["Verify Rust channel manifest", "Verify jactionlint"],
        )
        .is_err()
    );

    let root_level = VALID.replacen("runs:\n", "runs:\n  env:\n    GH_TOKEN: \"\"\n", 1);
    assert!(
        assert_exact_empty_verifier_credentials(
            &root_level,
            &["Verify Rust channel manifest", "Verify jactionlint"],
        )
        .is_err()
    );

    let inline = VALID.replacen(
        "    - name: Setup Mise\n",
        "    - name: Setup Mise\n      env: { GH_TOKEN: \"\" }\n",
        1,
    );
    assert!(
        assert_exact_empty_verifier_credentials(
            &inline,
            &["Verify Rust channel manifest", "Verify jactionlint"],
        )
        .is_err()
    );
}

#[test]
fn release_workflow_keeps_registry_token_empty_only_on_verifier() {
    const WORKFLOW: &str = include_str!("snapshots/product-release-images.yml");
    assert!(
        assert_release_workflow_verifier(
            WORKFLOW,
            "Verify Rust channel manifest",
            "CARGO_REGISTRY_TOKEN",
        )
        .is_ok()
    );

    let live = WORKFLOW.replacen(
        "CARGO_REGISTRY_TOKEN: \"\"",
        "CARGO_REGISTRY_TOKEN: ${{ secrets.REGISTRY_TOKEN }}",
        1,
    );
    assert!(
        assert_release_workflow_verifier(
            &live,
            "Verify Rust channel manifest",
            "CARGO_REGISTRY_TOKEN",
        )
        .is_err()
    );

    let misplaced = WORKFLOW.replacen(
        "      - name: Build runner and DinD images\n",
        "      - name: Build runner and DinD images\n        env:\n          CARGO_REGISTRY_TOKEN: \"\"\n",
        1,
    );
    assert!(
        assert_release_workflow_verifier(
            &misplaced,
            "Verify Rust channel manifest",
            "CARGO_REGISTRY_TOKEN",
        )
        .is_err()
    );
}
