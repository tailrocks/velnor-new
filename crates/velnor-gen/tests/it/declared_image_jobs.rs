// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::declared_fixture::{TestResult, job, refusal, rendered, workflow};
use crate::yaml_tree::Node;

fn image_job(extra: &str) -> String {
    workflow(
        "",
        "",
        &format!(
            "\n[[workflow.job]]\nid = \"publish-images\"\n{extra}\n[[workflow.job.step]]\nname = \"Publish\"\nrun = \"echo publish\"\n"
        ),
    )
}

#[test]
fn an_image_job_renders_the_registry_and_attestation_permissions() -> TestResult {
    let tree = rendered(&image_job(
        "permissions = { packages = \"write\", attestations = \"read\", id-token = \"write\" }\n",
    ))?;
    let permissions = job(&tree, "publish-images")?
        .get("permissions")
        .ok_or("no permissions")?;
    assert_eq!(permissions.keys(), ["attestations", "id-token", "packages"]);
    for (scope, access) in [
        ("packages", "write"),
        ("attestations", "read"),
        ("id-token", "write"),
    ] {
        assert_eq!(permissions.get(scope).and_then(Node::text), Some(access));
    }
    Ok(())
}

#[test]
fn an_unknown_permission_key_or_level_fails_generation() -> TestResult {
    for bad in [
        "permissions = { package = \"write\" }\n",
        "permissions = { packages = \"admin\" }\n",
        "permissions = { id-token = \"read\" }\n",
    ] {
        let message = refusal(&image_job(bad))?;
        assert!(message.contains("publish-images"), "{bad}: {message}");
        assert!(message.contains("permission"), "{bad}: {message}");
    }
    Ok(())
}

#[test]
fn a_job_environment_renders_before_permissions_and_must_not_be_empty() -> TestResult {
    let tree = rendered(&image_job(
        "environment = \"registry\"\npermissions = { packages = \"write\" }\n",
    ))?;
    let image = job(&tree, "publish-images")?;
    assert_eq!(
        image.get("environment").and_then(Node::text),
        Some("registry")
    );
    let keys = image.keys();
    let at = |name: &str| keys.iter().position(|key| *key == name);
    assert!(at("environment") < at("permissions"), "{keys:?}");
    let message = refusal(&image_job("environment = \" \"\n"))?;
    assert!(message.contains("environment"), "{message}");
    Ok(())
}
