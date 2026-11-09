// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{Paths, Scenario};
use std::{error::Error, fs};

pub(super) fn assert_reached(
    paths: &Paths,
    scenario: Scenario,
    requests: &str,
) -> Result<(), Box<dyn Error>> {
    if !matches!(
        scenario,
        Scenario::MergePendingDeadline | Scenario::MergePendingApiDeadline
    ) {
        return Ok(());
    }
    let state = paths.root.join("provider");
    let count = fs::read_to_string(state.join("pull-count"))?.parse::<usize>()?;
    let routes = requests
        .lines()
        .filter(|line| *line == "route=repos/test-owner/test-tap/pulls/17")
        .count();
    assert!(
        count >= 2,
        "mergeability polling did not reach the second pull"
    );
    assert_eq!(routes, count);
    if matches!(scenario, Scenario::MergePendingApiDeadline) {
        assert_eq!(count, 2);
        assert_eq!(fs::read_to_string(state.join("delayed-pull-count"))?, "2");
        assert!(matches!(
            fs::symlink_metadata(state.join("delayed-pull-completed")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound
        ));
    }
    Ok(())
}
