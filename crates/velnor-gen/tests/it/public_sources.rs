// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write;

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
    locked_all(&[source], config, helper)
}

pub(crate) fn locked_all(
    sources: &[&str],
    config: &str,
    helper: &str,
) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut view = pinned(helper, config)?;
    let mut lock =
        "version = 4\n\n[[package]]\nname = \"package-0\"\nversion = \"0.1.0\"\n".to_owned();
    for (index, source) in sources.iter().enumerate() {
        write!(
            lock,
            "\n[[package]]\nname = \"dep{index}\"\nversion = \"1.0.0\"\nsource = \"{source}\"\n"
        )?;
    }
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
    let output = render(&locked(&pinned_source(), DECLARED, "0.1.110")?)?;
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
    let output = render(&locked(&pinned_source(), "", "0.1.110")?)?;
    assert!(!output.saves_bases());
    assert!(
        output.note("base caches are not saved").is_some(),
        "{:?}",
        output.notes
    );
    let note = output.note("[public_sources]").ok_or("no suggestion")?;
    assert!(note.contains(&format!("git = ['{SOURCE}']")), "{note}");
    assert!(
        note.contains("Cargo.lock") && note.contains("dep0"),
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
    let sound = pinned_source();
    for source in sources {
        let output = render(&locked_all(&[&sound, &source], DECLARED, "0.1.110")?)?;
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
        let output = render(&locked(&source, DECLARED, "0.1.110")?)?;
        assert!(output.saves_bases(), "{source}");
    }
    Ok(())
}

fn refused_unused(view: &RepoView) -> Result<String, Box<dyn std::error::Error>> {
    Ok(render(view)
        .err()
        .ok_or("accepted an unused declaration")?
        .to_string())
}

#[test]
fn a_declared_repository_that_no_lock_names_is_refused() -> TestResult {
    let error = refused_unused(&locked(
        "sparse+https://index.crates.io/",
        DECLARED,
        "0.1.110",
    )?)?;
    assert!(error.contains("[public_sources].git lists"), "{error}");
    assert!(error.contains("acme/shared-lib"), "{error}");
    Ok(())
}

#[test]
fn a_declared_repository_is_refused_when_no_external_package_remains() -> TestResult {
    let error = refused_unused(&locked_all(&[], DECLARED, "0.1.110")?)?;
    assert!(error.contains("acme/shared-lib"), "{error}");
    Ok(())
}

#[test]
fn only_the_unused_declarations_are_named() -> TestResult {
    let config = "[public_sources]\ngit = ['https://github.com/acme/shared-lib', 'https://github.com/acme/gone', 'https://github.com/acme/also-gone']\n";
    let error = refused_unused(&locked(&pinned_source(), config, "0.1.110")?)?;
    assert!(
        error.contains("acme/gone, acme/also-gone") && !error.contains("acme/shared-lib,"),
        "{error}"
    );
    Ok(())
}

#[test]
fn a_repository_cannot_be_both_private_and_public() -> TestResult {
    let both = format!(
        "{DECLARED}[private_sources]\napp_id = 'vars.APP_ID'\nprivate_key = 'secrets.APP_KEY'\nrepositories = ['ACME/Shared-Lib']\n"
    );
    let error = render(&locked(&pinned_source(), &both, "0.1.110")?)
        .err()
        .ok_or("accepted")?
        .to_string();
    assert!(
        error.contains("acme/shared-lib")
            && error.contains("both [private_sources] and [public_sources]"),
        "{error}"
    );
    let apart = format!(
        "{DECLARED}[private_sources]\napp_id = 'vars.APP_ID'\nprivate_key = 'secrets.APP_KEY'\nrepositories = ['acme/other']\n"
    );
    let outcome = render(&locked(&pinned_source(), &apart, "0.1.110")?);
    assert!(
        !outcome
            .err()
            .is_some_and(|error| error.to_string().contains("both [private_sources]")),
        "disjoint tables were refused as overlapping"
    );
    Ok(())
}

#[test]
fn the_remote_mbx_cache_follows_the_declaration() -> TestResult {
    let mbx = "[cache]\nmbx_remote = \"github-actions\"\n";
    let declared = format!("{DECLARED}{mbx}");
    let output = render(&locked(&pinned_source(), &declared, "0.1.110")?)?;
    assert!(output.workflow.contains("mr-boxington-action"));
    let refused = render(&locked(&pinned_source(), mbx, "0.1.110")?)
        .err()
        .ok_or("accepted")?
        .to_string();
    assert!(refused.contains("mbx_remote"), "{refused}");
    Ok(())
}

#[test]
fn the_remote_mbx_cache_is_refused_when_private_credentials_sit_beside_a_public_declaration()
-> TestResult {
    let both = format!(
        "{DECLARED}[cache]\nmbx_remote = \"github-actions\"\n[private_sources]\napp_id = 'vars.APP_ID'\nprivate_key = 'secrets.APP_KEY'\nrepositories = ['acme/other']\n"
    );
    let refused = render(&locked(&pinned_source(), &both, "0.1.110")?)
        .err()
        .ok_or("a fork-readable cache was accepted beside private credentials")?
        .to_string();
    assert!(
        refused.contains("mbx_remote") && refused.contains("private submodule"),
        "{refused}"
    );
    Ok(())
}

#[test]
fn private_credentials_beside_a_public_declaration_keep_the_base_caches_off_without_the_remote_cache()
-> TestResult {
    let both = format!(
        "{DECLARED}[private_sources]\napp_id = 'vars.APP_ID'\nprivate_key = 'secrets.APP_KEY'\nrepositories = ['acme/other']\n"
    );
    let output = render(&locked(&pinned_source(), &both, "0.1.110")?)?;
    assert!(!output.saves_bases(), "{}", output.workflow);
    assert!(
        output.note("private submodule").is_some(),
        "{:?}",
        output.notes
    );
    Ok(())
}

#[test]
fn the_declaration_needs_the_helper_that_checks_it() -> TestResult {
    for (version, refused) in [("0.1.107", true), ("0.1.109", true), ("0.1.110", false)] {
        let outcome = render(&locked(&pinned_source(), DECLARED, version)?);
        match outcome {
            Err(error) => {
                assert!(refused, "{version}: {error}");
                assert!(error.to_string().contains("`public_sources`"), "{error}");
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
        "git = ['https://github.com/.acme/lib']",
        "git = ['https://github.com/acme/.lib']",
        "git = ['https://github.com/acme/lib/tree/main']",
        "git = ['acme/lib']",
        "git = ['https://github.com/acme/lib', 'https://github.com/ACME/Lib']",
        "git = ['https://github.com/acme/lib']\nextra = 1",
    ];
    for body in invalid {
        let config = format!("[public_sources]\n{body}\n");
        let error = render(&locked(&pinned_source(), &config, "0.1.110")?)
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
