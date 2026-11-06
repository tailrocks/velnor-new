// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::time::Duration;

use serde_json::json;

use super::super::http::{Reply, Request};
use super::super::state::{ApiFault, HiddenIndexMutation, ImageFault, ImageRole, State, digest_id};
use super::{missing, path_id};

pub(super) fn route(request: &Request, path: &str, state: &mut State) -> Option<Reply> {
    match (request.method.as_str(), path) {
        ("GET", "version") => {
            let changed = state.outer_info_calls > 0
                && state.plan.outer_mutation == super::super::state::OuterMutation::Version;
            state.fault_reached |= changed;
            let version = if changed {
                "99.0.0"
            } else {
                state.plan.docker_version.as_str()
            };
            Some(Reply::json(
                200,
                &json!({"Version": version, "ApiVersion": "1.45"}),
            ))
        }
        ("GET", "info") => Some(info(state)),
        ("GET", _) if path.starts_with("images/") && path.ends_with("/json") => {
            Some(inspect_image(path, state))
        }
        _ => None,
    }
}

fn info(state: &mut State) -> Reply {
    state.outer_info_calls += 1;
    let changed = state.outer_info_calls > 1;
    let mutation = state.plan.outer_mutation;
    state.fault_reached |= changed && mutation != super::super::state::OuterMutation::None;
    let id = if changed && mutation == super::super::state::OuterMutation::Id {
        "changed-outer-daemon-id"
    } else {
        state.plan.outer_id.as_str()
    };
    let root = if changed && mutation == super::super::state::OuterMutation::Root {
        "/changed/docker/root"
    } else {
        state.plan.outer_root.as_str()
    };
    Reply::json(200, &json!({"ID": id, "DockerRootDir": root}))
}

fn inspect_image(path: &str, state: &mut State) -> Reply {
    let Some(image_id) = path_id(path, "images/", "/json") else {
        return missing("image");
    };
    if !state.images.contains(&image_id) {
        return missing("image");
    }
    let delayed = state.take_fault(ApiFault::StallImageInspect);
    let count = state.image_inspections.entry(image_id.clone()).or_default();
    *count += 1;
    let count = *count;
    let role = image_role(&image_id, &state.plan.runner_id, &state.plan.dind_id);
    let expected = match role {
        ImageRole::Runner => state.plan.runner_id.as_str(),
        ImageRole::Dind => state.plan.dind_id.as_str(),
        ImageRole::ResourceProbe => state.plan.probe_id.as_str(),
    };
    let mut reported_id = expected.to_owned();
    let mut architecture = "amd64";
    let mut revision = state.plan.source_sha.clone();
    let mut working_dir = "/";
    let mut image_kind = match role {
        ImageRole::Runner => "runner",
        ImageRole::Dind => "dind",
        ImageRole::ResourceProbe => "resource-probe",
    };
    let (user, entrypoint, runtime_fault) = runtime_config(role, state.plan.image_fault);
    let fault_reached = match state.plan.image_fault {
        Some((target, ImageFault::Id)) if target == role => {
            reported_id = digest_id('d');
            true
        }
        Some((target, ImageFault::Platform)) if target == role => {
            architecture = "arm64";
            true
        }
        Some((target, ImageFault::Source)) if target == role => {
            revision = "f".repeat(40);
            true
        }
        Some((target, ImageFault::Kind)) if target == role => {
            image_kind = wrong_image_kind(role);
            true
        }
        Some((target, ImageFault::WorkingDir)) if target == role => {
            working_dir = "/unexpected";
            true
        }
        Some((target, ImageFault::Reinspection)) if target == role && count > 1 => {
            reported_id = digest_id('d');
            true
        }
        _ => false,
    };
    let (descriptor, descriptor_fault) = image_descriptor(
        &image_id,
        role,
        state.plan.manifest_store,
        state.plan.image_fault,
        count,
    );
    state.fault_reached |= fault_reached || runtime_fault || descriptor_fault;
    mutate_source(state);
    let value = json!({
        "Id": reported_id,
        "Descriptor": descriptor,
        "Os": "linux",
        "Architecture": architecture,
        "Variant": "",
        "Config": {"User": user, "WorkingDir": working_dir, "Entrypoint": entrypoint.map(|value| vec![value]), "Labels": {
            "org.opencontainers.image.revision": revision,
            "io.velnor.image.kind": image_kind
        }}
    });
    let reply = Reply::json(200, &value);
    if delayed {
        reply.with_delay(Duration::from_secs(6))
    } else {
        reply
    }
}

