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
        "case \"$RUNNER_TEMP\" in /*) ;; *) exit 1 ;; esac; velnor_gh_path=\"$RUNNER_TEMP/velnor-gh-bootstrap-path\"; rm -f \"$velnor_gh_path\"; set -C; LD_PRELOAD= LD_LIBRARY_PATH= LD_AUDIT= LD_DEBUG= LD_PROFILE= GCONV_PATH= LOCPATH= /usr/bin/env -i RUNNER_TEMP=\"$RUNNER_TEMP\" PATH=/usr/bin:/bin /usr/bin/python3 -I -S -c {program} > \"$velnor_gh_path\" || exit 1; set +C; IFS= read -r VELNOR_VERIFIED_GH_DATA_DIR < \"$velnor_gh_path\" || exit 1; rm -f \"$velnor_gh_path\"; export VELNOR_VERIFIED_GH_DATA_DIR"
    )
}

#[must_use]
pub fn git_credential_config() -> String {
    let helper = format!("!{} auth git-credential", command()).replace('\'', "'\\''");
    format!(
        "export GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_COUNT=4\nexport GIT_CONFIG_KEY_0=credential.helper GIT_CONFIG_VALUE_0=''\nexport GIT_CONFIG_KEY_1=credential.https://github.com.helper GIT_CONFIG_VALUE_1='{helper}'\nexport GIT_CONFIG_KEY_2=core.hooksPath GIT_CONFIG_VALUE_2=/dev/null\nexport GIT_CONFIG_KEY_3=core.fsmonitor GIT_CONFIG_VALUE_3=false"
    )
}
