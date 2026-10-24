use std::collections::BTreeMap;

use velnor_actions_contract::workflow::{RUST_VERIFIER_USES, Step, StepKind, StepRole};

use super::{
    VERIFY_JACTIONLINT_USES, render_verifier_composite, verifier_composite_document,
    verifier_shell_argv, verifier_shell_run_scalar, verifier_shell_step,
    verifier_shell_step_to_yaml,
};
use crate::VERIFY_VERIFIER_SHELL;
use crate::yaml::Yaml;

fn rust_step(uses: &str) -> Step {
    let restored = uses == RUST_VERIFIER_USES[0];
    let name = if restored {
        "Verify restored Rust toolchain"
    } else {
        crate::VERIFY_RUST_TOOLCHAIN_STEP
    };
    let mut step = crate::steps::shell_step(
        name,
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            "set -eu; echo verified; echo rust@${VELNOR_TOOL_RUST}".to_owned(),
        ],
        BTreeMap::from([
            (
                "RUSTUP_HOME".to_owned(),
                "${{ runner.temp }}/velnor/rustup".to_owned(),
            ),
            (
                "CARGO_HOME".to_owned(),
                "${{ runner.temp }}/velnor/cargo".to_owned(),
            ),
        ]),
    )
    .expect("canonical scrubbed shell");
    step.role = Some(StepRole::VerifyRustManifest);
    step
}

fn pinned_rust_step(uses: &str) -> Step {
    let mut step = rust_step(uses);
    if let StepKind::Shell { run, .. } = &mut step.kind {
        run[2] = run[2].replace("${VELNOR_TOOL_RUST}", "1.88.0");
    }
    step
}

fn mapping(node: &Yaml) -> &[(String, Yaml)] {
    let Yaml::Map(entries) = node else {
        panic!("expected YAML mapping, got {node:?}");
    };
    entries
}

fn field<'a>(node: &'a Yaml, key: &str) -> &'a Yaml {
    mapping(node)
        .iter()
        .find_map(|(candidate, value)| (candidate == key).then_some(value))
        .unwrap_or_else(|| panic!("missing YAML field {key:?} in {node:?}"))
}

fn scalar(node: &Yaml) -> &str {
    let Yaml::Str(value) = node else {
        match node {
            Yaml::Quoted(value)
            | Yaml::Annotated { value, .. }
            | Yaml::AnchoredScalar { value, .. } => return value,
            _ => panic!("expected YAML string, got {node:?}"),
        }
    };
    value
}

fn scalar_map(node: &Yaml) -> BTreeMap<String, String> {
    let mut result = BTreeMap::new();
    for (key, value) in mapping(node) {
        assert!(
            result
                .insert(key.clone(), scalar(value).to_owned())
                .is_none(),
            "duplicate YAML key {key:?}"
        );
    }
    result
}

#[test]
fn verifier_composite_keeps_scrubbed_shell_body_homes_and_trusted_shell() {
    let step = pinned_rust_step(RUST_VERIFIER_USES[0]);
    let file =
        render_verifier_composite(RUST_VERIFIER_USES[0], &step, "0.1.40").expect("fixed composite");
    assert_eq!(
        file.path,
        ".github/actions/velnor-verify-restored-rust/action.yml"
    );
    assert!(
        file.bytes
            .contains(&format!("shell: {VERIFY_VERIFIER_SHELL}"))
    );
    assert!(!file.bytes.contains("Resolve project tool versions"));
    assert!(file.bytes.contains("RUSTUP_HOME:"));
    assert!(file.bytes.contains("${{ runner.temp }}/velnor/rustup"));
    assert!(file.bytes.contains("CARGO_HOME:"));
    assert!(file.bytes.contains("${{ runner.temp }}/velnor/cargo"));
    let document = verifier_composite_document("Velnor restored Rust verifier", &step)
        .expect("structured verifier composite");
    let runs = field(&document, "runs");
    assert_eq!(scalar(field(runs, "using")), "composite");
    let Yaml::Seq(steps) = field(runs, "steps") else {
        panic!("composite steps must be a sequence");
    };
    assert_eq!(steps.len(), 1, "literal catalog pins need no resolver");
    let verifier = &steps[0];
    assert_eq!(
        scalar(field(verifier, "name")),
        "Verify restored Rust toolchain"
    );
    assert_eq!(scalar(field(verifier, "shell")), VERIFY_VERIFIER_SHELL);
    assert!(mapping(verifier).iter().all(|(key, _)| key != "uses"));
    let mut expected_env = crate::toolchain_env::STEP_CREDENTIAL_DENYLIST
        .into_iter()
        .chain(crate::toolchain_env::STEP_ENDPOINT_DENYLIST)
        .map(|key| (key.to_owned(), String::new()))
        .collect::<std::collections::BTreeMap<_, _>>();
    expected_env.insert(
        "RUSTUP_HOME".to_owned(),
        "${{ runner.temp }}/velnor/rustup".to_owned(),
    );
    expected_env.insert(
        "CARGO_HOME".to_owned(),
        "${{ runner.temp }}/velnor/cargo".to_owned(),
    );
    expected_env.insert("PATH".to_owned(), crate::VERIFY_PATH.to_owned());
    assert_eq!(scalar_map(field(verifier, "env")), expected_env);
    let run = scalar(field(verifier, "run"));
    assert!(!run.contains("${VELNOR_TOOL_RUST}"), "{run}");
    assert!(
        run.contains("set -eu; echo verified; echo rust@1.88.0"),
        "canonical payload and literal version remain in the verifier step: {run}"
    );
    let prelude = format!(
        "unset {};",
        crate::toolchain_env::CREDENTIAL_UNSET_VARS.join(" ")
    );
    assert!(run.contains(&prelude));
}

