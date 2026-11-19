// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

use super::{ApiPlan, HiddenIndexMutation, OuterMutation};

impl ApiPlan {
    pub(crate) fn new(source_sha: &str) -> Self {
        Self {
            source_sha: source_sha.to_owned(),
            runner_id: digest_id('a'),
            dind_id: digest_id('b'),
            runner_version: "2.338.0".to_owned(),
            docker_version: "29.9.1".to_owned(),
            buildx_version: "0.38.0".to_owned(),
            compose_version: "5.6.0".to_owned(),
            node_version: "26.12.0".to_owned(),
            image_fault: None,
            version_mismatch: None,
            readiness_empty_first: true,
            outer_id: "outer-daemon-id".to_owned(),
            inner_id: "inner-daemon-id".to_owned(),
            inner_root: "/var/lib/docker".to_owned(),
            inner_driver: "vfs".to_owned(),
            outer_root: "/var/lib/docker".to_owned(),
            runner_socket_id: None,
            outer_mutation: OuterMutation::None,
            mutate_lock_on_first_image_inspect: None,
            mutate_source_on_first_image_inspect: None,
            mutate_hidden_index_on_first_image_inspect: None,
            fault: None,
            foreign_sentinel: false,
        }
    }

    pub(crate) fn mutate_skip_worktree_on_first_image_inspect(&mut self, root: PathBuf) {
        self.mutate_hidden_index_on_first_image_inspect =
            Some(HiddenIndexMutation::SkipWorktree(root));
    }

    pub(crate) fn mutate_assume_unchanged_on_first_image_inspect(&mut self, root: PathBuf) {
        self.mutate_hidden_index_on_first_image_inspect =
            Some(HiddenIndexMutation::AssumeUnchanged(root));
    }
}

pub(crate) fn digest_id(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}
