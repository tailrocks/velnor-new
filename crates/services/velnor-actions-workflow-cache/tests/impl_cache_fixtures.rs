//! Shared fixtures for the cache-template test family.
use std::collections::BTreeMap;
use velnor_actions_contract_workflow::workflow::permissions::PermissionLevel;
use velnor_actions_contract_workflow::{
    Job, JobTimeout, Permissions, REQUIRED_JOB_ID as CONTRACT_REQUIRED_JOB_ID, Step,
};
use velnor_actions_workflow_cache::cache_steps::{CompileDriver, mbx_steps_for_driver};
use velnor_actions_workflow_steps::{MiseSetup, RenderError};

pub(crate) const LABEL: &str = "ubuntu-26.04";
pub(crate) const MISE_USES: &str = "jdx/mise-action@9149ea85001c7435d5a66bb127d6a1b6227cb0a5";
pub(crate) const MISE_VERSION: &str = "2026.9.18";
pub(crate) const MISE_SHA256: &str =
    "d24fe0bf7e613824ad99f7b8dac3f2b381a37b9f75f84dd250855217095a8de4";

pub(crate) const TEST_MBX_VERSION: &str = "1.21.1";
pub(crate) const TEST_RUST_TOOLCHAIN: &str = "1.98.1";

pub(crate) fn mbx_tool_env(rust_toolchain: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("MISE_NO_CONFIG".to_owned(), "1".to_owned()),
        ("MISE_NO_ENV".to_owned(), "1".to_owned()),
        ("MISE_NO_HOOKS".to_owned(), "1".to_owned()),
        ("MISE_LOCKFILE".to_owned(), "0".to_owned()),
        ("MISE_AUTO_INSTALL".to_owned(), "false".to_owned()),
        ("MISE_EXEC_AUTO_INSTALL".to_owned(), "false".to_owned()),
        (
            "MISE_RUSTUP_HOME".to_owned(),
            "${{ runner.temp }}/velnor/rustup".to_owned(),
        ),
        (
            "MISE_CARGO_HOME".to_owned(),
            "${{ runner.temp }}/velnor/cargo".to_owned(),
        ),
        ("RUSTUP_TOOLCHAIN".to_owned(), rust_toolchain.to_owned()),
    ])
}

pub(crate) fn mbx_tool_steps(
    uses: &str,
    mbx_version: &str,
    rust_toolchain: &str,
) -> Result<[Step; 3], RenderError> {
    mbx_steps_for_driver(
        uses,
        CompileDriver::Mbx,
        mbx_version,
        rust_toolchain,
        mbx_tool_env(rust_toolchain),
    )?
    .ok_or_else(|| RenderError::InvalidWorkflow("mbx_steps_missing".to_owned()))
}

pub(crate) fn checkout_pin() -> String {
    format!("actions/checkout@{:040x}", 0)
}

pub(crate) fn mise() -> MiseSetup {
    MiseSetup {
        uses: MISE_USES.to_owned(),
        version: MISE_VERSION.to_owned(),
        sha256: MISE_SHA256.to_owned(),
    }
}

pub(crate) fn job(id: &str, display: &str, needs: Vec<String>, steps: Vec<Step>) -> (String, Job) {
    (
        id.to_owned(),
        Job {
            display_name: display.to_owned(),
            runs_on: LABEL.to_owned(),
            check_runner: None,
            timeout_minutes: JobTimeout::CRATE,
            needs,
            condition: None,
            permissions: (id == CONTRACT_REQUIRED_JOB_ID).then_some(Permissions {
                contents: PermissionLevel::Read,
                actions: PermissionLevel::Read,
                pull_requests: PermissionLevel::None,
                id_token: PermissionLevel::None,
            }),
            environment: None,
            steps,
        },
    )
}
