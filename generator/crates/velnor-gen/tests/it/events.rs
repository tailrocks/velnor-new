// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_gen::expected_files;
use velnor_model::{PolicyTools, RepoPath};
use velnor_rust::{CONFIG_PATH, PolicyRuns, policy_runs};

use crate::facts::TestResult;
use crate::pipeline::{view, workflow};

const EVENTS: &str = r"on:
  pull_request:
    types:
      - opened
      - synchronize
      - reopened
      - ready_for_review
      - edited
  push:
    branches:
      - main
  merge_group: {}
  workflow_dispatch: {}
permissions:
  contents: read
concurrency:
  group: ci-${{ github.workflow_ref }}-${{ github.event_name == 'pull_request' && github.event.pull_request.number || github.run_id }}
  cancel-in-progress: ${{ github.event_name == 'pull_request' }}
";

const DENY_MISE: &str = "[tools]\nrust = \"1.99.1\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\n\"aqua:EmbarkStudios/cargo-deny\" = \"0.18.3\"\n";

const DRAFT: &str = "github.event.pull_request.draft != true";

fn with(files: &[(&str, &str)], config: Option<&[u8]>) -> TestResult2 {
    let mut view = view(&["Cargo.toml"], config)?;
    for (path, text) in files {
        let path = RepoPath::new(*path)?;
        view.tracked_paths.insert(path.clone());
        view.files.insert(path, text.as_bytes().to_vec());
    }
    workflow(&view)
}

type TestResult2 = Result<String, Box<dyn std::error::Error>>;

fn unquoted(text: &str) -> String {
    text.replace('\'', "")
}

#[test]
fn the_workflow_starts_on_the_four_events_and_cancels_only_older_pull_request_runs() -> TestResult {
    let text = unquoted(&with(&[], Some(b"full_run = \"off\"\n"))?);
    assert!(text.contains(&unquoted(EVENTS)), "{text}");
    Ok(())
}

#[test]
fn the_default_branch_comes_from_the_config_and_a_malformed_name_is_refused() -> TestResult {
    let text = with(&[], Some(b"default_branch = \"trunk\"\n"))?;
    assert!(text.contains("    branches:\n      - trunk\n"), "{text}");
    for name in [
        "",
        "a..b",
        "/main",
        "main/",
        "ma in",
        "x\\ny",
        ".release",
        "feature/.hidden",
        "release.lock",
        "feature/x.lock/y",
    ] {
        let config = format!("default_branch = \"{name}\"\n");
        let mut bad = view(&["Cargo.toml"], Some(config.as_bytes()))?;
        bad.files.entry(CONFIG_PATH).or_default();
        assert!(expected_files(&bad).is_err(), "{name:?}");
    }
    Ok(())
}

#[test]
fn a_machete_pin_filtered_to_another_os_does_not_enable_the_policy_lane() -> TestResult {
    let mise = |os: &str| {
        format!(
            "[tools]\nrust = \"1.99.1\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\ncargo-machete = {{ version = \"0.9.1\", os = [\"{os}\"] }}\n"
        )
    };
    let elsewhere = with(&[("mise.toml", &mise("macos"))], None)?;
    assert!(!elsewhere.contains("cargo-machete"), "{elsewhere}");
    let here = with(&[("mise.toml", &mise("linux"))], None)?;
    assert!(here.contains("cargo-machete"), "{here}");
    let mac = with(
        &[("mise.toml", &mise("macos"))],
        Some(b"runner = \"macos-latest\"\n"),
    )?;
    assert!(mac.contains("cargo-machete"), "{mac}");
    Ok(())
}

#[test]
fn a_machete_pin_reads_mise_os_arch_filters_and_darwin_as_macos() -> TestResult {
    let mise = |os: &str| {
        format!(
            "[tools]\nrust = \"1.99.1\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\ncargo-machete = {{ version = \"0.9.1\", os = [\"{os}\"] }}\n"
        )
    };
    let linux_arch = with(
        &[("mise.toml", &mise("linux/x64"))],
        Some(b"runner = \"ubuntu-latest\"\n"),
    )?;
    assert!(linux_arch.contains("cargo-machete"), "{linux_arch}");
    let darwin = with(
        &[("mise.toml", &mise("darwin"))],
        Some(b"runner = \"macos-latest\"\n"),
    )?;
    assert!(darwin.contains("cargo-machete"), "{darwin}");
    Ok(())
}

