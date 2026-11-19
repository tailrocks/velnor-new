// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};

use crate::ImageKind;

#[derive(Debug)]
pub struct ExpectedBuildSource<'a> {
    pub role: ImageKind,
    pub source_commit: &'a str,
    pub platform_digest: &'a str,
    pub base_image: Option<&'a str>,
    pub named_context_paths: &'a BTreeMap<String, String>,
}

#[must_use]
pub fn admits_build_source(bytes: &[u8], expected: &ExpectedBuildSource<'_>) -> bool {
    let Ok(statement) = crate::strict_json::parse_strict_json_bytes(bytes, bytes.len()) else {
        return false;
    };
    admits_statement(&statement, expected)
}

fn admits_statement(statement: &Value, expected: &ExpectedBuildSource<'_>) -> bool {
    let Some(build) = statement
        .pointer("/predicate/buildDefinition")
        .and_then(Value::as_object)
    else {
        return false;
    };
    let Some(external) = build.get("externalParameters").and_then(Value::as_object) else {
        return false;
    };
    let Some(request) = external.get("request").and_then(Value::as_object) else {
        return false;
    };
    let Some(vcs) = statement
        .pointer("/predicate/runDetails/metadata/buildkit_metadata/vcs")
        .and_then(Value::as_object)
    else {
        return false;
    };
    let (image, context, locals, source_arg) = role_spec(expected.role);
    let Some(digest) = expected
        .platform_digest
        .strip_prefix("sha256:")
        .filter(|value| is_hex(value, 64))
    else {
        return false;
    };
    if !is_hex(expected.source_commit, 40)
        || statement.get("_type").and_then(Value::as_str) != Some("https://in-toto.io/Statement/v1")
        || statement.get("predicateType").and_then(Value::as_str)
            != Some("https://slsa.dev/provenance/v1")
        || build.get("buildType").and_then(Value::as_str)
            != Some(
                "https://github.com/moby/buildkit/blob/master/docs/attestations/slsa-definitions.md",
            )
        || build
            .get("internalParameters")
            .and_then(Value::as_object)
            .and_then(|p| p.get("builderPlatform"))
            .and_then(Value::as_str)
            != Some("linux/amd64")
        || external.len() != 2
        || external
            .get("configSource")
            .and_then(Value::as_object)
            .is_none_or(|v| {
                v.len() != 1 || v.get("path").and_then(Value::as_str) != Some("Dockerfile")
            })
        || !valid_subject(statement, image, digest)
        || !valid_dependencies(build, expected)
        || !valid_request(
            request,
            vcs,
            locals,
            context,
            source_arg,
            expected.source_commit,
            expected.named_context_paths,
        )
    {
        return false;
    }
    true
}

fn role_spec(
    role: ImageKind,
) -> (
    &'static str,
    &'static str,
    &'static [&'static str],
    &'static str,
) {
    match role {
        ImageKind::Runner => (
            "velnor-runner",
            "images/runner/ubuntu-26.04",
            &[
                "context",
                "dockerfile",
                "resource_probe",
                "storage_init",
                "job_start_hook",
                "job_started_hook",
            ][..],
            "VELNOR_SOURCE_SHA",
        ),
        ImageKind::Dind => (
            "velnor-dind",
            "images/dind",
            &["context", "dockerfile", "resource_probe"][..],
            "VELNOR_SOURCE_SHA",
        ),
        ImageKind::ResourceProbe => (
            "velnor-resource-probe",
            "images/resource-probe",
            &["context", "dockerfile", "resource_probe"][..],
            "SOURCE_SHA",
        ),
    }
}

