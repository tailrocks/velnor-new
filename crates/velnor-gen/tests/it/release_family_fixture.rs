// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_gen::GenerateError;
use velnor_model::{RELEASE_WORKFLOW_PATH, RepoPath};
use velnor_rust::LOCK_PATH;

use crate::release_fixture::{APP, LOCK, TOOLS, release_view};
use crate::release_unguarded::expected_release;
use crate::yaml_tree::{Node, parse};

const RELEASE_PLZ: &str = "[workspace]\ngit_tag_name = \"{{ package }}-v{{ version }}\"\ngit_release_draft = true\nrelease_always = false\nsemver_check = false\npr_branch_prefix = \"{prefix}\"\ngit_only = true\n\n[[package]]\nname = \"{package}\"\n{release}";

pub(crate) const HELPER: &str = "0.1.98";

pub(crate) const TOOLS_LIKE: &str = "roots = [\".\", \"tools\"]\nfamily_tag = \"tools-v{{ version }}\"\n[[binaries]]\npackage = \"app\"\nsmoke = true\nplatforms = [\"linux-x64\", \"linux-arm64\"]\n[[binaries]]\npackage = \"tools\"\nsmoke = true\nplatforms = [\"linux-x64\", \"linux-arm64\"]\n";

pub(crate) fn tools_like(extra: &str) -> String {
    format!("{extra}{TOOLS_LIKE}")
}

pub(crate) fn generate(helper: &str, config: &str) -> Result<(String, Node), Box<dyn Error>> {
    generate_releasing(helper, config, false)
}

pub(crate) fn generate_releasing(
    helper: &str,
    config: &str,
    tools_released: bool,
) -> Result<(String, Node), Box<dyn Error>> {
    let mut view = release_view(&[APP, TOOLS], Some(config))?;
    view.files
        .insert(LOCK_PATH, LOCK.replace("0.1.81", helper).into_bytes());
    for (root, path) in [
        (&APP, "release-plz.toml"),
        (&TOOLS, "tools/release-plz.toml"),
    ] {
        let text = RELEASE_PLZ
            .replace("{prefix}", root.prefix)
            .replace("{package}", root.package)
            .replace(
                "{release}",
                if tools_released && root.package == "tools" {
                    ""
                } else {
                    "release = false\n"
                },
            );
        view.files.insert(RepoPath::new(path)?, text.into_bytes());
    }
    let text = expected_release(&view)?
        .remove(&RELEASE_WORKFLOW_PATH)
        .ok_or("no release.yml")?;
    let tree = parse(&text)?;
    Ok((text, tree))
}

pub(crate) fn refused(config: &str) -> Result<GenerateError, Box<dyn Error>> {
    refused_releasing(config, false)
}

pub(crate) fn refused_releasing(
    config: &str,
    tools_released: bool,
) -> Result<GenerateError, Box<dyn Error>> {
    match generate_releasing(HELPER, config, tools_released) {
        Ok(_) => Err("the configuration was accepted".into()),
        Err(error) => match error.downcast::<GenerateError>() {
            Ok(error) => Ok(*error),
            Err(other) => Err(other),
        },
    }
}