#[test]
fn dynamic_verifier_composite_self_binds_its_project_tool_reference() {
    let step = rust_step(RUST_VERIFIER_USES[1]);
    let file = render_verifier_composite(RUST_VERIFIER_USES[1], &step, "0.1.40")
        .expect("dynamic verifier composite");
    let document = verifier_composite_document("Velnor Rust channel verifier", &step)
        .expect("structured dynamic verifier composite");
    let runs = field(&document, "runs");
    let Yaml::Seq(steps) = field(runs, "steps") else {
        panic!("composite steps must be a sequence");
    };
    assert_eq!(steps.len(), 2, "one resolver precedes the verifier");
    let resolver = &steps[0];
    assert_eq!(
        scalar(field(resolver, "name")),
        "Resolve project tool versions"
    );
    assert_eq!(scalar(field(resolver, "shell")), VERIFY_VERIFIER_SHELL);
    let resolver_run = scalar(field(resolver, "run"));
    assert!(resolver_run.contains("velnor_bind_selected_mise"));
    assert!(resolver_run.contains("function_env_scan_complete=0"));
    assert!(resolver_run.contains("unset ACTIONS_ID_TOKEN_REQUEST_TOKEN"));
    let verifier = &steps[1];
    assert_eq!(scalar(field(verifier, "shell")), VERIFY_VERIFIER_SHELL);
    assert!(scalar(field(verifier, "run")).contains("${VELNOR_TOOL_RUST}"));
    assert_eq!(
        file.bytes.matches("Resolve project tool versions").count(),
        1
    );

    let mut wrong_reference = rust_step(RUST_VERIFIER_USES[1]);
    if let StepKind::Shell { run, .. } = &mut wrong_reference.kind {
        run[2] = run[2].replace("VELNOR_TOOL_RUST", "VELNOR_TOOL_JACTIONLINT");
    }
    let error = render_verifier_composite(RUST_VERIFIER_USES[1], &wrong_reference, "0.1.40")
        .expect_err("the verifier cannot bind a different tool");
    assert!(
        error
            .to_string()
            .contains("verifier_composite_tool_binding"),
        "{error}"
    );
}

#[test]
fn verifier_composite_rejects_unregistered_paths_and_mutated_step_identity() {
    let canonical = rust_step(RUST_VERIFIER_USES[1]);
    for uses in [
        "./.github/actions/velnor-verify-rust-manifest/",
        "./.github/actions/velnor-verify-rust-manifest@deadbeef",
        "./.github/actions/../velnor-verify-rust-manifest",
        "./.github/actions/velnor-verify-rust-manifest-extra",
    ] {
        assert!(
            render_verifier_composite(uses, &canonical, "0.1.40").is_err(),
            "unexpected accepted path {uses}"
        );
    }
    assert!(render_verifier_composite(VERIFY_JACTIONLINT_USES, &canonical, "0.1.40").is_err());
    let mut changed_name = canonical.clone();
    changed_name.name = "anything".to_owned();
    let renamed = render_verifier_composite(RUST_VERIFIER_USES[1], &changed_name, "0.1.40")
        .expect("display names do not control verifier authority");
    assert!(renamed.bytes.contains("name: anything"));
    let mut changed_role = canonical;
    changed_role.role = None;
    assert!(render_verifier_composite(RUST_VERIFIER_USES[1], &changed_role, "0.1.40").is_err());
}

