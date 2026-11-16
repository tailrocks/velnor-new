// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fmt::Write as _, fs};

use super::workflow_configuration_fixture;

pub(super) fn repository() -> Result<tempfile::TempDir, Box<dyn Error>> {
    let hash = "a".repeat(64);
    let config = format!(
        r#"schema = 1
[workflow]
name = "CI"
default_branch = "testmain"
[stacks.dbt]
projects = ["dbt"]
[stacks.ansible]
roots = ["ansible"]
python_requirements = "ansible/requirements.txt"
inventory = "ansible/hosts.ini"
checks = ["syntax"]
[[stacks.ansible.collections]]
name = "community.general"
version = "13.4.0"
sha256 = "{hash}"
"#
    );
    let repo = workflow_configuration_fixture::make_repo(&config)?;
    let root = repo.path();
    fs::write(
        root.join("mise.toml"),
        "[tools]\ndbt-fusion = \"2.0.0-preview.35\"\npython = \"3.13.0\"\nuv = \"0.9.0\"\nansible-core = \"2.19.0\"\n",
    )?;
    fs::write(root.join("mise.lock"), lockfile(&hash)?)?;
    for directory in ["dbt", "ansible"] {
        fs::create_dir(root.join(directory))?;
    }
    for (path, text) in [
        (
            "dbt/dbt_project.yml",
            "name: fixture\nprofile: fixture\n".to_owned(),
        ),
        (
            "ansible/ansible.cfg",
            "[defaults]\ninventory = hosts.ini\n".to_owned(),
        ),
        ("ansible/hosts.ini", "[local]\nlocalhost\n".to_owned()),
        (
            "ansible/requirements.txt",
            format!("ansible-core==2.19.0 --hash=sha256:{hash}\n"),
        ),
        (
            "ansible/requirements.yaml",
            "collections:\n  - name: community.general\n    version: 13.4.0\n".to_owned(),
        ),
        ("ansible/site.yml", "- hosts: all\n  tasks: []\n".to_owned()),
    ] {
        fs::write(root.join(path), text)?;
    }
    Ok(repo)
}

fn lockfile(hash: &str) -> Result<String, Box<dyn Error>> {
    let mut lock =
        "lockfile_version = 3\n[[tools.dbt-fusion]]\nversion = \"2.0.0-preview.35\"\n".to_owned();
    for (platform, target) in [
        ("linux-x64", "x86_64-unknown-linux-gnu"),
        ("linux-arm64", "aarch64-unknown-linux-gnu"),
        ("macos-arm64", "aarch64-apple-darwin"),
    ] {
        write!(
            lock,
            "[tools.dbt-fusion.\"platforms.{platform}\"]\nurl = \"https://public.cdn.getdbt.com/fs/cli/fs-v2.0.0-preview.35-{target}.tar.gz\"\nchecksum = \"sha256:{hash}\"\n"
        )?;
    }
    for (tool, version, url) in [
        (
            "python",
            "3.13.0",
            "https://github.com/astral-sh/python-build-standalone/releases/download/fixture/python.tar.gz",
        ),
        (
            "uv",
            "0.9.0",
            "https://github.com/astral-sh/uv/releases/download/0.9.0/uv.tar.gz",
        ),
    ] {
        write!(
            lock,
            "[[tools.{tool}]]\nversion = \"{version}\"\n[tools.{tool}.\"platforms.linux-x64\"]\nurl = \"{url}\"\nchecksum = \"sha256:{hash}\"\n"
        )?;
    }
    Ok(lock)
}
