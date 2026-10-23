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

#[path = "rust_install_audit_verifier_tests.rs"]
mod verifier_tests;
