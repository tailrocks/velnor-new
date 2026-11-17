// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::runner_family_selection::{
    FAMILY_CONFIG, HELPER_INPUTS, family_workspace, populate_family_fixture,
};
use super::scoped_workspace::{DOCS_CONFIG, TestResult, commit, plan, put, scoped_workspace};

const IMAGE_CONFIG: &str = "\n[[images]]\nid = \"app\"\nrepository = \"ghcr.io/acme/app\"\ndockerfile = \"image/Dockerfile\"\ncontext = \"image\"\nplatforms = [\"linux_amd64\"]\n";

const COPY_CASES: [(&str, &str, &str, bool); 11] = [
    (
        "JSON source beginning with dashes",
        "COPY [\"--src/main.txt\", \"/main.txt\"]",
        "--src/main.txt",
        true,
    ),
    (
        "declared unset default",
        "ARG SRC\nCOPY ${SRC:-src}/main.txt /main.txt",
        "src/main.txt",
        true,
    ),
    (
        "SOURCE_SHA default",
        "ARG SOURCE_SHA=src\nCOPY ${SOURCE_SHA}/main.txt /main.txt",
        "src/main.txt",
        true,
    ),
    (
        "VELNOR_SOURCE_SHA default",
        "ARG VELNOR_SOURCE_SHA=src\nCOPY ${VELNOR_SOURCE_SHA}/main.txt /main.txt",
        "src/main.txt",
        true,
    ),
    (
        "stage arg",
        "ARG SRC=src\nCOPY ${SRC}/main.txt /main.txt",
        "src/main.txt",
        true,
    ),
    (
        "default expansion",
        "COPY ${SRC:-src}/main.txt /main.txt",
        "src/main.txt",
        true,
    ),
    (
        "literal",
        "COPY src/main.txt /main.txt",
        "src/main.txt",
        true,
    ),
    (
        "JSON stage arg with spaces",
        "ARG SRC=\"src dir\"\nCOPY [\"$SRC/main.txt\", \"/main.txt\"]",
        "src dir/main.txt",
        true,
    ),
    (
        "JSON literal with spaces",
        "COPY [\"src dir/main.txt\", \"/main.txt\"]",
        "src dir/main.txt",
        true,
    ),
    (
        "JSON literal with comma",
        "COPY [\"src,dir/main.txt\", \"/main.txt\"]",
        "src,dir/main.txt",
        true,
    ),
    (
        "unresolved",
        "COPY ${SRC}/main.txt /main.txt",
        "src/main.txt",
        false,
    ),
];

#[test]
fn image_tracks_expanded_copy_sources() -> TestResult {
    for (label, source, input, known) in COPY_CASES {
        let mut repo = scoped_workspace(&format!("{DOCS_CONFIG}\n{IMAGE_CONFIG}"))?;
        let root = repo.root();
        put(
            root,
            "image/Dockerfile",
            &format!("FROM scratch\n{source}\n"),
        )?;
        let input_path = format!("image/{input}");
        put(root, &input_path, "initial\n")?;
        repo.base = commit(root, "Dockerfile source baseline")?;
        if known {
            let prepared = velnor_actions_orchestrator::prepare(repo.root())?;
            let jobs = velnor_actions_orchestrator::finalized_jobs(&prepared)?;
            assert!(jobs.contains_key("image-app-amd64"), "{label}");
        }
        for (path, expected) in [
            (input_path.as_str(), "run"),
            (
                "image/fixture-unrelated.txt",
                if known { "not_affected" } else { "run" },
            ),
        ] {
            let root = repo.root();
            super::workflow_configuration_fixture::git(
                &["checkout", "--detach", &repo.base],
                root,
            )?;
            put(root, path, "changed\n")?;
            let head = commit(root, "change context file")?;
            let result = plan(root, "pull_request", Some(&repo.base), &head);
            if !known && result.is_err() {
                continue;
            }
            let run = result?;
            assert_eq!(
                run.jobs()?.get("image-app-amd64").map(String::as_str),
                Some(expected),
                "{label}: {path}"
            );
        }
    }
    Ok(())
}

#[test]
fn runner_family_with_unrecognized_copy_runs_conservatively() -> TestResult {
    let mut repo = family_workspace(&format!("{DOCS_CONFIG}\n{FAMILY_CONFIG}{HELPER_INPUTS}"))?;
    let root = repo.root();
    populate_family_fixture(root)?;
    put(
        root,
        "images/resource-probe/Dockerfile",
        "FROM scratch\nARG SRC=src\nCOPY ${SRC}/main.txt /main.txt\n",
    )?;
    repo.base = commit(root, "variable family Dockerfile")?;
    let root = repo.root();
    put(
        root,
        "images/resource-probe/fixture-unrelated.txt",
        "changed\n",
    )?;
    let head = commit(root, "unrelated context change")?;
    let run = plan(root, "pull_request", Some(&repo.base), &head)?;
    assert_eq!(
        run.jobs()?
            .get("image-runner-family-linux-amd64")
            .map(String::as_str),
        Some("run")
    );
    Ok(())
}

