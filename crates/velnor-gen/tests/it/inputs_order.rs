// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::RepoPath;

use crate::facts::{TestResult, path};
use crate::freshness::named;
use crate::lock::locked;
use crate::pipeline::{view, workflow};

const MIRROR: &str = "velnor-ci crates-mirror --registry 'sparse+https://mirror.example/index/' --cainfo /opt/velnor/ca.pem";

fn plain(run: &str) -> String {
    match run.strip_prefix(". \"$VELNOR_EXEC\" ") {
        Some(rest) => format!("velnor-ci {rest}"),
        None => run.to_owned(),
    }
}

fn trusted_before_restore(step: &serde_json::Value) -> bool {
    if step["id"] == "sources_locks" {
        return true;
    }
    if let Some(uses) = step["uses"].as_str() {
        return matches!(
            uses,
            "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1"
                | "tailrocks/velnor-new/actions/setup-velnor@1390e6f6a4b64e1f33dbddfbeedcd097db32ec36"
                | "actions/cache/restore@55cc8345863c7cc4c66a329aec7e433d2d1c52a9"
                | "actions/cache/save@55cc8345863c7cc4c66a329aec7e433d2d1c52a9"
                | "jdx/mise-action@236a21c3b91f5db6d4e174b348674a0f0caac06d"
        );
    }
    step["run"].as_str().map(plain).is_some_and(|run| {
        matches!(
            run.as_str(),
            "eval \"$VELNOR_HELPER_INSTALL_TOOLS\""
                | "eval \"$VELNOR_HELPER_INSTALL_LAUNCHER\""
                | "velnor-ci plan --explain"
                | "velnor-ci prepare"
                | "echo \"MBX_CACHE_DIR=$RUNNER_TEMP/mbx\" >> \"$GITHUB_ENV\""
                | "velnor-ci check-metadata"
                | "rustup component add clippy rustfmt"
        ) || run == MIRROR
            || run.starts_with("velnor-ci delta prepare ")
            || run.starts_with("velnor-ci delta apply ")
    })
}

#[test]
fn released_helper_restores_inputs_after_checkout_and_cache_before_repository_code() -> TestResult {
    for shape in ["one-job", "lint-test", "groups"] {
        for mirror in [false, true] {
            let config = format!(
                "shape = '{shape}'\n{}",
                if mirror {
                    "runner = 'mirror-box'\n[crates_mirror]\nrunner = 'mirror-box'\nregistry = 'sparse+https://mirror.example/index/'\ncainfo = '/opt/velnor/ca.pem'\n"
                } else {
                    ""
                }
            );
            let mut repo = view(&["Cargo.toml", "tools/Cargo.toml"], Some(config.as_bytes()))?;
            let lock = repo
                .files
                .get_mut(&path(".velnor/rust-ci.lock")?)
                .ok_or("missing helper lock")?;
            *lock = String::from_utf8(lock.clone())?
                .replace("0.1.81", "0.1.97")
                .into_bytes();
            let yaml: serde_json::Value = serde_saphyr::from_str(&workflow(&repo)?)?;
            let jobs = yaml["jobs"].as_object().ok_or("missing jobs")?;
            let mut restores = 0;
            for (job, body) in jobs {
                let steps = body["steps"].as_array().ok_or("missing steps")?;
                for (restore, _) in steps.iter().enumerate().filter(|(_, step)| {
                    step["run"].as_str().map(plain).is_some_and(|run| {
                        run.starts_with("velnor-ci inputs restore-all --root-target ")
                    })
                }) {
                    restores += 1;
                    assert!(
                        steps[0]["uses"]
                            .as_str()
                            .is_some_and(|uses| uses.starts_with("actions/checkout@")),
                        "{shape}: {job}: {steps:?}"
                    );
                    assert!(steps[..restore].iter().enumerate().any(|(index, step)| {
                        index > 0 && step["name"] == "Restore the target cache"
                            && step["uses"] == "actions/cache/restore@55cc8345863c7cc4c66a329aec7e433d2d1c52a9"
                    }), "{shape}: {job}: target cache must precede inputs restore");
                    for step in &steps[..restore] {
                        assert!(
                            trusted_before_restore(step),
                            "{shape}: {job}: repository-controlled step before inputs restore: {step}"
                        );
                    }
                }
            }
            assert!(restores >= 2, "{shape}: no root inputs restores");
        }
    }
    Ok(())
}

