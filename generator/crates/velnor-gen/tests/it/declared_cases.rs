// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::declared_fixture::{TestResult, job, refusal, rendered_file};
use crate::release_fixture::jobs;
use crate::yaml_tree::Node;

const MODES: &str = r#"
[[workflow]]
file = "qualification"
name = "Qualification"
hosted_runner = "ubuntu-24.04"
scale_set_runner = "ubuntu-26.04-scale-set"
fork_runner = "ubuntu-24.04"

[workflow.on.workflow_dispatch]

[[workflow.on.workflow_dispatch.input]]
name = "mode"
type = "choice"
options = ["both", "features", "js", "mask", "cache"]
default = "both"
"#;

const CASES: &str = r#"
[[workflow.case]]
id = "js"
title = "JavaScript actions"
modes = ["features", "js"]
timeout_minutes = 5

[[workflow.case.step]]
name = "Run a JavaScript action"
run = "echo js"

[[workflow.case]]
id = "mask"
title = "Mask"
timeout_minutes = 5

[[workflow.case.step]]
name = "Mask canary"
run = "echo mask"

[[workflow.case]]
id = "cache"
title = "Cache"
if = "github.ref != format('refs/heads/{0}', github.event.repository.default_branch)"

[[workflow.case.step]]
name = "Cache"
run = "echo cache"
"#;

fn config() -> String {
    format!("{MODES}{CASES}")
}

fn qualification(config: &str) -> Result<Node, Box<dyn std::error::Error>> {
    rendered_file(config, "qualification")
}

fn selected(tree: &Node, mode: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut chosen = Vec::new();
    for (id, body) in jobs(tree)? {
        let condition = body.get("if").and_then(Node::text).ok_or("no if")?;
        let inner = condition
            .trim_start_matches("${{ ")
            .trim_end_matches(" }}")
            .trim_start_matches('(')
            .split(") && (")
            .next()
            .ok_or("no condition")?
            .trim_end_matches(')');
        if inner
            .split(" || ")
            .any(|term| term == format!("inputs.mode == '{mode}'"))
        {
            chosen.push(id.to_owned());
        }
    }
    Ok(chosen)
}

#[test]
fn the_workflow_has_one_choice_input_named_mode() -> TestResult {
    let tree = qualification(&config())?;
    let inputs = tree
        .at(&["on", "workflow_dispatch", "inputs"])
        .ok_or("no inputs")?;
    assert_eq!(inputs.keys(), ["mode"]);
    assert_eq!(
        inputs.at(&["mode", "type"]).and_then(Node::text),
        Some("choice")
    );
    Ok(())
}

#[test]
fn each_case_has_a_hosted_and_a_scale_set_job_with_the_same_suffix() -> TestResult {
    let tree = qualification(&config())?;
    let ids: Vec<&str> = jobs(&tree)?.into_iter().map(|(id, _)| id).collect();
    assert_eq!(
        ids,
        [
            "js-hosted",
            "js-scale-set",
            "mask-hosted",
            "mask-scale-set",
            "cache-hosted",
            "cache-scale-set"
        ]
    );
    let name = |id: &str| job(&tree, id).map(|body| body.get("name").and_then(Node::text));
    assert_eq!(
        name("js-hosted")?,
        Some("JavaScript actions / GitHub hosted")
    );
    assert_eq!(
        name("js-scale-set")?,
        Some("JavaScript actions / Velnor Scale Set")
    );
    Ok(())
}

#[test]
fn each_mode_selects_only_its_own_jobs() -> TestResult {
    let tree = qualification(&config())?;
    assert_eq!(selected(&tree, "js")?, ["js-hosted", "js-scale-set"]);
    assert_eq!(selected(&tree, "features")?, ["js-hosted", "js-scale-set"]);
    assert_eq!(selected(&tree, "mask")?, ["mask-hosted", "mask-scale-set"]);
    assert_eq!(selected(&tree, "both")?, Vec::<String>::new());
    Ok(())
}