fn assert_image_context_selection(
    dockerfile: &str,
    input: (&str, &str),
    precise: bool,
) -> TestResult {
    let mut repo = scoped_workspace(&format!("{DOCS_CONFIG}\n{IMAGE_CONFIG}"))?;
    put(repo.root(), "image/Dockerfile", dockerfile)?;
    let input_path = format!("image/{}", input.0);
    put(repo.root(), &input_path, "initial\n")?;
    repo.base = commit(repo.root(), "Dockerfile context baseline")?;
    let prepared = velnor_actions_orchestrator::prepare(repo.root())?;
    assert!(
        velnor_actions_orchestrator::finalized_jobs(&prepared)?.contains_key("image-app-amd64")
    );
    for (path, expected) in [
        (input_path.as_str(), input.1),
        (
            "image/fixture-unrelated.txt",
            if precise { "not_affected" } else { "run" },
        ),
    ] {
        super::workflow_configuration_fixture::git(
            &["checkout", "--detach", &repo.base],
            repo.root(),
        )?;
        put(repo.root(), path, "changed\n")?;
        let head = commit(repo.root(), "change context input")?;
        let run = plan(repo.root(), "pull_request", Some(&repo.base), &head)?;
        assert_eq!(
            run.jobs()?.get("image-app-amd64").map(String::as_str),
            Some(expected),
            "{dockerfile}: {path}"
        );
    }
    Ok(())
}

#[test]
fn normalized_copy_paths_select_the_image_job() -> TestResult {
    for keyword in ["COPY", "ADD"] {
        for source in ["$SRC/./main.txt", "$SRC//main.txt"] {
            assert_image_context_selection(
                &format!("FROM scratch\nARG SRC=src\n{keyword} {source} /dest\n"),
                ("src/main.txt", "run"),
                true,
            )?;
        }
        for source in ["$SRC/sub/../main.txt", "$SRC/../../main.txt"] {
            let repo = scoped_workspace(&format!("{DOCS_CONFIG}\n{IMAGE_CONFIG}"))?;
            put(
                repo.root(),
                "image/Dockerfile",
                &format!("FROM scratch\nARG SRC=src\n{keyword} {source} /dest\n"),
            )?;
            let error = velnor_actions_orchestrator::prepare(repo.root())
                .err()
                .ok_or("parent source accepted")?;
            assert!(error.to_string().contains("image_context_escape"));
        }
    }
    Ok(())
}

#[test]
fn client_proxy_arguments_select_the_whole_context() -> TestResult {
    for name in [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "FTP_PROXY",
        "NO_PROXY",
        "ALL_PROXY",
        "http_proxy",
        "https_proxy",
        "ftp_proxy",
        "no_proxy",
        "all_proxy",
    ] {
        assert_image_context_selection(
            &format!("FROM scratch\nARG {name}=src\nCOPY ${name}/main.txt /dest\n"),
            ("override/main.txt", "run"),
            false,
        )?;
    }
    Ok(())
}

#[test]
fn inherited_onbuild_inputs_select_the_image_job() -> TestResult {
    assert_image_context_selection(
        &format!(
            "FROM example@sha256:{}\nCOPY src/main.txt /dest\n",
            "a".repeat(64)
        ),
        ("inherited/main.txt", "run"),
        false,
    )?;
    for reference in ["base", "0"] {
        for keyword in ["COPY", "ADD"] {
            assert_image_context_selection(
                &format!(
                    "FROM scratch AS base\nONBUILD {keyword} inherited/main.txt /dest\nFROM {reference} AS child\nFROM child\nCOPY src/main.txt /dest\n"
                ),
                ("inherited/main.txt", "run"),
                true,
            )?;
        }
    }
    assert_image_context_selection(
        "FROM scratch AS base\nONBUILD COPY inherited/main.txt /dest\nCOPY src/main.txt /dest\n",
        ("inherited/main.txt", "not_affected"),
        true,
    )?;
    assert_image_context_selection(
        "FROM scratch AS base\nARG SRC=src\nONBUILD COPY $SRC/main.txt /dest\nFROM base\n",
        ("override/main.txt", "run"),
        false,
    )?;
    Ok(())
}

#[test]
fn unused_complex_arguments_preserve_precise_selection() -> TestResult {
    for keyword in ["COPY", "ADD"] {
        assert_image_context_selection(
            &format!(
                "FROM scratch\nARG CONFIG='{{\"mode\":\"fast\"}}'\n{keyword} src/main.txt /dest\n"
            ),
            ("src/main.txt", "run"),
            true,
        )?;
    }
    assert_image_context_selection(
        "FROM scratch\nARG CONFIG='{\"mode\":\"fast\"}'\nCOPY $CONFIG/main.txt /dest\n",
        ("actual/main.txt", "run"),
        false,
    )?;
    Ok(())
}

#[test]
fn unused_quoted_argument_values_do_not_poison_other_expansions() -> TestResult {
    for prefix in [
        "ARG CONFIG=foo\" bar\"\nARG BASE=scratch\nFROM $BASE\n",
        "ARG CONFIG=foo\\ bar\nARG BASE=scratch\nFROM $BASE\n",
        "FROM scratch\nARG CONFIG=foo\" bar\"\n",
    ] {
        for keyword in ["COPY", "ADD"] {
            assert_image_context_selection(
                &format!("{prefix}{keyword} ${{SRC:-src}}/main.txt /dest\n"),
                ("src/main.txt", "run"),
                true,
            )?;
        }
    }
    Ok(())
}
