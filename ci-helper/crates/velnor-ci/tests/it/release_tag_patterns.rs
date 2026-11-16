// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_ci::release::check_tag_patterns;

fn config(tag: &str) -> String {
    format!(
        "[workspace]\nrelease_always = false\ngit_release_draft = true\nsemver_check = false\npublish_no_verify = true\nrelease = false\n[[package]]\nname = \"app\"\nrelease = true\ngit_tag_name = \"{tag}\"\n"
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

fn run_binary(folder: &str, tag: &str) -> (Option<i32>, String) {
    let dir = std::env::temp_dir().join(format!(
        "velnor-ci-tag-pattern-{}-{}",
        std::process::id(),
        folder.len()
    ));
    let root = dir.join(folder);
    std::fs::create_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::write(root.join("release-plz.toml"), config(tag))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let file = format!("{folder}/release-plz.toml");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["release", "check-config", &file])
        .args(["--tag-pattern", &file, "app-v{{ version }}"])
        .args(["--repo", "tailrocks/velnor-new"])
        .env("GITHUB_TOKEN", "unused")
        .env("GITHUB_API_URL", "http://127.0.0.1:9")
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
fn the_binary_exits_1_on_a_changed_pattern_before_it_asks_github() {
    let (code, stderr) = run_binary("plain", "evil-{{ version }}");
    assert_eq!(code, Some(1));
    assert!(stderr.contains("provisioned"), "{stderr}");
}

#[test]
fn a_root_folder_holding_an_equals_sign_is_matched_whole() {
    let (code, stderr) = run_binary("a=b", "app-v{{ version }}");
    assert_eq!(code, Some(1));
    assert!(!stderr.contains("provisioned"), "{stderr}");
    let (_, stderr) = run_binary("a=bc", "evil-{{ version }}");
    assert!(stderr.contains("a=bc/release-plz.toml"), "{stderr}");
    assert!(stderr.contains("provisioned"), "{stderr}");
}

#[test]
fn a_tag_pattern_without_a_repository_is_refused() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["release", "check-config", "release-plz.toml"])
        .args(["--tag-pattern", "release-plz.toml", "app-v{{ version }}"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(output.status.code(), Some(2));
}