#[test]
fn a_case_condition_joins_its_mode_condition() -> TestResult {
    let tree = qualification(&config())?;
    assert_eq!(
        job(&tree, "cache-hosted")?.get("if").and_then(Node::text),
        Some(
            "${{ (inputs.mode == 'cache') && (github.ref != format('refs/heads/{0}', github.event.repository.default_branch)) }}"
        )
    );
    Ok(())
}

#[test]
fn the_scale_set_job_keeps_the_fork_fence_and_the_hosted_job_has_none() -> TestResult {
    let tree = qualification(&config())?;
    for id in ["js", "mask", "cache"] {
        assert_eq!(
            job(&tree, &format!("{id}-hosted"))?
                .get("runs-on")
                .and_then(Node::text),
            Some("ubuntu-24.04")
        );
        let runs_on = job(&tree, &format!("{id}-scale-set"))?
            .get("runs-on")
            .and_then(Node::text)
            .ok_or("no runs-on")?;
        assert!(
            runs_on.contains("github.event.pull_request.head.repo.full_name == github.repository")
                && runs_on.contains("'ubuntu-26.04-scale-set'")
                && runs_on.contains("'ubuntu-24.04'"),
            "{runs_on}"
        );
    }
    Ok(())
}

#[test]
fn both_jobs_of_a_case_run_the_same_steps() -> TestResult {
    let tree = qualification(&config())?;
    let steps = |id: &str| -> Result<Vec<String>, Box<dyn std::error::Error>> {
        Ok(job(&tree, id)?
            .get("steps")
            .ok_or("no steps")?
            .items()
            .iter()
            .filter_map(|step| step.get("run").and_then(Node::text).map(str::to_owned))
            .collect())
    };
    assert_eq!(steps("js-hosted")?, steps("js-scale-set")?);
    assert_eq!(steps("js-hosted")?, ["echo js"]);
    Ok(())
}

#[test]
fn a_case_needs_a_mode_input_that_offers_its_modes() -> TestResult {
    let no_input = "[[workflow]]\nfile = \"qualification\"\nname = \"Q\"\nhosted_runner = \"ubuntu-24.04\"\nscale_set_runner = \"ubuntu-26.04-scale-set\"\n[workflow.on.workflow_dispatch]\n";
    let message = refusal(&format!("{no_input}{CASES}"))?;
    assert!(message.contains("mode"), "{message}");
    let unknown = CASES.replace("modes = [\"features\", \"js\"]", "modes = [\"nope\"]");
    let message = refusal(&format!("{MODES}{unknown}"))?;
    assert!(message.contains("nope"), "{message}");
    Ok(())
}

#[test]
fn the_pair_runners_must_be_one_hosted_and_one_not() -> TestResult {
    for (from, to, needle) in [
        ("hosted_runner = \"ubuntu-24.04\"", "", "hosted_runner"),
        (
            "scale_set_runner = \"ubuntu-26.04-scale-set\"",
            "",
            "scale_set_runner",
        ),
        (
            "hosted_runner = \"ubuntu-24.04\"",
            "hosted_runner = \"my-box\"",
            "hosted_runner",
        ),
        (
            "scale_set_runner = \"ubuntu-26.04-scale-set\"",
            "scale_set_runner = \"ubuntu-24.04\"",
            "scale_set_runner",
        ),
    ] {
        let message = refusal(&format!("{}{CASES}", MODES.replace(from, to)))?;
        assert!(message.contains(needle), "{needle}: {message}");
    }
    Ok(())
}

#[test]
fn case_ids_cannot_repeat_or_collide_with_a_job() -> TestResult {
    let repeated = format!(
        "{MODES}{CASES}\n[[workflow.case]]\nid = \"js\"\n[[workflow.case.step]]\nrun = \"echo\"\n"
    );
    let message = refusal(&repeated)?;
    assert!(message.contains("js"), "{message}");
    Ok(())
}