fn is_hex(value: &str, size: usize) -> bool {
    value.len() == size
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_subject(statement: &Value, image: &str, digest: &str) -> bool {
    let Some(subjects) = statement.get("subject").and_then(Value::as_array) else {
        return false;
    };
    subjects.len() == 1
        && subjects[0]
            .as_object()
            .is_some_and(|object| object.len() == 2)
        && subjects[0]
            .get("digest")
            .and_then(Value::as_object)
            .is_some_and(|object| object.len() == 1)
        && subjects[0].get("name").and_then(Value::as_str)
            == Some(format!("pkg:docker/{image}@linux-amd64?platform=linux%2Famd64").as_str())
        && subjects[0]
            .pointer("/digest/sha256")
            .and_then(Value::as_str)
            == Some(digest)
}

fn valid_dependencies(build: &Map<String, Value>, expected: &ExpectedBuildSource<'_>) -> bool {
    if expected.role == ImageKind::ResourceProbe {
        return expected.base_image.is_none()
            && build.len() == 3
            && !build.contains_key("resolvedDependencies");
    }
    let Some(image) = expected.base_image else {
        return false;
    };
    let Some((reference, digest)) = image.split_once('@') else {
        return false;
    };
    let Some(digest_hex) = digest
        .strip_prefix("sha256:")
        .filter(|value| is_hex(value, 64))
    else {
        return false;
    };
    let Some((name, tag)) = reference.rsplit_once(':') else {
        return false;
    };
    if name != "ubuntu" || tag != "26.04" {
        return false;
    }
    let Some(deps) = build.get("resolvedDependencies").and_then(Value::as_array) else {
        return false;
    };
    build.len() == 4
        && deps.len() == 1
        && deps[0].as_object().is_some_and(|object| object.len() == 2)
        && deps[0]
            .get("digest")
            .and_then(Value::as_object)
            .is_some_and(|object| object.len() == 1)
        && deps[0].get("uri").and_then(Value::as_str)
            == Some(
                format!("pkg:docker/{name}@{tag}?digest={digest}&platform=linux%2Famd64").as_str(),
            )
        && deps[0].pointer("/digest/sha256").and_then(Value::as_str) == Some(digest_hex)
}

fn valid_request(
    request: &Map<String, Value>,
    vcs: &Map<String, Value>,
    locals: &[&str],
    context: &str,
    source_arg: &str,
    revision: &str,
    expected_paths: &BTreeMap<String, String>,
) -> bool {
    let names = locals.iter().copied().collect::<BTreeSet<_>>();
    let Some(actual_locals) = request.get("locals").and_then(Value::as_array) else {
        return false;
    };
    let actual_names = actual_locals
        .iter()
        .filter_map(|item| {
            item.as_object()
                .filter(|item| item.len() == 1)
                .and_then(|item| item.get("name"))
                .and_then(Value::as_str)
        })
        .collect::<BTreeSet<_>>();
    let Some(args) = request.get("args").and_then(Value::as_object) else {
        return false;
    };
    let Some(root) = request.get("root").and_then(Value::as_object) else {
        return false;
    };
    let Some(root_args) = root
        .get("request")
        .and_then(Value::as_object)
        .filter(|item| item.len() == 1)
        .and_then(|item| item.get("args"))
        .and_then(Value::as_object)
    else {
        return false;
    };
    if request.len() != 5
        || request.get("frontend").and_then(Value::as_str) != Some("dockerfile.v0")
        || request.get("compatibilityVersion").and_then(Value::as_u64) != Some(30)
        || root.len() != 2
        || root
            .get("configSource")
            .and_then(Value::as_object)
            .is_none_or(|v| {
                v.len() != 1 || v.get("path").and_then(Value::as_str) != Some("Dockerfile")
            })
        || actual_locals.len() != names.len()
        || actual_names != names
        || args
            .get(&format!("build-arg:{source_arg}"))
            .and_then(Value::as_str)
            != Some(revision)
        || args.get("frontend.caps").and_then(Value::as_str)
            != Some("moby.buildkit.frontend.contexts+forward")
        || vcs.len() != names.len() + 2
        || vcs.get("revision").and_then(Value::as_str) != Some(revision)
        || vcs
            .get("source")
            .and_then(Value::as_str)
            .map(|s| s.strip_suffix(".git").unwrap_or(s))
            != Some("https://github.com/tailrocks/velnor-new")
        || vcs.get("localdir:context").and_then(Value::as_str) != Some(context)
        || vcs.get("localdir:dockerfile").and_then(Value::as_str) != Some(context)
        || root_args.len() != args.len() + vcs.len()
        || expected_paths.len() != names.len() - 2
    {
        return false;
    }
    if !args
        .iter()
        .all(|(key, value)| root_args.get(key) == Some(value))
        || !vcs
            .iter()
            .all(|(key, value)| root_args.get(&format!("vcs:{key}")) == Some(value))
    {
        return false;
    }
    valid_named_contexts(&names, args, vcs, expected_paths)
}

fn valid_named_contexts(
    names: &BTreeSet<&str>,
    args: &Map<String, Value>,
    vcs: &Map<String, Value>,
    expected_paths: &BTreeMap<String, String>,
) -> bool {
    for name in names
        .iter()
        .filter(|name| !matches!(**name, "context" | "dockerfile"))
    {
        if args.get(&format!("context:{name}")).and_then(Value::as_str)
            != Some(format!("local:{name}").as_str())
            || args
                .get(&format!("sharedkey:localdir:{name}"))
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
            || vcs.get(&format!("localdir:{name}")).and_then(Value::as_str)
                != expected_paths.get(*name).map(String::as_str)
            || expected_paths.get(*name).is_none_or(String::is_empty)
        {
            return false;
        }
    }
    args.len() == 2 + (names.len() - 2) * 2
}
