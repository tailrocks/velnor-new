// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn untracked_source_referent_cannot_claim_unchanged_workspace_waste() -> super::super::ResultTest {
    let repo = fixture("report-untracked-source-link")?;
    repo.write_unstaged("source-content", "pub fn before() {}\n");
    fs::remove_file(repo.root().join("src/lib.rs"))?;
    symlink("../source-content", repo.root().join("src/lib.rs"))?;
    repo.git(&["add", "src/lib.rs"]);
    save_restore(&repo);
    repo.write_unstaged("source-content", "pub fn after() {}\n");
    assert_unknown(&repo)
}

#[test]
fn external_source_referent_cannot_claim_unchanged_workspace_waste() -> super::super::ResultTest {
    let outside = Repo::new("report-external-source");
    outside.write("source", "pub fn before() {}\n");
    let repo = fixture("report-external-source-link")?;
    fs::remove_file(repo.root().join("src/lib.rs"))?;
    symlink(
        outside.root().join("source"),
        repo.root().join("src/lib.rs"),
    )?;
    repo.git(&["add", "."]);
    save_restore(&repo);
    outside.write("source", "pub fn after() {}\n");
    assert_unknown(&repo)?;
    fs::remove_file(outside.root().join("source"))?;
    assert_unknown(&repo)
}

#[test]
fn stable_tracked_source_chain_is_unknown_only_for_warm_compilation() -> super::super::ResultTest {
    let repo = tracked_chain()?;
    save_restore(&repo);
    assert_unknown(&repo)?;
    for steps in [
        r#"{"restore":{"outcome":"skipped","outputs":{}}}"#,
        r#"{"restore":{"outcome":"success","outputs":{"cache-primary-key":"key","cache-hit":"false"}}}"#,
    ] {
        let metrics = report(&repo, &cache(), steps)?;
        assert_eq!(metrics.invocations[0].compilations.len(), 1);
        assert!(!metrics.caches[0].usable, "{metrics:?}");
        assert!(metrics.verification_errors.is_empty(), "{metrics:?}");
        assert!(metrics.violations.is_empty(), "{metrics:?}");
        assert!(velnor_ci::report::check(&[metrics]).is_ok());
    }
    let root = fs::canonicalize(repo.root())?.join("demo");
    let mut fresh = artifact(
        &repo,
        "demo",
        "lib",
        "target/debug/deps/libdemo.rmeta",
        true,
        false,
    )?;
    fresh["package_id"] = json!(format!("path+file://{}#demo@0.1.0", root.display()));
    fresh["manifest_path"] = json!(root.join("Cargo.toml"));
    fresh["target"]["src_path"] = json!(root.join("src/lib.rs"));
    let metrics = observation(&repo, &[], &[fresh])?;
    assert!(
        metrics.invocations[0].compilations.is_empty(),
        "{metrics:?}"
    );
    assert!(metrics.caches[0].usable, "{metrics:?}");
    assert!(metrics.verification_errors.is_empty(), "{metrics:?}");
    assert!(metrics.violations.is_empty(), "{metrics:?}");
    assert!(velnor_ci::report::check(&[metrics]).is_ok());
    Ok(())
}

#[test]
fn changed_tracked_referent_cannot_prove_historical_source_contents() -> super::super::ResultTest {
    let repo = tracked_chain()?;
    assert!(
        super::super::cli(&repo, &["inputs", "save"])
            .status
            .success()
    );
    repo.write("README.md", "pub fn changed() {}\n");
    assert!(
        super::super::cli(&repo, &["inputs", "restore"])
            .status
            .success()
    );
    assert_unknown(&repo)
}

