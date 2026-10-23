use crate::yaml::Yaml;

use super::{SETUP, composite, render_yaml, step, workflow};

fn resolver_steps(node: &Yaml) -> Vec<&Yaml> {
    match node {
        Yaml::Map(fields) => {
            let mut found = fields
                .iter()
                .any(|(key, value)| {
                    key == "name" && value == &Yaml::str("Resolve project tool versions")
                })
                .then_some(node)
                .into_iter()
                .collect::<Vec<_>>();
            found.extend(fields.iter().flat_map(|(_, value)| resolver_steps(value)));
            found
        }
        Yaml::Seq(items) => items.iter().flat_map(resolver_steps).collect(),
        _ => Vec::new(),
    }
}

fn map_value<'a>(node: &'a Yaml, key: &str) -> Option<&'a Yaml> {
    match node {
        Yaml::Map(fields) => fields
            .iter()
            .find_map(|(name, value)| (name == key).then_some(value)),
        _ => None,
    }
}

fn run_values(node: &Yaml) -> Vec<&Yaml> {
    match node {
        Yaml::Map(fields) => fields
            .iter()
            .flat_map(|(key, value)| {
                if key == "run" {
                    vec![value]
                } else {
                    run_values(value)
                }
            })
            .collect(),
        Yaml::Seq(items) => items.iter().flat_map(run_values).collect(),
        _ => Vec::new(),
    }
}

#[test]
fn only_generated_resolver_writes_receive_the_same_narrow_annotation() {
    let unsafe_write = "echo UNTRUSTED=$INPUT >> $GITHUB_ENV";
    for mut doc in [
        composite(vec![
            step(&[("uses", SETUP)]),
            step(&[("run", "echo ${VELNOR_TOOL_RUST}")]),
            step(&[("run", unsafe_write)]),
        ]),
        workflow(
            Vec::new(),
            vec![
                step(&[("uses", "actions/checkout@0123")]),
                step(&[("uses", SETUP)]),
                step(&[("run", "echo ${VELNOR_TOOL_RUST}")]),
                step(&[("run", unsafe_write)]),
            ],
        ),
    ] {
        super::bind_workflow(&mut doc).expect("resolver binds");
        let text = render_yaml(&doc);
        assert_eq!(text.matches("zizmor: ignore[github-env]").count(), 1);
        assert!(text.contains(unsafe_write));
        let resolver = resolver_steps(&doc)
            .into_iter()
            .next()
            .expect("resolver map");
        assert_eq!(
            map_value(resolver, "shell"),
            Some(&Yaml::str(crate::VERIFY_VERIFIER_SHELL))
        );
        let Some(Yaml::Map(env)) = map_value(resolver, "env") else {
            panic!("resolver env map");
        };
        assert_eq!(env.len(), 12, "trusted PATH plus eleven scrub values");
        for key in crate::toolchain_env::STEP_CREDENTIAL_DENYLIST
            .into_iter()
            .chain(crate::toolchain_env::STEP_ENDPOINT_DENYLIST)
        {
            assert!(env.contains(&(key.to_owned(), Yaml::str(""))), "{key}");
        }
        assert!(map_value(resolver, "run").is_some_and(|value| {
            matches!(value, Yaml::Annotated { value, comment }
                if value.starts_with("function_env_unset=(); function_env_scan_complete=0; while IFS= read -r -d '' env_entry;")
                    && value.contains("exec /usr/bin/env \"${function_env_unset[@]}\" -u BASH_ENV -u ENV -u ACTIONS_ID_TOKEN_REQUEST_TOKEN -u")
                    && comment == "zizmor: ignore[github-env]")
        }));
        // The caller's arbitrary write remains an ordinary, unannotated scalar.
        assert!(
            run_values(&doc)
                .iter()
                .any(|value| matches!(value, Yaml::Str(script) if script == unsafe_write))
        );
        assert!(run_values(&doc).iter().any(|value| {
            matches!(value, Yaml::Annotated { value, comment }
                if value.contains("velnor_exact()") && comment == "zizmor: ignore[github-env]")
        }));
    }
}

#[test]
fn identity_lock_references_resolve_their_tool_and_lock_digest() {
    let mut doc = workflow(
        Vec::new(),
        vec![
            step(&[("uses", "actions/checkout@0123")]),
            step(&[("uses", SETUP)]),
            Yaml::Map(vec![
                (
                    "uses".to_owned(),
                    Yaml::str("./.github/actions/velnor-tools-prelude-u26"),
                ),
                (
                    "with".to_owned(),
                    Yaml::Map(vec![(
                        "d".to_owned(),
                        Yaml::str(format!(
                            "{}-${{VELNOR_TOOL_RUST}}-${{VELNOR_TOOL_RUST_LOCK}}",
                            "a".repeat(64)
                        )),
                    )]),
                ),
            ]),
        ],
    );
    super::bind_workflow(&mut doc).expect("bound");
    let text = render_yaml(&doc);
    assert!(
        text.contains("-${{ env.VELNOR_TOOL_RUST }}-${{ env.VELNOR_TOOL_RUST_LOCK }}"),
        "{text}"
    );
    assert_eq!(
        text.matches("velnor_tool VELNOR_TOOL_RUST '").count(),
        1,
        "{text}"
    );
    let resolver = resolver_steps(&doc).into_iter().next().expect("resolver");
    let run = match map_value(resolver, "run") {
        Some(Yaml::Annotated { value, comment }) => {
            assert_eq!(comment, "zizmor: ignore[github-env]");
            value
        }
        other => panic!("resolver run lost its narrow annotation: {other:?}"),
    };
    let binder = velnor_actions_contract::config::selected_mise::BIND_SELECTED_MISE_FUNCTION
        .replace('\'', "'\\''");
    assert!(run.contains(&binder), "selected-Mise binder changed: {run}");
    for required in [
        r#"velnor_bind_selected_mise "$velnor_dir/mise-data-dir" ||"#,
        r#"[ -f "$mise_bin" ] && [ -x "$mise_bin" ] && [ ! -L "$mise_bin" ]"#,
        r#"velnor_mise() { "$mise_bin" --no-config --no-env --no-hooks "$@"; }"#,
        r#"velnor_mise config get -f "$velnor_dir/src.toml" > /dev/null 2>&1"#,
        r#"velnor_mise config get -f "$velnor_dir/src.toml" tools > "$velnor_dir/tools" 2>/dev/null"#,
    ] {
        assert!(run.contains(required), "resolver lost {required:?}: {run}");
    }
    assert!(
        run.contains(r"velnor_tool VELNOR_TOOL_RUST '\''rust'\'' '\''core:rust'\''"),
        "{run}"
    );
    assert!(
        run.contains(r"velnor_lock VELNOR_TOOL_RUST_LOCK '\''rust'\'' '\''core:rust'\''"),
        "{run}"
    );
    assert!(
        run.contains("lock_digest=$(sha256sum < \"$velnor_dir/lock\")")
            && run.contains("lock_digest=${lock_digest%% *}")
            && run.contains(r#"printf '\''%s=%s\n'\'' "$name" "$lock_digest" >> "$GITHUB_ENV""#),
        "the selected lock entries must produce and export their digest: {run}"
    );
    let resolver = text
        .find("Resolve project tool versions")
        .expect("resolver");
    assert!(resolver < text.find("velnor-tools-prelude-u26").expect("identity"));
}
