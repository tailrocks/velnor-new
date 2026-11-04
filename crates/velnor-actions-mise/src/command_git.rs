// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;

use super::{EnvPolicy, IsolatedCommand};

impl IsolatedCommand {
    pub(crate) fn release_git(
        args: Vec<OsString>,
        git: &impl crate::release_git_pin::ReleaseGitExecutable,
    ) -> Result<Self, crate::MiseError> {
        let path = std::env::join_paths([
            git.exec_path(),
            std::path::Path::new("/usr/bin"),
            std::path::Path::new("/bin"),
        ])
        .map_err(|_| crate::MiseError::InvalidStepInput {
            field: "release_git_path".to_owned(),
            value: "invalid".to_owned(),
        })?;
        Ok(Self {
            program: git.executable().as_os_str().to_owned(),
            args,
            cwd: None,
            extra_env: [
                ("HOME", "/dev/null"),
                ("GIT_CONFIG_NOSYSTEM", "1"),
                ("GIT_CONFIG_GLOBAL", "/dev/null"),
                ("GIT_NO_REPLACE_OBJECTS", "1"),
                ("GIT_OPTIONAL_LOCKS", "0"),
            ]
            .into_iter()
            .map(|(key, value)| (OsString::from(key), OsString::from(value)))
            .chain(std::iter::once((OsString::from("PATH"), path)))
            .chain(std::iter::once((
                OsString::from("GIT_EXEC_PATH"),
                git.exec_path().as_os_str().to_owned(),
            )))
            .collect(),
            policy: EnvPolicy::QualifiedProbe,
            operator_guarded: false,
        })
    }
    pub(crate) fn direct(program: &str, args: Vec<OsString>) -> Self {
        let extra_env = if program == "git" {
            vec![(OsString::from("GIT_OPTIONAL_LOCKS"), OsString::from("0"))]
        } else {
            Vec::new()
        };
        Self {
            program: OsString::from(program),
            args,
            cwd: None,
            extra_env,
            policy: EnvPolicy::Discovery,
            operator_guarded: false,
        }
    }
}
