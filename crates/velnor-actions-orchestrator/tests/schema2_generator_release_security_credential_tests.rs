use super::{assert_exact_empty_verifier_credentials, assert_release_workflow_verifier};

const VALID: &str = include_str!("snapshots/generator-release-qualify-linux.yml");
const TRUSTED_PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin";
const ROOT_VALID: &str =
    include_str!("../../../.github/actions/generator-release-qualify-linux/action.yml");
const ROOT_VERIFIERS: &[&str] = &[
    "Resolve project tool versions",
    "Verify Rust channel manifest",
    "Verify jactionlint",
];
const FIXTURE_VERIFIERS: &[&str] = &["Verify Rust channel manifest", "Verify jactionlint"];

fn replace_fixture_step_env(
    body: &str,
    step_name: &str,
    needle: &str,
    replacement: &str,
) -> String {
    let marker = format!("    - name: {step_name}\n");
    assert_eq!(body.matches(&marker).count(), 1, "unique step {step_name}");
    let start = body.find(&marker).expect("step marker");
    let rest = &body[start..];
    let end = rest.find("\n    - name: ").unwrap_or(rest.len());
    let step = &rest[..end];
    assert_eq!(
        step.matches(needle).count(),
        1,
        "unique {needle:?} in {step_name}"
    );
    let changed_step = step.replacen(needle, replacement, 1);
    let mut changed = String::with_capacity(body.len() + replacement.len());
    changed.push_str(&body[..start]);
    changed.push_str(&changed_step);
    changed.push_str(&rest[end..]);
    assert_ne!(changed, body, "step mutation must change the fixture");
    changed
}

fn assert_fixture_mutation_rejected(body: &str, expected_error: &str, label: &str) {
    assert_ne!(body, VALID, "{label} mutation must change its fixture");
    let error = assert_exact_empty_verifier_credentials(body, FIXTURE_VERIFIERS)
        .expect_err("invalid verifier credential mapping was accepted");
    assert!(
        error.to_string().contains(expected_error),
        "{label} produced the wrong rejection: {error}"
    );
}

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

fn root_verifier_step<'a>(body: &'a str, step_name: &str) -> &'a str {
    let marker = format!("    - name: {step_name}\n");
    assert_eq!(body.matches(&marker).count(), 1, "unique step {step_name}");
    let start = body.find(&marker).expect("step marker");
    let rest = &body[start..];
    let end = rest.find("\n    - name: ").unwrap_or(rest.len());
    &rest[..end]
}

fn replace_root_verifier_path(body: &str, step_name: &str, replacement: &str) -> String {
    let step = root_verifier_step(body, step_name);
    assert_eq!(
        step.matches("        PATH: ").count(),
        1,
        "one PATH in {step_name}"
    );
    let step_start = body.find(step).expect("step belongs to workflow");
    let path_start = step.find("        PATH: ").expect("PATH entry");
    let path_end = step[path_start..]
        .find('\n')
        .map_or(step.len(), |offset| path_start + offset + 1);
    let mut changed = String::with_capacity(body.len() + replacement.len());
    changed.push_str(&body[..step_start + path_start]);
    changed.push_str(replacement);
    changed.push_str(&step[path_end..]);
    changed.push_str(&body[step_start + step.len()..]);
    assert_ne!(changed, body, "PATH mutation must change the fixture");
    changed
}

fn assert_root_path_rejected(
    step_name: &str,
    replacement: &str,
    expected_error: &str,
    label: &str,
) {
    let changed = replace_root_verifier_path(ROOT_VALID, step_name, replacement);
    for other in ROOT_VERIFIERS {
        if *other != step_name {
            assert_eq!(
                root_verifier_step(ROOT_VALID, other),
                root_verifier_step(&changed, other),
                "{label} must mutate only {step_name}"
            );
        }
    }
    let error = assert_exact_empty_verifier_credentials(&changed, ROOT_VERIFIERS)
        .expect_err("invalid committed verifier environment was accepted");
    assert!(
        error.to_string().contains(expected_error),
        "{label} produced the wrong rejection: {error}"
    );
}

