use velnor_actions_mise::ToolCatalog;
use velnor_actions_workflow_renderer::render::RenderedFile;

use super::audit;
use crate::verifier_actions::VerifierActionSet;

fn verifiers() -> VerifierActionSet {
    VerifierActionSet::build(
        &ToolCatalog::pinned().for_emission(),
        env!("CARGO_PKG_VERSION"),
    )
    .expect("canonical verifier actions")
}

fn files(path: &str, bytes: &str) -> Vec<RenderedFile> {
    let mut files = vec![RenderedFile {
        path: path.to_owned(),
        bytes: bytes.to_owned(),
    }];
    let verifiers = verifiers();
    files.extend(verifiers.files());
    files
}

fn problem(files: &[RenderedFile]) -> String {
    match audit(files) {
        Ok(()) => String::new(),
        Err(error) => error.to_string(),
    }
}

fn inline_step(name: &str, template: &crate::verifier_actions::InlineVerifierTemplate) -> String {
    inline_step_with_env(name, template, template.env.iter())
}

fn inline_step_with_env<'a>(
    name: &str,
    template: &'a crate::verifier_actions::InlineVerifierTemplate,
    env: impl Iterator<Item = (&'a String, &'a String)>,
) -> String {
    let mut lines = vec![
        format!(
            "      - name: {}",
            velnor_actions_workflow_renderer::yaml::quote_scalar(name)
        ),
        "        env:".to_owned(),
    ];
    lines.extend(env.map(|(key, value)| {
        format!(
            "          {key}: {}",
            velnor_actions_workflow_renderer::yaml::quote_scalar(value)
        )
    }));
    lines.push(format!(
        "        shell: {}",
        velnor_actions_workflow_renderer::yaml::quote_scalar(
            template.shell.expect("canonical verifier shell")
        )
    ));
    lines.push(format!("        run: {}", template.run_scalar));
    lines.join("\n") + "\n"
}

const INSTALLED: &str = "      - name: Verify Rust channel manifest
        uses: ./.github/actions/velnor-verify-rust-manifest # zizmor: ignore[self-repository]
";

const VERIFIED: &str = "jobs:
  build:
    steps:
      - name: Verify restored Rust toolchain
        uses: ./.github/actions/velnor-verify-restored-rust # zizmor: ignore[self-repository]
      - name: Install pinned Rust and MBX
        run: mise --no-config --no-env --no-hooks install 'rust@'\"${VELNOR_TOOL_RUST}\" mr-boxington@1.26.0
      - name: Install x86_64-apple-darwin Rust target
        run: mise exec 'rust@'\"${VELNOR_TOOL_RUST}\" -- rustup target add x86_64-apple-darwin
      - name: Verify Rust channel manifest
        uses: ./.github/actions/velnor-verify-rust-manifest # zizmor: ignore[self-repository]
      - name: Build
        run: mise exec rust@1.99.1 -- cargo build
";

#[test]
fn an_install_followed_by_the_verification_passes() {
    assert_eq!(
        problem(&files(".github/workflows/release.yml", VERIFIED)),
        ""
    );
}

#[test]
fn verifier_composites_are_catalog_pinned_and_have_no_unbound_project_refs() {
    let actions = verifiers();
    for action in [&actions.restored_rust, &actions.installed_rust] {
        assert!(action.file.bytes.contains("1.99.1"), "{}", action.file.path);
        assert!(
            !action.file.bytes.contains("${VELNOR_TOOL_"),
            "{} contains an unresolved project tool reference",
            action.file.path
        );
    }
    assert!(actions.jactionlint.file.bytes.contains("2.3.0"));
    assert!(!actions.jactionlint.file.bytes.contains("${VELNOR_TOOL_"));
}

#[test]
fn an_install_without_the_verification_fails() {
    // The shape of every release, qualification and freshness job before
    // the check covered them: install, then build.
    let text = VERIFIED.replace(INSTALLED, "");
    let found = problem(&files(".github/workflows/release.yml", &text));
    assert!(
        found.contains("rust_install_unverified:.github/workflows/release.yml:build:Build:"),
        "{found}"
    );
}

