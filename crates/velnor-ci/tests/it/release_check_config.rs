// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_ci::release::check_config;

const COMPLIANT: &str = "[workspace]\nrelease_always = false\ngit_release_draft = true\nsemver_check = false\npublish_no_verify = true\ngit_tag_name = \"v{{ version }}\"\n";

fn checked(text: &str) -> Result<String, String> {
    check_config(&[("release-plz.toml".to_owned(), text.to_owned())]).map_err(|e| e.to_string())
}

#[test]
fn a_release_plz_config_without_commands_passes() {
    assert_eq!(
        checked(&format!(
            "{COMPLIANT}[changelog]\nprotect_breaking_commits = true\n"
        )),
        Ok(String::new())
    );
}

#[test]
fn a_command_key_is_refused_however_toml_spells_it() {
    for text in [
        "[changelog]\ncommit_preprocessors = [{ pattern = \"x\", replace_command = \"curl evil\" }]\n",
        "[changelog]\ncommit_preprocessors = [{ pattern = \"x\", \"replace_command\" = \"curl evil\" }]\n",
        "[changelog]\ncommit_preprocessors = [{ pattern = \"x\", 'replace_command' = \"curl evil\" }]\n",
        "[changelog]\ncommit_preprocessors = [{ pattern = \"x\", \"replace\\u005fcommand\" = \"curl evil\" }]\n",
        "[changelog]\ncommit_preprocessors = [{ pattern = \"x\", \"\\x72eplace_command\" = \"curl evil\" }]\n",
        "[workspace]\nchangelog_config = \"cliff.toml\"\n",
        "[[package]]\nname = \"a\"\n\"changelog\\u005fconfig\" = \"cliff.toml\"\n",
    ] {
        let error = checked(text);
        assert!(
            error
                .as_ref()
                .is_err_and(|message| message.contains("runs an external command")),
            "{text}: {error:?}"
        );
    }
}

#[test]
fn a_file_that_is_not_toml_is_refused_and_every_file_is_checked() {
    assert!(checked("[workspace\n").is_err_and(|message| message.contains("not valid TOML")));
    let both = check_config(&[
        ("a/release-plz.toml".to_owned(), COMPLIANT.to_owned()),
        (
            "b/release-plz.toml".to_owned(),
            "[workspace]\nchangelog_config = \"x\"\n".to_owned(),
        ),
    ]);
    assert!(both.is_err_and(|e| e.to_string().starts_with("b/release-plz.toml")));
}

#[test]
fn the_binary_exits_1_on_an_escaped_command_key_and_0_on_a_clean_file() {
    let dir = std::env::temp_dir().join(format!("velnor-ci-check-config-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::write(dir.join("clean.toml"), COMPLIANT).unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::write(
        dir.join("evil.toml"),
        "[changelog]\ncommit_preprocessors = [{ \"replace_command\" = \"x\" }]\n",
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    let run = |file: &str| {
        std::process::Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
            .args(["release", "check-config"])
            .arg(file)
            .current_dir(&dir)
            .status()
            .unwrap_or_else(|e| unreachable!("{e}"))
            .code()
    };
    let (ok, refused) = (run("clean.toml"), run("evil.toml"));
    let _ignored = std::fs::remove_dir_all(&dir);
    assert_eq!((ok, refused), (Some(0), Some(1)));
}

#[test]
fn a_setting_the_generator_refuses_is_refused_at_runtime_too() {
    for (old, new) in [
        ("release_always = false", "release_always = true"),
        ("git_release_draft = true", "git_release_draft = false"),
        ("semver_check = false", "semver_check = true"),
        ("git_tag_name = \"v{{ version }}\"", "git_tag_name = \"\""),
        (
            "git_tag_name = \"v{{ version }}\"",
            "git_tag_name = \"v{{ version }}\"\ngit_release_enable = false",
        ),
        (
            "git_tag_name = \"v{{ version }}\"",
            "git_tag_name = \"v{{ version }}\"\npublish = true\ngit_only = true",
        ),
        (
            "git_tag_name = \"v{{ version }}\"",
            "git_tag_name = \"v{{ version }}\"\npr_branch_prefix = \"\"",
        ),
    ] {
        let text = COMPLIANT.replace(old, new);
        assert!(checked(&text).is_err(), "{new}");
    }
}

fn two_roots(a: &str, b: &str) -> Result<String, String> {
    check_config(&[
        ("a/release-plz.toml".to_owned(), a.to_owned()),
        ("b/release-plz.toml".to_owned(), b.to_owned()),
    ])
    .map_err(|e| e.to_string())
}

#[test]
fn two_roots_sharing_a_tag_pattern_are_refused_at_runtime() {
    let error = two_roots(COMPLIANT, COMPLIANT);
    assert!(
        error.is_err_and(|message| message.contains("v{{ version }}")),
        "shared tag"
    );
}

#[test]
fn two_roots_with_overlapping_pr_branch_prefixes_are_refused_at_runtime() {
    let tagged = |tag: &str| COMPLIANT.replace("v{{ version }}", tag);
    let error = two_roots(&tagged("a-v{{ version }}"), &tagged("b-v{{ version }}"));
    assert!(
        error.is_err_and(|message| message.contains("release-plz-")),
        "shared prefix"
    );
    let apart =
        |tag: &str, prefix: &str| format!("{}pr_branch_prefix = \"{prefix}\"\n", tagged(tag));
    assert_eq!(
        two_roots(
            &apart("a-v{{ version }}", "a-plz-"),
            &apart("b-v{{ version }}", "b-plz-")
        ),
        Ok(String::new())
    );
}
