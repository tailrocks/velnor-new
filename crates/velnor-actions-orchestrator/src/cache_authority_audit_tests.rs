// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::path::Path;

use velnor_actions_workflow_renderer::render::RenderedFile;

use super::audit;

const WORKFLOWS: [&str; 2] = [
    ".github/workflows/ci.yml",
    ".github/workflows/qualification.yml",
];

fn committed() -> Result<Vec<RenderedFile>, Box<dyn Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    WORKFLOWS
        .iter()
        .map(|path| {
            Ok(RenderedFile {
                path: (*path).to_owned(),
                bytes: std::fs::read_to_string(root.join(path))?,
            })
        })
        .collect()
}

fn mutate(
    files: &mut [RenderedFile],
    path: &str,
    anchor: &str,
    from: &str,
    to: &str,
) -> Result<(), Box<dyn Error>> {
    let file = files
        .iter_mut()
        .find(|file| file.path == path)
        .ok_or("workflow missing")?;
    let at = file.bytes.find(anchor).ok_or("anchor missing")?;
    let relative = file.bytes[at..].find(from).ok_or("target missing")?;
    let start = at + relative;
    file.bytes.replace_range(start..start + from.len(), to);
    Ok(())
}

#[test]
fn a_writer_job_cannot_enable_mise_config() -> Result<(), Box<dyn Error>> {
    audit(&committed()?)?;
    let cases = [
        (
            WORKFLOWS[0],
            "\n  sources-producer:\n",
            "\n    env:\n",
            "\n    env:\n      MISE_NO_CONFIG: \"0\"\n",
            "sources-producer",
        ),
        (
            WORKFLOWS[1],
            "\n  empty-cache-hosted:\n",
            "MISE_NO_ENV: \"1\"",
            "MISE_NO_ENV: \"0\"",
            "empty-cache-hosted",
        ),
        (
            WORKFLOWS[0],
            "\nenv:\n",
            "  MISE_NO_CONFIG: \"1\"\n",
            "",
            "",
        ),
    ];
    for (path, anchor, from, to, job) in cases {
        let mut files = committed()?;
        mutate(&mut files, path, anchor, from, to)?;
        let error = audit(&files)
            .err()
            .ok_or_else(|| format!("audit accepted {path} {job}"))?
            .to_string();
        assert!(
            error.contains(&format!("cache_writer_mise_config_enabled:{job}")),
            "{path} {job}: {error}"
        );
    }
    Ok(())
}
