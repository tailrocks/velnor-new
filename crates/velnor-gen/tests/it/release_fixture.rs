// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;

use crate::release_unguarded::expected_release;
use velnor_model::{
    PackageName, RELEASE_CONFIG_PATH, RELEASE_WORKFLOW_PATH, RepoPath, RepoView, Target,
    TargetKind, TargetName,
};
use velnor_rust::LOCK_PATH;

use crate::pipeline::view;
use crate::yaml_tree::{Node, parse};

pub(crate) const LOCK: &str =
    include_str!("../../../../generator/samples/helper-lock/view/.velnor/rust-ci.lock");
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
        "publish_no_verify = true\npublish_timeout = \"5m\""
    } else {
        "git_only = true"
    };
    format!(
        "[workspace]\ngit_tag_name = \"{{{{ package }}}}-v{{{{ version }}}}\"\ngit_release_draft = true\nrelease_always = false\nsemver_check = false\npr_branch_prefix = \"{}\"\n{publishing}\n\n[[package]]\nname = \"{}\"\n",
        root.prefix, root.package
    )
}

fn join_manifest(dir: &str) -> String {
    if dir == "." {
        "Cargo.toml".to_owned()
    } else {
        format!("{dir}/Cargo.toml")
    }
}

pub(crate) fn release_view(
    roots: &[Root],
    release_toml: Option<&str>,
) -> Result<RepoView, Box<dyn Error>> {
    let manifests: Vec<String> = if roots.is_empty() {
        vec!["Cargo.toml".to_owned()]
    } else {
        roots.iter().map(|root| join_manifest(root.dir)).collect()
    };
    let refs: Vec<&str> = manifests.iter().map(String::as_str).collect();
    let mut view = view(&refs, Some(b"full_run = \"off\"\n"))?;
    for (root, manifest) in roots.iter().zip(&manifests) {
        let metadata = view
            .rust_metadata
            .get_mut(&RepoPath::new(manifest.as_str())?)
            .ok_or("no metadata for the root")?;
        metadata.members[0].name = PackageName::new(root.package)?;
        for name in [root.package, "rg"] {
            metadata.members[0].targets.push(Target {
                name: TargetName::new(name)?,
                kinds: vec![TargetKind::Bin],
                tests: false,
                doctest: false,
                source: None,
                requires_features: false,
            });
        }
    }
    view.tracked_paths.insert(LOCK_PATH);
    view.files.insert(LOCK_PATH, LOCK.as_bytes().to_vec());
    view.files.insert(
        RepoPath::new("mise.toml")?,
        b"[tools]\nrust = { version = \"1.99.1\", mr_boxington = true }\nmr-boxington = \"v1.30.0\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\n\"aqua:release-plz/release-plz\" = \"0.4.3\"\n\n[wrappers.cargo]\ncommand = \"mbx\"\nenv = { MBX_CARGO_SHIM_MODE = \"1\" }\n".to_vec(),
    );
    view.files.insert(
        RepoPath::new("mise.lock")?,
        b"[[tools.mr-boxington]]\nversion = \"1.30.0\"\nspecifiers = [\"v1.30.0\"]\n\n[tools.mr-boxington.\"platforms.linux-x64\"]\nurl = \"https://example.invalid/mbx\"\n\n[[tools.\"aqua:nextest-rs/nextest/cargo-nextest\"]]\nversion = \"0.9.151\"\n\n[tools.\"aqua:nextest-rs/nextest/cargo-nextest\".\"platforms.linux-x64\"]\nurl = \"https://example.invalid/nextest\"\n\n[[tools.\"aqua:release-plz/release-plz\"]]\nversion = \"0.4.3\"\n\n[tools.\"aqua:release-plz/release-plz\".\"platforms.linux-x64\"]\nchecksum = \"sha256:a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90\"\nurl = \"https://github.com/release-plz/release-plz/releases/download/release-plz-v0.4.3/release-plz-x86_64-unknown-linux-gnu.tar.gz\"\n".to_vec(),
    );
    view.files.insert(
        RepoPath::new(".github/workflows/lint.yml")?,
        b"name: Lint\non: pull_request\n".to_vec(),
    );
    if let Some(text) = release_toml {
        view.files
            .insert(RELEASE_CONFIG_PATH, text.as_bytes().to_vec());
    }
    for root in roots {
        let manifest = RepoPath::new(if root.dir == "." {
            "Cargo.toml".to_owned()
        } else {
            format!("{}/Cargo.toml", root.dir)
        })?;
        if let Some(metadata) = view.rust_metadata.get_mut(&manifest) {
            metadata.members[0].name = PackageName::new(root.package)?;
        }
        view.files.entry(manifest).or_insert_with(|| {
            format!(
                "[package]\nname = \"{}\"\nversion = \"0.1.0\"\n",
                root.package
            )
            .into_bytes()
        });
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
    let mut files = expected_release(&release_view(roots, release_toml)?)?;
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
