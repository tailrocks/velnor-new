// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_ci::release::check_tag_patterns;

fn config(tag: &str) -> String {
    format!(
        "[workspace]\nrelease_always = false\ngit_release_draft = true\nsemver_check = false\npublish_no_verify = true\ngit_tag_name = \"{tag}\"\n[[package]]\nname = \"app\"\n"
    )
}

fn checked(tag: &str, expected: &[&str]) -> Result<(), String> {
    let expected: Vec<(String, String)> = expected
        .iter()
        .map(|pattern| ("release-plz.toml".to_owned(), (*pattern).to_owned()))
        .collect();
    check_tag_patterns(&[("release-plz.toml".to_owned(), config(tag))], &expected)
        .map_err(|e| e.to_string())
}

#[test]
fn the_pattern_the_rulesets_were_provisioned_for_passes() {
    assert_eq!(
        checked("app-v{{ version }}", &["app-v{{ version }}"]),
        Ok(())
    );
}

#[test]
fn a_changed_pattern_is_refused() {
    let error = checked("other-{{ version }}", &["app-v{{ version }}"]);
    assert!(
        error.is_err_and(|message| message.contains("provisioned")),
        "a changed git_tag_name must be refused"
    );
}

#[test]
fn a_file_with_no_listed_pattern_is_refused_once_any_is_listed() {
    let files = [("release-plz.toml".to_owned(), config("app-v{{ version }}"))];
    let other = [("other.toml".to_owned(), "x".to_owned())];
    assert!(check_tag_patterns(&files, &other).is_err());
    assert!(check_tag_patterns(&files, &[]).is_ok());
}

#[test]
fn the_binary_exits_1_on_a_changed_pattern() {
    let dir = std::env::temp_dir().join(format!("velnor-ci-tag-pattern-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::write(dir.join("release-plz.toml"), config("evil-{{ version }}"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let code = std::process::Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["release", "check-config", "release-plz.toml"])
        .args(["--tag-pattern", "release-plz.toml=app-v{{ version }}"])
        .current_dir(&dir)
        .status()
        .unwrap_or_else(|e| unreachable!("{e}"))
        .code();
    let _ignored = std::fs::remove_dir_all(&dir);
    assert_eq!(code, Some(1));
}
