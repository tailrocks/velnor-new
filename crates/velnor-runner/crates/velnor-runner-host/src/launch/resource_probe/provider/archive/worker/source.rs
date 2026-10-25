use std::collections::HashSet;

use serde_json::{Map, Value};

use super::super::types::Descriptor;
use super::ProviderFailure;
use super::model::{BUILD_KIT_TYPE, PLATFORM, SLSA_PREDICATE, SOURCE_REPOSITORY_URL, WorkerImage};

pub(super) fn verify_build_source(
    statement: &Value,
    image: WorkerImage,
    source_sha: &str,
    platform_manifest: &Descriptor,
) -> Result<(), ProviderFailure> {
    verify_build_subject(statement, image, platform_manifest)?;
    let vcs = statement
        .pointer("/predicate/runDetails/metadata/buildkit_metadata/vcs")
        .and_then(Value::as_object)
        .ok_or(ProviderFailure::Archive)?;
    let build = statement
        .pointer("/predicate/buildDefinition")
        .and_then(Value::as_object)
        .ok_or(ProviderFailure::Archive)?;
    verify_build_metadata(statement, vcs, build, image, source_sha)
}

fn verify_build_subject(
    statement: &Value,
    image: WorkerImage,
    platform_manifest: &Descriptor,
) -> Result<(), ProviderFailure> {
    let subject = statement
        .get("subject")
        .and_then(Value::as_array)
        .ok_or(ProviderFailure::Archive)?;
    let expected_subject = format!(
        "pkg:docker/{}@linux-amd64?platform=linux%2Famd64",
        image.image_name()
    );
    if subject.len() != 1
        || subject[0].get("name").and_then(Value::as_str) != Some(expected_subject.as_str())
        || subject[0].pointer("/digest/sha256").and_then(Value::as_str)
            != platform_manifest.digest.strip_prefix("sha256:")
    {
        return Err(ProviderFailure::Archive);
    }
    Ok(())
}

fn verify_build_metadata(
    statement: &Value,
    vcs: &Map<String, Value>,
    build: &Map<String, Value>,
    image: WorkerImage,
    source_sha: &str,
) -> Result<(), ProviderFailure> {
    let external = build
        .get("externalParameters")
        .and_then(Value::as_object)
        .ok_or(ProviderFailure::Archive)?;
    let config_source = external
        .get("configSource")
        .and_then(Value::as_object)
        .ok_or(ProviderFailure::Archive)?;
    let request = external
        .get("request")
        .and_then(Value::as_object)
        .ok_or(ProviderFailure::Archive)?;
    let locals = request
        .get("locals")
        .and_then(Value::as_array)
        .ok_or(ProviderFailure::Archive)?;
    let local_names = locals
        .iter()
        .map(|local| {
            let local = local.as_object().ok_or(ProviderFailure::Archive)?;
            if local.len() != 1 {
                return Err(ProviderFailure::Archive);
            }
            local
                .get("name")
                .and_then(Value::as_str)
                .ok_or(ProviderFailure::Archive)
        })
        .collect::<Result<HashSet<_>, _>>()?;
    let builder_platform = build
        .get("internalParameters")
        .and_then(Value::as_object)
        .and_then(|parameters| parameters.get("builderPlatform"))
        .and_then(Value::as_str);
    let context = vcs.get("localdir:context").and_then(Value::as_str);
    if statement.get("_type").and_then(Value::as_str) != Some("https://in-toto.io/Statement/v0.1")
        || statement.get("predicateType").and_then(Value::as_str) != Some(SLSA_PREDICATE)
        || build.get("buildType").and_then(Value::as_str) != Some(BUILD_KIT_TYPE)
        || config_source.get("path").and_then(Value::as_str) != Some("Dockerfile")
        || request.get("frontend").and_then(Value::as_str) != Some("dockerfile.v0")
        || locals.len() != 2
        || local_names != HashSet::from(["context", "dockerfile"])
        || external.len() != 2
        || config_source.len() != 1
        || request.len() != 2
        || build.len() != 4
        || builder_platform != Some(PLATFORM)
        || context != Some(image.source_context())
        || vcs.len() != 4
        || vcs.get("revision").and_then(Value::as_str) != Some(source_sha)
        || vcs.get("source").and_then(Value::as_str) != Some(SOURCE_REPOSITORY_URL)
        || vcs.get("localdir:context").and_then(Value::as_str)
            != vcs.get("localdir:dockerfile").and_then(Value::as_str)
    {
        return Err(ProviderFailure::Archive);
    }
    Ok(())
}
