use super::bind_workflow;
use crate::yaml::{Yaml, render_yaml};

fn step(pairs: &[(&str, &str)]) -> Yaml {
    Yaml::Map(
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), Yaml::str(*value)))
            .collect(),
    )
}

fn workflow(job_env: Vec<(String, Yaml)>, steps: Vec<Yaml>) -> Yaml {
    let mut job = vec![("runs-on".to_owned(), Yaml::str("ubuntu-26.04"))];
    if !job_env.is_empty() {
        job.push(("env".to_owned(), Yaml::Map(job_env)));
    }
    job.push(("steps".to_owned(), Yaml::Seq(steps)));
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("CI")),
        (
            "jobs".to_owned(),
            Yaml::Map(vec![("build".to_owned(), Yaml::Map(job))]),
        ),
    ])
}

const SETUP: &str = "jdx/mise-action@9b200bcecbfd6303e69a4a261b2e09e3757440a7";

#[test]
fn inserts_resolver_after_setup_and_rewrites_non_run_values() {
    let mut doc = workflow(
        vec![(
            "RUSTUP_TOOLCHAIN".to_owned(),
            Yaml::str("${VELNOR_TOOL_RUST}"),
        )],
        vec![
            step(&[("uses", "actions/checkout@0123")]),
            step(&[("uses", SETUP)]),
            step(&[(
                "run",
                "mise exec 'rust@'\"${VELNOR_TOOL_RUST}\" -- cargo build",
            )]),
            Yaml::Map(vec![
                ("uses".to_owned(), Yaml::str("jdx/mr-boxington-action@0123")),
                (
                    "with".to_owned(),
                    Yaml::Map(vec![(
                        "version".to_owned(),
                        Yaml::str("${VELNOR_TOOL_MR_BOXINGTON}"),
                    )]),
                ),
            ]),
        ],
    );
    bind_workflow(&mut doc).expect("bound");
    let text = render_yaml(&doc);
    assert!(!text.contains("RUSTUP_TOOLCHAIN: ${"), "{text}");
    assert!(
        text.contains("version: ${{ env.VELNOR_TOOL_MR_BOXINGTON }}"),
        "{text}"
    );
    assert!(
        text.contains(r#"\"${VELNOR_TOOL_RUST}\" -- cargo build"#),
        "{text}"
    );
    let resolver = text
        .find("Resolve project tool versions")
        .expect("resolver step");
    assert!(text.find(SETUP).expect("setup") < resolver);
    assert!(resolver < text.find("cargo build").expect("payload"));
    assert!(
        text.contains("velnor_tool VELNOR_TOOL_MR_BOXINGTON '"),
        "{text}"
    );
    assert!(text.contains("velnor_tool VELNOR_TOOL_RUST '"), "{text}");
    assert!(text.contains("velnor_export_rustup=1"), "{text}");
    assert!(text.contains("config get"), "{text}");
}

#[test]
fn reference_without_setup_or_before_setup_fails_closed() {
    let mut missing = workflow(
        Vec::new(),
        vec![step(&[(
            "run",
            "mise exec rust@${VELNOR_TOOL_RUST} -- cargo build",
        )])],
    );
    assert!(bind_workflow(&mut missing).is_err());
    let mut early = workflow(
        Vec::new(),
        vec![
            step(&[("run", "echo ${VELNOR_TOOL_RUST}")]),
            step(&[("uses", SETUP)]),
        ],
    );
    assert!(bind_workflow(&mut early).is_err());
}

#[test]
fn documents_without_references_are_untouched() {
    let original = workflow(
        Vec::new(),
        vec![step(&[("run", "mise exec rust@1.2.3 -- cargo")])],
    );
    let mut doc = original.clone();
    bind_workflow(&mut doc).expect("bound");
    assert_eq!(doc, original);
}

#[test]
fn run_references_become_single_shell_words() {
    use super::quote_run_refs;
    assert_eq!(
        quote_run_refs("mise exec rust@${VELNOR_TOOL_RUST} -- x").expect("plain"),
        "mise exec rust@\"${VELNOR_TOOL_RUST}\" -- x"
    );
    assert_eq!(
        quote_run_refs("sh -c 'exec rust@${VELNOR_TOOL_RUST} -- x'").expect("single"),
        "sh -c 'exec rust@'\"${VELNOR_TOOL_RUST}\"' -- x'"
    );
    assert_eq!(
        quote_run_refs("test \"$V\" = \"${VELNOR_TOOL_RUST}\"").expect("double"),
        "test \"$V\" = \"${VELNOR_TOOL_RUST}\""
    );
    assert_eq!(
        quote_run_refs("a 'b'\\''c' ${VELNOR_TOOL_RUST}").expect("escaped quote"),
        "a 'b'\\''c' \"${VELNOR_TOOL_RUST}\""
    );
    assert_eq!(
        quote_run_refs("printf x \"$(mise exec rust@${VELNOR_TOOL_RUST} -- rustc)\"")
            .expect("subst"),
        "printf x \"$(mise exec rust@\"${VELNOR_TOOL_RUST}\" -- rustc)\""
    );
    assert!(quote_run_refs("echo 'open ${VELNOR_TOOL_RUST}").is_err());
}

fn composite(steps: Vec<Yaml>) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("shared")),
        (
            "runs".to_owned(),
            Yaml::Map(vec![
                ("using".to_owned(), Yaml::str("composite")),
                ("steps".to_owned(), Yaml::Seq(steps)),
            ]),
        ),
    ])
}

#[test]
fn binding_is_idempotent_and_emits_one_resolver_per_document() {
    let steps = || {
        vec![
            step(&[("uses", SETUP)]),
            step(&[("run", "mise exec rust@${VELNOR_TOOL_RUST} -- cargo build")]),
        ]
    };
    for mut doc in [
        composite(steps()),
        workflow(Vec::new(), {
            let mut all = vec![step(&[("uses", "actions/checkout@0123")])];
            all.extend(steps());
            all
        }),
    ] {
        bind_workflow(&mut doc).expect("first");
        let once = render_yaml(&doc);
        bind_workflow(&mut doc).expect("second");
        let twice = render_yaml(&doc);
        assert_eq!(once, twice, "a second bind must change nothing");
        assert_eq!(twice.matches("Resolve project tool versions").count(), 1);
    }
}

#[test]
fn composite_reading_a_version_without_its_own_setup_fails_closed() {
    let mut doc = composite(vec![step(&[(
        "run",
        "mise exec rust@${VELNOR_TOOL_RUST} -- cargo build",
    )])]);
    let err = bind_workflow(&mut doc).expect_err("unbound composite");
    assert!(err.to_string().contains("composite_no_mise_setup"), "{err}");
    // No reference, no requirement.
    let mut inert = composite(vec![step(&[("run", "echo hi")])]);
    bind_workflow(&mut inert).expect("no references");
}

#[test]
fn case_commands_inside_substitutions_are_refused_not_guessed() {
    use super::quote_run_refs;
    let err = quote_run_refs("v=$(case x in a) echo ${VELNOR_TOOL_RUST} ;; esac)")
        .expect_err("case in substitution");
    assert!(
        err.to_string().contains("case_in_command_substitution"),
        "{err}"
    );
    // A word that merely contains `case`, and a top-level case, are fine.
    assert!(quote_run_refs("v=$(showcase ${VELNOR_TOOL_RUST})").is_ok());
    assert!(quote_run_refs("case x in a) echo ${VELNOR_TOOL_RUST} ;; esac").is_ok());
}
