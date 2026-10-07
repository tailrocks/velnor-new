//! `[workflow.verify]` vector cases.

use super::*;
/// Config jobs in scrambled order.
fn scrambled() -> Vec<String> {
    [
        "native-validators",
        "link-check",
        "alint",
        "frontmatter-id",
        "markdownlint",
        "strict-json",
        "zizmor",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

#[test]
fn verify_kinds_emit_canonical_order() {
    let kinds = verify_kinds(&scrambled()).expect("known jobs resolve");
    let ids: Vec<&str> = kinds.iter().map(ValidatorKind::job_id).collect();
    assert_eq!(
        ids,
        [
            "zizmor",
            "alint",
            "markdownlint",
            "strict-json",
            "frontmatter-id",
            "link-check",
            "native-validators"
        ]
    );
    assert_eq!(verify_kinds(&[]).expect("empty resolves"), Vec::new());
    let err = verify_kinds(&["wat".to_owned()]).expect_err("unknown fails");
    assert!(err.to_string().contains("unknown_verify_job:wat"), "{err}");
}

#[test]
fn verify_vectors_carry_pins_and_payloads() {
    let catalog = ToolCatalog::pinned();
    let joined = |kind: ValidatorKind| {
        verify_command(kind, &catalog)
            .expect("command builds")
            .expect("shell kind has a command")
            .argv
            .join(" ")
    };
    let markdownlint = joined(ValidatorKind::Markdownlint);
    for token in [
        "npm:markdownlint-cli2@0.23.3",
        "markdownlint-cli2",
        "**/*.{md,markdown}",
        "#**/node_modules/**",
        "#.github/AGENTS.md",
        "#.github/CLAUDE.md",
    ] {
        assert!(markdownlint.contains(token), "{token}: {markdownlint}");
    }
    let strict = joined(ValidatorKind::StrictJson);
    assert!(strict.contains("node@24.21.0"), "{strict}");
    assert!(strict.contains("duplicate-key:"), "{strict}");
    let frontmatter = joined(ValidatorKind::FrontmatterId);
    assert!(frontmatter.contains("node@24.21.0"), "{frontmatter}");
    assert!(frontmatter.contains("name-dir-mismatch:"), "{frontmatter}");
    let links = joined(ValidatorKind::LinkCheck);
    for token in ["ubi:lycheeverse/lychee@0.15.1", "lychee", "--no-progress"] {
        assert!(links.contains(token), "{token}: {links}");
    }
    let native = verify_command(ValidatorKind::NativeValidators, &catalog)
        .expect("command builds")
        .expect("native has a command")
        .argv;
    assert_eq!(native[0], "sh");
    assert_eq!(native[1], "-c");
    assert!(native[2].contains("claude plugin validate --strict"));
    assert!(
        verify_command(ValidatorKind::Alint, &catalog)
            .expect("alint resolves")
            .is_none(),
        "alint renders as an action step"
    );
    let err = verify_command(ValidatorKind::CargoDeny, &catalog).expect_err("deny rejected");
    assert!(err.to_string().contains("verify_kind_rejected"), "{err}");
}

#[test]
fn verify_vectors_pass_renderer_argv_gates() {
    let catalog = ToolCatalog::pinned();
    for kind in ValidatorKind::consumer_verify() {
        let Some(command) = verify_command(kind, &catalog).expect("command builds") else {
            continue;
        };
        velnor_actions_workflow_renderer::validate_command_argv(&command.argv)
            .expect("argv gates pass");
        if !command.prepare_argv.is_empty() {
            velnor_actions_workflow_renderer::validate_command_argv(&command.prepare_argv)
                .expect("prepare gates pass");
        }
        for arg in &command.argv {
            // Script args are whitespace-split by the lock audit; a
            // stray `mise` token inside one blocks generation.
            if arg.chars().any(char::is_whitespace) {
                assert!(
                    !arg.split_whitespace().any(|token| token == "mise"),
                    "no stray mise token trips the lock audit"
                );
            }
        }
    }
    for script in [STRICT_JSON_SCRIPT, FRONTMATTER_ID_SCRIPT] {
        assert!(!script.contains(['\n', '\r', '`']), "single line");
        assert!(!script.contains("$("), "no substitution");
        // No single quotes: the shellcheck pass over rendered `run:`
        // lines cannot parse quote escapes inside large quoted args.
        assert!(!script.contains('\''), "no single quotes");
    }
}

#[test]
fn verify_exec_pairs_with_matching_install() {
    let catalog = ToolCatalog::pinned();
    for kind in ValidatorKind::consumer_verify() {
        let Some(command) = verify_command(kind, &catalog).expect("command builds") else {
            continue;
        };
        let exec = command.argv.iter().find_map(|arg| {
            (command.argv.iter().any(|word| word == "exec") && arg.contains('@'))
                .then_some(arg.as_str())
        });
        match exec {
            None => assert!(
                command.prepare_argv.is_empty(),
                "{kind:?} carries no exec, so no install"
            ),
            Some(spec) => {
                assert_eq!(command.prepare_argv.last().map(String::as_str), Some(spec));
                assert!(
                    command.prepare_argv.iter().any(|word| word == "install"),
                    "{kind:?} prepares through install"
                );
                assert!(
                    crate::vectors::validator_install_pin(spec).is_some(),
                    "{spec} resolves through the validator pin"
                );
            }
        }
    }
}
