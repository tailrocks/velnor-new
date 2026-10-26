// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{ffi::OsString, path::Path};

use crate::{CheckDeadline, IsolatedCommand, MiseError, ProcessOutput};

const CURL: &str = "/usr/bin/curl";
const BASH: &str = "/usr/bin/bash";
const DOCKER: &str = "/usr/bin/docker";
const OUTPUT_LIMIT: usize = 128 * 1024;

#[derive(Debug, Clone, Copy)]
pub struct ImageCandidateCommand;

impl ImageCandidateCommand {
    pub fn download_buildx(url: &str, destination: &Path) -> Result<IsolatedCommand, MiseError> {
        if !url.starts_with("https://github.com/docker/buildx/releases/download/")
            || url.bytes().any(|byte| matches!(byte, b'\n' | b'\r' | 0))
            || !destination.is_absolute()
        {
            return Err(invalid("buildx_download_identity"));
        }
        let args = vec![
            "--disable",
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--tlsv1.2",
            "--connect-timeout",
            "20",
            "--max-time",
            "120",
            "--max-filesize",
            "100000000",
            "--output",
        ]
        .into_iter()
        .map(OsString::from)
        .chain([destination.as_os_str().to_owned(), OsString::from(url)])
        .collect();
        IsolatedCommand::repo_task(CURL, args, &[])
    }

    pub fn check_job_source(root: &Path) -> Result<IsolatedCommand, MiseError> {
        if !root.is_absolute() {
            return Err(invalid("image_source_root"));
        }
        IsolatedCommand::repo_task(
            BASH,
            vec!["images/job/ubuntu-26.04/build-test.sh".into()],
            &[],
        )
        .map(|command| command.with_cwd(root.to_path_buf()))
    }

    pub fn buildx_version(docker_config: &Path) -> Result<IsolatedCommand, MiseError> {
        if !docker_config.is_absolute() {
            return Err(invalid("image_candidate_docker_config"));
        }
        IsolatedCommand::repo_task(DOCKER, vec!["buildx".into(), "version".into()], &[]).and_then(
            |command| {
                command.with_env(&[("DOCKER_CONFIG".into(), docker_config.as_os_str().to_owned())])
            },
        )
    }

    pub fn buildx_plugin_info(docker_config: &Path) -> Result<IsolatedCommand, MiseError> {
        if !docker_config.is_absolute() {
            return Err(invalid("image_candidate_docker_config"));
        }
        IsolatedCommand::repo_task(
            DOCKER,
            vec![
                "info".into(),
                "--format".into(),
                "{{json .ClientInfo.Plugins}}".into(),
            ],
            &[],
        )
        .and_then(|command| {
            command.with_env(&[("DOCKER_CONFIG".into(), docker_config.as_os_str().to_owned())])
        })
    }

    pub fn build_oci(
        context: &Path,
        archive: &Path,
        docker_config: &Path,
    ) -> Result<IsolatedCommand, MiseError> {
        if !context.is_absolute()
            || !archive.is_absolute()
            || !docker_config.is_absolute()
            || context == archive
            || archive.to_str().is_none_or(|path| path.contains(','))
        {
            return Err(invalid("image_candidate_build_paths"));
        }
        let output = format!(
            "type=oci,dest={},compression=gzip,force-compression=true",
            archive.display()
        );
        let args = vec![
            "buildx".into(),
            "build".into(),
            "--platform".into(),
            "linux/amd64".into(),
            "--file".into(),
            context.join("Dockerfile").into_os_string(),
            "--provenance=false".into(),
            "--output".into(),
            output.into(),
            context.as_os_str().to_owned(),
        ];
        IsolatedCommand::repo_task(DOCKER, args, &[]).and_then(|command| {
            command.with_env(&[("DOCKER_CONFIG".into(), docker_config.as_os_str().to_owned())])
        })
    }

    pub fn run(
        command: &IsolatedCommand,
        deadline: CheckDeadline,
    ) -> Result<ProcessOutput, MiseError> {
        let (output, _drained) = command.run_quiescent_until(OUTPUT_LIMIT, deadline)?;
        output.require_success(command.program().to_string_lossy().as_ref())?;
        Ok(output)
    }
}

fn invalid(field: &str) -> MiseError {
    MiseError::InvalidStepInput {
        field: field.to_owned(),
        value: "fixed_policy_rejected".to_owned(),
    }
}
