// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::verdict_world::{World, path};

#[test]
fn a_failing_artifact_listing_or_download_is_asked_once_and_leaves_reuse_off() {
    for route in [
        "/actions/runs/100/artifacts?per_page=100&page=1",
        "/actions/artifacts/5000/zip",
    ] {
        let mut world = World::new();
        let key = path(route);
        world.routes.remove(&key);
        let result = world.run();
        assert_eq!(result.outputs["reuse"], "false", "{route}");
        let asked = result
            .requests
            .iter()
            .filter(|requested| **requested == key)
            .count();
        assert_eq!(asked, 1, "{route}: {:?}", result.requests);
    }
}
