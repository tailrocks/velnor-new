// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::runner_family_copy_sources::assert_image_context_selection;
use super::scoped_workspace::{DOCS_CONFIG, TestResult, commit, plan, put, scoped_workspace};

#[test]
fn platform_dockerfiles_select_independently() -> TestResult {
    let config = format!(
        "{DOCS_CONFIG}\n[[images]]\nid = \"app\"\nrepository = \"ghcr.io/acme/app\"\ncontext = \"image\"\nplatforms = [\"linux_amd64\", \"linux_arm64\"]\ndockerfile = {{ linux_amd64 = \"image/amd64.Dockerfile\", linux_arm64 = \"image/arm64.Dockerfile\" }}\n"
    );
    let mut repo = scoped_workspace(&config)?;
    put(
        repo.root(),
        "image/amd64.Dockerfile",
        "FROM scratch\nUNKNOWN value\n",
    )?;
    put(
        repo.root(),
        "image/arm64.Dockerfile",
        "FROM scratch\nCOPY arm.txt /arm.txt\n",
    )?;
    put(repo.root(), "image/arm.txt", "initial\n")?;
    repo.base = commit(repo.root(), "platform image baseline")?;
    for (path, amd, arm) in [
        ("image/other.txt", "run", "not_affected"),
        ("image/arm.txt", "run", "run"),
        ("outside.txt", "not_affected", "not_affected"),
    ] {
        super::workflow_configuration_fixture::git(
            &["checkout", "--detach", &repo.base],
            repo.root(),
        )?;
        put(repo.root(), path, "changed\n")?;
        let head = commit(repo.root(), "change platform input")?;
        let jobs = plan(repo.root(), "pull_request", Some(&repo.base), &head)?.jobs()?;
        assert_eq!(
            jobs.get("image-app-amd64").map(String::as_str),
            Some(amd),
            "{path}"
        );
        assert_eq!(
            jobs.get("image-app-arm64").map(String::as_str),
            Some(arm),
            "{path}"
        );
    }
    Ok(())
}

#[test]
fn copy_exclusions_limit_context_selection() -> TestResult {
    for (instructions, path, expected, precise) in [
        (
            "COPY --exclude=*.txt src/ /dest/",
            "src/ignored.txt",
            "not_affected",
            true,
        ),
        (
            "COPY --exclude=*.txt src/ /dest/",
            "src/included.rs",
            "run",
            true,
        ),
        (
            "COPY --exclude=secret.txt src/ /dest/",
            "src/secret.txt",
            "not_affected",
            true,
        ),
        (
            "COPY --exclude=secret.txt src/ /dest/",
            "src/public.txt",
            "run",
            true,
        ),
        (
            "COPY --exclude=secret.txt src/ /dest/\nCOPY src/ /other/",
            "src/secret.txt",
            "run",
            true,
        ),
        (
            "COPY --exclude=**/secret.txt src/ /dest/",
            "src/secret.txt",
            "run",
            false,
        ),
    ] {
        assert_image_context_selection(
            &format!("FROM scratch\n{instructions}\n"),
            (path, expected),
            precise,
        )?;
    }
    Ok(())
}

#[test]
fn replacing_source_ancestor_selects_image() -> TestResult {
    let config = format!(
        "{DOCS_CONFIG}\n[[images]]\nid = \"app\"\nrepository = \"ghcr.io/acme/app\"\ndockerfile = \"image/Dockerfile\"\ncontext = \"image\"\nplatforms = [\"linux_amd64\"]\n"
    );
    let mut repo = scoped_workspace(&config)?;
    put(
        repo.root(),
        "image/Dockerfile",
        "FROM scratch\nCOPY src/file /dest\n",
    )?;
    repo.base = commit(repo.root(), "source ancestor baseline")?;
    for (path, expected) in [("image/src", "run"), ("image/other", "not_affected")] {
        super::workflow_configuration_fixture::git(
            &["checkout", "--detach", &repo.base],
            repo.root(),
        )?;
        put(repo.root(), path, "changed\n")?;
        let head = commit(repo.root(), "change source ancestor")?;
        let jobs = plan(repo.root(), "pull_request", Some(&repo.base), &head)?.jobs()?;
        assert_eq!(
            jobs.get("image-app-amd64").map(String::as_str),
            Some(expected),
            "{path}"
        );
    }
    Ok(())
}
