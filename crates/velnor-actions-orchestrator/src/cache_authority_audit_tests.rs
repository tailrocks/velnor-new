// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::path::Path;

use velnor_actions_workflow_renderer::render::RenderedFile;

use super::audit;

const WORKFLOWS: [&str; 2] = [
    ".github/workflows/state-seal.yml",
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

#[test]
fn anchored_writer_environment_keeps_isolation_mandatory() -> Result<(), Box<dyn Error>> {
    let mut safe = committed()?;
    safe[0].bytes = safe[0]
        .bytes
        .replacen("\nenv:\n", "\nenv: &writer_env\n", 1);
    let writer = safe[0]
        .bytes
        .find("\n  sources-producer:\n")
        .ok_or("writer missing")?;
    let start = writer
        + safe[0].bytes[writer..]
            .find("\n    env:\n")
            .ok_or("writer env missing")?;
    let values = start + "\n    env:\n".len();
    let length: usize = safe[0].bytes[values..]
        .split_inclusive('\n')
        .take_while(|line| line.starts_with("      "))
        .map(str::len)
        .sum();
    safe[0]
        .bytes
        .replace_range(start..values + length, "\n    env: *writer_env\n");
    audit(&safe)?;
    for (from, to) in [
        ("MISE_NO_CONFIG: \"1\"", "MISE_NO_CONFIG: \"0\""),
        ("env: &writer_env", "env: *missing_writer_env"),
        ("    env:\n", "    env: &writer_env\n"),
        ("MISE_NO_CONFIG: \"1\"", "MISE_NO_CONFIG: 1"),
        (
            "MISE_NO_CONFIG: \"1\"",
            "MISE_NO_CONFIG: \"1\"\n  MISE_NO_CONFIG: \"0\"",
        ),
    ] {
        let mut files = safe.clone();
        files[0].bytes = files[0].bytes.replacen(from, to, 1);
        assert!(audit(&files).is_err(), "accepted {to}");
    }
    Ok(())
}