#[test]
fn draft_pull_requests_skip_every_lane_but_lint_only_when_the_config_says_so() -> TestResult {
    let shape = "shape = \"lint-test\"\n";
    let off = with(&[], Some(shape.as_bytes()))?;
    assert!(!off.contains("draft"), "{off}");
    let on = with(
        &[],
        Some(format!("{shape}skip_draft_pull_requests = true\n").as_bytes()),
    )?;
    let job = |id: &str| {
        let lines: Vec<&str> = on.lines().skip_while(|line| *line != "jobs:").collect();
        let start = lines.iter().position(|line| *line == format!("  {id}:"));
        lines[start.unwrap_or_default() + 1..]
            .iter()
            .take_while(|line| line.starts_with("   "))
            .copied()
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert!(!job("rust-lint").contains("draft"), "{on}");
    assert!(unquoted(&job("rust-test")).contains(DRAFT), "{on}");
    assert!(!job("required").contains("draft != true"), "{on}");
    Ok(())
}

#[test]
fn the_policy_lane_exists_only_for_a_deny_config_or_a_pinned_machete() -> TestResult {
    let none = with(&[], None)?;
    assert!(!none.contains("rust-policy"), "{none}");
    let unpinned = with(&[("deny.toml", "")], None);
    assert!(
        unpinned.is_err_and(|error| error.to_string().contains("cargo-deny is not pinned")),
        "a deny.toml without a pinned cargo-deny must be refused"
    );
    let deny = with(&[("deny.toml", ""), ("mise.toml", DENY_MISE)], None)?;
    assert!(deny.contains("  policy:\n"), "{deny}");
    assert!(deny.contains("cargo-deny check"), "{deny}");
    assert!(!deny.contains("cargo-machete"), "{deny}");
    assert!(deny.contains("      - policy\n"), "{deny}");
    let machete = with(
        &[(
            "mise.toml",
            "[tools]\nrust = \"1.99.1\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\ncargo-machete = \"0.9.1\"\n",
        )],
        None,
    )?;
    assert!(machete.contains("  policy:\n"), "{machete}");
    assert!(machete.contains("cargo-machete"), "{machete}");
    assert!(!machete.contains("cargo-deny"), "{machete}");
    Ok(())
}

#[test]
fn the_policy_tools_run_for_their_own_triggers() -> TestResult {
    let changed = |paths: &[&str]| -> Result<BTreeSet<RepoPath>, Box<dyn std::error::Error>> {
        Ok(paths
            .iter()
            .map(|path| RepoPath::new(*path))
            .collect::<Result<_, _>>()?)
    };
    let both = PolicyTools {
        deny: true,
        machete: true,
    };
    let cases: [(&[&str], bool, bool, (bool, bool)); 11] = [
        (&["README.md"], false, false, (false, false)),
        (&[], false, false, (false, false)),
        (&["Cargo.lock"], false, true, (true, true)),
        (&["crates/a/Cargo.toml"], false, true, (true, true)),
        (&["deny.toml"], false, true, (true, true)),
        (&[".cargo/config.toml"], false, true, (true, true)),
        (
            &["docs/deny.toml.md", "src/Cargo.tomls"],
            false,
            false,
            (false, false),
        ),
        (&["src/lib.rs", "README.md"], false, true, (false, true)),
        (&["docs/example.rs"], false, false, (false, false)),
        (&["mise.toml"], false, false, (false, true)),
        (&[], true, true, (true, true)),
    ];
    for (paths, everything, selected, (deny, machete)) in cases {
        let runs = policy_runs(both, &changed(paths)?, everything, selected);
        assert_eq!((runs.deny, runs.machete), (deny, machete), "{paths:?}");
    }
    let no_tools = policy_runs(
        PolicyTools::default(),
        &changed(&["Cargo.lock"])?,
        true,
        true,
    );
    assert_eq!(no_tools, PolicyRuns::default());
    Ok(())
}

#[test]
fn a_source_change_runs_the_policy_lane_when_machete_is_pinned() -> TestResult {
    let changed: BTreeSet<RepoPath> = BTreeSet::from([RepoPath::new("crates/a/src/lib.rs")?]);
    let deny_only = PolicyTools {
        deny: true,
        machete: false,
    };
    let machete = PolicyTools {
        deny: false,
        machete: true,
    };
    assert_eq!(
        policy_runs(deny_only, &changed, false, true),
        PolicyRuns::default()
    );
    assert!(policy_runs(machete, &changed, false, true).machete);
    Ok(())
}

#[test]
fn a_deny_config_without_a_pinned_cargo_deny_is_refused() -> TestResult {
    let mut bad = view(&["Cargo.toml"], None)?;
    let deny = RepoPath::new("deny.toml")?;
    bad.tracked_paths.insert(deny.clone());
    bad.files.insert(deny, Vec::new());
    let error = expected_files(&bad).err().map(|error| error.to_string());
    assert!(
        error.is_some_and(|text| text.contains("cargo-deny")),
        "the missing pin must be named"
    );
    Ok(())
}

#[test]
fn a_root_named_policy_cannot_share_a_job_id_with_the_policy_lane() -> TestResult {
    let mut multi = view(&["policy/Cargo.toml", "b/Cargo.toml"], None)?;
    for (path, text) in [("deny.toml", ""), ("mise.toml", DENY_MISE)] {
        let path = RepoPath::new(path)?;
        multi.tracked_paths.insert(path.clone());
        multi.files.insert(path, text.as_bytes().to_vec());
    }
    let text = workflow(&multi)?;
    assert_eq!(text.matches("\n  policy:\n").count(), 1, "{text}");
    assert_eq!(text.matches("\n  rust-policy:\n").count(), 1, "{text}");
    Ok(())
}

#[test]
fn a_draft_run_fails_the_verdict_and_a_conversion_to_draft_starts_a_run() -> TestResult {
    let off = with(&[], None)?;
    assert!(!off.contains("converted_to_draft"), "{off}");
    assert!(off.contains("      - edited\n"), "{off}");
    assert!(!off.contains("draft == true"), "{off}");
    let on = with(&[], Some(b"skip_draft_pull_requests = true\n"))?;
    assert!(on.contains("      - converted_to_draft\n"), "{on}");
    let verdict = on.split("\n  required:\n").nth(1).unwrap_or_default();
    assert!(
        unquoted(verdict).contains("if: ${{ github.event.pull_request.draft == true }}"),
        "{on}"
    );
    Ok(())
}

#[test]
fn the_policy_lane_installs_the_helper_it_plans_and_runs_with() -> TestResult {
    let mut multi = view(&["Cargo.toml"], None)?;
    for (path, text) in [("deny.toml", ""), ("mise.toml", DENY_MISE)] {
        let path = RepoPath::new(path)?;
        multi.tracked_paths.insert(path.clone());
        multi.files.insert(path, text.as_bytes().to_vec());
    }
    let lock = RepoPath::new(".velnor/rust-ci.lock")?;
    multi.tracked_paths.insert(lock.clone());
    multi.files.insert(
        lock,
        std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../samples/helper-lock/view/.velnor/rust-ci.lock"),
        )?,
    );
    let text = workflow(&multi)?;
    assert!(text.contains("Install velnor-ci"), "{text}");
    let policy = text.split("\n  policy:\n").nth(1).unwrap_or_default();
    let policy = policy.split("\n  required:\n").next().unwrap_or_default();
    assert!(policy.contains("cargo-deny"), "{policy}");
    assert!(policy.contains("Install velnor-ci"), "{policy}");
    Ok(())
}

