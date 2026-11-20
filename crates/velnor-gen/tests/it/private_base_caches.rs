// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_gen::expected_with_notes;
use velnor_model::RepoView;

use crate::archive::jobs_of;
use crate::base_save::{names, step};
use crate::facts::{TestResult, path};
use crate::pipeline::workflow;
use crate::private_sources::{SECTION, private_repo};

const GIT_LOCK: &str = "version = 4\n\n[[package]]\nname = \"package-0\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"shared\"\nversion = \"1.0.0\"\nsource = \"git+https://github.com/acme/shared-lib?branch=main#0123456789abcdef\"\n";
const VERDICT: &str = "steps.private_repo.outputs.private == ''true''";
const TRUSTED_PULL_REQUEST: &str = "github.event.pull_request.head.repo.full_name == github.repository && !endsWith(github.actor, ''[bot]'') && !endsWith(github.event.pull_request.user.login, ''[bot]'')";
const SAVING_NOTE: &str = "only while GitHub reports this repository private";
const REFUSED_NOTE: &str =
    "base caches are not saved because this repository has private dependencies";

fn consumer(
    helper: &str,
    lock: &str,
    config: &str,
) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut repo = private_repo(&["Cargo.toml"], &format!("{SECTION}{config}"), helper)?;
    repo.files
        .insert(path("Cargo.lock")?, lock.as_bytes().to_vec());
    Ok(repo)
}

fn lane(text: &str) -> Result<String, Box<dyn std::error::Error>> {
    jobs_of(text)
        .into_iter()
        .find(|(_, block)| block.contains("Restore the target cache"))
        .map(|(_, block)| block)
        .ok_or_else(|| "no lane restores the target cache".into())
}

fn text_and_notes(repo: &RepoView) -> Result<(String, Vec<String>), Box<dyn std::error::Error>> {
    let mut locked = repo.clone();
    crate::facts::lock_pinned_tools(&mut locked)?;
    let (mut files, notes) = expected_with_notes(&locked)?;
    let text = files
        .remove(&path(".github/workflows/rust-ci.yml")?)
        .ok_or("no workflow")?;
    Ok((text, notes))
}

#[test]
fn a_private_consumer_with_a_new_helper_restores_and_saves_both_base_caches() -> TestResult {
    let repo = consumer("0.1.108", GIT_LOCK, "")?;
    let (text, notes) = text_and_notes(&repo)?;
    let lane = lane(&text)?;
    for present in [
        "Restore the sources cache",
        "Restore the target cache",
        "Save the target cache",
        "Save the sources cache",
        "Decide whether to save the build directory",
        "Fetch the sources of every root",
    ] {
        assert!(lane.contains(present), "{present} missing:\n{lane}");
    }
    assert!(
        notes.iter().any(|note| note.contains(SAVING_NOTE)),
        "{notes:?}"
    );
    assert!(
        notes.iter().all(|note| !note.contains(REFUSED_NOTE)),
        "{notes:?}"
    );
    Ok(())
}

#[test]
fn the_helpers_verdict_gates_every_restore_and_every_save_of_the_base_caches() -> TestResult {
    let text = workflow(&consumer("0.1.108", GIT_LOCK, "")?)?;
    let lane = lane(&text)?;
    let verdict = step(&lane, "Check that the repository is private").ok_or("no verdict step")?;
    assert!(verdict.contains("id: private_repo"), "{verdict}");
    assert!(
        verdict.contains("velnor-ci cache private-repo"),
        "{verdict}"
    );
    assert!(
        verdict.contains("GITHUB_TOKEN: '${{ github.token }}'"),
        "{verdict}"
    );
    assert!(
        verdict.contains("VELNOR_API_URL: '${{ github.api_url }}'"),
        "{verdict}"
    );
    let names = names(&lane);
    let at = |name: &str| names.iter().position(|each| each == name);
    assert!(
        at("Check that the repository is private") < at("Restore the sources cache"),
        "{names:?}"
    );
    for gated_restore in ["Restore the sources cache", "Restore the target cache"] {
        let restore = step(&lane, gated_restore).ok_or(gated_restore)?;
        let condition = restore
            .lines()
            .find(|line| line.trim_start().starts_with("if: "))
            .ok_or("restore has no condition")?;
        assert!(condition.contains(VERDICT), "{condition}");
        assert!(
            condition.contains(&format!(
                "(github.event_name != ''pull_request'' || ({TRUSTED_PULL_REQUEST}))"
            )),
            "{condition}"
        );
        assert!(
            condition.contains("!endsWith(github.actor, ''[bot]'')"),
            "{condition}"
        );
        assert!(
            condition.contains("!endsWith(github.event.pull_request.user.login, ''[bot]'')"),
            "{condition}"
        );
    }
    let token = step(
        &lane,
        "Create a read-only token for the private repositories",
    )
    .ok_or("no token step")?;
    assert!(token.contains(TRUSTED_PULL_REQUEST), "{token}");
    for gated_save in [
        "velnor-ci inputs save",
        "Decide whether to save the build directory",
        "Save the target cache",
        "Fetch the sources of every root",
        "Save the sources cache",
    ] {
        let save = step(&lane, gated_save).ok_or(gated_save)?;
        let condition = save
            .lines()
            .find(|line| line.trim_start().starts_with("if: "))
            .ok_or("save has no condition")?;
        assert!(condition.contains(VERDICT), "{gated_save}: {condition}");
        assert!(
            condition.contains("github.event.repository.default_branch")
                || condition.contains("steps.base.outputs.sources == ''true''"),
            "{gated_save}: {condition}"
        );
    }
    let decide = step(&lane, "Decide whether to save the build directory").ok_or("no decide")?;
    assert!(
        decide.contains("GITHUB_TOKEN: '${{ github.token }}'"),
        "{decide}"
    );
    assert!(
        decide.contains("VELNOR_API_URL: '${{ github.api_url }}'"),
        "{decide}"
    );
    let fetch = step(&lane, "Fetch the sources of every root").ok_or("no sources fetch")?;
    assert!(fetch.contains("CARGO_NET_OFFLINE: 'true'"), "{fetch}");
    Ok(())
}

