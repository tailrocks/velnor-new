// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;

use velnor_gen::expected_files;
use velnor_model::{RELEASE_CONFIG_PATH, RELEASE_WORKFLOW_PATH, RepoPath, RepoView};
use velnor_rust::LOCK_PATH;

use crate::pipeline::view;
use crate::yaml_tree::{Node, parse};

const LOCK: &str = include_str!("../../../../samples/helper-lock/view/.velnor/rust-ci.lock");
pub(crate) const READY: &str = "needs.gate.outputs.ready == 'true'";
pub(crate) const PUSH: &str = "github.event.workflow_run.event == 'push'";
pub(crate) const DEFAULT_BRANCH: &str =
    "github.event.workflow_run.head_branch == github.event.repository.default_branch";
pub(crate) const GREEN: &str = "github.event.workflow_run.conclusion == 'success'";
pub(crate) const APP_TOKEN: &str = "actions/create-github-app-token";

pub(crate) struct Root {
    pub(crate) dir: &'static str,
    pub(crate) package: &'static str,
    pub(crate) prefix: &'static str,
    pub(crate) publishes: bool,
}

pub(crate) const APP: Root = Root {
    dir: ".",
    package: "app",
    prefix: "release-plz-app-",
    publishes: false,
};
pub(crate) const TOOLS: Root = Root {
    dir: "tools",
    package: "tools",
    prefix: "release-plz-tools-",
    publishes: false,
};

pub(crate) const DASHED: Root = Root {
    dir: ".",
    package: "my-app",
    prefix: "release-plz-my-app-",
    publishes: false,
};
pub(crate) const SPACED: Root = Root {
    dir: "my tools",
    package: "tools",
    prefix: "release-plz-tools-",
    publishes: false,
};

pub(crate) fn dashed() -> Result<(String, Node), Box<dyn Error>> {
    render(&[DASHED], Some("[[binaries]]\npackage = \"my-app\"\n"))
}

pub(crate) fn release_plz(root: &Root) -> String {
    let publishing = if root.publishes {
        "publish_no_verify = true"
    } else {
        "git_only = true"
    };
    format!(
        "[workspace]\ngit_tag_name = \"{{{{ package }}}}-v{{{{ version }}}}\"\ngit_release_draft = true\nrelease_always = false\nsemver_check = false\npr_branch_prefix = \"{}\"\n{publishing}\n\n[[package]]\nname = \"{}\"\n",
        root.prefix, root.package
    )
}

pub(crate) fn release_view(
    roots: &[Root],
    release_toml: Option<&str>,
) -> Result<RepoView, Box<dyn Error>> {
    let mut view = view(&["Cargo.toml"], None)?;
    view.tracked_paths.insert(LOCK_PATH);
    view.files.insert(LOCK_PATH, LOCK.as_bytes().to_vec());
    view.files.insert(
        RepoPath::new("mise.toml")?,
        b"[tools]\nrust = \"1.99.1\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\n\"aqua:release-plz/release-plz\" = \"0.4.3\"\n".to_vec(),
    );
    view.files.insert(
        RepoPath::new("mise.lock")?,
        b"[[tools.\"aqua:release-plz/release-plz\"]]\nversion = \"0.4.3\"\n".to_vec(),
    );
    if let Some(text) = release_toml {
        view.files
            .insert(RELEASE_CONFIG_PATH, text.as_bytes().to_vec());
    }
    for root in roots {
        let path = if root.dir == "." {
            "release-plz.toml".to_owned()
        } else {
            format!("{}/release-plz.toml", root.dir)
        };
        view.files
            .insert(RepoPath::new(path)?, release_plz(root).into_bytes());
    }
    Ok(view)
}

pub(crate) fn render(
    roots: &[Root],
    release_toml: Option<&str>,
) -> Result<(String, Node), Box<dyn Error>> {
    let mut files = expected_files(&release_view(roots, release_toml)?)?;
    let text = files
        .remove(&RELEASE_WORKFLOW_PATH)
        .ok_or("release.yml was not rendered")?;
    let tree = parse(&text)?;
    Ok((text, tree))
}

pub(crate) fn app() -> Result<(String, Node), Box<dyn Error>> {
    render(&[APP], Some("[[binaries]]\npackage = \"app\"\n"))
}

pub(crate) fn two_roots() -> Result<(String, Node), Box<dyn Error>> {
    render(
        &[APP, TOOLS],
        Some("roots = [\".\", \"tools\"]\nafter = [\"Rust CI\", \"Lint\"]\n"),
    )
}

pub(crate) const TWO_BINARIES: &str = "roots = [\".\", \"tools\"]\n[[binaries]]\npackage = \"app\"\n[[binaries]]\npackage = \"tools\"\nplatforms = [\"linux-x64\", \"macos-arm64\"]\n";

pub(crate) fn two_binaries() -> Result<(String, Node), Box<dyn Error>> {
    render(&[APP, TOOLS], Some(TWO_BINARIES))
}

pub(crate) fn jobs(tree: &Node) -> Result<Vec<(&str, &Node)>, Box<dyn Error>> {
    let jobs = tree.get("jobs").ok_or("no jobs")?;
    jobs.keys()
        .into_iter()
        .map(|id| Ok((id, jobs.get(id).ok_or("job vanished")?)))
        .collect()
}

pub(crate) fn steps(job: &Node) -> &[Node] {
    job.get("steps").map_or(&[], Node::items)
}

pub(crate) fn uses(step: &Node) -> Option<&str> {
    step.get("uses").and_then(Node::text)
}

pub(crate) fn conjuncts(job: &Node) -> BTreeSet<String> {
    job.get("if")
        .and_then(Node::text)
        .map(|text| {
            text.trim_start_matches("${{ ")
                .trim_end_matches(" }}")
                .split(" && ")
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn app_steps(job: &Node) -> Vec<&Node> {
    steps(job)
        .iter()
        .filter(|step| uses(step).is_some_and(|u| u.starts_with(APP_TOKEN)))
        .collect()
}
