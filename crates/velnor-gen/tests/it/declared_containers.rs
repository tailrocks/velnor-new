// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::declared_fixture::{TestResult, job, refusal, rendered, workflow};
use crate::yaml_tree::Node;

const DIGEST: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const HEALTH: &str =
    "--health-cmd 'redis-cli ping' --health-interval 2s --health-timeout 2s --health-retries 15";

fn config(container: &str, service: &str) -> String {
    workflow(
        "",
        "",
        &format!(
            "\n[[workflow.job]]\nid = \"check\"\n{container}\n[[workflow.job.step]]\nname = \"Check\"\nrun = \"echo ok\"\n{service}"
        ),
    )
}

fn container(image: &str) -> String {
    format!("\n[workflow.job.container]\nimage = \"{image}\"\n")
}

fn service(image: &str, extra: &str) -> String {
    format!(
        "\n[[workflow.job.service]]\nname = \"redis\"\nimage = \"{image}\"\nports = [\"6379:6379\"]\noptions = \"{}\"\n{extra}",
        HEALTH.replace('"', "'")
    )
}

#[test]
fn a_job_container_and_a_redis_service_render_pinned_by_digest() -> TestResult {
    let image = format!("redis:7@{DIGEST}");
    let tree = rendered(&config(
        &container(&format!("ghcr.io/acme/build@{DIGEST}")),
        &service(
            &image,
            "[workflow.job.service.env]\nREDIS_ARGS = \"--save ''\"\n",
        ),
    ))?;
    let check = job(&tree, "check")?;
    assert_eq!(
        check.at(&["container", "image"]).and_then(Node::text),
        Some(format!("ghcr.io/acme/build@{DIGEST}").as_str())
    );
    let redis = check.at(&["services", "redis"]).ok_or("no service")?;
    assert_eq!(
        redis.get("image").and_then(Node::text),
        Some(image.as_str())
    );
    assert_eq!(redis.get("ports").ok_or("no ports")?.texts(), ["6379:6379"]);
    assert_eq!(redis.get("options").and_then(Node::text), Some(HEALTH));
    assert_eq!(
        redis.at(&["env", "REDIS_ARGS"]).and_then(Node::text),
        Some("--save ''")
    );
    Ok(())
}

#[test]
fn an_image_without_a_digest_is_refused_and_names_the_job() -> TestResult {
    for bad in [
        "redis:7",
        "redis",
        "redis@sha256:abc",
        &format!("redis@sha512:{DIGEST}"),
        &format!("redis@{}", DIGEST.to_uppercase()),
    ] {
        let message = refusal(&config(&container(bad), ""))?;
        assert!(
            message.contains("job check") && message.contains("container"),
            "{message}"
        );
        let message = refusal(&config("", &service(bad, "")))?;
        assert!(
            message.contains("job check") && message.contains("service redis"),
            "{message}"
        );
    }
    Ok(())
}

#[test]
fn a_service_needs_a_health_check_and_valid_ports() -> TestResult {
    let image = format!("redis@{DIGEST}");
    let no_health = format!(
        "\n[[workflow.job.service]]\nname = \"redis\"\nimage = \"{image}\"\nports = [\"6379\"]\n"
    );
    let message = refusal(&config("", &no_health))?;
    assert!(
        message.contains("--health-cmd") && message.contains("job check"),
        "{message}"
    );
    for ports in ["\"0:80\"", "\"80:99999\"", "\"abc\"", "\"1:2:3\""] {
        let bad = service(&image, "").replace("\"6379:6379\"", ports);
        assert!(refusal(&config("", &bad))?.contains("port"), "{ports}");
    }
    Ok(())
}

#[test]
fn a_service_is_unique_and_a_call_job_has_none() -> TestResult {
    let image = format!("redis@{DIGEST}");
    let twice = format!("{}{}", service(&image, ""), service(&image, ""));
    assert!(refusal(&config("", &twice))?.contains("redis"));
    let multi_line = service(&image, "").replace("--health-cmd", "\\n--health-cmd");
    assert!(refusal(&config("", &multi_line))?.contains("options"));
    let called = workflow(
        "",
        "",
        &format!(
            "\n[[workflow.job]]\nid = \"call\"\nuses = \"./.github/workflows/other.yml\"\n{}",
            container(&format!("a@{DIGEST}"))
        ),
    );
    assert!(refusal(&called)?.contains("container"));
    Ok(())
}
