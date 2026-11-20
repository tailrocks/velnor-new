// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_gen::expected_with_notes;
use velnor_model::{RepoPath, RepoView};

use crate::base_save::pinned;
use crate::facts::TestResult;

pub(crate) const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
pub(crate) const SOURCE: &str = "https://github.com/acme/shared-lib";
pub(crate) const DECLARED: &str =
    "[public_sources]\ngit = ['https://github.com/acme/shared-lib']\n";

pub(crate) fn locked(
    source: &str,
    config: &str,
    helper: &str,
) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut view = pinned(helper, config)?;
    let lock = format!(
        "version = 4\n\n[[package]]\nname = \"package-0\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"dep\"\nversion = \"1.0.0\"\nsource = \"{source}\"\n"
    );
    view.files
        .insert(RepoPath::new("Cargo.lock")?, lock.into_bytes());
    Ok(view)
}

pub(crate) fn pinned_source() -> String {
    format!("git+{SOURCE}?rev=v1#{SHA}")
}

struct Output {
    workflow: String,
    notes: Vec<String>,
}

impl Output {
    fn saves_bases(&self) -> bool {
        self.workflow.contains("cache decide")
    }

    fn note(&self, part: &str) -> Option<&String> {
        self.notes.iter().find(|note| note.contains(part))
    }
}

fn render(view: &RepoView) -> Result<Output, Box<dyn std::error::Error>> {
    let (files, notes) = expected_with_notes(view)?;
    Ok(Output {
        workflow: files.into_values().collect::<Vec<_>>().join("\n"),
        notes,
    })
}

#[test]
fn a_declared_commit_pinned_github_dependency_keeps_the_base_caches() -> TestResult {
    let output = render(&locked(&pinned_source(), DECLARED, "0.1.108")?)?;
    assert!(output.saves_bases(), "{}", output.workflow);
    assert!(
        output.note("base caches are not saved").is_none(),
        "{:?}",
        output.notes
    );
    assert!(
        output.note("[public_sources]").is_none(),
        "{:?}",
        output.notes
    );
    Ok(())
}

#[test]
fn an_undeclared_commit_pinned_github_dependency_suggests_the_public_declaration() -> TestResult {
    let output = render(&locked(&pinned_source(), "", "0.1.108")?)?;
    assert!(!output.saves_bases());
    assert!(
        output.note("base caches are not saved").is_some(),
        "{:?}",
        output.notes
    );
    let note = output.note("[public_sources]").ok_or("no suggestion")?;
    assert!(note.contains(&format!("git = ['{SOURCE}']")), "{note}");
    assert!(
        note.contains("Cargo.lock") && note.contains("dep"),
        "{note}"
    );
    Ok(())
}

#[test]
fn a_declaration_does_not_admit_other_sources() -> TestResult {
    let sources = [
        format!("git+{SOURCE}?rev=v1#abc"),
        format!("git+{SOURCE}?rev=v1"),
        format!("git+{SOURCE}?rev=v1#{}", SHA.to_uppercase()),
        format!("git+{SOURCE}?rev=v1#{SHA}0"),
        format!("git+ssh://git@github.com/acme/shared-lib#{SHA}"),
        format!("git+http://github.com/acme/shared-lib#{SHA}"),
        format!("git+https://deploy@github.com/acme/shared-lib#{SHA}"),
        format!("git+https://github.com:8443/acme/shared-lib#{SHA}"),
        format!("git+https://gitlab.com/acme/shared-lib#{SHA}"),
        format!("git+https://github.com/acme/other-lib#{SHA}"),
        format!("git+https://github.com/acme/shared-lib/sub#{SHA}"),
    ];
    for source in sources {
        let output = render(&locked(&source, DECLARED, "0.1.108")?)?;
        assert!(!output.saves_bases(), "{source}");
        assert!(
            output.note("base caches are not saved").is_some(),
            "{source}"
        );
    }
    Ok(())
}

#[test]
fn the_declaration_matches_the_repository_without_regard_to_case_or_a_dot_git_suffix() -> TestResult
{
    for source in [
        format!("git+https://github.com/ACME/Shared-Lib.git#{SHA}"),
        format!("git+{SOURCE}.git?branch=main#{SHA}"),
    ] {
        let output = render(&locked(&source, DECLARED, "0.1.108")?)?;
        assert!(output.saves_bases(), "{source}");
    }
    Ok(())
}

#[test]
fn a_declared_repository_that_no_lock_names_is_reported() -> TestResult {
    let output = render(&locked(
        "sparse+https://index.crates.io/",
        DECLARED,
        "0.1.108",
    )?)?;
    let note = output.note("[public_sources].git lists").ok_or("no note")?;
    assert!(note.contains("acme/shared-lib"), "{note}");
    Ok(())
}

#[test]
fn the_remote_mbx_cache_follows_the_declaration() -> TestResult {
    let mbx = "[cache]\nmbx_remote = \"github-actions\"\n";
    let declared = format!("{DECLARED}{mbx}");
    let output = render(&locked(&pinned_source(), &declared, "0.1.108")?)?;
    assert!(output.workflow.contains("mr-boxington-action"));
    let refused = render(&locked(&pinned_source(), mbx, "0.1.108")?)
        .err()
        .ok_or("accepted")?
        .to_string();
    assert!(refused.contains("mbx_remote"), "{refused}");
    Ok(())
}

#[test]
fn the_declaration_needs_the_helper_that_checks_it() -> TestResult {
    for (version, refused) in [("0.1.107", true), ("0.1.108", false)] {
        let outcome = render(&locked(&pinned_source(), DECLARED, version)?);
        match outcome {
            Err(error) => {
                assert!(refused, "{version}: {error}");
                assert!(error.to_string().contains("[public_sources]"), "{error}");
            }
            Ok(_) => assert!(!refused, "{version}"),
        }
    }
    Ok(())
}

#[test]
fn only_a_plain_github_repository_url_can_be_declared() -> TestResult {
    let invalid = [
        "git = []",
        "git = ['https://gitlab.com/acme/lib']",
        "git = ['http://github.com/acme/lib']",
        "git = ['https://github.com/acme/lib.git']",
        "git = ['https://github.com/acme']",
        "git = ['https://github.com/acme/lib/tree/main']",
        "git = ['acme/lib']",
        "git = ['https://github.com/acme/lib', 'https://github.com/ACME/Lib']",
        "git = ['https://github.com/acme/lib']\nextra = 1",
    ];
    for body in invalid {
        let config = format!("[public_sources]\n{body}\n");
        let error = render(&locked(&pinned_source(), &config, "0.1.108")?)
            .err()
            .ok_or_else(|| format!("accepted: {body}"))?
            .to_string();
        assert!(
            error.contains("public_sources") || error.contains("unknown field"),
            "{body}: {error}"
        );
    }
    Ok(())
}
