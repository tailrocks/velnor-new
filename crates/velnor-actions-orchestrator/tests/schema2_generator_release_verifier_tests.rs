use super::{VERIFY_PATH, assert_exact_empty_verifier_credentials};

const QUALIFIER: &str = include_str!("snapshots/generator-release-qualify-linux.yml");
const QUALIFIER_WITH_RESOLVER: &str =
    include_str!("../../../.github/actions/generator-release-qualify-linux/action.yml");
const RESOLVER: &str = "Resolve project tool versions";
const RUST: &str = "Verify Rust channel manifest";
const JACTIONLINT: &str = "Verify jactionlint";
const SYNTHETIC_VERIFIERS: &[&str] = &[RUST, JACTIONLINT];
const REPOSITORY_VERIFIERS: &[&str] = &[RESOLVER, RUST, JACTIONLINT];

fn changed_verifier(body: &str, step_name: &str, from: &str, to: &str) -> String {
    super::policy::replace_named_step(body, step_name, from, to)
        .expect("named verifier tamper must change its fixture")
}

fn assert_rejected_tamper(original: &str, tampered: &str, verifier_steps: &[&str], label: &str) {
    assert_ne!(tampered, original, "tamper fixture is a no-op: {label}");
    assert!(
        assert_exact_empty_verifier_credentials(tampered, verifier_steps).is_err(),
        "tampered verifier credentials were accepted: {label}"
    );
}

#[test]
fn generated_qualifier_verifiers_have_exact_empty_credentials() {
    assert_eq!(
        QUALIFIER
            .matches("- name: Resolve project tool versions")
            .count(),
        0
    );
    assert!(assert_exact_empty_verifier_credentials(QUALIFIER, SYNTHETIC_VERIFIERS).is_ok());
}

#[test]
fn resolver_is_required_unique_and_uses_the_same_boundary() {
    assert!(
        assert_exact_empty_verifier_credentials(QUALIFIER_WITH_RESOLVER, REPOSITORY_VERIFIERS)
            .is_ok()
    );
    let missing = super::policy::remove_named_step(QUALIFIER_WITH_RESOLVER, RESOLVER)
        .expect("resolver fixture has one named step");
    assert_rejected_tamper(
        QUALIFIER_WITH_RESOLVER,
        &missing,
        REPOSITORY_VERIFIERS,
        "missing resolver",
    );
    let duplicate = super::policy::duplicate_named_step(QUALIFIER_WITH_RESOLVER, RESOLVER)
        .expect("resolver fixture has one named step");
    assert_rejected_tamper(
        QUALIFIER_WITH_RESOLVER,
        &duplicate,
        REPOSITORY_VERIFIERS,
        "duplicate resolver",
    );
    assert_rejected_tamper(
        QUALIFIER_WITH_RESOLVER,
        &changed_verifier(
            QUALIFIER_WITH_RESOLVER,
            RESOLVER,
            "GH_TOKEN: \"\"",
            "GH_TOKEN: \"live\"",
        ),
        REPOSITORY_VERIFIERS,
        "resolver live token",
    );
    assert_rejected_tamper(
        QUALIFIER_WITH_RESOLVER,
        &changed_verifier(
            QUALIFIER_WITH_RESOLVER,
            RESOLVER,
            VERIFY_PATH,
            "/tmp/poison",
        ),
        REPOSITORY_VERIFIERS,
        "resolver path",
    );
    assert_rejected_tamper(
        QUALIFIER_WITH_RESOLVER,
        &changed_verifier(
            QUALIFIER_WITH_RESOLVER,
            RESOLVER,
            "/bin/bash --noprofile --norc -p -e -o pipefail {0}",
            "/bin/bash --noprofile --norc -e -o pipefail {0}",
        ),
        REPOSITORY_VERIFIERS,
        "resolver startup shell",
    );
}

#[test]
fn resolver_annotation_requires_a_complete_quoted_scalar_and_exact_comment() {
    let malformed_comment = mutate_resolver_run(QUALIFIER_WITH_RESOLVER, |run| {
        run.replace(
            " # zizmor: ignore[github-env]",
            " # zizmor: ignore[github-env] extra",
        )
    });
    assert_ne!(malformed_comment, QUALIFIER_WITH_RESOLVER);
    assert!(
        assert_exact_empty_verifier_credentials(&malformed_comment, REPOSITORY_VERIFIERS).is_err()
    );

    let malformed_quote = mutate_resolver_run(QUALIFIER_WITH_RESOLVER, |run| {
        run.replacen(
            "\" # zizmor: ignore[github-env]",
            " # zizmor: ignore[github-env]",
            1,
        )
    });
    assert_ne!(malformed_quote, QUALIFIER_WITH_RESOLVER);
    assert!(
        assert_exact_empty_verifier_credentials(&malformed_quote, REPOSITORY_VERIFIERS).is_err()
    );
}

fn mutate_resolver_run(body: &str, mutate: impl FnOnce(&str) -> String) -> String {
    let mut lines = body.lines().map(str::to_owned).collect::<Vec<_>>();
    let resolver = lines
        .iter()
        .position(|line| line.trim() == "- name: Resolve project tool versions")
        .expect("resolver fixture step");
    let run = lines[resolver..]
        .iter()
        .position(|line| line.trim_start().starts_with("run: "))
        .map(|offset| resolver + offset)
        .expect("resolver fixture run scalar");
    let (prefix, value) = lines[run].split_once("run: ").expect("run scalar prefix");
    lines[run] = format!("{prefix}run: {}", mutate(value));
    lines.join("\n") + "\n"
}

