// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_release::{ReleaseError, ReleasePlan};

use crate::fixture::{PLZ, TestResult, edit, files, plz_refusal_of, refusal};

const WORKSPACE_ONLY: &str = "[workspace]\ngit_tag_name = \"v{{ version }}\"\ngit_release_draft = true\nrelease_always = false\nsemver_check = false\npublish_no_verify = true\n";

#[test]
fn a_package_that_turns_release_always_back_on_is_refused() -> TestResult {
    let error = plz_refusal_of(
        "name = \"app\"\n",
        "name = \"app\"\nrelease_always = true\n",
    )?;
    assert!(
        matches!(error, Some(ReleaseError::ReleaseAlwaysNotFalse { .. })),
        "{error:?}"
    );
    Ok(())
}

#[test]
fn changelog_processors_that_run_commands_or_read_other_files_are_refused() {
    for extra in [
        "\n[changelog]\ncommit_preprocessors = [{ pattern = \"a\", replace_command = \"sh -c x\" }]\n",
        "\n[changelog]\npostprocessors = [{ pattern = \"a\", replace_command = \"x\" }]\n",
        "\n[[package]]\nname = \"lib\"\nchangelog_config = \"cliff.toml\"\n",
    ] {
        let mut files = files();
        files
            .release_plz_toml
            .insert(".".to_owned(), format!("{PLZ}{extra}"));
        let error = refusal(&files);
        assert!(
            matches!(error, Some(ReleaseError::ExecutableConfig { .. })),
            "{extra}: {error:?}"
        );
    }
    let mut plain = files();
    plain.release_plz_toml.insert(
        ".".to_owned(),
        format!("{PLZ}\n[changelog]\nprotect_breaking_commits = true\n"),
    );
    assert_eq!(refusal(&plain), None);
}

#[test]
fn two_workspace_only_roots_with_the_same_literal_tag_are_refused() {
    let mut two = files();
    for (root, prefix) in [(".", "a-"), ("tools", "b-")] {
        two.release_plz_toml.insert(
            root.to_owned(),
            format!("{WORKSPACE_ONLY}pr_branch_prefix = \"{prefix}\"\n"),
        );
    }
    two.release_toml = Some("roots = [\".\", \"tools\"]\n".to_owned());
    assert!(matches!(
        ReleasePlan::read(&two).err(),
        Some(ReleaseError::DuplicateTagPattern { .. })
    ));
    for root in [".", "tools"] {
        let text =
            two.release_plz_toml[root].replace("v{{ version }}", "{{ package }}-v{{ version }}");
        two.release_plz_toml.insert(root.to_owned(), text);
    }
    assert_eq!(refusal(&two), None);
}

#[test]
fn unlisted_packages_inherit_the_workspace_so_it_must_name_a_tag_unless_it_releases_nothing()
-> TestResult {
    let listed = edit(
        PLZ,
        "name = \"app\"\n",
        "name = \"app\"\ngit_tag_name = \"app-v{{ version }}\"\n",
    )?;
    let no_default = edit(
        &listed,
        "git_tag_name = \"{{ package }}-v{{ version }}\"\n",
        "",
    )?;
    let mut files = files();
    files
        .release_plz_toml
        .insert(".".to_owned(), no_default.clone());
    assert!(matches!(
        refusal(&files),
        Some(ReleaseError::MissingGitTagName { package: None, .. })
    ));
    let allowlist = edit(
        &no_default,
        "[workspace]\n",
        "[workspace]\nrelease = false\n",
    )?
    .replace("name = \"app\"\n", "name = \"app\"\nrelease = true\n");
    files.release_plz_toml.insert(".".to_owned(), allowlist);
    assert_eq!(refusal(&files), None);
    Ok(())
}