#[test]
fn verifier_call_requires_exact_path_and_no_caller_inputs() {
    let wrong_path = VERIFIED.replace(
        "./.github/actions/velnor-verify-rust-manifest # zizmor: ignore[self-repository]",
        "./.github/actions/velnor-verify-rust-manifest-extra # zizmor: ignore[self-repository]",
    );
    let found = problem(&files(".github/workflows/release.yml", &wrong_path));
    assert!(found.contains("noncanonical verifier call"), "{found}");

    let with_input = VERIFIED.replace(
        INSTALLED,
        "      - name: Verify Rust channel manifest\n        uses: ./.github/actions/velnor-verify-rust-manifest # zizmor: ignore[self-repository]\n        with:\n          phase: restored\n",
    );
    let found = problem(&files(".github/workflows/release.yml", &with_input));
    assert!(found.contains("noncanonical verifier call"), "{found}");
}

#[test]
fn missing_or_changed_verifier_composite_blocks_audit() {
    let mut missing = files(".github/workflows/release.yml", VERIFIED);
    missing.retain(|file| file.path != ".github/actions/velnor-verify-jactionlint/action.yml");
    let found = problem(&missing);
    assert!(
        found.contains("missing or duplicate verifier action definition"),
        "{found}"
    );

    let mut unbound = files(".github/workflows/release.yml", VERIFIED);
    let action = unbound
        .iter_mut()
        .find(|file| file.path == ".github/actions/velnor-verify-rust-manifest/action.yml")
        .expect("canonical manifest verifier");
    let original = action.bytes.clone();
    action.bytes = action.bytes.replacen("1.99.1", "1.99.0", 1);
    assert_ne!(
        action.bytes, original,
        "mutation must change the pinned version"
    );
    let found = problem(&unbound);
    assert!(
        found.contains("verifier action body, shell, environment, or marker changed"),
        "missing pinned Rust version binding must block generation: {found}"
    );

    let mut wrong_binding = files(".github/workflows/release.yml", VERIFIED);
    let action = wrong_binding
        .iter_mut()
        .find(|file| file.path == ".github/actions/velnor-verify-jactionlint/action.yml")
        .expect("canonical jactionlint verifier");
    let original = action.bytes.clone();
    action.bytes = action.bytes.replacen("2.3.0", "2.3.1", 1);
    assert_ne!(
        action.bytes, original,
        "mutation must change the pinned version"
    );
    let found = problem(&wrong_binding);
    assert!(
        found.contains("verifier action body, shell, environment, or marker changed"),
        "wrong pinned jactionlint version binding must block generation: {found}"
    );

    let mut changed = files(".github/workflows/release.yml", VERIFIED);
    let action = changed
        .iter_mut()
        .find(|file| file.path == ".github/actions/velnor-verify-rust-manifest/action.yml")
        .expect("canonical manifest verifier");
    action.bytes = action
        .bytes
        .replace("unset ACTIONS_ID_TOKEN_REQUEST_TOKEN", "true");
    let found = problem(&changed);
    assert!(
        found.contains("verifier action body, shell, environment, or marker changed"),
        "{found}"
    );

    let mut changed = files(".github/workflows/release.yml", VERIFIED);
    let action = changed
        .iter_mut()
        .find(|file| file.path == ".github/actions/velnor-verify-rust-manifest/action.yml")
        .expect("canonical manifest verifier");
    action.bytes = action.bytes.replace(
        "${{ runner.temp }}/velnor/rustup",
        "${{ runner.temp }}/velnor/other-rustup",
    );
    let found = problem(&changed);
    assert!(
        found.contains("verifier action body, shell, environment, or marker changed"),
        "{found}"
    );

    let mut changed = files(".github/workflows/release.yml", VERIFIED);
    let action = changed
        .iter_mut()
        .find(|file| file.path == ".github/actions/velnor-verify-rust-manifest/action.yml")
        .expect("canonical manifest verifier");
    action.bytes = action.bytes.replace("cmp -s", "true # cmp -s");
    let found = problem(&changed);
    assert!(
        found.contains("verifier action body, shell, environment, or marker changed"),
        "{found}"
    );
}

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

