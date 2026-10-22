use std::collections::BTreeMap;

use velnor_actions_contract::{ExecutionMode, JobTimeout, ScaleSetSelector, Step, StepKind};

use super::render_publish_workflow_with_routing;
use crate::{
    publish_spec::{
        PublishJobSpec, PublishPermissions, PublishRole, PublishStep, PublishTriggers,
        PublishWorkflowSpec,
    },
    release_tree::ReleaseRenderContext,
};

fn job(role: PublishRole, needs: &[&str]) -> PublishJobSpec {
    PublishJobSpec {
        role,
        display_name: role.as_str().to_owned(),
        runs_on: "ubuntu-24.04".to_owned(),
        timeout_minutes: JobTimeout::RELEASE,
        needs: needs.iter().map(ToString::to_string).collect(),
        condition: None,
        environment: (role == PublishRole::ArtifactPublish).then(|| "release".to_owned()),
        outputs: BTreeMap::new(),
        permissions: PublishPermissions::expected(role),
        services: Vec::new(),
        steps: vec![PublishStep {
            id: None,
            step: Step {
                parallel: false,
                name: "Publish work".to_owned(),
                id: None,
                role: None,
                condition: None,
                kind: StepKind::Shell {
                    run: vec!["sh".to_owned(), "-c".to_owned(), "exit 0".to_owned()],
                    env: BTreeMap::new(),
                },
            },
        }],
    }
}

fn spec() -> PublishWorkflowSpec {
    PublishWorkflowSpec {
        name: "Publish".to_owned(),
        repository: "acme/widgets".to_owned(),
        staged_binary: None,
        triggers: PublishTriggers {
            branches: Vec::new(),
            tag_glob: Some("*-v[0-9]*.[0-9]*.[0-9]*".to_owned()),
        },
        jobs: BTreeMap::from([
            (
                "publish-authority".to_owned(),
                job(PublishRole::Authority, &[]),
            ),
            (
                "bundle-build-a".to_owned(),
                job(PublishRole::ArtifactBuild, &["publish-authority"]),
            ),
            (
                "bundle-attest-a".to_owned(),
                job(PublishRole::ArtifactAttest, &["bundle-build-a"]),
            ),
            (
                "bundle-publish-a".to_owned(),
                job(PublishRole::ArtifactPublish, &["bundle-attest-a"]),
            ),
        ]),
    }
}

fn parse(rendered: &crate::RenderedFile) -> saphyr::Yaml<'_> {
    crate::workflow_routing::parse_authority_document(rendered)
        .expect("The rendered publication workflow must parse.")
}

fn field<'a, 'input>(value: &'a saphyr::Yaml<'input>, name: &str) -> &'a saphyr::Yaml<'input> {
    value
        .as_mapping()
        .expect("the value must be a YAML map")
        .iter()
        .find(|(key, _)| key.as_str() == Some(name))
        .map(|(_, value)| value)
        .expect("the required YAML field must exist")
}

fn retained_fields<'a, 'input>(
    value: &'a saphyr::Yaml<'input>,
    excluded: &[&str],
) -> BTreeMap<&'a str, &'a saphyr::Yaml<'input>> {
    value
        .as_mapping()
        .expect("the value must be a YAML map")
        .iter()
        .filter_map(|(key, value)| {
            let key = key.as_str()?;
            (!excluded.contains(&key)).then_some((key, value))
        })
        .collect()
}

fn retained_job_fields<'a, 'input>(
    job: &'a saphyr::Yaml<'input>,
) -> BTreeMap<&'a str, &'a saphyr::Yaml<'input>> {
    retained_fields(job, &["runs-on", "container", "defaults"])
}

fn assert_publish_authority_preserved(hosted: &saphyr::Yaml<'_>, routed: &saphyr::Yaml<'_>) {
    assert_eq!(
        retained_fields(hosted, &["jobs"]),
        retained_fields(routed, &["jobs"]),
        "Routing must preserve the workflow triggers, permissions, and concurrency."
    );
    let hosted_jobs = field(hosted, "jobs");
    let routed_jobs = field(routed, "jobs");
    for role in [
        "publish-authority",
        "bundle-build-a",
        "bundle-attest-a",
        "bundle-publish-a",
    ] {
        assert_eq!(
            retained_job_fields(field(hosted_jobs, role)),
            retained_job_fields(field(routed_jobs, role)),
            "Routing must keep the {role} job's other fields unchanged."
        );
    }
}

