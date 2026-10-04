use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepKind};
use velnor_actions_workflow_renderer::cache_p08::{ensure_setup_p08, infer_job_tools};

use super::{TARGET, job, mise, scrubbed_shell_step};

fn raw_shell(argv: &[&str]) -> Step {
    Step {
        name: "Run tool".to_owned(),
        condition: None,
        kind: StepKind::Shell {
            run: argv.iter().map(|arg| (*arg).to_owned()).collect(),
            env: BTreeMap::new(),
        },
    }
}

fn assert_cache_rejects(step: Step) {
    let (_, mut workflow_job) = job("bad-tools", "Bad tools", Vec::new(), vec![step]);
    assert!(
        !infer_job_tools(&workflow_job).is_empty(),
        "possible Mise use must not disappear from inference"
    );
    assert!(
        ensure_setup_p08("bad-tools", &mut workflow_job, &mise(), false, TARGET).is_err(),
        "uncertain tool use must not create a cache key"
    );
}

#[test]
fn hidden_mise_invocations_fail_closed() {
    for argv in [
        &["sh", "-c", "mise install gh@latest 2>/dev/null"][..],
        &["sh", "-c", "2>/dev/null mise install gh@latest"][..],
        &["mise", "--config", ".mise.toml", "install", "gh@2.88.0"][..],
        &["sh", "-c", "FOO=bar mise install gh@latest"][..],
        &["env", "FOO=bar", "mise", "install", "gh@latest"][..],
        &["env", "-u", "CUSTOM", "mise", "install", "gh@latest"][..],
        &["env", "-S", "mise install gh@latest"][..],
        &["sh", "-c", "if true; then mise install gh@latest; fi"][..],
        &["sh", "-c", "if true; then 'mi''se' install gh@latest; fi"][..],
        &["sh", "-c", "! mise install gh@latest"][..],
        &["sh", "-c", "time mise install gh@latest"][..],
        &["timeout", "20", "mise", "install", "gh@latest"][..],
        &["eval", "mise install gh@latest"][..],
        &["custom-launcher", "mise", "install", "gh@2.88.0"][..],
        &["exec", "-a", "alias", "mise", "install", "gh@latest"][..],
        &["bash", "-c", "cat <(mise install gh@latest)"][..],
        &["sh", "-c", "bash -c 'mise install gh@latest'"][..],
        &["sh", "-c", "$MISE_BIN install gh@latest"][..],
        &["sh", "-c", "TOOL=mise; \"$TOOL\" install gh@2.88.0"][..],
        &["sh", "-c", "$TOOL install gh@2.88.0"][..],
        &["/usr/local/bin/mise", "install", "gh@latest"][..],
        &["./mise", "install", "gh@latest"][..],
        &["mise", "install", "gh"][..],
        &["mise", "exec", "gh", "--", "gh", "--version"][..],
        &[
            "mise",
            "exec",
            "rust@1.98.1",
            "--",
            "$TOOL",
            "install",
            "gh@2.88.0",
        ][..],
        &[
            "mise",
            "exec",
            "rust@1.98.1",
            "--",
            "custom-launcher",
            "mise",
            "install",
            "gh@2.88.0",
        ][..],
    ] {
        assert_cache_rejects(raw_shell(argv));
    }
    assert_cache_rejects(raw_shell(&["sh", "-c", "echo `mise install gh@latest`"]));
}

#[test]
fn exact_mise_commands_remain_cacheable() {
    for (index, command) in [
        "(mise install gh@2.88.0)",
        "true; mise install gh@2.88.0",
        "true&&mise install gh@2.88.0",
        "echo '>' && mise install gh@2.88.0 >&2",
    ]
    .into_iter()
    .enumerate()
    {
        let exact = raw_shell(&["sh", "-c", command]);
        let job_id = format!("exact-tools-{index}");
        let (_, mut workflow_job) = job(&job_id, "Exact tools", Vec::new(), vec![exact]);
        assert_eq!(infer_job_tools(&workflow_job), ["gh@2.88.0".to_owned()]);
        assert!(ensure_setup_p08(&job_id, &mut workflow_job, &mise(), false, TARGET).is_ok());
    }

    let nested = raw_shell(&[
        "mise",
        "--no-config",
        "exec",
        "rust@1.98.1",
        "--",
        "mise",
        "--no-config",
        "install",
        "gh@2.88.0",
    ]);
    let (_, mut nested_job) = job("nested-tools", "Nested tools", Vec::new(), vec![nested]);
    assert_eq!(
        infer_job_tools(&nested_job),
        ["gh@2.88.0".to_owned(), "rust@1.98.1".to_owned()]
    );
    assert!(ensure_setup_p08("nested-tools", &mut nested_job, &mise(), false, TARGET).is_ok());

    let direct_flags = raw_shell(&[
        "mise",
        "--no-config",
        "--no-env",
        "--no-hooks",
        "install",
        "gh@2.88.0",
    ]);
    let (_, mut flags_job) = job(
        "global-flags",
        "Global flags",
        Vec::new(),
        vec![direct_flags],
    );
    assert_eq!(infer_job_tools(&flags_job), ["gh@2.88.0".to_owned()]);
    assert!(ensure_setup_p08("global-flags", &mut flags_job, &mise(), false, TARGET).is_ok());

    let nested_data = raw_shell(&[
        "mise",
        "exec",
        "rust@1.98.1",
        "--",
        "echo",
        "mise",
        "install",
        "gh@latest",
    ]);
    let (_, mut nested_data_job) = job("nested-data", "Nested data", Vec::new(), vec![nested_data]);
    assert_eq!(
        infer_job_tools(&nested_data_job),
        ["rust@1.98.1".to_owned()]
    );
    assert!(ensure_setup_p08("nested-data", &mut nested_data_job, &mise(), false, TARGET).is_ok());

    let safe_probe = scrubbed_shell_step(
        "Check Mise",
        vec!["command".into(), "-v".into(), "mise".into()],
    )
    .expect("fixed availability probe");
    let (_, mut probe_job) = job("probe", "Probe", Vec::new(), vec![safe_probe]);
    assert!(infer_job_tools(&probe_job).is_empty());
    assert!(ensure_setup_p08("probe", &mut probe_job, &mise(), false, TARGET).is_ok());

    let redirect = raw_shell(&["sh", "-c", "echo value >&2"]);
    let (_, redirect_job) = job("redirect", "Redirect", Vec::new(), vec![redirect]);
    assert!(infer_job_tools(&redirect_job).is_empty());

    for script in [
        "echo >&2 mise install gh@latest",
        "echo 'mise install gh@latest'",
        "printf '%s' 'mise install gh@latest'",
    ] {
        let quoted = raw_shell(&["sh", "-c", script]);
        let (_, quoted_job) = job("quoted-data", "Quoted data", Vec::new(), vec![quoted]);
        assert!(
            infer_job_tools(&quoted_job).is_empty(),
            "data is not execution: {script}"
        );
    }

    let opaque_argument = raw_shell(&["custom-launcher", "mise install gh@latest"]);
    let (_, opaque_job) = job(
        "opaque-data",
        "Opaque data",
        Vec::new(),
        vec![opaque_argument],
    );
    assert!(infer_job_tools(&opaque_job).is_empty());
}
