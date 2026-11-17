// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_ci::release::check_branch_prefixes;

fn config(prefix: Option<&str>) -> String {
    let prefix = prefix.map_or_else(String::new, |p| format!("pr_branch_prefix = \"{p}\"\n"));
    format!(
        "[workspace]\nrelease_always = false\ngit_release_draft = true\nsemver_check = false\npublish_no_verify = true\nrelease = false\n{prefix}[[package]]\nname = \"app\"\nrelease = true\ngit_tag_name = \"app-v{{{{ version }}}}\"\n"
    )
}

fn checked(live: Option<&str>, expected: &[&str]) -> Result<(), String> {
    let expected: Vec<(String, String)> = expected
        .iter()
        .map(|prefix| ("release-plz.toml".to_owned(), (*prefix).to_owned()))
        .collect();
    check_branch_prefixes(&[("release-plz.toml".to_owned(), config(live))], &expected)
        .map_err(|e| e.to_string())
}

#[test]
fn the_prefix_the_workflow_was_generated_for_passes() {
    assert_eq!(checked(Some("app-"), &["app-"]), Ok(()));
}

#[test]
fn the_default_prefix_passes_when_none_is_written() {
    assert_eq!(checked(None, &["release-plz-"]), Ok(()));
}

#[test]
fn a_changed_prefix_is_refused_with_a_hint_to_regenerate() {
    let error = checked(Some("new-"), &["old-"]);
    assert!(
        error.is_err_and(|m| m.contains("regenerate release.yml") && m.contains("new-")),
        "a changed pr_branch_prefix must be refused"
    );
}

#[test]
fn a_removed_prefix_falls_back_to_the_default_and_is_refused() {
    assert!(checked(None, &["app-"]).is_err());
}

#[test]
fn a_file_with_no_listed_prefix_is_refused_once_any_is_listed() {
    let files = [("release-plz.toml".to_owned(), config(Some("app-")))];
    let other = [("other.toml".to_owned(), "app-".to_owned())];
    assert!(check_branch_prefixes(&files, &other).is_err());
    assert!(check_branch_prefixes(&files, &[]).is_ok());
}

fn run_binary(live: &str) -> (Option<i32>, String) {
    let dir = std::env::temp_dir().join(format!(
        "velnor-ci-branch-prefix-{}-{}",
        std::process::id(),
        live.len()
    ));
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::write(dir.join("release-plz.toml"), config(Some(live)))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["release", "check-config", "release-plz.toml"])
        .args(["--branch-prefix", "release-plz.toml", "old-"])
        .current_dir(&dir)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let _ignored = std::fs::remove_dir_all(&dir);
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn the_binary_exits_1_on_a_changed_prefix_without_asking_github() {
    let (code, stderr) = run_binary("new-");
    assert_eq!(code, Some(1));
    assert!(stderr.contains("regenerate release.yml"), "{stderr}");
}

#[test]
fn the_binary_passes_an_unchanged_prefix() {
    assert_eq!(run_binary("old-").0, Some(0));
}
