use std::collections::BTreeMap;

use velnor_actions_contract::workflow::{RUST_VERIFIER_USES, Step, StepKind, StepRole};

use super::{
    VERIFY_JACTIONLINT_USES, render_verifier_composite, verifier_composite_document,
    verifier_shell_argv, verifier_shell_step,
};
use crate::VERIFY_SHELL;
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
        panic!("expected YAML string, got {node:?}");
    };
    value
}

#[test]
fn verifier_composite_keeps_scrubbed_shell_body_homes_and_trusted_shell() {
    let step = rust_step(RUST_VERIFIER_USES[0]);
    let file =
        render_verifier_composite(RUST_VERIFIER_USES[0], &step, "0.1.40").expect("fixed composite");
    assert_eq!(
        file.path,
        ".github/actions/velnor-verify-restored-rust/action.yml"
    );
    assert!(file.bytes.contains(&format!("shell: {VERIFY_SHELL}")));
    assert!(file.bytes.contains("RUSTUP_HOME:"));
    assert!(file.bytes.contains("${{ runner.temp }}/velnor/rustup"));
    assert!(file.bytes.contains("CARGO_HOME:"));
    assert!(file.bytes.contains("${{ runner.temp }}/velnor/cargo"));
    let document =
        verifier_composite_document("Velnor restored Rust verifier", "VELNOR_TOOL_RUST", &step)
            .expect("structured verifier composite");
    let runs = field(&document, "runs");
    assert_eq!(scalar(field(runs, "using")), "composite");
    let Yaml::Seq(steps) = field(runs, "steps") else {
        panic!("composite steps must be a sequence");
    };
    assert_eq!(
        steps.len(),
        1,
        "there is no resolver or extra executable step"
    );
    let verifier = &steps[0];
    assert_eq!(
        scalar(field(verifier, "name")),
        "Verify restored Rust toolchain"
    );
    assert_eq!(scalar(field(verifier, "shell")), VERIFY_SHELL);
    assert!(mapping(verifier).iter().all(|(key, _)| key != "uses"));
    let env = mapping(field(verifier, "env"));
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
    assert_eq!(
        env.len(),
        expected_env.len(),
        "no duplicate or extra env keys"
    );
    for (key, expected) in expected_env {
        let values = env
            .iter()
            .filter_map(|(actual_key, value)| (actual_key == &key).then_some(value))
            .collect::<Vec<_>>();
        assert_eq!(values.len(), 1, "exactly one {key} binding");
        assert_eq!(scalar(values[0]), expected, "exact {key} value");
    }
    let run = scalar(field(verifier, "run"));
    assert!(
        run.contains(&format!("PATH={}", crate::VERIFY_PATH)),
        "{run}"
    );
    assert_eq!(run.matches("${VELNOR_TOOL_RUST}").count(), 1, "{run}");
    assert!(
        run.contains(r#"set -eu; echo verified; echo rust@'"${VELNOR_TOOL_RUST}"'"#),
        "canonical payload and caller binding remain in the verifier step: {run}"
    );
    assert!(!run.contains("Resolve project tool versions"));
    let prelude = format!(
        "unset {};",
        crate::toolchain_env::CREDENTIAL_UNSET_VARS.join(" ")
    );
    assert!(run.contains(&prelude));
}

#[test]
fn jactionlint_composite_uses_the_trusted_shell_and_scrubbed_script() {
    let step = crate::steps::shell_step(
        "Verify jactionlint",
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            "set -eu; echo digest-verified; echo jactionlint@${VELNOR_TOOL_JACTIONLINT}".to_owned(),
        ],
        BTreeMap::new(),
    )
    .expect("canonical scrubbed shell");
    let file = render_verifier_composite(VERIFY_JACTIONLINT_USES, &step, "0.1.40")
        .expect("fixed jactionlint composite");
    assert_eq!(
        file.path,
        ".github/actions/velnor-verify-jactionlint/action.yml"
    );
    assert!(file.bytes.contains(&format!("shell: {VERIFY_SHELL}")));
    assert!(file.bytes.contains("GITHUB_TOKEN: \"\""));
    let prelude = format!(
        "unset {};",
        crate::toolchain_env::CREDENTIAL_UNSET_VARS.join(" ")
    );
    assert!(file.bytes.contains(&prelude));
    assert!(file.bytes.contains("digest-verified"));
    assert!(!file.bytes.contains("Resolve project tool versions"));
    assert!(file.bytes.contains(&format!("PATH={}", crate::VERIFY_PATH)));
    assert!(file.bytes.contains("VELNOR_TOOL_JACTIONLINT"));
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
    assert!(render_verifier_composite(RUST_VERIFIER_USES[1], &changed_name, "0.1.40").is_err());
    let mut changed_role = canonical;
    changed_role.role = None;
    assert!(render_verifier_composite(RUST_VERIFIER_USES[1], &changed_role, "0.1.40").is_err());
}

