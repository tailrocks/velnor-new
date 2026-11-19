// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::declared_fixture::{TestResult, job, refusal, rendered, workflow};
use crate::yaml_tree::Node;

const DIGEST: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const HEALTH: &str =
    "--health-cmd \"redis-cli ping\" --health-interval 2s --health-timeout 2s --health-retries 15";
const PING: &str = "--health-cmd \"redis-cli ping\"";

fn config(container: &str, service: &str) -> String {
    config_in("", container, service)
}

fn config_in(header: &str, container: &str, service: &str) -> String {
    workflow(
        header,
        "",
        &format!(
            "\n[[workflow.job]]\nid = \"check\"\n{container}\n[[workflow.job.step]]\nname = \"Check\"\nrun = \"echo ok\"\n{service}"
        ),
    )
}

fn toml_text(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

fn container(image: &str) -> String {
    container_with(image, "")
}

fn container_with(image: &str, options: &str) -> String {
    let options = if options.is_empty() {
        String::new()
    } else {
        format!("options = \"{}\"\n", toml_text(options))
    };
    format!("\n[workflow.job.container]\nimage = \"{image}\"\n{options}")
}

fn service(image: &str, extra: &str) -> String {
    service_with("redis", image, "6379:6379", HEALTH, extra)
}

fn service_with(name: &str, image: &str, ports: &str, options: &str, extra: &str) -> String {
    format!(
        "\n[[workflow.job.service]]\nname = \"{name}\"\nimage = \"{image}\"\nports = [\"{ports}\"]\noptions = \"{}\"\n{extra}",
        toml_text(options)
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
        &format!("redis@sha256:bad@{DIGEST}"),
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
        "\n[[workflow.job.service]]\nname = \"redis\"\nimage = \"{image}\"\nports = [\"6379:6379\"]\n"
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

#[test]
fn options_refuse_positional_tokens_and_flags_the_runner_cannot_take() -> TestResult {
    let image = format!("redis@{DIGEST}");
    for bad in [
        format!("{PING} attacker/image:latest"),
        format!("{PING} --network host"),
        format!("{PING} --network=host"),
        format!("{PING} --entrypoint sh"),
        format!("{PING} --privileged"),
        format!("{PING} -v /:/host"),
        format!("{PING} --publish 6379:6379"),
        format!("{PING} --health-interval"),
        format!("{PING} --health-interval --privileged"),
        format!("{PING} 'two words'"),
        format!("{PING} \"unclosed"),
        "--health-cmd".to_owned(),
        "--health-cmd=".to_owned(),
        "--health-cmd \"\"".to_owned(),
    ] {
        let service = service_with("redis", &image, "6379:6379", &bad, "");
        let message = refusal(&config("", &service))?;
        assert!(
            message.contains("job check") && message.contains("options"),
            "{bad}: {message}"
        );
    }
    for bad in [
        "attacker/image:latest",
        "--entrypoint sh",
        "--network=host",
        "--user 1000 attacker/image",
        "-p 80:80",
    ] {
        let message = refusal(&config(&container_with(&image, bad), ""))?;
        assert!(
            message.contains("job check") && message.contains("options"),
            "{bad}: {message}"
        );
    }
    let options = format!("{PING} --health-retries=5");
    let service = service_with("redis", &image, "6379:6379", &options, "");
    let container = container_with(&image, "--user 1000 --cpus=2 --memory 1g --init");
    rendered(&config(&container, &service))?;
    Ok(())
}

#[test]
fn host_ports_are_unique_across_services_and_explicit_without_a_container() -> TestResult {
    let image = format!("redis@{DIGEST}");
    let twice = format!(
        "{}{}",
        service_with("one", &image, "5432:5432", HEALTH, ""),
        service_with("two", &image, "5432:5433", HEALTH, "")
    );
    let message = refusal(&config("", &twice))?;
    assert!(
        message.contains("5432") && message.contains("job check"),
        "{message}"
    );
    let distinct = format!(
        "{}{}",
        service_with("one", &image, "5432:5432", HEALTH, ""),
        service_with("two", &image, "5433:5432", HEALTH, "")
    );
    rendered(&config("", &distinct))?;
    let random = service_with("redis", &image, "6379", HEALTH, "");
    let message = refusal(&config("", &random))?;
    assert!(message.contains("host port"), "{message}");
    rendered(&config(&container(&image), &random))?;
    Ok(())
}

#[test]
fn containers_are_refused_on_runners_that_are_not_linux() -> TestResult {
    let image = format!("redis@{DIGEST}");
    let cases = [
        ("runner = \"macos-latest\"", ""),
        ("", "runner = \"windows-2022\"\n"),
        (
            "fork_runner = \"macos-14\"",
            "runner = \"self-hosted-linux\"\n",
        ),
    ];
    for (header, job_runner) in cases {
        let with_container = format!("{job_runner}{}", container(&image));
        let with_service = service(&image, "");
        for config in [
            config_in(header, &with_container, ""),
            config_in(header, job_runner, &with_service),
        ] {
            let message = refusal(&config)?;
            assert!(
                message.contains("Linux") && message.contains("job check"),
                "{header} {job_runner}: {message}"
            );
        }
    }
    let ubuntu = config_in(
        "runner = \"ubuntu-24.04\"",
        &container(&image),
        &service(&image, ""),
    );
    rendered(&ubuntu)?;
    Ok(())
}