#[test]
fn a_case_holds_its_permissions_and_a_plain_job_can_need_its_jobs() -> TestResult {
    let permitted = CASES.replace(
        "title = \"JavaScript actions\"",
        "title = \"JavaScript actions\"\npermissions = { contents = \"read\" }",
    );
    let after = "\n[[workflow.job]]\nid = \"after\"\nneeds = [\"js-hosted\", \"js-scale-set\"]\n\n[[workflow.job.step]]\nname = \"After\"\nrun = \"echo after\"\n";
    let tree = qualification(&format!("{MODES}{permitted}{after}"))?;
    for id in ["js-hosted", "js-scale-set"] {
        assert_eq!(
            job(&tree, id)?
                .at(&["permissions", "contents"])
                .and_then(Node::text),
            Some("read")
        );
    }
    assert_eq!(
        job(&tree, "mask-hosted")?.get("permissions"),
        Some(&Node::Map(Vec::new()))
    );
    assert_eq!(
        job(&tree, "after")?
            .get("needs")
            .map(|needs| needs.texts())
            .ok_or("no needs")?,
        ["js-hosted", "js-scale-set"]
    );
    let refused = permitted.replace("contents = \"read\"", "contents = \"admin\"");
    let message = refusal(&format!("{MODES}{refused}"))?;
    assert!(message.contains("contents"), "{message}");
    Ok(())
}

const DIGEST: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn with_container_and_service(container: &str, service: &str) -> String {
    CASES.replace(
        "[[workflow.case.step]]\nname = \"Mask canary\"",
        &format!(
            "[workflow.case.container]\nimage = \"{container}\"\n\n[[workflow.case.service]]\nname = \"redis\"\nimage = \"{service}\"\nports = [\"6379:6379\"]\noptions = \"--health-cmd 'redis-cli ping'\"\n\n[[workflow.case.step]]\nname = \"Mask canary\""
        ),
    )
}

#[test]
fn a_case_container_and_service_render_on_both_jobs() -> TestResult {
    let image = format!("alpine:3@{DIGEST}");
    let redis = format!("redis:7@{DIGEST}");
    let cases = with_container_and_service(&image, &redis);
    let tree = qualification(&format!("{MODES}{cases}"))?;
    for id in ["mask-hosted", "mask-scale-set"] {
        let body = job(&tree, id)?;
        assert_eq!(
            body.at(&["container", "image"]).and_then(Node::text),
            Some(image.as_str()),
            "{id}"
        );
        let service = body.at(&["services", "redis"]).ok_or("no service")?;
        assert_eq!(
            service.get("image").and_then(Node::text),
            Some(redis.as_str()),
            "{id}"
        );
        assert_eq!(
            service.get("ports").ok_or("no ports")?.texts(),
            ["6379:6379"]
        );
    }
    for id in ["js-hosted", "js-scale-set"] {
        let body = job(&tree, id)?;
        assert!(
            body.get("container").is_none() && body.get("services").is_none(),
            "{id}"
        );
    }
    Ok(())
}

#[test]
fn a_case_container_or_service_without_a_digest_is_refused() -> TestResult {
    let pinned = format!("alpine:3@{DIGEST}");
    let message = refusal(&format!(
        "{MODES}{}",
        with_container_and_service("alpine:3", &pinned)
    ))?;
    assert!(
        message.contains("container") && message.contains("digest"),
        "{message}"
    );
    let message = refusal(&format!(
        "{MODES}{}",
        with_container_and_service(&pinned, "redis:7")
    ))?;
    assert!(
        message.contains("service redis") && message.contains("digest"),
        "{message}"
    );
    Ok(())
}

#[test]
fn a_case_service_needs_a_health_check() -> TestResult {
    let pinned = format!("alpine:3@{DIGEST}");
    let cases = with_container_and_service(&pinned, &pinned)
        .replace("--health-cmd 'redis-cli ping'", "--health-retries 3");
    let message = refusal(&format!("{MODES}{cases}"))?;
    assert!(message.contains("--health-cmd"), "{message}");
    Ok(())
}
