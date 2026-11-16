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
    assert_eq!(text.matches("velnor-ci cache budget").count(), 1);
    for (id, block) in jobs_of(&text) {
        if block.contains("name: Save the") {
            assert!(trim.contains(&format!("      - {id}\n")), "{id}: {trim}");
        }
    }
    assert!(!trim.contains("checkout"), "{trim}");
    assert!(!trim.contains("cargo "), "{trim}");
    Ok(())
}

#[test]
fn a_source_helper_repository_without_a_released_helper_from_0_1_81_gets_no_trim_job() -> TestResult
{
    let text = text_of(&source_helper("0.1.80")?)?;
    assert!(!text.contains("rust-trim"), "{text}");
    assert!(!text.contains("cache trim"), "{text}");
    assert!(!text.contains("cache budget"), "{text}");
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

const PRIVATE_ONLY: &str = "github.event.repository.private == true";

#[test]
fn a_git_or_private_registry_dependency_saves_the_base_caches_only_in_private_repositories()
-> TestResult {
    for source in [GIT, PRIVATE_REGISTRY] {
        let view = with_lock_file(source)?;
        let text = text_of(&view)?;
        let lane = block(&text, "rust").ok_or("no lane")?;
        for name in [
            "Save the target cache",
            "Save the sources cache",
            "Decide whether to save the build directory",
            "Build the tests",
            "velnor-ci inputs save",
        ] {
            let saved = step(&lane, name).ok_or_else(|| format!("{source}: no {name}"))?;
            assert!(saved.contains(PRIVATE_ONLY), "{source}: {name}: {saved}");
        }
        let trim = block(&text, "rust-trim").ok_or("no budget cleanup")?;
        assert!(
            trim.contains("cache budget --budget-bytes 8000000000"),
            "{trim}"
        );
        for name in [
            "Delete the older rust entries",
            "Delete the older rust sources entries",
        ] {
            let prune = step(&trim, name).ok_or("no family trim")?;
            assert!(prune.contains(PRIVATE_ONLY), "{prune}");
        }
        assert!(!trim.contains("download-artifact"), "{trim}");
        assert!(trim.contains("Install velnor-ci 0.1.81"), "{trim}");
        for (id, lane) in jobs_of(&text) {
            if lane.contains("name: Save the tools cache") {
                assert!(trim.contains(&format!("      - {id}\n")), "{id}: {trim}");
            }
        }
        let required = block(&text, "required").ok_or("no required")?;
        assert!(!required.contains("rust-trim"), "{required}");
        let (_, notes) = expected_with_notes(&view)?;
        assert!(
            notes.iter().any(|note| note.contains("private")
                && note.contains("saves run only in private repositories")),
            "{notes:?}"
        );
        let notice = step(&lane, "Explain the skipped base cache saves").ok_or("no notice")?;
        assert!(
            notice.contains("github.event.repository.private != true"),
            "{notice}"
        );
        assert!(notice.contains("::notice::"), "{notice}");
    }
    Ok(())
}

#[test]
fn public_repositories_with_private_dependencies_still_budget_their_tools_caches() -> TestResult {
    let text = text_of(&with_lock_file(GIT)?)?;
    let trim = block(&text, "rust-trim").ok_or("no trim job")?;
    let condition = trim
        .lines()
        .find(|line| line.trim_start().starts_with("if: "))
        .ok_or("no job condition")?;
    for needle in [
        "github.ref == format(''refs/heads/{0}'', github.event.repository.default_branch)",
        "github.event_name == ''push''",
        "!cancelled()",
    ] {
        assert!(condition.contains(needle), "{condition}");
    }
    assert!(!condition.contains("repository.private"), "{condition}");
    assert!(!condition.contains("needs."), "{condition}");
    for name in [
        "Install velnor-ci 0.1.81",
        "Enforce the repository cache budget",
    ] {
        let command = step(&trim, name).ok_or("no cleanup command")?;
        assert!(!command.contains("    if:"), "{command}");
        assert!(!command.contains("base_save"), "{command}");
    }
    let all = jobs_of(&text);
    let mut writers = 0;
    for (id, lane) in all {
        if lane.contains("name: Save the") {
            writers += 1;
            assert!(trim.contains(&format!("      - {id}\n")), "{id}: {trim}");
        }
    }
    assert!(writers > 0);
    assert!(trim.contains("actions: write"), "{trim}");
    Ok(())
}

#[test]
fn a_repository_without_private_dependencies_renders_no_visibility_condition() -> TestResult {
    let text = text_of(&with_lock_file(CRATES_IO)?)?;
    assert!(!text.contains("repository.private"), "{text}");
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
