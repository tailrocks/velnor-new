// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;
use velnor_actions_contract::{JobTimeout, Step, StepKind};
use velnor_actions_workflow_renderer::{
    publish_render::render_publish_workflow,
    publish_spec::{
        PublishJobSpec, PublishPermissions, PublishRole, PublishStep, PublishTriggers,
        PublishWorkflowSpec,
    },
    release_tree::ReleaseRenderContext,
};

fn job(
    role: PublishRole,
    needs: Vec<String>,
    steps: Vec<PublishStep>,
) -> Result<PublishJobSpec, Box<dyn Error>> {
    Ok(PublishJobSpec {
        role,
        display_name: role.as_str().to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        timeout_minutes: JobTimeout::try_from(30)?,
        needs,
        condition: None,
        environment: (role == PublishRole::NativeImagePublish)
            .then(|| "image-publication".to_owned()),
        outputs: BTreeMap::new(),
        permissions: PublishPermissions::expected(role),
        steps,
        services: vec![],
    })
}
fn shell() -> Step {
    Step {
        name: "Initialize".to_owned(),
        id: None,
        role: None,
        condition: None,
        parallel: false,
        kind: StepKind::Shell {
            run: vec!["true".to_owned()],
            env: BTreeMap::new(),
        },
    }
}
fn spec() -> Result<PublishWorkflowSpec, Box<dyn Error>> {
    let authority = job(
        PublishRole::BranchAuthority,
        vec![],
        vec![PublishStep {
            id: None,
            step: shell(),
        }],
    )?;
    let operation = Step {
        name: "Publish authenticated native image".to_owned(),
        id: None,
        role: None,
        condition: None,
        parallel: false,
        kind: StepKind::Internal {
            operation: "native-image-publish-v1".to_owned(),
            env: BTreeMap::from([
                ("IMAGE_PROFILE".to_owned(), "runner".to_owned()),
                (
                    "IMAGE_REPOSITORY".to_owned(),
                    "ghcr.io/tailrocks/velnor-runner".to_owned(),
                ),
                ("GH_VERSION".to_owned(), "2.103.3".to_owned()),
                ("CRANE_VERSION".to_owned(), "0.22.1".to_owned()),
            ]),
        },
    };
    let image = job(
        PublishRole::NativeImagePublish,
        vec!["authority".to_owned()],
        vec![
            PublishStep {
                id: None,
                step: shell(),
            },
            PublishStep {
                id: Some("native-push".to_owned()),
                step: operation,
            },
        ],
    )?;
    Ok(PublishWorkflowSpec {
        name: "Publish".to_owned(),
        repository: "tailrocks/velnor-new".to_owned(),
        triggers: PublishTriggers {
            branches: vec!["main".to_owned()],
            tag_glob: None,
        },
        jobs: BTreeMap::from([
            ("authority".to_owned(), authority),
            ("runner".to_owned(), image),
        ]),
        staged_binary: None,
    })
}
#[test]
fn native_publisher_renders_only_the_fixed_trusted_operation() -> Result<(), Box<dyn Error>> {
    let ctx = ReleaseRenderContext {
        generator_version: "0.1.75".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
    };
    let mut request = spec()?;
    let workflow = render_publish_workflow(&request, &ctx)?.bytes;
    for expected in [
        "Acquire independently verified exact-source SDK preview",
        "image-maintenance publish-image",
        "velnor-image-publication-",
        "queue: max",
        "actions/runs/",
    ] {
        assert!(workflow.contains(expected), "missing {expected}");
    }
    assert!(!workflow.contains("actions/checkout"));
    let job = request.jobs.get_mut("runner").ok_or("runner")?;
    let StepKind::Internal { env, .. } = &mut job.steps[1].step.kind else {
        return Err("operation".into());
    };
    env.insert(
        "IMAGE_PROFILE".to_owned(),
        "runner; touch /tmp/unsafe".to_owned(),
    );
    assert!(render_publish_workflow(&request, &ctx).is_err());
    Ok(())
}

#[test]
fn native_promotion_renders_only_the_fixed_digest_operation() -> Result<(), Box<dyn Error>> {
    let ctx = ReleaseRenderContext {
        generator_version: "0.1.75".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
    };
    let mut request = spec()?;
    let authority = request.jobs.get_mut("authority").ok_or("authority")?;
    authority.role = PublishRole::Authority;
    authority.permissions = PublishPermissions::expected(PublishRole::Authority);
    let image = request.jobs.get_mut("runner").ok_or("runner")?;
    image.role = PublishRole::NativeImagePromote;
    image.permissions = PublishPermissions::expected(PublishRole::NativeImagePromote);
    image.steps[1].step.name = "Promote authenticated native image digest".to_owned();
    let StepKind::Internal { operation, .. } = &mut image.steps[1].step.kind else {
        return Err("operation".into());
    };
    *operation = "native-image-promote-v1".to_owned();
    let workflow = render_publish_workflow(&request, &ctx)?.bytes;
    assert!(workflow.contains("image-maintenance promote-image"));
    assert!(workflow.contains("Acquire independently verified exact-source SDK preview"));
    assert!(!workflow.contains("actions/checkout"));
    assert!(!workflow.contains("docker build"));
    Ok(())
}
