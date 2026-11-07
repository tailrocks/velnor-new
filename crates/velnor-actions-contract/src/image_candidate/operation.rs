// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::task::{
    JOB_IMAGE_CANDIDATE_DRIVER, JOB_IMAGE_CANDIDATE_LOADER, JOB_IMAGE_CANDIDATE_MAX_SCRIPT_BYTES,
    JOB_IMAGE_CANDIDATE_TASK_ID, JOB_IMAGE_CANDIDATE_TASK_KIND, JOB_IMAGE_CANDIDATE_TASK_SCHEMA,
    JOB_IMAGE_CANDIDATE_UNIT_ID,
};
use crate::workflow::cache_family::{CARGO_HOME_EXPR, RUSTUP_HOME_EXPR};
use crate::{ContractError, TaskIdentity};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobImageCandidateInputs {
    pub rust: String,
    pub mr_boxington: String,
    pub export_rustup: bool,
}

impl JobImageCandidateInputs {
    pub fn validate(&self) -> Result<(), ContractError> {
        for (selector, prefix, project_ref) in [
            (&self.rust, "rust@", "${VELNOR_TOOL_RUST}"),
            (
                &self.mr_boxington,
                "mr-boxington@",
                "${VELNOR_TOOL_MR_BOXINGTON}",
            ),
        ] {
            let version = selector
                .strip_prefix(prefix)
                .ok_or_else(|| invalid("toolchain"))?;
            if version != project_ref
                && !crate::config::project_tool_refs::is_exact_project_version(version)
            {
                return Err(invalid("toolchain"));
            }
        }
        Ok(())
    }