#[test]
fn historical_directory_link_replacement_cannot_bless_a_tracked_decoy() -> super::super::ResultTest
{
    let outside = Repo::new("report-historical-directory");
    fs::create_dir_all(outside.root().join("nested"))?;
    outside.write("README.md", "pub fn outside() {}\n");
    let repo = tracked_chain()?;
    fs::remove_file(repo.root().join("source-link"))?;
    symlink(
        "untracked-dir/../README.md",
        repo.root().join("source-link"),
    )?;
    repo.git(&["add", "source-link"]);
    symlink(
        outside.root().join("nested"),
        repo.root().join("untracked-dir"),
    )?;
    assert_eq!(
        fs::read_to_string(repo.root().join("demo/src/lib.rs"))?,
        "pub fn outside() {}\n"
    );
    assert!(
        super::super::cli(&repo, &["inputs", "save"])
            .status
            .success()
    );
    fs::remove_file(repo.root().join("untracked-dir"))?;
    fs::create_dir(repo.root().join("untracked-dir"))?;
    for path in ["demo/src/lib.rs", "README.md"] {
        assert_eq!(
            fs::read_to_string(repo.root().join(path))?,
            "pub fn before() {}\n"
        );
    }
    assert!(
        super::super::cli(&repo, &["inputs", "restore"])
            .status
            .success()
    );
    assert_unknown(&repo)
}

#[test]
fn intermediate_link_before_parent_component_cannot_prove_source_contents()
-> super::super::ResultTest {
    let repo = tracked_chain()?;
    fs::remove_file(repo.root().join("source-link"))?;
    symlink(
        "directory-link/../README.md",
        repo.root().join("source-link"),
    )?;
    symlink("README.md", repo.root().join("directory-link"))?;
    repo.git(&["add", "."]);
    save_restore(&repo);
    assert_unknown(&repo)
}

#[test]
fn tracked_link_cycle_cannot_prove_source_contents() -> super::super::ResultTest {
    let repo = tracked_chain()?;
    fs::remove_file(repo.root().join("source-link"))?;
    symlink("demo/src/lib.rs", repo.root().join("source-link"))?;
    repo.git(&["add", "."]);
    save_restore(&repo);
    assert_unknown(&repo)
}

#[test]
fn untracked_directory_link_before_parent_cannot_bless_a_tracked_decoy() -> super::super::ResultTest
{
    let outside = Repo::new("report-external-directory");
    fs::create_dir_all(outside.root().join("nested"))?;
    outside.write("README.md", "pub fn before() {}\n");
    let repo = tracked_chain()?;
    fs::remove_file(repo.root().join("source-link"))?;
    symlink(
        "untracked-dir/../README.md",
        repo.root().join("source-link"),
    )?;
    repo.git(&["add", "source-link"]);
    symlink(
        outside.root().join("nested"),
        repo.root().join("untracked-dir"),
    )?;
    save_restore(&repo);
    outside.write("README.md", "pub fn after() {}\n");
    assert_eq!(
        fs::read_to_string(repo.root().join("demo/src/lib.rs"))?,
        "pub fn after() {}\n"
    );
    assert_eq!(
        fs::read_to_string(repo.root().join("README.md"))?,
        "pub fn before() {}\n"
    );
    assert_unknown(&repo)
}

#[test]
fn missing_directory_before_parent_cannot_prove_source_contents() -> super::super::ResultTest {
    let repo = tracked_chain()?;
    fs::remove_file(repo.root().join("source-link"))?;
    symlink("missing-dir/../README.md", repo.root().join("source-link"))?;
    repo.git(&["add", "source-link"]);
    save_restore(&repo);
    assert!(fs::read(repo.root().join("demo/src/lib.rs")).is_err());
    assert_unknown(&repo)
}

#[test]
fn untracked_source_parent_directory_link_cannot_prove_source_contents() -> super::super::ResultTest
{
    let outside = Repo::new("report-external-source-parent");
    let repo = tracked_chain()?;
    fs::rename(repo.root().join("demo/src"), outside.root().join("src"))?;
    symlink(outside.root().join("src"), repo.root().join("demo/src"))?;
    save_restore(&repo);
    assert_unknown(&repo)
}

#[test]
fn regular_referent_with_directory_suffix_cannot_prove_source_contents() -> super::super::ResultTest
{
    for target in ["README.md/", "README.md/."] {
        let repo = tracked_chain()?;
        fs::remove_file(repo.root().join("source-link"))?;
        symlink(target, repo.root().join("source-link"))?;
        repo.git(&["add", "source-link"]);
        save_restore(&repo);
        assert_unknown(&repo)?;
    }
    Ok(())
}

