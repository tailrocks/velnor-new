// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::AllTrigger;

use crate::select::{all, assert_selects, path, two_roots};

#[test]
fn a_nextest_config_or_a_script_beside_it_selects_every_package() {
    let facts = two_roots();
    for file in [
        ".config/nextest.toml",
        ".config/nextest-ci.toml",
        ".config/nextest-prepull-images.sh",
        "helper/.config/nextest.toml",
        "helper/.config/nextest-prepull-images.sh",
    ] {
        let trigger = AllTrigger::Global(path(file));
        assert_selects(
            &facts,
            vec![(&[file], vec![all(trigger.clone()), all(trigger)])],
        );
    }
}
