use std::fmt::Write as _;

use super::super::{StepText, parse::step_lists};
use super::{files, problem, verifiers};
use crate::workflow::verifier_actions::InlineVerifierTemplate;
use velnor_actions_workflow_renderer::yaml::quote_scalar;

fn layout(compact: bool) -> (&'static str, &'static str, &'static str, &'static str) {
    if compact {
        ("jobs:\n build:\n  steps:\n", "   -", "    ", "     ")
    } else {
        (
            "jobs:\n  build:\n    steps:\n",
            "      -",
            "        ",
            "          ",
        )
    }
}

fn anchored_workflow(template: &InlineVerifierTemplate, compact: bool) -> String {
    let (prefix, item, field, child) = layout(compact);
    let mut yaml =
        format!("{prefix}{item}\n{field}name: Neutral verifier\n{field}env: &verify_env\n");
    for (key, value) in &template.env {
        writeln!(yaml, "{child}{key}: {}", quote_scalar(value)).unwrap_or_default();
    }
    writeln!(
        yaml,
        "{field}shell: {}\n{field}run: &verify_run {}",
        quote_scalar(template.shell.expect("canonical verifier shell")),
        template.run_scalar
    )
    .unwrap_or_default();
    writeln!(
        yaml,
        "{item}\n{field}name: Another neutral label\n{field}env: *verify_env\n{field}shell: {}\n{field}run: *verify_run",
        quote_scalar(template.shell.expect("canonical verifier shell"))
    )
    .unwrap_or_default();
    yaml
}

fn aliased_step(yaml: &str) -> StepText {
    step_lists(yaml)
        .into_iter()
        .flat_map(|(_, steps)| steps)
        .last()
        .expect("aliased verifier step")
}

fn action_with_env_and_with(compact: bool) -> String {
    if compact {
        concat!(
            "jobs:\n build:\n  steps:\n",
            "   -\n",
            "    name: Environment source\n",
            "    env: &shared_env\n",
            "     KEY: value\n",
            "   -\n",
            "    name: External action\n",
            "    uses: actions/checkout@v4\n",
            "    env: *shared_env\n",
            "    with:\n",
            "     ref: main\n",
        )
        .to_owned()
    } else {
        concat!(
            "jobs:\n  build:\n    steps:\n",
            "      - name: Environment source\n",
            "        env: &shared_env\n",
            "          KEY: value\n",
            "      - name: External action\n",
            "        uses: actions/checkout@v4\n",
            "        env: *shared_env\n",
            "        with:\n",
            "          ref: main\n",
        )
        .to_owned()
    }
}

#[test]
fn env_aliases_reject_content_but_allow_comments_and_sibling_maps() {
    let verifier_set = verifiers();
    let template = verifier_set
        .inline_installed_rust_templates
        .last()
        .expect("installed Rust inline verifier");
    for compact in [false, true] {
        let yaml = anchored_workflow(template, compact);
        let (_, _, field, child) = layout(compact);
        let forged = yaml.replace(
            &format!("{field}env: *verify_env\n"),
            &format!("{field}env: *verify_env\n{child}INJECTED: value\n"),
        );
        assert_ne!(forged, yaml);
        let parsed = aliased_step(&forged);
        assert!(parsed.invalid_anchor && parsed.unexpected_nested);
        let found = problem(&files(".github/workflows/release.yml", &forged));
        assert!(
            found.contains("unresolved or malformed YAML anchor"),
            "env alias descendants fail at anchor validation: {found}"
        );
        assert!(!found.contains("noncanonical verifier call or body"));

        let with_comment = yaml.replace(
            &format!("{field}env: *verify_env\n"),
            &format!("{field}env: *verify_env\n{child}# legal alias comment\n"),
        );
        assert_ne!(with_comment, yaml);
        assert_eq!(
            problem(&files(".github/workflows/release.yml", &with_comment)),
            "",
            "comment-only continuation is legal at either layout width"
        );

        let with_sibling_map = action_with_env_and_with(compact);
        let parsed = aliased_step(&with_sibling_map);
        assert!(!parsed.invalid_anchor);
        assert!(parsed.top_fields.iter().any(|(key, _)| key == "with"));
        assert!(
            parsed
                .fields
                .iter()
                .any(|(key, value)| key == "ref" && value == "main")
        );
    }
}
