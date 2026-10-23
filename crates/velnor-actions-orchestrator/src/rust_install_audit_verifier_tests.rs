use super::{VERIFIED, files, inline_step, inline_step_with_env, problem, verifiers};

#[test]
fn existing_inline_release_verifiers_remain_exactly_audited() {
    let verifiers = verifiers();
    let rust_template = verifiers.inline_rust_templates["Verify Rust channel manifest"]
        .get(1)
        .expect("canonical direct-shell Rust verifier");
    let jactionlint_template = verifiers
        .inline_jactionlint_templates
        .last()
        .expect("canonical direct-shell jactionlint verifier");
    let workflow = format!(
        "jobs:\n  release:\n    steps:\n      - name: Install pinned Rust\n        run: mise install rust@1.99.1\n{}{}",
        inline_step("Verify Rust channel manifest", rust_template),
        inline_step("Verify jactionlint", jactionlint_template),
    );
    assert_eq!(
        problem(&files(".github/workflows/release.yml", &workflow)),
        ""
    );

    let changed = workflow.replace("channel.toml", "untrusted.toml");
    let found = problem(&files(".github/workflows/release.yml", &changed));
    assert!(
        found.contains("noncanonical verifier call or body"),
        "{found}"
    );
}

fn producer_home_template() -> crate::workflow::verifier_actions::InlineVerifierTemplate {
    verifiers().inline_rust_templates["Verify Rust channel manifest"]
        .last()
        .expect("canonical producer-home Rust verifier")
        .clone()
}

fn source_prep_workflow(
    template: &crate::workflow::verifier_actions::InlineVerifierTemplate,
    env_reversed: bool,
) -> String {
    let step = if env_reversed {
        inline_step_with_env(
            "Verify Rust channel manifest",
            template,
            template.env.iter().rev(),
        )
    } else {
        inline_step("Verify Rust channel manifest", template)
    };
    format!(
        "jobs:\n  producer:\n    steps:\n      - name: Install pinned Rust\n        run: mise install rust@1.99.1\n{step}"
    )
}

fn assert_source_prep_rejected(workflow: &str, reason: &str) {
    let found = problem(&files(".github/workflows/binary-release.yml", workflow));
    assert!(
        found.contains("noncanonical verifier call or body"),
        "{reason} must fail closed: {found}"
    );
}

#[test]
fn standalone_source_prep_verifier_accepts_only_its_shared_exact_job_environment() {
    let template = producer_home_template();
    assert_eq!(template.env["MISE_AUTO_INSTALL"], "false");
    assert_eq!(template.env["MISE_NO_CONFIG"], "1");
    assert_eq!(template.env["CARGO_HOME"], template.env["MISE_CARGO_HOME"]);
    assert_eq!(
        template.env["RUSTUP_HOME"],
        template.env["MISE_RUSTUP_HOME"]
    );
    assert_eq!(template.env["RUSTUP_TOOLCHAIN"], "1.99.1");

    let canonical = source_prep_workflow(&template, false);
    assert_eq!(
        problem(&files(".github/workflows/binary-release.yml", &canonical)),
        ""
    );
    let reordered = source_prep_workflow(&template, true);
    assert_eq!(
        problem(&files(".github/workflows/binary-release.yml", &reordered)),
        "",
        "environment declaration order does not change the exact mapping"
    );
}

#[test]
fn standalone_source_prep_verifier_rejects_changed_configuration_and_home() {
    let template = producer_home_template();
    let canonical = source_prep_workflow(&template, false);
    let changed_flag = canonical.replace(
        &format!(
            "          MISE_NO_CONFIG: {}",
            velnor_actions_workflow_renderer::yaml::quote_scalar(&template.env["MISE_NO_CONFIG"])
        ),
        "          MISE_NO_CONFIG: \"0\"",
    );
    assert_ne!(changed_flag, canonical);
    assert_source_prep_rejected(&changed_flag, "changed project-config isolation");

    let changed_home = canonical.replace(
        &format!(
            "          RUSTUP_HOME: {}",
            velnor_actions_workflow_renderer::yaml::quote_scalar(&template.env["RUSTUP_HOME"])
        ),
        "          RUSTUP_HOME: /tmp/untrusted-rustup",
    );
    assert_ne!(changed_home, canonical);
    assert_source_prep_rejected(&changed_home, "changed producer home");
}

#[test]
fn standalone_source_prep_verifier_rejects_missing_duplicate_and_extra_environment() {
    let template = producer_home_template();
    let canonical = source_prep_workflow(&template, false);
    let no_config = format!(
        "          MISE_NO_CONFIG: {}\n",
        velnor_actions_workflow_renderer::yaml::quote_scalar(&template.env["MISE_NO_CONFIG"])
    );
    let missing_entry = canonical.replace(&no_config, "");
    assert_ne!(missing_entry, canonical);
    assert_source_prep_rejected(&missing_entry, "missing project-config isolation");

    let duplicate_entry = canonical.replace(&no_config, &format!("{no_config}{no_config}"));
    assert_ne!(duplicate_entry, canonical);
    assert_source_prep_rejected(&duplicate_entry, "duplicate environment keys");

    let extra_entry = canonical.replace(
        "        env:\n",
        "        env:\n          UNEXPECTED: \"1\"\n",
    );
    assert_ne!(extra_entry, canonical);
    assert_source_prep_rejected(&extra_entry, "additional caller environment");
}

