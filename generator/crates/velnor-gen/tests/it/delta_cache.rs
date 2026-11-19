// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::base_save::{jobs, names, pinned, step};
use crate::facts::TestResult;
use crate::pipeline::workflow;
use velnor_gen::expected_files;
use velnor_model::{CacheKey, CacheRoot, Host, JobId, RepoPath, RepoView};
use velnor_render::render_key;

fn source_repo(version: &str) -> Result<RepoView, Box<dyn std::error::Error>> {
    let lock = pinned(version, config_for(version))?
        .files
        .get(&velnor_rust::LOCK_PATH)
        .ok_or("no lock")?
        .clone();
    let mut repo = crate::golden::load(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/helper-source"),
    )?;
    repo.files.insert(velnor_rust::LOCK_PATH, lock);
    let config = repo
        .files
        .get_mut(&velnor_rust::CONFIG_PATH)
        .ok_or("no config")?;
    config.extend_from_slice(config_for(version).as_bytes());
    Ok(repo)
}

#[test]
fn a_source_helper_locked_below_the_restoring_release_is_refused() -> TestResult {
    let error = expected_files(&source_repo("0.1.80")?)
        .err()
        .ok_or("accepted")?;
    let message = error.to_string();
    assert!(
        message.contains("older than this generator supports"),
        "{message}"
    );
    assert!(message.contains("0.1.80"), "{message}");
    assert!(message.contains("0.1.81"), "{message}");
    Ok(())
}

fn config_for(version: &str) -> &'static str {
    if version == "0.1.80" {
        "on_main = \"affected\"\nfull_run = \"off\"\n"
    } else {
        ""
    }
}

/// The oldest helper the generator accepts: it has no delta command, so the lane gets no delta steps.
const OLDEST_HELPER: &str = "0.1.81";

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
        for (id, lane) in jobs("0.1.95", shape)? {
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
                assert!(
                    !value.contains("github.event.repository.private"),
                    "{value}"
                );
                let condition = value
                    .lines()
                    .find(|line| line.trim_start().starts_with("if: "))
                    .ok_or("no delta condition")?;
                if name == "Restore the delta cache" {
                    assert!(
                        condition.contains("steps.target_root.outputs.cache-matched-key != ''''"),
                        "{condition}"
                    );
                } else {
                    assert!(!condition.contains("cache-matched-key"), "{condition}");
                }
                if name == "Save the delta cache" || name == "velnor-ci delta stage" {
                    assert!(
                        value.contains("head.repo.full_name == github.repository"),
                        "{value}"
                    );
                }
            }
            assert_delta_saves_are_not_cancelled(&lane)?;
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
            assert!(
                step(&lane, "velnor-ci delta stage")
                    .is_some_and(|s| s.contains("VELNOR_DELTA_PACKAGES:"))
            );
            source_identity(&lane)?;
        }
    }
    private_sources_disable_deltas()?;
    custom_targets_omit_deltas()?;
    Ok(())
}

fn custom_targets_omit_deltas() -> TestResult {
    let mut custom = pinned("0.1.95", "")?;
    let config = RepoPath::new(".cargo/config.toml")?;
    custom.tracked_paths.insert(config.clone());
    custom
        .files
        .insert(config, b"[build]\ntarget-dir = 'build'\n".to_vec());
    let text = workflow(&custom)?;
    assert!(text.contains("path: build"), "{text}");
    assert!(!text.contains("velnor-ci delta"), "{text}");
    assert!(!text.contains("the delta cache"), "{text}");
    assert!(!expected_files(&custom)?.contains_key(&velnor_render::DELTA_CLEANUP_PATH));
    Ok(())
}

fn assert_delta_saves_are_not_cancelled(lane: &str) -> TestResult {
    let rendered_steps = crate::base_save::steps(lane);
    let guarded_delta_saves: Vec<_> = rendered_steps
        .iter()
        .filter(|rendered| rendered.contains("head.repo.full_name == github.repository"))
        .collect();
    assert_eq!(guarded_delta_saves.len(), 3, "{lane}");
    for rendered in guarded_delta_saves {
        let condition = rendered
            .lines()
            .find(|line| line.trim_start().starts_with("if: "))
            .ok_or("no guarded delta condition")?;
        assert!(condition.contains("!cancelled()"), "{rendered}");
    }
    Ok(())
}

