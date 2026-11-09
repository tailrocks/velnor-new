// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{ffi::OsString, path::Path};

use super::{DOCKER, invalid};
use crate::{IsolatedCommand, MiseError};

pub(super) fn build(
    root: &Path,
    archive: &Path,
    config: &Path,
    commit: &str,
    role: &str,
) -> Result<IsolatedCommand, MiseError> {
    validate_identity(root, archive, config, commit)?;
    let (context, tag, source_arg, contexts): (&str, &str, &str, &[&str]) = match role {
        "runner" => (
            "images/runner/ubuntu-26.04",
            "velnor-runner",
            "VELNOR_SOURCE_SHA",
            &[
                "resource_probe",
                "storage_init",
                "job_start_hook",
                "job_started_hook",
            ],
        ),
        "dind" => (
            "images/dind",
            "velnor-dind",
            "VELNOR_SOURCE_SHA",
            &["resource_probe"],
        ),
        "resource_probe" => (
            "images/resource-probe",
            "velnor-resource-probe",
            "SOURCE_SHA",
            &["resource_probe"],
        ),
        _ => return Err(invalid("runner_family_build_role")),
    };
    let mut args: Vec<OsString> = vec![
        "buildx".into(),
        "build".into(),
        "--platform".into(),
        "linux/amd64".into(),
        "--file".into(),
        root.join(context).join("Dockerfile").into_os_string(),
        "--tag".into(),
        format!("{tag}:linux-amd64").into(),
        "--build-arg".into(),
        format!("{source_arg}={commit}").into(),
        "--provenance=mode=max,version=v1".into(),
        "--output".into(),
        format!(
            "type=oci,dest={},compression=gzip,force-compression=true",
            archive.display()
        )
        .into(),
    ];
    for name in contexts {
        args.push("--build-context".into());
        args.push(
            format!(
                "{name}={}",
                root.join("target/velnor-runner-family")
                    .join(name)
                    .display()
            )
            .into(),
        );
    }
    args.push(root.join(context).into_os_string());
    IsolatedCommand::repo_task(DOCKER, args, &[])?
        .with_cwd(root.to_owned())
        .with_env(&[
            ("DOCKER_CONFIG".into(), config.as_os_str().to_owned()),
            ("BUILDX_GIT_INFO".into(), "true".into()),
        ])
}

fn validate_identity(
    root: &Path,
    archive: &Path,
    config: &Path,
    commit: &str,
) -> Result<(), MiseError> {
    if [root, archive, config]
        .iter()
        .any(|path| !path.is_absolute())
        || archive.to_str().is_none_or(|path| path.contains(','))
        || commit.len() != 40
        || !commit
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(invalid("runner_family_build_identity"));
    }
    Ok(())
}