#[test]
fn verifier_renderer_reapplies_scrub_and_rejects_forged_step_shapes() {
    let canonical = rust_step(RUST_VERIFIER_USES[1]);
    assert!(verifier_shell_argv(&canonical).is_ok());

    let mut missing_scrub = canonical.clone();
    if let StepKind::Shell { env, .. } = &mut missing_scrub.kind {
        env.remove("GITHUB_TOKEN");
    }
    let rendered = verifier_shell_step_to_yaml(&missing_scrub)
        .expect("renderer reapplies the complete credential boundary");
    let emitted_env = scalar_map(field(&rendered, "env"));
    assert_eq!(
        emitted_env.get("GITHUB_TOKEN").map(String::as_str),
        Some("")
    );
    for key in crate::toolchain_env::STEP_CREDENTIAL_DENYLIST
        .into_iter()
        .chain(crate::toolchain_env::STEP_ENDPOINT_DENYLIST)
    {
        assert_eq!(emitted_env.get(key).map(String::as_str), Some(""), "{key}");
    }
    assert_eq!(
        emitted_env.len(),
        14,
        "11 scrub values, PATH, and two homes"
    );
    assert_eq!(
        emitted_env.get("PATH").map(String::as_str),
        Some(crate::VERIFY_PATH)
    );
    assert!(emitted_env.contains_key("RUSTUP_HOME"));
    assert!(emitted_env.contains_key("CARGO_HOME"));

    let mut changed_identity = canonical.clone();
    changed_identity.name = "Verify something else".to_owned();
    assert!(verifier_shell_argv(&changed_identity).is_ok());

    let mut roleless = canonical.clone();
    roleless.role = None;
    assert!(verifier_shell_argv(&roleless).is_err());

    let mut untrusted_path = canonical;
    if let StepKind::Shell { env, .. } = &mut untrusted_path.kind {
        env.insert("PATH".to_owned(), "/tmp/poison".to_owned());
    }
    let rendered = verifier_shell_step_to_yaml(&untrusted_path)
        .expect("renderer replaces an input PATH with the trusted verifier PATH");
    assert_eq!(
        scalar_map(field(&rendered, "env"))
            .get("PATH")
            .map(String::as_str),
        Some(crate::VERIFY_PATH)
    );
}

#[test]
fn canonical_verifier_step_requires_one_full_credential_prelude() {
    let prelude = crate::toolchain_env::credential_unset_prelude();
    let canonical = verifier_shell_step(
        StepRole::VerifyRustManifest,
        "Verify Rust manifest",
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            format!("{prelude} set -eu; echo digest-verified"),
        ],
        BTreeMap::new(),
    )
    .expect("complete canonical prelude");
    let StepKind::Shell { run, env } = &canonical.kind else {
        panic!("canonical verifier shell");
    };
    assert_eq!(run[2].matches(&prelude).count(), 1, "{}", run[2]);
    assert_eq!(env.get("GITHUB_TOKEN").map(String::as_str), Some(""));

    let argv = verifier_shell_argv(&canonical).expect("trusted fixed-shell argv");
    let expected_prefix = [
        "/usr/bin/env".to_owned(),
        format!("PATH={}", crate::VERIFY_PATH),
        "/bin/bash".to_owned(),
        "--noprofile".to_owned(),
        "--norc".to_owned(),
        "-p".to_owned(),
        "-e".to_owned(),
        "-o".to_owned(),
        "pipefail".to_owned(),
        "-c".to_owned(),
    ];
    assert_eq!(
        &argv[..expected_prefix.len()],
        expected_prefix.as_slice(),
        "standalone argv binds the trusted PATH before the privileged Bash"
    );
    let run = verifier_shell_run_scalar(&canonical).expect("actual rendered run scalar");
    assert_eq!(argv.last(), Some(&run));
    assert_eq!(run.matches(&prelude).count(), 1, "{run}");
    let emitted = verifier_shell_step_to_yaml(&canonical).expect("rendered verifier step");
    assert_eq!(
        scalar_map(field(&emitted, "env"))
            .get("PATH")
            .map(String::as_str),
        Some(crate::VERIFY_PATH),
        "trusted PATH is a step environment value"
    );

    for script in ["set -eu".to_owned(), format!("{prelude} {prelude} set -eu")] {
        assert!(
            verifier_shell_step(
                StepRole::VerifyRustManifest,
                "Verify Rust manifest",
                vec!["sh".to_owned(), "-c".to_owned(), script],
                BTreeMap::new(),
            )
            .is_err(),
            "partial or duplicate prelude must fail closed"
        );
    }
}