fn private_sources_disable_deltas() -> TestResult {
    let mut private_sources = pinned("0.1.95", "")?;
    private_sources.files.insert(
        RepoPath::new("Cargo.lock")?,
        b"version = 4\n\n[[package]]\nname = \"dep\"\nversion = \"1.0.0\"\nsource = \"git+https://github.com/example/private#0123456789abcdef\"\n".to_vec(),
    );
    let text = workflow(&private_sources)?;
    for forbidden in [
        "velnor-ci delta",
        "the delta cache",
        "velnor-dlt-",
        ".velnor-delta",
    ] {
        assert!(!text.contains(forbidden), "{text}");
    }
    assert!(!expected_files(&private_sources)?.contains_key(&velnor_render::DELTA_CLEANUP_PATH));
    Ok(())
}

#[test]
fn delta_staging_rejects_tracked_files_only_when_the_lane_uses_it() -> TestResult {
    for path in ["target.velnor-delta", "target.velnor-delta/owned"] {
        let path = RepoPath::new(path)?;
        let mut repo = pinned("0.1.95", "")?;
        repo.tracked_paths.insert(path.clone());
        repo.files.insert(path.clone(), b"project data".to_vec());
        let error = expected_files(&repo).expect_err("tracked staging must not be overwritten");
        assert_eq!(
            error.to_string(),
            format!("tracked path {path} collides with delta cache staging")
        );
    }
    for path in [
        "target.velnor-delta-other",
        "target.velnor-delta-other/owned",
    ] {
        let path = RepoPath::new(path)?;
        let mut repo = pinned("0.1.95", "")?;
        repo.tracked_paths.insert(path.clone());
        repo.files.insert(path, b"project data".to_vec());
        assert!(workflow(&repo)?.contains("Restore the delta cache"));
    }
    for unsupported in ["old-helper", "private-source", "custom-target"] {
        let version = if unsupported == "old-helper" {
            OLDEST_HELPER
        } else {
            "0.1.95"
        };
        let mut repo = pinned(version, config_for(version))?;
        let path = RepoPath::new(if unsupported == "custom-target" {
            "build.velnor-delta/owned"
        } else {
            "target.velnor-delta/owned"
        })?;
        repo.tracked_paths.insert(path.clone());
        repo.files.insert(path, b"project data".to_vec());
        if unsupported == "private-source" {
            repo.files.insert(
                RepoPath::new("Cargo.lock")?,
                b"version = 4\n\n[[package]]\nname = \"dep\"\nversion = \"1.0.0\"\nsource = \"git+https://github.com/example/private#0123456789abcdef\"\n".to_vec(),
            );
        }
        if unsupported == "custom-target" {
            let config = RepoPath::new(".cargo/config.toml")?;
            repo.tracked_paths.insert(config.clone());
            repo.files
                .insert(config, b"[build]\ntarget-dir = 'build'\n".to_vec());
        }
        assert!(
            !workflow(&repo)?.contains("Restore the delta cache"),
            "{unsupported}"
        );
    }
    Ok(())
}

