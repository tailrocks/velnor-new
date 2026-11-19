// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::declared_fixture::{JOB, TestResult, refusal, rendered, rendered_file, workflow};

const PUSH: &str = "[workflow.on.push]\nbranches = [\"main\"]\n";
const GROUP: &str = "build-${{ github.workflow }}-${{ github.ref }}";

fn chain(length: usize) -> String {
    let mut config = String::new();
    for level in 1..=length {
        let job = match level {
            1 => "[[workflow.job.step]]\nname = \"Build\"\nrun = \"echo ok\"".to_owned(),
            _ => String::new(),
        };
        let uses = match level {
            1 => String::new(),
            _ => format!("uses = \"level-{}\"\n", level - 1),
        };
        config.push_str(&format!(
            "[[workflow]]\nfile = \"level-{level}\"\nname = \"Level {level}\"\n\n[[workflow.job]]\nid = \"run\"\n{uses}\n{job}\n\n"
        ));
    }
    config
}

#[test]
fn a_chain_of_ten_reusable_workflows_is_accepted_and_eleven_is_refused() -> TestResult {
    rendered_file(&chain(10), "level-10")?;
    let message = refusal(&chain(11))?;
    assert!(
        message.contains("level-10") && message.contains("11 workflows"),
        "{message}"
    );
    Ok(())
}

fn shared(caller_job: &str, caller_workflow: &str, callee: &str) -> String {
    format!(
        "[[workflow]]\nfile = \"leaf\"\nname = \"Leaf\"\n{callee}\n\n[[workflow.job]]\nid = \"build\"\n\n[[workflow.job.step]]\nname = \"Build\"\nrun = \"echo ok\"\n\n[[workflow]]\nfile = \"ci\"\nname = \"CI\"\n{caller_workflow}\n{PUSH}\n[[workflow.job]]\nid = \"release\"\nuses = \"leaf\"\n{caller_job}\n"
    )
}

fn group(cancel: bool) -> String {
    format!("group = \"{GROUP}\"\ncancel_in_progress = {cancel}")
}

#[test]
fn a_call_job_and_its_callee_cannot_share_a_cancelling_concurrency_group() -> TestResult {
    let callee = format!("[workflow.concurrency]\n{}", group(false));
    let cases = [
        (
            format!("concurrency = {{ group = \"{GROUP}\", cancel_in_progress = true }}"),
            String::new(),
            callee.clone(),
        ),
        (
            String::new(),
            format!("[workflow.concurrency]\n{}", group(true)),
            callee,
        ),
        (
            String::new(),
            format!("[workflow.concurrency]\n{}", group(false)),
            format!(
                "[[workflow.job]]\nid = \"first\"\nconcurrency = {{ group = \"{GROUP}\", cancel_in_progress = true }}\n\n[[workflow.job.step]]\nname = \"First\"\nrun = \"echo ok\"\n"
            ),
        ),
    ];
    for (job, flow, callee) in cases {
        let message = refusal(&shared(&job, &flow, &callee))?;
        assert!(message.contains("deadlock"), "{job} {flow}: {message}");
    }
    Ok(())
}

#[test]
fn a_call_job_and_its_callee_may_use_different_groups_or_none_that_cancel() -> TestResult {
    let own = "[workflow.concurrency]\ngroup = \"leaf-${{ github.workflow }}-${{ github.ref }}\"\ncancel_in_progress = true";
    rendered(&shared(
        &format!("concurrency = {{ group = \"{GROUP}\", cancel_in_progress = true }}"),
        "",
        own,
    ))?;
    rendered(&shared(
        "",
        &format!("[workflow.concurrency]\n{}", group(false)),
        &format!("[workflow.concurrency]\n{}", group(false)),
    ))?;
    Ok(())
}

fn steps_of(uses: &str, with: &str, permissions: &str) -> String {
    workflow(
        "",
        PUSH,
        &format!(
            "\n[[workflow.job]]\nid = \"check\"\n{permissions}\n[[workflow.job.step]]\nuses = \"{uses}\"\n{with}\n"
        ),
    )
}

