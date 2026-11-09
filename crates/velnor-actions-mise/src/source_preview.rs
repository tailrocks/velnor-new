// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub const BOOTSTRAP: &str = include_str!("source_preview/source_preview_bootstrap.py");
pub const STAGE_VERIFIER: &str = include_str!("source_preview/source_preview_stage_verify.py");
pub const RUNTIME_VERIFIER: &str = include_str!("source_preview/source_preview_runtime.py");
pub const SEALED_EXECUTOR: &str = include_str!("source_preview/source_preview_run_verified.py");

use std::{ffi::OsString, path::Path};

use crate::{IsolatedCommand, MiseError};

pub fn verify_only_command(
    source: &str,
    run_id: u64,
    attempt: u32,
    gh: &Path,
    temp: &Path,
    token: &str,
) -> Result<IsolatedCommand, MiseError> {
    if source.len() != 40
        || !source
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        || run_id == 0
        || attempt == 0
        || !gh.is_absolute()
        || !temp.is_absolute()
    {
        return Err(MiseError::InvalidStepInput {
            field: "source_preview_verification".to_owned(),
            value: "invalid_identity".to_owned(),
        });
    }
    let code = "import sys,types\nfor n,h in zip(('source_preview_bootstrap','source_preview_stage_verify','source_preview_runtime'),sys.argv[1:4]):\n m=types.ModuleType(n);sys.modules[n]=m;exec(compile(bytes.fromhex(h),n,'exec'),m.__dict__)\nsys.argv=['source-preview-runtime','--verify-only',*sys.argv[4:]]\nsys.modules['source_preview_runtime'].main()";
    let args = vec![
        OsString::from("-I"),
        OsString::from("-S"),
        OsString::from("-c"),
        OsString::from(code),
        OsString::from(hex(BOOTSTRAP.as_bytes())),
        OsString::from(hex(STAGE_VERIFIER.as_bytes())),
        OsString::from(hex(RUNTIME_VERIFIER.as_bytes())),
        OsString::from(source),
        OsString::from(run_id.to_string()),
        OsString::from(attempt.to_string()),
        gh.as_os_str().to_owned(),
        temp.as_os_str().to_owned(),
    ];
    IsolatedCommand::repo_task("/usr/bin/python3", args, &[])?.with_github_token(token)
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(char::from(DIGITS[usize::from(byte >> 4)]));
        result.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
    result
}