fn assert_scale_set_placement(workflow: &saphyr::Yaml<'_>, role: &str) {
    let job = field(field(workflow, "jobs"), role);
    let labels = field(job, "runs-on")
        .as_vec()
        .expect("the Scale Set runner must use a label list")
        .iter()
        .map(|label| label.as_str().expect("runner labels must be strings"))
        .collect::<Vec<_>>();
    assert_eq!(labels, ["velnor", "linux-amd64"]);

    let container = field(job, "container");
    assert_eq!(
        field(container, "image").as_str(),
        Some(
            "ghcr.io/tailrocks/velnor-job@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        )
    );
    let volumes = field(container, "volumes")
        .as_vec()
        .expect("The container volume list must be a sequence.")
        .iter()
        .map(|volume| {
            volume
                .as_str()
                .expect("Each container volume must be a string.")
        })
        .collect::<Vec<_>>();
    assert_eq!(
        volumes,
        [
            "/opt/velnor/job-facts:/opt/velnor/job-facts:ro",
            "/.velnor-runtime-identity:/.velnor-runtime-identity:ro",
        ]
    );
    assert_eq!(
        field(field(field(job, "defaults"), "run"), "shell").as_str(),
        Some("bash -e {0}")
    );
}

#[test]
fn scale_set_routes_every_publication_job_and_keeps_release_authority() {
    let selector = ScaleSetSelector::try_new(
        "linux-amd64",
        &["velnor".to_owned(), "linux-amd64".to_owned()],
    )
    .expect("The selector must be valid.");
    let routing = crate::workflow_routing::test_policy(ExecutionMode::ScaleSet, selector);
    let context = ReleaseRenderContext {
        generator_version: "0.1.29".to_owned(),
        runs_on: "ubuntu-24.04".to_owned(),
    };

    let rendered = render_publish_workflow_with_routing(&spec(), &context, &routing)
        .expect("The Scale Set publication workflow must render.");
    let hosted = super::render_publish_workflow(&spec(), &context)
        .expect("The hosted publication workflow must render.");
    let hosted_document = parse(&hosted);
    let scale_set_document = parse(&rendered);
    assert_publish_authority_preserved(&hosted_document, &scale_set_document);
    for role in [
        "publish-authority",
        "bundle-build-a",
        "bundle-attest-a",
        "bundle-publish-a",
    ] {
        assert_scale_set_placement(&scale_set_document, role);
    }

    assert_eq!(
        rendered
            .bytes
            .matches("runs-on: [velnor, linux-amd64]")
            .count(),
        4
    );
    assert_eq!(
        rendered
            .bytes
            .matches("image: ghcr.io/tailrocks/velnor-job@")
            .count(),
        4
    );
    assert!(rendered.bytes.contains("tags:"));
    assert!(rendered.bytes.contains("group: publish-${{ github.ref }}"));
    assert!(rendered.bytes.contains("cancel-in-progress: false"));
    assert!(rendered.bytes.contains("environment: release"));
    assert!(!rendered.bytes.contains("actions/checkout@"));
}

#[test]
fn hosted_api_keeps_the_configured_runner_label() {
    let context = ReleaseRenderContext {
        generator_version: "0.1.29".to_owned(),
        runs_on: "ubuntu-24.04".to_owned(),
    };
    let rendered = super::render_publish_workflow(&spec(), &context)
        .expect("The hosted publication workflow must render.");
    assert_eq!(rendered.bytes.matches("runs-on: ubuntu-24.04").count(), 4);
    assert!(!rendered.bytes.contains("container:"));
}

#[test]
fn both_mode_preserves_hosted_publication_and_hosted_api_output() {
    let context = ReleaseRenderContext {
        generator_version: "0.1.29".to_owned(),
        runs_on: "ubuntu-24.04".to_owned(),
    };
    let selector = ScaleSetSelector::try_new(
        "linux-amd64",
        &["velnor".to_owned(), "linux-amd64".to_owned()],
    )
    .expect("The selector must be valid.");
    let routing = crate::workflow_routing::test_policy(ExecutionMode::Both, selector);
    let both = render_publish_workflow_with_routing(&spec(), &context, &routing)
        .expect("Execution mode \"Both\" must preserve the hosted publication workflow.");
    let hosted = super::render_publish_workflow(&spec(), &context)
        .expect("The hosted publication workflow must render.");

    assert_eq!(both.bytes, hosted.bytes);
    assert!(!both.bytes.contains("container:"));
    assert_publish_authority_preserved(&parse(&hosted), &parse(&both));
}

#[test]
fn scale_set_mode_requires_an_immutable_job_image() {
    let selector = ScaleSetSelector::try_new(
        "linux-amd64",
        &["velnor".to_owned(), "linux-amd64".to_owned()],
    )
    .expect("The selector must be valid.");
    let routing = crate::workflow_routing::WorkflowRoutingPolicy::configured(
        ExecutionMode::ScaleSet,
        selector,
        None,
    );
    let context = ReleaseRenderContext {
        generator_version: "0.1.29".to_owned(),
        runs_on: "ubuntu-24.04".to_owned(),
    };

    assert!(render_publish_workflow_with_routing(&spec(), &context, &routing).is_err());
}