fn image_role(image_id: &str, runner_id: &str, dind_id: &str) -> ImageRole {
    if image_id == runner_id {
        ImageRole::Runner
    } else if image_id == dind_id {
        ImageRole::Dind
    } else {
        ImageRole::ResourceProbe
    }
}

fn image_descriptor(
    image_id: &str,
    role: ImageRole,
    manifest_store: Option<()>,
    fault: Option<(ImageRole, ImageFault)>,
    count: usize,
) -> (serde_json::Value, bool) {
    let mut descriptor = if manifest_store.is_some() {
        json!({"mediaType":"application/vnd.oci.image.manifest.v1+json", "digest":image_id, "size":123})
    } else {
        serde_json::Value::Null
    };
    let reached = match fault {
        Some((target, ImageFault::DescriptorDigest)) if target == role => {
            descriptor["digest"] = json!(digest_id('e'));
            true
        }
        Some((target, ImageFault::DescriptorPlatform)) if target == role => {
            descriptor["platform"] = json!({"os":"linux", "architecture":"arm64"});
            true
        }
        Some((target, ImageFault::DescriptorReinspection)) if target == role && count > 1 => {
            descriptor = serde_json::Value::Null;
            true
        }
        _ => false,
    };
    (descriptor, reached)
}

fn mutate_source(state: &mut State) {
    if let Some(lock) = state.plan.mutate_lock_on_first_image_inspect.take() {
        state.fault_reached = true;
        if let Ok(mut bytes) = fs::read(&lock) {
            bytes.extend_from_slice(b"\nfixture_mutation = true\n");
            drop(fs::write(lock, bytes));
        }
    }
    if let Some(source_file) = state.plan.mutate_source_on_first_image_inspect.take() {
        state.fault_reached = true;
        drop(fs::write(source_file, "changed during qualification\n"));
    }
    mutate_hidden_index_on_first_image_inspect(state);
}

fn runtime_config(
    role: ImageRole,
    fault: Option<(ImageRole, ImageFault)>,
) -> (Option<&'static str>, Option<&'static str>, bool) {
    let (user, entrypoint) = match role {
        ImageRole::Runner => ("runner", "/usr/local/bin/velnor-runner-entrypoint"),
        ImageRole::Dind => ("", "/usr/local/bin/velnor-dind-entrypoint"),
        ImageRole::ResourceProbe => ("65532:65532", "/velnor/resource-probe"),
    };
    match fault {
        Some((target, ImageFault::User)) if target == role => {
            (Some("unexpected"), Some(entrypoint), true)
        }
        Some((target, ImageFault::MissingUser)) if target == role => (None, Some(entrypoint), true),
        Some((target, ImageFault::Entrypoint)) if target == role => {
            (Some(user), Some("/unexpected"), true)
        }
        Some((target, ImageFault::MissingEntrypoint)) if target == role => (Some(user), None, true),
        _ => (Some(user), Some(entrypoint), false),
    }
}

fn wrong_image_kind(role: ImageRole) -> &'static str {
    match role {
        ImageRole::Runner => "dind",
        ImageRole::Dind | ImageRole::ResourceProbe => "runner",
    }
}

fn mutate_hidden_index_on_first_image_inspect(state: &mut State) {
    let Some(mutation) = state.plan.mutate_hidden_index_on_first_image_inspect.take() else {
        return;
    };
    state.fault_reached = true;
    let result = match mutation {
        HiddenIndexMutation::SkipWorktree(root) => super::super::super::git::mutate_tracked_file(
            &root,
            super::super::super::git::HiddenIndexFlag::SkipWorktree,
        ),
        HiddenIndexMutation::AssumeUnchanged(root) => {
            super::super::super::git::mutate_tracked_file(
                &root,
                super::super::super::git::HiddenIndexFlag::AssumeUnchanged,
            )
        }
    };
    state.hidden_index_mutation = Some(result.map_err(|error| error.to_string()));
}
