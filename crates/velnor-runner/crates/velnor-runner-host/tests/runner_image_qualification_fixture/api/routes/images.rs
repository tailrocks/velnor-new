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
    let role = if image_id == state.plan.runner_id {
        ImageRole::Runner
    } else {
        ImageRole::Dind
    };
    let expected = match role {
        ImageRole::Runner => state.plan.runner_id.as_str(),
        ImageRole::Dind => state.plan.dind_id.as_str(),
    };
    let mut reported_id = expected.to_owned();
    let mut architecture = "amd64";
    let mut revision = state.plan.source_sha.clone();
    let fault_reached = match state.plan.image_fault {
        Some((target, ImageFault::Id)) if target == role => {
            reported_id = digest_id('c');
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
        Some((target, ImageFault::Reinspection)) if target == role && *count > 1 => {
            reported_id = digest_id('c');
            true
        }
        _ => false,
    };
    state.fault_reached |= fault_reached;
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
    let reply = Reply::json(
        200,
        &json!({
            "Id": reported_id,
            "Os": "linux",
            "Architecture": architecture,
            "Variant": "",
            "Config": {"Labels": {"org.opencontainers.image.revision": revision}}
        }),
    );
    if delayed {
        reply.with_delay(Duration::from_secs(6))
    } else {
        reply
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
