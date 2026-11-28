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

fn fingerprint_directory(path: &str) -> Option<(String, String)> {
    let (prefix, rest) = path.rsplit_once("/build/")?;
    let parts: Vec<&str> = rest.split('/').collect();
    match parts.as_slice() {
        [folder, _] => Some((
            format!("{prefix}/.fingerprint/{folder}"),
            folder.rsplit_once('-')?.1.to_owned(),
        )),
        [package, hash, _, _] => Some((
            format!("{prefix}/build/{package}/{hash}/fingerprint"),
            (*hash).to_owned(),
        )),
        _ => None,
    }
}

pub(super) fn link_script(repo: &Repo, compiled: &str, run: &str) {
    let ((compiled_directory, hash), (run_directory, _)) = fingerprint_directory(compiled)
        .zip(fingerprint_directory(run))
        .unwrap_or_else(|| unreachable!("{compiled} {run}"));
    unit(
        repo,
        &format!("{compiled_directory}/build-script-build-script-build"),
        &hash,
    );
    let digest = u64::from_str_radix(&hash, 16).unwrap_or_else(|e| unreachable!("{e}"));
    unit(
        repo,
        &format!("{run_directory}/run-build-script-build-script-build.json"),
        &format!(
            "{{\"deps\":[[0,\"build_script_build\",false,{}]]}}",
            digest.swap_bytes()
        ),
    );
}

pub(super) fn link_builds(repo: &Repo, target: &str) {
    let build = repo.root().join(target).join("build");
    let mut compiled = std::collections::BTreeMap::new();
    let mut runs = Vec::new();
    for entry in std::fs::read_dir(&build).into_iter().flatten().flatten() {
        let folder = entry.file_name().to_string_lossy().into_owned();
        let Some((package, hash)) = folder.rsplit_once('-') else {
            continue;
        };
        if entry.path().join("output").is_file() {
            runs.push((package.to_owned(), folder.clone()));
        }
        if entry
            .path()
            .join(format!("build_script_build-{hash}.d"))
            .is_file()
        {
            compiled.insert(package.to_owned(), (folder.clone(), hash.to_owned()));
        }
    }
    for (package, run) in runs {
        if let Some((folder, hash)) = compiled.get(&package) {
            link_script(
                repo,
                &format!("{target}/build/{folder}/build_script_build-{hash}.d"),
                &format!("{target}/build/{run}/output"),
            );
        }
    }
}

pub(super) fn own_script(package: &str) -> &'static str {
    OWN_UNITS
        .iter()
        .find_map(|(_, _, file, _, _)| {
            (file.starts_with(&format!("build/{package}-")) && file.contains("build_script_build-"))
                .then_some(*file)
        })
        .unwrap_or_else(|| unreachable!("{package}"))
}

pub(super) fn link_own_run(repo: &Repo, directory: &str, package: &str, run: &str) {
    link_script(
        repo,
        &format!("{directory}/{}", own_script(package)),
        &format!("{directory}/build/{run}/output"),
    );
}

pub(super) fn own_units_in(repo: &Repo, host: &str, target: &str, left_out: &[&str]) {
    placed(repo, target);
    let mut compiled = std::collections::BTreeMap::new();
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
        if file.contains("build_script_build-") {
            compiled.insert(key, format!("{directory}/{file}"));
        }
        if let Some(folder) = file.strip_suffix("/output") {
            unit(
                repo,
                &format!("{directory}/{folder}/root-output"),
                &format!("{}/{directory}/{folder}/out\n", repo.root().display()),
            );
            if let Some(script) = compiled.get(key) {
                link_script(repo, script, &format!("{directory}/{file}"));
            }
        }
        if !fingerprint.is_empty() {
            unit(repo, &format!("{directory}/.fingerprint/{fingerprint}"), "");
        }
    }
}
