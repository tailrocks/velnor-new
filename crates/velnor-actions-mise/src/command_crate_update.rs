// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{collections::BTreeSet, ffi::OsString, path::Path};

use super::{EnvPolicy, IsolatedCommand};
use crate::MiseError;

const ALLOWED_ENV: &[&str] = &[
    "PATH",
    "CARGO",
    "RUSTC",
    "RUSTDOC",
    "HOME",
    "CARGO_HOME",
    "CARGO_TARGET_DIR",
    "RUSTUP_HOME",
    "RUSTUP_AUTO_INSTALL",
    "RUNNER_TEMP",
];

fn invalid() -> MiseError {
    MiseError::InvalidStepInput {
        field: "crate_update".to_owned(),
        value: "invalid_input".to_owned(),
    }
}

impl IsolatedCommand {
    pub fn crate_update(
        program: &Path,
        args: Vec<OsString>,
        cwd: &Path,
        tool_env: &[(OsString, OsString)],
    ) -> Result<Self, MiseError> {
        if !program.is_absolute() || !cwd.is_absolute() {
            return Err(invalid());
        }
        let source = cwd.canonicalize().map_err(|_| invalid())?;
        let executable = program.canonicalize().map_err(|_| invalid())?;
        let executable_meta = std::fs::symlink_metadata(program).map_err(|_| invalid())?;
        if !source.is_dir()
            || !executable_meta.is_file()
            || executable_meta.file_type().is_symlink()
            || executable.starts_with(&source)
            || !valid_args(&args, &source)
        {
            return Err(invalid());
        }
        validate_env(tool_env, &source)?;
        Ok(Self {
            program: executable.into_os_string(),
            args,
            cwd: Some(source),
            extra_env: tool_env.to_vec(),
            policy: EnvPolicy::QualifiedProbe,
            operator_guarded: false,
        })
    }
}

fn valid_args(args: &[OsString], source: &Path) -> bool {
    if !matches!(args.len(), 5 | 7)
        || args[0] != "update"
        || args[1] != "--config"
        || args[3] != "--manifest-path"
    {
        return false;
    }
    for index in [2, 4] {
        let path = Path::new(&args[index]);
        if !path.is_absolute()
            || !path
                .canonicalize()
                .is_ok_and(|resolved| resolved.is_file() && resolved.starts_with(source))
        {
            return false;
        }
    }
    if args.len() == 7 {
        if args[5] != "--registry" {
            return false;
        }
        let Some(registry) = args[6].to_str() else {
            return false;
        };
        if registry.is_empty()
            || registry.starts_with('-')
            || !registry
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        {
            return false;
        }
    }
    true
}

fn validate_env(env: &[(OsString, OsString)], source: &Path) -> Result<(), MiseError> {
    let mut names = BTreeSet::new();
    for (key, value) in env {
        let key = key.to_str().ok_or_else(invalid)?;
        if !ALLOWED_ENV.contains(&key) || !names.insert(key) || value.is_empty() {
            return Err(invalid());
        }
        if key == "RUSTUP_AUTO_INSTALL" && value != "0" {
            return Err(invalid());
        }
        if matches!(
            key,
            "CARGO"
                | "RUSTC"
                | "RUSTDOC"
                | "HOME"
                | "CARGO_HOME"
                | "CARGO_TARGET_DIR"
                | "RUSTUP_HOME"
                | "RUNNER_TEMP"
        ) {
            let path = Path::new(value);
            if !path.is_absolute()
                || !path.canonicalize().is_ok_and(|resolved| {
                    !resolved.starts_with(source)
                        && if matches!(key, "CARGO" | "RUSTC" | "RUSTDOC") {
                            resolved.is_file()
                        } else {
                            resolved.is_dir()
                        }
                })
            {
                return Err(invalid());
            }
        }
        if key == "PATH"
            && std::env::split_paths(value).any(|path| {
                !path.is_absolute()
                    || !path
                        .canonicalize()
                        .is_ok_and(|resolved| resolved.is_dir() && !resolved.starts_with(source))
            })
        {
            return Err(invalid());
        }
    }
    if [
        "PATH",
        "CARGO",
        "RUSTC",
        "RUSTDOC",
        "HOME",
        "CARGO_HOME",
        "CARGO_TARGET_DIR",
        "RUSTUP_HOME",
        "RUSTUP_AUTO_INSTALL",
    ]
    .iter()
    .any(|key| !names.contains(key))
    {
        return Err(invalid());
    }
    Ok(())
}