#[test]
fn a_license_text_change_runs_the_policy_lane_only_with_cargo_deny() -> TestResult {
    let changed = |path: &str| -> Result<BTreeSet<RepoPath>, Box<dyn std::error::Error>> {
        Ok(BTreeSet::from([RepoPath::new(path)?]))
    };
    let deny = PolicyTools {
        deny: true,
        machete: false,
    };
    let machete = PolicyTools {
        deny: false,
        machete: true,
    };
    for path in [
        "LICENSE",
        "LICENSE-MIT",
        "crates/a/LICENSE.md",
        "licence.txt",
    ] {
        assert!(
            policy_runs(deny, &changed(path)?, false, true).deny,
            "{path}"
        );
        assert!(
            !policy_runs(machete, &changed(path)?, false, true).deny,
            "{path}"
        );
    }
    assert!(!policy_runs(deny, &changed("docs/readme.md")?, false, true).deny);
    Ok(())
}

#[test]
fn the_policy_lane_checks_every_cargo_root_in_its_own_folder() -> TestResult {
    let mut view = view(&["Cargo.toml", "tools/Cargo.toml"], None)?;
    let deny = RepoPath::new("deny.toml")?;
    view.tracked_paths.insert(deny.clone());
    view.files.insert(deny, Vec::new());
    view.files
        .insert(RepoPath::new("mise.toml")?, DENY_MISE.as_bytes().to_vec());
    let text = workflow(&view)?;
    let policy = text
        .split("\n  policy:\n")
        .nth(1)
        .ok_or("no policy job")?
        .split("\n  required:\n")
        .next()
        .ok_or("no policy job end")?;
    assert_eq!(policy.matches("Dependency policy").count(), 2, "{policy}");
    assert!(policy.contains("cargo-deny check"), "{policy}");
    assert!(
        policy.contains("cargo-deny --manifest-path ''tools/Cargo.toml'' check"),
        "{policy}"
    );
    Ok(())
}

#[test]
fn the_policy_lane_runs_machete_once_from_the_repository_root() -> TestResult {
    let mut view = view(&["Cargo.toml", "tools/Cargo.toml"], None)?;
    let mise = RepoPath::new("mise.toml")?;
    view.files.insert(
        mise,
        b"[tools]\nrust = \"1.99.1\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\ncargo-machete = \"0.9.1\"\n"
            .to_vec(),
    );
    let text = workflow(&view)?;
    let policy = text
        .split("\n  policy:\n")
        .nth(1)
        .ok_or("no policy job")?
        .split("\n  required:\n")
        .next()
        .ok_or("no policy job end")?;
    assert_eq!(policy.matches("cargo-machete").count(), 1, "{policy}");
    assert!(!policy.contains("working-directory"), "{policy}");
    Ok(())
}