#[test]
fn the_sources_cache_saves_the_cargo_downloads_and_nothing_that_holds_a_credential() -> TestResult {
    let text = workflow(&consumer("0.1.108", GIT_LOCK, "")?)?;
    let lane = lane(&text)?;
    for name in ["Restore the sources cache", "Save the sources cache"] {
        let block = step(&lane, name).ok_or(name)?;
        let paths: Vec<&str> = block
            .lines()
            .skip_while(|line| !line.trim_start().starts_with("path: |"))
            .skip(1)
            .take_while(|line| line.starts_with("            "))
            .map(str::trim)
            .collect();
        assert_eq!(
            paths,
            [
                "~/.cargo/registry/index",
                "~/.cargo/registry/cache",
                "~/.cargo/git/db"
            ],
            "{name}"
        );
    }
    Ok(())
}

#[test]
fn an_older_helper_or_a_config_that_names_registries_keeps_the_refusal() -> TestResult {
    for (helper, config, lock) in [("0.1.107", "", GIT_LOCK), ("0.1.88", "", GIT_LOCK)] {
        let repo = consumer(helper, lock, config)?;
        let (text, notes) = text_and_notes(&repo)?;
        for absent in [
            "Save the target cache",
            "Save the sources cache",
            "cache decide",
            "private_repo",
            "cache private-repo",
        ] {
            assert!(!text.contains(absent), "{helper}: {absent}");
        }
        assert!(
            notes.iter().any(|note| note.contains(REFUSED_NOTE)),
            "{helper}: {notes:?}"
        );
    }
    Ok(())
}

#[test]
fn a_lock_that_is_not_only_git_or_a_cargo_config_with_registries_keeps_the_refusal() -> TestResult {
    let registry = GIT_LOCK.replace(
        "git+https://github.com/acme/shared-lib?branch=main#0123456789abcdef",
        "registry+https://registry.example.invalid/index",
    );
    let patched = format!("{GIT_LOCK}\n[[patch.unused]]\nname = \"secret\"\nversion = \"1.0.0\"\n");
    for lock in [registry, patched] {
        let (text, notes) = text_and_notes(&consumer("0.1.108", &lock, "")?)?;
        assert!(!text.contains("Save the target cache"), "{lock}");
        assert!(
            notes.iter().any(|note| note.contains(REFUSED_NOTE)),
            "{notes:?}"
        );
    }
    let mut repo = consumer("0.1.108", GIT_LOCK, "")?;
    repo.files.insert(
        path(".cargo/config.toml")?,
        b"[registries.private]\nindex = \"sparse+https://registry.example.invalid/\"\n".to_vec(),
    );
    let (text, notes) = text_and_notes(&repo)?;
    assert!(!text.contains("Save the target cache"));
    assert!(
        notes.iter().any(|note| note.contains(REFUSED_NOTE)),
        "{notes:?}"
    );
    Ok(())
}

#[test]
fn without_the_private_sources_table_private_git_dependencies_still_save_nothing() -> TestResult {
    let mut repo = consumer("0.1.108", GIT_LOCK, "")?;
    repo.files.insert(
        path(".velnor/rust-ci.toml")?,
        b"full_run = 'off'\n".to_vec(),
    );
    let (text, notes) = text_and_notes(&repo)?;
    assert!(!text.contains("Save the target cache"));
    assert!(!text.contains("cache private-repo"));
    assert!(
        notes.iter().any(|note| note.contains(REFUSED_NOTE)),
        "{notes:?}"
    );
    Ok(())
}

#[test]
fn a_repository_without_private_dependencies_has_no_verdict_step() -> TestResult {
    let repo = private_repo(&["Cargo.toml"], SECTION, "0.1.108")?;
    let text = workflow(&repo)?;
    assert!(text.contains("Save the target cache"));
    assert!(!text.contains("cache private-repo"));
    assert!(!text.contains("private_repo"));
    Ok(())
}