fn assert_committed_verifier_path_cases(step_name: &str) {
    assert_root_path_rejected(
        step_name,
        "",
        "verifier environment set differs",
        "missing PATH",
    );
    assert_root_path_rejected(
        step_name,
        "        PATH: \"\"\n",
        "PATH must be exactly",
        "empty PATH",
    );
    assert_root_path_rejected(
        step_name,
        "        PATH: /tmp/poison\n",
        "PATH must be exactly",
        "poison PATH",
    );
    assert_root_path_rejected(
        step_name,
        &format!("        PATH: /tmp/poison:{TRUSTED_PATH}\n"),
        "PATH must be exactly",
        "poison PATH prefix",
    );
    assert_root_path_rejected(
        step_name,
        &format!("        PATH: {TRUSTED_PATH}:/tmp/poison\n"),
        "PATH must be exactly",
        "poison PATH suffix",
    );
    assert_root_path_rejected(
        step_name,
        "        PATH: ${{ env.PATH }}\n",
        "PATH must be exactly",
        "PATH expression",
    );
    assert_root_path_rejected(
        step_name,
        &format!("        PATH: \"{TRUSTED_PATH}\"\n"),
        "PATH must be exactly",
        "quoted PATH",
    );
    assert_root_path_rejected(
        step_name,
        &format!("        PATH: {TRUSTED_PATH}\n        PATH: {TRUSTED_PATH}\n"),
        "duplicate \"PATH\" env binding",
        "duplicate PATH",
    );
    assert_root_path_rejected(
        step_name,
        &format!("        PATH: {TRUSTED_PATH}\n        EXTRA_SETTING: value\n"),
        "unexpected env binding \"EXTRA_SETTING\"",
        "extra verifier environment key",
    );
}

#[test]
fn committed_verifiers_require_the_exact_twelve_key_environment() {
    let mut expected = velnor_actions_workflow_renderer::toolchain_env::STEP_CREDENTIAL_DENYLIST
        .into_iter()
        .chain(velnor_actions_workflow_renderer::toolchain_env::STEP_ENDPOINT_DENYLIST)
        .collect::<std::collections::BTreeSet<_>>();
    expected.insert("PATH");
    assert_eq!(expected.len(), 12);
    assert!(
        assert_exact_empty_verifier_credentials(ROOT_VALID, ROOT_VERIFIERS).is_ok(),
        "the committed verifier steps must satisfy the exact environment contract"
    );
    for step_name in ROOT_VERIFIERS {
        assert_committed_verifier_path_cases(step_name);
    }
}

#[test]
fn verifier_path_is_required_and_trusted() {
    assert_eq!(
        velnor_actions_workflow_renderer::VERIFY_PATH,
        TRUSTED_PATH,
        "the committed verifier PATH must remain independently pinned"
    );
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
    for step_name in FIXTURE_VERIFIERS {
        for value in ["\"literal\"", "\"${{ github.token }}\""] {
            let tampered = replace_fixture_step_env(
                VALID,
                step_name,
                "GH_TOKEN: \"\"\n",
                &format!("GH_TOKEN: {value}\n"),
            );
            assert_fixture_mutation_rejected(
                &tampered,
                "must be exactly empty in",
                &format!("{step_name} live or expression credential"),
            );
        }
    }
}

#[test]
fn rejects_duplicate_missing_and_alternate_token_bindings() {
    for step_name in FIXTURE_VERIFIERS {
        let duplicate = replace_fixture_step_env(
            VALID,
            step_name,
            "GH_TOKEN: \"\"\n",
            "GH_TOKEN: \"\"\n        GH_TOKEN: \"\"\n",
        );
        assert_fixture_mutation_rejected(
            &duplicate,
            "duplicate \"GH_TOKEN\" env binding",
            &format!("{step_name} duplicate token"),
        );

        let missing = replace_fixture_step_env(VALID, step_name, "GH_TOKEN: \"\"\n", "\n");
        assert_fixture_mutation_rejected(
            &missing,
            "verifier environment set differs",
            &format!("{step_name} missing token"),
        );

        let alternate = replace_fixture_step_env(
            VALID,
            step_name,
            "GH_TOKEN: \"\"\n",
            "GH_TOKEN: \"\"\n        EXTRA_TOKEN: \"\"\n",
        );
        assert_fixture_mutation_rejected(
            &alternate,
            "unexpected env binding \"EXTRA_TOKEN\"",
            &format!("{step_name} alternate token"),
        );
    }
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