#[test]
fn an_unlisted_package_that_inherits_publishing_makes_the_root_publish() -> TestResult {
    let git_only_package = edit(PLZ, "name = \"app\"\n", "name = \"app\"\ngit_only = true\n")?;
    let mut files = files();
    files
        .release_plz_toml
        .insert(".".to_owned(), git_only_package);
    let plan = ReleasePlan::read(&files)?;
    assert!(plan.roots[0].publishes_to_crates_io);
    let allowlist = edit(
        &PLZ.replace(
            "name = \"app\"\n",
            "name = \"app\"\nrelease = true\ngit_only = true\n",
        ),
        "[workspace]\n",
        "[workspace]\nrelease = false\n",
    )?;
    files.release_plz_toml.insert(".".to_owned(), allowlist);
    assert!(!ReleasePlan::read(&files)?.roots[0].publishes_to_crates_io);
    Ok(())
}

#[test]
fn release_plz_declared_only_in_the_dot_mise_file_is_found_and_checked() {
    let mut files = files();
    files.mise_tomls = vec!["[tools]\nbun = \"1.4.4\"\n".to_owned()];
    assert_eq!(refusal(&files), Some(ReleaseError::ReleasePlzNotInMiseToml));
    files
        .mise_tomls
        .push("[tools]\nrelease-plz = \"0.4.3\"\n".to_owned());
    assert_eq!(refusal(&files), None);
    files
        .mise_tomls
        .push("[tools]\nrelease-plz = \"0.4.2\"\n".to_owned());
    assert!(matches!(
        refusal(&files),
        Some(ReleaseError::ReleasePlzLockMismatch { .. })
    ));
    files.mise_tomls = vec!["[tools]\nrelease-plz = \"0.4.3\"\n".to_owned()];
    files
        .mise_tomls
        .push("[tools]\nbun = \"1.4.4\"\n".to_owned());
    assert_eq!(refusal(&files), None);
}

#[test]
fn a_literal_workspace_tag_shared_by_two_unlisted_members_is_refused_naming_both() {
    let mut files = files();
    files
        .release_plz_toml
        .insert(".".to_owned(), WORKSPACE_ONLY.to_owned());
    crate::fixture::with_metadata(
        &mut files,
        &[(
            "Cargo.toml",
            &[("app", "Cargo.toml"), ("lib", "lib/Cargo.toml")],
        )],
    );
    assert!(matches!(
        refusal(&files),
        Some(ReleaseError::DuplicateTagPattern { packages, .. }) if packages == ["app", "lib"]
    ));
    let templated = WORKSPACE_ONLY.replace("v{{ version }}", "{{ package }}-v{{ version }}");
    files.release_plz_toml.insert(".".to_owned(), templated);
    assert_eq!(refusal(&files), None);
    crate::fixture::with_metadata(&mut files, &[("Cargo.toml", &[("app", "Cargo.toml")])]);
    files
        .release_plz_toml
        .insert(".".to_owned(), WORKSPACE_ONLY.to_owned());
    assert_eq!(refusal(&files), None);
}

#[test]
fn templated_workspace_tags_that_render_the_same_for_two_unlisted_members_are_refused() {
    let mut files = files();
    files.release_plz_toml.insert(
        ".".to_owned(),
        WORKSPACE_ONLY.replace("v{{ version }}", "{{ package }}{{ version }}"),
    );
    crate::fixture::with_metadata(
        &mut files,
        &[(
            "Cargo.toml",
            &[("a", "Cargo.toml"), ("a1", "a1/Cargo.toml")],
        )],
    );
    assert!(matches!(
        refusal(&files),
        Some(ReleaseError::DuplicateTagPattern { packages, .. }) if packages == ["a", "a1"]
    ));
}

#[test]
fn an_unlisted_member_may_not_share_a_tag_with_a_listed_package() {
    let mut files = files();
    files.release_plz_toml.insert(
        ".".to_owned(),
        format!(
            "{}\n[[package]]\nname = \"tool\"\ngit_tag_name = \"a{{{{ version }}}}\"\n",
            WORKSPACE_ONLY.replace("v{{ version }}", "{{ package }}{{ version }}")
        ),
    );
    crate::fixture::with_metadata(
        &mut files,
        &[(
            "Cargo.toml",
            &[("a", "Cargo.toml"), ("tool", "tool/Cargo.toml")],
        )],
    );
    assert!(matches!(
        refusal(&files),
        Some(ReleaseError::DuplicateTagPattern { packages, .. }) if packages == ["a", "tool"]
    ));
}