#[test]
fn custom_target_cache_and_inputs_restore_use_the_configured_directory() -> TestResult {
    let mut repo = locked("0.1.97")?;
    let path = RepoPath::new(".cargo/config.toml")?;
    repo.tracked_paths.insert(path.clone());
    repo.files
        .insert(path, b"[build]\ntarget-dir = \"custom-target\"\n".to_vec());
    let text = workflow(&repo)?;
    let caches = named(&text, "Restore the target cache");
    let restores = named(&text, "velnor-ci inputs restore");
    assert_ne!(caches.len(), 0);
    assert_ne!(restores.len(), 0);
    assert!(
        caches
            .iter()
            .all(|step| step.contains("path: custom-target\n"))
    );
    for step in restores {
        let command = step
            .lines()
            .find_map(|line| line.trim().strip_prefix("run: "))
            .ok_or("no command")?;
        let yaml = crate::yaml_tree::parse(&format!("run: {command}"))?;
        assert_eq!(
            yaml.get("run").and_then(crate::yaml_tree::Node::text),
            Some(
                ". \"$VELNOR_EXEC\" inputs restore-all --root-target \"${GITHUB_WORKSPACE}\"/. \"${GITHUB_WORKSPACE}\"/custom-target --scope-cache \"${RUNNER_TEMP}/velnor-input-scopes-${GITHUB_JOB}.json\""
            )
        );
    }
    Ok(())
}

#[test]
fn inputs_restore_uses_each_cached_target_directory_from_any_working_directory() -> TestResult {
    let mut repo = locked("0.1.97")?;
    let more = view(&["Cargo.toml", "lab/Cargo.toml"], None)?;
    repo.rust_metadata = more.rust_metadata;
    repo.tracked_paths.extend(more.tracked_paths);
    for (path, text) in [
        (
            ".cargo/config.toml",
            "[build]\ntarget-dir = \"cache dir\"\n",
        ),
        (
            "lab/.cargo/config.toml",
            "[build]\ntarget-dir = \"../lab's target\"\n",
        ),
    ] {
        let path = RepoPath::new(path)?;
        repo.tracked_paths.insert(path.clone());
        repo.files.insert(path, text.as_bytes().to_vec());
    }
    let text = workflow(&repo)?;
    let restores = named(&text, "velnor-ci inputs restore");
    assert_eq!(restores.len(), 3, "{text}");
    for target in ["'cache dir'", "'lab'\\''s target'"] {
        let matching: Vec<_> = restores
            .iter()
            .filter(|step| {
                step.contains("--root-target")
                    && step.contains(if target == "'cache dir'" {
                        "cache dir"
                    } else {
                        "lab"
                    })
            })
            .collect();
        assert_eq!(matching.len(), 2, "{text}");
        let step = matching[0];
        let command = step
            .lines()
            .find_map(|line| line.trim().strip_prefix("run: "))
            .ok_or("no command")?;
        let yaml = crate::yaml_tree::parse(&format!("run: {command}"))?;
        let command = yaml
            .get("run")
            .and_then(crate::yaml_tree::Node::text)
            .ok_or("invalid scalar")?;
        assert_eq!(
            command,
            format!(
                ". \"$VELNOR_EXEC\" inputs restore-all --root-target \"${{GITHUB_WORKSPACE}}\"/{} \"${{GITHUB_WORKSPACE}}\"/{target} --scope-cache \"${{RUNNER_TEMP}}/velnor-input-scopes-${{GITHUB_JOB}}.json\"",
                if target == "'cache dir'" { "." } else { "lab" }
            )
        );
    }
    Ok(())
}