#[test]
fn cleanup_uses_only_a_trusted_locked_helper_and_old_helpers_get_no_delta_flags() -> TestResult {
    for (version, source) in [(OLDEST_HELPER, false), ("0.1.95", false), ("0.1.95", true)] {
        let repo = if source {
            source_repo(version)?
        } else {
            pinned(version, config_for(version))?
        };
        let text = workflow(&repo)?;
        let files = expected_files(&repo)?;
        let cleanup = files.get(&velnor_render::DELTA_CLEANUP_PATH);
        if version == OLDEST_HELPER {
            assert!(!text.contains("velnor-ci delta"));
            assert!(cleanup.is_none());
            continue;
        }
        let cleanup = cleanup.ok_or("no cleanup")?;
        assert!(cleanup.contains("workflow_run:"), "{cleanup}");
        assert!(
            cleanup.contains("  # jactionlint ignore=dangerous-triggers\n  workflow_run:"),
            "{cleanup}"
        );
        assert!(
            cleanup.contains("group: 'velnor-pr-delta-cleanup-${{ github.event.workflow_run.pull_requests[0].number || github.event.workflow_run.id }}'"),
            "{cleanup}"
        );
        assert!(cleanup.contains("cancel-in-progress: false"), "{cleanup}");
        assert!(cleanup.contains("- Rust CI"), "{cleanup}");
        assert!(cleanup.contains("- completed"), "{cleanup}");
        assert!(cleanup.contains("  rust-trim:"), "{cleanup}");
        assert!(cleanup.contains("actions: write"), "{cleanup}");
        let condition = cleanup
            .lines()
            .find(|line| line.starts_with("    if: "))
            .ok_or("no cleanup condition")?;
        assert!(
            condition
                .contains("github.event.workflow_run.path == ''.github/workflows/rust-ci.yml''"),
            "{condition}"
        );
        assert!(
            cleanup.contains(
                "github.event.workflow_run.head_repository.id == github.event.repository.id"
            ),
            "{cleanup}"
        );
        assert!(cleanup.contains("Install velnor-ci 0.1.95"), "{cleanup}");
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

#[test]
fn a_workspace_with_many_crates_stages_its_delta_through_one_package_variable() -> TestResult {
    let repo = crate::golden::load(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/lab-150"),
    )?;
    let repo = with_helper(repo, "0.1.93", "0.1.95")?;
    let files = expected_files(&repo)?;
    let text = files
        .get(&RepoPath::new(".github/workflows/rust-ci.yml")?)
        .ok_or("no rust-ci workflow")?;
    let stage = step(text, "velnor-ci delta stage").ok_or("no delta stage")?;
    assert!(!stage.contains("--package"), "{stage}");
    let packages: Vec<&str> = stage
        .lines()
        .skip_while(|line| !line.trim_start().starts_with("VELNOR_DELTA_PACKAGES:"))
        .skip(1)
        .take_while(|line| line.starts_with("            "))
        .map(str::trim)
        .collect();
    assert!(packages.contains(&"app"), "{stage}");
    assert!(packages.contains(&"grow-149"), "{stage}");
    Ok(())
}

/// Re-pins a sample's helper lock to a release the sample does not lock itself.
fn with_helper(
    mut repo: RepoView,
    from: &str,
    to: &str,
) -> Result<RepoView, Box<dyn std::error::Error>> {
    let lock = RepoPath::new(".velnor/rust-ci.lock")?;
    let text = String::from_utf8(repo.files.remove(&lock).ok_or("no helper lock")?)?;
    if !text.contains(from) {
        return Err(format!("the sample lock does not pin {from}").into());
    }
    repo.files.insert(lock, text.replace(from, to).into_bytes());
    Ok(repo)
}

#[test]
fn a_workspace_locked_at_0_1_93_renders_no_delta_steps() -> TestResult {
    let repo = crate::golden::load(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/lab-150"),
    )?;
    let files = expected_files(&repo)?;
    let text = files
        .get(&RepoPath::new(".github/workflows/rust-ci.yml")?)
        .ok_or("no rust-ci workflow")?;
    assert!(!text.contains("velnor-ci delta"), "{text}");
    assert!(!text.contains("the delta cache"), "{text}");
    Ok(())
}

fn source_identity(lane: &str) -> Result<(), Box<dyn std::error::Error>> {
    let prepare = step(lane, "velnor-ci delta prepare").ok_or("no prepare")?;
    assert!(
        prepare.contains("--target-dir target --staging-dir target.velnor-delta"),
        "{prepare}"
    );
    let restore = step(lane, "Restore the delta cache").ok_or("no delta restore")?;
    assert!(
        restore.contains("steps.delta_base_root.outcome == ''success''"),
        "{restore}"
    );
    let stage = step(lane, "velnor-ci delta stage").ok_or("no delta stage")?;
    assert!(stage.contains("EXPECTED_SOURCE_IDENTITY:"), "{stage}");
    assert!(
        stage.contains("${{ steps.delta_base_root.outputs.source_identity }}"),
        "{stage}"
    );
    Ok(())
}

#[test]
fn delta_steps_need_a_helper_that_has_the_delta_command() -> TestResult {
    let old = workflow(&pinned(OLDEST_HELPER, "")?)?;
    assert!(!old.contains("velnor-ci delta"), "{old}");
    assert!(!old.contains("velnor-dlt-"), "{old}");
    let new = workflow(&pinned("0.1.95", "")?)?;
    assert!(new.contains("velnor-ci delta prepare"), "{new}");
    assert!(new.contains("Restore the delta cache"), "{new}");
    Ok(())
}

#[test]
fn helper_0_1_94_gets_no_delta_steps_and_0_1_95_gets_them() -> TestResult {
    let refused = workflow(&pinned("0.1.94", "")?)?;
    assert!(!refused.contains("velnor-ci delta"), "{refused}");
    assert!(!refused.contains("the delta cache"), "{refused}");
    let accepted = workflow(&pinned("0.1.95", "")?)?;
    assert!(accepted.contains("velnor-ci delta prepare"), "{accepted}");
    Ok(())
}
