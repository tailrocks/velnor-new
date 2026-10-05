use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepKind};
use velnor_actions_workflow_renderer::cache_p08::{ensure_setup_p08, infer_job_tools};

use super::{TARGET, job, mise, scrubbed_shell_step};

#[path = "impl_renderer_cache_mbx_lane.rs"]
mod mbx_lane;

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

fn assert_cacheable_tools(run: &[&str], job_id: &str, expected: &[&str]) {
    let step = raw_shell(run);
    let (_, mut workflow_job) = job(job_id, "Exact tools", Vec::new(), vec![step]);
    let expected = expected
        .iter()
        .map(|spec| (*spec).to_owned())
        .collect::<Vec<_>>();
    assert_eq!(infer_job_tools(&workflow_job), expected);
    assert!(ensure_setup_p08(job_id, &mut workflow_job, &mise(), false, TARGET).is_ok());
}

#[test]
fn hidden_mise_invocations_fail_closed() {
    for argv in [
        &["sh", "-c", "mise install gh@latest 2>/dev/null"][..],
        &["sh", "-c", "2>/dev/null mise install gh@latest"][..],
        &["mise", "--config", ".mise.toml", "install", "gh@2.88.0"][..],
        &["mise", "where", "gh@2.88.0"][..],
        &["sh", "-c", "FOO=bar mise install gh@latest"][..],
        &["env", "FOO=bar", "mise", "install", "gh@latest"][..],
        &["env", "-u", "CUSTOM", "mise", "install", "gh@latest"][..],
        &["env", "-S", "mise install gh@latest"][..],
        &["sh", "-c", "if true; then mise install gh@latest; fi"][..],
        &[
            "sh",
            "-c",
            "if true; then mise --no-config install gh@2.88.0; fi",
        ][..],
        &["sh", "-c", "if true; then 'mi''se' install gh@latest; fi"][..],
        &["sh", "-c", "! mise install gh@latest"][..],
        &["sh", "-c", "time mise install gh@latest"][..],
        &["timeout", "20", "mise", "install", "gh@latest"][..],
        &["eval", "mise install gh@latest"][..],
        &["custom-launcher", "mise", "install", "gh@2.88.0"][..],
        &["exec", "-a", "alias", "mise", "install", "gh@latest"][..],
        &["env", "bash", "-c", "mise install gh@2.88.0"][..],
        &["exec", "--", "bash", "-c", "mise install gh@2.88.0"][..],
        &[
            "command",
            "timeout",
            "20",
            "mise",
            "--no-config",
            "install",
            "gh@2.88.0",
        ][..],
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
            "env",
            "timeout",
            "20",
            "mise",
            "--no-config",
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
fn path_qualified_launchers_fail_closed() {
    for argv in [
        &["/bin/bash", "-c", "mise install gh@latest"][..],
        &["/bin/dash", "-c", "mise install gh@latest"][..],
        &["/usr/bin/env", "mise", "install", "gh@latest"][..],
        &["/usr/bin/env", "/bin/bash", "-c", "mise install gh@latest"][..],
        &["exec", "--", "/bin/bash", "-c", "mise install gh@latest"][..],
        &[
            "mise",
            "exec",
            "rust@1.98.1",
            "--",
            "/usr/bin/env",
            "/bin/bash",
            "-c",
            "mise install gh@latest",
        ][..],
        &[
            "mise",
            "exec",
            "rust@1.98.1",
            "--",
            "/usr/bin/command",
            "timeout",
            "20",
            "/usr/local/bin/mise",
            "install",
            "gh@latest",
        ][..],
    ] {
        assert_cache_rejects(raw_shell(argv));
    }
}

#[test]
fn hidden_installations_reject_mixed_tool_cache_inference() {
    let hidden = [
        &["env", "bash", "-c", "mise install gh@2.88.0"][..],
        &["exec", "--", "bash", "-c", "mise install gh@2.88.0"][..],
        &[
            "command",
            "timeout",
            "20",
            "mise",
            "--no-config",
            "install",
            "gh@2.88.0",
        ][..],
        &[
            "sh",
            "-c",
            "if true; then mise --no-config install gh@2.88.0; fi",
        ][..],
        &[
            "mise",
            "exec",
            "node@22.19.0",
            "--",
            "env",
            "timeout",
            "20",
            "mise",
            "--no-config",
            "install",
            "gh@2.88.0",
        ][..],
    ];
    for (index, run) in hidden.into_iter().enumerate() {
        let exact = raw_shell(&["mise", "install", "rust@1.98.1"]);
        let opaque = raw_shell(run);
        let job_id = format!("mixed-tools-{index}");
        let (_, mut workflow_job) = job(&job_id, "Mixed tools", Vec::new(), vec![exact, opaque]);
        assert!(
            infer_job_tools(&workflow_job).contains(&"rust@1.98.1".to_owned()),
            "the known tool remains visible"
        );
        assert!(
            ensure_setup_p08(&job_id, &mut workflow_job, &mise(), false, TARGET).is_err(),
            "an opaque installation must invalidate the mixed cache key"
        );
    }
}

#[test]
fn path_qualified_hidden_installations_reject_mixed_cache_keys() {
    for (index, hidden) in [
        &["/bin/bash", "-c", "mise install gh@latest"][..],
        &["/usr/bin/env", "/bin/bash", "-c", "mise install gh@latest"][..],
        &[
            "mise",
            "exec",
            "node@22.19.0",
            "--",
            "/usr/bin/command",
            "timeout",
            "20",
            "/usr/local/bin/mise",
            "install",
            "gh@latest",
        ][..],
    ]
    .into_iter()
    .enumerate()
    {
        let exact = raw_shell(&["mise", "install", "rust@1.98.1"]);
        let opaque = raw_shell(hidden);
        let job_id = format!("path-mixed-tools-{index}");
        let (_, mut workflow_job) =
            job(&job_id, "Mixed path tools", Vec::new(), vec![exact, opaque]);
        assert!(ensure_setup_p08(&job_id, &mut workflow_job, &mise(), false, TARGET).is_err());
    }
}

#[test]
fn exact_mise_scripts_remain_cacheable() {
    for (index, command) in [
        "(mise install gh@2.88.0)",
        "true; mise install gh@2.88.0",
        "true&&mise install gh@2.88.0",
        "echo '>' && mise install gh@2.88.0 >&2",
    ]
    .into_iter()
    .enumerate()
    {
        let job_id = format!("exact-tools-{index}");
        assert_cacheable_tools(&["sh", "-c", command], &job_id, &["gh@2.88.0"]);
    }
}

#[test]
fn nested_mise_install_remains_cacheable() {
    assert_cacheable_tools(
        &[
            "mise",
            "--no-config",
            "exec",
            "rust@1.98.1",
            "--",
            "mise",
            "--no-config",
            "install",
            "gh@2.88.0",
        ],
        "nested-tools",
        &["gh@2.88.0", "rust@1.98.1"],
    );
}

#[test]
fn direct_mise_global_flags_remain_cacheable() {
    assert_cacheable_tools(
        &[
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "install",
            "gh@2.88.0",
        ],
        "global-flags",
        &["gh@2.88.0"],
    );
}

#[test]
fn nested_echo_payload_is_data() {
    assert_cacheable_tools(
        &[
            "mise",
            "exec",
            "rust@1.98.1",
            "--",
            "echo",
            "mise",
            "install",
            "gh@latest",
        ],
        "nested-data",
        &["rust@1.98.1"],
    );
}

#[test]
fn env_and_command_wrappers_preserve_exact_tool_specs() {
    for (index, run) in [
        &[
            "env",
            "-u",
            "CUSTOM",
            "mise",
            "--no-config",
            "install",
            "gh@2.88.0",
        ][..],
        &["command", "-p", "mise", "install", "gh@2.88.0"][..],
    ]
    .into_iter()
    .enumerate()
    {
        let job_id = format!("supported-wrapper-{index}");
        assert_cacheable_tools(run, &job_id, &["gh@2.88.0"]);
    }
}

#[test]
fn probes_redirects_and_quoted_text_are_not_tools() {
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
