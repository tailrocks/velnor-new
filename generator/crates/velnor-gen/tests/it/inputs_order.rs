// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::{TestResult, path};
use crate::pipeline::{view, workflow};

const MIRROR: &str = "velnor-ci crates-mirror --registry 'sparse+https://mirror.example/index/' --cainfo /opt/velnor/ca.pem";

fn trusted_before_restore(step: &serde_json::Value) -> bool {
    if let Some(uses) = step["uses"].as_str() {
        return matches!(
            uses,
            "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1"
                | "tailrocks/velnor-new/actions/setup-velnor@1793cf84b5924e2542a03b0fb2580e7395b2f4cb"
                | "actions/cache/restore@55cc8345863c7cc4c66a329aec7e433d2d1c52a9"
                | "actions/cache/save@55cc8345863c7cc4c66a329aec7e433d2d1c52a9"
                | "jdx/mise-action@f5be966e4a917d54244728e6a5fdcd1d93a35e6c"
        );
    }
    step["run"].as_str().is_some_and(|run| {
        matches!(
            run,
            "velnor-ci plan --explain"
                | "velnor-ci prepare"
                | "velnor-ci check-metadata"
                | "rustup component add clippy rustfmt"
                | "velnor-ci inputs restore --target-dir \"${GITHUB_WORKSPACE}\"/target"
                | "velnor-ci inputs restore --target-dir \"${GITHUB_WORKSPACE}\"/tools/target"
        ) || run == MIRROR
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
                .replace("0.1.79", "0.1.81")
                .into_bytes();
            let yaml: serde_json::Value = serde_saphyr::from_str(&workflow(&repo)?)?;
            let jobs = yaml["jobs"].as_object().ok_or("missing jobs")?;
            let mut restores = 0;
            for (job, body) in jobs {
                let steps = body["steps"].as_array().ok_or("missing steps")?;
                for (restore, _) in steps.iter().enumerate().filter(|(_, step)| {
                    step["run"].as_str().is_some_and(|run| {
                        run.starts_with("velnor-ci inputs restore --target-dir ")
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