#[test]
fn a_pinned_action_takes_only_its_own_inputs_and_needs_its_required_ones() -> TestResult {
    let read = "permissions = { contents = \"read\" }";
    let cases = [
        ("checkout", "with = { reff = \"main\" }", "reff"),
        ("mise", "with = { versoin = \"1\" }", "versoin"),
        (
            "upload-artifact",
            "with = { name = \"x\" }",
            "required input path",
        ),
        (
            "cache-restore",
            "with = { path = \"x\" }",
            "required input key",
        ),
        (
            "cache-save",
            "with = { key = \"x\" }",
            "required input path",
        ),
        (
            "github-script",
            "with = { debug = \"true\" }",
            "required input script",
        ),
        ("checkout", "with = { ref = \"a\", REF = \"b\" }", "unique"),
    ];
    for (action, with, needle) in cases {
        let message = refusal(&steps_of(action, with, read))?;
        assert!(message.contains(needle), "{action} {with}: {message}");
    }
    for (action, with) in [
        ("checkout", "with = { Fetch-Depth = \"1\", ref = \"main\" }"),
        ("upload-artifact", "with = { path = \"dist\" }"),
        ("cache-restore", "with = { path = \"x\", key = \"k\" }"),
        ("github-script", "with = { script = \"return 1\" }"),
    ] {
        rendered(&steps_of(action, with, read))?;
    }
    Ok(())
}

#[test]
fn a_checkout_step_needs_contents_read_on_its_job() -> TestResult {
    for permissions in [
        "",
        "permissions = { actions = \"read\" }",
        "permissions = { contents = \"none\" }",
    ] {
        let message = refusal(&steps_of("checkout", "", permissions))?;
        assert!(
            message.contains("contents") && message.contains("\"read\""),
            "{permissions}: {message}"
        );
    }
    for level in ["read", "write"] {
        rendered(&steps_of(
            "checkout",
            "",
            &format!("permissions = {{ contents = \"{level}\" }}"),
        ))?;
    }
    Ok(())
}

#[test]
fn a_condition_with_a_line_or_paragraph_separator_is_refused() -> TestResult {
    for separator in ["\\u2028", "\\u2029", "\\n"] {
        let config = workflow(
            "",
            PUSH,
            &JOB.replace(
                "id = \"check\"\n",
                &format!("id = \"check\"\nif = \"github.ref == 'a'{separator}\"\n"),
            ),
        );
        let message = refusal(&config)?;
        assert!(message.contains("one line"), "{separator}: {message}");
    }
    Ok(())
}

fn followers(count: usize) -> String {
    let mut config = String::new();
    for level in 0..=count {
        let on = match level {
            0 => PUSH.to_owned(),
            _ => format!(
                "[workflow.on.workflow_run]\nworkflows = [\"Stage {}\"]\n",
                level - 1
            ),
        };
        config.push_str(&format!(
            "[[workflow]]\nfile = \"stage-{level}\"\nname = \"Stage {level}\"\n{on}\n{JOB}\n"
        ));
    }
    config
}

#[test]
fn workflow_run_chains_stop_at_three_levels_and_never_loop() -> TestResult {
    rendered_file(&followers(3), "stage-3")?;
    let message = refusal(&followers(4))?;
    assert!(
        message.contains("stage-4") && message.contains("workflow_run"),
        "{message}"
    );
    let looped = "[[workflow]]\nfile = \"one\"\nname = \"One\"\n[workflow.on.workflow_run]\nworkflows = [\"Two\"]\n\n[[workflow.job]]\nid = \"check\"\n[[workflow.job.step]]\nname = \"Check\"\nrun = \"echo ok\"\n\n[[workflow]]\nfile = \"two\"\nname = \"Two\"\n[workflow.on.workflow_run]\nworkflows = [\"One\"]\n\n[[workflow.job]]\nid = \"check\"\n[[workflow.job.step]]\nname = \"Check\"\nrun = \"echo ok\"\n";
    assert!(refusal(looped)?.contains("follows itself"));
    Ok(())
}
