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

/// Asserts that the rendered job has exactly `scopes` as its permissions and
/// `environment` as its environment. Both roles are the current velnor-gen
/// renderer contract for declared image jobs.
fn assert_role_shape(extra: &str, scopes: &[(&str, &str)], environment: &str) -> TestResult {
    let tree = rendered(&image_job(extra))?;
    let image = job(&tree, "publish-images")?;
    assert_eq!(
        image.get("environment").and_then(Node::text),
        Some(environment)
    );
    let permissions = image.get("permissions").ok_or("no permissions")?;
    let mut names: Vec<&str> = scopes.iter().map(|(scope, _)| *scope).collect();
    names.sort_unstable();
    assert_eq!(permissions.keys(), names);
    for (scope, access) in scopes {
        assert_eq!(permissions.get(scope).and_then(Node::text), Some(*access));
    }
    Ok(())
}

#[test]
fn the_publish_role_renders_its_contract_permissions() -> TestResult {
    assert_role_shape(
        "environment = \"images\"\npermissions = { contents = \"read\", actions = \"read\", packages = \"write\", id-token = \"write\", attestations = \"write\" }\n",
        &[
            ("contents", "read"),
            ("actions", "read"),
            ("packages", "write"),
            ("id-token", "write"),
            ("attestations", "write"),
        ],
        "images",
    )
}

#[test]
fn the_promote_role_renders_its_contract_permissions() -> TestResult {
    assert_role_shape(
        "environment = \"images\"\npermissions = { contents = \"read\", packages = \"write\", attestations = \"read\" }\n",
        &[
            ("contents", "read"),
            ("packages", "write"),
            ("attestations", "read"),
        ],
        "images",
    )
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
    let (Some(environment), Some(permissions)) = (at("environment"), at("permissions")) else {
        return Err(format!("{keys:?}: environment and permissions must both render").into());
    };
    assert!(environment < permissions, "{keys:?}");
    let message = refusal(&image_job("environment = \" \"\n"))?;
    assert!(message.contains("environment"), "{message}");
    Ok(())
}
