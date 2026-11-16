// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::base_save::{jobs, names, pinned, step};
use crate::facts::TestResult;
use crate::pipeline::workflow;
use velnor_gen::expected_files;
use velnor_model::{CacheKey, CacheRoot, Host, JobId, RepoPath};
use velnor_render::render_key;

#[test]
fn pr_delta_keys_preserve_every_identity_dimension_in_every_restore_fallback() -> TestResult {
    let key = render_key(&CacheKey::Delta {
        job: JobId::new("rust-test")?,
        root: CacheRoot::new(
            &RepoPath::new("lab/Cargo.toml")?,
            RepoPath::new("lab/target")?,
        ),
        host: Host,
    });
    for value in std::iter::once(&key.key).chain(&key.restore_keys) {
        assert!(value.starts_with("velnor-dlt-"), "{value}");
        for required in [
            "pr${{ github.event.pull_request.number }}",
            "rust-test.lab.${{ runner.os }}-${{ runner.arch }}",
            "${{ steps.prepare.outputs.compat }}",
            "${{ steps.delta_base_lab.outputs.base8 }}",
        ] {
            assert!(value.contains(required), "{value}");
        }
    }
    assert_eq!(key.restore_keys.len(), 1);
    Ok(())
}

#[test]
fn supported_lanes_restore_pr_deltas_then_save_after_the_last_compiling_step() -> TestResult {
    for shape in ["", "shape = \"lint-test\"", "shape = \"groups\""] {
        for (id, lane) in jobs("0.1.81", shape)? {
            if !lane.contains("cache decide") {
                continue;
            }
            let order = names(&lane);
            let at = |name: &str| {
                order
                    .iter()
                    .position(|value| value == name)
                    .ok_or_else(|| format!("{id} missing {name}"))
            };
            assert!(at("Restore the target cache")? < at("velnor-ci delta prepare")?);
            assert!(at("velnor-ci delta prepare")? < at("Restore the delta cache")?);
            assert!(at("Restore the delta cache")? < at("velnor-ci delta apply")?);
            assert!(at("velnor-ci delta apply")? < at("velnor-ci inputs restore")?);
            assert!(
                lane.contains("steps.target_root.outputs.cache-matched-key"),
                "{lane}"
            );
            for name in [
                "velnor-ci delta prepare",
                "Restore the delta cache",
                "velnor-ci delta apply",
                "velnor-ci delta stage",
                "Save the delta cache",
            ] {
                let value = step(&lane, name).ok_or("no step")?;
                assert!(
                    value.contains("github.event_name == ''pull_request''"),
                    "{value}"
                );
                if name == "Save the delta cache" || name == "velnor-ci delta stage" {
                    assert!(
                        value.contains("head.repo.full_name == github.repository"),
                        "{value}"
                    );
                }
            }
            let stage = at("velnor-ci delta stage")?;
            assert_eq!(order[stage - 1], "velnor-ci inputs save");
            assert_eq!(order[stage + 1], "Save the delta cache");
            let rendered = crate::base_save::steps(&lane);
            assert!(
                rendered[stage + 2..]
                    .iter()
                    .all(|step| !step.contains("run: velnor-ci run -- cargo")),
                "{lane}"
            );
            assert!(!lane.contains("actions: write"), "{lane}");
            assert!(
                step(&lane, "Save the target cache").is_some_and(|s| !s.contains("pull_request"))
            );
            assert!(step(&lane, "velnor-ci delta stage").is_some_and(|s| s.contains("--package")));
        }
    }
    let mut custom = pinned("0.1.81", "")?;
    let config = RepoPath::new(".cargo/config.toml")?;
    custom.tracked_paths.insert(config.clone());
    custom
        .files
        .insert(config, b"[build]\ntarget-dir = 'build'\n".to_vec());
    let text = workflow(&custom)?;
    assert!(text.contains("path: build"), "{text}");
    assert!(!text.contains("velnor-ci delta"), "{text}");
    assert!(!text.contains("the delta cache"), "{text}");
    Ok(())
}

#[test]
fn cleanup_uses_only_a_trusted_locked_helper_and_old_helpers_get_no_delta_flags() -> TestResult {
    for (version, source) in [
        ("0.1.80", false),
        ("0.1.81", false),
        ("0.1.80", true),
        ("0.1.81", true),
    ] {
        let mut repo = pinned(version, "")?;
        if source {
            let lock = repo
                .files
                .get(&velnor_rust::LOCK_PATH)
                .ok_or("no lock")?
                .clone();
            repo = crate::golden::load(
                &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../samples/helper-source"),
            )?;
            repo.files.insert(velnor_rust::LOCK_PATH, lock);
        }
        let text = workflow(&repo)?;
        let files = expected_files(&repo)?;
        let cleanup = files.get(&velnor_render::DELTA_CLEANUP_PATH);
        if version == "0.1.80" {
            assert!(!text.contains("velnor-ci delta"));
            assert!(cleanup.is_none());
            continue;
        }
        let cleanup = cleanup.ok_or("no cleanup")?;
        assert!(cleanup.contains("workflow_run:"), "{cleanup}");
        assert!(cleanup.contains("- Rust CI"), "{cleanup}");
        assert!(cleanup.contains("- completed"), "{cleanup}");
        assert!(cleanup.contains("  rust-trim:"), "{cleanup}");
        assert!(cleanup.contains("actions: write"), "{cleanup}");
        assert!(
            cleanup.contains(
                "github.event.workflow_run.head_repository.id == github.event.repository.id"
            ),
            "{cleanup}"
        );
        assert!(cleanup.contains("Install velnor-ci 0.1.81"), "{cleanup}");
        assert!(
            cleanup.contains("velnor-ci cache trim-deltas --run-id \"$RUN_ID\""),
            "{cleanup}"
        );
        for forbidden in [
            "checkout@",
            "download-artifact@",
            "cache/restore@",
            "cargo ",
            "HelperFromArtifact",
            "head_sha",
        ] {
            assert!(!cleanup.contains(forbidden), "{forbidden}: {cleanup}");
        }
    }
    Ok(())
}
