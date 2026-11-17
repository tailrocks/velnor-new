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
    assert!(
        !text.contains("Download velnor-ci"),
        "no lane downloads a build"
    );
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

const NO_BASE_SAVE: &str =
    "base caches are not saved because this repository has private dependencies";

fn saves_nothing(source: &str, view: &RepoView) -> TestResult {
    let (text, notes) = {
        let (mut files, notes) = expected_with_notes(view)?;
        let text = files
            .remove(&RepoPath::new(".github/workflows/rust-ci.yml")?)
            .ok_or("no workflow")?;
        (text, notes)
    };
    for absent in [
        "Save the target cache",
        "Save the sources cache",
        "cache decide",
        "Build the tests",
        "rust-trim",
        "repository.private",
        "actions: write",
        "Explain the skipped base cache saves",
    ] {
        assert!(!text.contains(absent), "{source}: still has {absent}");
    }
    assert!(
        notes.iter().any(|note| note.contains(NO_BASE_SAVE)),
        "{source}: {notes:?}"
    );
    Ok(())
}

#[test]
fn a_git_or_private_registry_dependency_saves_no_base_cache() -> TestResult {
    for source in [GIT, PRIVATE_REGISTRY] {
        let view = with_lock_file(source)?;
        saves_nothing(source, &view)?;
        let (_, notes) = expected_with_notes(&view)?;
        assert!(
            notes
                .iter()
                .any(|note| note.contains("Cargo.lock") && note.contains(source)),
            "{notes:?}"
        );
    }
    Ok(())
}

#[test]
fn a_cargo_config_registry_or_source_saves_no_base_cache() -> TestResult {
    for (config, section) in [
        (
            "[registries.private]\nindex = \"sparse+https://registry.example.invalid/\"\n",
            "[registries]",
        ),
        (
            "[source.mirror]\nregistry = \"https://registry.example.invalid/\"\n",
            "[source]",
        ),
    ] {
        let mut view = with_lock_file(CRATES_IO)?;
        view.files
            .insert(RepoPath::new(".cargo/config.toml")?, config.into());
        saves_nothing(section, &view)?;
        let (_, notes) = expected_with_notes(&view)?;
        assert!(
            notes
                .iter()
                .any(|note| note.contains(".cargo/config.toml") && note.contains(section)),
            "{section}: {notes:?}"
        );
    }
    for config in ["[registries]\n", "[source]\n", "[build]\njobs = 2\n"] {
        let mut view = with_lock_file(CRATES_IO)?;
        view.files
            .insert(RepoPath::new(".cargo/config.toml")?, config.into());
        let text = text_of(&view)?;
        assert!(text.contains("Save the target cache"), "{config}: {text}");
        let (_, notes) = expected_with_notes(&view)?;
        assert!(
            notes.iter().all(|note| !note.contains(NO_BASE_SAVE)),
            "{config}: {notes:?}"
        );
    }
    Ok(())
}

#[test]
fn a_cargo_config_outside_a_root_still_saves_and_one_on_the_root_does_not() -> TestResult {
    let source = "[source.mirror]\nregistry = \"https://registry.example.invalid/\"\n";
    let mut vendor = with_lock_file(CRATES_IO)?;
    vendor.files.insert(
        RepoPath::new("fixtures/vendor/.cargo/config.toml")?,
        source.into(),
    );
    still_saves(&vendor)?;
    still_saves(&rooted("app/Cargo.toml", &[])?)?;
    suppressed(
        &rooted("app/Cargo.toml", &[("app/.cargo/config.toml", source)])?,
        "app/.cargo/config.toml",
    )?;
    suppressed(
        &rooted("app/Cargo.toml", &[(".cargo/config.toml", source)])?,
        ".cargo/config.toml",
    )?;
    still_saves(&rooted(
        "app/Cargo.toml",
        &[("app/vendor/.cargo/config.toml", source)],
    )?)?;
    suppressed(
        &rooted(
            "app/crate/Cargo.toml",
            &[("app/.cargo/config.toml", source)],
        )?,
        "app/.cargo/config.toml",
    )?;
    still_saves(&rooted(
        "application/Cargo.toml",
        &[("app/.cargo/config.toml", source)],
    )?)?;
    Ok(())
}

fn rooted(root: &str, configs: &[(&str, &str)]) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut view = crate::pipeline::view(&[root], Some(b""))?;
    view.files.insert(
        LOCK_PATH,
        pin_lock("0.1.81", COMMIT_FILE, SUMS)?.into_bytes(),
    );
    for (path, text) in configs {
        view.files.insert(RepoPath::new(*path)?, (*text).into());
    }
    Ok(view)
}

fn still_saves(view: &RepoView) -> TestResult {
    let (mut files, notes) = expected_with_notes(view)?;
    let text = files
        .remove(&RepoPath::new(".github/workflows/rust-ci.yml")?)
        .ok_or("no workflow")?;
    assert!(text.contains("Save the target cache"), "{text}");
    assert!(
        notes.iter().all(|note| !note.contains(NO_BASE_SAVE)),
        "{notes:?}"
    );
    Ok(())
}

fn suppressed(view: &RepoView, config: &str) -> TestResult {
    saves_nothing(config, view)?;
    let (_, notes) = expected_with_notes(view)?;
    assert!(
        notes
            .iter()
            .any(|note| note.contains(config) && note.contains("[source]")),
        "{config}: {notes:?}"
    );
    Ok(())
}

#[test]
fn a_repository_without_private_dependencies_renders_no_visibility_condition() -> TestResult {
    let text = text_of(&with_lock_file(CRATES_IO)?)?;
    assert!(!text.contains("repository.private"), "{text}");
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
    let older = workflow(&pinned("0.1.79", "full_run = \"off\"\n")?)?;
    assert!(!older.contains("Build the tests"), "{older}");
    Ok(())
}

#[test]
fn input_timestamps_save_and_restore_in_the_configured_target_directory() -> TestResult {
    for (manifest, config, configured, expected) in [
        ("Cargo.toml", ".cargo/config.toml", "out", "out"),
        (
            "app/Cargo.toml",
            "app/.cargo/config.toml",
            "../out/app",
            "../out/app",
        ),
    ] {
        let mut view = rooted(manifest, &[])?;
        crate::cache_key_defects::with_config(
            &mut view,
            config,
            &format!("[build]\ntarget-dir = \"{configured}\"\n"),
        )?;
        let text = crate::archive::plain_invocations(&text_of(&view)?);
        for verb in ["save", "restore"] {
            let command = format!("run: velnor-ci inputs {verb} --target-dir {expected}");
            assert!(text.contains(&command), "{command} missing");
        }
    }
    Ok(())
}