    pub fn environment(&self) -> Result<std::collections::BTreeMap<String, String>, ContractError> {
        self.validate()?;
        let mut env = [
            ("MISE_NO_CONFIG", "1"),
            ("MISE_NO_ENV", "1"),
            ("MISE_NO_HOOKS", "1"),
            ("MISE_LOCKFILE", "0"),
            ("MISE_AUTO_INSTALL", "false"),
            ("MISE_EXEC_AUTO_INSTALL", "false"),
            ("MISE_RUSTUP_HOME", RUSTUP_HOME_EXPR),
            ("MISE_CARGO_HOME", CARGO_HOME_EXPR),
            ("RUSTUP_HOME", RUSTUP_HOME_EXPR),
            ("CARGO_HOME", CARGO_HOME_EXPR),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect::<std::collections::BTreeMap<_, _>>();
        if self.export_rustup {
            env.insert("RUSTUP_TOOLCHAIN".to_owned(), self.rust[5..].to_owned());
        }
        Ok(env)
    }

    pub fn argv(&self) -> Result<Vec<String>, ContractError> {
        self.validate()?;
        let source = job_image_candidate_source_script(
            "mise --no-config --no-env --no-hooks exec \"$1\" \"$2\" -- mbx build --release --locked --package velnor-actions-cli --bin velnor-actions",
        );
        let script = format!(
            "{source}\nexport VELNOR_NATIVE_IMAGE_ACTIONS=\"$image_binary\"\n\"$image_binary\" image-maintenance check-job-source --root \"$GITHUB_WORKSPACE\" --source-commit \"$GITHUB_SHA\" --source-closure-path \"$RUNNER_TEMP/job-image-native-source-closure.json\"\n\"$image_binary\" image-maintenance install-buildx --root \"$GITHUB_WORKSPACE\" --runner-temp \"$RUNNER_TEMP\"\nexec \"$image_binary\" image-maintenance build-job-candidate --root \"$GITHUB_WORKSPACE\" --source-commit \"$GITHUB_SHA\" --runner-temp \"$RUNNER_TEMP\" --archive-path \"$RUNNER_TEMP/job-image.oci.tar\" --record-path \"$RUNNER_TEMP/job-image-candidate.json\" --source-closure-path \"$RUNNER_TEMP/job-image-native-source-closure.json\""
        );
        if script.len() > JOB_IMAGE_CANDIDATE_MAX_SCRIPT_BYTES {
            return Err(invalid("script_size"));
        }
        let alphabet = b"0123456789abcdef";
        let encoded: String = script
            .bytes()
            .flat_map(|byte| {
                [
                    char::from(alphabet[usize::from(byte >> 4)]),
                    char::from(alphabet[usize::from(byte & 15)]),
                ]
            })
            .collect();
        Ok([
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "exec",
            &self.rust,
            "--",
            "/usr/bin/python3",
            "-I",
            "-S",
            "-c",
            JOB_IMAGE_CANDIDATE_LOADER,
            &encoded,
            &self.rust,
            &self.mr_boxington,
            if self.export_rustup {
                "export"
            } else {
                "no-export"
            },
        ]
        .into_iter()
        .map(str::to_owned)
        .collect())
    }
}

fn invalid(detail: &str) -> ContractError {
    ContractError::identity("step.job_image_candidate", detail)
}

pub(crate) fn validate_job_image_candidate_operation(
    argv: &[String],
    env: &std::collections::BTreeMap<String, String>,
    inputs: &JobImageCandidateInputs,
) -> Result<(), ContractError> {
    if argv != inputs.argv()? || *env != inputs.environment()? {
        return Err(invalid("operation"));
    }
    Ok(())
}

pub(crate) fn is_job_image_candidate_identity(task: &TaskIdentity) -> bool {
    task.stack_id == "native"
        && task.task_id == JOB_IMAGE_CANDIDATE_TASK_ID
        && task.task_kind == JOB_IMAGE_CANDIDATE_TASK_KIND
        && task.component_id == JOB_IMAGE_CANDIDATE_UNIT_ID
        && task.project_root == "."
        && task.working_dir == "."
        && task.configuration.target == "x86_64-unknown-linux-gnu"
        && task.configuration.profile == "oci-linux-amd64"
        && task.configuration.compile_driver == JOB_IMAGE_CANDIDATE_DRIVER
        && task.configuration.test_runner == JOB_IMAGE_CANDIDATE_DRIVER
        && task.configuration.task_contract == JOB_IMAGE_CANDIDATE_TASK_SCHEMA
        && task.configuration.features.is_empty()
        && task.configuration.flags.is_empty()
        && task.output_contract == JOB_IMAGE_CANDIDATE_TASK_SCHEMA
        && task.stack_extension.schema == JOB_IMAGE_CANDIDATE_TASK_SCHEMA
        && serde_json::from_value::<JobImageCandidateInputs>(task.stack_extension.data.clone())
            .is_ok_and(|inputs| {
                validate_job_image_candidate_operation(&task.argv, &task.environment, &inputs)
                    .is_ok()
            })
}

#[must_use]
pub fn job_image_candidate_source_script(build: &str) -> String {
    let script = format!(
        "set -euo pipefail\ntrusted_git() {{ /usr/bin/env -i PATH=/usr/bin:/bin GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null GIT_OPTIONAL_LOCKS=0 /usr/bin/git --git-dir=\"$GITHUB_WORKSPACE/.git\" --work-tree=\"$GITHUB_WORKSPACE\" -c core.fsmonitor=false \"$@\"; }}\nsource_sha=\"$(trusted_git rev-parse --verify HEAD)\"\ntest \"$source_sha\" = \"$GITHUB_SHA\"\ntest \"$GITHUB_WORKFLOW_SHA\" = \"$GITHUB_SHA\"\nif ! trusted_git diff --cached --quiet --no-ext-diff --no-textconv --ignore-submodules=none || ! trusted_git diff --quiet --no-ext-diff --no-textconv --ignore-submodules=none; then\n  printf '%s\\n' 'The source checkout is not clean.' >&2\n  exit 1\nfi\nstatus_file=\"$(mktemp \"$RUNNER_TEMP/velnor-image-source-status.XXXXXXXX\")\"\ntrusted_git status --porcelain=v1 --untracked-files=all --ignore-submodules=none -z > \"$status_file\"\nif [[ -s \"$status_file\" ]]; then\n  printf '%s\\n' 'The source checkout is not clean.' >&2\n  exit 1\nfi\nmetadata_file=\"$(mktemp \"$RUNNER_TEMP/velnor-native-image-metadata.XXXXXXXX\")\"\nartifacts_file=\"$(mktemp \"$RUNNER_TEMP/velnor-native-image-artifacts.XXXXXXXX\")\"\ntrap 'rm -f \"$metadata_file\" \"$artifacts_file\"' EXIT\ncargo metadata --locked --format-version=1 --filter-platform x86_64-unknown-linux-gnu > \"$metadata_file\"\nenv -u ACTIONS_ID_TOKEN_REQUEST_TOKEN -u ACTIONS_ID_TOKEN_REQUEST_URL -u ACTIONS_RUNTIME_TOKEN -u GITHUB_TOKEN -u MISE_GITHUB_TOKEN -u GH_TOKEN -u GH_HOST -u GH_CONFIG_DIR {build} --message-format=json > \"$artifacts_file\"\npython3 scripts/native-image-candidate-inventory.py --check --root \"$GITHUB_WORKSPACE\" --metadata \"$metadata_file\" --artifacts \"$artifacts_file\" --target x86_64-unknown-linux-gnu\ntest -f target/release/velnor-actions && test ! -L target/release/velnor-actions\numask 077\nimage_binary_dir=\"$(mktemp -d \"$RUNNER_TEMP/velnor-native-image-actions.XXXXXXXX\")\"\nimage_binary=\"$image_binary_dir/velnor-native-image-actions\"\n( set -o noclobber; cat target/release/velnor-actions > \"$image_binary\" )\nchmod 0500 \"$image_binary\"\nprintf 'VELNOR_NATIVE_IMAGE_ACTIONS=%s\\n' \"$image_binary\" >> \"$GITHUB_ENV\""
    );
    script
}
