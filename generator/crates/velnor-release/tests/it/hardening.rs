// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_release::ReleaseError;

use crate::fixture::{TestResult, files, plz_refusal_of, refusal};

#[test]
fn a_tag_template_without_a_version_is_refused() -> TestResult {
    for template in ["app", "{{ package }}", "{{ package }}-latest"] {
        let to = format!("git_tag_name = \"{template}\"");
        let error = plz_refusal_of("git_tag_name = \"{{ package }}-v{{ version }}\"", &to)?;
        assert!(
            matches!(error, Some(ReleaseError::TagTemplateWithoutVersion { .. })),
            "{template}: {error:?}"
        );
    }
    Ok(())
}

#[test]
fn switching_off_the_tag_or_the_github_release_is_refused() -> TestResult {
    for field in ["git_tag_enable", "git_release_enable"] {
        for place in ["[workspace]\n", "name = \"app\"\n"] {
            let error = plz_refusal_of(place, &format!("{place}{field} = false\n"))?;
            assert!(
                matches!(error, Some(ReleaseError::ReleaseSwitchOff { .. })),
                "{field} {place}: {error:?}"
            );
        }
        let on = plz_refusal_of("[workspace]\n", &format!("[workspace]\n{field} = true\n"))?;
        assert_eq!(on, None);
    }
    Ok(())
}

#[test]
fn an_empty_release_pr_branch_prefix_is_refused() -> TestResult {
    let error = plz_refusal_of("[workspace]\n", "[workspace]\npr_branch_prefix = \"\"\n")?;
    assert!(
        matches!(error, Some(ReleaseError::EmptyPrBranchPrefix { .. })),
        "{error:?}"
    );
    let named = plz_refusal_of(
        "[workspace]\n",
        "[workspace]\npr_branch_prefix = \"rel-\"\n",
    )?;
    assert_eq!(named, None);
    Ok(())
}

#[test]
fn publish_together_with_git_only_is_refused() -> TestResult {
    let both = "[workspace]\npublish = true\ngit_only = true\n";
    let error = plz_refusal_of("[workspace]\n", both)?;
    assert!(
        matches!(error, Some(ReleaseError::PublishWithGitOnly { .. })),
        "{error:?}"
    );
    let git_only = plz_refusal_of("[workspace]\n", "[workspace]\ngit_only = true\n")?;
    assert_eq!(git_only, None);
    Ok(())
}

#[test]
fn every_configured_release_plz_entry_must_be_locked_at_its_version() {
    let both = "[tools]\nrelease-plz = \"0.4.3\"\n\"aqua:release-plz/release-plz\" = \"0.4.3\"\n";
    let lock_one = "[[tools.release-plz]]\nversion = \"0.4.3\"\n";
    let lock_both =
        format!("{lock_one}\n[[tools.\"aqua:release-plz/release-plz\"]]\nversion = \"0.4.3\"\n");
    let mismatch =
        format!("{lock_one}\n[[tools.\"aqua:release-plz/release-plz\"]]\nversion = \"0.4.2\"\n");
    let refuse = |lock: &str| {
        let mut files = files();
        files.mise_toml = both.to_owned();
        files.mise_lock = lock.to_owned();
        refusal(&files)
    };
    assert_eq!(refuse(&lock_both), None);
    assert_eq!(
        refuse(lock_one),
        Some(ReleaseError::ReleasePlzNotInMiseLock)
    );
    assert!(matches!(
        refuse(&mismatch),
        Some(ReleaseError::ReleasePlzLockMismatch { .. })
    ));
}