#[test]
fn inline_verifiers_reject_skips_and_noncanonical_shell_or_environment() {
    let actions = verifiers();
    let rust_template = actions.inline_rust_templates["Verify Rust channel manifest"]
        .first()
        .expect("canonical Rust verifier with owned homes");
    let jactionlint_template = actions
        .inline_jactionlint_templates
        .last()
        .expect("canonical direct-shell jactionlint verifier");
    let canonical = format!(
        "jobs:\n  release:\n    steps:\n{}{}",
        inline_step("Verify Rust channel manifest", rust_template),
        inline_step("Verify jactionlint", jactionlint_template),
    );
    assert_eq!(
        problem(&files(".github/workflows/release.yml", &canonical)),
        ""
    );

    for (case, changed) in [
        (
            "skipped condition",
            canonical.replace(
                "      - name: Verify Rust channel manifest\n        env:",
                "      - name: Verify Rust channel manifest\n        if: false\n        env:",
            ),
        ),
        (
            "wrong shell",
            canonical.replace(
                &format!(
                    "        shell: {}",
                    velnor_actions_workflow_renderer::yaml::quote_scalar(
                        jactionlint_template
                            .shell
                            .expect("canonical jactionlint shell")
                    )
                ),
                "        shell: sh",
            ),
        ),
        (
            "wrong rustup home",
            canonical.replace(
                &format!(
                    "          RUSTUP_HOME: {}",
                    velnor_actions_workflow_renderer::yaml::quote_scalar(
                        rust_template.env["RUSTUP_HOME"].as_str()
                    )
                ),
                "          RUSTUP_HOME: /tmp/untrusted-rustup",
            ),
        ),
        (
            "unexpected environment",
            canonical.replace(
                "        env:\n",
                "        env:\n          JACTIONLINT_HOME: /tmp/untrusted\n",
            ),
        ),
    ] {
        assert_ne!(changed, canonical, "{case} control must mutate its source");
        let found = problem(&files(".github/workflows/release.yml", &changed));
        assert!(
            found.contains("noncanonical verifier call or body"),
            "{case} must fail closed: {found}"
        );
    }
}

#[test]
fn direct_jactionlint_script_is_exactly_audited() {
    let actions = verifiers();
    let jactionlint_template = actions
        .inline_jactionlint_templates
        .last()
        .expect("canonical direct-shell jactionlint verifier");
    let workflow = format!(
        "jobs:\n  release:\n    steps:\n{}",
        inline_step("Verify jactionlint", jactionlint_template),
    );
    assert_eq!(
        problem(&files(".github/workflows/upgrades.yml", &workflow)),
        ""
    );

    let changed = workflow.replace(
        "jactionlint_executable_digest_mismatch",
        "jactionlint_wrong_digest_failure",
    );
    let found = problem(&files(".github/workflows/upgrades.yml", &changed));
    assert!(
        found.contains("noncanonical verifier call or body"),
        "a changed direct digest script must fail exact auditing: {found}"
    );

    let expected_digest = velnor_actions_mise::JACTIONLINT_DIGESTS[0].executable_sha256;
    let zero_digest = "0".repeat(64);
    for replacement in ["", zero_digest.as_str()] {
        let changed = workflow.replace(expected_digest, replacement);
        assert_ne!(changed, workflow, "digest mutation must change the script");
        let found = problem(&files(".github/workflows/upgrades.yml", &changed));
        assert!(
            found.contains("noncanonical verifier call or body"),
            "missing or wrong direct digest must fail exact auditing: {found}"
        );
    }
}

#[test]
fn inline_verifier_acceptance_uses_exact_payload_and_rust_install_phase() {
    let actions = verifiers();
    let installed_rust = actions
        .inline_installed_rust_templates
        .first()
        .expect("installed Rust inline verifier");
    let renamed_rust = inline_step("Check pinned toolchain evidence", installed_rust);
    let installed_workflow = format!(
        "jobs:\n  release:\n    steps:\n      - name: Install pinned Rust\n        run: mise install rust@1.99.1\n{renamed_rust}"
    );
    assert_eq!(
        problem(&files(".github/workflows/release.yml", &installed_workflow)),
        "",
        "a canonical installed verifier remains valid after a display-name change"
    );

    let restored_rust = actions.inline_rust_templates["Verify restored Rust toolchain"]
        .first()
        .expect("restored Rust inline verifier");
    let restored_workflow = format!(
        "jobs:\n  release:\n    steps:\n      - name: Install pinned Rust\n        run: mise install rust@1.99.1\n{}",
        inline_step("Check pinned toolchain evidence", restored_rust),
    );
    let found = problem(&files(".github/workflows/release.yml", &restored_workflow));
    assert!(
        found.contains("no verification follows the install"),
        "a restored-cache verifier cannot satisfy the installed phase: {found}"
    );

    let jactionlint = actions
        .inline_jactionlint_templates
        .last()
        .expect("canonical direct-shell Jactionlint verifier");
    let renamed_jactionlint = inline_step("Check executable digest", jactionlint);
    assert_eq!(
        problem(&files(
            ".github/workflows/release.yml",
            &format!("jobs:\n  release:\n    steps:\n{renamed_jactionlint}"),
        )),
        "",
        "a canonical Jactionlint payload remains valid after a display-name change"
    );
}