#[test]
fn standalone_source_prep_verifier_accepts_only_its_shared_exact_job_environment() {
    let actions = verifiers();
    let template = actions.inline_rust_templates["Verify Rust channel manifest"]
        .last()
        .expect("canonical producer-home Rust verifier");
    assert_eq!(template.env["MISE_AUTO_INSTALL"], "false");
    assert_eq!(template.env["MISE_NO_CONFIG"], "1");
    assert_eq!(template.env["CARGO_HOME"], template.env["MISE_CARGO_HOME"]);
    assert_eq!(
        template.env["RUSTUP_HOME"],
        template.env["MISE_RUSTUP_HOME"]
    );
    assert_eq!(template.env["RUSTUP_TOOLCHAIN"], "1.99.1");

    let canonical = format!(
        "jobs:\n  producer:\n    steps:\n      - name: Install pinned Rust\n        run: mise install rust@1.99.1\n{}",
        inline_step("Verify Rust channel manifest", template),
    );
    assert_eq!(
        problem(&files(".github/workflows/binary-release.yml", &canonical)),
        ""
    );

    let reordered = format!(
        "jobs:\n  producer:\n    steps:\n      - name: Install pinned Rust\n        run: mise install rust@1.99.1\n{}",
        inline_step_with_env(
            "Verify Rust channel manifest",
            template,
            template.env.iter().rev(),
        ),
    );
    assert_eq!(
        problem(&files(".github/workflows/binary-release.yml", &reordered)),
        "",
        "environment declaration order does not change the exact mapping"
    );

    let changed_flag = canonical.replace(
        &format!(
            "          MISE_NO_CONFIG: {}",
            velnor_actions_workflow_renderer::yaml::quote_scalar(&template.env["MISE_NO_CONFIG"])
        ),
        "          MISE_NO_CONFIG: \"0\"",
    );
    assert_ne!(changed_flag, canonical);
    assert!(
        problem(&files(
            ".github/workflows/binary-release.yml",
            &changed_flag
        ))
        .contains("noncanonical verifier call or body"),
        "changed project-config isolation must fail closed"
    );

    let changed_home = canonical.replace(
        &format!(
            "          RUSTUP_HOME: {}",
            velnor_actions_workflow_renderer::yaml::quote_scalar(&template.env["RUSTUP_HOME"])
        ),
        "          RUSTUP_HOME: /tmp/untrusted-rustup",
    );
    assert_ne!(changed_home, canonical);
    assert!(
        problem(&files(
            ".github/workflows/binary-release.yml",
            &changed_home
        ))
        .contains("noncanonical verifier call or body"),
        "changed producer home must fail closed"
    );

    let no_config = format!(
        "          MISE_NO_CONFIG: {}\n",
        velnor_actions_workflow_renderer::yaml::quote_scalar(&template.env["MISE_NO_CONFIG"])
    );
    let missing_entry = canonical.replace(&no_config, "");
    assert_ne!(missing_entry, canonical);
    assert!(
        problem(&files(
            ".github/workflows/binary-release.yml",
            &missing_entry
        ))
        .contains("noncanonical verifier call or body"),
        "missing project-config isolation must fail closed"
    );

    let duplicate_entry = canonical.replace(&no_config, &format!("{no_config}{no_config}"));
    assert_ne!(duplicate_entry, canonical);
    assert!(
        problem(&files(
            ".github/workflows/binary-release.yml",
            &duplicate_entry
        ))
        .contains("noncanonical verifier call or body"),
        "duplicate environment keys must fail closed"
    );

    let extra_entry = canonical.replace(
        "        env:\n",
        "        env:\n          UNEXPECTED: \"1\"\n",
    );
    assert_ne!(extra_entry, canonical);
    assert!(
        problem(&files(".github/workflows/binary-release.yml", &extra_entry))
            .contains("noncanonical verifier call or body"),
        "additional caller environment must fail closed"
    );
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
                        jactionlint_template.shell.unwrap()
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
