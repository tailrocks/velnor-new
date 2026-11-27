// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{placed, unit};
use crate::io::Repo;

pub(super) fn own_units(repo: &Repo, target: &str, left_out: &[&str]) {
    own_units_in(repo, target, target, left_out);
}

pub(super) const OWN_UNITS: &[(&str, bool, &str, &str, &str)] = &[
    (
        "app",
        false,
        "deps/app-f000000000000001.d",
        "app/src/lib.rs",
        "app-f000000000000001/lib-app",
    ),
    (
        "app",
        false,
        "deps/app-f000000000000009.d",
        "app/src/lib.rs",
        "app-f000000000000009/test-lib-app",
    ),
    (
        "app-it",
        false,
        "deps/it-f000000000000002.d",
        "app/tests/it.rs",
        "app-f000000000000002/test-integration-test-it",
    ),
    (
        "app-build",
        true,
        "build/app-f000000000000003/build_script_build-f000000000000003.d",
        "app/build.rs",
        "",
    ),
    (
        "app-build",
        false,
        "build/app-f000000000000004/output",
        "",
        "",
    ),
    (
        "lib",
        false,
        "deps/lib-f000000000000005.d",
        "lib/src/lib.rs",
        "lib-f000000000000005/lib-lib",
    ),
    (
        "lib",
        false,
        "deps/lib-f00000000000000a.d",
        "lib/src/lib.rs",
        "lib-f00000000000000a/test-lib-lib",
    ),
    (
        "lib-it",
        false,
        "deps/it-f000000000000006.d",
        "lib/tests/it.rs",
        "lib-f000000000000006/test-integration-test-it",
    ),
    (
        "lib-build",
        true,
        "build/lib-f000000000000007/build_script_build-f000000000000007.d",
        "lib/build.rs",
        "",
    ),
    (
        "lib-build",
        false,
        "build/lib-f000000000000008/output",
        "",
        "",
    ),
    (
        "macros",
        true,
        "deps/macros-f00000000000000b.d",
        "macros/src/lib.rs",
        "macros-f00000000000000b/lib-macros",
    ),
    (
        "macros",
        true,
        "deps/macros-f00000000000000c.d",
        "macros/src/lib.rs",
        "macros-f00000000000000c/test-lib-macros",
    ),
];

pub(super) fn own_units_in(repo: &Repo, host: &str, target: &str, left_out: &[&str]) {
    placed(repo, target);
    for &(key, on_host, file, text, fingerprint) in OWN_UNITS {
        if left_out.contains(&key) {
            continue;
        }
        let directory = if on_host { host } else { target };
        let text = if text.is_empty() {
            String::new()
        } else {
            format!("out.d: {text}\n")
        };
        unit(repo, &format!("{directory}/{file}"), &text);
        if let Some(folder) = file.strip_suffix("/output") {
            unit(
                repo,
                &format!("{directory}/{folder}/root-output"),
                &format!("/velnor-test-out/{folder}/out\n"),
            );
        }
        if !fingerprint.is_empty() {
            unit(repo, &format!("{directory}/.fingerprint/{fingerprint}"), "");
        }
    }
}
