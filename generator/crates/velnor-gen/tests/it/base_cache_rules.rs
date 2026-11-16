// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_gen::expected_with_notes;
use velnor_model::{RepoPath, RepoView};
use velnor_rust::{CONFIG_PATH, LOCK_PATH, pin_lock};

use crate::archive::jobs_of;
use crate::base_save::{jobs, pinned, step};
use crate::facts::TestResult;
use crate::golden::{load, samples};
use crate::lock::{COMMIT_FILE, SUMS};
use crate::pipeline::workflow;

const CRATES_IO: &str = "registry+https://github.com/rust-lang/crates.io-index";
const GIT: &str = "git+https://github.com/example/shared-lib?branch=main#0123456789abcdef";
const PRIVATE_REGISTRY: &str = "registry+https://registry.example.invalid/index";
const SPARSE_CRATES_IO: &str = "sparse+https://index.crates.io/";

fn source_helper(version: &str) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut view = load(&samples().join("helper-source"))?;
    view.files.insert(
        LOCK_PATH,
        pin_lock(version, COMMIT_FILE, SUMS)?.into_bytes(),
    );
    view.files
        .insert(CONFIG_PATH, b"helper = { path = \"ci-helper\" }\n".to_vec());
    Ok(view)
}

fn text_of(view: &RepoView) -> Result<String, Box<dyn std::error::Error>> {
    let (mut files, _) = expected_with_notes(view)?;
    files
        .remove(&RepoPath::new(".github/workflows/rust-ci.yml")?)
        .ok_or_else(|| "no workflow".into())
}

fn block(text: &str, id: &str) -> Option<String> {
    jobs_of(text)
        .into_iter()
        .find(|(job, _)| job == id)
        .map(|(_, block)| block)
}

#[test]
fn a_source_built_helper_never_runs_in_the_job_that_holds_actions_write() -> TestResult {
    let text = text_of(&source_helper("0.1.81")?)?;
    assert!(text.contains("Download velnor-ci"), "lanes use the build");
    let trim = block(&text, "rust-trim").ok_or("no trim job")?;
    assert!(!trim.contains("Download velnor-ci"), "{trim}");
    assert!(!trim.contains("download-artifact"), "{trim}");
    assert!(
        trim.contains("uses: tailrocks/velnor-new/actions/setup-velnor@"),
        "{trim}"
    );
    assert!(trim.contains("0.1.81"), "{trim}");
    Ok(())
}

#[test]
fn a_source_helper_repository_without_a_released_helper_from_0_1_81_gets_no_trim_job() -> TestResult
{
    let text = text_of(&source_helper("0.1.79")?)?;
    assert!(!text.contains("rust-trim"), "{text}");
    assert!(!text.contains("cache trim"), "{text}");
    assert!(!text.contains("actions: write"), "{text}");
    Ok(())
}

fn with_lock_file(source: &str) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut view = pinned("0.1.81", "")?;
    let lock = format!(
        "version = 4\n\n[[package]]\nname = \"package-0\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"dep\"\nversion = \"1.0.0\"\nsource = \"{source}\"\n"
    );
    view.files
        .insert(RepoPath::new("Cargo.lock")?, lock.into_bytes());
    Ok(view)
}

#[test]
fn a_lockfile_with_only_crates_io_sources_saves_the_base_caches() -> TestResult {
    for source in [CRATES_IO, SPARSE_CRATES_IO] {
        let text = text_of(&with_lock_file(source)?)?;
        for saved in [
            "Save the target cache",
            "Save the sources cache",
            "cache decide",
        ] {
            assert!(text.contains(saved), "{source}: {saved}");
        }
    }
    Ok(())
}

#[test]
fn a_git_or_private_registry_dependency_saves_neither_the_target_nor_the_sources_cache()
-> TestResult {
    for source in [GIT, PRIVATE_REGISTRY] {
        let view = with_lock_file(source)?;
        let text = text_of(&view)?;
        for absent in [
            "Save the target cache",
            "Save the sources cache",
            "cache decide",
            "rust-trim",
            "inputs save",
        ] {
            assert!(!text.contains(absent), "{source}: {absent}");
        }
        let (_, notes) = expected_with_notes(&view)?;
        assert!(
            notes
                .iter()
                .any(|note| note.contains("private") && note.contains("not saved")),
            "{notes:?}"
        );
    }
    Ok(())
}

#[test]
fn a_registry_declared_in_the_cargo_config_also_stops_the_saves() -> TestResult {
    let mut view = pinned("0.1.81", "")?;
    let config = RepoPath::new(".cargo/config.toml")?;
    view.tracked_paths.insert(config.clone());
    view.files.insert(
        config,
        b"[registries.corp]\nindex = \"sparse+https://registry.example.invalid/\"\n".to_vec(),
    );
    let text = text_of(&view)?;
    assert!(!text.contains("Save the target cache"), "{text}");
    Ok(())
}

#[test]
fn an_affected_only_push_never_expires_the_base_but_a_dispatch_does() -> TestResult {
    let all = jobs("0.1.81", "on_main = \"affected\"\nfull_run = \"off\"\n")?;
    let (_, lane) = all.iter().find(|(id, _)| id == "rust").ok_or("no lane")?;
    let restored = step(lane, "velnor-ci restored").ok_or("no restored")?;
    let limit = restored
        .lines()
        .find(|line| line.contains("VELNOR_BASE_MAX_AGE_DAYS"))
        .ok_or("no limit")?;
    assert!(
        limit.contains("github.event_name == ''workflow_dispatch''"),
        "{limit}"
    );
    assert!(!limit.contains("push"), "{limit}");
    assert!(!limit.contains("!= ''push''"), "{limit}");
    Ok(())
}

#[test]
fn the_trim_job_also_prunes_the_sources_family_of_the_lane_that_saves_it() -> TestResult {
    let all = jobs("0.1.81", "shape = \"lint-test\"\n")?;
    let lane = |id: &str| {
        all.iter()
            .find(|(job, _)| job == id)
            .map(|(_, b)| b.as_str())
    };
    let test = lane("rust-test").ok_or("no test lane")?;
    assert!(
        test.contains(
            "      sources_prefix: 'velnor-src-${{ runner.os }}-${{ runner.arch }}-root-'\n"
        ),
        "{test}"
    );
    assert!(
        !lane("rust-lint")
            .ok_or("no lint lane")?
            .contains("sources_prefix")
    );
    let trim = lane("rust-trim").ok_or("no trim job")?;
    let prune =
        step(trim, "Delete the older rust-test sources entries").ok_or("no sources trim")?;
    assert!(
        prune.contains("CACHE_PREFIX: '${{ needs.rust-test.outputs.sources_prefix }}'"),
        "{prune}"
    );
    assert!(
        prune.contains("run: 'velnor-ci cache trim --prefix \"$CACHE_PREFIX\"'"),
        "{prune}"
    );
    assert!(step(trim, "Delete the older rust-lint sources entries").is_none());
    Ok(())
}

#[test]
fn the_test_prebuild_runs_only_on_a_run_that_can_save() -> TestResult {
    let text = workflow(&pinned("0.1.81", "")?)?;
    let lane = block(&text, "rust").ok_or("no lane")?;
    let build = step(&lane, "Build the tests").ok_or("no prebuild")?;
    for needle in [
        "github.ref == format(''refs/heads/{0}'', github.event.repository.default_branch)",
        "github.event_name == ''push''",
    ] {
        assert!(build.contains(needle), "{build}");
    }
    let older = workflow(&pinned("0.1.79", "")?)?;
    assert!(!older.contains("Build the tests"), "{older}");
    Ok(())
}