#[test]
fn jactionlint_composite_binds_its_digest_table() {
    let digest = velnor_actions_mise::JACTIONLINT_DIGESTS[0].executable_sha256;
    let zero_digest = "0".repeat(64);
    for replacement in ["", zero_digest.as_str()] {
        let mut changed = files(".github/workflows/release.yml", VERIFIED);
        let action = changed
            .iter_mut()
            .find(|file| file.path == ".github/actions/velnor-verify-jactionlint/action.yml")
            .expect("canonical jactionlint verifier");
        let original = action.bytes.clone();
        action.bytes = action.bytes.replacen(digest, replacement, 1);
        assert_ne!(
            action.bytes, original,
            "digest mutation must change the action"
        );
        let found = problem(&changed);
        assert!(
            found.contains("verifier action body, shell, environment, or marker changed"),
            "missing or wrong composite digest must fail exact auditing: {found}"
        );
    }
}

#[test]
fn an_install_that_runs_rustc_in_the_same_step_fails() {
    let text = "jobs:
  q:
    steps:
      - name: Install pinned Rust toolchain
        run: mise install rust@1.99.1 && mise exec rust@1.99.1 -- rustc --print sysroot
      - name: Verify Rust channel manifest
        uses: ./.github/actions/velnor-verify-rust-manifest # zizmor: ignore[self-repository]
";
    let found = problem(&files(".github/workflows/qualification.yml", text));
    assert!(found.contains("runs the toolchain it installs"), "{found}");
}

#[test]
fn composite_actions_and_step_anchors_are_audited() {
    let action = "runs:
  using: composite
  steps:
    - name: Install pinned qualification tools
      run: mise --no-config --no-env --no-hooks install 'rust@'\"${VELNOR_TOOL_RUST}\" actionlint@1.7.12
    - name: Download built asset archive
      uses: actions/download-artifact@v4
";
    let found = problem(&files(".github/actions/qualify/action.yml", action));
    assert!(
        found.contains("rust_install_unverified:.github/actions/qualify/action.yml:runs:Install pinned qualification tools:"),
        "{found}"
    );
    let anchored = "jobs:
  a:
    steps:
      - &s1
        name: Prepare pinned tools
        run: mise --no-config --no-env --no-hooks install 'rust@'\"${VELNOR_TOOL_RUST}\"
      - name: Verify Rust channel manifest
        uses: ./.github/actions/velnor-verify-rust-manifest # zizmor: ignore[self-repository]
  b:
    steps:
      - *s1
      - name: Build
        run: cargo build
";
    let found = problem(&files(".github/workflows/ci.yml", anchored));
    assert!(found.contains("ci.yml:b:Prepare pinned tools:"), "{found}");
}

#[test]
fn files_outside_github_yaml_are_ignored() {
    let text = VERIFIED.replace("Verify Rust channel manifest", "Something else");
    assert_eq!(problem(&files("docs/example.yml", &text)), "");
    assert_eq!(problem(&files(".github/README.md", &text)), "");
}

#[test]
fn a_step_that_verifies_its_own_install_before_any_use_passes() {
    let verified = "jobs:
  verify:
    steps:
      - name: Regenerate
        run: \"mise --no-config --no-env --no-hooks install \\\"rust@$rust\\\" || return 1\\nVELNOR_TOOL_RUST=$rust sh -c \\\"$VELNOR_VERIFY_RUST_SCRIPT\\\"\\nmise exec \\\"rust@$rust\\\" -- cargo fetch\"
";
    assert_eq!(
        problem(&files(".github/workflows/upgrades.yml", verified)),
        ""
    );
    let late = verified.replace(
        "VELNOR_TOOL_RUST=$rust sh -c \\\"$VELNOR_VERIFY_RUST_SCRIPT\\\"\\nmise exec \\\"rust@$rust\\\" -- cargo fetch",
        "mise exec \\\"rust@$rust\\\" -- cargo fetch\\nVELNOR_TOOL_RUST=$rust sh -c \\\"$VELNOR_VERIFY_RUST_SCRIPT\\\"",
    );
    let found = problem(&files(".github/workflows/upgrades.yml", &late));
    assert!(
        found.contains("rust_install_unverified:.github/workflows/upgrades.yml:verify:Regenerate:"),
        "{found}"
    );
}
