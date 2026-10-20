//! Manual release-plz tag authority with the shared native release graph.
use super::{
    ConsumerReleaseSpec,
    consumer_preview::{self as shared, TapInvocationContext, steps},
};
use crate::{RenderError, render::RenderedFile, yaml::Yaml};
use velnor_actions_contract::package_release::PackageReleaseChannel;
#[path = "schema2_consumer_stable_source.rs"]
mod source;
#[path = "schema2_consumer_stable_tag.rs"]
mod tag;
/// Closed metadata-only coordinator settings; inventory is shared with preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsumerStableReleaseSpec {
    /// Exact shared version owner.
    pub version_package: String,
    /// Whether release-plz version PR preparation is available.
    pub release_pr: bool,
    /// Captured workspace manifest.
    pub manifest_path: String,
    /// Qualified tool prefix, never repository-selected executable or hooks.
    pub release_plz_argv: Vec<String>,
}
pub(super) fn render_consumer_binary_release(
    spec: &ConsumerReleaseSpec,
    routing: &crate::workflow_routing::WorkflowRoutingPolicy,
) -> Result<RenderedFile, RenderError> {
    spec.policy
        .validate()
        .map_err(|error| RenderError::InvalidWorkflow(error.to_string()))?;
    let stable = spec
        .stable
        .as_ref()
        .ok_or_else(|| invalid("stable_policy_missing"))?;
    if spec.policy.channel != PackageReleaseChannel::Stable
        || spec.policy.default_branch != "main"
        || spec.preview.is_some()
    {
        return Err(invalid("stable_channel_invalid"));
    }
    if !(1..=velnor_actions_contract::config::RustBinaryReleaseProducer::MAX_TIMEOUT_MINUTES)
        .contains(&spec.producer_timeout_minutes)
    {
        return Err(invalid("stable_producer_timeout"));
    }
    validate_coordinator(stable, spec)?;
    spec.linux_setup.validate()?;
    spec.producer_setup.validate()?;
    let mut jobs = vec![source::job(spec)?];
    if stable.release_pr {
        jobs.push(tag::prepare(spec, stable)?);
    }
    jobs.push(tag::release(spec, stable)?);
    jobs.extend(shared::production::jobs(spec)?);
    jobs.extend([
        shared::signer(spec)?,
        shared::publisher(spec)?,
        shared::recovery(spec)?,
    ]);
    if let Some(tap) = &spec.homebrew_tap {
        jobs.extend(shared::tap::jobs(
            spec,
            tap,
            TapInvocationContext::StableAuthority,
        )?);
    }
    let document = Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Stable binary release")),
        (
            "on".to_owned(),
            Yaml::Map(vec![(
                "workflow_dispatch".to_owned(),
                dispatch(stable.release_pr),
            )]),
        ),
        ("permissions".to_owned(), Yaml::Map(Vec::new())),
        (
            "concurrency".to_owned(),
            Yaml::Map(vec![
                (
                    "group".to_owned(),
                    Yaml::str("native-stable-${{ github.repository }}"),
                ),
                ("cancel-in-progress".to_owned(), Yaml::Bool(false)),
            ]),
        ),
        ("jobs".to_owned(), Yaml::Map(jobs)),
    ]);
    let path = PackageReleaseChannel::Stable.workflow_path();
    let document = crate::workflow_routing::route_workflow_yaml(path, document, routing)?;
    Ok(RenderedFile {
        path: path.to_owned(),
        bytes: crate::render::fallback::render_checked_workflow(
            path,
            &document,
            &spec.generator_version,
        )?,
    })
}
fn validate_coordinator(
    stable: &ConsumerStableReleaseSpec,
    spec: &ConsumerReleaseSpec,
) -> Result<(), RenderError> {
    let a = &stable.release_plz_argv;
    let pinned = |value: &str, tool: &str| {
        value
            .strip_prefix(tool)
            .is_some_and(velnor_actions_contract::config::is_exact_semver)
    };
    if a.len() != 9
        || a[..5] != ["mise", "--no-config", "--no-env", "--no-hooks", "exec"]
        || !pinned(&a[5], "rust@")
        || !pinned(&a[6], "release-plz@")
        || a[7..] != ["--", "release-plz"]
    {
        return Err(invalid("stable_coordinator_not_qualified"));
    }
    if stable.version_package != spec.policy.version.version_package
        || stable.manifest_path != spec.policy.version.bindings[0].workspace_manifest_path
        || !spec
            .policy
            .products
            .iter()
            .any(|product| product.package == stable.version_package)
        || !stable.manifest_path.ends_with("Cargo.toml")
        || stable
            .manifest_path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || !stable
            .manifest_path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'.' | b'_' | b'-'))
    {
        return Err(invalid("stable_coordinator_source_invalid"));
    }
    crate::release_config::stable_release_pr_config(&stable.version_package)?;
    Ok(())
}

fn dispatch(prepare: bool) -> Yaml {
    let mut operation = vec![
        (
            "description".to_owned(),
            Yaml::str("Prepare the version PR or release the approved version"),
        ),
        ("required".to_owned(), Yaml::Bool(true)),
        ("type".to_owned(), Yaml::str("choice")),
        (
            "options".to_owned(),
            Yaml::Seq(if prepare {
                vec![Yaml::str("prepare"), Yaml::str("release")]
            } else {
                vec![Yaml::str("release")]
            }),
        ),
    ];
    if prepare {
        operation.push(("default".to_owned(), Yaml::str("prepare")));
    }
    let mut inputs = vec![("operation".to_owned(), Yaml::Map(operation))];
    for key in ["original_run_id", "original_run_attempt"] {
        inputs.push((
            key.to_owned(),
            Yaml::Map(vec![
                (
                    "description".to_owned(),
                    Yaml::str(
                        "Original retained stable transaction; both recovery fields are required together",
                    ),
                ),
                ("required".to_owned(), Yaml::Bool(false)),
                ("type".to_owned(), Yaml::str("string")),
            ]),
        ));
    }
    Yaml::Map(vec![("inputs".to_owned(), Yaml::Map(inputs))])
}

fn invalid(message: &str) -> RenderError {
    RenderError::InvalidWorkflow(message.to_owned())
}