#[test]
fn shared_verifier_argv_fails_closed_on_forged_step_shapes() {
    let canonical = rust_step(RUST_VERIFIER_USES[1]);
    assert!(verifier_shell_argv(&canonical).is_ok());

    let mut missing_scrub = canonical.clone();
    if let StepKind::Shell { env, .. } = &mut missing_scrub.kind {
        env.remove("GITHUB_TOKEN");
    }
    assert!(verifier_shell_argv(&missing_scrub).is_err());

    let mut changed_identity = canonical.clone();
    changed_identity.name = "Verify something else".to_owned();
    assert!(verifier_shell_argv(&changed_identity).is_err());

    let mut untrusted_path = canonical;
    if let StepKind::Shell { env, .. } = &mut untrusted_path.kind {
        env.insert("PATH".to_owned(), "/tmp/poison".to_owned());
    }
    assert!(verifier_shell_argv(&untrusted_path).is_err());
}

#[test]
fn canonical_verifier_step_requires_one_full_credential_prelude() {
    let prelude = crate::toolchain_env::credential_unset_prelude();
    let canonical = verifier_shell_step(
        "Verify jactionlint",
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
    assert_eq!(argv[2].matches(&prelude).count(), 1, "{}", argv[2]);
    let path = argv[2].find("PATH=").expect("trusted PATH assignment");
    let payload = argv[2].find("set -eu").expect("canonical payload");
    assert!(path < payload, "{}", argv[2]);

    for script in ["set -eu".to_owned(), format!("{prelude} {prelude} set -eu")] {
        assert!(
            verifier_shell_step(
                "Verify jactionlint",
                vec!["sh".to_owned(), "-c".to_owned(), script],
                BTreeMap::new(),
            )
            .is_err(),
            "partial or duplicate prelude must fail closed"
        );
    }
}

#[path = "verifier_composite_exec_tests.rs"]
mod exec_tests;

#[cfg(unix)]
#[test]
fn emitted_verifiers_replace_poisoned_path_and_remove_all_credentials() {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "velnor-verifier-run-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&root).expect("temporary test root");
    let (poison, log) = exec_tests::poisoned_helper_dir(&root);
    let digest = if std::path::Path::new("/usr/bin/sha256sum").exists() {
        "sha256sum"
    } else {
        "shasum"
    };
    for (name, role, uses, reference) in [
        (
            "Verify restored Rust toolchain",
            Some(StepRole::VerifyRustManifest),
            RUST_VERIFIER_USES[0],
            "VELNOR_TOOL_RUST",
        ),
        (
            crate::VERIFY_RUST_TOOLCHAIN_STEP,
            Some(StepRole::VerifyRustManifest),
            RUST_VERIFIER_USES[1],
            "VELNOR_TOOL_RUST",
        ),
        (
            "Verify jactionlint",
            None,
            VERIFY_JACTIONLINT_USES,
            "VELNOR_TOOL_JACTIONLINT",
        ),
    ] {
        exec_tests::execute_emitted_verifier(
            &root,
            &poison,
            name,
            role,
            uses,
            reference,
            &exec_tests::verifier_body(reference, digest),
        );
    }
    assert!(!log.exists(), "a helper resolved through poisoned PATH");
    fs::remove_dir_all(root).expect("remove test tree");
}
