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