#[test]
fn jactionlint_verifier_is_checked_independently() {
    assert_rejected_tamper(
        QUALIFIER,
        &changed_verifier(
            QUALIFIER,
            JACTIONLINT,
            "GH_TOKEN: \"\"",
            "GH_TOKEN: \"live\"",
        ),
        SYNTHETIC_VERIFIERS,
        "jactionlint live token",
    );
    assert_rejected_tamper(
        QUALIFIER,
        &changed_verifier(QUALIFIER, JACTIONLINT, "        GH_TOKEN: \"\"\n", ""),
        SYNTHETIC_VERIFIERS,
        "jactionlint missing token",
    );
    assert_rejected_tamper(
        QUALIFIER,
        &changed_verifier(
            QUALIFIER,
            JACTIONLINT,
            "        GH_TOKEN: \"\"\n",
            "        GH_TOKEN: \"\"\n        GH_TOKEN: \"\"\n",
        ),
        SYNTHETIC_VERIFIERS,
        "jactionlint duplicate token",
    );
    assert_rejected_tamper(
        QUALIFIER,
        &changed_verifier(
            QUALIFIER,
            JACTIONLINT,
            "/bin/bash --noprofile --norc -p -e -o pipefail {0}",
            "/bin/bash --noprofile --norc -e -o pipefail {0}",
        ),
        SYNTHETIC_VERIFIERS,
        "jactionlint outer shell",
    );
    assert_rejected_tamper(
        QUALIFIER,
        &changed_verifier(QUALIFIER, JACTIONLINT, "-u BASH_ENV -u ENV", "-u ENV"),
        SYNTHETIC_VERIFIERS,
        "jactionlint inner boundary",
    );
}

fn reject_synthetic_tamper(label: &str, tampered: &str) {
    assert_rejected_tamper(QUALIFIER, tampered, SYNTHETIC_VERIFIERS, label);
}

#[test]
fn rejects_live_duplicate_missing_and_extra_verifier_credentials() {
    reject_synthetic_tamper(
        "live",
        &changed_verifier(QUALIFIER, RUST, "GH_TOKEN: \"\"", "GH_TOKEN: \"live\""),
    );
    reject_synthetic_tamper(
        "expression",
        &changed_verifier(
            QUALIFIER,
            RUST,
            "GH_TOKEN: \"\"",
            "GH_TOKEN: \"${{ github.token }}\"",
        ),
    );
    reject_synthetic_tamper(
        "duplicate",
        &changed_verifier(
            QUALIFIER,
            RUST,
            "        GH_TOKEN: \"\"\n",
            "        GH_TOKEN: \"\"\n        GH_TOKEN: \"\"\n",
        ),
    );
    reject_synthetic_tamper(
        "missing",
        &changed_verifier(QUALIFIER, RUST, "        GH_TOKEN: \"\"\n", ""),
    );
    reject_synthetic_tamper(
        "PATH",
        &changed_verifier(QUALIFIER, RUST, VERIFY_PATH, "/tmp/poison"),
    );
    reject_synthetic_tamper(
        "extra",
        &changed_verifier(
            QUALIFIER,
            RUST,
            "        GH_TOKEN: \"\"\n",
            "        GH_TOKEN: \"\"\n        EXTRA_TOKEN: \"\"\n",
        ),
    );
}

#[test]
fn rejects_workflow_level_and_inline_credentials() {
    reject_synthetic_tamper(
        "setup",
        &QUALIFIER.replacen(
            "    - name: Setup Mise\n",
            "    - name: Setup Mise\n      env:\n        GH_TOKEN: \"\"\n",
            1,
        ),
    );
    reject_synthetic_tamper(
        "workflow",
        &QUALIFIER.replacen("runs:\n", "runs:\n  env:\n    GH_TOKEN: \"\"\n", 1),
    );
    reject_synthetic_tamper(
        "inline",
        &QUALIFIER.replacen(
            "    - name: Setup Mise\n",
            "    - name: Setup Mise\n      env: { GH_TOKEN: \"\" }\n",
            1,
        ),
    );
}

#[test]
fn rejects_verifier_startup_and_runtime_boundary_tampering() {
    reject_synthetic_tamper(
        "shell -p",
        &changed_verifier(
            QUALIFIER,
            RUST,
            "/bin/bash --noprofile --norc -p -e -o pipefail {0}",
            "/bin/bash --noprofile --norc -e -o pipefail {0}",
        ),
    );
    reject_synthetic_tamper(
        "inner -p",
        &changed_verifier(
            QUALIFIER,
            RUST,
            "/bin/bash --noprofile --norc -p -e -o pipefail -c",
            "/bin/bash --noprofile --norc -e -o pipefail -c",
        ),
    );
    reject_synthetic_tamper(
        "env boundary",
        &changed_verifier(QUALIFIER, RUST, "exec /usr/bin/env", "exec /usr/bin/true"),
    );
    reject_synthetic_tamper(
        "BASH_ENV",
        &changed_verifier(QUALIFIER, RUST, "-u BASH_ENV -u ENV", "-u ENV"),
    );
    reject_synthetic_tamper(
        "ENV",
        &changed_verifier(QUALIFIER, RUST, "-u BASH_ENV -u ENV", "-u BASH_ENV"),
    );
    reject_synthetic_tamper(
        "functions",
        &changed_verifier(
            QUALIFIER,
            RUST,
            r#"BASH_FUNC_*) function_env_unset+=(-u \"$env_name\");;"#,
            "BASH_FUNC_*) :;;",
        ),
    );
    reject_synthetic_tamper(
        "inner credentials",
        &changed_verifier(
            QUALIFIER,
            RUST,
            "unset ACTIONS_ID_TOKEN_REQUEST_TOKEN",
            "true",
        ),
    );
}
