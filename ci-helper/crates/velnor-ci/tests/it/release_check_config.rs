// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_ci::release::check_config;

fn checked(text: &str) -> Result<String, String> {
    check_config(&[("release-plz.toml".to_owned(), text.to_owned())]).map_err(|e| e.to_string())
}

#[test]
fn a_release_plz_config_without_commands_passes() {
    assert_eq!(
        checked(
            "[workspace]\ngit_release_draft = true\n[changelog]\nprotect_breaking_commits = true\n"
        ),
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
        ("a/release-plz.toml".to_owned(), "[workspace]\n".to_owned()),
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
    std::fs::write(dir.join("clean.toml"), "[workspace]\n").unwrap_or_else(|e| unreachable!("{e}"));
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
