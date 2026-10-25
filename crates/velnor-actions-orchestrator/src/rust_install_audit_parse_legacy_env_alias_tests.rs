use super::super::{audit_workflow, step_lists};
use super::{inline_env_alias_workflow, verifiers};

#[test]
fn descendants_taint_neutral_inline_verifiers_in_both_layouts() {
    let verifiers = verifiers();
    let template = verifiers
        .inline_installed_rust_templates
        .last()
        .expect("installed Rust inline verifier");
    for compact in [false, true] {
        let canonical = inline_env_alias_workflow(template, compact, false)
            .replace("Verify Rust channel manifest", "Run verifier");
        assert_eq!(audit_workflow(canonical.clone(), &verifiers), "");

        let (shell_field, descendant_indent) = if compact {
            ("    shell:", "      ")
        } else {
            ("        shell:", "            ")
        };
        let forged = canonical.replacen(
            shell_field,
            &format!("{descendant_indent}FORGED: true\n{shell_field}"),
            1,
        );
        let steps = step_lists(&forged);
        let verifier = &steps[1].1[1];
        assert!(verifier.invalid_anchor);
        assert!(verifier.unexpected_nested);
        assert!(
            audit_workflow(forged, &verifiers).contains("invalid YAML anchor declaration or alias")
        );
    }
}

#[test]
fn comments_and_following_fields_keep_their_own_scope() {
    let verifiers = verifiers();
    let template = verifiers
        .inline_installed_rust_templates
        .last()
        .expect("installed Rust inline verifier");
    for compact in [false, true] {
        let canonical = inline_env_alias_workflow(template, compact, false)
            .replace("Verify Rust channel manifest", "Run verifier");
        let (shell_field, comment_indent, with_field, with_child) = if compact {
            (
                "    shell:",
                "      ",
                "    with:\n",
                "     phase: installed\n",
            )
        } else {
            (
                "        shell:",
                "            ",
                "        with:\n",
                "          phase: installed\n",
            )
        };
        let commented = canonical.replacen(
            shell_field,
            &format!("{comment_indent}# valid comment\n{shell_field}"),
            1,
        );
        assert_eq!(audit_workflow(commented, &verifiers), "");

        let with_child_step = canonical.replacen(
            shell_field,
            &format!("{with_field}{with_child}{shell_field}"),
            1,
        );
        let steps = step_lists(&with_child_step);
        let verifier = &steps[1].1[1];
        assert!(!verifier.invalid_anchor);
        assert!(verifier.unexpected_nested);
        assert_ne!(audit_workflow(with_child_step, &verifiers), "");
    }
}
