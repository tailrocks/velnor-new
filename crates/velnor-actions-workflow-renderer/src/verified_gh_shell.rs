// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

const PROGRAM: &str = include_str!("verified_gh_launcher.py");

fn python_argument(program: &str) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut hex = String::with_capacity(program.len() * 2);
    for byte in program.bytes() {
        hex.push(char::from(DIGITS[usize::from(byte >> 4)]));
        hex.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
    format!("'exec(bytes.fromhex(\"{hex}\"))'")
}

#[must_use]
pub fn command() -> String {
    command_with_version_variable("")
}

fn command_with_version_variable(variable: &str) -> String {
    let pin = velnor_actions_mise::catalog::QUALIFIED_GH_LINUX_AMD64;
    let program = PROGRAM
        .replace("@VERSION@", pin.version)
        .replace("@PLATFORM@", pin.platform)
        .replace("@SHA256@", pin.sha256)
        .replace("@VERSION_VARIABLE@", variable);
    let program = python_argument(&program);
    format!(
        "LD_PRELOAD= LD_LIBRARY_PATH= LD_AUDIT= LD_DEBUG= LD_PROFILE= GCONV_PATH= LOCPATH= PYTHONPATH= PYTHONHOME= PYTHONUSERBASE= /usr/bin/python3 -I -S -c {program}"
    )
}

pub fn command_for(argv: &[String]) -> Result<String, crate::RenderError> {
    let pin = velnor_actions_mise::catalog::QUALIFIED_GH_LINUX_AMD64;
    let spec = format!("gh@{}", pin.version);
    let fixed = [
        "mise",
        "--no-config",
        "--no-env",
        "--no-hooks",
        "exec",
        spec.as_str(),
        "--",
        "gh",
    ];
    let dynamic = [
        "mise",
        "--no-config",
        "--no-env",
        "--no-hooks",
        "exec",
        "gh@$VELNOR_RELEASE_GH_VERSION",
        "--",
        "gh",
    ];
    if argv.iter().map(String::as_str).eq(fixed) {
        Ok(command())
    } else if argv.iter().map(String::as_str).eq(dynamic) {
        Ok(command_with_version_variable("VELNOR_RELEASE_GH_VERSION"))
    } else {
        Err(crate::RenderError::BadCommand(
            "unqualified_github_cli".into(),
        ))
    }
}

#[must_use]
pub fn bootstrap() -> String {
    let pin = velnor_actions_mise::catalog::QUALIFIED_GH_LINUX_AMD64;
    let program = include_str!("verified_gh_acquire.py")
        .replace("@VERSION@", pin.version)
        .replace("@PLATFORM@", pin.platform)
        .replace("@SHA256@", pin.sha256)
        .replace("@ARCHIVE_SHA256@", pin.archive_sha256);
    let program = python_argument(&program);
    format!(
        "case \"$RUNNER_TEMP\" in /*) ;; *) exit 1 ;; esac; velnor_gh_path=\"$RUNNER_TEMP/velnor-gh-bootstrap-path\"; rm -f \"$velnor_gh_path\"; set -C; GH_TOKEN= GITHUB_TOKEN= MISE_GITHUB_TOKEN= ACTIONS_RUNTIME_TOKEN= ACTIONS_ID_TOKEN_REQUEST_TOKEN= ACTIONS_ID_TOKEN_REQUEST_URL= LD_PRELOAD= LD_LIBRARY_PATH= LD_AUDIT= LD_DEBUG= LD_PROFILE= GCONV_PATH= LOCPATH= PYTHONPATH= PYTHONHOME= PYTHONUSERBASE= /usr/bin/python3 -I -S -c {program} > \"$velnor_gh_path\" || exit 1; set +C; IFS= read -r VELNOR_VERIFIED_GH_DATA_DIR < \"$velnor_gh_path\" || exit 1; rm -f \"$velnor_gh_path\"; export VELNOR_VERIFIED_GH_DATA_DIR"
    )
}