#[test]
fn links_outside_every_compilation_input_keep_waste_verifiable() -> super::super::ResultTest {
    let rebuilt = unrelated_links("report-unrelated-links-rebuilt")?;
    assert!(
        super::super::cli(&rebuilt, &["inputs", "save"])
            .status
            .success()
    );
    rebuilt.write("src/lib.rs", "pub fn changed() {}\n");
    assert!(
        super::super::cli(&rebuilt, &["inputs", "restore"])
            .status
            .success()
    );
    let compiling = event(&rebuilt, "Compiling", "demo", "0.1.0", true);
    let metrics = progress(&rebuilt, &[compiling])?;
    assert_eq!(
        metrics.verification_errors,
        [] as [String; 0],
        "{metrics:?}"
    );
    assert!(metrics.violations.is_empty(), "{metrics:?}");
    assert!(velnor_ci::report::check(&[metrics]).is_ok());

    let wasted = unrelated_links("report-unrelated-links-wasted")?;
    save_restore(&wasted);
    let metrics = progress(
        &wasted,
        &[event(&wasted, "Compiling", "demo", "0.1.0", true)],
    )?;
    assert_eq!(
        metrics.verification_errors,
        [] as [String; 0],
        "{metrics:?}"
    );
    assert!(
        metrics.invocations[0].compilations[0].unchanged,
        "{metrics:?}"
    );
    Ok(())
}

fn unrelated_links(name: &str) -> Result<Repo, Box<dyn std::error::Error>> {
    let repo = fixture(name)?;
    repo.write_all(&[("AGENTS.md", "notes\n"), ("docs/README.md", "guide\n")]);
    symlink("AGENTS.md", repo.root().join("CLAUDE.md"))?;
    symlink("README.md", repo.root().join("docs/guide.md"))?;
    repo.git(&["add", "."]);
    Ok(repo)
}

#[test]
fn link_at_an_unclassified_path_still_cannot_claim_waste() -> super::super::ResultTest {
    let repo = fixture("report-unclassified-link")?;
    repo.write("tool-source", "echo\n");
    fs::create_dir_all(repo.root().join("scripts"))?;
    symlink("../tool-source", repo.root().join("scripts/tool"))?;
    repo.git(&["add", "."]);
    save_restore(&repo);
    assert_unknown(&repo)
}

fn tracked_chain() -> Result<Repo, Box<dyn std::error::Error>> {
    let repo = Repo::new("report-tracked-source-chain");
    repo.write_all(&[
        (
            "Cargo.toml",
            "[workspace]\nmembers=['demo']\nresolver='2'\n",
        ),
        (
            "demo/Cargo.toml",
            "[package]\nname='demo'\nversion='0.1.0'\nedition='2024'\n",
        ),
        ("demo/src/placeholder", ""),
        (
            "Cargo.lock",
            "version=4\n[[package]]\nname='demo'\nversion='0.1.0'\n",
        ),
        ("README.md", "pub fn before() {}\n"),
    ]);
    symlink("../../source-link", repo.root().join("demo/src/lib.rs"))?;
    symlink("README.md", repo.root().join("source-link"))?;
    super::super::metadata_inputs(&repo)?;
    velnor_gen::io::write_metadata(repo.root())?;
    repo.git(&["add", "."]);
    Ok(repo)
}

fn save_restore(repo: &Repo) {
    for action in ["save", "restore"] {
        let output = super::super::cli(repo, &["inputs", action]);
        assert!(output.status.success(), "{output:?}");
    }
}

fn assert_unknown(repo: &Repo) -> super::super::ResultTest {
    let metrics = progress(repo, &[event(repo, "Compiling", "demo", "0.1.0", true)])?;
    assert_eq!(metrics.invocations[0].compilations.len(), 1);
    assert!(
        !metrics.invocations[0].compilations[0].unchanged,
        "{metrics:?}"
    );
    assert!(metrics.violations.is_empty(), "{metrics:?}");
    assert!(
        metrics
            .verification_errors
            .iter()
            .any(|error| error.contains("symlink referents")),
        "{metrics:?}"
    );
    assert!(velnor_ci::report::check(&[metrics]).is_err());
    Ok(())
}
